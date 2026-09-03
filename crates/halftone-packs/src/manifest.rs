//! `pack.json` and `calibration.json` formats.
use serde::{Deserialize, Serialize};
use std::fmt;

/// Which terms a pack is distributed under. Lives inside the signed
/// manifest so it cannot be edited without invalidating the signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Free for non-commercial evaluation; no license key required.
    Eval,
    /// Requires a valid offline license key.
    Pro,
}

impl Tier {
    /// Whether `halftone-packs::license` must find a valid key before
    /// this pack is loaded.
    pub fn requires_key(self) -> bool {
        matches!(self, Tier::Pro)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Tier::Eval => "eval",
            Tier::Pro => "pro",
        })
    }
}

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
    /// Distribution tier. Required: a pack with no tier is malformed.
    pub tier: Tier,
    /// License identifier. SPDX id where one exists
    /// (`PolyForm-Noncommercial-1.0.0`), otherwise a stable
    /// vendor string (`Halftone-Pack-Commercial`).
    pub license: String,
    /// Where the full license text lives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_url: Option<String>,
    /// Files with hashes.
    pub files: Vec<PackFile>,
}

impl PackManifest {
    /// Sanity check that the tier and license id agree, so a mislabelled
    /// pack fails at signing time rather than at a customer's.
    pub fn validate_license(&self) -> Result<(), String> {
        match (self.tier, self.license.as_str()) {
            (Tier::Eval, "PolyForm-Noncommercial-1.0.0") => Ok(()),
            (Tier::Pro, "Halftone-Pack-Commercial") => Ok(()),
            (t, l) => Err(format!("tier `{t}` does not match license `{l}`")),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_roundtrips_lowercase() {
        assert_eq!(serde_json::to_string(&Tier::Pro).unwrap(), "\"pro\"");
        assert_eq!(
            serde_json::from_str::<Tier>("\"eval\"").unwrap(),
            Tier::Eval
        );
    }

    #[test]
    fn missing_tier_is_rejected() {
        let json = r#"{"name":"x","version":"1","min_tool_version":"0.1.0",
                       "license":"PolyForm-Noncommercial-1.0.0","files":[]}"#;
        assert!(serde_json::from_str::<PackManifest>(json).is_err());
    }

    #[test]
    fn mismatched_tier_fails_validation() {
        let m = PackManifest {
            name: "x".into(),
            version: "1".into(),
            min_tool_version: "0.1.0".into(),
            tier: Tier::Pro,
            license: "PolyForm-Noncommercial-1.0.0".into(),
            license_url: None,
            files: vec![],
        };
        assert!(m.validate_license().is_err());
    }
}
