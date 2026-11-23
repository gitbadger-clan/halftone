//! The verdict schema. Semver'd independently of the crates.
//!
//! [`SCHEMA_VERSION`] covers what `schemas/verdict.schema.json` and
//! `schemas/batch.schema.json` define: the inspection envelope, the fields of an
//! evidence entry, and the batch summary rows. Bump it when one of those changes, and
//! keep both files in sync. An addition (a new field or summary-row column) is a minor
//! version, as 1.2.0 was for the certificate columns and 1.5.0 for the conditions of a
//! run (`tool.build` and `run`); a removed or changed field is a major one.
//!
//! An evidence entry's `details` is outside that contract. The schema declares it as
//! any value: it is each source's own format, and a source may add, rename or reshape
//! what it puts there, or mask it, without a schema version. A new source needs none
//! either. The only version `details` carries is the entry's `source.version`, the
//! crate version. A value that consumers should be able to rely on belongs in a
//! summary-row column, which the schema does define and version.

use serde::{Deserialize, Serialize};

use crate::{AssetInfo, RunInfo};

/// Version of the JSON verdict schema emitted by [`Inspection`].
pub const SCHEMA_VERSION: &str = "1.5.0";

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

impl Layer {
    /// All four, in execution order.
    pub const ALL: [Layer; 4] = [Layer::Manifest, Layer::Container, Layer::Mark, Layer::Blind];

    /// What was not done for a file when no source reported in this layer, as a
    /// clause for a sentence.
    pub fn not_done(self) -> &'static str {
        match self {
            Layer::Manifest => "no signed manifest was looked for",
            Layer::Container => "the file's structure and metadata were not read",
            Layer::Mark => "no watermark was looked for",
            Layer::Blind => "no classifier was run",
        }
    }
}

/// The layers of `asked` in which no source reported, in the order asked.
///
/// The registry writes a row for every source of a layer that ran, a
/// `not_applicable` one when the source does not read the file. So a layer that was
/// asked for and has no row at all had no source: nothing was looked for there, which
/// is not the same as nothing being found.
pub fn empty_layers(asked: &[Layer], has_row: impl Fn(Layer) -> bool) -> Vec<Layer> {
    asked.iter().copied().filter(|l| !has_row(*l)).collect()
}

/// `a`, `a and b`, `a, b and c`.
fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// One sentence for the layers [`empty_layers`] found, or `None` when there are
/// none. The same words in the terminal, on the report page and in the PDF.
pub fn empty_layers_note(layers: &[Layer]) -> Option<String> {
    if layers.is_empty() {
        return None;
    }
    let names: Vec<String> = layers.iter().map(|l| format!("{l:?}")).collect();
    let clauses: Vec<String> = layers.iter().map(|l| l.not_done().to_string()).collect();
    let mut not_done = listed(&clauses);
    if let Some(first) = not_done.get(..1) {
        not_done = first.to_uppercase() + &not_done[1..];
    }
    Some(format!(
        "No source in this build: {}. {not_done}; a layer with no source says nothing \
         about the file.",
        names.join(", ")
    ))
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
    /// Stable machine name, e.g. `c2pa`, `jpeg_quant`, `marking_metadata`.
    pub name: String,
    /// Source/model version, e.g. `1.1.0` or `2026.08`.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Binary name.
    pub name: String,
    /// Binary version.
    pub version: String,
    /// Which build of that version this is. Absent from documents written before
    /// schema 1.5.0 and from a caller that does not know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildInfo>,
}

/// Which build produced an inspection. A version number covers every build made
/// while that number stood; the commit separates them.
///
/// This is the binary's own statement, recorded when it was compiled. Nothing in an
/// inspection verifies it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    /// The git commit the source was at: 40 hex digits, or 64 in a SHA-256
    /// repository. Absent when the build could not tell: no git checkout and no
    /// packaging record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Whether the source differed from that commit when the binary was built. When
    /// true the commit names where the source started from, not what was compiled.
    /// Absent when there is no commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<bool>,
    /// Compile-time features that change what is reported (`c2pa`: manifest
    /// validation is built in). Empty when none is enabled.
    #[serde(default)]
    pub features: Vec<String>,
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
    /// The conditions the evidence holds under: options, trust lists, installed
    /// packs. Absent from documents written before schema 1.5.0 and from a
    /// [`crate::Registry`] that was not given one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunInfo>,
    /// RFC 3339 timestamp.
    pub created_at: String,
}

