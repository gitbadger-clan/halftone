#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""One entry point for every task in scripts/.

    uv run scripts/dev.py                    # list the tasks
    uv run scripts/dev.py collect 04 06      # = uv run scripts/collect.py 04 06
    uv run scripts/dev.py collect --help     # the task's own help
    uv run scripts/dev.py bootstrap          # prepare this machine (also offered automatically)

A task is any scripts/<name>.py; a new script shows up here without registering it.
Everything after the task name goes to the task unchanged, and the task's exit code
is dev's exit code. Running a script directly keeps working: dev adds a list and a
setup check, nothing a script depends on.

Before a task runs, dev checks whether scripts/bootstrap.py has prepared the scripts
as they are now. If not, and this is an interactive terminal outside CI, it explains
why and asks once; a "no" is remembered until a script's dependencies change. In CI
($CI set) or without a terminal it never asks. --no-setup-check skips the check.

Shortcut: `alias dev 'uv run scripts/dev.py'` (fish) or
`alias dev='uv run scripts/dev.py'` (bash, zsh). Standard library only: it starts
fast and works before anything is installed.
"""

from __future__ import annotations

import ast
import difflib
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from envstate import SCRIPTS, record, state  # noqa: E402
from repo import C  # noqa: E402

NOT_TASKS = {"dev"}
WHY = """\
These scripts each declare their own Python packages. Bootstrap installs them all
once, pinned by the committed lock files, so every task starts in under a second
and works offline afterwards; without it, a task's first run downloads what it
needs. It also checks for ExifTool, c2patool, Credentio and the other tools the
tasks call, and says which are missing. It takes about a minute the first time."""


def tasks() -> dict[str, str]:
    """name -> first line of the script's docstring."""
    out = {}
    for p in sorted(SCRIPTS.glob("*.py")):
        if p.stem in NOT_TASKS:
            continue
        try:
            doc = ast.get_docstring(ast.parse(p.read_text())) or ""
        except SyntaxError:
            doc = "(does not parse)"
        para = " ".join(doc.strip().split("\n\n")[0].split()) if doc.strip() else ""
        out[p.stem] = para
    return out


def show_tasks() -> None:
    print(__doc__.splitlines()[0])
    print(f"\nusage: uv run scripts/dev.py [--no-setup-check] TASK [ARGS…]\n\n{C.BOLD_CYAN}tasks{C.RESET}")
    t = tasks()
    width = max(map(len, t))
    room = max(30, shutil.get_terminal_size((100, 20)).columns - width - 4)
    for name, line in t.items():
        line = line if len(line) <= room else line[: room - 1].rstrip() + "…"
        print(f"  {name:<{width}}  {C.DIM}{line}{C.RESET}")
    print(f"\n{C.DIM}TASK --help shows a task's own options. Frames live in scripts/pub/ (fish).{C.RESET}")


def interactive() -> bool:
    return not os.environ.get("CI") and sys.stdin.isatty() and sys.stdout.isatty()


def offer_bootstrap() -> None:
    s = state()
    if s in ("current", "declined") or not interactive():
        return
    first = "has not been run on this machine" if s == "missing" else "is out of date: a script's dependencies changed"
    print(f"{C.YELLOW}Bootstrap {first}.{C.RESET}\n\n{WHY}\n")
    try:
        answer = input("Run it now? [Y/n] ").strip().lower()
    except EOFError:
        answer = "n"
    if answer in ("", "y", "yes"):
        rc = subprocess.run(["uv", "run", str(SCRIPTS / "bootstrap.py")]).returncode
        if rc != 0:
            print(f"{C.YELLOW}bootstrap reported problems (above); continuing with the task{C.RESET}\n")
    else:
        record("declined")
        print(f"{C.DIM}Not asking again until a script's dependencies change; "
              f"run `uv run scripts/dev.py bootstrap` any time.{C.RESET}\n")


def main() -> int:
    args = sys.argv[1:]
    check = True
    if args and args[0] == "--no-setup-check":
        check, args = False, args[1:]
    if not args or args[0] in ("-h", "--help", "help"):
        show_tasks()
        return 0
    name, rest = args[0], args[1:]
    known = tasks()
    if name not in known:
        close = difflib.get_close_matches(name, known, n=3)
        hint = f" (did you mean {', '.join(close)}?)" if close else ""
        print(f"error: no task '{name}'{hint}; `uv run scripts/dev.py` lists them", file=sys.stderr)
        return 2
    if check and name != "bootstrap":
        offer_bootstrap()
    # Replace this process: the task gets the terminal, signals and exit code directly.
    os.execvp("uv", ["uv", "run", str(SCRIPTS / f"{name}.py"), *rest])


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BrokenPipeError:  # `dev | head`
        sys.exit(0)
