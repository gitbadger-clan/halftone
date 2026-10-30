"""c2patool for scripts, isolated the way the differential collector isolates it
(DIFFERENTIAL.md D-008): $C2PATOOL_SETTINGS and $C2PATOOL_TRUST_ANCHORS removed, an
empty $XDG_CONFIG_HOME, and a settings file built by differential.py's own
c2patool_settings(), so a c2pa.toml on this machine can never change what a frame
or a check prints, and no script can drift from the ground truth.

    from c2p import run, json_of
    json_of(f)                          # network off, no anchors (what the frames used)
    json_of(f, trust="vendored")        # anchors + TSA + EKU policy, as the ground truth
    json_of(f, trust="eku")             # EKU policy only (collector --no-trust)
    run(f, "--certs", fetch=True)       # remote manifest fetch on; OCSP stays off

Settings files are written once per mode and removed when the process exits.
"""

from __future__ import annotations

import atexit
import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
from functools import cache
from pathlib import Path
from typing import Literal

from repo import ROOT, TRUST_DIR

Trust = Literal["none", "eku", "vendored"]


@cache
def _collector():
    path = ROOT / "scripts" / "differential.py"
    spec = importlib.util.spec_from_file_location("differential", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


@cache
def _workdir() -> Path:
    d = Path(tempfile.mkdtemp(prefix="c2p-"))
    atexit.register(shutil.rmtree, d, ignore_errors=True)
    (d / "xdg").mkdir()
    return d


def settings_text(trust: Trust = "none", fetch: bool = False) -> str:
    eku = TRUST_DIR / "C2PA-EKU-CONFIG.cfg"
    anchors = TRUST_DIR / "C2PA-TRUST-LIST.pem"
    tsa = TRUST_DIR / "C2PA-TSA-TRUST-LIST.pem"
    need = {"none": [], "eku": [eku], "vendored": [eku, anchors, tsa]}[trust]
    missing = [str(p) for p in need if not p.is_file()]
    if missing:
        raise FileNotFoundError(f"missing: {', '.join(missing)}")
    d = _collector()
    text = {
        "none": lambda: d.c2patool_settings(None),
        "eku": lambda: d.c2patool_settings(None, None, eku),
        "vendored": lambda: d.c2patool_settings(anchors, tsa, eku),
    }[trust]()
    if fetch:
        off = "remote_manifest_fetch = false\n"
        if text.count(off) != 1:
            raise RuntimeError("collector settings changed shape: no single remote_manifest_fetch line")
        text = text.replace(off, "remote_manifest_fetch = true\n")
    return text


@cache
def settings_path(trust: Trust = "none", fetch: bool = False) -> Path:
    p = _workdir() / f"{trust}{'-fetch' if fetch else ''}.toml"
    p.write_text(settings_text(trust, fetch))
    return p


def env() -> dict[str, str]:
    e = {k: v for k, v in os.environ.items() if k not in ("C2PATOOL_SETTINGS", "C2PATOOL_TRUST_ANCHORS")}
    e["XDG_CONFIG_HOME"] = str(_workdir() / "xdg")
    return e


def run(file: str | Path, *args: str, trust: Trust = "none", fetch: bool = False,
        exe: str = "c2patool") -> subprocess.CompletedProcess[str]:
    """c2patool --settings <mode> FILE ARGS…; never raises on a non-zero exit."""
    return subprocess.run([exe, "--settings", str(settings_path(trust, fetch)), str(file), *args],
                          capture_output=True, text=True, env=env())


def json_of(file: str | Path, *args: str, trust: Trust = "none", fetch: bool = False) -> dict | None:
    """c2patool's JSON report, or None when it printed none (no manifest, unreadable)."""
    out = run(file, *args, trust=trust, fetch=fetch).stdout
    try:
        j = json.loads(out)
    except json.JSONDecodeError:
        return None
    return j if isinstance(j, dict) else None
