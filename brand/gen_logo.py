#!/usr/bin/env python3
"""Halftone logo generator. Source of truth for every brand asset.

    python3 brand/gen_logo.py            regenerate brand/ (this directory)
    python3 brand/gen_logo.py --embed    also refresh crates/halftone-report/assets/
    python3 brand/gen_logo.py --check    CI: fail if brand/ or the embedded copy drifted
    python3 brand/gen_logo.py --out DIR  write somewhere else (no --embed)

Deterministic. PNG/ICO need cairosvg + Pillow; without them only the vector
files are written, and --check compares vector/text files only."""

from __future__ import annotations

import argparse
import filecmp
import math
import shutil
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
EMBED_DIR = REPO / "crates" / "halftone-report" / "assets"

INK_LIGHT = "#1B3A5C"   # cyanotype blue, on light backgrounds
INK_DARK = "#9CC0E8"    # same pigment on dark backgrounds (pulled toward cyan, not just lightened)
MONO_LIGHT = "#000000"  # plain-ink variants for print, single-colour and CLI contexts
MONO_DARK = "#F4F4F2"
NA_GREY = "#878783"     # NotApplicable glyph; same value both modes (>= 3:1 on white)

# Wordmark type stack. Replace with the mono you pick for the CLI and outline it.
MONO = "'JetBrains Mono', 'IBM Plex Mono', ui-monospace, Menlo, Consolas, monospace"

# Master mark: 128 box, r=60, five columns at pitch 12, clipped edge so the
# silhouette stays a true circle.
MASTER = dict(size=128, r=60, pitch=12, radii=[5.5, 4.8, 4.0, 3.1, 2.1], clip=True)

# Hand-tuned favicon geometry: fewer columns, no cut dots.
SMALL = {
    48: dict(size=48, r=22.5, pitch=6, radii=[2.7, 2.1, 1.5, 0.9], clip=False),
    32: dict(size=32, r=15, pitch=5, radii=[2.2, 1.6, 1.0], clip=False),
    16: dict(size=16, r=7.5, pitch=3.5, radii=[1.4, 0.9], clip=False),
}

STATUSES = ("present", "absent", "inconclusive", "not_applicable")

# Files the report crate embeds with include_str!. Relative to the output dir.
EMBED = ["halftone-mark.svg", "halftone-mark-light.svg", "halftone-mark-dark.svg"] + [
    f"verdict/{s}{v}.svg" for s in STATUSES for v in ("", "-light", "-dark")
]

TEXT_SUFFIXES = {".svg", ".md", ".txt"}


# ---------------------------------------------------------------- geometry

def dot_columns(size, r, pitch, radii, clip):
    """Dots for the dissolving right half. Column k sits at cx + pitch*(k+0.5).
    With clip=False, drop any dot the circle would cut (favicon sizes)."""
    c = size / 2
    dots = []
    for k, rad in enumerate(radii):
        x = c + pitch * (k + 0.5)
        n = int(c // pitch)
        for j in range(-n, n + 1):
            y = c + pitch * j
            d = math.hypot(x - c, y - c)
            if (d - rad < r) if clip else (d + rad <= r + 0.25):
                dots.append((x, y, rad))
    return dots


def _f(v, prec=2):
    return f"{v:.{prec}f}".rstrip("0").rstrip(".")


def mark_svg(size, r, pitch, radii, clip, ink):
    c = size / 2
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" '
           f'width="{size}" height="{size}" role="img" aria-label="Halftone">']
    if clip:
        out.append(f'<clipPath id="c"><circle cx="{_f(c)}" cy="{_f(c)}" r="{_f(r)}"/></clipPath>')
    out.append(f'<g fill="{ink}"' + (' clip-path="url(#c)"' if clip else "") + ">")
    out.append(f'<path d="M{_f(c)} {_f(c - r)}A{_f(r)} {_f(r)} 0 0 0 {_f(c)} {_f(c + r)}Z"/>')
    for x, y, rad in dot_columns(size, r, pitch, radii, clip):
        out.append(f'<circle cx="{_f(x)}" cy="{_f(y)}" r="{_f(rad)}"/>')
    out.append("</g></svg>")
    return "\n".join(out) + "\n"


