#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Which transfer routes deliver a phone capture byte for byte, and what the others change.

Pull the same captures off the phone by every route you want to compare, name each
copy with the 06-phones scheme `<device>__<variant>__<transfer>__<subject>__<n>.<ext>`
(the transfer token is the route: afc-pull, image-capture, airdrop, photos-original,
adb-pull, …), put them in one folder outside the corpus, and run:

    uv run scripts/transfer-routes.py ~/halftone-candidates/iphone-routes
    uv run scripts/transfer-routes.py --no-ht ~/halftone-candidates/iphone-routes

Per capture (every name token but the transfer and the extension), one row per
route: bytes, sha256, format from the magic bytes, ExifTool's MPF image count and
container-directory items, and `ht`'s mpf_index and container_directory verdicts.
Routes with identical bytes share a class letter; a route whose bytes differ is a
finding for SOURCE.md, not an error. The summary lists, per route pair, in how many
captures they agreed.

Copies of one capture must agree on EXIF DateTimeOriginal + SubSecTimeOriginal; if
they do not, the files are mislabelled (two different shots under one name) and
the script exits 1. Exit 0 otherwise; 2 on a usage or setup error.

Needs exiftool on PATH; `ht` too unless --no-ht. Offline. Reads only the folder it
is given; writes nothing. Never reads the clipboard.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from itertools import combinations
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from exiftool import ExifTool  # noqa: E402
from repo import C, die, need  # noqa: E402

NAME = re.compile(
    r"^(?P<device>[a-z0-9.-]+)__(?P<variant>[a-z0-9.-]+)__(?P<transfer>[a-z0-9.-]+)"
    r"__(?P<subject>s\d\d)__(?P<n>\d+)\.(?P<ext>[A-Za-z0-9]+)$"
)
EXIF_TAGS = (
    "-FileType", "-MPF:NumberOfImages", "-XMP-GContainer:DirectoryItemSemantic",
    "-EXIF:DateTimeOriginal", "-EXIF:SubSecTimeOriginal", "-EXIF:Make", "-EXIF:Model",
)


@dataclass
class Copy:
    path: Path
    transfer: str
    size: int
    sha256: str
    magic: str
    exif: dict = field(default_factory=dict)
    verdicts: dict[str, str] = field(default_factory=dict)

    @property
    def taken(self) -> str | None:
        t = self.exif.get("DateTimeOriginal")
        if not t:
            return None
        sub = self.exif.get("SubSecTimeOriginal")
        return f"{t}.{sub}" if sub not in (None, "") else str(t)


