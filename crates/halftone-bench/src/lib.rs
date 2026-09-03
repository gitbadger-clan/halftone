//! `halftone bench` — the eval harness, and a product in its own right.
//!
//! Inputs: a corpus manifest (real sources labelled by class; generated sets
//! labelled by generator), a distortion suite, and a registry of sources.
//! Outputs: per-source ROC, TPR@{1%,0.1%} FPR per generator per distortion,
//! real-source FPR breakdown, and a `calibration.json` ready to ship in a pack.

pub mod corpus;
#[cfg(feature = "distort")]
pub mod distort;
pub mod metrics;
