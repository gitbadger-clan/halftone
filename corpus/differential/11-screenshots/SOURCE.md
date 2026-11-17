# 11-screenshots — provenance

Screen captures, written by the operating system's own screenshot tool. A
screenshot is a new file made from pixels on a display, so nothing embedded in
what was on screen can reach it. The stratum records what each system writes
instead, and holds one row per device where the thing on screen was a signed
image. Files are not committed; this file and `expectations.json` are. Names:
`<device>__<what was captured>__<tool>__<subject>__<n>.<ext>`.

Collected with `scripts/htc.py` (`HT_STRATUM=11-screenshots`).

## macOS — 27.2.1, 2026-11-13 (`s03` added 2026-11-16)

Taken with `/usr/sbin/screencapture -x` into `~/Downloads`, brought in with
`htc desk take`. Three displays were attached. The full-screen and region
captures come from the third, a 1080×1920 display at 1× that was kept empty:
they show the wallpaper, a coastal photograph, with no menu bar, window or icon
in the frame. The window captures are at 2×.

`screencapture -R` takes its rectangle in global coordinates, measured from the
top-left of the main display across the whole arrangement, and `-D` does not
change that. The region row was aimed at the empty display by moving the
coordinates over to it.

| File | Command | Size | On screen |
|---|---|---|---|
| `macos__fullscreen__screencapture__s01__1.png` | `screencapture -x -D <n>` | 1080×1920, 3,060,650 B | the whole display: wallpaper only |
| `macos__region__screencapture__s02__1.png` | `screencapture -x -R <x,y,w,h>` | 1080×757 | the lower part of the same wallpaper |
| `macos__window__screencapture__s03__1.png` | `screencapture -x -w` | 2296×2324 | a Brave browser window on its New Tab page, with the window shadow |
| `macos__window-marked__screencapture__s04__1.png` | `screencapture -x -w` | 3608×2306 | a Preview window showing `04-generators/gemini-app__nano-banana__web-download__p1__1.jpg`, a signed image; the title bar carries that file name |
| `macos__fullscreen-jpg__screencapture__s05__1.jpg` | `screencapture -x -t jpg` | 1080×1920, 279,349 B | the whole display again, as JPEG |

## Pixel 9 Pro Fold — Android 17, 2026-11-16

Taken with the hardware shortcut (power + volume down) on the outer display,
saved by the system to `Pictures/Screenshots/`, pulled with `htc android grab`
over wireless adb.

The status bar was put in **System UI demo mode** for the captures, so its
clock (12:00) and battery (100) are Android's placeholders, not the device's
state. Do Not Disturb was on and notifications were cleared; one ongoing
notification icon remains in three files. The web row was not retaken after the
microphone indicator was noticed in it.

| File | Size | On screen |
|---|---|---|
| `pixel9fold__home__screenshot__s01__1.png` | 1080×2424 | home screen: stock wallpaper, stock apps, date and weather widget |
| `pixel9fold__app__screenshot__s02__1.png` | 1080×2424, 150,996 B | Calculator, dark theme |
| `pixel9fold__web__screenshot__s03__1.png` | 1080×2424, 434,808 B | Chrome on the English Wikipedia article "Renaissance". Its status bar shows Android's microphone-in-use indicator where the other files show Wi-Fi and battery |
| `pixel9fold__photo-marked__screenshot__s04__1.png` | 1080×2424 | `06-phones/pixel9fold__tele5x__adb-pull__s08__1.jpg` full screen in Google Photos, controls hidden |
| `pixel9fold__scrolling__screenshot__s05__1.png` | 1080×6228 | Settings → Accessibility, extended with "Capture more" to about 2.6 screens; no status bar |

"Capture more" was not offered in Chrome; it was in Settings.

## Findings

- No file carries a C2PA manifest (10 of 10).
- The two marked rows are the point of the stratum. On the Fold, the image on
  screen is a camera capture with a trusted Google manifest declaring
  computationalCapture; the screenshot of it has no manifest and no metadata
  that says so. On macOS the same holds for the signed image shown in Preview.
  A signed file becomes an unsigned one by being displayed.
- macOS PNG: `png_writer` reads the chunk layout as a macOS screenshot (Apple
  ImageIO, XMP `UserComment=Screenshot`). The rule was written from the
  documented layout and marked "not yet verified"; these four files were its
  first real samples, and it is marked verified since 2026-11-17. The XMP
  holds none of the rights, identifier, vendor-flag or data-mining fields
  (`ht inspect --metadata`).
