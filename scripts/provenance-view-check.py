#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Compare Halftone's provenance view with c2patool's tree on files whose history is
known, by the facts a reader needs, not by taste.

For each file in CASES, both views are produced:

    ht inspect --only manifest --provenance <file>
    c2patool --settings <collector settings> <file> --tree

c2patool gets the collector's settings (scripts/differential.py: vendored trust
list, TSA list and EKU policy, no network) and its clean environment, so it judges
trust exactly as the ground truth does and as Halftone does (DIFFERENTIAL.md D-008,
D-011). Without them every signer reads `untrusted` and the views disagree on trust
for no reason.

and checked for:
  - facts: a regex per fact a reader must be able to find (camera capture, signer,
    generative step, SynthID, which step is this file, input vs parent);
  - order: pairs of facts that must appear in history order (capture before
    generation), checked by first position in the text;
  - noise: URNs, UUIDs and base64 hashes a person has to read past, and line count.

Exit code 1 if Halftone's view misses a required fact, a fact c2patool's view has,
or an order; 0 otherwise. Run from the repo root:

    uv run scripts/provenance-view-check.py
    uv run scripts/provenance-view-check.py --show   # print both views per file
    uv run scripts/provenance-view-check.py --prompt e2__2 > /tmp/judge.md
        # one self-contained evaluation prompt for an LLM: the views of that file,
        # anonymised and shuffled; the label key and the answer key go to stderr
    uv run scripts/provenance-view-check.py --extract reply.txt > judge.json
        # the judge's JSON from a pasted reply (code fences, text around it)
    uv run scripts/provenance-view-check.py --schema > schema.json
        # JSON Schema of the reply, for an API's structured-output mode
    uv run scripts/provenance-view-check.py --write-questions > /tmp/write-q.md
        # a prompt that has a model write the editor's questions blind, before it
        # sees any view; save its reply as a file and pass it with --questions FILE

For the whole judging loop in one command (prompt saved, reply read from a file you
write, repaired, scored and saved), use scripts/provenance-judge.py, which builds on
this script.

The cases encode what the corpus established (04-generators SOURCE.md, D-013).
Add a case whenever a new chain shape enters the corpus.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import random
import re
import subprocess
import sys
import tempfile
from pathlib import Path

G = "corpus/differential/04-generators"

# Facts: (name, regex, required). "required" facts must be in Halftone's view even if
# c2patool's tree lacks them; the rest must be there whenever the tree has them.
CAMERA = ("camera capture", r"Pixel Camera|computationalCapture", True)
GOOGLE = ("signer Google", r"Google LLC", True)
GENERATIVE = ("generative step", r"trainedAlgorithmicMedia|generative AI", True)
SYNTHID = ("SynthID declared", r"SynthID", True)
THIS_FILE = ("which step is this file", r"this file|active[ _]manifest", True)
INPUT = ("photo was an input", r"inputTo", True)
TRANSCODE = ("re-encode on top", r"transcoded", True)
CANVA = ("signer Canva", r"Canva", True)
COMPOSITE = ("composite declared", r"compositeWithTrainedAlgorithmicMedia", True)

# What a careful reader should answer for each question, per file; used to score the
# judge's answers by hand. Never put into the prompt.
E2_KEY = {
    "Q1": "Yes: a Pixel camera capture (computationalCapture, 'Created by Pixel Camera.')",
    "Q2": "Google LLC signed every step",
    "Q3": "Yes: one step before this file, 'Created by Google Generative AI.' (trainedAlgorithmicMedia); the camera photo was its input",
    "Q4": "Yes: SynthID ('Applied imperceptible SynthID watermark.')",
    "Q5": "Opened and transcoded (re-encoded) by Google, algorithmicallyEnhanced; no generation in this last step",
    "Q6": "camera capture -> upload transcode -> generation + SynthID -> re-encode (this file)",
    "Q7": "They are signers' statements; the store validates (signature and hash), which does not prove the content",
}

