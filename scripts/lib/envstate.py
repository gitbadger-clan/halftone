"""Whether scripts/bootstrap.py has prepared this machine for the scripts as they are
now: a fingerprint of every script's dependency block and lock file, recorded by
bootstrap after it synced, compared by scripts/dev.py before it runs a task.

The record lives in target/scripts/ (already ignored by git); `cargo clean` removes
it, and the next `dev` run offers bootstrap again, which is then quick.
Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import re
import tomllib
from pathlib import Path
from typing import Literal

SCRIPTS = Path(__file__).resolve().parents[1]
ROOT = SCRIPTS.parent
STAMP = ROOT / "target" / "scripts" / "bootstrap.json"
# PEP 723's reference regex for the inline metadata block.
BLOCK = re.compile(r"(?m)^# /// (?P<type>[a-zA-Z0-9-]+)$\s(?P<content>(^#(| .*)$\s)+)^# ///$")

State = Literal["current", "stale", "missing", "declined"]


def metadata(script: Path) -> str | None:
    """The script's PEP 723 block as TOML text, or None."""
    m = next((m for m in BLOCK.finditer(script.read_text()) if m["type"] == "script"), None)
    if m is None:
        return None
    return "".join(line[2:] if line.startswith("# ") else line[1:] for line in m["content"].splitlines(keepends=True))


def dependencies(script: Path) -> list[str]:
    toml = metadata(script)
    return tomllib.loads(toml).get("dependencies", []) if toml else []


def scripts_with_dependencies() -> list[Path]:
    return [p for p in sorted(SCRIPTS.glob("*.py")) if dependencies(p)]


def fingerprint() -> str:
    """Changes whenever a script's dependencies or its lock file change."""
    h = hashlib.sha256()
    for p in scripts_with_dependencies():
        lock = p.with_name(p.name + ".lock")
        h.update(p.name.encode() + b"\0" + (metadata(p) or "").encode() + b"\0")
        h.update(lock.read_bytes() if lock.is_file() else b"no lock")
    return h.hexdigest()


def state() -> State:
    try:
        rec = json.loads(STAMP.read_text())
    except (OSError, ValueError):
        return "missing"
    if rec.get("fingerprint") != fingerprint():
        return "stale"
    return "declined" if rec.get("status") == "declined" else "current"


def record(status: Literal["done", "declined"]) -> None:
    STAMP.parent.mkdir(parents=True, exist_ok=True)
    STAMP.write_text(json.dumps({"fingerprint": fingerprint(), "status": status}, indent=2) + "\n")
