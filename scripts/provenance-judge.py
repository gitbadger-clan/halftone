#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Run the LLM-judge test of the provenance views end to end, one command per batch.

    uv run scripts/provenance-judge.py run e2__2
    uv run scripts/provenance-judge.py run e2__2 --seeds 2 --input editor --judge "claude sonnet (picker)"
    uv run scripts/provenance-judge.py run bowl --questions blind.txt   # a blind question set
    uv run scripts/provenance-judge.py summary [--only e2__2] [--since 20261017]
    uv run scripts/provenance-judge.py rescore target/provenance-judge/<run>

Cases with answer keys: e2__2 and e1__1 (Flow edits), canva-ai-magic-edit (one Canva
record, no camera), photo_bowl (the Fold original). The summary keeps every file and
question set in its own table, and prints which judges ran how often.

`run` does, for each seed (default 1 2 3, the three rotations that put every view in
every position once):

  1. builds the anonymised prompt (scripts/provenance-view-check.py) and saves it as
     prompt.md in a new run folder;
  2. waits for the judge's reply in that folder's reply.txt, which you write
     yourself: by default it waits for the file to appear (e.g. you run
     `pbpaste > <folder>/reply.txt` after copying the reply); with --input editor it
     opens reply.txt in $VISUAL/$EDITOR for you to paste into;
  3. repairs what chat apps do to JSON on the way (code fences, text around it,
     markdown escaping, typographic quotes, trailing commas); when the reply is not
     usable it says why, keeps it as reply.rejected-N.txt and waits for a new one;
  4. scores it: maps labels back to tools, checks each answer against the case's
     answer key with simple rules (marked "auto": read the answers when they
     disagree with your impression), collects the ratings and the choice;
  5. saves key, parsed reply and score next to the prompt.

The judge is recorded from the reply: the prompt asks the model to name itself in
"judge_model", and the run is marked "self-reported". Models are often vague or
wrong about their own version, so when it matters pass --judge with the name from
the chat app's model picker; that overrides the self-report.

What it never does: read the clipboard, watch anything but the one reply.txt it
named, echo the text of a reply it could not use, or touch the network. It writes
to the clipboard only with --copy-prompt, and then only the prompt.

`summary` aggregates every saved run per tool: runs, answers correct, mean ratings,
times chosen, and the weaknesses named for Halftone's view. `rescore` re-parses and
re-scores a saved run, for when the rules or the parser change.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

OUT = Path("target/provenance-judge")