# The other chains: the same story without a re-encode on top (E1); one record, no
# camera, a composite (Canva Magic Edit); one record, a plain camera capture (the
# Fold original). Keys say what the records state; a correct answer may be "no"
# or "none recorded" where the records hold nothing.
E1_KEY = {
    "Q1": "Yes: a Pixel camera capture (computationalCapture), used as input",
    "Q2": "Google LLC signed every step",
    "Q3": "Yes: this file's own step, 'Created by Google Generative AI.' (trainedAlgorithmicMedia), with the camera photo as input",
    "Q4": "Yes: SynthID ('Applied imperceptible SynthID watermark.')",
    "Q5": "Created by Google Generative AI and the SynthID watermark applied; nothing after that",
    "Q6": "camera capture -> upload transcode -> generation + SynthID (this file)",
    "Q7": "They are signers' statements; signatures and hash validate (trusted), which does not prove the history; revocation not checked",
}
CANVA_KEY = {
    "Q1": "No camera capture is recorded: Canva's record replaced the camera's (zero ingredients); 'not stated' or 'no' is correct",
    "Q2": "Canva",
    "Q3": "Yes: created as a composite including generative AI (compositeWithTrainedAlgorithmicMedia) by Canva AI",
    "Q4": "No watermark is declared; 'none' or 'not stated' is correct",
    "Q5": "Created by Canva AI (composite including generative AI); it is the only step",
    "Q6": "One step: created by Canva AI",
    "Q7": "Date-dependent: from 2026-10-15T23:52Z the record does not validate because the certificate expired (no time-stamp); signer not on the trust list; not evidence of tampering",
}
BOWL_KEY = {
    "Q1": "Yes: a Pixel camera capture (computationalCapture, 'Created by Pixel Camera.')",
    "Q2": "Google LLC",
    "Q3": "No generative AI step is recorded",
    "Q4": "No watermark is declared; 'none' or 'not stated' is correct",
    "Q5": "Captured by Pixel Camera; it is the only step",
    "Q6": "One step: the camera capture",
    "Q7": "They are signers' statements; signature and hash validate (trusted), which does not prove the content",
}

CASES: list[dict] = [
    {
        "file": f"{G}/google-flow__nano-banana-2.1__edit-photo__e2__2.jpg",
        "key": E2_KEY,
        "facts": [CAMERA, GOOGLE, GENERATIVE, SYNTHID, THIS_FILE, INPUT, TRANSCODE],
        "order": [("camera capture", "generative step")],
    },
    {
        "file": f"{G}/google-flow__nano-banana-2.1__edit-photo__e1__1.jpg",
        "key": E1_KEY,
        "facts": [CAMERA, GOOGLE, GENERATIVE, SYNTHID, THIS_FILE, INPUT],
        "order": [("camera capture", "generative step")],
    },
    {
        "file": f"{G}/google-flow__nano-banana-2.1__edit-photo-redownload__e2__2.jpg",
        "facts": [CAMERA, GOOGLE, GENERATIVE, SYNTHID, THIS_FILE, INPUT],
        "order": [("camera capture", "generative step")],
    },
    {
        "file": f"{G}/canva__canva-ai-magic-edit__edit-photo__e1__1.png",
        "key": CANVA_KEY,
        "facts": [CANVA, COMPOSITE],
        "order": [],
    },
    {
        "file": "corpus/differential/00-base/photo_bowl.jpg",
        "key": BOWL_KEY,
        "facts": [CAMERA, GOOGLE],
        "order": [],
    },
]

NOISE = re.compile(
    r"urn:c2pa:[0-9a-f-]+|urn:uuid:[0-9a-f-]+|\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-"
    r"[0-9a-f]{4}-[0-9a-f]{12}\b|[A-Za-z0-9+/]{40,}={0,2}",
    re.I,
)


TRUST = Path("crates/halftone-c2pa/trust")


