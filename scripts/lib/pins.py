"""The reference-tool pins (docker/references.env) and how a tool's version is read,
shared by references.py, which manages the pins and the image, and collect.py, which
refuses to write a header with tools that differ from them.

One way of reading each version, on the host and in the image, so "the same version"
means the same string everywhere. Standard library only.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
from pathlib import Path

from repo import ROOT

PINS = ROOT / "docker" / "references.env"

# Pin -> what a mismatch is called in a message. Only pins a collection header records:
# uv and the build-only pins change how things are built or run, not what is collected.
RECORDED = {
    "EXIFTOOL_VERSION": "ExifTool",
    "OPENSSL_VERSION": "OpenSSL",
    "C2PATOOL_VERSION": "c2patool",
    "CREDENTIO_COMMIT": "Credentio",
    "CONFORMANCE_COMMIT": "conformance-public (rubrics)",
}


def output(cmd: list[str], cwd: Path | None = None) -> str | None:
    if shutil.which(cmd[0]) is None:
        return None
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    return (r.stdout or r.stderr).strip() if r.returncode == 0 else None


def first(pattern: str, text: str | None) -> str | None:
    m = re.search(pattern, text or "", re.M)
    return m.group(1) if m else None


def credentio_name(path: Path) -> str | None:
    """The commit in `c2pa_validate-<commit>`, the name the symlink points to."""
    real = path.expanduser().resolve()
    if not real.is_file():
        return None
    return real.name.removeprefix("c2pa_validate-") if real.name.startswith("c2pa_validate-") else None


def host_versions(credentio: Path | None = None, rubrics: Path | None = None) -> dict[str, str | None]:
    """What this machine (or the image, when run inside it) has, keyed like the pins.
    Credentio and the rubrics checkout default to $CREDENTIO and $RUBRICS."""
    cred = credentio or Path(os.environ.get("CREDENTIO", "~/oss-bins/c2pa_validate"))
    rub = (ROOT / (rubrics or Path(os.environ.get("RUBRICS", "../conformance-public"))).expanduser()).resolve()
    return {
        "UV_VERSION": first(r"^uv (\S+)", output(["uv", "--version"])),
        "EXIFTOOL_VERSION": (output(["exiftool", "-ver"]) or "").strip() or None,
        "OPENSSL_VERSION": first(r"^OpenSSL (\S+)", output(["openssl", "version"])),
        "C2PATOOL_VERSION": first(r"(\d+\.\d+\.\d+\S*)", output(["c2patool", "--version"])),
        "CREDENTIO_COMMIT": credentio_name(cred),
        "CONFORMANCE_COMMIT": output(["git", "-C", str(rub), "rev-parse", "HEAD"]) if rub.is_dir() else None,
    }


def same(pin: str, got: str | None) -> bool:
    """Commits match on a prefix (a pin may be short or full), versions exactly."""
    if not got:
        return False
    if re.fullmatch(r"[0-9a-f]{7,40}", pin):
        return got.startswith(pin) or pin.startswith(got)
    return got == pin


def pin_values() -> dict[str, str]:
    """KEY -> value from docker/references.env; empty when there is no such file.
    Tolerant on purpose: references.py owns the strict parse and its messages."""
    if not PINS.is_file():
        return {}
    out: dict[str, str] = {}
    for line in PINS.read_text().splitlines():
        key, sep, value = line.strip().partition("=")
        if sep and key and not key.startswith("#") and value.strip():
            out[key.strip()] = value.strip()
    return out


def drift(keys: list[str], credentio: Path | None = None, rubrics: Path | None = None) -> list[tuple[str, str, str]]:
    """(tool, pinned, here) for each of `keys` where this machine differs from its pin.
    Empty when everything matches or there are no pins to compare with."""
    pins = pin_values()
    if not pins:
        return []
    here = host_versions(credentio, rubrics)
    return [(RECORDED.get(k, k), pins[k], here.get(k) or "not found")
            for k in keys if k in pins and not same(pins[k], here.get(k))]
