# 08-multipart — provenance

Byte-level copies of one signed multi-part file, for the multi-asset hash fallback
(C2PA 2.4 §15.12.4; DIFFERENTIAL.md D-014, D-015). Files are not committed; this
file and `expectations.json` are. Names follow 07:
`<editor>__<operation>__<input device>-<input variant>__<subject>__<n>`, with
`bytes` as the editor: the edits are made by `scripts/make-multipart-stratum.fish`,
not by an application.

Built 2026-10-25 from the source below. Stratum 08 was first planned for
screenshots; those moved to 11-screenshots.

## Source

`06-phones/pixel9fold__motion__adb-pull__s02__1.jpg`, a Pixel 9 Pro Fold motion
photo, 5,353,659 bytes. Its manifest signs a whole-file `c2pa.hash.data` and a
`c2pa.hash.multi-asset` over three parts: 0, the primary JPEG, [0, 2047094),
required; 1, the gain map, [2047094, 2056812), optional; 2, the MP4 clip,
[2056812, 5353659), optional. The script reads this table from the file and checks
it against the bytes before writing anything.

## Files

| File | Edit | §15.12.4 expects |
|---|---|---|
| `bytes__remove-clip__…` | file cut at 2056812: part 2 removed | `multiAssetHash.match`; part 2 skipped |
| `bytes__remove-gainmap-and-clip__…` | file cut at 2047094: parts 1 and 2 removed | `multiAssetHash.match`; parts 1, 2 skipped |
| `bytes__flip-clip__…` | 1 bit at 3705235, the middle of part 2 (0x22 → 0x23) | `multiAssetHash.mismatch`; part 2 mismatch |
| `bytes__cut-clip__…` | file cut at 3705235, inside part 2 | `multiAssetHash.mismatch`; part 2 cut short (Credentio: `missingPart` + `malformed`, D-015) |
| `bytes__flip-still__…` | 1 bit at 1069236, the middle of part 0's scan data (0x7b → 0x7a) | `multiAssetHash.mismatch`; part 0 mismatch |

c2pa (c2patool, and Halftone's `validation_state`) reports every file `Invalid`: it
checks only the whole-file hash (D-014). Halftone's `details.multi_asset` and
Credentio carry the §15.12.4 result.

## Results

Differential run 2026-10-25 (expectations collected 2026-10-25T19:58:50Z with
Credentio ee4078b): 5 of 5 files compared, 0 disagreements. `cut-clip` is
reported as `known: D-015` on every run.

## Regenerating

```fish
fish scripts/make-multipart-stratum.fish
uv run scripts/differential.py corpus/differential/08-multipart \
    --trust-anchors crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem \
    --tsa-anchors crates/halftone-c2pa/trust/C2PA-TSA-TRUST-LIST.pem \
    --trust-config crates/halftone-c2pa/trust/C2PA-EKU-CONFIG.cfg \
    --credentio ~/oss-bins/c2pa_validate
```

The script writes identical bytes on every run (positions derive from the source
file), so the recorded sha256s stay valid.