def _check_module():
    path = Path(__file__).with_name("provenance-view-check.py")
    spec = importlib.util.spec_from_file_location("provenance_view_check", path)
    if spec is None or spec.loader is None:
        sys.exit(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


PVC = _check_module()

# ---- automatic answer checks ---------------------------------------------------------
# Rules per case, keyed by a substring of the case's file name. Each rule says what a
# correct answer must contain; answers that say "not stated" are always wrong. These
# are deliberately simple: they tell views apart, they do not grade prose.
NOT_STATED = re.compile(r"^\s*(not stated|n/?a|none)\b", re.I)


def _all(*patterns: str):
    rx = [re.compile(p, re.I) for p in patterns]
    return lambda a: all(r.search(a) for r in rx)


def _in_order(*patterns: str):
    """The patterns occur in this order (first match of each after the previous)."""
    rx = [re.compile(p, re.I) for p in patterns]

    def check(a: str) -> bool:
        pos = 0
        for r in rx:
            m = r.search(a, pos)
            if not m:
                return False
            pos = m.end()
        return True

    return check


def _no(a: str) -> bool:
    """A correct "nothing recorded" answer, in any wording, that does not open with yes."""
    return bool(NO.search(a)) and not re.match(r"\s*yes\b", a, re.I)


NO = re.compile(
    r"\bno\b|\bnone\b|\bnot (recorded|stated|declared|mentioned|indicated|present|shown|used)"
    r"|does not (say|state|record|show|declare)|\bwithout\b",
    re.I,
)
SIGNER_STATEMENT = _all(
    r"signer|statement|stated|assert|claim|declar|not (been )?(independently )?"
    r"(checked|verified)|self-reported"
)

# Each rule: (check, a "not stated" answer counts as correct). The keys say why.
FLOW_E2_RULES = {
    "Q1": (_all(r"pixel|camera"), False),
    "Q2": (_all(r"google"), False),
    "Q3": (_all(r"generative|\bAI\b"), False),
    "Q4": (_all(r"synthid"), False),
    "Q5": (_all(r"transcod|re-?encod|re-?saved|re-?compress"), False),
    "Q6": (_in_order(r"pixel|camera|capture", r"generative|\bAI\b", r"transcod|re-?encod"), False),
    "Q7": (SIGNER_STATEMENT, False),
}
FLOW_E1_RULES = {
    **FLOW_E2_RULES,
    "Q5": (_all(r"generative|synthid|watermark"), False),
    "Q6": (_in_order(r"pixel|camera|capture", r"generative|\bAI\b"), False),
}
CANVA_RULES = {
    "Q1": (_no, True),
    "Q2": (_all(r"canva"), False),
    "Q3": (lambda a: _all(r"composite|generative|\bAI\b")(a) and not re.match(r"\s*no\b", a, re.I), False),
    "Q4": (_no, True),
    "Q5": (_all(r"creat|composite|canva"), False),
    "Q6": (_all(r"\bone\b|single|only|canva|creat"), False),
    "Q7": (_all(r"expir|invalid|does not validate|not valid|fail|untrusted|not (on|in) (the|any) trust"), False),
}
BOWL_RULES = {
    "Q1": (_all(r"pixel|camera"), False),
    "Q2": (_all(r"google"), False),
    "Q3": (_no, False),
    "Q4": (_no, True),
    "Q5": (_all(r"pixel|camera|captur|creat"), False),
    "Q6": (_all(r"pixel|camera|captur|\bone\b|single"), False),
    "Q7": (SIGNER_STATEMENT, False),
}

RULES = {
    "nano-banana-2.1__edit-photo__e2__2": FLOW_E2_RULES,
    "nano-banana-2.1__edit-photo__e1__1": FLOW_E1_RULES,
    "canva-ai-magic-edit__edit-photo__e1__1": CANVA_RULES,
    "00-base/photo_bowl": BOWL_RULES,
}


def rules_for(case_file: str) -> dict | None:
    return next((r for k, r in RULES.items() if k in case_file), None)


def auto_check(answers: dict, rules: dict | None, n: int) -> dict[str, bool | None]:
    """Per question: True/False by the case's rule, None where there is no rule (a
    custom question set, or a case without a key)."""
    out: dict[str, bool | None] = {}
    for i in range(1, n + 1):
        q = f"Q{i}"
        text = str((answers.get(q) or {}).get("answer", "")).strip()
        if rules is None or q not in rules:
            out[q] = None
            continue
        check, not_stated_ok = rules[q]
        if not text:
            out[q] = False
        elif NOT_STATED.match(text):
            out[q] = not_stated_ok
        else:
            out[q] = bool(check(text))
    return out


# ---- reply shape -----------------------------------------------------------------------
def shape_warnings(obj: dict, labels: list[str], n: int) -> list[str]:
    """Missing parts of an otherwise parsed reply; scored anyway, reported."""
    w = []
    for lab in labels:
        ans = (obj.get("answers") or {}).get(lab)
        if not isinstance(ans, dict):
            w.append(f"no answers for view {lab}")
        else:
            w += [f"{lab}.Q{i} missing" for i in range(1, n + 1) if f"Q{i}" not in ans]
        rat = (obj.get("ratings") or {}).get(lab)
        if not isinstance(rat, dict):
            w.append(f"no ratings for view {lab}")
            continue
        for c in PVC.CRITERIA:
            sc = (rat.get(c) or {}).get("score") if isinstance(rat.get(c), dict) else None
            if not isinstance(sc, int) or not 1 <= sc <= 5:
                w.append(f"{lab}.{c} score {sc!r} is not 1-5")
    if (obj.get("choice") or {}).get("view") not in labels:
        w.append(f"choice.view {(obj.get('choice') or {}).get('view')!r} is not one of {labels}")
    return w


# ---- scoring and display -------------------------------------------------------------
SHORT = {"findability": "find", "plain_language": "plain", "misleading_risk": "safe", "noise": "clean"}
LEGEND = (
    "ratings 1-5, higher is better on every column: find = answers easy to find, "
    "plain = no C2PA knowledge needed, safe = hard to misread, clean = little to skip past"
)


def _cols() -> str:
    return " ".join(f"{SHORT.get(c, c[:8]):>8}" for c in PVC.CRITERIA)


def score(obj: dict, label_map: dict[str, str], case_file: str, n: int, custom: bool) -> dict:
    rules = None if custom else rules_for(case_file)
    rows = {}
    for lab, tool in label_map.items():
        rat = (obj.get("ratings") or {}).get(lab) or {}
        rows[tool] = {
            "label": lab,
            "auto": auto_check((obj.get("answers") or {}).get(lab) or {}, rules, n),
            "ratings": {
                c: (rat.get(c) or {}).get("score") if isinstance(rat.get(c), dict) else None
                for c in PVC.CRITERIA
            },
        }
    choice = obj.get("choice") or {}
    return {
        "rows": rows,
        "choice": label_map.get(choice.get("view", ""), choice.get("view")),
        "weakness": choice.get("weakness", ""),
    }


def show(sc: dict) -> None:
    print(f"  {'view':<18} {'label':<5} {'correct':>8}   {_cols()}")
    for tool, r in sc["rows"].items():
        checked = [v for v in r["auto"].values() if v is not None]
        correct = f"{sum(checked)}/{len(checked)}" if checked else "n/a"
        rats = " ".join(
            f"{str(r['ratings'][c] if r['ratings'][c] is not None else '-'):>8}" for c in PVC.CRITERIA
        )
        print(f"  {tool:<18} {r['label']:<5} {correct:>8}   {rats}")
    misses = {
        tool: [q for q, v in r["auto"].items() if v is False]
        for tool, r in sc["rows"].items()
    }
    for tool, qs in misses.items():
        if qs and tool.startswith("ht"):
            print(f"  ! Halftone's view missed {', '.join(qs)} (auto): read those answers in judge.json")
    print(f"  ({LEGEND})")
    print(f"  chosen: {sc['choice']}")
    print(f"  weakness: {sc['weakness']}")


# ---- getting the reply --------------------------------------------------------------
def copy_prompt(prompt: str) -> bool:
    """--copy-prompt: put the prompt (and nothing else) on the clipboard. Write only;
    the clipboard is never read."""
    if shutil.which("pbcopy") is None:
        return False
    subprocess.run(["pbcopy"], input=prompt, text=True, check=True)
    return True


def _set_aside(path: Path) -> Path:
    n = 1
    while (dest := path.with_name(f"reply.rejected-{n}.txt")).exists():
        n += 1
    path.rename(dest)
    return dest


def _try(path: Path) -> tuple[str, dict, list[str]] | None:
    """Parse the reply file; on failure explain, set the file aside, return None."""
    raw = path.read_text(errors="replace")
    obj, notes, error = PVC.parse_reply(raw)
    if obj is not None:
        return raw, obj, notes
    kept = _set_aside(path)
    print("  that reply is not usable:")
    print("    " + (error or "unknown problem").replace("\n", "\n    "))
    print(f"  kept as {kept.name}; write the corrected or new reply to {path.name} again.")
    return None


def wait_for_file(path: Path, timeout: float) -> tuple[str, dict, list[str]]:
    """Wait for `path`, and only `path`, to be written. A file is read once its size
    has stopped changing for a second, so a slow write is not read half-done."""
    print(f"  then write the reply to:\n    {path}")
    print(f"  for example, after copying it:  pbpaste > {path}")
    print("  waiting for that file (Ctrl-C to stop)…")
    deadline = time.monotonic() + timeout
    last_size = -1
    while time.monotonic() < deadline:
        time.sleep(1)
        if not path.exists():
            last_size = -1
            continue
        size = path.stat().st_size
        if size == 0 or size != last_size:
            last_size = size
            continue
        got = _try(path)
        if got is not None:
            return got
        last_size = -1
    sys.exit(f"no usable reply in {path} within {timeout:.0f} s")


def edit_reply(path: Path) -> tuple[str, dict, list[str]]:
    """Open `path` in the user's editor until it holds a usable reply."""
    editor = os.environ.get("VISUAL") or os.environ.get("EDITOR") or "nano"
    while True:
        if not path.exists():
            path.write_text("paste the judge's whole reply below this line, save, and quit\n\n")
        print(f"  opening {path.name} in {editor}: paste the reply, save, quit")
        subprocess.run([*editor.split(), str(path)], check=False)
        got = _try(path)
        if got is not None:
            return got
        if input("  open the editor again? [Y/n] ").strip().lower().startswith("n"):
            sys.exit(1)


def read_piped(path: Path) -> tuple[str, dict, list[str]]:
    """--input stdin, for a reply piped from a file: `... < reply.txt`."""
    if sys.stdin.isatty():
        sys.exit("--input stdin reads a piped file; for pasting use the default or --input editor")
    path.write_text(sys.stdin.read())
    got = _try(path)
    if got is None:
        sys.exit(1)
    return got


# ---- commands ------------------------------------------------------------------------
def save_run(folder: Path, prompt: str, label_map: dict, case: dict, raw: str, obj: dict,
             notes: list[str], warnings: list[str], sc: dict, meta: dict) -> None:
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "prompt.md").write_text(prompt)
    (folder / "key.json").write_text(json.dumps(
        {"labels": label_map, "file": case["file"], "answer_key": case.get("key"),
         "question_set": meta["question_set"], "questions": meta["questions"]}, indent=2))
    (folder / "reply.txt").write_text(raw)
    (folder / "judge.json").write_text(json.dumps(obj, indent=2, ensure_ascii=False))
    (folder / "score.json").write_text(json.dumps(
        {**meta, "notes": notes, "warnings": warnings, **sc}, indent=2, ensure_ascii=False))


