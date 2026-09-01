# Halftone

Layered provenance and forensics for image, video, audio and text. Local-first.

Four independent evidence layers, four separate verdicts, no merged score:

| Layer | Crate | Question |
|---|---|---|
| Manifest | `halftone-c2pa` | Is there a valid signed C2PA manifest? |
| Container | `halftone-container` | Does the file structure match its claimed origin? |
| Mark | `halftone-mark` | Does it carry a watermark we hold a key/decoder for? |
| Blind | `halftone-blind` | Does a calibrated classifier flag it — at what FPR? |

```
halftone inspect photo.jpg
halftone inspect --json --only manifest,container *.jpg
halftone inspect --report clip.mp4
halftone sign photo.jpg --key mykey.pem
halftone bench corpus.json
halftone packs update
```

## Layout

```
halftone/
├── Cargo.toml                  workspace
├── crates/
│   ├── halftone-core/          Asset, Evidence, Status, EvidenceSource trait, Registry
│   ├── halftone-c2pa/          layer 1
│   ├── halftone-container/     layer 4: jpeg quant tables, png chunks, exif consistency
│   ├── halftone-mark/          layer 2: detect + embed (DWT-DCT, AudioSeal via ort)
│   ├── halftone-blind/         layer 3: ort-backed classifiers from signed packs
│   ├── halftone-bench/         eval harness → calibration.json
│   ├── halftone-packs/         signed pack download/verify, offline license
│   ├── halftone-report/        per-file HTML/PDF report
│   └── halftone-cli/           binary `halftone`
├── python/
│   ├── export/                 PyTorch → ONNX export scripts only
│   └── train/                  head training + calibration; never imported by Rust
├── packs/                      pack manifests + calibration.json (weights are not committed)
├── schemas/verdict.schema.json semver'd JSON schema for Inspection
├── testdata/                   tiny synthetic assets for unit tests
└── .github/workflows/          fmt, clippy, test, bench, pack signing
```

## Rules

- Every source records name + version + calibration hash. Old verdicts stay readable.
- `Absent` from a blind source means "not flagged at FPR x", never "human".
- Text: keyed tests first; blind text is feature-gated, reports `Inconclusive` below a
  minimum length, and never emits per-sentence output.
- No network except `halftone packs update`. No telemetry.
- Not for education or hiring decisions. See `halftone-report::NOTICE`.
