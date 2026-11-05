#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27"]
#
# [tool.uv]
# exclude-newer = "14 days"  # only releases at least two weeks old
# ///
"""Re-collect the differential ground truth, stratum by stratum, with the same
references as last time.

Every stratum gets the vendored trust inputs: manifest anchors, TSA anchors and EKU
policy, so every header passes preflight step 8. Whether a stratum also gets
Credentio and the C2PA rubrics follows its own expectations.json: a re-collection
reproduces the inputs the stratum was last collected with, so a plain run never
changes what a stratum is compared on. A new stratum (no expectations.json yet) gets
both. Changing a stratum's inputs is an explicit decision: --with-credentio /
--no-credentio and --with-rubrics / --no-rubrics override the record for every
stratum named. The rubrics read Credentio's crJSON, so they need it; 08-multipart
cannot drop Credentio, its reference for the multi-asset hash (D-014).

Before a stratum is collected, every file its expectations.json records must be on
disk. A fresh checkout has the ground truth but not the uncommitted files, and
collecting there would replace the record with an empty one. Such a stratum is
skipped; --allow-missing collects it anyway (after a deliberate rename or deletion).

Ends with `git diff --stat` over corpus/differential. Exit 1 if any stratum failed or
was skipped, 2 on a usage or setup error. No network (c2patool runs with fetching
off). Never reads the clipboard.
"""

from __future__ import annotations

import json
import os
import re
import shlex
import subprocess
import sys
import time
from pathlib import Path
from typing import Annotated

import typer

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from progress import clock  # noqa: E402
from repo import CORPUS, ROOT, TRUST_DIR, C, die, need  # noqa: E402

COLLECTOR = ROOT / "scripts" / "differential.py"
MULTIPART = "08-multipart"
STRATUM = re.compile(r"^\d\d-")
EXAMPLES = """
Examples:

    uv run scripts/collect.py                 # every stratum, each with its recorded inputs
    uv run scripts/collect.py 04 06 07 08     # these strata (number or full name)
    uv run scripts/collect.py 05-fold         # a new stratum: Credentio and rubrics included
    uv run scripts/collect.py 01 --with-credentio --with-rubrics   # change 01's inputs, on purpose
    uv run scripts/collect.py --dry-run       # the checks, inputs and commands, nothing run
"""

app = typer.Typer(
    add_completion=False,
    pretty_exceptions_show_locals=False,  # never print locals (paths today, keys elsewhere)
    rich_markup_mode="markdown",
)


def strata_dirs() -> list[Path]:
    return sorted(d for d in CORPUS.iterdir() if d.is_dir() and STRATUM.match(d.name))


def resolve(name: str) -> Path:
    """`04` or `04-generators` -> the stratum directory."""
    hits = [d for d in strata_dirs() if d.name == name or d.name.startswith(f"{name}-")]
    if len(hits) != 1:
        known = ", ".join(d.name for d in strata_dirs())
        die(f"'{name}' names {len(hits)} strata (known: {known})", 2)
    return hits[0]


def record(stratum: Path) -> dict | None:
    exp = stratum / "expectations.json"
    return json.loads(exp.read_text()) if exp.is_file() else None


def missing_files(rec: dict | None, stratum: Path) -> tuple[int, list[str]]:
    """How many files the record lists, and which are not on disk."""
    files = (rec or {}).get("files", {})
    return len(files), [r for r in sorted(files) if not (stratum / r).is_file()]


def recorded_inputs(rec: dict | None) -> tuple[bool, bool] | None:
    """(credentio, rubrics) the stratum was last collected with; None for a new stratum."""
    if rec is None:
        return None
    credentio = bool((rec.get("tools") or {}).get("credentio"))
    rubrics = bool(rec.get("signals_rubric") or rec.get("conformance_rubrics"))
    return credentio, rubrics


AS_LAST_TIME = "as last time"


def why_color(why: str) -> str:
    """Green when a stratum keeps its recorded inputs, yellow when they change or
    the stratum is new: either way it is compared on something it was not before."""
    return C.GREEN if why == AS_LAST_TIME else C.YELLOW


def rel(p: Path) -> str:
    try:
        return str(p.resolve().relative_to(ROOT))
    except ValueError:
        return str(p)


