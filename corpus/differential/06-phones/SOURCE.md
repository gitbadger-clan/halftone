# 06-phones — provenance

Camera captures from own devices. Files are not committed; this file and
`expectations.json` are. Naming: `<device>__<variant>__<transfer>__<subject>__<n>.<ext>`
(variant = mode/settings, transfer = how the bytes reached the Mac, subject =
s01…s15; the same subject number is the same scene on every device). Stills are
collected, JPEG and since 2026-11-26 HEIC (`ht` reads a HEIC's metadata; it
never decodes the pixels). DNG and MOV stay out.

Location: every file here was shot with location off. The first set
(2026-10-06/07) was shot with location on; it was moved to 07-editors and
GPS-stripped there, so no file with coordinates remains in the corpus. The
hashing and container findings below were first measured on that set and are
confirmed on the reshoot.

## iPad Pro (12.9-inch) (5th generation) — iPadOS 18.7.3

- Date: 2026-11-26, location off for the camera. Model identifier `iPad13,10`;
  the name above is the one the files carry in EXIF `Model`. Pulled with
  `htc ios grab` over Wi-Fi (AFC).
- Settings: Smart HDR, Scene Detection and Lens Correction on. Formats on High
  Efficiency for the `heic` rows and Most Compatible for the `jpeg` rows; Live
  Photo on for the two `-live` rows. Back wide camera for all six.
- Scenes: s01 a bookshelf, s02 water running into a glass, s09 a ceiling spot
  in a lit room. s09 is new with this device and exists on no other.
- Finding: no C2PA manifest and no XMP in any of the six. EXIF names the make,
  the model (also as `HostComputer` and in the lens model) and the OS version
  (`Software`), and gives the capture time with sub-seconds and a UTC offset.
  All six say `CompositeImage = 2` (a general composite: the camera states the
  photo was merged from several frames). No GPS, no serial number, no owner
  name, no EXIF `ImageUniqueID`.
- Identifiers sit in Apple's MakerNote (version 15): `RunTime` (the time since
  power-on), `PhotoIdentifier` and `ImageCaptureRequestID` in all six, and
  `ContentIdentifier` in the two Live Photo rows only. Until 2026-11-26
  Halftone read all six as carrying no identifier and could not tell the Live
  Photo stills from the others; since then `embedded_identifiers` lists the
  four fields and `live_photo` reports the last.
- Live Photo: the clip is a separate MOV on the device and was not collected.
  That it carries the same `ContentIdentifier` was not checked.
- HDR data, without a gain map. No file holds a second image: the JPEGs have
  no MPF index, the HEICs no auxiliary item. The three JPEGs carry a 764-byte
  `APP10` segment tagged `AROT`, which ExifTool reads as `HDRGainCurve`; no
  counterpart was found in the HEICs. The MakerNote's `HDRHeadroom` is 1.69 in
  both s09 files and the HEIC s02, and 0 in both s01 files and the JPEG s02.
  Halftone reads none of this.
- JPEG container: 4032×3024 baseline, 4:2:0, Display P3 profile. Segments in
  the order APP1 (EXIF, with a thumbnail), APP2 (ICC), APP10 (`AROT`), DQT,
  DRI (restart interval 252), SOF0, DHT, SOS. The quantization tables are
  identical in all three files. `jpeg_quant` reads them as non-standard and
  not in the fingerprint DB.
- HEIC container: brand `heic` (compatible: mif1, MiPr, miaf, MiHB, heic). The
  primary item is a grid of 48 HEVC tiles, 4032×3024, 8 bits per channel; with
  it one 320×240 thumbnail, one EXIF item and a Display P3 profile. No
  Halftone source reads HEIF structure, so these three files get no container
  evidence beyond their metadata.
- Privacy: each file carries per-photo UUIDs and the device's run time, so
  the six link to each other and to one stretch of use of one device. s01
  shows the spines of a bookshelf. None is for publication as an image.
- Differential run 2026-11-26 (ground truth collected 2026-11-26T10:56:04Z):
  19 of 19 files in the stratum compared, 0 disagreements; identifier values
  compared by keyed fingerprint.
- Open: the HDR curve and headroom, `CompositeImage`, a fingerprint for the
  JPEG encoder and a structure source for HEIF are not read yet. Two
  rationales (`mpf_index`, `container_directory`) mention an iPhone gain map
  in the MPF index; this iPad writes none, and no iPhone has been measured.
- Files (6): s01 heic, s01 jpeg, s02 heic-live, s02 jpeg-live, s09 heic,
  s09 jpeg.

## Pixel 9 Pro Fold — Pixel Camera 10.4.117, Android 17

