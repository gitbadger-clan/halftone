#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27", "pillow>=11", "numpy>=2"]
#
# [tool.uv]
# exclude-newer = "14 days"  # only releases at least two weeks old
# ///
"""Stratum 03: synthetic DigitalSourceType and Data Mining edge cases, written with
ExifTool onto plain base images, into corpus/differential/03-synthetic.

Every file's intended Halftone verdict is known by construction and recorded in
cases.json next to the files; scripts/differential.py merges it into
expectations.json as `halftone`, and the differential test asserts it alongside the
ExifTool/c2patool comparison. The stratum is matched by file name, not hash.
`status` and `digital_source_type` are what marking_metadata must say; the
`data_mining` object, on the `dm_*` cases, is what data_mining_reservation must say;
`vendor_ai`, `rights` and `identifiers`, on the `va_*`, `rs_*` and `id_*` cases, are
what vendor_ai_flag, rights_statements and embedded_identifiers must say. Every name,
address and number in those cases is invented.

Bytes do not depend on the ExifTool that wrote them. By default ExifTool stamps every
XMP packet with its own version (x:xmptk='Image::ExifTool 13.59'), which made the
files, and the sha256s recorded for them, differ between a Mac, the reference image
and the CI runner. Every case that writes XMP sets that field to a constant instead
(TOOLKIT). Measured with ExifTool 12.76 and 13.59: all 63 files come out
byte-identical. A case that writes no XMP gets no toolkit either, so `plain` and
`iim_only` still carry no packet at all.

Which ExifTool wrote the files is still recorded, next to them instead of inside
them: SOURCE.txt names the generator, the ExifTool version, the toolkit constant and
the sha256 of each base image. It has no date, so it only changes when one of those
does. It is committed like the SOURCE.txt of 01 and 02; a run with another ExifTool
changes its one version line and nothing else, and CI restores the committed copy
after generating, so a different ExifTool on a runner cannot fail a build.

Base images (00-base/base.{jpg,png,webp}) are made with Pillow and numpy from seed 0
when absent; the script's lock file pins both, so they come out the same everywhere.
Needs exiftool >= 12 on PATH. Offline. Never reads the clipboard.
Exit 0 written or already populated, 1 ExifTool failed, 2 setup error.
"""

from __future__ import annotations

import hashlib
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
# In place of ExifTool's version stamp, so the bytes are the same whichever ExifTool
# runs. Added only where a case writes XMP: on its own it would create a packet.
TOOLKIT = "-XMP-x:XMPToolkit=Halftone gen-synthetic"

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


DATA_MINING = "-XMP-plus:DataMining#"  # `#`: the raw code; ExifTool adds the vocabulary URI
PLUS_VOCAB = "http://ns.useplus.org/ldf/vocab/"