def collector_c2patool() -> tuple[Path, dict[str, str]]:
    """Settings file and environment for c2patool, built by the collector's own
    functions so the two can never drift apart."""
    path = Path(__file__).with_name("differential.py")
    spec = importlib.util.spec_from_file_location("differential", path)
    if spec is None or spec.loader is None:
        sys.exit(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    version = subprocess.run(["c2patool", "--version"], capture_output=True, text=True).stdout
    if not mod.c2patool_supports_typed_anchors(version):
        sys.exit(f"c2patool {version.strip()} ignores typed trust anchors; need 0.28 or later")
    text = mod.c2patool_settings(
        TRUST / "C2PA-TRUST-LIST.pem",
        TRUST / "C2PA-TSA-TRUST-LIST.pem",
        TRUST / "C2PA-EKU-CONFIG.cfg",
    )
    tmp = Path(tempfile.mkdtemp(prefix="provenance-view-")) / "c2patool.toml"
    tmp.write_text(text)
    return tmp, mod.C2PATOOL_ENV


def view(cmd: list[str], env: dict[str, str] | None = None) -> tuple[str, str | None]:
    """Text of a view, or an error message. ht exits 2 when a layer is Present."""
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=60, env=env)
    except FileNotFoundError:
        return "", f"{cmd[0]} not found"
    if r.returncode not in (0, 2) or not r.stdout.strip():
        return r.stdout, (r.stderr or r.stdout).strip()[:200] or f"exit {r.returncode}"
    return r.stdout, None


def check(text: str, case: dict) -> dict:
    found = {name: re.search(rx, text) for name, rx, _ in case["facts"]}
    # Order is judged on the history itself: the timeline's step lines, which start
    # with ┌ │ ○ ● (inputs and steps). Everything above them (the rationale, the
    # bottom line, the "in short" line) summarises the story out of order on
    # purpose. Views without a timeline are taken whole.
    first_step = re.search(r"^\s*[┌│○●]", text, re.M)
    body = text[first_step.start():] if first_step else text
    rx = {name: r for name, r, _ in case["facts"]}
    order = {}
    for a, b in case["order"]:
        ma, mb = re.search(rx[a], body), re.search(rx[b], body)
        order[f"{a} < {b}"] = bool(ma and mb and ma.start() < mb.start())
    return {
        "facts": {k: v is not None for k, v in found.items()},
        "order": order,
        "lines": len(text.splitlines()),
        "noise": len(NOISE.findall(text)),
    }


CRITERIA = ["findability", "plain_language", "misleading_risk", "noise"]

# The built-in questions. A blind set, written by a model that has seen no view
# (--write-questions), can replace them with --questions FILE; answers are then not
# auto-scored, because only the built-in set has keys.
QUESTIONS = [
    "Q1. Was the original image a camera photo? If so, from which device or app?",
    "Q2. Which organisation or organisations signed the recorded steps?",
    "Q3. Was generative AI used? If so, at which step and doing what?",
    "Q4. Is an invisible watermark declared? If so, which one?",
    "Q5. What was the last thing done to this file?",
    "Q6. List the recorded steps in the order they happened, oldest first.",
    "Q7. What does the view say about whether these statements are verified?",
]

PROMPT = """You are evaluating how well different text views explain the provenance of ONE
image file to a non-specialist reader: a photo editor at an agency who must decide
whether the image is a camera photo, an AI generation, or an AI-edited photo, and
who has a few minutes, not a C2PA background.

Below are {n} views of the same file's C2PA provenance, produced by different tools.
They are labelled {labels} in random order; the tools are not named on purpose. Do
not guess which tool made which view, and do not prefer a view for its length,
formatting or confident tone.

{views}

TASK 1 — comprehension. For EACH view separately, answer every question using ONLY
what that view states. Do not use the other views, the file name, or outside
knowledge. If a view does not state the answer, write "not stated". Quote the
words in the view that support each answer.

{questions}

TASK 2 — rating. For each view, rate 1 (poor) to 5 (excellent), with one sentence
of justification that points at the view's own text:
  - findability: how quickly the reader finds the answers above;
  - plain_language: understandable without knowing C2PA;
  - misleading_risk: 5 = hard to misread, 1 = invites a wrong conclusion (e.g.
    presents a signer's statement as established fact, or hides the AI step);
  - noise: 5 = nothing to skip, 1 = mostly identifiers and syntax to read past.

TASK 3 — choice. Which single view should this reader use, and what is the most
important weakness of that view? One short paragraph.

OUTPUT. Reply with one JSON object and nothing else: the first character of your
reply is {{ and the last is }}. No code fences, no text before or after it, plain
ASCII double quotes. Fill in exactly this structure; scores are integers 1 to 5.
Set "judge_model" to your own model name and version as precisely as you know them,
or "unknown" if you do not know:

{skeleton}
"""


