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

## iPad Pro 12.9-inch, M1 (`iPad13,10`) — iPadOS 18.7.3, 2026-11-23

Taken with the hardware shortcut (top button + a volume button), saved by the
system to the camera roll (`/DCIM/100APPLE/`), pulled with `htc ios grab` over
Wi-Fi. `htc` read each file from the device twice and compared the two reads.

iPadOS has no demo mode for the status bar. Where one is in the frame it is the
device's own: the clock and date, the Wi-Fi sign, and the battery at 100% and
charging. Calculator and Safari were opened from Search, so those two frames
also show the "Search" link back. No notification badge or banner is in any
frame.

All four files are 2732×2048, the whole display in landscape, and keep the
upper-case `.PNG` the device gave them.

| File | Size | On screen |
|---|---|---|
| `ipadpro129m1__home__screenshot__s01__1.PNG` | 12,561,362 B | a Home Screen page: two of Apple's apps (Stocks, Voice Memos), Notes in the Dock, and the wallpaper, a photograph of star trails over a lake |
| `ipadpro129m1__app__screenshot__s02__1.PNG` | 228,777 B | Calculator on its black background, after a multiplication |
| `ipadpro129m1__web__screenshot__s03__1.PNG` | 1,116,857 B | Safari on the English Wikipedia article "Renaissance", in the site's light appearance, with its Contents and Appearance panels open |
| `ipadpro129m1__photo-marked__screenshot__s04__1.PNG` | 9,628,237 B | `10-messaging/airdrop__file__gemini-app-nano-banana__p1__1.JPG` full screen in Photos, controls hidden: the image between black bars, no status bar. That file is byte-identical to `04-generators/gemini-app__nano-banana__web-download__p1__1.jpg`, the signed image the macOS row shows |

## Findings

- No file carries a C2PA manifest (14 of 14).
- The three marked rows are the point of the stratum. On the Fold, the image on
  screen is a camera capture with a trusted Google manifest declaring
  computationalCapture; the screenshot of it has no manifest and no metadata
  that says so. On macOS the same holds for the signed image shown in Preview,
  and on the iPad for the same image shown in Photos. The copy on the iPad
  still validates and declares trainedAlgorithmicMedia; the screenshot of it
  declares nothing. A signed file becomes an unsigned one by being displayed.
- macOS PNG: `png_writer` reads the chunk layout as a macOS screenshot (Apple
  ImageIO, XMP `UserComment=Screenshot`). The rule was written from the
  documented layout and marked "not yet verified"; these four files were its
  first real samples, and it is marked verified since 2026-11-17. The XMP
  holds none of the rights, identifier, vendor-flag or data-mining fields
  (`ht inspect --metadata`). Since 2026-11-23 the rule also asks for the
  profile chunk's name, `ICC Profile` (see the iPad findings).
- Fold PNG: the chunk layout is that of Skia's PNG encoder (sBIT and sRGB,
  8 KiB blocks), a rule measured on a Skia canvas binding. No XMP and no ICC
  profile. So by layout alone an Android screenshot is not told apart from
  any other Skia-written PNG, and at collection time `png_writer` named Skia.
  Since 2026-11-17 a second rule asks for that layout together with the EXIF
  `Software` tag described below, and all five files read as Android's system
  screenshot tool. The tag is unsigned; the rule needs the layout as well.
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
- The profile in the full-screen PNG (`s01`) is a reduced one. Its tags, by
  ExifTool's names (2026-11-17): description, copyright, white point, the
  three colourant columns, the three tone curves and the chromatic-adaptation
  matrix. It has none of the tags a display's profile can carry to describe
  the one screen (`vcgt`, Apple's `mmod` and `ndin`). So no tag sets it apart
  from the profile of a colour space; its description does. One file checked.
- The profile's header, all five files (2026-11-17): the device model and the
  profile ID are empty, and the manufacturer is Apple's throughout. The
  header's date is set, differs between the three displays, and is the same on
  the three captures from the 1× display. So it is not the capture time. It is
  not an old date either: by day count it falls on or just before the capture
  day for four files and about a day earlier for `s04`. It reads as the time
  that display's profile was generated, and captures from one display share it
  exactly. Whether it survives a login, a restart or a reconnect was not
  measured. No Halftone source reads it.
- Since 2026-11-17 `embedded_identifiers` lists the `ImageUniqueID` of all
  five Fold screenshots as a document identifier, masked unless `--reveal`,
  and `png_writer` quotes the `Software` tag. (`exif_consistency` is right to
  say no generator is named: an Android build is not one.)
- At collection time no Halftone source read the display's name: checked on
  these files 2026-11-16, neither monitor name occurred anywhere in the output
  of `ht inspect --batch --json --reveal`. Since 2026-11-17
  `embedded_identifiers` reads the profile description and lists it for all
  five macOS files as a display name (kind `display`): the profile is of the
  display class and its name is no standard colour space's. The value is
  printed only with `--reveal`; without it the listing gives the field, the
  kind and the length. The five lengths fall into the same three groups as the
  descriptions above, the generic `Display` among them. `png_writer` still
  reads the chunk's name and not the profile, and the JPEG and WebP sources
  still record only whether a profile is present.
- iPad PNG: iPadOS writes two layouts, two files of each. The Home Screen and
  the Safari page are 16 bits per channel with an `iCCP` chunk named
  `kCGColorSpaceDisplayP3` (a 536-byte profile whose description is
  `Display P3`) and a `cICP` chunk (12, 13, 0, 1: P3 primaries, the sRGB
  transfer curve, RGB, full range). Calculator and the photo in Photos are 8
  bits per channel with an `sRGB` chunk and no profile. All four have `eXIf`
  and an XMP `iTXt`, no `pHYs`, and image data in 16,384-byte blocks behind
  zlib header `78 01`. Why the iPad chose which layout was not established.
- At collection time `png_writer` read the two Display P3 files as a macOS
  screenshot, which names the wrong system, and found no known layout in the
  two sRGB files. The macOS rule asked only for the XMP comment, and iPadOS
  writes the same one. Since 2026-11-23 there is one rule per iPadOS layout,
  each marked "iPhone not measured", and the macOS rule needs its own profile
  chunk name. Re-read that day: the four iPad files by the new rules, two
  each, and the four macOS PNGs still as macOS screenshots.
- The iPad's EXIF block, all four files (read from the chunk directly,
  2026-11-24, not with ExifTool): orientation, the capture time to the second
  with no UTC offset, `UserComment = Screenshot` and the pixel size; the two
  sRGB files add `ColorSpace = sRGB`. No `Software`, no make or model, no
  `ImageUniqueID`. The XMP is the same 495-byte packet in all four (toolkit
  `XMP Core 6.0.0`): `exif:UserComment = Screenshot` and
  `photoshop:DateCreated` with the same time, again without a zone. In the
  three files with a status bar that time agrees with the clock on screen.
