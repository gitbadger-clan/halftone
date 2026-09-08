#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.15"
# dependencies = []
# ///
"""Collect ground truth for the differential test from ExifTool and c2patool.

Walks a corpus directory and writes ``expectations.json`` next to it, recording for
every image file what the two reference tools report for the fields Halftone's
``marking_metadata`` and ``c2pa`` sources emit. The Rust test in
``crates/halftone-cli/tests/differential.rs`` runs ``ht inspect --batch --json`` on
the same files and asserts field-by-field agreement.

This script is a collector, not a dependency: Halftone never imports it, and the
expectations file is committed (or regenerated) so the Rust side needs neither tool.

Usage (uv resolves the interpreter from the inline metadata; no venv needed):
    uv run scripts/differential.py corpus/differential
    uv run scripts/differential.py corpus/differential --trust-anchors crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem
    uv run scripts/differential.py corpus/differential --out /tmp/expectations.json

Network: c2patool is run with ``verify.remote_manifest_fetch = false`` and
``verify.ocsp_fetch = false`` (a temporary settings file), matching Halftone, so the
expectations do not depend on the network or on the day they were collected. A file
that only references a remote manifest is recorded as ``remote_manifest: <url>``.

Trust: c2patool reports ``Trusted`` only when given the same anchors Halftone uses.
Pass ``--trust-anchors`` for an exact ``validation_state`` comparison; without it the
file records ``trust_anchors: null`` and the Rust test accepts ``Trusted`` where
c2patool said ``Valid``.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

C2PATOOL_SETTINGS = "[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\n"

# Extensions worth sniffing. The decision is made on ExifTool's MIME type, never on
# the extension: fixture corpora contain mislabeled files on purpose.
IMAGE_EXTS = {".jpg", ".jpeg", ".jpe", ".png", ".webp", ".heic", ".heif", ".avif"}
# What `ht` can load today; anything else is skipped, not recorded, because one
# unloadable file fails the whole `--batch` run.
HALFTONE_MIMES = {"image/jpeg", "image/png", "image/webp", "image/heic", "image/heif", "image/avif"}


def code_of(raw: str) -> str:
    raw = raw.strip().rstrip("/")
    return raw.rsplit("/", 1)[-1] if "/" in raw else raw


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def run(cmd: list[str], env: dict | None = None) -> tuple[int, str, str]:
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env)
    return proc.returncode, proc.stdout, proc.stderr


def tool_version(cmd: list[str]) -> str | None:
    try:
        rc, out, _ = run(cmd)
    except FileNotFoundError:
        return None
    return out.strip().splitlines()[0] if rc == 0 and out.strip() else None


def exiftool_facts(p: Path) -> dict:
    """XMP presence, XMP DigitalSourceType values, IIM program fields, MIME."""
    rc, out, err = run([
        "exiftool", "-j", "-G1", "-a", "-struct", "-n",
        "-XMP:all", "-IPTC:OriginatingProgram", "-IPTC:ProgramVersion",
        "-File:MIMEType", str(p),
    ])
    if rc != 0 or not out.strip():
        return {"error": err.strip() or f"exiftool exit {rc}"}
    d = json.loads(out)
    d = d[0] if isinstance(d, list) and d else {}
    xmp_keys = [k for k in d if k.startswith("XMP")]
    dst_raw: list[str] = []
    for k in xmp_keys:
        if k.split(":", 1)[-1] == "DigitalSourceType":
            v = d[k]
            dst_raw.extend(v if isinstance(v, list) else [v])
    codes: list[str] = []
    for v in dst_raw:
        c = code_of(str(v))
        if c not in codes:
            codes.append(c)
    return {
        "mime": d.get("File:MIMEType"),
        "xmp_present": bool(xmp_keys),
        "xmp_tag_count": len(xmp_keys),
        "digital_source_type_raw": [str(v) for v in dst_raw],
        "digital_source_type": codes,
        "iim_originating_program": d.get("IPTC:OriginatingProgram"),
        "iim_program_version": (
            str(d["IPTC:ProgramVersion"]) if "IPTC:ProgramVersion" in d else None
        ),
    }


def walk_dst(v, out: list[str]) -> None:
    if isinstance(v, dict):
        for k, child in v.items():
            if (k == "digitalSourceType" or k.endswith(":DigitalSourceType")) and isinstance(child, str):
                out.append(child)
            else:
                walk_dst(child, out)
    elif isinstance(v, list):
        for child in v:
            walk_dst(child, out)


def c2patool_facts(p: Path, anchors: Path | None, settings: Path) -> dict:
    env = dict(os.environ)
    if anchors:
        env["C2PATOOL_TRUST_ANCHORS"] = str(anchors)
    try:
        rc, out, err = run(["c2patool", "--settings", str(settings), str(p)], env=env)
    except FileNotFoundError:
        return {"error": "c2patool not found"}
    text = (out + "\n" + err).lower()
    if "no claim found" in text or "no manifest" in text or "jumbfnotfound" in text:
        return {"present": False}
    m = re.search(r"must fetch remote manifests from url (\S+)", out + "\n" + err)
    if m:
        return {"present": None, "remote_manifest": m.group(1)}
    if rc != 0 and not out.strip().startswith("{"):
        return {"present": None, "error": (err or out).strip()[:400]}
    try:
        js = json.loads(out)
    except json.JSONDecodeError:
        return {"present": None, "error": "unparseable c2patool output"}
    label = js.get("active_manifest")
    active = js.get("manifests", {}).get(label, {}) if label else {}
    gen = active.get("claim_generator")
    if not gen:
        info = active.get("claim_generator_info") or []
        if info and isinstance(info[0], dict) and info[0].get("name"):
            gen = info[0]["name"]
            if info[0].get("version"):
                gen = f"{gen} {info[0]['version']}"
    raw: list[str] = []
    walk_dst(active.get("assertions", []), raw)
    codes: list[str] = []
    for v in raw:
        c = code_of(v)
        if c not in codes:
            codes.append(c)
    return {
        "present": True,
        "validation_state": js.get("validation_state"),
        "validation_codes": [s.get("code") for s in js.get("validation_status") or []],
        "issuer": (active.get("signature_info") or {}).get("issuer"),
        "claim_generator": gen,
        "claim_version": active.get("claim_version"),
        "assertions": [a.get("label") for a in active.get("assertions", [])],
        "digital_source_type_raw": raw,
        "digital_source_type": codes,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus", type=Path)
    ap.add_argument("--out", type=Path, help="default: <corpus>/expectations.json")
    ap.add_argument("--trust-anchors", type=Path, help="PEM bundle handed to c2patool")
    ap.add_argument("--no-c2pa", action="store_true", help="skip c2patool entirely")
    a = ap.parse_args()

    corpus: Path = a.corpus
    if not corpus.is_dir():
        print(f"not a directory: {corpus}", file=sys.stderr)
        return 2
    out_path = a.out or corpus / "expectations.json"
    exif_ver = tool_version(["exiftool", "-ver"])
    if not exif_ver:
        print("exiftool not found on PATH", file=sys.stderr)
        return 2
    c2pa_ver = None if a.no_c2pa else tool_version(["c2patool", "--version"])
    if not a.no_c2pa and not c2pa_ver:
        print("c2patool not found on PATH (pass --no-c2pa to skip manifest ground truth)", file=sys.stderr)
        return 2

    settings_file = Path(tempfile.mkstemp(suffix=".toml", prefix="c2patool-")[1])
    settings_file.write_text(C2PATOOL_SETTINGS)

    files: dict[str, dict] = {}
    paths = sorted(p for p in corpus.rglob("*") if p.is_file() and p.suffix.lower() in IMAGE_EXTS)
    skipped: list[str] = []
    for p in paths:
        rel = p.relative_to(corpus).as_posix()
        ex = exiftool_facts(p)
        if ex.get("mime") not in HALFTONE_MIMES:
            skipped.append(f"{rel} ({ex.get('mime') or ex.get('error') or 'unknown'})")
            continue
        entry = {
            "sha256": sha256(p),
            "size_bytes": p.stat().st_size,
            "exiftool": ex,
        }
        if not a.no_c2pa:
            entry["c2patool"] = c2patool_facts(p, a.trust_anchors, settings_file)
        files[rel] = entry
        dst = ex.get("digital_source_type") or []
        ct = entry.get("c2patool") or {}
        state = ct.get("validation_state") or ("remote" if ct.get("remote_manifest") else "-")
        print(f"{rel:60} xmp={'y' if ex.get('xmp_present') else 'n'} dst={','.join(dst) or '-':40} c2pa={state}")

    doc = {
        "schema": "halftone-differential-expectations/1",
        "corpus": str(corpus),
        "tools": {"exiftool": exif_ver, "c2patool": c2pa_ver},
        "trust_anchors": str(a.trust_anchors) if a.trust_anchors else None,
        "c2patool_settings": C2PATOOL_SETTINGS,
        "files": files,
    }
    settings_file.unlink(missing_ok=True)
    out_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
    print(f"\n{len(files)} files -> {out_path}")
    if skipped:
        print(f"{len(skipped)} skipped (not a format ht loads):")
        for s_ in skipped:
            print(f"  {s_}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
