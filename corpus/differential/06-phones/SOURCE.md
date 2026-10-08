# 06-phones — provenance

Camera captures from own devices. Files are not committed; this file and
`expectations.json` are. Naming: `<device>__<variant>__<transfer>__<subject>__<n>.<ext>`
(variant = mode/settings, transfer = how the bytes reached the Mac, subject =
s01…s15; the same subject number is the same scene on every device). Only the
JPEG is collected; DNG, MOV and HEIC stay out (HEIC until `ht` can load it).

Location: every file here was shot with location off. The first set
(2026-10-06/07) was shot with location on; it was moved to 07-editors and
GPS-stripped there, so no file with coordinates remains in the corpus. The
hashing and container findings below were first measured on that set and are
confirmed on the reshoot.

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
