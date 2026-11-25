#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27", "inquirer>=3.4", "pymobiledevice3>=11.45,<12"]
#
# [tool.uv]
# exclude-newer = "14 days"  # only releases at least two weeks old
# ///
"""Bring files from outside the repo into a corpus stratum: Android phones over adb,
iPhones and iPads over AFC, and browser downloads from ~/Downloads.

An iPhone or iPad has to trust this Mac once, over a cable, before anything else
works; after that it can be reached over Wi-Fi. `htc ios connect` walks through that
and says at each wait where to look; `htc ios connect --check` shows what each device
needs without changing anything.

Every command that writes into the corpus needs the target stratum:

    set -gx HT_STRATUM 06-phones

One of 00-base, 04-generators, 05-fold, 06-phones, 07-editors, 10-messaging,
11-screenshots (08-multipart is built by make-multipart-stratum.fish). Without it
they refuse: a missing value once filed phone shots under 04. Files keep
the extension they were delivered with (SOURCE.md naming rule). `-f` on grab and take
replaces an existing file of the same stem, under any extension, once the new copy
has passed its check, and keeps the old copy in /tmp/ht-replaced/ (cleared on
restart). When the new copy has the same sha256 as the file it replaces, a warning
says so: the capture probably produced no new file.

Every copy is checked before it gets its name: Android pulls by the sha256 the phone
computes, iOS pulls by the size before and after reading plus the SHA-1 the device
computes (pymobiledevice3 11.50 or newer; older versions cannot ask for it, and the
file is read twice and the two reads compared instead). A copy that fails the check
never lands in the stratum; it is kept in /tmp/ht-mismatch/ for inspection.

Needs adb for `android`, exiftool for `layout`, ht for the inspection after a grab or
take. That inspection is how a capture gets judged, so the commands that start or
finish one (mark, grab, take, layout) refuse to run with an `ht` older than the
source (lib/htbin.py), before they touch a device or a file; `dev --stale-ok htc …`
runs with it anyway. Without any `ht` they work and skip the inspection.
Offline. Never reads the clipboard. Exit 0 done, 1 refused or failed, 2 usage or a
stale ht.
"""

from __future__ import annotations

import asyncio
import glob
import hashlib
import os
import posixpath
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Annotated, NamedTuple

import typer

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from exiftool import ExifTool  # noqa: E402
from htbin import require_ht  # noqa: E402
from repo import CORPUS, C, die, need  # noqa: E402

EXAMPLES = """
Examples (`dev` as set up in scripts/README.md; or `uv run scripts/htc.py …`):

    dev htc android use fold | source            # this terminal's phone
    dev htc android mark                         # before saving on the phone
    dev htc android type "A ceramic bowl of …"   # into the focused field
    dev htc android send                         # tap the app's Send button
    dev htc android grab gemini-app__nano-banana__app-save-open__p1__2
    dev htc android range 2026-11-09 14:00 +45m  # candidates, outside the corpus
    dev htc desk mark
    dev htc desk take -f firefly__image-4__web-download__p1__1
    dev htc ios connect                          # trust, Wi-Fi connections, then unplug
    dev htc ios connect --check                  # what each device needs, changing nothing
    dev htc ios mark
    dev htc ios grab iphone13__default__afc-pull__s01__1
    dev htc layout corpus/differential/06-phones/*.jpg
"""

IMAGE = re.compile(r"\.(?:jpe?g|png|webp|gif|heic|heif|avif)$", re.I)
LAYOUT_IMAGE = re.compile(r"\.(?:jpe?g|png|webp|heic|heif|avif)$", re.I)
REPLACED = Path("/tmp/ht-replaced")
MISMATCH = Path("/tmp/ht-mismatch")
CANDIDATES = Path.home() / "halftone-candidates"

app = typer.Typer(add_completion=False, pretty_exceptions_show_locals=False,  # never print locals
                  rich_markup_mode="markdown", no_args_is_help=True, help=__doc__, epilog=EXAMPLES)
android = typer.Typer(no_args_is_help=True, help="Android phones over adb (one phone per terminal).")
desk = typer.Typer(no_args_is_help=True, help="Browser downloads in ~/Downloads.")
ios = typer.Typer(no_args_is_help=True, help="iPhone and iPad over AFC (USB, or Wi-Fi sync).")
app.add_typer(android, name="android")
app.add_typer(desk, name="desk")
app.add_typer(ios, name="ios")

Force = Annotated[bool, typer.Option("-f", "--force", help="Replace an existing file of this stem (old copy kept).")]
Stem = Annotated[str, typer.Argument(help="File name without extension; the delivered extension is added.")]


# ---- shared ---------------------------------------------------------------------


def stratum() -> Path:
    """The stratum named by $HT_STRATUM (required)."""
    name = os.environ.get("HT_STRATUM", "")
    if not name:
        die("HT_STRATUM is not set: e.g. set -gx HT_STRATUM 06-phones", 2)
    d = CORPUS / name
    if not d.is_dir():
        die(f"no such stratum folder: {d}", 2)
    return d


def existing(folder: Path, stem: str) -> list[Path]:
    """Every <stem>.* in the folder, under any extension."""
    return sorted(folder.glob(glob.escape(stem) + ".*"))


def refuse_existing(folder: Path, stem: str, force: bool, again: str) -> None:
    """Without -f, an existing <stem>.* stops the command before anything moves."""
    old = existing(folder, stem)
    if old and not force:
        die(f"exists: {old[0]} (use {again} -f {stem} to replace it)", 1)


def install(new: Path, folder: Path, stem: str) -> Path:
    """Give a checked file its name: move every old <stem>.* to /tmp/ht-replaced/
    first, so one name never holds two versions. Runs only after the new copy passed
    its check, so a failed copy never costs the old file its place. An old file with
    the new copy's sha256 is still replaced, with a warning."""
    old = existing(folder, stem)
    if old:
        new_sha = sha256_of(new)
        REPLACED.mkdir(parents=True, exist_ok=True)
        stamp = time.strftime("%Y%m%d-%H%M%S")
        for o in old:
            same = sha256_of(o) == new_sha
            kept = REPLACED / f"{stamp}-{o.name}"
            shutil.move(o, kept)
            print(f"replaced: {o} (old copy: {kept})")
            if same:
                warn_same(o.name, new_sha)
    dst = folder / f"{stem}.{new.name.rsplit('.', 1)[-1]}"
    os.replace(new, dst)
    return dst


def staging(folder: Path, stem: str, ext: str) -> Path:
    """Where a copy waits for its check: hidden, in the same folder, so the final
    rename is atomic."""
    return folder / f".{stem}.incoming.{ext}"