def lockup_svg(ink):
    """Mark + wordmark. Text left live; outline it once the face is chosen."""
    inner = mark_svg(**MASTER, ink=ink).split(">", 1)[1].rsplit("</svg>", 1)[0]
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 468 128" width="468" height="128" '
        'role="img" aria-label="Halftone">\n'
        f'{inner}\n'
        f'<text x="156" y="87" fill="{ink}" font-family="{MONO}" font-size="60" '
        'font-weight="500" letter-spacing="1.5">halftone</text>\n'
        '</svg>\n'
    )


def glyph_svg(status, ink):
    """Verdict glyphs derived from the mark: solid / ring / halftone / dash. 24 box."""
    body = {
        "present": f'<circle cx="12" cy="12" r="10" fill="{ink}"/>',
        "absent": f'<circle cx="12" cy="12" r="9" fill="none" stroke="{ink}" stroke-width="2"/>',
        "inconclusive": (f'<g fill="{ink}">'
                         '<circle cx="12" cy="12" r="2.4"/><circle cx="6" cy="12" r="2.4"/>'
                         '<circle cx="18" cy="12" r="2.4"/><circle cx="12" cy="6" r="2.4"/>'
                         '<circle cx="12" cy="18" r="2.4"/><circle cx="6" cy="6" r="1.5"/>'
                         '<circle cx="18" cy="6" r="1.5"/><circle cx="6" cy="18" r="1.5"/>'
                         '<circle cx="18" cy="18" r="1.5"/></g>'),
        "not_applicable": (f'<line x1="5" y1="12" x2="19" y2="12" stroke="{NA_GREY}" '
                           'stroke-width="3" stroke-linecap="round"/>'),
    }[status]
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" '
            f'role="img" aria-label="{status}">\n{body}\n</svg>\n')