WRITE_QUESTIONS = """You will later be shown several text views of ONE image file's C2PA
provenance record (signed statements about how the file was made). The reader is a
photo editor at an agency, not a C2PA specialist, who must decide in a few minutes
whether the image is a camera photo, an AI generation or an AI-edited photo, and
how far the record can be trusted.

You have not seen any view and must not guess what they look like. Write exactly 7
questions this reader needs a view to answer. Each question should be answerable
from a provenance record alone (no looking at the pixels), and together they
should cover what decides the reader's choice and what could mislead them.

Reply with the 7 questions only, one per line, numbered Q1 to Q7, nothing else.
"""


def load_questions(path: str) -> list[str]:
    """A question set from a file, one question per line. Numbering in any common
    form ("Q1.", "1.", "1)", "-") is replaced by Q1, Q2, ...; blank lines and lines
    without a question mark are ignored, so a model's reply can be saved as is."""
    lines = Path(path).read_text().splitlines()
    out = []
    for line in lines:
        text = re.sub(r"^\s*(?:[-*•]|\(?Q?\d+[.):]?\)?)\s*", "", line, flags=re.I).strip()
        text = text.replace("**", "").strip()
        if "?" in text:
            out.append(text)
    if not 3 <= len(out) <= 10:
        sys.exit(f"{path}: found {len(out)} questions; expected 3 to 10, one per line")
    return [f"Q{i}. {q}" for i, q in enumerate(out, 1)]


def skeleton(labels: list[str], n: int = len(QUESTIONS)) -> str:
    """The reply's exact shape for these labels, empty values, for the prompt."""
    qs = [f"Q{i}" for i in range(1, n + 1)]
    obj = {
        "judge_model": "",
        "answers": {l: {q: {"answer": "", "quote": ""} for q in qs} for l in labels},
        "ratings": {l: {c: {"score": 0, "why": ""} for c in CRITERIA} for l in labels},
        "choice": {"view": "", "weakness": ""},
    }
    return json.dumps(obj, indent=2)


def schema(labels: list[str], n: int = len(QUESTIONS)) -> dict:
    """JSON Schema of the reply for structured-output APIs (strict mode: every
    object closed, every property required, labels fixed)."""
    qs = [f"Q{i}" for i in range(1, n + 1)]

    def obj(props: dict) -> dict:
        return {
            "type": "object",
            "properties": props,
            "required": list(props),
            "additionalProperties": False,
        }

    answer = obj({"answer": {"type": "string"}, "quote": {"type": "string"}})
    rating = obj({"score": {"type": "integer", "enum": [1, 2, 3, 4, 5]}, "why": {"type": "string"}})
    return obj(
        {
            "judge_model": {"type": "string"},
            "answers": obj({l: obj({q: answer for q in qs}) for l in labels}),
            "ratings": obj({l: obj({c: rating for c in CRITERIA}) for l in labels}),
            "choice": obj({"view": {"type": "string", "enum": labels}, "weakness": {"type": "string"}}),
        }
    )


def unmarkdown(raw: str) -> str:
    """Undo the escaping a chat app's markdown copy adds: a doubled backslash before
    a quote (\\\\" for \\") and a backslash before characters JSON never escapes
    (\\@, \\:, \\_, \\*, ...)."""
    raw = raw.replace('\\\\"', '\\"')
    return re.sub(r'\\(?=[^"\\/bfnrtu])', "", raw)


CURLY = str.maketrans({"\u201c": '"', "\u201d": '"', "\u201e": '"', "\u2033": '"'})