def magic(b: bytes) -> str:
    if b.startswith(b"\xff\xd8\xff"):
        return "JPEG"
    if b.startswith(b"\x89PNG\r\n\x1a\n"):
        return "PNG"
    if len(b) >= 12 and b[4:8] == b"ftyp":
        brand = b[8:12].decode("ascii", "replace")
        return {"qt  ": "MOV"}.get(brand, f"BMFF/{brand.strip()}")
    return "?"


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def ht_verdicts(ht: str, p: Path) -> dict[str, str]:
    """mpf_index and container_directory status from `ht inspect`, or '?' each."""
    want = ("mpf_index", "container_directory")
    r = subprocess.run([ht, "inspect", "--json", "--only", "container", str(p)],
                       capture_output=True, text=True)
    try:
        ev = json.loads(r.stdout)["evidence"]
    except (json.JSONDecodeError, KeyError, TypeError):
        return dict.fromkeys(want, "?")
    got = {e["source"]["name"]: e["status"] for e in ev if e.get("source", {}).get("name") in want}
    return {w: got.get(w, "-") for w in want}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("folder", type=Path, help="Folder with the route copies (not a corpus stratum).")
    ap.add_argument("--ht", default="ht", help="ht binary (default: ht on PATH).")
    ap.add_argument("--no-ht", action="store_true", help="Skip the ht verdict columns.")
    from complete import serve  # tab completion (lib/complete.py); returns unless asked

    serve(ap)
    a = ap.parse_args()

    folder = a.folder.expanduser()
    if not folder.is_dir():
        die(f"not a folder: {folder}", 2)
    need("exiftool")
    if not a.no_ht and shutil.which(a.ht) is None:
        die(f"{a.ht} not found (pass --no-ht to skip its columns)", 2)

    groups: dict[tuple, list[Copy]] = defaultdict(list)
    stray: list[str] = []
    files = sorted(p for p in folder.iterdir() if p.is_file() and not p.name.startswith("."))
    for p in files:
        m = NAME.match(p.name)
        if not m:
            stray.append(p.name)
            continue
        with p.open("rb") as f:
            head = f.read(16)
        key = (m["device"], m["variant"], m["subject"], int(m["n"]))
        groups[key].append(Copy(p, m["transfer"], p.stat().st_size, sha256(p), magic(head)))
    if not groups:
        die(f"no file in {folder} follows <device>__<variant>__<transfer>__<subject>__<n>.<ext>", 2)

    every = [c for cs in groups.values() for c in cs]
    with ExifTool() as et:
        rows = et.json(*EXIF_TAGS, *(str(c.path) for c in every))
    by_path = {str(Path(r.get("SourceFile", ""))): r for r in rows}
    for c in every:
        c.exif = by_path.get(str(c.path), {})
        if not a.no_ht:
            c.verdicts = ht_verdicts(a.ht, c.path)

    mislabelled = 0
    agree: dict[tuple[str, str], list[int]] = defaultdict(lambda: [0, 0])  # [same, compared]
    for key in sorted(groups):
        copies = sorted(groups[key], key=lambda c: c.transfer)
        device, variant, subject, n = key
        times = {c.taken for c in copies}
        when = next(iter(times)) if len(times) == 1 else None
        print(f"\n{C.BOLD_CYAN}== {device}__{variant}__{subject}__{n}{C.RESET}  "
              f"{C.DIM}{when or 'capture times differ'}{C.RESET}")
        if len(times) > 1:
            mislabelled += 1
            for c in copies:
                print(f"  {C.RED}{c.transfer}: taken {c.taken}{C.RESET}")

        classes: dict[str, str] = {}
        width = max(len(c.transfer) for c in copies)
        head = f"  {'route':<{width}}  cls  {'bytes':>11}  {'sha256':<12}  {'type':<10} mpf  directory"
        if not a.no_ht:
            head += "        mpf_index     container_directory"
        print(C.DIM + head + C.RESET)
        for c in copies:
            cls = classes.setdefault(c.sha256, chr(ord("A") + len(classes)))
            mpf = c.exif.get("NumberOfImages", "-")
            items = c.exif.get("DirectoryItemSemantic", "-")
            if isinstance(items, list):
                items = ",".join(map(str, items))
            line = (f"  {c.transfer:<{width}}  {cls:^3}  {c.size:>11,}  {c.sha256[:12]}  "
                    f"{c.magic:<10} {str(mpf):>3}  {str(items):<16}")
            if not a.no_ht:
                line += f" {c.verdicts['mpf_index']:<13} {c.verdicts['container_directory']}"
            tone = C.GREEN if cls == "A" else C.YELLOW
            print(tone + line + C.RESET)
        for x, y in combinations(copies, 2):
            pair = tuple(sorted((x.transfer, y.transfer)))
            agree[pair][1] += 1
            agree[pair][0] += x.sha256 == y.sha256

    print(f"\n{C.BOLD_CYAN}== route pairs{C.RESET}  {C.DIM}captures with identical bytes / captures compared{C.RESET}")
    if not agree:
        print("  (every capture came through one route only; nothing to compare)")
    for (x, y), (same, total) in sorted(agree.items()):
        tone = C.GREEN if same == total else C.RED if same == 0 else C.YELLOW
        print(f"  {tone}{x} = {y}: {same}/{total}{C.RESET}")
    if stray:
        print(f"\n{C.DIM}ignored (name does not parse): {', '.join(stray)}{C.RESET}")
    if mislabelled:
        print(f"\n{C.RED}{mislabelled} capture(s) whose copies disagree on capture time: "
              f"names are wrong, fix them before reading the table{C.RESET}")
    sys.exit(1 if mislabelled else 0)


if __name__ == "__main__":
    main()
