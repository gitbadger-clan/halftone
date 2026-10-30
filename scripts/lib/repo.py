"""Shared basics for the scripts in scripts/ and scripts/pub/: the repo root, failing
with a message, checking for external tools, and terminal colours.

Import from a script (PEP 723 scripts cannot declare a local dependency, and there is
no venv, so the lib goes on sys.path):

    sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))        # scripts/
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))    # scripts/pub/
    from repo import ROOT, die, need
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parents[2]
CORPUS = ROOT / "corpus" / "differential"
TRUST_DIR = ROOT / "crates" / "halftone-c2pa" / "trust"


def die(msg: str, code: int = 1) -> NoReturn:
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(code)


def need(*tools: str) -> None:
    """Exit 2 naming every missing tool at once, not the first one only."""
    missing = [t for t in tools if shutil.which(t) is None]
    if missing:
        die(f"not on PATH: {', '.join(missing)}", 2)


class C:
    """ANSI colours, the ones the fish frames used via set_color, so a converted frame
    renders the same under freeze. Empty when NO_COLOR is set."""

    _on = "NO_COLOR" not in os.environ
    BOLD_CYAN = "\x1b[1;96m" if _on else ""
    DIM = "\x1b[90m" if _on else ""
    GREEN = "\x1b[92m" if _on else ""
    RED = "\x1b[91m" if _on else ""
    YELLOW = "\x1b[33m" if _on else ""
    RESET = "\x1b[0m" if _on else ""
