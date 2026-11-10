#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27", "pymobiledevice3>=11.45,<12"]
#
# [tool.uv]
# exclude-newer = "14 days"  # only releases at least two weeks old
# ///
"""Bring files from outside the repo into a corpus stratum: Android phones over adb,
iPhones and iPads over AFC, and browser downloads from ~/Downloads.

Every command that writes into the corpus needs the target stratum:

    set -gx HT_STRATUM 06-phones

One of 00-base, 04-generators, 05-fold, 06-phones, 07-editors, 10-messaging,
11-screenshots (08-multipart is built by make-multipart-stratum.fish). Without it
they refuse: a missing value once filed phone shots under 04. Files keep
the extension they were delivered with (SOURCE.md naming rule). `-f` on grab and take
replaces an existing file of the same stem, under any extension, once the new copy
has passed its check, and keeps the old copy in /tmp/ht-replaced/ (cleared on
restart).

Every copy is checked before it gets its name: Android pulls by the sha256 the phone
computes, iOS pulls by the size before and after reading plus the SHA-1 the device
computes (pymobiledevice3 11.50 or newer; older versions cannot ask for it, and the
file is read twice and the two reads compared instead). A copy that fails the check
never lands in the stratum; it is kept in /tmp/ht-mismatch/ for inspection.

Needs adb for `android`, exiftool for `layout`, ht for the inspection after a grab or
take. Offline. Never reads the clipboard. Exit 0 done, 1 refused or failed, 2 usage.
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
from typing import Annotated

import typer

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from exiftool import ExifTool  # noqa: E402
from repo import CORPUS, C, die, need  # noqa: E402

EXAMPLES = """
Examples (fish, with `htc` defined as in scripts/README.md):

    htc android use fold | source            # this terminal's phone
    htc android mark                         # before saving on the phone
    htc android type "A ceramic bowl of …"   # into the focused field
    htc android send                         # tap the app's Send button
    htc android grab gemini-app__nano-banana__app-save-open__p1__2
    htc android range 2026-11-09 14:00 +45m  # candidates, outside the corpus
    htc desk mark
    htc desk take -f firefly__image-4__web-download__p1__1
    htc ios mark
    htc ios grab iphone13__default__afc-pull__s01__1
    htc layout corpus/differential/06-phones/*.jpg
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
    its check, so a failed copy never costs the old file its place."""
    old = existing(folder, stem)
    if old:
        REPLACED.mkdir(parents=True, exist_ok=True)
        stamp = time.strftime("%Y%m%d-%H%M%S")
        for o in old:
            kept = REPLACED / f"{stamp}-{o.name}"
            shutil.move(o, kept)
            print(f"replaced: {o} (old copy: {kept})")
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


def mime(p: Path) -> str:
    r = subprocess.run(["file", "-b", "--mime-type", str(p)], capture_output=True, text=True)
    return r.stdout.strip() or "?"


def ok_line(p: Path) -> None:
    print(f"ok  {p}  {mime(p)}")


def inspect(p: Path) -> None:
    if shutil.which("ht") is None:
        print(f"{C.DIM}(ht not on PATH; not inspected){C.RESET}")
        return
    subprocess.run(["ht", "inspect", str(p)])


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


async def ios_devices() -> list[tuple[str, str, dict]]:
    """(udid, connection, lockdown short_info) per device. Wi-Fi sync can list a device
    a second time, under its UDID without the dash; USB wins."""
    from pymobiledevice3 import usbmux
    from pymobiledevice3.lockdown import create_using_usbmux

    seen: dict[str, tuple[str, str]] = {}
    for d in await usbmux.list_devices():
        key = d.serial.replace("-", "")
        if key not in seen or d.is_usb:
            seen[key] = (d.serial, d.connection_type)
    out = []
    for serial, conn in seen.values():
        async with await create_using_usbmux(serial=serial) as ld:
            out.append((serial, conn, ld.short_info))
    return out


async def ios_pick(want: str | None) -> tuple[str, dict]:
    devs = await ios_devices()
    if not devs:
        die("no iOS device: connect it by USB (or Wi-Fi sync), unlock it, trust this Mac", 1)
    if want:
        w = want.lower()
        devs = [d for d in devs if any(w in str(d[2].get(k) or "").lower()
                                       for k in ("DeviceName", "ProductType", "DeviceClass", "UniqueDeviceID"))]
        if not devs:
            die(f"no connected device matches {want!r} (see: htc ios devices)", 2)
    if len(devs) > 1:
        names = ", ".join(f"{d[2].get('DeviceName')} ({d[2].get('ProductType')})" for d in devs)
        die(f"several devices connected ({names}): set HT_IOS or pass --device", 2)
    return devs[0][0], devs[0][2]


async def with_afc(want: str | None, fn):
    from pymobiledevice3.lockdown import create_using_usbmux
    from pymobiledevice3.services.afc import AfcService

    udid, info = await ios_pick(want)
    async with await create_using_usbmux(serial=udid) as ld, AfcService(lockdown=ld) as afc:
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
    """Connected devices: name, model, iOS version, USB or Network, UDID."""
    async def go():
        for udid, conn, i in await ios_devices():
            print(f"{i.get('DeviceName')!s:<24} {i.get('ProductType')!s:<14} "
                  f"iOS {i.get('ProductVersion')!s:<8} {conn:<8} {udid}")
    asyncio.run(go())


@ios.command("mark")
def ios_mark(device: Device = None) -> None:
    """Remember the camera roll's file names on this Mac (nothing is written to the
    device)."""
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

    async def go(udid, info, afc):
        sha = await ios_pull_verified(afc, remote, dst)
        print(f"ok  {dst}  {mime(dst)}  sha256 {sha}")
    asyncio.run(with_afc(device, go))


# ---- layout ---------------------------------------------------------------------


@app.command("layout")
def layout(files: Annotated[list[Path], typer.Argument(help="Images to describe.")]) -> None:
    """Per image: size, the parts its own metadata declares, and the c2pa status.

    Parts come from the container directory (Motion Photo / Ultra HDR) and, when there
    is none, the MPF index (iPhone gain maps), so a multi-part file without a
    directory is no longer called a single JPEG."""
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
        if shutil.which("ht"):
            import json

            out = subprocess.run(["ht", "inspect", "--json", "--only", "manifest", str(f)],
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