impl Inspection {
    /// The layers this inspection was asked for in which no source reported. Empty
    /// when the inspection carries no run record: what was asked for is then not
    /// known, and a missing layer cannot be told from one that was left out.
    pub fn empty_layers(&self) -> Vec<Layer> {
        match &self.run {
            Some(run) => empty_layers(&run.options.layers, |l| {
                self.evidence.iter().any(|e| e.layer == l)
            }),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_that_was_asked_for_and_has_no_row_is_named() {
        let row = |layer| {
            Evidence::not_applicable(
                layer,
                SourceId {
                    name: "s".into(),
                    version: "1".into(),
                },
                "source does not support this asset",
            )
        };
        let rows = [row(Layer::Manifest), row(Layer::Container)];
        let has = |l: Layer| rows.iter().any(|e| e.layer == l);
        // A not-applicable row is a row: the layer had a source.
        assert_eq!(empty_layers(&Layer::ALL, has), [Layer::Mark, Layer::Blind]);
        // A layer that was not asked for is not empty; it did not run.
        assert_eq!(empty_layers(&[Layer::Container], has), []);
        assert_eq!(empty_layers(&[Layer::Blind], has), [Layer::Blind]);

        assert_eq!(empty_layers_note(&[]), None);
        assert_eq!(
            empty_layers_note(&[Layer::Mark, Layer::Blind]).unwrap(),
            "No source in this build: Mark, Blind. No watermark was looked for and no \
             classifier was run; a layer with no source says nothing about the file."
        );
        assert_eq!(
            empty_layers_note(&[Layer::Blind]).unwrap(),
            "No source in this build: Blind. No classifier was run; a layer with no source \
             says nothing about the file."
        );
        assert_eq!(
            empty_layers_note(&Layer::ALL).unwrap(),
            "No source in this build: Manifest, Container, Mark, Blind. No signed manifest \
             was looked for, the file's structure and metadata were not read, no watermark \
             was looked for and no classifier was run; a layer with no source says nothing \
             about the file."
        );
    }

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
    fn a_document_from_before_1_5_still_reads_and_says_nothing_about_the_run() {
        let old = r#"{"schema_version":"1.4.0","tool":{"name":"halftone","version":"0.1.0"},
            "asset":{"path":null,"modality":"image","mime":"image/png","sha256":"00","size_bytes":1},
            "evidence":[],"created_at":"2026-11-01T00:00:00Z"}"#;
        let i: Inspection = serde_json::from_str(old).unwrap();
        assert_eq!(i.tool.build, None);
        assert_eq!(i.run, None);
        // Written back, the absent fields stay absent: nothing is made up for them.
        let js = serde_json::to_value(&i).unwrap();
        assert!(js.get("run").is_none());
        assert!(js["tool"].get("build").is_none());
    }

    #[test]
    fn a_build_without_a_commit_says_so_by_leaving_it_out() {
        let b = BuildInfo {
            commit: None,
            modified: None,
            features: vec!["c2pa".into()],
        };
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            serde_json::json!({ "features": ["c2pa"] })
        );
        let known = BuildInfo {
            commit: Some("a".repeat(40)),
            modified: Some(true),
            features: vec![],
        };
        let js = serde_json::to_value(&known).unwrap();
        assert_eq!(js["modified"], true);
        assert_eq!(js["features"], serde_json::json!([]));
    }

    #[test]
    fn layers_order_cheapest_first() {
        assert!(Layer::Manifest < Layer::Container);
        assert!(Layer::Container < Layer::Mark);
        assert!(Layer::Mark < Layer::Blind);
    }
}
