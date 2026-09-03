# What is open, what is paid

Halftone is open-core. The detector code is permissively licensed so
that anyone — including the fact-checkers and institutions who buy it —
can audit exactly what a verdict means. The *calibrated* parts that
require ongoing work are paid.

| Component | License | Where |
|---|---|---|
| All `halftone-*` crates, the `halftone` CLI, `schemas/`, `python/export/` | MIT OR Apache-2.0 | this repo, crates.io |
| Layer 1 (C2PA) and Layer 4 (container forensics) — fully functional | MIT OR Apache-2.0 | this repo |
| Layer 2 (keyed watermark) detectors and embedders | MIT OR Apache-2.0 | this repo (you bring your own keys) |
| Layer 3 inference plumbing (`halftone-blind`) | MIT OR Apache-2.0 | this repo |
| Seed fingerprint DB (Pillow, libjpeg-turbo entries) | MIT OR Apache-2.0 | `crates/halftone-container/fingerprints.json` |
| Pack manifest format, schema, verification code | MIT OR Apache-2.0 | `crates/halftone-packs`, `packs/**/pack.json` |
| **Eval packs** — Layer 3 ONNX weights + `calibration.json`, `"tier": "eval"` | PolyForm Noncommercial 1.0.0 ([packs/LICENSE-EVAL.md](packs/LICENSE-EVAL.md)) | `ht packs update` |
| **Pro packs** — maintained models, calibration sets, extended fingerprint DB, `"tier": "pro"` | Commercial terms, not yet published | planned |
| License keys, pack signing keys, distribution service | proprietary, not published | — |

Every pack carries its `tier` and `license` inside the signed
`pack.json`, so `ht packs list` shows the terms you are running
under.

## Tiers

- **Free** — everything in this repo. Layers 1, 2, 4 and the eval
  harness work fully offline with no key. Layer 3 runs with any ONNX
  model you supply and calibrate yourself, or with eval packs for
  non-commercial use.
- **Pro / on-prem** (planned) — signed model packs with maintained
  calibration sets, fingerprint DB updates, offline license keys, and
  support. Not available yet; terms will be published here when they are.

## What this means in practice

- You can fork the code, build a competing product, and sell it. You
  must rename it (see TRADEMARK.md).
- You can train your own models and ship your own packs; the pack
  format is open and `halftone-packs` will verify any Ed25519-signed
  pack whose public key you trust.
- You can not use eval packs commercially, redistribute official packs,
  share license keys, or use pro packs outside the seats/hosts your key
  covers. That is governed by the pack license (PolyForm Noncommercial
  for eval, the commercial agreement for pro), not by the code license —
  removing the signature check from your build does not change what you
  are licensed to use.
- You can fork the code, build a competing product, and sell it. Please
  give it a different name (see the README).
