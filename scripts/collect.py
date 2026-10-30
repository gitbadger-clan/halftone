#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27"]
# ///
"""Re-collect the differential ground truth with every reference, stratum by stratum.

Each stratum gets the same inputs: the vendored manifest anchors, TSA anchors and EKU
policy (so every header passes preflight step 8 by construction), Credentio and the
conformance-public checkout. 08-multipart refuses --no-credentio, because Credentio
is its reference (D-014).

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

    uv run scripts/collect.py                 # every stratum that has an expectations.json
    uv run scripts/collect.py 04 06 07 08     # these strata (number or full name)
    uv run scripts/collect.py 05-fold         # a new stratum: no expectations.json yet
    uv run scripts/collect.py --dry-run       # the checks and commands, nothing run
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


def missing_files(stratum: Path) -> tuple[int, list[str]]:
    """How many files the stratum's expectations.json records, and which are not on disk."""
    exp = stratum / "expectations.json"
    if not exp.is_file():
        return 0, []
    files = json.loads(exp.read_text()).get("files", {})
    return len(files), [rel for rel in sorted(files) if not (stratum / rel).is_file()]


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
    no_credentio: Annotated[bool, typer.Option("--no-credentio", help="Collect without Credentio.")] = False,
    no_rubrics: Annotated[bool, typer.Option("--no-rubrics", help="Collect without the C2PA rubrics.")] = False,
    allow_missing: Annotated[
        bool, typer.Option("--allow-missing", help="Collect even when recorded files are gone (the record shrinks).")
    ] = False,
    dry_run: Annotated[bool, typer.Option("--dry-run", help="Show checks and commands, run nothing.")] = False,
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

    extra: list[str] = []
    if no_credentio:
        if not no_rubrics:
            die("--no-credentio needs --no-rubrics too: the rubrics read Credentio's crJSON", 2)
        if any(s.name == MULTIPART for s in chosen):
            die(f"{MULTIPART} needs Credentio: it is the reference for the multi-asset hash (D-014)", 2)
    else:
        cred = credentio.expanduser()
        if not (cred.is_file() and os.access(cred, os.X_OK)):
            die(f"Credentio not executable at {cred} (set CREDENTIO, or pass --no-credentio)", 2)
        extra += ["--credentio", str(cred)]
    if not no_rubrics:
        rub = (ROOT / rubrics.expanduser()).resolve()
        if not (rub / "asset-rubrics" / "c2pa_signals_rubric_evaluator.py").is_file():
            die(f"no conformance-public checkout at {rub} (set RUBRICS, or pass --no-rubrics)", 2)
        extra += ["--rubrics", str(rub)]
    need("uv", "git", "exiftool", "c2patool", "openssl")

    base = ["--trust-anchors", rel(anchors), "--tsa-anchors", rel(tsa), "--trust-config", rel(eku)]
    results: list[tuple[str, str, bool]] = []  # stratum, result, ok
    started = time.monotonic()
    for i, s in enumerate(chosen, 1):
        recorded, gone = missing_files(s)
        print(f"\n{C.BOLD_CYAN}== {s.name}{C.RESET}  {C.DIM}[{i}/{len(chosen)}]{C.RESET}")
        if gone and not allow_missing:
            print(f"{C.RED}skip:{C.RESET} {len(gone)} of {recorded} recorded files are not on disk, e.g.")
            for g in gone[:3]:
                print(f"  {g}")
            results.append((s.name, f"skipped, {len(gone)} of {recorded} recorded files missing", False))
            continue
        cmd = ["uv", "run", rel(COLLECTOR), rel(s), *base, *extra]
        if dry_run:
            print(shlex.join(cmd))
            note = f" ({len(gone)} recorded files missing, --allow-missing)" if gone else ""
            results.append((s.name, "would collect" + note, True))
            continue
        exp = s / "expectations.json"
        before = exp.read_bytes() if exp.is_file() else None
        t = time.monotonic()
        rc = subprocess.run(cmd, cwd=ROOT).returncode
        took = clock(time.monotonic() - t)
        if rc != 0:
            results.append((s.name, f"failed, exit {rc}, after {took}", False))
            continue
        after = exp.read_bytes() if exp.is_file() else None
        if before is None:
            result = "new"
        elif after == before:
            result = "unchanged"
        else:
            result = "rewritten" + (f", {len(gone)} recorded files dropped" if gone else "")
        results.append((s.name, f"{result}, {took}", True))

    print(f"\n{C.BOLD_CYAN}== summary{C.RESET}  {C.DIM}{clock(time.monotonic() - started)}{C.RESET}")
    width = max(len(n) for n, _, _ in results)
    for name, result, ok in results:
        print(f"  {name:<{width}}  {(C.GREEN if ok else C.RED)}{result}{C.RESET}")
    if not dry_run:
        print()
        subprocess.run(["git", "diff", "--stat", "--", rel(CORPUS)], cwd=ROOT)
    raise typer.Exit(0 if all(ok for _, _, ok in results) else 1)


if __name__ == "__main__":
    app()