def contrast(fg, bg):
    def lum(h):
        c = [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        c = [v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4 for v in c]
        return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    a, b = lum(fg), lum(bg)
    return (max(a, b) + 0.05) / (min(a, b) + 0.05)


def readme():
    rows = [("ink-light on white", INK_LIGHT, "#FFFFFF"), ("ink-light on paper", INK_LIGHT, "#FAFAF7"),
            ("ink-dark on #1B1B1D", INK_DARK, "#1B1B1D"), ("ink-dark on black", INK_DARK, "#000000"),
            ("na-grey on white", NA_GREY, "#FFFFFF"), ("na-grey on #1B1B1D", NA_GREY, "#1B1B1D")]
    return (
        "# Halftone brand\n\n"
        "Generated by `gen_logo.py`; edit the script, not these files. "
        "`--embed` refreshes `crates/halftone-report/assets/`; CI runs `--check`.\n\n"
        "| Token | Value | Use |\n|---|---|---|\n"
        f"| ink-light | `{INK_LIGHT}` | mark, wordmark, verdict glyphs on light |\n"
        f"| ink-dark | `{INK_DARK}` | same on dark |\n"
        f"| mono-light | `{MONO_LIGHT}` | print / single-colour / CLI |\n"
        f"| mono-dark | `{MONO_DARK}` | same on dark |\n"
        f"| na-grey | `{NA_GREY}` | NotApplicable only |\n\n"
        "Verdict glyphs (`verdict/`): present = solid dot, absent = ring, inconclusive = halftone dot, "
        "not_applicable = grey dash. Shape carries the meaning; colour is never required.\n\n"
        "## Contrast (WCAG ratio; 4.5 = AA text, 3.0 = AA large/graphics)\n\n| Pair | Ratio |\n|---|---|\n"
        + "".join(f"| {n} | {contrast(f, b):.2f} |\n" for n, f, b in rows)
    )


# ---------------------------------------------------------------- output

def generate(out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    (out / "verdict").mkdir(exist_ok=True)

    w = lambda name, text: (out / name).write_text(text)
    w("README.md", readme())
    w("halftone-mark.svg", mark_svg(**MASTER, ink="currentColor"))
    w("halftone-mark-light.svg", mark_svg(**MASTER, ink=INK_LIGHT))
    w("halftone-mark-dark.svg", mark_svg(**MASTER, ink=INK_DARK))
    w("halftone-mark-mono-light.svg", mark_svg(**MASTER, ink=MONO_LIGHT))
    w("halftone-mark-mono-dark.svg", mark_svg(**MASTER, ink=MONO_DARK))
    w("halftone-lockup.svg", lockup_svg("currentColor"))
    w("halftone-lockup-light.svg", lockup_svg(INK_LIGHT))
    w("halftone-lockup-dark.svg", lockup_svg(INK_DARK))
    for s, p in SMALL.items():
        w(f"favicon-{s}.svg", mark_svg(**p, ink=INK_LIGHT))
    for st in STATUSES:
        w(f"verdict/{st}.svg", glyph_svg(st, "currentColor"))
        w(f"verdict/{st}-light.svg", glyph_svg(st, INK_LIGHT))
        w(f"verdict/{st}-dark.svg", glyph_svg(st, INK_DARK))

    try:
        import cairosvg
        from PIL import Image
    except ImportError:
        print("cairosvg/Pillow missing; vector files only", file=sys.stderr)
        return
    pngs = []
    for s, p in SMALL.items():
        png = out / f"favicon-{s}.png"
        cairosvg.svg2png(bytestring=mark_svg(**p, ink=INK_LIGHT).encode(),
                         write_to=str(png), output_width=s, output_height=s)
        pngs.append(png)
    for s in (128, 256, 512, 1024):
        cairosvg.svg2png(bytestring=mark_svg(**MASTER, ink=INK_LIGHT).encode(),
                         write_to=str(out / f"halftone-mark-{s}.png"),
                         output_width=s, output_height=s)
    imgs = [Image.open(p).convert("RGBA") for p in reversed(pngs)]  # 16, 32, 48
    imgs[0].save(out / "favicon.ico", sizes=[(i.width, i.height) for i in imgs],
                 append_images=imgs[1:])


def embed(src: Path, dst: Path) -> None:
    for rel in EMBED:
        target = dst / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src / rel, target)


def check() -> int:
    """Regenerate into a temp dir; diff text/vector files against brand/ and the
    embedded copy. Rasters are skipped (byte output varies by cairosvg version)."""
    with tempfile.TemporaryDirectory() as td:
        fresh = Path(td) / "brand"
        generate(fresh)
        drift = []

        def compare(expected_root: Path, actual_root: Path, rels):
            for rel in rels:
                a, b = expected_root / rel, actual_root / rel
                if not b.exists():
                    drift.append(f"missing  {b.relative_to(REPO)}")
                elif not filecmp.cmp(a, b, shallow=False):
                    drift.append(f"changed  {b.relative_to(REPO)}")

        vector = sorted(p.relative_to(fresh) for p in fresh.rglob("*")
                        if p.is_file() and p.suffix in TEXT_SUFFIXES)
        compare(fresh, HERE, vector)
        compare(fresh, EMBED_DIR, EMBED)

    if drift:
        print("brand assets out of date; run `python3 brand/gen_logo.py --embed`:", file=sys.stderr)
        print("\n".join("  " + d for d in drift), file=sys.stderr)
        return 1
    print("brand assets up to date")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=Path, help="write here instead of this directory")
    ap.add_argument("--embed", action="store_true", help="also refresh the report crate's assets/")
    ap.add_argument("--check", action="store_true", help="verify committed files match; exit 1 on drift")
    a = ap.parse_args()

    if a.check:
        return check()
    out = a.out or HERE
    generate(out)
    if a.embed:
        if a.out:
            ap.error("--embed only makes sense when writing to brand/ itself")
        embed(HERE, EMBED_DIR)
        print(f"embedded {len(EMBED)} files into {EMBED_DIR.relative_to(REPO)}")
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
