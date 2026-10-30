#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27", "pillow>=11", "numpy>=2"]
# ///
"""Stratum 03: synthetic DigitalSourceType edge cases, written with ExifTool onto plain
base images, into corpus/differential/03-synthetic.

Every file's intended Halftone verdict is known by construction and recorded in
cases.json next to the files; scripts/differential.py merges it into
expectations.json as `halftone`, and the differential test asserts it alongside the
ExifTool/c2patool comparison. The stratum is matched by file name, not hash: ExifTool
writes its version into the packets, so bytes differ between machines, facts do not.

Base images (00-base/base.{jpg,png,webp}) are made with Pillow and numpy from seed 0
when absent; the script's lock file pins both, so they come out the same everywhere.
Needs exiftool >= 12 on PATH. Offline. Never reads the clipboard.
Exit 0 written or already populated, 1 ExifTool failed, 2 setup error.
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path
from typing import Annotated

import typer

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from exiftool import ExifTool  # noqa: E402
from repo import CORPUS, die, need  # noqa: E402

BASE = CORPUS / "00-base"
DEST = CORPUS / "03-synthetic"
SCHEME = "http://cv.iptc.org/newscodes/digitalsourcetype"
DST = "-XMP-iptcExt:DigitalSourceType"
KEEP = {"expectations.json"}  # committed ground truth survives --refresh

GENERATIVE = ["trainedAlgorithmicMedia", "compositeWithTrainedAlgorithmicMedia"]
NON_GENERATIVE = [
    "digitalCapture", "computationalCapture", "negativeFilm", "positiveFilm", "print", "humanEdits",
    "compositeCapture", "algorithmicallyEnhanced", "dataDrivenMedia", "digitalCreation",
    "virtualRecording", "compositeSynthetic", "algorithmicMedia", "screenCapture",
]
RETIRED = ["digitalArt", "minorHumanEdits"]


def cases(pad: Path) -> list[tuple[str, str, str, str | None, str, list[str]]]:
    """(name, container, intended status, intended code or None, note, ExifTool args),
    in the order cases.json lists them."""
    t = f"{SCHEME}/trainedAlgorithmicMedia"
    out = []
    # 1. Every vocabulary term as a URI, on JPEG.
    out += [(f"term_{x}", "jpg", "present", x, "generative term, URI form", [f"{DST}={SCHEME}/{x}"]) for x in GENERATIVE]
    out += [(f"term_{x}", "jpg", "absent", x, "non-generative term, URI form", [f"{DST}={SCHEME}/{x}"]) for x in NON_GENERATIVE]
    out += [(f"term_{x}", "jpg", "absent", x, "retired term; interpreted, flagged retired", [f"{DST}={SCHEME}/{x}"]) for x in RETIRED]
    # 2. Generative term on the other two containers.
    out += [
        ("gen_png", "png", "present", "trainedAlgorithmicMedia", "PNG iTXt XML:com.adobe.xmp", [f"{DST}={t}"]),
        ("gen_webp", "webp", "present", "compositeWithTrainedAlgorithmicMedia", "WebP XMP chunk",
         [f"{DST}={SCHEME}/compositeWithTrainedAlgorithmicMedia"]),
    ]
    # 3. Value spellings.
    out += [
        ("bare_term", "jpg", "present", "trainedAlgorithmicMedia", "bare term, no scheme URI", [f"{DST}=trainedAlgorithmicMedia"]),
        ("https_scheme", "jpg", "present", "trainedAlgorithmicMedia", "https scheme instead of http",
         [f"{DST}=https://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"]),
        ("trailing_slash", "jpg", "present", "trainedAlgorithmicMedia", "trailing slash on the URI", [f"{DST}={t}/"]),
        ("case_variant", "jpg", "inconclusive", "TrainedAlgorithmicMedia", "capitalised term: not in vocabulary, verbatim",
         [f"{DST}={SCHEME}/TrainedAlgorithmicMedia"]),
        ("unknown_term", "jpg", "inconclusive", "syntheticFoo", "term outside the vocabulary", [f"{DST}={SCHEME}/syntheticFoo"]),
        ("unknown_scheme", "jpg", "present", "trainedAlgorithmicMedia", "foreign scheme, known term: last segment wins",
         [f"{DST}=http://example.org/vocab/trainedAlgorithmicMedia"]),
    ]
    # 4. Syntactic form: ExifTool's default is element form; XMPShorthand writes attributes.
    out += [("form_attribute", "jpg", "present", "trainedAlgorithmicMedia", "attribute (shorthand) form",
             ["-api", "XMPShorthand=1", f"{DST}={t}"])]
    # 5. Packet size: more than 64 KiB forces ExtendedXMP on JPEG.
    out += [
        ("extended_xmp", "jpg", "present", "trainedAlgorithmicMedia", "ExtendedXMP: field in the extension packet",
         [f"-XMP-dc:Description<={pad}", f"{DST}={t}"]),
        ("extended_xmp_no_field", "jpg", "absent", None, "ExtendedXMP present, no field anywhere", [f"-XMP-dc:Description<={pad}"]),
    ]
    # 6. XMP present without the field.
    out += [("xmp_no_field", e, "absent", None, "XMP with CreatorTool only", ["-XMP-xmp:CreatorTool=Synthetic Writer 1.0"])
            for e in ("jpg", "png", "webp")]
    # 7. IIM.
    out += [
        ("iim_only", "jpg", "absent", None, "IIM OriginatingProgram, no XMP",
         ["-IPTC:OriginatingProgram=SynthCam", "-IPTC:ProgramVersion=2.1"]),
        ("iim_and_dst", "jpg", "present", "trainedAlgorithmicMedia", "IIM plus generative XMP",
         ["-IPTC:OriginatingProgram=SynthGen", f"{DST}={t}"]),
        ("iim_and_capture", "jpg", "absent", "digitalCapture", "IIM plus capture XMP",
         ["-IPTC:OriginatingProgram=SynthCam", f"{DST}={SCHEME}/digitalCapture"]),
    ]
    # 8. Plain copies as negatives.
    out += [("plain", e, "absent", None, "no metadata at all", []) for e in ("jpg", "png", "webp")]
    return out


def make_base() -> None:
    """96x96 noise from seed 0, saved as the three base images."""
    import numpy as np  # only needed the first time
    from PIL import Image

    rng = np.random.default_rng(0)
    img = Image.fromarray(rng.integers(0, 255, (96, 96, 3), dtype=np.uint8))
    img.save(BASE / "base.jpg", quality=85)
    img.save(BASE / "base.png")
    img.save(BASE / "base.webp", quality=80)


def cases_json(rows: list[tuple[str, dict]]) -> str:
    """One case per line, in generation order: the layout the committed file has."""
    body = ",\n".join(f"  {json.dumps(name)}: {json.dumps(entry)}" for name, entry in rows)
    return "{\n" + body + "\n}\n"


app = typer.Typer(add_completion=False, pretty_exceptions_show_locals=False)


@app.command(help=__doc__)
def main(
    refresh: Annotated[bool, typer.Option("--refresh", help="Regenerate; keeps expectations.json.")] = False,
) -> None:
    need("exiftool")
    if refresh and DEST.is_dir():
        for f in DEST.iterdir():
            if f.name not in KEEP:
                shutil.rmtree(f) if f.is_dir() else f.unlink()
    generated = [f for f in DEST.glob("*") if f.is_file() and f.name not in KEEP | {"cases.json"}] if DEST.is_dir() else []
    if generated:
        print(f"{DEST} already populated ({len(generated)} files); use --refresh to regenerate")
        raise typer.Exit(0)
    DEST.mkdir(parents=True, exist_ok=True)
    BASE.mkdir(parents=True, exist_ok=True)
    if not all((BASE / f"base.{e}").exists() for e in ("jpg", "png", "webp")):
        make_base()

    pad = DEST / ".pad.txt"
    pad.write_text("X" * 70000 + "\n")  # what `string repeat -n 70000 X` wrote
    rows: list[tuple[str, dict]] = []
    try:
        with ExifTool() as et:
            for name, ext, status, code, note, args in cases(pad):
                out = DEST / f"{name}.{ext}"
                _, err = et.run("-q", "-q", "-o", str(out), *args, str(BASE / f"base.{ext}"))
                if not out.is_file() or "Error" in err:
                    die(f"exiftool failed for {name}: {err.strip() or 'no output file'}", 1)
                rows.append((out.name, {"status": status, "digital_source_type": [code] if code else [], "note": note}))
    finally:
        pad.unlink(missing_ok=True)
    (DEST / "cases.json").write_text(cases_json(rows))
    print(f"{len(rows)} files -> {DEST} (cases.json holds the intended verdicts)")
    print(f"next: uv run scripts/collect.py {DEST.name}")


if __name__ == "__main__":
    app()