def _find_reply(raw: str) -> tuple[dict | None, json.JSONDecodeError | None, int]:
    """First JSON object with answers, ratings and choice; else the decode error of
    the outermost candidate (the first '{') and where it starts. Inner objects also
    fail or succeed on their own, but only the outermost one is the reply."""
    dec = json.JSONDecoder()
    first: tuple[json.JSONDecodeError | None, int] = (None, -1)
    for i, ch in enumerate(raw):
        if ch != "{":
            continue
        try:
            obj, _ = dec.raw_decode(raw, i)
        except json.JSONDecodeError as e:
            if first[1] < 0:
                first = (e, i)
            continue
        if isinstance(obj, dict) and {"answers", "ratings", "choice"} <= obj.keys():
            return obj, None, i
    return None, first[0], first[1]


def _is_template(obj: dict) -> bool:
    """The prompt's own empty skeleton: every answer blank, every score 0."""
    answers = [a.get("answer", "") for v in obj["answers"].values() for a in v.values()]
    return all(not a for a in answers)


TRAILING_COMMA = re.compile(r",(\s*[}\]])")

# One string field per line: `"key": "value",`. The value is everything between the
# opening quote and the last quote on the line, so quotes inside it can be escaped.
FIELD_LINE = re.compile(r'^(\s*"[A-Za-z0-9_]+"\s*:\s*")(.*)("\s*,?\s*)$')
MD_LINK = re.compile(r"\[(https?://[^\]\s]+)\]\(\1\)")


def escape_inner_quotes(raw: str) -> str:
    """Escape unescaped double quotes inside one-line string values, as Gemini's
    copy leaves them (`"quote": "… "Created by Pixel Camera.""`). Only lines shaped
    like a single string field are touched; numbers, braces and keys are not."""
    out = []
    for line in raw.splitlines():
        m = FIELD_LINE.match(line)
        if m:
            inner = re.sub(r'(?<!\\)"', r'\\"', m.group(2))
            line = m.group(1) + inner + m.group(3)
        out.append(line)
    return "\n".join(out)


def unwrap_links(raw: str) -> str:
    """`[http://x](http://x)` (a chat app turning URLs into links) back to `http://x`."""
    return MD_LINK.sub(r"\1", raw)


