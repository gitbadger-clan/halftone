#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27"]
# ///
"""Fetch a public fixture set into its stratum: c2pa-rs's test fixtures (01-c2pa-rs) or
ExifTool's test images (02-exiftool).

Only files Halftone can load are kept, decided by the MIME type ExifTool sniffs from
the bytes, never by extension: the ExifTool set holds mislabeled files on purpose, and
the collector decides the same way. ExifTool runs once over the whole clone, not once
per file. Paths are flattened (`sub/dir/x.jpg` -> `sub__dir__x.jpg`); in the ExifTool
set a copy whose extension disagrees with its bytes gets the right one appended, so a
disagreement still points at the upstream file. SOURCE.txt records repo, commit,
counts and licence.

The directory is not for redistribution: the photos carry their own terms. Commit
expectations.json, not the files.

Needs git >= 2.25 (sparse checkout) and exiftool. Network: github.com only.
Exit 0 fetched or already populated, 1 git or ExifTool failed, 2 usage error.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from enum import StrEnum
from pathlib import Path
from typing import Annotated

import typer

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from exiftool import ExifTool  # noqa: E402
from progress import step  # noqa: E402
from repo import CORPUS, die, need  # noqa: E402

KEEP_MIME = {"image/jpeg", "image/png", "image/webp", "image/heic", "image/heif", "image/avif"}
EXT_FOR = {"image/jpeg": ".jpg", "image/png": ".png", "image/webp": ".webp",
           "image/heic": ".heic", "image/heif": ".heic", "image/avif": ".avif"}
PROTECTED = {"expectations.json", "SOURCE.txt", "cases.json"}  # not "fetched files"


@dataclass(frozen=True)
class Source:
    repo: str
    subdir: str
    stratum: str
    pin: str          # the commit CI uses; override with --commit or the env var
    env: str
    relabel: bool     # append the extension the bytes deserve (ExifTool's set)
    licence: str


class Which(StrEnum):
    c2pa_rs = "c2pa-rs"
    exiftool = "exiftool"


SOURCES = {
    Which.c2pa_rs: Source(
        "https://github.com/contentauth/c2pa-rs", "sdk/tests/fixtures", "01-c2pa-rs",
        "8641fbac3971dc7163c4ba67d8c588f62c99761b", "C2PA_FIXTURES_COMMIT", False,
        "repository Apache-2.0 OR MIT; image contents may carry their own terms. Do not redistribute this directory.",
    ),
    Which.exiftool: Source(
        "https://github.com/exiftool/exiftool", "t/images", "02-exiftool",
        "2200871d9cef988051d2a99d67df3bda6cbb30a8", "EXIFTOOL_FIXTURES_COMMIT", True,
        "ExifTool is GPL-1+ OR Artistic; test images are test material. Do not redistribute this directory.",
    ),
}


def git(*args: str) -> None:
    r = subprocess.run(["git", *args], capture_output=True, text=True)
    if r.returncode != 0:
        die(f"git {' '.join(args[:3])} …: {(r.stderr or r.stdout).strip()[:300]}", 1)


def git_out(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout.strip()


def target_name(rel: str, mime: str, relabel: bool) -> tuple[str, bool]:
    """Flattened name, and whether an extension was appended."""
    name = rel.replace("/", "__")
    if not relabel:
        return name, False
    ext = os.path.splitext(name)[1].lower()
    want = EXT_FOR[mime]
    if ext != want and not (ext == ".jpeg" and want == ".jpg"):
        return name + want, True
    return name, False


app = typer.Typer(add_completion=False, pretty_exceptions_show_locals=False)


@app.command(help=__doc__)
def main(
    source: Annotated[Which, typer.Argument(help="Which fixture set.")],
    refresh: Annotated[bool, typer.Option("--refresh", help="Re-fetch; keeps expectations.json.")] = False,
    commit: Annotated[str | None, typer.Option(help="Upstream commit (full sha). Default: $C2PA_FIXTURES_COMMIT / $EXIFTOOL_FIXTURES_COMMIT, else the pin.")] = None,
) -> None:
    src = SOURCES[source]
    need("git", "exiftool")
    dest = CORPUS / src.stratum
    commit = commit or os.environ.get(src.env) or src.pin

    if refresh and dest.is_dir():
        for f in dest.iterdir():
            if f.name != "expectations.json":
                shutil.rmtree(f) if f.is_dir() else f.unlink()
    fetched = [f for f in dest.iterdir() if f.is_file() and f.name not in PROTECTED] if dest.is_dir() else []
    if fetched:
        print(f"{dest} already populated ({len(fetched)} files); use --refresh to re-fetch")
        raise typer.Exit(0)
    dest.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="fixtures-") as tmp:
        repo = Path(tmp) / "repo"
        with step(f"sparse clone of {src.repo} ({src.subdir}) @ {commit[:12]}"):
            git("clone", "--quiet", "--no-checkout", "--filter=blob:none", "--sparse", src.repo, str(repo))
            git("-C", str(repo), "sparse-checkout", "set", src.subdir)
            git("-C", str(repo), "fetch", "--quiet", "--depth", "1", "origin", commit)
            git("-C", str(repo), "checkout", "--quiet", commit)
        commit = git_out("-C", str(repo), "rev-parse", "HEAD")
        source_txt = dest / "SOURCE.txt"
        # Provenance first, so an interrupted run is visibly incomplete.
        source_txt.write_text(f"source: {src.repo}\nsubdir: {src.subdir}\ncommit: {commit}\n"
                              "status: fetch in progress (interrupted if you can read this)\n")

        root = repo / src.subdir
        files = sorted(p for p in root.rglob("*") if p.is_file())
        mimes: dict[str, str] = {}
        with step(f"sniff {len(files)} files with ExifTool"), ExifTool() as et:
            # An argument file names exactly these files: given a directory, ExifTool
            # would skip unknown extensions and hidden files the old per-file loop sniffed.
            argfile = Path(tmp) / "files.txt"
            argfile.write_text("".join(f"{p}\n" for p in files))
            for row in et.json("-MIMEType", "-@", str(argfile)):
                mimes[row.get("SourceFile", "")] = row.get("MIMEType", "")
        kept = skipped = relabeled = 0
        for p in files:
            mime = mimes.get(str(p), "")
            if mime not in KEEP_MIME:
                skipped += 1
                continue
            name, appended = target_name(p.relative_to(root).as_posix(), mime, src.relabel)
            shutil.copyfile(p, dest / name)
            kept += 1
            relabeled += appended

    kept_line = f"kept: {kept} image files (sniffed jpeg/png/webp/heic/avif)"
    if src.relabel:
        kept_line += f", {relabeled} of them with an extension that did not match their bytes (suffix added)"
    fetched_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    source_txt.write_text(
        f"source: {src.repo}\nsubdir: {src.subdir}\ncommit: {commit}\nfetched: {fetched_at}\n"
        f"{kept_line}\nskipped: {skipped} non-image or unsupported files\nlicence: {src.licence}\n"
    )
    extra = f" ({relabeled} relabeled)" if src.relabel else ""
    print(f"kept {kept}{extra}, skipped {skipped} -> {dest} (commit {commit})")
    print(f"next: uv run scripts/collect.py {src.stratum}")


if __name__ == "__main__":
    app()