def data_mining_cases(pad: Path) -> list[tuple[str, str, str, list[str], str, list[str], str, str | None]]:
    """(stem, ext, intended data_mining_reservation status, its term codes, note,
    exiftool args, intended marking status, marking term). The IPTC / PLUS Data
    Mining property: every prohibiting kind, the two that prohibit nothing, the
    spellings ExifTool can produce that are not in the vocabulary, the containers and
    forms, and the property next to a generative marking (two sources, two answers)."""
    t = f"{SCHEME}/trainedAlgorithmicMedia"

    def case(stem, ext, status, codes, note, args, marking=("absent", None)):
        return (stem, ext, status, codes, note, args, *marking)

    def one(stem, ext, status, code, note, extra=()):
        return case(stem, ext, status, [code], note, [*extra, f"{DATA_MINING}={code}"])

    return [
        one("dm_prohibited", "jpg", "present", "DMI-PROHIBITED", "prohibited, URI form (element)"),
        one("dm_prohibited_aiml", "png", "present", "DMI-PROHIBITED-AIMLTRAINING", "prohibited for AI/ML training, PNG"),
        one("dm_prohibited_genai", "webp", "present", "DMI-PROHIBITED-GENAIMLTRAINING",
            "prohibited for generative AI/ML training, WebP"),
        one("dm_prohibited_search", "jpg", "present", "DMI-PROHIBITED-EXCEPTSEARCHENGINEINDEXING",
            "prohibited except for search engine indexing"),
        one("dm_see_constraint", "jpg", "present", "DMI-PROHIBITED-SEECONSTRAINT",
            "defers to the Other Constraints text, which is present",
            ["-XMP-plus:OtherConstraints=Research use only, ask first"]),
        one("dm_see_linked", "jpg", "present", "DMI-PROHIBITED-SEELINKEDRIGHTSEXPR",
            "defers to a linked rights expression, which is not read"),
        one("dm_allowed", "png", "absent", "DMI-ALLOWED", "allowed: no reservation"),
        one("dm_unspecified", "webp", "absent", "DMI-UNSPECIFIED", "unspecified: no reservation"),
        one("dm_attribute", "jpg", "present", "DMI-PROHIBITED", "attribute (shorthand) form", ["-api", "XMPShorthand=1"]),
        one("dm_extended_xmp", "jpg", "present", "DMI-PROHIBITED", "ExtendedXMP: packet over 64 KiB",
            [f"-XMP-dc:Description<={pad}"]),
        one("dm_unknown_term", "jpg", "inconclusive", "DMI-ALLOWED-noaitraining",
            "a term from the 2023 draft that the published vocabulary dropped"),
        # Given a full URI as the raw value, ExifTool prepends the vocabulary URI again
        # and reads the result back as unknown; so does Halftone.
        case("dm_doubled_uri", "jpg", "inconclusive", [f"{PLUS_VOCAB}DMI-PROHIBITED"],
             "URI written twice by ExifTool: not in the vocabulary",
             [f"{DATA_MINING}={PLUS_VOCAB}DMI-PROHIBITED"]),
        case("dm_and_generative", "jpg", "present", ["DMI-PROHIBITED-AIMLTRAINING"],
             "reservation next to a generative marking: each source answers for itself",
             [f"{DATA_MINING}=DMI-PROHIBITED-AIMLTRAINING", f"{DST}={t}"],
             marking=("present", "trainedAlgorithmicMedia")),
    ]


def packet(xmlns: str, body: str) -> str:
    """A whole XMP packet, for properties ExifTool cannot write by name (a vendor's own
    namespace). Written into the file as a block (`-xmp<=`), byte for byte."""
    return ("<?xpacket begin='\ufeff' id='W5M0MpCehiHzreSzNTczkc9d'?>\n"
            "<x:xmpmeta xmlns:x='adobe:ns:meta/' x:xmptk='Halftone gen-synthetic'>\n"
            "<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n"
            f" <rdf:Description rdf:about=''\n  {xmlns}>\n{body}\n </rdf:Description>\n"
            "</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end='w'?>")


# The shape measured on a design tool's exports: its own yes/no AI field (the prefix
# has the field's name), an ads record with a JSON blob, an author in the PDF namespace.
# Every value is invented.
DESIGN_TOOL_NS = ("xmlns:Attrib='http://ns.attribution.com/ads/1.0/'\n  "
                  "xmlns:pdf='http://ns.adobe.com/pdf/1.3/'\n  "
                  "xmlns:ContainsAiGeneratedContent='https://canva.com/export'")


def design_tool_packet(flag: str, with_account: bool) -> str:
    account = ("""  <Attrib:Ads>
   <rdf:Seq>
    <rdf:li rdf:parseType='Resource'>
     <Attrib:Created>2026-01-02</Attrib:Created>
     <Attrib:Data>{&quot;doc&quot;:&quot;DOC-EXAMPLE&quot;,&quot;user&quot;:&quot;USER-EXAMPLE&quot;}</Attrib:Data>
     <Attrib:ExtId>00000000-1111-2222-3333-444444444444</Attrib:ExtId>
     <Attrib:FbId>123456789012345</Attrib:FbId>
     <Attrib:TouchType>2</Attrib:TouchType>
    </rdf:li>
   </rdf:Seq>
  </Attrib:Ads>
  <pdf:Author>Example Author</pdf:Author>
""" if with_account else "")
    return packet(DESIGN_TOOL_NS, account + "  <ContainsAiGeneratedContent:ContainsAiGeneratedContent>"
                  f"{flag}</ContainsAiGeneratedContent:ContainsAiGeneratedContent>")