def judge_of(obj: dict, flag: str | None) -> tuple[str, str]:
    """The judge's name and where it came from: the --judge flag, the model's own
    report in the reply, or neither. The self-report is model text, so it is
    reduced to a short plain label before it reaches a file name or the summary."""
    if flag:
        return flag, "flag"
    told = re.sub(r"[^A-Za-z0-9 ._:+/-]", "", str(obj.get("judge_model", ""))).strip()[:60]
    if told and told.lower() not in {"unknown", "n/a", "none"}:
        return told, "self-reported"
    return "unknown", "unknown"


def slugify(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "-", text).strip("-").lower() or "x"


def cmd_run(a: argparse.Namespace) -> int:
    if a.input == "stdin" and len(a.seeds) > 1:
        sys.exit("--input stdin takes one piped reply: give a single --seeds value")
    custom = PVC.load_questions(a.questions) if a.questions else None
    qset = f"custom:{Path(a.questions).name}" if a.questions else "built-in"
    if custom:
        print(f"question set {qset}: answers are not auto-scored (no key); ratings and choice are")
    for seed in a.seeds:
        prompt, label_map, case, questions = PVC.make_prompt(a.case, seed, custom)
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        folder = OUT / f"{stamp}-{slugify(a.case)}-s{seed}"
        folder.mkdir(parents=True, exist_ok=True)
        (folder / "prompt.md").write_text(prompt)
        reply = folder / "reply.txt"
        print(f"\n== seed {seed}: views {', '.join(f'{l}={t}' for l, t in label_map.items())}")
        if a.copy_prompt and copy_prompt(prompt):
            print("  prompt copied to the clipboard.")
        else:
            print(f"  prompt saved; copy it with:  pbcopy < {folder / 'prompt.md'}")
        print("  paste it into a FRESH chat with the judge model, outside any project, send it as is,")
        if a.input == "editor":
            raw, obj, notes = edit_reply(reply)
        elif a.input == "stdin":
            raw, obj, notes = read_piped(reply)
        else:
            raw, obj, notes = wait_for_file(reply, a.timeout)
        for n in notes:
            print(f"  note: {n}")
        judge, source = judge_of(obj, a.judge)
        print(f"  judge: {judge} ({source})")
        # The folder is named once the judge is known.
        named = folder.with_name(f"{folder.name}-{slugify(judge)}")
        n = 2
        while named.exists():  # two runs in the same second with the same judge
            named = folder.with_name(f"{folder.name}-{slugify(judge)}-{n}")
            n += 1
        folder.rename(named)
        folder = named
        warnings = shape_warnings(obj, list(label_map), len(questions))
        for w in warnings:
            print(f"  warning: {w}")
        sc = score(obj, label_map, case["file"], len(questions), custom is not None)
        save_run(folder, prompt, label_map, case, raw, obj, notes, warnings, sc,
                 {"judge": judge, "judge_source": source, "seed": seed,
                  "file": case["file"], "at": stamp, "question_set": qset,
                  "questions": questions})
        show(sc)
        print(f"  saved: {folder}")
    print("\nall runs so far for this file:")
    a.only = a.case
    a.since = None
    return cmd_summary(a)


