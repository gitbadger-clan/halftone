//! `pack.json` and `calibration.json` formats.

use serde::{Deserialize, Serialize};

/// One file inside a pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackFile {
    /// Relative path inside the archive.
    pub path: String,
    /// SHA-256 hex.
    pub sha256: String,
}

/// `pack.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    /// Pack name, e.g. `blind-image-dinov2-probe`.
    pub name: String,
    /// Pack version, e.g. `2026.08.1`.
    pub version: String,
    /// Minimum `halftone` binary version.
    pub min_tool_version: String,
    /// Files with hashes.
    pub files: Vec<PackFile>,
}

/// Per-transformation robustness row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobustnessRow {
    /// Distortion name, e.g. `jpeg_q50`, `resize_0.5`, `crop_10`.
    pub distortion: String,
    /// TPR at the target FPR after this distortion.
    pub tpr_at_fpr: f64,
}

/// `calibration.json` — the numbers that get published with every pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibration {
    /// Calibration set id.
    pub set_id: String,
    /// SHA-256 of the corpus manifest.
    pub corpus_sha256: String,
    /// Target FPR the threshold was set at.
    pub fpr_target: f64,
    /// Threshold on the model's raw statistic.
    pub threshold: f64,
    /// Held-out generators used for TPR.
    pub heldout_generators: Vec<String>,
    /// Overall TPR at target FPR on held-out generators.
    pub tpr_at_fpr: f64,
    /// Measured real-image FPR broken out by source class.
    pub fpr_by_real_source: std::collections::BTreeMap<String, f64>,
    /// Robustness table.
    pub robustness: Vec<RobustnessRow>,
}