@app.command(help=__doc__, epilog=EXAMPLES)
def main(
    strata: Annotated[
        list[str] | None,
        typer.Argument(help="Stratum number or name. Default: every stratum with an expectations.json."),
    ] = None,
    credentio: Annotated[
        Path,
        typer.Option(envvar="CREDENTIO", help="Credentio's c2pa_validate (a symlink to the pinned copy)."),
    ] = Path("~/oss-bins/c2pa_validate"),
    rubrics: Annotated[
        Path,
        typer.Option(envvar="RUBRICS", help="conformance-public checkout; relative paths are from the repo root."),
    ] = Path("../conformance-public"),
    with_credentio: Annotated[
        bool | None,
        typer.Option("--with-credentio/--no-credentio", show_default=False,
                     help="Override the recorded choice: collect with / without Credentio."),
    ] = None,
    with_rubrics: Annotated[
        bool | None,
        typer.Option("--with-rubrics/--no-rubrics", show_default=False,
                     help="Override the recorded choice: collect with / without the C2PA rubrics."),
    ] = None,
    allow_missing: Annotated[
        bool, typer.Option("--allow-missing", help="Collect even when recorded files are gone (the record shrinks).")
    ] = False,
    dry_run: Annotated[bool, typer.Option("--dry-run", help="Show checks, inputs and commands, run nothing.")] = False,
) -> None:
    if strata:
        chosen = list(dict.fromkeys(resolve(s) for s in strata))
    else:
        chosen = [d for d in strata_dirs() if (d / "expectations.json").is_file()]
        if not chosen:
            die(f"no stratum under {rel(CORPUS)} has an expectations.json; name one", 2)

    anchors = TRUST_DIR / "C2PA-TRUST-LIST.pem"
    tsa = TRUST_DIR / "C2PA-TSA-TRUST-LIST.pem"
    eku = TRUST_DIR / "C2PA-EKU-CONFIG.cfg"
    for p in (anchors, tsa, eku, COLLECTOR):
        if not p.is_file():
            die(f"missing {rel(p)}", 2)

    # Decide every stratum's inputs before running anything, so a bad combination
    # stops the run instead of half-collecting it.
    plan: list[tuple[Path, dict | None, bool, bool, str]] = []
    for s in chosen:
        rec = record(s)
        before = recorded_inputs(rec)
        use_cred = with_credentio if with_credentio is not None else (before[0] if before else True)
        use_rub = with_rubrics if with_rubrics is not None else (before[1] if before else True)
        if use_rub and not use_cred:
            die(f"{s.name}: the rubrics need Credentio (they read its crJSON); "
                "pass --with-credentio or --no-rubrics", 2)
        if s.name == MULTIPART and not use_cred:
            die(f"{MULTIPART} needs Credentio: it is the reference for the multi-asset hash (D-014)", 2)
        if before is None:
            why = "new stratum"
        elif (use_cred, use_rub) == before:
            why = AS_LAST_TIME
        else:
            why = f"CHANGED from {'with' if before[0] else 'without'} Credentio, " \
                  f"{'with' if before[1] else 'without'} rubrics"
        plan.append((s, rec, use_cred, use_rub, why))

    cred = rub = None
    if any(p[2] for p in plan):
        cred = credentio.expanduser()
        if not (cred.is_file() and os.access(cred, os.X_OK)):
            die(f"Credentio not executable at {cred} (set CREDENTIO, or pass --no-credentio)", 2)
    if any(p[3] for p in plan):
        rub = (ROOT / rubrics.expanduser()).resolve()
        if not (rub / "asset-rubrics" / "c2pa_signals_rubric_evaluator.py").is_file():
            die(f"no conformance-public checkout at {rub} (set RUBRICS, or pass --no-rubrics)", 2)
    need("uv", "git", "exiftool", "c2patool", "openssl")

    base = ["--trust-anchors", rel(anchors), "--tsa-anchors", rel(tsa), "--trust-config", rel(eku)]
    results: list[tuple[str, str, bool, str]] = []  # stratum, result, ok, why
    started = time.monotonic()
    for i, (s, rec, use_cred, use_rub, why) in enumerate(plan, 1):
        recorded, gone = missing_files(rec, s)
        inputs = " · ".join(["trust"] + ["Credentio"] * use_cred + ["rubrics"] * use_rub)
        print(f"\n{C.BOLD_CYAN}== {s.name}{C.RESET}  {C.DIM}[{i}/{len(plan)}]{C.RESET}  "
              f"{why_color(why)}{inputs} ({why}){C.RESET}")
        if gone and not allow_missing:
            print(f"{C.RED}skip:{C.RESET} {len(gone)} of {recorded} recorded files are not on disk, e.g.")
            for g in gone[:3]:
                print(f"  {g}")
            results.append((s.name, f"skipped, {len(gone)} of {recorded} recorded files missing", False, why))
            continue
        extra = (["--credentio", str(cred)] if use_cred else []) + (["--rubrics", str(rub)] if use_rub else [])
        cmd = ["uv", "run", rel(COLLECTOR), rel(s), *base, *extra]
        if dry_run:
            print(shlex.join(cmd))
            note = f" ({len(gone)} recorded files missing, --allow-missing)" if gone else ""
            results.append((s.name, "would collect" + note, True, why))
            continue
        exp = s / "expectations.json"
        before_bytes = exp.read_bytes() if exp.is_file() else None
        t = time.monotonic()
        rc = subprocess.run(cmd, cwd=ROOT).returncode
        took = clock(time.monotonic() - t)
        if rc != 0:
            results.append((s.name, f"failed, exit {rc}, after {took}", False, why))
            continue
        after_bytes = exp.read_bytes() if exp.is_file() else None
        if before_bytes is None:
            result = "new"
        elif after_bytes == before_bytes:
            result = "unchanged"
        else:
            result = "rewritten" + (f", {len(gone)} recorded files dropped" if gone else "")
        results.append((s.name, f"{result}, {took}", True, why))

    print(f"\n{C.BOLD_CYAN}== summary{C.RESET}  {C.DIM}{clock(time.monotonic() - started)}{C.RESET}")
    same = sum(why == AS_LAST_TIME for *_, why in results)
    # Green: every stratum compared on what it was compared on before. Red: none
    # was. Yellow: a mix, so read the rows below before trusting the diff.
    tone = C.GREEN if same == len(results) else C.RED if same == 0 else C.YELLOW
    print(f"  {tone}inputs: {same} of {len(results)} strata as last time{C.RESET}")
    width = max(len(n) for n, *_ in results)
    rwidth = max(len(r) for _, r, *_ in results)
    for name, result, ok, why in results:
        print(f"  {name:<{width}}  {(C.GREEN if ok else C.RED)}{result:<{rwidth}}{C.RESET}  "
              f"{why_color(why)}{why}{C.RESET}")
    if not dry_run:
        print()
        subprocess.run(["git", "diff", "--stat", "--", rel(CORPUS)], cwd=ROOT)
    raise typer.Exit(0 if all(ok for _, _, ok, _ in results) else 1)


if __name__ == "__main__":
    app()
