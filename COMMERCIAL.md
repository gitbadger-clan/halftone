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
| Pack manifest format, schema, verification code | MIT OR Apache-2.0 | `crates/halftone-packs`, `packs/*.json` |
| **Layer 3 model packs** (ONNX weights + `calibration.json` with TPR@FPR tables) | Halftone Pack License | `halftone packs update` |
| **Extended fingerprint DB** updates | Halftone Pack License | `halftone packs update` |
| License keys, pack signing keys, distribution service | proprietary, not published | — |

## Tiers

- **Free** — everything in this repo. Layers 1, 2, 4 and the eval
  harness work fully offline with no key. Layer 3 runs with any ONNX
  model you supply and calibrate yourself.
- **Pro / on-prem** — signed model packs with maintained calibration
  sets, fingerprint DB updates, offline license keys, and support.
  Pricing and terms: https://halftone.gitbadger.com/pricing

## What this means in practice

- You can fork the code, build a competing product, and sell it. You
  must rename it (see TRADEMARK.md).
- You can train your own models and ship your own packs; the pack
  format is open and `halftone-packs` will verify any Ed25519-signed
  pack whose public key you trust.
- You can not redistribute official packs, share license keys, or use
  official packs outside the seats/hosts your key covers. That is
  governed by `packs/LICENSE.md`, not by the code license — removing
  the signature check from your build does not change what you are
  licensed to use.

Not legal advice for your situation; the pack license text should be
reviewed by a lawyer before you take money for it.
