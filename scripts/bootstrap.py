#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Prepare this machine for the scripts once: Python, every script's locked
environment, and a check of the external tools.

    uv run scripts/bootstrap.py              # lock what has no lock yet, sync everything, check tools
    uv run scripts/bootstrap.py --upgrade    # re-resolve every lock to the newest allowed versions
    uv run scripts/bootstrap.py --tools      # only the tool check

Each script with dependencies (PEP 723) gets its lock file (`<script>.lock`, commit
it) and its environment built in uv's cache. After that, `uv run` starts a script
without resolving or downloading anything, and works offline (UV_OFFLINE=1).
Downloads and installs happen here, once; re-run after pulling changes to a script's
dependencies, or after `uv cache clean`.

Standard library only, so it runs on a machine where nothing is set up yet. When the
environments are synced it records that in target/scripts/bootstrap.json, which
scripts/dev.py reads.
Network: PyPI and GitHub (for Python and packages), unless everything is cached.
Exit 0 ready, 1 a lock or sync failed or a required tool is missing.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from envstate import ROOT, SCRIPTS, dependencies, record  # noqa: E402
from progress import step  # noqa: E402

PYTHON = "3.14"

# (tool, version command, needed by, required for the collector)
TOOLS = [
    ("uv", ["uv", "--version"], "every script", True),
    ("git", ["git", "--version"], "fetch, collect, refresh", True),
    ("exiftool", ["exiftool", "-ver"], "collect, fetch, gen-synthetic", True),
    ("c2patool", ["c2patool", "--version"], "collect, c2p", True),
    ("openssl", ["openssl", "version"], "collect (signing certificates)", True),
    ("cargo", ["cargo", "--version"], "preflight, differential test", False),
    ("jq", ["jq", "--version"], "pub frames", False),
    ("freeze", ["freeze", "--version"], "pub frames (--png)", False),
    ("adb", ["adb", "version"], "htc (phone)", False),
    ("magick", ["magick", "-version"], "corpus-inspect", False),
    ("docker", ["docker", "--version"], "offline-docker", False),
]


def uv(*args: str) -> tuple[bool, str]:
    r = subprocess.run(["uv", *args], cwd=ROOT, capture_output=True, text=True)
    return r.returncode == 0, (r.stderr or r.stdout).strip()


def version(cmd: list[str]) -> str | None:
    if shutil.which(cmd[0]) is None:
        return None
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.TimeoutExpired):
        return "found, version unknown"
    first = (r.stdout or r.stderr).strip().splitlines()
    return first[0][:44] if first else "found"


def check_tools() -> bool:
    print("\n== external tools")
    ok = True
    for name, cmd, used_by, required in TOOLS:
        v = version(cmd)
        mark = "ok  " if v else ("MISS" if required else "-   ")
        ok &= bool(v) or not required
        print(f"  {mark} {name:<9} {v or 'not on PATH':<45} {used_by}")
    cred = Path(os.environ.get("CREDENTIO", "~/oss-bins/c2pa_validate")).expanduser()
    rub = (ROOT / os.environ.get("RUBRICS", "../conformance-public")).resolve()
    have_cred = cred.is_file() and os.access(cred, os.X_OK)
    have_rub = (rub / "asset-rubrics" / "c2pa_signals_rubric_evaluator.py").is_file()
    print(f"  {'ok  ' if have_cred else '-   '} credentio {str(cred) if have_cred else 'not found ($CREDENTIO)':<45} collect (or --no-credentio)")
    print(f"  {'ok  ' if have_rub else '-   '} rubrics   {str(rub) if have_rub else 'not found ($RUBRICS)':<45} collect (or --no-rubrics)")
    return ok


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--upgrade", action="store_true", help="re-resolve every lock to the newest allowed versions")
    ap.add_argument("--tools", action="store_true", help="only check the external tools")
    a = ap.parse_args()
    if shutil.which("uv") is None:
        print("uv not found: https://docs.astral.sh/uv/getting-started/installation/", file=sys.stderr)
        return 1
    if a.tools:
        return 0 if check_tools() else 1

    failed = False
    with step(f"Python {PYTHON}", summary=False):
        ok, msg = uv("python", "install", PYTHON)
    print(f"== Python {PYTHON}: {'ready' if ok else 'FAILED: ' + msg[:200]}")
    failed |= not ok

    print("\n== script environments")
    todo = [(s, dependencies(s)) for s in sorted(SCRIPTS.glob("*.py"))]
    todo = [(s, d) for s, d in todo if d]
    for i, (script, deps) in enumerate(todo, 1):
        name = script.relative_to(ROOT).as_posix()
        lock = script.with_name(script.name + ".lock")
        t = time.monotonic()
        steps = []
        with step(f"[{i}/{len(todo)}] {name}", summary=False):
            if a.upgrade or not lock.exists():
                ok, msg = uv("lock", "--script", name, *(["--upgrade"] if a.upgrade else []))
                steps.append("locked" if ok else f"lock FAILED: {msg.splitlines()[-1][:120] if msg else ''}")
                failed |= not ok
            if not failed or lock.exists():
                ok, msg = uv("sync", "--script", name)
                steps.append("synced" if ok else f"sync FAILED: {msg.splitlines()[-1][:120] if msg else ''}")
                failed |= not ok
        print(f"  {name:<36} {len(deps):>2} deps  {', '.join(steps):<24} {time.monotonic() - t:5.1f} s")

    if not failed:
        record("done")  # scripts/dev.py stops offering bootstrap until the locks change
    failed |= not check_tools()
    new_locks = subprocess.run(["git", "status", "--short", "--", "scripts/*.lock"], cwd=ROOT,
                               capture_output=True, text=True).stdout.strip()
    if new_locks:
        print(f"\nlock files to commit:\n{new_locks}")
    print("\nready" if not failed else "\nnot ready: see FAILED and MISS above")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
