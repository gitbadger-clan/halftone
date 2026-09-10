//! The batch envelope: one document for a set of inspected files, with a per-file
//! summary row derived from the evidence so downstream consumers (the CLI matrix,
//! the signed report, the differential runner) never dig into `details` themselves.
//!
//! A row carries every source's [`Status`] in execution order plus the named columns
//! the metadata-marking scan is about: the manifest layer's `validation_state`,
//! signer, claim generator and declared `digitalSourceType`, and the container
//! layer's XMP `DigitalSourceType`, packet count and whether a manifest container is
//! present. Nothing here aggregates across sources; the row is a projection, not a
//! score.
//!
//! Column derivation reads `details` by field name and tolerates absence: a source
//! that did not run, or an older tool version without a field, yields `None`, never
//! a fabricated value.

use serde::{Deserialize, Serialize};

use crate::{Inspection, Layer, Status, ToolInfo, SCHEMA_VERSION};

/// Source name of the manifest layer, as registered by the CLI.
pub const SOURCE_C2PA: &str = "c2pa";
/// Source name of the XMP marking source.
pub const SOURCE_MARKING: &str = "marking_metadata";

/// One source's status in a summary row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    /// Source name (`c2pa`, `jpeg_quant`, …).
    pub source: String,
    /// Layer it belongs to.
    pub layer: Layer,
    /// Its verdict on this file.
    pub status: Status,
}

/// Named columns from the manifest layer (`c2pa`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestColumns {
    /// `Trusted`, `Valid` or `Invalid` as reported by the validator; `None` when no
    /// manifest was found or the layer did not run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_state: Option<String>,
    /// Certificate issuer of the signer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// Claim generator (v1 string or v2 `claim_generator_info` name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_generator: Option<String>,
    /// Distinct `digitalSourceType` term codes declared in the active manifest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub digital_source_type: Vec<String>,
    /// Any declared term is generative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declares_ai: Option<bool>,
    /// The file carries only a reference to a remote manifest at this URL; nothing
    /// was fetched, so no `validation_state` exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_manifest_url: Option<String>,
}

/// Named columns from the XMP marking source (`marking_metadata`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkingColumns {
    /// Distinct `DigitalSourceType` term codes found in XMP.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub digital_source_type: Vec<String>,
    /// XMP packets examined; `Some(0)` means the file has no XMP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xmp_packets: Option<u64>,
    /// A C2PA container (JUMBF / `caBX` / `C2PA`) is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_container_present: Option<bool>,
}

/// Per-file summary row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRow {
    /// Path as given, if the asset had one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// SHA-256 of the bytes.
    pub sha256: String,
    /// Sniffed MIME type.
    pub mime: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Every source's status, in execution order.
    pub statuses: Vec<SourceStatus>,
    /// Names of sources that reported `Present`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub present: Vec<String>,
    /// Manifest-layer columns.
    pub manifest: ManifestColumns,
    /// XMP-marking columns.
    pub marking: MarkingColumns,
}

/// A set of inspections plus their summary rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    /// Verdict schema version (shared with [`Inspection`]).
    pub schema_version: String,
    /// Tool that produced it.
    pub tool: ToolInfo,
    /// One row per inspection, same order.
    pub summary: Vec<FileRow>,
    /// The full inspections.
    pub inspections: Vec<Inspection>,
    /// RFC 3339 timestamp.
    pub created_at: String,
}

impl Batch {
    /// Build a batch from inspections, deriving the summary rows.
    pub fn new(tool: ToolInfo, inspections: Vec<Inspection>) -> Self {
        let summary = inspections.iter().map(summarize).collect();
        Self {
            schema_version: SCHEMA_VERSION.into(),
            tool,
            summary,
            inspections,
            created_at: crate::registry::now_rfc3339(),
        }
    }

    /// Source names in the order they first appear across rows.
    pub fn columns(&self) -> Vec<String> {
        let mut cols: Vec<String> = Vec::new();
        for row in &self.summary {
            for s in &row.statuses {
                if !cols.contains(&s.source) {
                    cols.push(s.source.clone());
                }
            }
        }
        cols
    }

    /// Number of rows with at least one `Present`.
    pub fn files_with_present(&self) -> usize {
        self.summary
            .iter()
            .filter(|r| !r.present.is_empty())
            .count()
    }