- Fold PNG: the chunk layout is that of Skia's PNG encoder (sBIT and sRGB,
  8 KiB blocks), a rule measured on a Skia canvas binding. No XMP. So by layout
  alone an Android screenshot is not told apart from any other Skia-written
  PNG, and at collection time `png_writer` named Skia. Since 2026-11-17 a
  second rule asks for that layout together with the EXIF `Software` tag
  described below, and all five files read as Android's system screenshot
  tool. The tag is unsigned; the rule needs the layout as well.
- macOS JPEG: no XMP, but an IPTC IIM block; non-standard quantization tables,
  4:2:0, default Huffman tables; not in the writer fingerprint DB.
- The Fold PNG carries an EXIF block (`eXIf`). Read on the Mac with its own
  ExifTool 13.55, a separate look outside the reference image, whose pinned
  version is the one in the `expectations.json` header:
  `Software = Android CP3A.261005.005` (the system build), the capture time to
  the millisecond with its UTC offset, and an `ImageUniqueID` that is a random
  UUID. This is what tells an Android system screenshot from any other
  Skia-written PNG. The capture time is the real one: the image shows the
  demo-mode 12:00, the file says when it was taken and in which time zone.
- The macOS JPEG carries EXIF `UserComment = Screenshot`, an empty IPTC IIM
  block (its digest is the MD5 of nothing) and the ICC profile of the display
  it was taken on. That profile is named after the monitor model and dated, so
  a macOS screenshot says which display it came from.
- The macOS PNGs carry a display profile too. In all four the `iCCP` chunk
  itself is named `ICC Profile`, a fixed keyword; the display's name is the
  description inside the compressed profile. The five files hold three
  different descriptions: one monitor's model on the three captures from the
  1× display, a second monitor's model on the plain window capture (`s03`),
  and the generic name `Display` on the marked window capture (`s04`), taken
  three days before `s03`. So the name is not always a model, and captures from
  one machine do not all carry the same one. With three displays attached, the
  three names may simply be one per display; which window was on which was not
  noted. The models are not recorded here.
- Since 2026-11-17 `embedded_identifiers` lists the `ImageUniqueID` of all
  five Fold screenshots as a document identifier, masked unless `--reveal`,
  and `png_writer` quotes the `Software` tag. Still not reported: no source
  reads the ICC profile description. (`exif_consistency` is right to say no
  generator is named: an Android build is not one.)
- No Halftone source prints the display's name, because none reads it.
  `png_writer` reads the chunk's name, not the profile inside it, and the JPEG
  and WebP sources record only whether a profile is present. Checked on these
  files 2026-11-16: neither monitor name occurs anywhere in the output of
  `ht inspect --batch --json --reveal`.
- All files inspected read as dark-mode images to `pixel_lattice` (macOS PNG
  0.0207, Fold Calculator 0.0026); no verdict, the statistic is uncalibrated.

## Privacy

`pixel9fold__photo-marked__screenshot__s04__1.png` shows the same view as the
`s08` capture in 06-phones: a roofline and distant hills from which the place
could be worked out. Neither file is to be published. The home-screen file
shows a date and a temperature.

Every Fold screenshot holds its real capture time, UTC offset and a unique ID
in EXIF; every macOS screenshot holds the display's ICC profile, usually named
after the monitor. Strip metadata from any of these before publishing one.

`macos__window__screencapture__s03__1.png` shows no account, bookmark or
history. It does show which browser is installed, that it is not the default,
and the title of a second tab (the browser's own settings page).

Three of the macOS files are pictures of the wallpaper, and the browser window
shows a credited photograph on its New Tab page. Those are other people's
photographs: the files serve the corpus, and are not for publication as images.

## Open

- iPhone and iPad screenshots: wait for those devices.
- `adb exec-out screencap -p` on the Fold, a different writer from the
  hardware shortcut: not collected.
- Chrome's own "Long screenshot": not collected.
- A light-theme capture: every file inspected here is dark.
- The Fold unfolded (inner display): not collected.

## Results

Differential run 2026-11-17 (ground truth collected 2026-11-17T06:08:34Z): 10
of 10 files compared, 0 disagreements; rights and identifier values compared by
keyed fingerprint. No signals or conformance rubric applies, since no file has
a manifest. Whole corpus: 390 of 390 files across 10 corpora, 0 disagreements.
