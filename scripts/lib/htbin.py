"""Which `ht` a script runs, and whether it is the one this checkout would build.

A script that shows or records what `ht` says is only as current as the binary it
calls. The usual way to get that wrong: edit the Rust source, run the tests (which
build their own copy), and then run a script that calls the `ht` installed days ago.
The output looks fine and describes the old code.

    from htbin import require_ht
    ht = require_ht()            # `ht` on PATH
    ht = require_ht(a.ht)        # a script with its own --ht option

`require_ht` returns the binary to run. When the binary is stale it exits 2, with the
file that makes it stale and the command that rebuilds it; a script calls it before
it does anything else, so nothing is half done. `HT_STALE_OK=1` in the environment
turns that into a one-line warning and runs the old binary, for the times that is
what you want (comparing against the installed release, a source change that cannot
matter). `dev --stale-ok TASK …` sets it for one task.

Stale means: a build input is newer than the binary. Build inputs are the files git
knows under crates/ (tracked, or new and not ignored), without tests, benches,
examples, fuzz targets and Markdown, plus the workspace's Cargo.toml, Cargo.lock and
rust-toolchain.toml. That covers the data compiled into the binary (trust lists,
rubrics, the writer fingerprints), which all live under crates/.

This is make's rule, with make's limits. It compares times, not content:
- A file touched without a real change (a checkout, a rebase) reads as stale. That
  errs on the side of a rebuild.
- A binary newer than the source but built from something else (another checkout,
  another feature set) reads as current. A binary inside another checkout's target/
  directory is recognised and not checked at all: it was built from that source, and
  naming it is the explicit choice (scripts/versiondiff.py compares two such builds).

Standard library only. Runs `git ls-files`; reads no file's content.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

from repo import ROOT, C

OVERRIDE = "HT_STALE_OK"
WORKSPACE_FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")
NOT_INPUTS = {"tests", "benches", "examples", "fuzz"}  # directories that build no part of ht


@dataclass
class State:
    """What is known about the binary a script would run."""

    binary: Path | None  # None: not found
    asked: str = "ht"  # the name or path that was looked for
    built: float = 0.0  # the binary's modification time
    newer: tuple[Path, ...] = ()  # build inputs changed after that, newest first
    foreign: bool = False  # built in another checkout: not compared with this source

    @property
    def stale(self) -> bool:
        return bool(self.newer)


def find(explicit: str | None = None) -> Path | None:
    """The binary to run: the one named, else `ht` on PATH."""
    found = shutil.which(explicit or "ht")
    return Path(found) if found else None


def build_inputs() -> list[Path]:
    """Every file whose change means `ht` has to be rebuilt (see the module text)."""
    try:
        out = subprocess.run(
            ["git", "-C", str(ROOT), "ls-files", "-z", "--cached", "--others", "--exclude-standard",
             "--", "crates", *WORKSPACE_FILES],
            capture_output=True, text=True, check=True,
        ).stdout
        names = [n for n in out.split("\0") if n]
    except (OSError, subprocess.CalledProcessError):
        # No git (an unpacked archive): walk crates/ instead, without build output.
        names = [str(p.relative_to(ROOT)) for p in (ROOT / "crates").rglob("*")
                 if p.is_file() and "target" not in p.parts]
        names += WORKSPACE_FILES
    return [ROOT / n for n in names
            if not NOT_INPUTS & set(Path(n).parts[:-1]) and not n.endswith(".md")]


def state(explicit: str | None = None) -> State:
    asked = explicit or "ht"
    binary = find(explicit)
    if binary is None:
        return State(None, asked)
    real = binary.resolve()
    built = real.stat().st_mtime
    target = ROOT / "target"
    if "target" in real.parts and target not in real.parents:
        return State(binary, asked, built, foreign=True)
    changed = []
    for p in build_inputs():
        try:
            m = p.stat().st_mtime
        except OSError:  # tracked and deleted: the edit that removed its use is what counts
            continue
        if m > built:
            changed.append((m, p))
    return State(binary, asked, built, tuple(p for _, p in sorted(changed, reverse=True)))


def rebuild_command(binary: Path) -> str:
    """The command that replaces this binary with a current one."""
    real, target = binary.resolve(), ROOT / "target"
    if target in real.parents:
        release = " --release" if "release" in real.parts else ""
        return f"cargo build -p halftone-cli --features c2pa{release}"
    return "cargo install --path crates/halftone-cli --features c2pa --locked"


def _when(t: float) -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(t))


def _shown(p: Path) -> str:
    """A path as short as it can be said: inside the repo, under ~, or whole."""
    for base, prefix in ((ROOT, ""), (Path.home(), "~/")):
        try:
            return prefix + str(p.relative_to(base))
        except ValueError:
            continue
    return str(p)


def describe(s: State) -> list[str]:
    """The lines that say what was found, without colour: for `ht-check` and the error."""
    if s.binary is None:
        named = "/" in s.asked  # a path, not a name to look up
        where = Path(s.asked).absolute() if named else Path.home() / ".cargo/bin/ht"
        return [f"ht       `{s.asked}` not found" + ("" if named else " on PATH"),
                f"build    {rebuild_command(where)}"]
    lines = [f"ht       {_shown(s.binary)}, built {_when(s.built)}"]
    if s.foreign:
        lines.append("source   another checkout's build: not compared with this source")
    elif s.stale:
        first = s.newer[0]
        more = len(s.newer) - 1
        lines.append(f"changed  {_shown(first)}, {_when(first.stat().st_mtime)}"
                     + (f" (and {more} more file{'s' if more > 1 else ''})" if more else ""))
        lines.append(f"rebuild  {rebuild_command(s.binary)}")
    else:
        lines.append("source   nothing under crates/ is newer")
    return lines


def require_ht(explicit: str | None = None, *, optional: bool = False) -> str | None:
    """The path of a current `ht`, or exit 2 saying why there is none. With
    HT_STALE_OK=1 a stale binary is returned after a warning on stderr. `optional`
    is for a script that works without `ht` and only shows less: no binary is then
    `None`, not an error. A stale one still exits."""
    s = state(explicit)
    if s.binary is None and optional:
        return None
    if s.binary is not None and not s.stale:
        return str(s.binary)
    allowed = s.binary is not None and os.environ.get(OVERRIDE, "") not in ("", "0")
    if allowed:
        n = len(s.newer)
        print(f"{C.YELLOW}warning: running a stale ht{C.RESET} ({_shown(s.binary)}, built "
              f"{_when(s.built)}; {n} source file{'s' if n != 1 else ''} newer) because "
              f"{OVERRIDE} is set", file=sys.stderr)
        return str(s.binary)
    head = "ht is stale: the source changed after this binary was built" if s.binary is not None else \
        "ht is not built" if "/" in s.asked else "ht is not installed"
    print(f"{C.RED}error: {head}{C.RESET}", file=sys.stderr)
    for line in describe(s):
        print(f"  {line}", file=sys.stderr)
    if s.binary is not None:
        print(f"  {C.DIM}or run with the old binary, this once: dev --stale-ok <task> …  "
              f"(any command: {OVERRIDE}=1 <command>){C.RESET}", file=sys.stderr)
    sys.exit(2)