def listing_cases() -> list[tuple[str, str, dict[str, str], str, list[str], str | None]]:
    """(stem, ext, intended status per listing source, note, exiftool args, XMP packet
    or None). Cases for vendor_ai_flag, rights_statements and embedded_identifiers; a
    source not named in the dict must say `absent`. Every name, address and number is
    invented, so the stratum's ground truth can record the values in the clear."""
    notice = "(c) 2026 Ada Example"
    long_credit = "Example Agency for International Photography"  # over IIM's 32 bytes

    def case(stem, ext, intent, note, args, xmp=None):
        return (stem, ext, intent, note, args, xmp)

    return [
        case("va_flag_yes", "png", {"vendor_ai": "present", "identifiers": "present"},
             "a design tool's export: its AI flag says Yes, with its ads record and author",
             [], design_tool_packet("Yes", True)),
        case("va_flag_no", "jpg", {}, "the same flag saying No, nothing else", [],
             design_tool_packet("No", False)),
        case("va_google_credit", "png", {"vendor_ai": "present", "rights": "present"},
             "the credit line that is a vendor's AI statement", ["-XMP-photoshop:Credit=Made with Google AI"]),
        case("rs_full", "jpg", {"rights": "present"},
             "notice in all three copies, terms, licence, licensor, credit cut at IIM's limit",
             [f"-IFD0:Copyright={notice}", f"-IPTC:CopyrightNotice={notice}", f"-XMP-dc:Rights={notice}",
              "-XMP-xmpRights:Marked=True", "-XMP-xmpRights:UsageTerms=Editorial use only",
              "-XMP-xmpRights:WebStatement=https://example.org/licence",
              f"-IPTC:Credit={long_credit}", f"-XMP-photoshop:Credit={long_credit}",
              "-IPTC:Source=Example Wire", "-XMP-photoshop:Source=Example Wire",
              "-XMP-plus:LicensorName=Example Licensing", "-XMP-plus:LicensorURL=https://example.org/l",
              "-XMP-cc:License=https://creativecommons.org/licenses/by/4.0/"]),
        case("rs_disagree", "jpg", {"rights": "inconclusive"},
             "EXIF and XMP copies of the notice differ",
             ["-IFD0:Copyright=(c) Example One", "-XMP-dc:Rights=(c) Example Two"]),
        case("rs_instructions", "jpg", {"rights": "present"}, "an instruction to editors",
             ["-IPTC:SpecialInstructions=Not for resale"]),
        case("id_camera", "jpg", {"identifiers": "present"},
             "a camera file: names, contact, serial, owner, position, document id; lens serial N/A",
             ["-IFD0:Artist=Ada Example", "-IPTC:By-line=Ada Example", "-XMP-dc:Creator=Ada Example",
              "-XMP-iptcCore:CreatorWorkEmail=ada@example.org", "-ExifIFD:SerialNumber=SN-0042",
              "-ExifIFD:OwnerName=Ada Example", "-ExifIFD:LensSerialNumber=N/A",
              "-GPS:GPSLatitude=47.3769", "-GPS:GPSLatitudeRef=N",
              "-GPS:GPSLongitude=8.5417", "-GPS:GPSLongitudeRef=E",
              "-XMP-xmpMM:DocumentID=xmp.did:00000000-0000-4000-8000-000000000001"]),
        case("id_marker", "jpg", {"identifiers": "present"},
             "a platform's tracking marker in the instructions field: an identifier, not an instruction",
             ["-IPTC:SpecialInstructions=FBMD0a000a880100003e0200000000"]),
        case("id_placeholder", "jpg", {}, "serial fields holding placeholders identify nobody",
             ["-ExifIFD:LensSerialNumber=N/A", "-ExifIFD:SerialNumber=0000000"]),
        case("id_png_text", "png", {"identifiers": "present", "rights": "present"},
             "PNG text chunks: Author and Copyright",
             ["-PNG:Author=Ada Example", f"-PNG:Copyright={notice}"]),
    ]


