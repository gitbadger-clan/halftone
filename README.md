# Halftone

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="brand/halftone-lockup-dark.svg">
  <img alt="Halftone" src="brand/halftone-lockup-light.svg" width="360">
</picture>

Layered provenance and forensics for image, video, audio and text. Local-first.

Four independent evidence layers, four separate verdicts, no merged score:

| Layer | Crate | Question |
|---|---|---|
| Manifest | `halftone-c2pa` | Is there a valid signed C2PA manifest? |
| Container | `halftone-container` | Does the file structure match its claimed origin? |
| Mark | `halftone-mark` | Does it carry a watermark we hold a key/decoder for? |
| Blind | `halftone-blind` | Does a calibrated classifier flag it — at what FPR? |

## Install

    cargo install halftone-cli

Installs the `ht` binary. Not related to the unrelated `halftone` crate
on crates.io.

```
ht inspect photo.jpg
ht inspect --json --only manifest,container *.jpg
ht inspect --report clip.mp4
ht inspect --fingerprints my-writers.json --trust-anchors anchors.pem photo.jpg
ht fingerprint --writer "Canon EOS R5 fw 1.8.1" --class camera --into my-writers.json shots/*.jpg
ht fingerprint --writer "macOS 15 screenshot" --class screenshot --into my-writers.json ~/Desktop/Screenshot*.png
ht sign photo.jpg --key mykey.pem
ht bench corpus.json
ht packs update
```

## Container layer (implemented)

| Source | Format | What it reports | `Present` means |
|---|---|---|---|
| `jpeg_quant` | JPEG | Annex-K quality (luma + chroma), chroma subsampling, Huffman class (default/optimised), marker inventory, writer fingerprint → DB lookup | libjpeg-family or Adobe re-encode, or a DB match. Encoder path, not authorship. |
| `jpeg_double` | JPEG (baseline) | Double-quantization comb in luma DCT histograms (own baseline Huffman decoder) | Re-encoded after an earlier JPEG encoding. Blind spots stated in the rationale. |
| `png_writer` | PNG | Chunk inventory → built-in writer rules (`png_rules.rs`, each marked verified/documented) + exact-hash DB, iCCP name, pHYs, `caBX`, generation-parameters text | Embedded metadata names a generator or carries pipeline parameters. Writer matches are `Inconclusive` unless the writer is a generator. |
| `webp_writer` | WebP | Chunk inventory, lossy/lossless, EXIF/XMP | XMP names a generator. |
| `exif_consistency` | JPEG/PNG/WebP/HEIF | Self-identification, camera-metadata contradictions, EXIF-vs-frame dimensions | Metadata explicitly names a generator. Contradictions are `Inconclusive`. |

None of these carries a calibration yet; the `details` object exposes every raw
discriminant so `ht bench` can measure per-signal false-positive rates against a
labelled corpus. The `jpeg_double` threshold is provisional and says so.

Layer 1 (`c2pa`) is implemented behind the `c2pa` feature: `cargo build --features halftone-cli/c2pa`.

## Pixel layer (dark mode)

`halftone-pixel` is the first crate that decodes pixels (via `image`); still model-free.
`pixel_lattice` measures the period-8 residual energy rhythm left by latent-diffusion
decoders — and equally by earlier JPEG compression re-saved losslessly and by
nearest-neighbour 8× upscales, which the rationale names. PNG and lossless WebP only.
It ships **dark**: `threshold: None`, verdict always `Inconclusive`, statistic reported.
Promote it by constructing it with the threshold from its calibration file once
`ht bench` has produced one on ≥300 stratified real negatives with zero hits.

## Calibration workflow

```
corpus/
├── real-phone/          # straight off the device
├── real-screenshot/     # your OS screenshots
├── real-messaging/      # same photos after WhatsApp/Telegram
├── gen-sdxl/            # generated locally, native resolution
└── gen-flux/
ht corpus corpus/ --out corpus.json
ht bench corpus.json --fpr 0.01 --stats-out stats.jsonl --calib-dir calib/
```

`bench` runs every statistical source (`jpeg_double`, `pixel_lattice`, later the mark
and blind layers), prints per-class FPR and per-generator TPR at the threshold, and
writes `calib/<source>.calibration.json` in the pack `Calibration` shape — including
`fpr_by_real_source`, which is the number that decides whether a source may leave dark
mode.

## Layout

```
halftone/
├── Cargo.toml                  workspace
├── crates/
│   ├── halftone-core/          Asset, Evidence, Status, EvidenceSource trait, Registry
│   ├── halftone-c2pa/          layer 1
│   ├── halftone-container/     layer 4: jpeg quant/huffman/double-compression, png, webp, exif
│   ├── halftone-mark/          layer 2: detect + embed (DWT-DCT, AudioSeal via ort)
│   ├── halftone-blind/         layer 3: ort-backed classifiers from signed packs
│   ├── halftone-bench/         eval harness → calibration.json
│   ├── halftone-packs/         signed pack download/verify, offline license
│   ├── halftone-pixel/         pixel-domain model-free statistics (dark until calibrated)
│   ├── halftone-report/        per-file HTML/PDF report
│   └── halftone-cli/           binary `ht`
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
- No network except `ht packs update`. No telemetry.
- Not for education or hiring decisions. See `halftone-report::NOTICE`.

## License

The code in this repository is dual-licensed under either the
[Apache License 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT),
at your option.

Model packs, calibration sets and fingerprint DB updates are separate
works. Eval packs are under the
[PolyForm Noncommercial License 1.0.0](packs/LICENSE-EVAL.md); pro
packs are under commercial terms. See [COMMERCIAL.md](COMMERCIAL.md)
for exactly what is open and what is paid.

### Name

Forks are welcome under the licenses above. Please don't call a modified
version "Halftone": verdicts from this tool carry a stated false-positive
rate, and a fork with different thresholds or models shouldn't be
mistaken for it. "Forked from Halftone" is fine.
