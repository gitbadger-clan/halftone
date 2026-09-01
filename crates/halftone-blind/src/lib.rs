//! Layer 3 — blind / forensic classifiers. The weakest layer, and the only one that
//! churns. Rules:
//! - Every model is a signed pack from `halftone-packs`, never bundled.
//! - Every source carries a `CalibrationRef` and thresholds at a published FPR.
//! - `Absent` means "not flagged at FPR x", never "human". Rationale must say so.
//! - Text sources report `Inconclusive` below a minimum length and never emit
//!   per-sentence output.

pub mod image;

pub use image::FeatureProbe;