def encoded_cases() -> list[tuple[str, str, str, str, str]]:
    """(stem, ext, packet encoding, Python codec, note). XMP packets in the encodings
    the specification allows besides UTF-8, each declaring trainedAlgorithmicMedia: a
    reader that assumes UTF-8 finds no property in them. ExifTool cannot be told to
    write these, so the packet is built here and embedded as a block. UTF-32 is left to
    Halftone's unit tests: ExifTool does not embed such a block, so there is no second
    reading to compare with."""
    return [
        ("utf16be_packet", "jpg", "UTF-16 big-endian", "utf-16-be", "what older Adobe and ExifTool writers produced"),
        ("utf16le_packet", "png", "UTF-16 little-endian", "utf-16-le", "the other byte order, in a PNG"),
    ]


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


def source_txt(count: int, exiftool_version: str) -> str:
    """Provenance of the generated files. No date: the same inputs give the same text."""
    bases = "".join(
        f"base.{e}: sha256 {hashlib.sha256((BASE / f'base.{e}').read_bytes()).hexdigest()}\n"
        for e in ("jpg", "png", "webp")
    )
    return (
        "generator: scripts/gen-synthetic.py\n"
        f"files: {count} (cases.json holds the intended verdicts)\n"
        f"exiftool: {exiftool_version}\n"
        f"xmp toolkit: {TOOLKIT.split('=', 1)[1]} (in place of ExifTool's version stamp, so the\n"
        "  files' bytes do not depend on the ExifTool version above; DIFFERENTIAL.md D-020)\n"
        f"{bases}"
    )


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
    generated = [f for f in DEST.glob("*") if f.is_file() and f.name not in KEEP | {"cases.json", "SOURCE.txt"}] if DEST.is_dir() else []
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
            exiftool_version = et.version()
            def write(name: str, ext: str, args: list[str]) -> Path:
                out = DEST / f"{name}.{ext}"
                if any(a.startswith("-XMP") for a in args):
                    args = [*args, TOOLKIT]
                _, err = et.run("-q", "-q", "-o", str(out), *args, str(BASE / f"base.{ext}"))
                if not out.is_file() or "Error" in err:
                    die(f"exiftool failed for {name}: {err.strip() or 'no output file'}", 1)
                return out

            for name, ext, status, code, note, args in cases(pad):
                out = write(name, ext, args)
                rows.append((out.name, {"status": status, "digital_source_type": [code] if code else [], "note": note}))
            # After the earlier cases, so their lines in cases.json stay where they were.
            for name, ext, dm_status, dm_codes, note, args, status, code in data_mining_cases(pad):
                out = write(name, ext, args)
                rows.append((out.name, {
                    "status": status, "digital_source_type": [code] if code else [],
                    "data_mining": {"status": dm_status, "codes": dm_codes}, "note": note,
                }))
            block = DEST / ".packet.xmp"
            marked = packet("xmlns:Iptc4xmpExt='http://iptc.org/std/Iptc4xmpExt/2008-02-29/'",
                            f"  <Iptc4xmpExt:DigitalSourceType>{SCHEME}/trainedAlgorithmicMedia"
                            "</Iptc4xmpExt:DigitalSourceType>")
            for name, ext, encoding, codec, note in encoded_cases():
                block.write_bytes(marked.encode(codec))
                out = write(name, ext, [f"-xmp<={block}"])
                rows.append((out.name, {"status": "present", "digital_source_type": ["trainedAlgorithmicMedia"],
                                        "note": f"XMP packet in {encoding}: {note}"}))
            for name, ext, intent, note, args, xmp in listing_cases():
                if xmp is not None:
                    block.write_text(xmp)
                    args = [*args, f"-xmp<={block}"]  # lower case: a block, no toolkit to set
                out = write(name, ext, args)
                block.unlink(missing_ok=True)
                rows.append((out.name, {
                    "status": "absent", "digital_source_type": [],
                    **{k: {"status": intent.get(k, "absent")} for k in ("vendor_ai", "rights", "identifiers")},
                    "note": note,
                }))
    finally:
        pad.unlink(missing_ok=True)
        (DEST / ".packet.xmp").unlink(missing_ok=True)
    (DEST / "cases.json").write_text(cases_json(rows))
    (DEST / "SOURCE.txt").write_text(source_txt(len(rows), exiftool_version))
    print(f"{len(rows)} files -> {DEST} (cases.json holds the intended verdicts)")
    print(f"next: uv run scripts/references.py collect {DEST.name}")


if __name__ == "__main__":
    from complete import serve  # tab completion (lib/complete.py); returns unless asked

    serve(app)
    app()
