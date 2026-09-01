//! The verdict schema. Semver'd independently of the crates: bump [`SCHEMA_VERSION`]
//! on any breaking change and keep `schemas/verdict.schema.json` in sync.

use serde::{Deserialize, Serialize};

use crate::AssetInfo;

/// Version of the JSON verdict schema emitted by [`Inspection`].
pub const SCHEMA_VERSION: &str = "1.0.0";

/// The four independent evidence layers. Order is cheapest-first and is the
/// default execution order in [`crate::Registry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Signed provenance manifest (C2PA).
    Manifest,
    /// File-structure forensics (quant tables, chunk order, codec fingerprints).
    Container,
    /// Keyed watermark detection with a decoder/key we hold.
    Mark,
    /// Blind / forensic classifier. Weakest layer; always calibrated.
    Blind,
}

/// Outcome of one source on one asset. Deliberately not a probability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The signal this source looks for is present (e.g. valid manifest, mark decoded,
    /// statistic beyond calibrated threshold).
    Present,
    /// The signal is absent. For blind sources this means "not flagged", never "human".
    Absent,
    /// Source ran but cannot say (too short, too degraded, ambiguous statistic).
    Inconclusive,
    /// Source does not apply to this asset (wrong modality, format, or missing pack).
    NotApplicable,
}

/// Identity of a source. `version` is the source's own version, which changes
/// whenever the model, decoder, or rules change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId {
    /// Stable machine name, e.g. `c2pa`, `jpeg_quant`, `dwtdct`, `clip_probe`.
    pub name: String,
    /// Source/model version, e.g. `1.2.0` or `2026.08`.
    pub version: String,
}

/// A calibrated test statistic. Present only for statistical sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Statistic {
    /// Name of the statistic, e.g. `z_score`, `bit_accuracy`, `logit`.
    pub name: String,
    /// Observed value.
    pub value: f64,
    /// Description of the null model the p-value is computed against.
    pub null_model: String,
    /// p-value under the null, if computed.
    pub p_value: Option<f64>,
    /// Decision threshold that produced [`Status`], if one was applied.
    pub threshold: Option<f64>,
}

/// Pointer to the calibration set a statistical source was thresholded on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRef {
    /// Identifier of the calibration set, e.g. `ntire2026-heldout5`.
    pub set_id: String,
    /// SHA-256 of the calibration manifest.
    pub sha256: String,
    /// Target false-positive rate the threshold was chosen for.
    pub fpr_target: f64,
    /// Measured true-positive rate at that FPR on held-out data.
    pub tpr_at_fpr: Option<f64>,
}

/// One source's verdict on one asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Which layer produced this.
    pub layer: Layer,
    /// Which source, at which version.
    pub source: SourceId,
    /// The verdict.
    pub status: Status,
    /// Test statistic, for statistical sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statistic: Option<Statistic>,
    /// Calibration the threshold came from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calibration: Option<CalibrationRef>,
    /// One or two sentences a non-expert can read. Never a percentage.
    pub rationale: String,
    /// Source-specific structured details (parsed manifest, quant tables, etc.).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub details: serde_json::Value,
    /// Wall time spent in this source.
    pub duration_ms: u64,
}

impl Evidence {
    /// Convenience constructor for a not-applicable result.
    pub fn not_applicable(layer: Layer, source: SourceId, why: impl Into<String>) -> Self {
        Self {
            layer,
            source,
            status: Status::NotApplicable,
            statistic: None,
            calibration: None,
            rationale: why.into(),
            details: serde_json::Value::Null,
            duration_ms: 0,
        }
    }
}

/// Tool metadata stamped on every inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Binary name.
    pub name: String,
    /// Binary version.
    pub version: String,
}

/// The complete output for one asset. A list of evidence; no aggregate score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inspection {
    /// Verdict schema version.
    pub schema_version: String,
    /// Tool that produced it.
    pub tool: ToolInfo,
    /// Asset summary.
    pub asset: AssetInfo,
    /// One entry per source that was run, in execution order.
    pub evidence: Vec<Evidence>,
    /// RFC 3339 timestamp.
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_roundtrips_json() {
        let e = Evidence::not_applicable(
            Layer::Mark,
            SourceId {
                name: "dwtdct".into(),
                version: "0.1.0".into(),
            },
            "no image data",
        );
        let s = serde_json::to_string(&e).unwrap();
        let back: Evidence = serde_json::from_str(&s).unwrap();
        assert_eq!(back.status, Status::NotApplicable);
        assert!(!s.contains("statistic"), "None fields must be omitted");
    }

    #[test]
    fn layers_order_cheapest_first() {
        assert!(Layer::Manifest < Layer::Container);
        assert!(Layer::Container < Layer::Mark);
        assert!(Layer::Mark < Layer::Blind);
    }
}