- So the iPad screenshot says the least about where it came from. Unlike the
  Fold's it names no system build, has no unique ID and no time zone. Unlike
  the Mac's it names no display: the profile is the colour space's own, and
  `embedded_identifiers` lists nothing for any of the four. What is left is
  an unsigned comment that it is a screenshot, a local time, and a pixel size
  that is this display's.
- `pixel_lattice` gives no verdict on any file here. It runs without a
  calibrated threshold, and its rationale calls that state "dark mode": the
  words describe the source, not the image. Its statistic: macOS PNG 0.0207,
  Fold Calculator 0.0026, iPad 0.0097 (Home Screen), 0.0114 (Calculator),
  0.0302 (web) and 0.0167 (photo). This line used to say the files "read as
  dark-mode images", which misread that rationale; corrected 2026-11-24.

## Privacy

`pixel9fold__photo-marked__screenshot__s04__1.png` shows the same view as the
`s08` capture in 06-phones: a roofline and distant hills from which the place
could be worked out. Neither file is to be published. The home-screen file
shows a date and a temperature.

Every Fold screenshot holds its real capture time, UTC offset and a unique ID
in EXIF; every macOS screenshot holds the display's ICC profile, usually named
after the monitor; every iPad screenshot holds its capture time, without a
zone, in EXIF and XMP. Strip metadata from any of these before publishing one.

Three of the iPad files show the real clock, date and battery state, since
iPadOS has no demo mode. The Home Screen file shows two installed apps, both
Apple's, and that the device has two more pages. The Safari file shows no tab,
bookmark or account.

`macos__window__screencapture__s03__1.png` shows no account, bookmark or
history. It does show which browser is installed, that it is not the default,
and the title of a second tab (the browser's own settings page).

Three of the macOS files are pictures of the wallpaper, and the browser window
shows a credited photograph on its New Tab page. Those are other people's
photographs: the files serve the corpus, and are not for publication as images.
The iPad Home Screen file is mostly wallpaper too, a photograph whose source
was not noted; the same holds for it.

## Open

- iPhone screenshots: not collected. The two iPadOS rules say "iPhone not
  measured" for that reason.
- Other iPadOS versions, and what decides between the Display P3 and the sRGB
  layout: not measured.
- A full-page capture in Safari on the iPad: not collected, and whether it
  gives a PNG was not checked.
- `adb exec-out screencap -p` on the Fold, a different writer from the
  hardware shortcut: not collected.
- Chrome's own "Long screenshot": not collected.
- The Fold unfolded (inner display): not collected.

## Results

Differential run 2026-11-24 (ground truth collected 2026-11-24T13:22:38Z): 14
of 14 files compared, 0 disagreements; rights and identifier values compared by
keyed fingerprint. No signals or conformance rubric applies, since no file has
a manifest. Whole corpus: 410 of 410 files across 10 corpora, 0 disagreements.
