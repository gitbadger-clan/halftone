#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27"]
# ///
"""Rebuild the public differential strata (01 c2pa-rs fixtures, 02 ExifTool images,
03 synthetic), re-collect their ground truth, and run the differential test.

Pins: $C2PA_FIXTURES_COMMIT / $EXIFTOOL_FIXTURES_COMMIT when set; with --keep-pins
the commits recorded in each SOURCE.txt; otherwise upstream HEAD today. The ground
truth is collected by scripts/collect.py with the same trust inputs as every other
stratum, so the headers pass preflight step 8. A fetch just defined each file set,
so files that upstream dropped are dropped from the record too (--allow-missing).

Ground truth (expectations.json, cases.json) is what gets committed, not the files.
Afterwards `git diff corpus/differential` shows what changed in the reference tools'
view, and the two pins printed at the end go into .github/workflows/differential.yml.
Network: github.com (git). Exit 0 done, 1 a step failed, 2 setup error.
"""

from __future__ import annotations

import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Annotated

import typer

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from progress import clock  # noqa: E402
from repo import CORPUS, ROOT, C, die, need  # noqa: E402

SCRIPTS = ROOT / "scripts"
PINS = [  # env var, upstream, stratum, fetch-fixtures source
    ("C2PA_FIXTURES_COMMIT", "https://github.com/contentauth/c2pa-rs", "01-c2pa-rs", "c2pa-rs"),
    ("EXIFTOOL_FIXTURES_COMMIT", "https://github.com/exiftool/exiftool", "02-exiftool", "exiftool"),
]

app = typer.Typer(add_completion=False, pretty_exceptions_show_locals=False)


class Steps:
    """Numbered headers and a duration per step; the steps' own output streams between."""

    def __init__(self, total: int) -> None:
        self.n, self.total, self.start = 0, total, time.monotonic()

    def run(self, title: str, cmd: list[str]) -> None:
        self.n += 1
        print(f"\n{C.BOLD_CYAN}== {title}{C.RESET}  {C.DIM}[{self.n}/{self.total}]{C.RESET}")
        t = time.monotonic()
        rc = subprocess.run(cmd, cwd=ROOT).returncode
        print(f"{C.DIM}{title}: {clock(time.monotonic() - t)} (total {clock(time.monotonic() - self.start)}){C.RESET}")
        if rc != 0:
            die(f"{title}: exit {rc}", 1)


def pin(var: str, upstream: str, stratum: str, keep: bool) -> str:
    if os.environ.get(var):
        print(f"{var}: from environment")
        return os.environ[var]
    source_txt = CORPUS / stratum / "SOURCE.txt"
    if keep and source_txt.is_file():
        for line in source_txt.read_text().splitlines():
            if line.startswith("commit: "):
                print(f"{var}: from {source_txt.relative_to(ROOT)}")
                return line.removeprefix("commit: ").strip()
    r = subprocess.run(["git", "ls-remote", upstream, "HEAD"], capture_output=True, text=True)
    sha = r.stdout.split("\t")[0].strip() if r.returncode == 0 else ""
    if len(sha) != 40:
        die(f"git ls-remote {upstream}: {(r.stderr or 'no HEAD').strip()[:200]}", 1)
    print(f"{var}: upstream HEAD")
    return sha


@app.command(help=__doc__)
def main(
    keep_pins: Annotated[bool, typer.Option("--keep-pins", help="Reuse the commits in SOURCE.txt.")] = False,
    no_test: Annotated[bool, typer.Option("--no-test", help="Stop after collecting.")] = False,
    no_credentio: Annotated[bool, typer.Option("--no-credentio", help="Passed to collect.py.")] = False,
    no_rubrics: Annotated[bool, typer.Option("--no-rubrics", help="Passed to collect.py.")] = False,
) -> None:
    need("git", "uv", "exiftool")
    flags = [f for f, on in (("--no-credentio", no_credentio), ("--no-rubrics", no_rubrics)) if on]
    collect = ["uv", "run", str(SCRIPTS / "collect.py"), "01", "02", "03", "--allow-missing", *flags]
    # Fail on a missing collector input now, not after a minute of cloning.
    r = subprocess.run([*collect, "--dry-run"], cwd=ROOT, capture_output=True, text=True)
    if r.returncode != 0:
        die(f"collector not ready: {(r.stderr or r.stdout).strip().splitlines()[-1]}", 2)
    pins = {var: pin(var, up, stratum, keep_pins) for var, up, stratum, _ in PINS}
    steps = Steps(len(PINS) + 2 + (0 if no_test else 1))
    for var, _, stratum, source in PINS:
        steps.run(f"{stratum} @ {pins[var][:12]}",
                  ["uv", "run", str(SCRIPTS / "fetch-fixtures.py"), source, "--refresh", "--commit", pins[var]])
    steps.run("03-synthetic", ["uv", "run", str(SCRIPTS / "gen-synthetic.py"), "--refresh"])
    steps.run("ground truth", collect)

    print(f"\n{C.BOLD_CYAN}== pins for .github/workflows/differential.yml{C.RESET}")
    for var in pins:
        print(f"  {var}: {pins[var]}")
    if not no_test:
        steps.run("differential test", ["cargo", "test", "-p", "halftone-cli", "--features", "c2pa",
                                        "--test", "differential", "--", "--ignored", "--nocapture"])


if __name__ == "__main__":
    app()
