#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Where a JPEG's MPF primary size went stale: the segments in front of the MPF index.

MPF places later images relative to its own header, so a segment added or removed
in front of the header after the index was written changes the primary's real
length without moving anything else, and the primary's declared size goes stale.
`mpf_index` records that gap (details.primary_size_found minus the declared size)
instead of counting it as a mismatch. This script lists, per file, the gap and
every segment before the MPF index, and marks a segment whose size equals the gap:
the likely late insertion (an ExifTool-added EXIF block, a signed manifest, …).

    uv run scripts/mpf-gap.py corpus/differential/06-phones/*.jpg
    uv run scripts/mpf-gap.py --ht target/release/ht corpus/differential/07-editors/*.jpg

A matching size is a lead, not proof: only the file as first written could show
what was inserted. Files without an MPF index, or whose index agrees, are skipped.
Needs `ht` (built with this change). Offline; reads only the files given.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

NAMES = {0xE0: "APP0", 0xE1: "APP1", 0xE2: "APP2", 0xEB: "APP11", 0xEC: "APP12",
         0xED: "APP13", 0xEE: "APP14", 0xFE: "COM", 0xDB: "DQT", 0xC0: "SOF0",
         0xC2: "SOF2", 0xC4: "DHT", 0xDD: "DRI"}


def mpf_details(ht: str, path: Path) -> dict | None:
    r = subprocess.run([ht, "inspect", "--json", "--only", "container", str(path)],
                       capture_output=True, text=True)
    try:
        ev = json.loads(r.stdout)["evidence"]
    except (json.JSONDecodeError, KeyError, TypeError):
        return None
    return next((e["details"] for e in ev if e["source"]["name"] == "mpf_index"), None)


def segments(b: bytes) -> list[tuple[int, int, int, str]]:
    """(offset, marker, total size incl. marker, identifier) up to SOS."""
    out, i = [], 2
    while i + 4 <= len(b) and b[i] == 0xFF and b[i + 1] not in (0xDA, 0xD9):
        m, n = b[i + 1], int.from_bytes(b[i + 2:i + 4], "big")
        ident = b[i + 4:i + 4 + 32].split(b"\0")[0].decode("latin-1")
        out.append((i, m, n + 2, ident if ident.isprintable() else ""))
        i += 2 + n
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("files", nargs="+", type=Path)
    ap.add_argument("--ht", default="ht", help="ht binary (default: ht on PATH).")
    sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
    from complete import serve  # tab completion (lib/complete.py); returns unless asked

    serve(ap)
    a = ap.parse_args()

    shown = 0
    for path in a.files:
        d = mpf_details(a.ht, path)
        if not d or not d.get("index") or d.get("primary_size_found") is None:
            continue
        declared = d["index"]["entries"][0]["size"]
        found = d["primary_size_found"]
        if found == declared:
            continue
        gap = found - declared
        segs = segments(path.read_bytes())
        mpf_at = next((o for o, m, _, t in segs if m == 0xE2 and t.startswith("MPF")), None)
        if mpf_at is None:
            continue
        shown += 1
        print(f"\n{path.name}\n  declared {declared:,}  found {found:,}  gap {gap:+,}")
        for o, m, size, ident in segs:
            if o > mpf_at:
                break
            hit = "   <- size equals the gap" if size == abs(gap) else ""
            print(f"  {o:>9,}  {NAMES.get(m, hex(m)):<6} {size:>8,}  {ident[:40]}{hit}")
    if not shown:
        print("no file with a stale MPF primary size", file=sys.stderr)


if __name__ == "__main__":
    main()