def _unlink(value):
    if isinstance(value, str):
        return unwrap_links(value)
    if isinstance(value, dict):
        return {k: _unlink(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_unlink(v) for v in value]
    return value


def normalize(obj: dict) -> dict:
    """Bring a parsed reply to the expected shape where the meaning is clear:
    labels upper-case, scores as integers, ratings given as [score, why] (the first
    version of the prompt) turned into {"score", "why"}, and markdown links a chat
    app made of URLs turned back into plain URLs."""
    obj = _unlink(obj)
    for part in ("answers", "ratings"):
        if isinstance(obj.get(part), dict):
            obj[part] = {str(k).strip().upper(): v for k, v in obj[part].items()}
    for crit in (obj.get("ratings") or {}).values():
        if not isinstance(crit, dict):
            continue
        for name, val in list(crit.items()):
            if isinstance(val, list) and len(val) == 2:
                val = {"score": val[0], "why": val[1]}
            if isinstance(val, dict) and isinstance(val.get("score"), str):
                digits = re.search(r"[1-5]", val["score"])
                val["score"] = int(digits.group()) if digits else 0
            crit[name] = val
    choice = obj.get("choice")
    if isinstance(choice, dict) and isinstance(choice.get("view"), str):
        choice["view"] = choice["view"].strip().upper().removeprefix("VIEW ").strip()
    return obj


def parse_reply(raw: str) -> tuple[dict | None, list[str], str | None]:
    """The judge's reply as a dict, whatever a chat app did to it on the way: code
    fences, text around it, markdown escaping from the copy, typographic quotes,
    trailing commas. Each repair is tried only if the plain text does not parse,
    and each one used is reported in the notes. On failure, the error says why in
    words a person can act on: prose, a reply cut off, the prompt pasted back, or
    the exact place the JSON breaks."""
    notes: list[str] = []
    obj, err, at = _find_reply(raw)
    if obj is None:
        repairs = [
            ("removed markdown escaping added by the copy", unmarkdown),
            ("replaced typographic double quotes", lambda t: t.translate(CURLY)),
            ("removed trailing commas", lambda t: TRAILING_COMMA.sub(r"\1", t)),
            ("escaped quotes inside values", escape_inner_quotes),
        ]
        text, used = raw, []
        for note, fix in repairs:
            fixed = fix(text)
            if fixed == text:
                continue
            text = fixed
            used.append(note)
            obj, _, _ = _find_reply(text)
            if obj is not None:
                notes.extend(used)
                break
    if obj is not None:
        if _is_template(obj):
            return None, notes, (
                "this is the prompt's empty template, not a reply: the clipboard still held "
                "the prompt. Copy the model's answer."
            )
        return normalize(obj), notes, None
    # The text is not echoed: it is whatever the user supplied, and an error message
    # is no place to repeat it. Length and position are enough to act on.
    where = f"no usable JSON in the reply ({len(raw)} characters)\n"
    if raw.lstrip().startswith("You are evaluating"):
        return None, notes, where + "that is the prompt itself: copy the model's answer"
    if err is None:
        return None, notes, where + (
            "no '{' at all: the reply is prose. Ask the model in the same chat: "
            "'Return only the JSON object, nothing else.'"
        )
    if err.pos >= len(raw.rstrip()) - 3:
        return None, notes, where + (
            f"the JSON starting at character {at} stops before it closes ({err.msg} at line "
            f"{err.lineno}): the reply was probably cut off. Ask the model to 'continue' or to "
            "shorten the quotes, and copy the whole reply."
        )
    return None, notes, where + (
        f"the JSON starting at character {at} breaks at line {err.lineno}, column "
        f"{err.colno}: {err.msg}. Open the reply file at that spot to fix it, or ask the "
        "model to send the JSON again."
    )


def extract(path: str) -> int:
    """--extract: print the judge's JSON from a saved reply."""
    raw = Path(path).read_text() if path != "-" else sys.stdin.read()
    obj, notes, error = parse_reply(raw)
    for n in notes:
        print(f"note: {n}", file=sys.stderr)
    if obj is None:
        print(error, file=sys.stderr)
        return 1
    print(json.dumps(obj, indent=2, ensure_ascii=False))
    return 0


def view_order(names: list[str], seed: int | None) -> list[str]:
    """Order of the views for one run. Seeds 1, 2, 3 are the three rotations, so
    over those three runs every view sits in every position exactly once (a Latin
    square, which cancels a judge's position bias); 4, 5, 6 are the rotations of
    the reversed order. No seed: a random one of the six."""
    orders = [names[i:] + names[:i] for i in range(len(names))]
    rev = names[::-1]
    orders += [rev[i:] + rev[:i] for i in range(len(rev))]
    if seed is None:
        return random.choice(orders)
    return orders[(seed - 1) % len(orders)]


def make_prompt(
    match: str, seed: int | None, questions: list[str] | None = None
) -> tuple[str, dict[str, str], dict, list[str]]:
    """The evaluation prompt for the case whose path contains `match`, the label
    → tool map, the case, and the questions used (the built-in set unless
    `questions` is given). Exits with a message when the case or a view is missing."""
    questions = questions or QUESTIONS
    case = next((c for c in CASES if match in c["file"]), None)
    if case is None:
        sys.exit(f"no case matches '{match}'")
    f = case["file"]
    if not Path(f).exists():
        sys.exit(f"not on disk: {f}")
    settings, env = collector_c2patool()
    sources = {
        "ht --provenance": (["ht", "inspect", "--only", "manifest", "--provenance", f], None),
        "c2patool --tree": (["c2patool", "--settings", str(settings), f, "--tree"], env),
        "c2patool (json)": (["c2patool", "--settings", str(settings), f], env),
    }
    texts = {}
    for name, (cmd, cmd_env) in sources.items():
        text, err = view(cmd, cmd_env)
        if err:
            sys.exit(f"{name}: {err}")
        # The file name would give the answers away (and identify nothing useful).
        texts[name] = text.replace(f, "<file>").replace(Path(f).name, "<file>")
    names = view_order(list(texts), seed)
    labels = [chr(ord("A") + i) for i in range(len(names))]
    blocks = "\n\n".join(
        f"===== VIEW {lab} =====\n{texts[n].rstrip()}\n===== END VIEW {lab} ====="
        for lab, n in zip(labels, names)
    )
    prompt = PROMPT.format(
        n=len(names),
        labels=", ".join(labels),
        views=blocks,
        questions="\n".join(questions),
        skeleton=skeleton(labels, len(questions)),
    )
    return prompt, dict(zip(labels, names)), case, questions


def build_prompt(match: str, seed: int | None, questions: list[str] | None = None) -> int:
    """--prompt: the prompt to stdout, the label and answer keys to stderr."""
    prompt, label_map, case, _ = make_prompt(match, seed, questions)
    print(prompt)
    print("label key:  " + ", ".join(f"{l} = {n}" for l, n in label_map.items()), file=sys.stderr)
    if questions:
        print("custom questions: no answer key; read the answers yourself", file=sys.stderr)
        return 0
    for q, a in (case.get("key") or {}).items():
        print(f"answer key {q}: {a}", file=sys.stderr)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--show", action="store_true", help="print both views per file")
    ap.add_argument(
        "--prompt",
        metavar="MATCH",
        help="print an LLM evaluation prompt for the case whose path contains MATCH",
    )
    ap.add_argument(
        "--extract",
        metavar="REPLY",
        help="print the judge's JSON from a pasted reply file ('-' reads stdin)",
    )
    ap.add_argument(
        "--schema",
        action="store_true",
        help="print the reply's JSON Schema for an API's structured-output mode",
    )
    ap.add_argument(
        "--questions",
        metavar="FILE",
        help="use this question set instead of the built-in one (with --prompt or --schema)",
    )
    ap.add_argument(
        "--write-questions",
        action="store_true",
        help="print a prompt that has a model write a question set blind, before seeing any view",
    )
    ap.add_argument(
        "--seed",
        type=int,
        help="view order: 1, 2, 3 put every view in every position once (default: random)",
    )
    a = ap.parse_args()
    questions = load_questions(a.questions) if a.questions else None
    if a.write_questions:
        print(WRITE_QUESTIONS)
        return 0
    if a.extract:
        return extract(a.extract)
    if a.schema:
        print(json.dumps(schema(["A", "B", "C"], len(questions or QUESTIONS)), indent=2))
        return 0
    if a.prompt:
        return build_prompt(a.prompt, a.seed, questions)

    settings, env = collector_c2patool()
    failures: list[str] = []
    print(f"{'file':<58} {'view':<8} {'facts':>6} {'order':>6} {'lines':>6} {'noise':>6}")
    for case in CASES:
        f = case["file"]
        if not Path(f).exists():
            print(f"{Path(f).name:<58} skipped, not on disk")
            continue
        views = {
            "ht": view(["ht", "inspect", "--only", "manifest", "--provenance", f]),
            "c2patool": view(["c2patool", "--settings", str(settings), f, "--tree"], env),
        }
        results = {}
        for name, (text, err) in views.items():
            if err:
                print(f"{Path(f).name:<58} {name:<8} error: {err}")
                continue
            r = check(text, case)
            results[name] = r
            nf, tf = sum(r["facts"].values()), len(r["facts"])
            no, to = sum(r["order"].values()), len(r["order"])
            print(
                f"{Path(f).name:<58} {name:<8} {nf:>3}/{tf:<2} {no:>3}/{to:<2} "
                f"{r['lines']:>6} {r['noise']:>6}"
            )
            if a.show:
                print("\n".join("    | " + line for line in text.splitlines()), "\n")
        ht, tree = results.get("ht"), results.get("c2patool")
        if ht is None:
            failures.append(f"{Path(f).name}: no Halftone view")
            continue
        required = {n for n, _, req in case["facts"] if req}
        for fact, ok in ht["facts"].items():
            if not ok and (fact in required or (tree and tree["facts"].get(fact))):
                failures.append(f"{Path(f).name}: Halftone's view lacks '{fact}'")
        for pair, ok in ht["order"].items():
            if not ok:
                failures.append(f"{Path(f).name}: Halftone's view breaks order '{pair}'")
    if failures:
        print("\nproblems:")
        print("\n".join(f"  {x}" for x in failures))
        return 1
    print("\nHalftone's view states every fact and order; compare lines and noise above.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