    /// Number of rows with a `Present` in the given layer.
    pub fn files_with_present_in(&self, layer: Layer) -> usize {
        self.summary
            .iter()
            .filter(|r| {
                r.statuses
                    .iter()
                    .any(|s| s.layer == layer && s.status == Status::Present)
            })
            .count()
    }
}

/// Derive one summary row from an inspection.
pub fn summarize(insp: &Inspection) -> FileRow {
    let mut manifest = ManifestColumns::default();
    let mut marking = MarkingColumns::default();
    let mut statuses = Vec::with_capacity(insp.evidence.len());
    let mut present = Vec::new();
    for e in &insp.evidence {
        statuses.push(SourceStatus {
            source: e.source.name.clone(),
            layer: e.layer,
            status: e.status,
        });
        if e.status == Status::Present {
            present.push(e.source.name.clone());
        }
        let d = &e.details;
        match e.source.name.as_str() {
            SOURCE_C2PA => {
                manifest.validation_state = str_field(d, "validation_state");
                manifest.issuer = str_field(d, "issuer");
                manifest.claim_generator = str_field(d, "claim_generator");
                manifest.digital_source_type = str_list(d, "digital_source_type");
                manifest.declares_ai = d.get("declares_ai").and_then(|v| v.as_bool());
                manifest.remote_manifest_url = str_field(d, "remote_manifest_url");
            }
            SOURCE_MARKING => {
                marking.digital_source_type = str_list(d, "digital_source_type");
                marking.xmp_packets = d.get("xmp_packets").and_then(|v| v.as_u64());
                marking.manifest_container_present = d
                    .get("manifest_container_present")
                    .and_then(|v| v.as_bool());
            }
            _ => {}
        }
    }
    FileRow {
        path: insp
            .asset
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        sha256: insp.asset.sha256.clone(),
        mime: insp.asset.mime.clone(),
        size_bytes: insp.asset.size_bytes,
        statuses,
        present,
        manifest,
        marking,
    }
}

fn str_field(d: &serde_json::Value, key: &str) -> Option<String> {
    d.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn str_list(d: &serde_json::Value, key: &str) -> Vec<String> {
    d.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Render the file × source matrix for a terminal. `glyph` maps a status to its
/// one-character mark; the caller owns the character set. Columns are numbered and
/// listed in a legend so long source names do not widen the grid.
pub fn render_matrix(batch: &Batch, glyph: impl Fn(Status) -> &'static str) -> String {
    let cols = batch.columns();
    let mut out = String::new();
    if cols.is_empty() {
        out.push_str("(no evidence)\n");
        return out;
    }
    let legend: Vec<String> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{} {c}", i + 1))
        .collect();
    out.push_str("columns: ");
    out.push_str(&legend.join(" · "));
    out.push('\n');

    let name_w = batch
        .summary
        .iter()
        .map(|r| display_name(r).chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 40);
    let cell_w = if cols.len() >= 10 { 3 } else { 2 };

    out.push_str(&format!("{:<name_w$} ", "file"));
    for i in 1..=cols.len() {
        out.push_str(&format!("{:>cell_w$}", i));
    }
    out.push('\n');

    for row in &batch.summary {
        let name = truncate(&display_name(row), name_w);
        out.push_str(&format!("{name:<name_w$} "));
        for c in &cols {
            let g = row
                .statuses
                .iter()
                .find(|s| &s.source == c)
                .map(|s| glyph(s.status))
                .unwrap_or(" ");
            out.push_str(&format!("{g:>cell_w$}"));
        }
        out.push('\n');
        let detail = detail_line(row);
        if !detail.is_empty() {
            out.push_str(&format!("{:<name_w$} {detail}\n", ""));
        }
    }
    // Per-layer counts: a fingerprint match in the container layer is a Present
    // too, and lumping it with a valid manifest misleads.
    out.push_str(&format!(
        "{} file{}: manifest present {}, mark present {}, blind present {}, container {}\n",
        batch.summary.len(),
        if batch.summary.len() == 1 { "" } else { "s" },
        batch.files_with_present_in(Layer::Manifest),
        batch.files_with_present_in(Layer::Mark),
        batch.files_with_present_in(Layer::Blind),
        batch.files_with_present_in(Layer::Container),
    ));
    out
}

fn display_name(row: &FileRow) -> String {
    match &row.path {
        Some(p) => std::path::Path::new(p)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.clone()),
        None => row.sha256.chars().take(12).collect(),
    }
}

fn truncate(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        s.to_string()
    } else {
        let keep: String = s.chars().skip(n - (w - 1)).collect();
        format!("…{keep}")
    }
}