- Date: reshoot 2026-10-07 to 2026-10-08, location off. adb over wireless debugging.
- Settings per file: Ultra HDR on (default) except the `uhdr-off` rows; Top Shot
  ("Settings → Top Shot → Manual") On for the motion rows; RAW+JPEG on for
  `raw-jpeg`.
- Finding: every capture carries a C2PA manifest from "Google C2PA SDK for
  Android 912750169:936816820", signed by Google LLC (signed_at = capture time),
  content hash intact, digitalSourceType computationalCapture, zero ingredients.
  XMP present without IPTC DigitalSourceType. c2patool 0.28.0 and Halftone:
  Trusted on all six files, no validation codes (collector, all three trust
  inputs, 2026-10-08).
- Container and hashing, by layout (from `c2patool --detailed`; Halftone's
  assertion list omits `c2pa.hash.data`):
  - default (s01, s03): MPF, primary + gain map; whole-file `c2pa.hash.data`
    plus `c2pa.hash.data.part` ×2 + `c2pa.hash.multi-asset`.
  - motion (s02): primary + gain map + MP4, `.MP.jpg` on the phone; whole-file
    `c2pa.hash.data` plus `c2pa.hash.data.part` ×3 + `c2pa.hash.multi-asset`.
  - Ultra HDR off, motion on (s05): primary + MP4, no gain map; whole-file
    `c2pa.hash.data` plus `c2pa.hash.data.part` ×2 + `c2pa.hash.multi-asset`.
  - Ultra HDR off (s07): plain single JPEG; whole-file `c2pa.hash.data` only.
  - RAW+JPEG (s01 raw-jpeg): the JPEG saved beside the DNG is a plain single
    JPEG, no gain map (unlike the Pixel 5's RAW cover), and is signed;
    whole-file `c2pa.hash.data` only.
- Verification (2026-10-08, tamper test on a copy of s02): one byte changed
  inside the MP4 → Invalid in both tools, `assertion.dataHash.mismatch` on the
  whole-file `c2pa.hash.data`; the whole-file hash covers the video. c2patool
  0.28 reports no content result for the per-part hashes, only
  `hashedURI.match`, on unmodified and modified files alike. The whole-file hash
  reports `assertion.dataHash.additionalExclusionsPresent`; what it excludes
  besides the manifest is not yet checked.
- Multi-asset (2026-10-23, s02): the signed `c2pa.hash.multi-asset` lists the
  primary JPEG [0, 2047094) as required, the gain map [2047094, 2056812) and
  the MP4 [2056812, 5353659) as optional; the offsets match the bytes. c2pa-rs
  (c2patool, Halftone) never evaluates it, so a copy with the MP4, or the MP4
  and gain map, removed reads Invalid; Credentio validates such a copy through
  the parts (`multiAssetHash.match`) and still rejects a changed bit in the
  MP4 or in the primary (`multiAssetHash.mismatch`). The 2026-10-08 result
  above holds for every tool tested: changing the video invalidates the
  manifest; removing it does so only for a validator without the §15.12.4
  fallback. D-014, D-015.
- Parser: all layouts read cleanly; jpeg_double unaffected by the MP4 tail.
- Tool note: ExifTool 13.55 reports `GainMapImage` with the MP4's length on
  three-part files; the container directory has the true gain-map length.
- Files (6): s01 default, s01 raw-jpeg, s02 motion, s03 default,
  s05 uhdr-off-motion, s07 uhdr-off.

## Pixel 5 — Pixel Camera 9.2.113 (system 8.3.252), Android 14

- Date: reshoot 2026-10-07, location off. No SIM; adb over wireless debugging.
- Finding: no C2PA manifest on any camera still; c2patool agrees. XMP present
  without IPTC DigitalSourceType. EXIF consistent (make Google).
- Container, by layout (Ultra HDR is written here too):
  - default (s03): primary + gain map.
  - motion (s04): primary + gain map + MP4, `.MP.jpg`.
  - Ultra HDR off, motion on (s05): primary + MP4, no gain map.
  - Ultra HDR off, motion off (s07): plain single JPEG.
  - RAW+JPEG (s01, s02): same layouts; the JPEG is named `…RAW-01.COVER.jpg` /
    `…RAW-01.MP.COVER.jpg`, with a `…RAW-02.ORIGINAL.dng` beside it (not
    collected).
- Motion setting: the camera kept saving `.MP.jpg` files while motion was meant
  to be off (consistent with an Auto mode that keeps the clip when it detects
  movement); the setting's label was not recorded.
- Parser: all layouts read cleanly; jpeg_double unaffected by the MP4 tail.
- Files (6): s01 raw-jpeg, s02 motion-raw-jpeg, s03 default, s04 motion,
  s05 uhdr-off-motion, s07 uhdr-off. The first set's s06 (a second
  uhdr-off-motion file) was not reshot.
- Device retired 2026-10-07 (swollen battery, handed in for recycling).
