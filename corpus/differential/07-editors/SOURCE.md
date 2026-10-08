# 07-editors — provenance

Edited versions of corpus files, one section per editor and operation. Files
are not committed; this file and `expectations.json` are. Names:
`<editor>__<operation>__<input device>-<input variant>__<subject>__<n>`.

## ExifTool — GPS removal (ExifTool 13.55)

- Date: 2026-10-07. Command: `exiftool -gps:all= '-xmp:gps*=' -overwrite_original`.
- Input: the first 06 captures (Pixel 9 Pro Fold s01–s03, Pixel 5 s01–s07),
  shot with location on; moved here and stripped in place, so no copy with
  coordinates remains in the corpus.
- Finding (Fold, signed): the manifest no longer validates; c2patool and
  Halftone both report Invalid. On every Fold file the failing assertion is the
  whole-file hard binding `c2pa.hash.data`, reported twice with two different
  explanations: the covered bytes changed ("Hashes do not match"), and the
  manifest no longer sits where the hash's exclusion range expects it ("data
  hash exclusion does not match the manifest location") — ExifTool's rewrite of
  the metadata segments before the C2PA segment moved it. The per-part hashes
  report only `hashedURI.match`, as on the unmodified originals (see 06). A
  privacy edit invalidates the camera's signature.
- Finding (Pixel 5, unsigned): no manifest, nothing to break.
- Container: parts unchanged per ht-layout (gain map and MP4 kept on every
  file); sizes changed by −243 to +127 bytes, i.e. ExifTool rewrote the
  metadata section rather than only deleting the GPS tags.
- Files (10): Fold default s01, motion s02, uhdr-off s03; Pixel 5 raw-jpeg
  s01, motion-raw-jpeg s02, default s03, motion s04, uhdr-off-motion s05 and
  s06, uhdr-off s07.