def cmd_rescore(a: argparse.Namespace) -> int:
    folder = Path(a.folder)
    key = json.loads((folder / "key.json").read_text())
    meta = json.loads((folder / "score.json").read_text()) if (folder / "score.json").exists() else {}
    raw = (folder / "reply.txt").read_text()
    obj, notes, error = PVC.parse_reply(raw)
    if obj is None:
        print(error, file=sys.stderr)
        return 1
    questions = key.get("questions") or PVC.QUESTIONS
    custom = key.get("question_set", "built-in") != "built-in"
    warnings = shape_warnings(obj, list(key["labels"]), len(questions))
    sc = score(obj, key["labels"], key["file"], len(questions), custom)
    (folder / "judge.json").write_text(json.dumps(obj, indent=2, ensure_ascii=False))
    keep = {k: meta[k] for k in ("judge", "judge_source", "seed", "file", "at", "question_set",
                                 "questions") if k in meta}
    (folder / "score.json").write_text(json.dumps(
        {**keep, "notes": notes, "warnings": warnings, **sc}, indent=2, ensure_ascii=False))
    show(sc)
    return 0


def cmd_summary(a: argparse.Namespace) -> int:
    """All saved runs, one table per file and question set, so different chains and
    blind question sets are never averaged together."""
    runs = [json.loads(p.read_text()) for p in sorted(OUT.glob("*/score.json"))]
    only = getattr(a, "only", None)
    since = getattr(a, "since", None)
    runs = [r for r in runs
            if (not only or only in r.get("file", ""))
            and (not since or str(r.get("at", "")) >= since)]
    if not runs:
        print("no saved runs in", OUT, "matching the filters")
        return 0
    groups: dict[tuple[str, str], list[dict]] = {}
    for r in runs:
        groups.setdefault((Path(r.get("file", "?")).name, r.get("question_set", "built-in")), []).append(r)
    for (name, qset), rs in groups.items():
        tools: dict[str, dict] = {}
        for r in rs:
            for tool, row in r["rows"].items():
                t = tools.setdefault(tool, {"runs": 0, "ok": 0, "checked": 0, "chosen": 0,
                                            "ratings": {c: [] for c in PVC.CRITERIA}})
                t["runs"] += 1
                checked = [v for v in row["auto"].values() if v is not None]
                t["ok"] += sum(checked)
                t["checked"] += len(checked)
                t["chosen"] += r.get("choice") == tool
                for c in PVC.CRITERIA:
                    if isinstance(row["ratings"].get(c), int):
                        t["ratings"][c].append(row["ratings"][c])
        judges: dict[str, int] = {}
        for r in rs:
            j = r.get("judge", "?") + (" (self-reported)" if r.get("judge_source") == "self-reported" else "")
            judges[j] = judges.get(j, 0) + 1
        print(f"\n  {name} · questions: {qset} · {len(rs)} run(s)")
        print("  judges: " + ", ".join(f"{j} ×{n}" for j, n in sorted(judges.items())))
        print(f"  {'view':<18} {'runs':>4} {'correct':>9} {'chosen':>7}   {_cols()}")
        for tool, t in tools.items():
            correct = f"{t['ok']}/{t['checked']}" if t["checked"] else "n/a"
            means = " ".join(
                f"{(sum(v) / len(v)):>8.1f}" if v else f"{'-':>8}" for v in t["ratings"].values()
            )
            print(f"  {tool:<18} {t['runs']:>4} {correct:>9} {t['chosen']:>7}   {means}")
        print(f"  ({LEGEND}; correct = automatic check against the answer key)")
        weak = [(r.get("at", "?"), r.get("judge", "?"), r.get("weakness", ""))
                for r in rs if str(r.get("choice", "")).startswith("ht")]
        if weak:
            print("  weaknesses named for Halftone's view:")
            for at, judge, w in weak:
                print(f"  - {at} {judge}: {w}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="prompt → your reply file → score → saved run")
    r.add_argument("case", help="substring of the case's file name, e.g. e2__2")
    r.add_argument(
        "--judge",
        help="judge model name for the record; default: the model's own report in the reply",
    )
    r.add_argument("--seeds", type=int, nargs="+", default=[1, 2, 3])
    r.add_argument(
        "--questions",
        metavar="FILE",
        help="a blind question set (see provenance-view-check.py --write-questions) instead "
        "of the built-in one; answers are then not auto-scored",
    )
    r.add_argument("--timeout", type=float, default=900, help="seconds to wait for reply.txt")
    r.add_argument(
        "--input",
        choices=["file", "editor", "stdin"],
        default="file",
        help="file: wait for reply.txt (default); editor: open it in $VISUAL/$EDITOR; "
        "stdin: read one piped reply",
    )
    r.add_argument(
        "--copy-prompt",
        action="store_true",
        help="also put the prompt on the clipboard (write only; it is never read)",
    )
    r.set_defaults(func=cmd_run)
    s = sub.add_parser("summary", help="aggregate saved runs, one table per file and question set")
    s.add_argument("--only", metavar="MATCH", help="only runs whose file name contains MATCH")
    s.add_argument("--since", metavar="YYYYMMDD[-HHMMSS]", help="only runs from this time on")
    s.set_defaults(func=cmd_summary)
    rs = sub.add_parser("rescore", help="re-parse and re-score a saved run folder")
    rs.add_argument("folder")
    rs.set_defaults(func=cmd_rescore)
    a = ap.parse_args()
    return a.func(a)


if __name__ == "__main__":
    sys.exit(main())
