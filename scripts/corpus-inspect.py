#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Contact sheet for the rows the corpus test reports, to look at them by eye.

Reads the problem table the corpus test prints in report mode, and writes an HTML
page with, for every row: the file, the file it was compared against, and a
difference image of the two (red = pixels that differ beyond a small fuzz). Rows
with no counterpart (extension, name, decode) show the file alone with its real
format. Nothing is copied: the page points at the corpus files where they are.

    env HALFTONE_CORPUS_REPORT=1 cargo test --release -p halftone-cli --test corpus \
        -- --ignored --nocapture | tee /tmp/corpus-report.txt
    uv run scripts/corpus-inspect.py /tmp/corpus-report.txt
    open /tmp/corpus-inspect/index.html

Needs ImageMagick 7 (`magick`) on PATH for the difference images.
"""
from __future__ import annotations

import argparse
import html
import re
import subprocess
import sys
from pathlib import Path

ROW = re.compile(r"^\| (?P<file>[^|]+?) \| (?P<check>[^|]+?) \| (?P<detail>.+) \|$")
NAME = re.compile(r"[a-z0-9.-]+__[a-z0-9._-]+\.(?:png|jpe?g|webp|heic|heif|avif)")
SIDE = 512


def run(cmd: list[str]) -> str:
    p = subprocess.run(cmd, capture_output=True, text=True)
    return (p.stdout or p.stderr).strip()


def identify(p: Path) -> str:
    """Real format and size, read from the bytes."""
    return run(["magick", "identify", "-format", "%m %wx%h", f"{p}[0]"]) + f", {p.stat().st_size:,} B"


def diff(a: Path, b: Path, out: Path) -> str:
    """Both scaled to SIDE×SIDE (aspect ignored, so a crop shows as a shift), then
    compared with a 3% fuzz. Returns the share of differing pixels as text."""
    out.parent.mkdir(parents=True, exist_ok=True)
    sa, sb = out.with_suffix(".a.png"), out.with_suffix(".b.png")
    for src, dst in ((a, sa), (b, sb)):
        run(["magick", f"{src}[0]", "-auto-orient", "-resize", f"{SIDE}x{SIDE}!", str(dst)])
    metric = run(["magick", "compare", "-metric", "AE", "-fuzz", "3%",
                  "-highlight-color", "red", "-lowlight-color", "white",
                  str(sa), str(sb), str(out)])
    sa.unlink(missing_ok=True)
    sb.unlink(missing_ok=True)
    try:
        share = float(metric.split()[0]) / (SIDE * SIDE)
        return f"{share:.1%} of pixels differ (3% fuzz, {SIDE}² thumbnails)"
    except (ValueError, IndexError):
        return f"compare: {metric[:120]}"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("report", type=Path, help="captured output of the corpus test in report mode")
    ap.add_argument("--corpus", type=Path, default=Path("corpus/differential/04-generators"))
    ap.add_argument("--out", type=Path, default=Path("/tmp/corpus-inspect"))
    a = ap.parse_args()

    corpus = a.corpus.resolve()
    rows = []
    for line in a.report.read_text().splitlines():
        m = ROW.match(line.strip())
        if not m or m["file"] in ("file", "---"):
            continue
        rows.append((m["file"], m["check"], m["detail"]))
    if not rows:
        print("no problem rows in the report (run the test with HALFTONE_CORPUS_REPORT=1)", file=sys.stderr)
        return 1

    a.out.mkdir(parents=True, exist_ok=True)
    parts = []
    for i, (file, check, detail) in enumerate(rows):
        f = corpus / file
        others = [n for n in NAME.findall(detail) if n != file]
        other = corpus / others[0] if others else None
        cells = []
        for p in (f, other):
            if p is None:
                continue
            if p.exists():
                cells.append(
                    f'<figure><img src="{p.as_uri()}"><figcaption>{html.escape(p.name)}<br>'
                    f"<small>{html.escape(identify(p))}</small></figcaption></figure>"
                )
            else:
                cells.append(f"<figure><figcaption>{html.escape(p.name)}<br><b>not on disk</b></figcaption></figure>")
        if other is not None and f.exists() and other.exists():
            d = a.out / f"diff-{i:03d}.png"
            note = diff(other, f, d)
            cells.append(f'<figure><img src="{d.name}"><figcaption>difference<br><small>{html.escape(note)}</small></figcaption></figure>')
        parts.append(
            f'<section><h2>{i + 1}. <code>{html.escape(check)}</code> {html.escape(file)}</h2>'
            f"<p>{html.escape(detail)}</p><div class=row>{''.join(cells)}</div></section>"
        )
        print(f"{i + 1:3}/{len(rows)} {check:12} {file}")

    page = (
        "<!doctype html><meta charset=utf-8><title>corpus inspection</title><style>"
        "body{font:14px system-ui;margin:24px;background:#fafafa;color:#111}"
        "section{background:#fff;border:1px solid #ddd;border-radius:8px;padding:12px 16px;margin:0 0 20px}"
        "h2{font-size:15px;margin:0 0 4px}p{margin:0 0 10px;color:#555}"
        ".row{display:flex;gap:12px;overflow-x:auto}"
        "figure{margin:0;flex:0 0 auto;width:360px}img{width:360px;height:auto;border:1px solid #ccc;background:#eee}"
        "figcaption{font-size:12px;word-break:break-all}"
        "@media (prefers-color-scheme:dark){body{background:#151515;color:#eee}"
        "section{background:#1f1f1f;border-color:#333}p{color:#aaa}}"
        f"</style><h1>{len(rows)} corpus rows to inspect</h1>{''.join(parts)}"
    )
    index = a.out / "index.html"
    index.write_text(page)
    print(f"\n{index}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