/// One line of named columns under a row; empty when neither layer said anything.
fn detail_line(row: &FileRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    let m = &row.manifest;
    if let Some(url) = &m.remote_manifest_url {
        let host = url
            .split("://")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .unwrap_or(url);
        parts.push(format!("manifest remote at {host}, not fetched"));
    }
    if let Some(state) = &m.validation_state {
        let mut s = format!("manifest {state}");
        if let Some(g) = &m.claim_generator {
            s.push_str(&format!(" · {g}"));
        }
        if let Some(i) = &m.issuer {
            s.push_str(&format!(" · signed by {i}"));
        }
        if !m.digital_source_type.is_empty() {
            s.push_str(&format!(" · {}", m.digital_source_type.join(", ")));
        }
        parts.push(s);
    }
    let k = &row.marking;
    if let Some(n) = k.xmp_packets {
        let mut s = if n == 0 {
            "xmp none".to_string()
        } else if k.digital_source_type.is_empty() {
            "xmp present, no DigitalSourceType".to_string()
        } else {
            format!("xmp {}", k.digital_source_type.join(", "))
        };
        if k.manifest_container_present == Some(true) && m.validation_state.is_none() {
            s.push_str(" · c2pa container present");
        }
        parts.push(s);
    }
    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetInfo, Evidence, Modality, SourceId};
    use serde_json::json;

    fn tool() -> ToolInfo {
        ToolInfo {
            name: "halftone".into(),
            version: "0.1.0".into(),
        }
    }

    fn ev(layer: Layer, name: &str, status: Status, details: serde_json::Value) -> Evidence {
        Evidence {
            layer,
            source: SourceId {
                name: name.into(),
                version: "0.1.0".into(),
            },
            status,
            statistic: None,
            calibration: None,
            rationale: String::new(),
            details,
            duration_ms: 0,
        }
    }

    fn insp(path: &str, evidence: Vec<Evidence>) -> Inspection {
        Inspection {
            schema_version: SCHEMA_VERSION.into(),
            tool: tool(),
            asset: AssetInfo {
                path: Some(std::path::PathBuf::from(path)),
                modality: Modality::Image,
                mime: "image/png".into(),
                sha256: "ab".repeat(32),
                size_bytes: 1234,
            },
            evidence,
            created_at: "2026-09-08T00:00:00Z".into(),
        }
    }

    fn chatgpt_png() -> Inspection {
        insp(
            "/tmp/navy_suit.png",
            vec![
                ev(
                    Layer::Manifest,
                    "c2pa",
                    Status::Present,
                    json!({
                        "validation_state": "Trusted",
                        "issuer": "OpenAI OpCo, LLC",
                        "claim_generator": "OpenAI Media Service API",
                        "digital_source_type": ["trainedAlgorithmicMedia"],
                        "declares_ai": true
                    }),
                ),
                ev(
                    Layer::Container,
                    "png_writer",
                    Status::Inconclusive,
                    json!({}),
                ),
                ev(
                    Layer::Container,
                    "marking_metadata",
                    Status::Absent,
                    json!({
                        "digital_source_type": [],
                        "xmp_packets": 0,
                        "manifest_container_present": true
                    }),
                ),
                ev(
                    Layer::Blind,
                    "feature_probe",
                    Status::NotApplicable,
                    serde_json::Value::Null,
                ),
            ],
        )
    }

    #[test]
    fn row_projects_the_named_columns() {
        let row = summarize(&chatgpt_png());
        assert_eq!(row.path.as_deref(), Some("/tmp/navy_suit.png"));
        assert_eq!(row.present, ["c2pa"]);
        assert_eq!(row.manifest.validation_state.as_deref(), Some("Trusted"));
        assert_eq!(
            row.manifest.claim_generator.as_deref(),
            Some("OpenAI Media Service API")
        );
        assert_eq!(
            row.manifest.digital_source_type,
            ["trainedAlgorithmicMedia"]
        );
        assert_eq!(row.manifest.declares_ai, Some(true));
        assert!(row.marking.digital_source_type.is_empty());
        assert_eq!(row.marking.xmp_packets, Some(0));
        assert_eq!(row.marking.manifest_container_present, Some(true));
        assert_eq!(row.statuses.len(), 4);
        assert_eq!(row.statuses[2].layer, Layer::Container);
    }

    #[test]
    fn missing_details_yield_none_not_defaults() {
        let i = insp(
            "/tmp/x.jpg",
            vec![ev(
                Layer::Manifest,
                "c2pa",
                Status::Absent,
                serde_json::Value::Null,
            )],
        );
        let row = summarize(&i);
        assert_eq!(row.manifest, ManifestColumns::default());
        assert_eq!(row.marking, MarkingColumns::default());
        assert!(row.present.is_empty());
    }

    #[test]
    fn batch_carries_schema_columns_and_counts() {
        let b = Batch::new(
            tool(),
            vec![
                chatgpt_png(),
                insp(
                    "/tmp/plain.png",
                    vec![ev(
                        Layer::Container,
                        "png_writer",
                        Status::Absent,
                        json!({}),
                    )],
                ),
            ],
        );
        assert_eq!(b.schema_version, SCHEMA_VERSION);
        assert_eq!(
            b.columns(),
            ["c2pa", "png_writer", "marking_metadata", "feature_probe"]
        );
        assert_eq!(b.files_with_present(), 1);
        assert_eq!(b.created_at.len(), 20);
        let js = serde_json::to_value(&b).unwrap();
        assert_eq!(js["summary"][0]["manifest"]["validation_state"], "Trusted");
        assert!(js["summary"][1]["manifest"]
            .get("validation_state")
            .is_none());
        let back: Batch = serde_json::from_value(js).unwrap();
        assert_eq!(back.summary, b.summary);
    }

    #[test]
    fn matrix_has_legend_grid_and_detail_lines() {
        let b = Batch::new(tool(), vec![chatgpt_png()]);
        let glyph = |s: Status| match s {
            Status::Present => "*",
            Status::Absent => "o",
            Status::Inconclusive => "?",
            Status::NotApplicable => "-",
        };
        let text = render_matrix(&b, glyph);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("columns: 1 c2pa · 2 png_writer"));
        assert!(lines[1].starts_with("file"));
        assert!(lines[2].starts_with("navy_suit.png"));
        assert!(lines[2].ends_with("* ? o -"), "{}", lines[2]);
        assert!(lines[3].contains("manifest Trusted · OpenAI Media Service API · signed by OpenAI OpCo, LLC · trainedAlgorithmicMedia"));
        assert!(lines[3].contains("| xmp none"));
        assert!(
            !lines[3].contains("c2pa container present"),
            "redundant when manifest column is set"
        );
        assert_eq!(
            lines[4],
            "1 file: manifest present 1, mark present 0, blind present 0, container 0"
        );
    }

    #[test]
    fn remote_manifest_reference_is_shown() {
        let i = insp(
            "/tmp/ff.png",
            vec![ev(
                Layer::Manifest,
                "c2pa",
                Status::Inconclusive,
                json!({"remote_manifest_url": "https://cai-manifests.adobe.com/manifests/urn-x"}),
            )],
        );
        let row = summarize(&i);
        assert_eq!(
            row.manifest.remote_manifest_url.as_deref(),
            Some("https://cai-manifests.adobe.com/manifests/urn-x")
        );
        let b = Batch::new(tool(), vec![i]);
        let text = render_matrix(&b, |_| "x");
        assert!(text.contains("manifest remote at cai-manifests.adobe.com, not fetched"));
    }

    #[test]
    fn matrix_points_to_container_when_manifest_layer_absent() {
        let i = insp(
            "/tmp/a.png",
            vec![ev(
                Layer::Container,
                "marking_metadata",
                Status::Absent,
                json!({"digital_source_type": [], "xmp_packets": 0, "manifest_container_present": true}),
            )],
        );
        let b = Batch::new(tool(), vec![i]);
        let text = render_matrix(&b, |_| "x");
        assert!(text.contains("xmp none · c2pa container present"));
    }

    #[test]
    fn long_names_are_truncated_from_the_left() {
        assert_eq!(truncate("abcdefghij", 6), "…fghij");
        assert_eq!(truncate("abc", 6), "abc");
    }
}