def ext_of(name: str) -> str:
    """The delivered extension, case kept: `IMG_0042.JPG` -> `JPG`."""
    return name.rsplit(".", 1)[-1]


def sha256_of(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def warn_same(name: str, sha: str) -> None:
    """Say that a replacement changed nothing: the new copy has the old file's bytes."""
    print(f"{C.YELLOW}warning: the new copy is byte-identical to the {name} it replaced "
          f"(sha256 {sha[:16]}…); the capture may not have produced a new file{C.RESET}")


def mime(p: Path) -> str:
    r = subprocess.run(["file", "-b", "--mime-type", str(p)], capture_output=True, text=True)
    return r.stdout.strip() or "?"


def ok_line(p: Path) -> None:
    print(f"ok  {p}  {mime(p)}")


_HT: list[str | None] = []  # the answer of ht_bin(), kept for the rest of the run


def ht_bin() -> str | None:
    """The `ht` that shows what was captured, or None when there is none. Exits 2
    when it is older than the source: a stale inspection is how a capture gets
    misjudged. Called first by every command that starts or finishes a capture."""
    if not _HT:
        _HT.append(require_ht(optional=True))
    return _HT[0]


def inspect(p: Path) -> None:
    ht = ht_bin()
    if ht is None:
        print(f"{C.DIM}(ht not on PATH; not inspected){C.RESET}")
        return
    subprocess.run([ht, "inspect", str(p)])


def keep_bad(tmp: Path, name: str) -> Path:
    """Move a copy that failed its check to /tmp/ht-mismatch/, out of the corpus."""
    MISMATCH.mkdir(parents=True, exist_ok=True)
    name = name.lstrip(".").replace(".incoming.", ".")
    kept = MISMATCH / f"{time.strftime('%Y%m%d-%H%M%S')}-{name}"
    shutil.move(tmp, kept)
    return kept


# ---- android --------------------------------------------------------------------

PHONE_ROOT = "/storage/emulated/0/"
PHONE_MARK = "/storage/emulated/0/Download/ht-marker"
PROMPT_FILE = "/data/local/tmp/ht-prompt.txt"
UI_DUMP = "/data/local/tmp/ui.xml"
MEDIA = "content://media/external/images/media"


def adb(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    """Run adb on $ANDROID_SERIAL's phone (or the only one connected)."""
    need("adb")
    r = subprocess.run(["adb", *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        err = (r.stderr or r.stdout).strip()
        if "more than one device" in err:
            die("several devices connected: pick one with `htc android use <model part> | source`", 2)
        if "no devices" in err or "not found" in err:
            die(f"no phone: {err} (adb devices -l lists what is connected)", 1)
        die(f"adb {' '.join(args[:2])}: {err}", 1)
    return r


def sh(cmd: str, check: bool = True) -> str:
    """One command in the phone's shell; output with Windows-era CRs removed."""
    return adb("shell", cmd, check=check).stdout.replace("\r", "")


def phone_mark() -> None:
    sh(f"touch {PHONE_MARK}")


def phone_new() -> list[str]:
    """Files newer than the marker. /sdcard is a symlink, so find starts at the real
    directory; permission noise from app-private folders is dropped."""
    if adb("shell", f"test -e {PHONE_MARK}", check=False).returncode != 0:
        die("no marker on the phone: run `htc android mark` before saving", 1)
    out = sh(f"find {PHONE_ROOT} -type f -newer {PHONE_MARK} 2>/dev/null", check=False)
    return [line.strip() for line in out.splitlines() if line.strip()]


def adb_pull_verified(remote: str, dst: Path, mismatch_to: Path | None = None) -> str | None:
    """Pull byte-exact via a temporary file and compare with the sha256 the phone
    computes. Returns the sha256, or None after printing why. A mismatched copy goes
    to `mismatch_to` (default: /tmp/ht-mismatch/), never to `dst`."""
    tmp = dst.with_name(f".{dst.name}.part")
    if adb("pull", remote, str(tmp), check=False).returncode != 0 or not tmp.exists():
        tmp.unlink(missing_ok=True)
        print(f"{C.RED}FAILED   {remote}{C.RESET}", file=sys.stderr)
        return None
    theirs = sh(f"sha256sum {shlex.quote(remote)}").split()[:1]
    ours = sha256_of(tmp)
    if theirs != [ours]:
        if mismatch_to is None:
            kept = keep_bad(tmp, dst.name)
        else:
            os.replace(tmp, mismatch_to)
            kept = mismatch_to
        print(f"{C.RED}HASH MISMATCH  {remote} (copy kept at {kept}){C.RESET}", file=sys.stderr)
        return None
    os.replace(tmp, dst)
    return ours


def devices_l() -> list[str]:
    return [ln for ln in adb("devices", "-l").stdout.splitlines()[1:] if ln.strip()]


@android.command("devices")
def android_devices() -> None:
    """Connected devices (adb devices -l)."""
    for ln in devices_l():
        print(ln)


@android.command("use")
def android_use(want: Annotated[str, typer.Argument(help="Part of the model name, e.g. fold or pixel_5.")]) -> None:
    """Print `set -gx ANDROID_SERIAL …` for the one ready device whose model matches;
    pipe it to `source`. Messages go to stderr, so only that line is sourced."""
    w = want.lower()
    hits = [ln for ln in devices_l()
            if " device " in ln and "model:" in ln and w in ln[ln.index("model:"):].lower()]
    if not hits:
        print(f"no ready device whose model matches {want!r}:", file=sys.stderr)
        for ln in devices_l():
            print(f"  {ln}", file=sys.stderr)
        raise typer.Exit(1)
    if len(hits) > 1:
        print(f"several connections match {want!r} (same phone over USB and wireless?);\n"
              "drop one with: adb disconnect <serial>", file=sys.stderr)
        for ln in hits:
            print(f"  {ln}", file=sys.stderr)
        raise typer.Exit(1)
    serial = hits[0].split()[0]
    print(f"ANDROID_SERIAL={serial}", file=sys.stderr)
    print(f"set -gx ANDROID_SERIAL {serial}")


@android.command("mark")
def android_mark() -> None:
    """Start a capture: marker file on the phone."""
    ht_bin()
    phone_mark()


@android.command("new")
def android_new() -> None:
    """Files saved since the mark (unfiltered, for diagnosis)."""
    for p in phone_new():
        print(p)


@android.command("latest")
def android_latest(n: Annotated[int, typer.Option("-n", help="How many.")] = 5) -> None:
    """Newest finished images by MediaStore date_added (trashed and pending excluded)."""
    out = sh(f'content query --uri {MEDIA} --projection _data:date_added:mime_type '
             f'--where "is_trashed=0 AND is_pending=0" --sort "date_added DESC"')
    for line in out.splitlines()[:n]:
        print(line)


@android.command("pull")
def android_pull(remote: Annotated[str, typer.Argument(help="Path on the phone.")],
                 name: Annotated[str, typer.Argument(help="File name in the stratum, with extension.")]) -> None:
    """Pull one file into the stratum, sha256-checked, then mark again."""
    dst = stratum() / Path(name).name
    if dst.exists():
        die(f"exists: {dst}", 1)
    if adb_pull_verified(remote, dst) is None:
        raise typer.Exit(1)
    ok_line(dst)
    phone_mark()


@android.command("grab")
def android_grab(stem: Stem, force: Force = False) -> None:
    """The one new image since the mark, as <stem>.<delivered ext>, then inspected."""
    ht_bin()
    found = [p for p in phone_new() if IMAGE.search(p)]
    # Hidden names are not finished photos: .trashed-… (in the bin), .pending-…
    # (still being written), .thumbnails/… (the gallery's cache).
    files = [p for p in found if "/." not in p]
    pending = [p for p in found if "/.pending-" in p]
    if not files:
        if pending:
            die(f"a photo is still being written ({pending[0]}); wait a moment and grab again", 1)
        die("no new image since the mark (check the save, or run `htc android latest`)", 1)
    if len(files) > 1:
        listing = "\n  ".join(files)
        die(f"more than one new image since the mark; pull the right one with "
            f"`htc android pull <remote> {stem}.<ext>`:\n  {listing}", 1)
    folder = stratum()
    refuse_existing(folder, stem, force, "htc android grab")
    print(f"found {files[0]}")
    stage = staging(folder, stem, ext_of(files[0]))
    if adb_pull_verified(files[0], stage) is None:
        raise typer.Exit(1)
    dst = install(stage, folder, stem)
    ok_line(dst)
    phone_mark()
    inspect(dst)


@android.command("type")
def android_type(words: Annotated[list[str], typer.Argument(help="The text; words are joined with single spaces.")],
                 send: Annotated[bool, typer.Option("-s", help="Press Enter after typing.")] = False) -> None:
    """Type text into the focused field on the phone.

    The text goes through a file on the phone, so its shell never re-parses the
    punctuation. `input text` takes plain ASCII only and reads %s as a space, so text
    with other characters, or with %s in it, is refused rather than typed wrong."""
    text = " ".join(words)
    if not text.isascii():
        odd = "".join(sorted({c for c in text if not c.isascii()}))
        die(f"`input text` takes plain ASCII only; not ASCII: {odd}", 2)
    if "%s" in text:
        die("the text contains %s, which `input text` turns into a space", 2)
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
        f.write(text)
        local = f.name
    try:
        adb("push", local, PROMPT_FILE)
    finally:
        os.unlink(local)
    sh(f'input text "$(sed "s/ /%s/g" {PROMPT_FILE})"')
    if send:
        time.sleep(0.5)
        sh("input keyevent KEYCODE_ENTER")


def bounds_center(b: str) -> tuple[int, int] | None:
    m = re.fullmatch(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]", b or "")
    if not m:
        return None
    x1, y1, x2, y2 = map(int, m.groups())
    return (x1 + x2) // 2, (y1 + y2) // 2


@android.command("send")
def android_send() -> None:
    """Tap the app's Send button: the first node whose content-desc contains "send"
    in a UI dump (resource-id as a fallback)."""
    sh(f"uiautomator dump {UI_DUMP}")
    raw = sh(f"cat {UI_DUMP}")
    try:
        nodes = list(ET.fromstring(raw[raw.find("<"):]).iter("node"))
    except ET.ParseError as e:
        die(f"could not read the UI dump ({e}); see: adb shell cat {UI_DUMP}", 1)
    for attr in ("content-desc", "resource-id"):
        for node in nodes:
            if "send" in node.get(attr, "").lower() and node.get("enabled", "true") == "true":
                xy = bounds_center(node.get("bounds", ""))
                if xy:
                    sh(f"input tap {xy[0]} {xy[1]}")
                    print(f"tapped {attr}={node.get(attr)!r} at {xy[0]},{xy[1]}")
                    return
    die(f"no send button in the UI dump (list labels: adb shell cat {UI_DUMP})", 1)


@android.command("range")
def android_range(
    day: Annotated[str, typer.Argument(help="YYYY-MM-DD")],
    start: Annotated[str | None, typer.Argument(help="HH:MM (default 00:00)")] = None,
    until: Annotated[str | None,
                     typer.Argument(help="HH:MM, or a duration like +45m / +2h (default: end of day)")] = None,
    out: Annotated[Path | None,
                   typer.Option("-o", "--out", help="Destination (default ~/halftone-candidates/<day>/).")] = None,
) -> None:
    """Pull every image taken in a time window as image1.<ext>, image2.<ext>, …,
    outside the corpus.

    Times are local, matched against the capture time Android records (MediaStore
    datetaken); trashed and unfinished files are excluded. Numbering continues after
    any imageN already in the folder; index.tsv records imageN, phone path and
    capture time. Every file is sha256-checked; the mark is not touched. A failed
    pull or mismatch still uses up its imageN, so numbering can have gaps; a
    mismatched copy stays in the folder without an index.tsv row."""
    try:
        d = date.fromisoformat(day)
        t0 = datetime.strptime(f"{day} {start or '00:00'}", "%Y-%m-%d %H:%M")
    except ValueError:
        die("usage: htc android range [-o dir] YYYY-MM-DD [HH:MM [HH:MM | +N(m|h)]]", 2)
    t_start = int(t0.timestamp())
    if until is None:
        # To midnight, via the next calendar day so DST days stay correct.
        t_end = int(datetime.combine(d + timedelta(days=1), datetime.min.time()).timestamp())
    elif m := re.fullmatch(r"\+(\d+)([mh])", until):
        t_end = t_start + int(m[1]) * (3600 if m[2] == "h" else 60)
    else:
        try:
            t_end = int(datetime.strptime(f"{day} {until}", "%Y-%m-%d %H:%M").timestamp())
        except ValueError:
            die(f"not a time or duration: {until} (use HH:MM, +45m or +2h)", 2)
    if t_end <= t_start:
        die("the window ends before it starts", 2)

    where = f"datetaken >= {t_start}000 AND datetaken < {t_end}000 AND is_trashed=0 AND is_pending=0"
    rows = sh(f'content query --uri {MEDIA} --projection _data:datetaken --where "{where}" --sort "datetaken ASC"')
    hits = re.findall(r"^Row: \d+ _data=(.*), datetaken=(\d+)$", rows, re.M)
    if not hits:
        fmt = "%Y-%m-%d %H:%M"
        die(f"no images taken between {datetime.fromtimestamp(t_start):{fmt}} "
            f"and {datetime.fromtimestamp(t_end):{fmt}}", 1)

    dest = (out or CANDIDATES / day).expanduser()
    dest.mkdir(parents=True, exist_ok=True)
    n = max((int(m[1]) for e in dest.glob("image*.*")
             if (m := re.match(r"^image(\d+)\.", e.name))), default=0)
    pulled = 0
    for path, taken_ms in hits:
        taken = datetime.fromtimestamp(int(taken_ms) // 1000).strftime("%Y-%m-%d %H:%M:%S")
        n += 1
        dst = dest / f"image{n}.{ext_of(path)}"
        if adb_pull_verified(path, dst, mismatch_to=dst) is None:
            continue
        with (dest / "index.tsv").open("a") as f:
            f.write(f"{dst.name}\t{path}\t{taken}\n")
        print(f"image{f'{n}.{ext_of(path)}':<4} {taken}  {posixpath.basename(path)}")
        pulled += 1
    print(f"{pulled} image(s) → {dest}")


# ---- desk -----------------------------------------------------------------------

DOWNLOADS = Path.home() / "Downloads"
DESK_MARK = Path.home() / ".ht-desk-marker"
UNFINISHED = (".crdownload", ".part", ".download")


def desk_mark() -> None:
    DESK_MARK.touch()


def desk_new() -> list[Path]:
    """Finished image downloads newer than the marker."""
    if not DESK_MARK.exists():
        die("run `htc desk mark` before downloading", 1)
    t = DESK_MARK.stat().st_mtime
    return sorted(p for p in DOWNLOADS.iterdir()
                  if p.is_file() and p.stat().st_mtime > t and p.name != ".DS_Store"
                  and not p.name.endswith(UNFINISHED) and IMAGE.search(p.name))


@desk.command("mark")
def desk_mark_cmd() -> None:
    """Start a capture: marker for ~/Downloads."""
    ht_bin()
    desk_mark()


@desk.command("new")
def desk_new_cmd() -> None:
    """Finished image downloads since the mark."""
    for p in desk_new():
        print(p)


@desk.command("take")
def desk_take(stem: Stem, force: Force = False) -> None:
    """Move the one new download into the stratum as <stem>.<delivered ext>, then
    inspect it."""
    ht_bin()
    files = desk_new()
    if not files:
        die("no new image in ~/Downloads since the mark (download still running?)", 1)
    if len(files) > 1:
        listing = "\n  ".join(map(str, files))
        die(f"more than one new image since the mark; move the right one by hand:\n  {listing}", 1)
    folder = stratum()
    refuse_existing(folder, stem, force, "htc desk take")
    print(f"found {files[0]}")
    stage = staging(folder, stem, ext_of(files[0].name))
    shutil.move(files[0], stage)
    dst = install(stage, folder, stem)
    ok_line(dst)
    inspect(dst)
    desk_mark()


@desk.command("tmp")
def desk_tmp(dest: Annotated[str, typer.Argument(
        help="Target path; any extension in it is replaced by the delivered one.")]) -> None:
    """Move the one new download to <dest>, outside the corpus."""
    files = desk_new()
    if len(files) != 1:
        listing = "\n  ".join(map(str, files))
        die(f"found {len(files)} new image(s) since the mark:\n  {listing}", 1)
    target = Path(re.sub(r"\.[A-Za-z0-9]+$", "", dest) + "." + ext_of(files[0].name)).expanduser()
    if target.exists():
        die(f"exists: {target}", 1)
    shutil.move(files[0], target)
    ok_line(target)
    desk_mark()


# ---- ios ------------------------------------------------------------------------

IOS_STILLS = (".jpg", ".jpeg", ".heic", ".heif", ".png", ".dng")
IOS_MARKS = Path.home() / ".cache" / "halftone"
IOS_CHUNK = 4 * 1024 * 1024
Device = Annotated[str | None, typer.Option("--device", envvar="HT_IOS",
                                           help="Part of the device name, model (iPhone14,2) or UDID.")]


class IosDev(NamedTuple):
    """One way to reach one device, as usbmuxd lists it and lockdown answers."""

    serial: str            # the UDID as usbmuxd spells it for this connection
    conn: str              # "USB" or "Network"
    info: dict             # lockdown short_info; a device answers this unpaired too
    paired: bool           # the device trusts this Mac and a session started
    wifi: bool | None      # EnableWifiConnections; None when it could not be read

    @property
    def key(self) -> str:
        """The UDID without the dash: Wi-Fi sync lists a device under that spelling."""
        return self.serial.replace("-", "")

    @property
    def name(self) -> str:
        return str(self.info.get("DeviceName") or self.info.get("ProductType") or self.serial)

    @property
    def kind(self) -> str:
        """`iPad`, `iPhone`: the word for the help lines."""
        return str(self.info.get("DeviceClass") or "device")


async def ios_probe(serial: str, conn: str) -> IosDev:
    """Ask one device who it is and whether it trusts this Mac. Never starts a pairing:
    an unpaired device would show its Trust dialog and this call would wait on it
    without a word."""
    from pymobiledevice3.exceptions import PyMobileDevice3Exception
    from pymobiledevice3.lockdown import create_using_usbmux

    try:
        async with await create_using_usbmux(serial=serial, connection_type=conn, autopair=False) as ld:
            wifi = None
            if ld.paired:
                try:
                    wifi = bool(await ld.get_enable_wifi_connections())
                except PyMobileDevice3Exception:
                    wifi = None
            return IosDev(serial, conn, dict(ld.short_info), bool(ld.paired), wifi)
    except (PyMobileDevice3Exception, OSError) as e:
        # Listed by usbmuxd but lockdown did not answer: gone to sleep, just unplugged.
        return IosDev(serial, conn, {"DeviceName": None, "Error": f"{type(e).__name__}: {e}".rstrip(": ")}, False, None)


async def ios_rows() -> list[IosDev]:
    """Every connection usbmuxd lists, probed: a device on a cable and on Wi-Fi is two
    rows."""
    from pymobiledevice3 import usbmux

    rows = [await ios_probe(d.serial, d.connection_type) for d in await usbmux.list_devices()]
    return sorted(rows, key=lambda r: (r.name.lower(), r.key, r.conn != "USB"))


async def ios_devices() -> list[IosDev]:
    """One row per device; where it is on a cable and on Wi-Fi, USB wins."""
    seen: dict[str, IosDev] = {}
    for r in await ios_rows():
        if r.key not in seen or r.conn == "USB":
            seen[r.key] = r
    return list(seen.values())


def ios_matches(dev: IosDev, want: str) -> bool:
    w = want.lower()
    return any(w in str(dev.info.get(k) or "").lower()
               for k in ("DeviceName", "ProductType", "DeviceClass", "UniqueDeviceID")) or w in dev.serial.lower()


async def ios_pick(want: str | None) -> tuple[str, dict]:
    devs = await ios_devices()
    if not devs:
        die("no iOS device: connect it by USB, or over Wi-Fi once `htc ios connect` has set that up", 1)
    if want:
        devs = [d for d in devs if ios_matches(d, want)]
        if not devs:
            die(f"no connected device matches {want!r} (see: htc ios devices)", 2)
    if len(devs) > 1:
        names = ", ".join(f"{d.name} ({d.info.get('ProductType')})" for d in devs)
        die(f"several devices connected ({names}): set HT_IOS or pass --device", 2)
    dev = devs[0]
    if not dev.paired:
        why = dev.info.get("Error") or "it has not trusted this Mac, or no longer does"
        die(f"{dev.name} is listed over {dev.conn} but cannot be used ({why}): "
            "plug in the cable and run `htc ios connect`", 1)
    return dev.serial, dev.info


async def with_afc(want: str | None, fn):
    from pymobiledevice3.lockdown import create_using_usbmux
    from pymobiledevice3.services.afc import AfcService

    udid, info = await ios_pick(want)
    async with await create_using_usbmux(serial=udid, autopair=False) as ld, AfcService(lockdown=ld) as afc:
        return await fn(udid, info, afc)


async def ios_roll(afc) -> list[str]:
    """Every file under /DCIM/<nnn>APPLE/, as device paths, sorted."""
    out = []
    for folder in sorted(await afc.listdir("/DCIM")):
        base = posixpath.join("/DCIM", folder)
        if await afc.isdir(base):
            out += [posixpath.join(base, f) for f in await afc.listdir(base)]
    return sorted(out)


def ios_mark_path(udid: str) -> Path:
    return IOS_MARKS / f"ios-mark-{udid}.json"


def ios_save_mark(udid: str, files: list[str]) -> None:
    import json

    IOS_MARKS.mkdir(parents=True, exist_ok=True)
    ios_mark_path(udid).write_text(json.dumps({"at": time.time(), "files": files}))


def ios_load_mark(udid: str) -> set[str]:
    import json

    p = ios_mark_path(udid)
    if not p.exists():
        die("no mark for this device: run `htc ios mark` before shooting", 1)
    return set(json.loads(p.read_text())["files"])


async def ios_settled_size(afc, src: str, wait: float = 1.0, tries: int = 10) -> int:
    """The file's size once two reads `wait` seconds apart agree."""
    last = (await afc.stat(src))["st_size"]
    for _ in range(tries):
        await asyncio.sleep(wait)
        now = (await afc.stat(src))["st_size"]
        if now == last and now > 0:
            return now
        last = now
    die(f"{src} is still changing size; wait for the camera to finish and retry", 1)


async def ios_read(afc, src: str, size: int, sink) -> tuple[bytes, str]:
    """Read `size` bytes of src, passing each chunk to `sink`; (sha1, sha256 hex)."""
    sha1, sha256 = hashlib.sha1(), hashlib.sha256()
    handle = await afc.fopen(src)
    try:
        for offset in range(0, size, IOS_CHUNK):
            chunk = await afc.fread(handle, min(IOS_CHUNK, size - offset))
            sink(chunk)
            sha1.update(chunk)
            sha256.update(chunk)
    finally:
        await afc.fclose(handle)
    return sha1.digest(), sha256.hexdigest()


async def ios_pull_verified(afc, src: str, dst: Path) -> str:
    """Copy via a temporary file; return the sha256. Checks: the size settled, the size
    did not change while reading, and the SHA-1 the device computes over its own copy
    matches the bytes received. pymobiledevice3 before 11.50 cannot ask the device for
    a hash; then the file is read a second time and the two reads must agree. A
    failed copy goes to /tmp/ht-mismatch/."""
    size = await ios_settled_size(afc, src)
    tmp = dst.with_name(f".{dst.name}.part")
    try:
        with tmp.open("wb") as f:
            sha1, sha256 = await ios_read(afc, src, size, f.write)
    except BaseException:
        tmp.unlink(missing_ok=True)
        raise
    got = tmp.stat().st_size
    after = (await afc.stat(src))["st_size"]
    problem = None
    if got != size or after != size:
        problem = f"size: device {size} before, {after} after, received {got}"
    elif hasattr(afc, "get_file_hash"):
        try:
            device_sha1 = await afc.get_file_hash(src)
        except Exception as e:  # an iOS version that refuses the request
            print(f"{C.YELLOW}device hash unavailable ({e}); size check only{C.RESET}")
            device_sha1 = None
        if device_sha1 is not None and device_sha1 != sha1:
            problem = "SHA-1 of the received bytes differs from the device's"
    else:
        _, again = await ios_read(afc, src, size, lambda _chunk: None)
        if again != sha256:
            problem = "two reads of the file returned different bytes"
        else:
            print(f"{C.DIM}checked by reading twice (device hash needs pymobiledevice3 11.50+){C.RESET}")
    if problem:
        kept = keep_bad(tmp, dst.name)
        die(f"{src}: {problem}; copy kept at {kept}, nothing written to the stratum", 1)
    os.replace(tmp, dst)
    return sha256


@ios.command("devices")
def ios_devices_cmd() -> None:
    """Connected devices: name, model, iOS version, USB or Network, UDID. A device
    that has not trusted this Mac is listed and marked; `htc ios connect` fixes that."""
    async def go():
        for d in await ios_devices():
            i = d.info
            mark = "" if d.paired else f"  {C.YELLOW}not trusted: htc ios connect{C.RESET}"
            print(f"{d.name:<24} {i.get('ProductType')!s:<14} "
                  f"iOS {i.get('ProductVersion')!s:<8} {d.conn:<8} {d.serial}{mark}")
    asyncio.run(go())


# ---- ios connect: trust, Wi-Fi connections, off the cable ------------------------

IOS_USB_WAIT = 120     # seconds to wait for a cable connection to show up
IOS_TRUST_WAIT = 180   # seconds to wait for Trust to be tapped
IOS_WIFI_WAIT = 90     # seconds to wait for the device to appear on the network
IOS_RESET_HINT = ("Settings > General > Transfer or Reset > Reset > Reset Location & Privacy "
                  "(forgets every trusted computer, and resets the apps' location and privacy permissions)")


def hint(text: str) -> None:
    """Where to look and what to do while this script waits."""
    print(f"  {C.YELLOW}->{C.RESET} {text}", flush=True)


def done(text: str) -> None:
    print(f"  {C.GREEN}ok{C.RESET}  {text}", flush=True)


def ios_advice(rows: list[IosDev]) -> str:
    """What one device needs, from all the ways it is listed."""
    usb = next((r for r in rows if r.conn == "USB"), None)
    net = next((r for r in rows if r.conn != "USB"), None)
    if net and net.paired:
        return "ready over Wi-Fi: mark, grab and pull work without the cable"
    if net and not net.paired and not (usb and usb.paired):
        return ("seen on the network but it does not trust this Mac any more: pair again, "
                "with the cable in: htc ios connect")
    if usb and not usb.paired:
        if usb.info.get("Error"):
            return f"on the cable but not answering ({usb.info['Error']}): unlock it, replug, try again"
        return "on the cable, not trusted yet: htc ios connect"
    if usb and usb.wifi is False:
        return "trusted, Wi-Fi connections off: htc ios connect"
    if usb and usb.wifi:
        return ("trusted and allowed over Wi-Fi; works on the cable now. Unplug and it should "
                "appear as Network within a minute if it is awake and on this Mac's Wi-Fi")
    if usb:
        return "trusted; the Wi-Fi setting could not be read: htc ios connect"
    return "listed, state unknown"


def ios_table(rows: list[IosDev]) -> None:
    by_dev: dict[str, list[IosDev]] = {}
    for r in rows:
        by_dev.setdefault(r.key, []).append(r)
    for group in by_dev.values():
        for r in group:
            trust = "trusted" if r.paired else "not trusted"
            wifi = {True: "Wi-Fi connections on", False: "Wi-Fi connections off", None: ""}[r.wifi]
            print(f"{r.name:<24} {r.info.get('ProductType')!s:<14} iOS {r.info.get('ProductVersion')!s:<8} "
                  f"{r.conn:<8} {trust:<12} {wifi}".rstrip())
        hint(ios_advice(group))


async def ios_wait_usb(want: str | None) -> list[IosDev]:
    """The devices on a cable, waiting for one when there is none yet."""
    from pymobiledevice3 import usbmux

    said = False
    for _ in range(IOS_USB_WAIT):
        if any(d.is_usb for d in await usbmux.list_devices()):
            break
        if not said:
            print("No iPhone or iPad on a cable yet.")
            hint("Plug it in and unlock it to the Home Screen.")
            hint('On this Mac: if a dialog asks "Allow accessory to connect?", click Allow.')
            hint(f"Waiting up to {IOS_USB_WAIT} s (Ctrl-C stops) ...")
            said = True
        await asyncio.sleep(1)
    else:
        die("no device showed up on USB. In the order to check: the Allow dialog on this Mac; "
            "the cable (some only charge); the port or hub (plug straight into the Mac); "
            "then unplug, unlock the device, plug in again", 1)
    if said:
        await asyncio.sleep(1)  # lockdown answers a moment after usbmuxd lists the device
    rows = [r for r in await ios_rows() if r.conn == "USB"]
    if want:
        rows = [r for r in rows if ios_matches(r, want)]
        if not rows:
            die(f"no device on a cable matches {want!r} (see: htc ios connect --check)", 2)
    return rows


def ios_choose(rows: list[IosDev]) -> list[IosDev]:
    """One device goes ahead. Several are offered as a list to pick from with the arrow
    keys; without a terminal there is nobody to ask, so the option is named instead."""
    if len(rows) == 1:
        return rows
    if not sys.stdin.isatty():
        names = ", ".join(r.name for r in rows)
        die(f"several devices on cables ({names}) and no terminal to ask in: pass --device", 2)
    import inquirer  # only needed when there is a choice to make

    def label(r: IosDev) -> str:
        trust = "trusted" if r.paired else "not trusted"
        return f"{r.name}  ({r.info.get('ProductType')}, iOS {r.info.get('ProductVersion')}, {trust})"

    everything = "All of them, one after the other"
    choices = [(label(r), r.key) for r in rows] + [(everything, "")]
    answer = inquirer.prompt([inquirer.List("device", message="Which device", choices=choices, carousel=True)])
    if not answer:  # Ctrl-C in the list
        die("stopped; nothing was changed", 1)
    picked = answer["device"]
    return rows if picked == "" else [r for r in rows if r.key == picked]


def ios_agree(question: str, yes: bool, default: bool) -> bool:
    """A yes/no question before a setting changes. `--yes` answers it; without a
    terminal the option is named instead of waiting on a prompt nobody sees."""
    if yes:
        return True
    if not sys.stdin.isatty():
        die("this changes a setting and there is no terminal to ask in: pass --yes", 2)
    return typer.confirm(question, default=default)


async def ios_trust(dev: IosDev) -> None:
    """Pair with a device on a cable, saying where to look while it waits."""
    from pymobiledevice3.exceptions import (
        PairingDialogResponsePendingError,
        PasswordRequiredError,
        PyMobileDevice3Exception,
        UserDeniedPairingError,
    )
    from pymobiledevice3.lockdown import create_using_usbmux

    print(f"{dev.name} has not trusted this Mac yet.")
    hint(f"On the {dev.kind}: unlock it, tap Trust on the dialog, then enter its passcode.")
    hint(f"Waiting up to {IOS_TRUST_WAIT} s for that (Ctrl-C stops) ...")
    deadline = time.monotonic() + IOS_TRUST_WAIT
    said_locked = False
    while True:
        left = deadline - time.monotonic()
        if left <= 0:
            die(f"no answer on {dev.name} in {IOS_TRUST_WAIT} s. If no dialog was showing: unplug, "
                f"unlock the {dev.kind}, plug in and run this again. If it still never asks, reset "
                f"its trust decisions, on the {dev.kind}: {IOS_RESET_HINT}", 1)
        try:
            async with await create_using_usbmux(serial=dev.serial, connection_type="USB", autopair=False) as ld:
                if ld.paired:
                    return
                try:
                    await ld.pair(timeout=left)
                except PasswordRequiredError:
                    if not said_locked:
                        hint(f"The {dev.kind} is locked: unlock it, the dialog comes after that.")
                        said_locked = True
                    await asyncio.sleep(2)
                    continue
                except PairingDialogResponsePendingError:
                    continue  # the deadline above ends it with the full message
                except UserDeniedPairingError:
                    die(f"Don't Trust was tapped on {dev.name}. Unplug the cable, plug it in again "
                        "and run this again: the dialog returns on a new connection", 1)
                if not await ld.validate_pairing():
                    die(f"{dev.name} accepted the pairing but no session started: unplug, plug in, run this again", 1)
                return
        except (PyMobileDevice3Exception, OSError) as e:
            what = f"{type(e).__name__}: {e}".rstrip(": ")
            die(f"pairing with {dev.name} failed ({what}): check the cable and run this again; "
                "if it fails the same way twice, `htc ios connect --forget` starts over", 1)


async def ios_allow_wifi(dev: IosDev, yes: bool) -> None:
    from pymobiledevice3.lockdown import create_using_usbmux

    async with await create_using_usbmux(serial=dev.serial, connection_type="USB", autopair=False) as ld:
        if await ld.get_enable_wifi_connections():
            done("Wi-Fi connections were already on")
            return
        if not ios_agree(f"Let this Mac reach {dev.name} over Wi-Fi (the device setting "
                         "EnableWifiConnections)?", yes, default=True):
            die("left off: the device stays reachable on the cable only", 1)
        await ld.set_enable_wifi_connections(True)
        if not await ld.get_enable_wifi_connections():
            die(f"{dev.name} did not keep the setting. Finder has the same switch: select the device, "
                f"General, 'Show this {dev.kind} when on Wi-Fi', Apply", 1)
        done("Wi-Fi connections turned on")


async def ios_wait_network(dev: IosDev) -> bool:
    """Wait until the device answers over Wi-Fi. False, with the reason printed, when
    it does not: the caller goes on to the next device."""
    from pymobiledevice3 import usbmux

    def failed(msg: str) -> bool:
        print(f"  {C.RED}no{C.RESET}  {msg}", flush=True)
        return False

    async def state() -> tuple[bool, str | None]:
        usb, net = False, None
        for d in await usbmux.list_devices():
            if d.serial.replace("-", "") == dev.key:
                if d.is_usb:
                    usb = True
                else:
                    net = d.serial
        return usb, net

    usb, net = await state()
    if not net:
        if usb:
            hint(f"Unplug the cable from the {dev.kind} now.")
        hint(f"Keep the {dev.kind} unlocked and on the same Wi-Fi network as this Mac.")
        hint(f"Waiting up to {IOS_WIFI_WAIT} s for it to appear on the network (Ctrl-C stops) ...")
        for _ in range(IOS_WIFI_WAIT):
            await asyncio.sleep(1)
            usb, net = await state()
            if net:
                break
        else:
            still = " It is still on the cable, where it works: unplug it to use Wi-Fi." if usb else ""
            return failed(
                f"{dev.name} did not appear on the network.{still} It is trusted and allowed, so "
                "nothing needs repeating. In the order to check: the device is awake and unlocked; "
                "both are on the same Wi-Fi network (not a guest network, which keeps devices apart); "
                "a VPN or firewall on this Mac. Then: htc ios connect --check")
    over = await ios_probe(net, "Network")
    if not over.paired:
        return failed(f"{dev.name} is on the network but did not start a session there "
                      f"({over.info.get('Error') or 'not trusted over Wi-Fi'}): "
                      "run `htc ios connect --check` in a moment")
    tail = " You can unplug the cable." if usb else ""
    done(f"{dev.name} answers over Wi-Fi.{tail} `htc ios mark`, `grab` and `pull` work without the cable.")
    return True


async def ios_forget(dev: IosDev, yes: bool) -> None:
    from pymobiledevice3.lockdown import create_using_usbmux

    if not dev.paired:
        print(f"{dev.name} does not trust this Mac, so there is nothing to remove from here.")
        hint(f"If its Trust dialog never appears, reset it on the {dev.kind}: {IOS_RESET_HINT}")
        return
    if not ios_agree(f"Remove the trust between this Mac and {dev.name}? It will ask to Trust "
                     "again next time", yes, default=False):
        die("nothing removed", 1)
    async with await create_using_usbmux(serial=dev.serial, connection_type="USB", autopair=False) as ld:
        await ld.unpair()
    done(f"{dev.name} no longer trusts this Mac. Unplug it, plug it in, and run `htc ios connect`.")


@ios.command("connect")
def ios_connect(device: Device = None,
                check: Annotated[bool, typer.Option(
                    "--check", help="Only show each device's state and what it needs; change nothing.")] = False,
                forget: Annotated[bool, typer.Option(
                    "--forget", help="Remove the trust between this Mac and the device, to start over.")] = False,
                yes: Annotated[bool, typer.Option("--yes", "-y", help="Do not ask before changing a setting.")] = False,
                ) -> None:
    """Get a device from "plugged in" to "works without the cable": wait for it on USB,
    have it trust this Mac, allow Wi-Fi connections, and wait until it answers over
    Wi-Fi. Each wait says where to look. Steps already done are skipped, so it is safe
    to run again.

    Pair again when `--check` says a device is not trusted. That happens after its
    trust decisions were reset or it was erased or restored; a device may also drop a
    computer it has not connected to for a long time (not measured here). Reset (`--forget`, and the device's own
    "Reset Location & Privacy") only when the Trust dialog never appears on an unlocked
    device, or pairing fails right after Trust was tapped."""
    async def go():
        if check:
            rows = await ios_rows()
            if device:
                rows = [r for r in rows if ios_matches(r, device)]
            if not rows:
                print("No iPhone or iPad on a cable or on the network.")
                hint("For a first connection: plug it in, unlock it, and run `htc ios connect`.")
                hint("For one that worked before: wake and unlock it, same Wi-Fi as this Mac, then try again.")
                return
            ios_table(rows)
            return
        def title(dev: IosDev) -> None:
            print(f"{C.BOLD_CYAN}{dev.name}{C.RESET}  ({dev.info.get('ProductType')}, "
                  f"iOS {dev.info.get('ProductVersion')})")

        # Nothing on a cable, but already answering over Wi-Fi: there is nothing to do,
        # and waiting two minutes for a cable would say the opposite.
        rows = [r for r in await ios_rows() if not device or ios_matches(r, device)]
        if rows and all(r.conn != "USB" for r in rows):
            ios_table(rows)
            if not forget and any(r.paired for r in rows):
                return
        chosen = ios_choose(await ios_wait_usb(device))
        # Everything that needs the cable first, for every device, then the unplugging.
        for dev in chosen:
            title(dev)
            if dev.info.get("Error"):
                die(f"{dev.name} is on the cable but not answering ({dev.info['Error']}): "
                    "unlock it, replug, run this again", 1)
            if forget:
                await ios_forget(dev, yes)
                continue
            if not dev.paired:
                await ios_trust(dev)
            done("trusts this Mac")
            await ios_allow_wifi(dev, yes)
        if forget:
            return
        reached = []
        for dev in chosen:
            if len(chosen) > 1:
                title(dev)
            reached.append(await ios_wait_network(dev))
        if not all(reached):
            sys.exit(1)
    from pymobiledevice3.exceptions import ConnectionFailedToUsbmuxdError, PyMobileDevice3Exception

    try:
        asyncio.run(go())
    except KeyboardInterrupt:
        die("stopped; nothing is left half-done, run it again when ready", 1)
    except ConnectionFailedToUsbmuxdError:
        die("this Mac's device service (usbmuxd) is not answering. It is part of macOS and restarts "
            "on its own: unplug the device, run `sudo killall usbmuxd`, plug it in, try again", 1)
    except PyMobileDevice3Exception as e:
        die(f"the device stopped answering ({type(e).__name__}: {e}); check the cable or the Wi-Fi "
            "and run this again: steps already done are skipped", 1)


@ios.command("mark")
def ios_mark(device: Device = None) -> None:
    """Remember the camera roll's file names on this Mac (nothing is written to the
    device)."""
    ht_bin()

    async def go(udid, info, afc):
        files = await ios_roll(afc)
        ios_save_mark(udid, files)
        print(f"marked {info.get('DeviceName')}: {len(files)} files in the camera roll")
    asyncio.run(with_afc(device, go))


@ios.command("new")
def ios_new(device: Device = None) -> None:
    """Files added to the camera roll since the mark."""
    async def go(udid, info, afc):
        new = [p for p in await ios_roll(afc) if p not in ios_load_mark(udid)]
        if not new:
            print("nothing new since the mark")
        for p in new:
            st = await afc.stat(p)
            print(f"{p:<40} {st['st_size']:>12,}  {st['st_mtime']:%Y-%m-%d %H:%M:%S}")
    asyncio.run(with_afc(device, go))


@ios.command("grab")
def ios_grab(stem: Stem, force: Force = False, device: Device = None,
             pick: Annotated[str | None, typer.Option(
                 "--pick", help="File to take when several stills are new (IMG_0042.HEIC).")] = None) -> None:
    """The one new still since the mark, as <stem>.<delivered ext>, then inspected. A
    Live Photo's MOV and edit sidecars (AAE) are listed, not pulled."""
    ht_bin()
    folder = stratum()
    if "__afc-pull__" not in stem:
        print(f"{C.YELLOW}stem has no __afc-pull__ transfer token; check the name{C.RESET}")

    async def go(udid, info, afc):
        files = await ios_roll(afc)
        new = [p for p in files if p not in ios_load_mark(udid)]
        stills = [p for p in new if posixpath.splitext(p)[1].lower() in IOS_STILLS]
        companions = [p for p in new if p not in stills]
        if pick:
            stills = [p for p in stills if posixpath.basename(p) == pick]
        if len(stills) != 1:
            listing = "\n  ".join(stills) or "(none)"
            die(f"{len(stills)} new stills since the mark (want exactly one; --pick NAME):\n  {listing}", 1)
        src = stills[0]
        refuse_existing(folder, stem, force, "htc ios grab")
        print(f"found {src}")
        stage = staging(folder, stem, ext_of(src))
        sha = await ios_pull_verified(afc, src, stage)
        dst = install(stage, folder, stem)
        ios_save_mark(udid, files)
        print(f"ok  {dst}  {mime(dst)}  sha256 {sha}")
        stem_src = posixpath.splitext(src)[0]
        for c in companions:
            same = " (same capture)" if posixpath.splitext(c)[0] == stem_src else ""
            print(f"{C.DIM}skipped {c}{same}{C.RESET}")
        return dst

    inspect(asyncio.run(with_afc(device, go)))


@ios.command("pull")
def ios_pull(remote: Annotated[str, typer.Argument(help="Device path, e.g. /DCIM/100APPLE/IMG_0042.JPG.")],
             dest: Annotated[Path, typer.Argument(help="Local file or folder (outside the corpus).")],
             force: Annotated[bool, typer.Option("-f", "--force", help="Overwrite.")] = False,
             device: Device = None) -> None:
    """Any device file, verified, to a local path."""
    dst = dest.expanduser()
    if dst.is_dir():
        dst = dst / posixpath.basename(remote)
    if dst.exists() and not force:
        die(f"exists: {dst} (-f overwrites)", 1)
    old_sha = sha256_of(dst) if dst.is_file() else None

    async def go(udid, info, afc):
        sha = await ios_pull_verified(afc, remote, dst)
        print(f"ok  {dst}  {mime(dst)}  sha256 {sha}")
        if sha == old_sha:
            warn_same(dst.name, sha)
    asyncio.run(with_afc(device, go))


# ---- layout ---------------------------------------------------------------------


@app.command("layout")
def layout(files: Annotated[list[Path], typer.Argument(help="Images to describe.")]) -> None:
    """Per image: size, the parts its own metadata declares, and the c2pa status.

    Parts come from the container directory (Motion Photo / Ultra HDR) and, when there
    is none, the MPF index (iPhone gain maps), so a multi-part file without a
    directory is no longer called a single JPEG."""
    ht = ht_bin()
    need("exiftool")
    imgs = [f for f in files if LAYOUT_IMAGE.search(f.name) and f.is_file()]
    with ExifTool() as et:
        rows = et.json("-XMP-GContainer:DirectoryItemSemantic", "-MPF:NumberOfImages", *map(str, imgs)) if imgs else []
    meta = {str(Path(r.get("SourceFile", ""))): r for r in rows}
    for f in imgs:
        r = meta.get(str(f), {})
        items = r.get("DirectoryItemSemantic")
        n_mpf = r.get("NumberOfImages")
        if items:
            parts = ", ".join(map(str, items)) if isinstance(items, list) else str(items)
        elif n_mpf:
            parts = f"MPF: {n_mpf} images"
        else:
            parts = "single JPEG" if re.search(r"\.jpe?g$", f.name, re.I) else "no container"
        status = "?"
        if ht:
            import json

            out = subprocess.run([ht, "inspect", "--json", "--only", "manifest", str(f)],
                                 capture_output=True, text=True).stdout
            try:
                status = next((e["status"] for e in json.loads(out)["evidence"]
                               if isinstance(e.get("source"), dict) and e["source"].get("name") == "c2pa"), "?")
            except (json.JSONDecodeError, KeyError, TypeError):
                pass
        print(f"{f.name:<50} {f.stat().st_size:>9} B  {parts:<30} c2pa={status}")


if __name__ == "__main__":
    from complete import serve  # tab completion (lib/complete.py); returns unless asked

    serve(app)
    app()
