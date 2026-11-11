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
//!
//! [`render_matrix`] shows every source's status per file; [`render_table`] shows the
//! named columns, one line per file, aligned for reading or tab-separated.

use serde::{Deserialize, Serialize};

use crate::{Inspection, Layer, SCHEMA_VERSION, Status, ToolInfo};

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
    /// Any manifest on the active manifest's `parentOf` lineage declares a
    /// generative term; the active manifest alone may not (DIFFERENTIAL.md D-013).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declares_ai: Option<bool>,
    /// Steps back from the active manifest to the nearest manifest declaring a
    /// generative term; 0 is the active manifest itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generative_depth: Option<u64>,
    /// Distinct term codes declared behind the active manifest on its lineage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineage_digital_source_type: Vec<String>,
    /// The file carries only a reference to a remote manifest at this URL; nothing
    /// was fetched, so no `validation_state` exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_manifest_url: Option<String>,
    /// The manifest was not in the file but fetched from this URL at inspection time
    /// (`--fetch-remote-manifests`); `validation_state` describes the fetched copy and
    /// can differ on a later check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_from: Option<String>,
    /// Serial of the certificate that signed the active manifest, uppercase hex as
    /// `openssl x509 -serial` prints it. Files signed with the same certificate share
    /// a validity window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_cert_serial: Option<String>,
    /// End of that certificate's validity window (RFC 3339). Without a trusted
    /// time-stamp, a check after this time reports the certificate expired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_cert_not_after: Option<String>,
    /// Time-stamp status of the signature under the TSA list this run loaded:
    /// `trusted`, `untrusted`, `failed`, `validated_only` or `none`. Another TSA list
    /// can give another answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_stamp: Option<String>,
    /// Ingredients the active manifest lists; `Some(0)` is a manifest with none,
    /// `None` no manifest read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingredients: Option<u64>,
    /// Distinct actions in the active manifest that carry a `digitalSourceType`
    /// (`c2pa.created`, `c2pa.edited`, …), in the order they appear.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_type_actions: Vec<String>,
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
    /// For survival runs: the original file this row was derived from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// For survival runs: the transform applied to `source` (`none` is the
    /// untouched original; `captured:<label>` a file a real application produced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
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

/// Source-type text for a manifest column: the active manifest's terms, plus
/// where a generative declaration sits when it is behind the active manifest.
pub fn manifest_terms(m: &ManifestColumns, sep: &str) -> String {
    let mut s = m.digital_source_type.join(sep);
    if let Some(n) = m.generative_depth.filter(|&n| n > 0) {
        let back = if n == 1 {
            "one step".to_string()
        } else {
            format!("{n} steps")
        };
        let terms = m.lineage_digital_source_type.join(sep);
        if s.is_empty() {
            s = format!("{terms} {back} back");
        } else {
            s.push_str(&format!(" (lineage: {terms} {back} back)"));
        }
    }
    s
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
                manifest.generative_depth = d.get("generative_depth").and_then(|v| v.as_u64());
                manifest.lineage_digital_source_type = str_list(d, "lineage_digital_source_type");
                manifest.remote_manifest_url = str_field(d, "remote_manifest_url");
                manifest.fetched_from = str_field(d, "fetched_from");
                manifest.signing_cert_serial = nested_str(d, "signing_cert", "serial");
                manifest.signing_cert_not_after = nested_str(d, "signing_cert", "not_after");
                manifest.time_stamp = nested_str(d, "time_stamp", "status");
                manifest.ingredients = d.get("ingredients").and_then(|v| v.as_u64());
                for hit in d
                    .get("digital_source_type_hits")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    if let Some(a) = hit.get("action").and_then(|v| v.as_str()) {
                        if !manifest.source_type_actions.iter().any(|x| x == a) {
                            manifest.source_type_actions.push(a.to_string());
                        }
                    }
                }
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
        source: None,
        transform: None,
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

fn nested_str(d: &serde_json::Value, outer: &str, key: &str) -> Option<String> {
    d.get(outer)
        .and_then(|o| o.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
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

/// How [`render_table`] separates its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableFormat {
    /// Columns padded to a common width, two spaces apart, for a terminal.
    Aligned,
    /// Tab-separated with a header line, for `cut`, `sort` and spreadsheets.
    Tsv,
}

/// Header of [`render_table`], in column order.
pub const TABLE_COLUMNS: [&str; 7] = [
    "file",
    "manifest",
    "signer",
    "declared",
    "ingredients",
    "actions",
    "xmp",
];

/// The manifest column as one word: the validation state, `remote` when the file
/// only references a manifest that was not fetched, `-` when there is none.
pub fn manifest_cell(row: &FileRow) -> String {
    let m = &row.manifest;
    m.validation_state
        .clone()
        .or_else(|| m.remote_manifest_url.as_ref().map(|_| "remote".to_string()))
        .unwrap_or_else(|| "-".to_string())
}

/// The XMP column: the `DigitalSourceType` terms; `present` for XMP without the
/// field; `none` for a file without XMP; `n/a` when the marking source did not run
/// or does not read this format, so "not looked at" never reads as "nothing there".
pub fn xmp_cell(row: &FileRow) -> String {
    let looked = row
        .statuses
        .iter()
        .any(|s| s.source == SOURCE_MARKING && s.status != Status::NotApplicable);
    let k = &row.marking;
    match k.xmp_packets {
        _ if !looked => "n/a".to_string(),
        Some(n) if n > 0 && k.digital_source_type.is_empty() => "present".to_string(),
        Some(n) if n > 0 => k.digital_source_type.join(","),
        _ => "none".to_string(),
    }
}

/// One line per file with the named columns ([`TABLE_COLUMNS`]): what the manifest
/// layer and the XMP marking say, without digging into `details`. `declared` is
/// [`manifest_terms`], so a generative declaration behind the active manifest shows
/// with its distance (D-013). Empty cells print `-`. A projection of the rows:
/// nothing is aggregated or scored.
pub fn render_table(batch: &Batch, format: TableFormat) -> String {
    let dash = |s: String| if s.is_empty() { "-".to_string() } else { s };
    // Tabs and line breaks inside a value would break the columns.
    let clean = |s: String| s.replace(['\t', '\n', '\r'], " ");
    let mut lines: Vec<Vec<String>> = vec![TABLE_COLUMNS.iter().map(|c| c.to_string()).collect()];
    for row in &batch.summary {
        let m = &row.manifest;
        lines.push(
            [
                display_name(row),
                manifest_cell(row),
                dash(m.issuer.clone().unwrap_or_default()),
                dash(manifest_terms(m, ",")),
                m.ingredients
                    .map_or_else(|| "-".to_string(), |n| n.to_string()),
                dash(m.source_type_actions.join(",")),
                xmp_cell(row),
            ]
            .map(clean)
            .to_vec(),
        );
    }
    let mut out = String::new();
    match format {
        TableFormat::Tsv => {
            for l in &lines {
                out.push_str(&l.join("\t"));
                out.push('\n');
            }
        }
        TableFormat::Aligned => {
            let widths: Vec<usize> = (0..TABLE_COLUMNS.len())
                .map(|c| {
                    lines
                        .iter()
                        .map(|l| l[c].chars().count())
                        .max()
                        .unwrap_or(0)
                })
                .collect();
            for l in &lines {
                let mut line = String::new();
                for (c, cell) in l.iter().enumerate() {
                    if c + 1 == l.len() {
                        line.push_str(cell);
                    } else {
                        let w = widths[c];
                        line.push_str(&format!("{cell:<w$}  "));
                    }
                }
                out.push_str(line.trim_end());
                out.push('\n');
            }
        }
    }
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

/// Host part of a URL for display; the whole string when it has no scheme.
fn host_of(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or(url)
}

/// One line of named columns under a row; empty when neither layer said anything.
fn detail_line(row: &FileRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    let m = &row.manifest;
    if let Some(url) = &m.remote_manifest_url {
        parts.push(format!("manifest remote at {}, not fetched", host_of(url)));
    }
    if let Some(state) = &m.validation_state {
        let mut s = format!("manifest {state}");
        if let Some(g) = &m.claim_generator {
            s.push_str(&format!(" · {g}"));
        }
        if let Some(i) = &m.issuer {
            s.push_str(&format!(" · signed by {i}"));
        }
        let terms = manifest_terms(m, ", ");
        if !terms.is_empty() {
            s.push_str(&format!(" · {terms}"));
        }
        if let Some(url) = &m.fetched_from {
            s.push_str(&format!(" · fetched from {}", host_of(url)));
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
    fn row_projects_the_signing_window() {
        let i = insp(
            "/tmp/canva.png",
            vec![ev(
                Layer::Manifest,
                "c2pa",
                Status::Inconclusive,
                json!({
                    "validation_state": "Valid",
                    "signing_cert": {
                        "serial": "1D529F6E94B0FF2F728E5A20F5E91098A3C0CEF5",
                        "serial_decimal": "167403281942339131879710527931510703633591226101",
                        "not_before": "2026-10-07T23:53:55Z",
                        "not_after": "2026-10-15T23:53:55Z"
                    },
                    "time_stamp": { "status": "none", "codes": [] }
                }),
            )],
        );
        let row = summarize(&i);
        assert_eq!(
            row.manifest.signing_cert_serial.as_deref(),
            Some("1D529F6E94B0FF2F728E5A20F5E91098A3C0CEF5")
        );
        assert_eq!(
            row.manifest.signing_cert_not_after.as_deref(),
            Some("2026-10-15T23:53:55Z")
        );
        assert_eq!(row.manifest.time_stamp.as_deref(), Some("none"));
    }

    #[test]
    fn row_projects_ingredients_and_source_type_actions() {
        let i = insp(
            "/tmp/edit.jpg",
            vec![ev(
                Layer::Manifest,
                "c2pa",
                Status::Present,
                json!({
                    "validation_state": "Valid",
                    "ingredients": 2,
                    "digital_source_type_hits": [
                        { "action": "c2pa.created", "code": "digitalCapture" },
                        { "action": "c2pa.edited", "code": "compositeWithTrainedAlgorithmicMedia" },
                        { "action": "c2pa.edited", "code": "trainedAlgorithmicMedia" },
                        { "assertion": "stds.iptc", "code": "trainedAlgorithmicMedia" }
                    ]
                }),
            )],
        );
        let row = summarize(&i);
        assert_eq!(row.manifest.ingredients, Some(2));
        assert_eq!(
            row.manifest.source_type_actions,
            ["c2pa.created", "c2pa.edited"],
            "distinct, in order; a hit outside an action adds nothing"
        );
        // Both are optional in JSON: absent when there is nothing to say.
        let plain = serde_json::to_value(summarize(&insp("/tmp/x.jpg", vec![]))).unwrap();
        assert!(plain["manifest"].get("ingredients").is_none());
        assert!(plain["manifest"].get("source_type_actions").is_none());
    }

    fn table_batch() -> Batch {
        let marked = |status, details| ev(Layer::Container, "marking_metadata", status, details);
        Batch::new(
            tool(),
            vec![
                chatgpt_png(),
                insp(
                    "/tmp/dir/edit.jpg",
                    vec![
                        ev(
                            Layer::Manifest,
                            "c2pa",
                            Status::Present,
                            json!({
                                "validation_state": "Valid",
                                "issuer": "Example\tCA",
                                "ingredients": 1,
                                "generative_depth": 1,
                                "lineage_digital_source_type": ["trainedAlgorithmicMedia"],
                                "digital_source_type_hits": [{ "action": "c2pa.edited" }]
                            }),
                        ),
                        marked(
                            Status::Present,
                            json!({ "xmp_packets": 1, "digital_source_type": ["trainedAlgorithmicMedia"] }),
                        ),
                    ],
                ),
                insp(
                    "/tmp/remote.jpg",
                    vec![
                        ev(
                            Layer::Manifest,
                            "c2pa",
                            Status::Inconclusive,
                            json!({ "remote_manifest_url": "https://cai-manifests.adobe.com/x" }),
                        ),
                        marked(Status::Absent, json!({ "xmp_packets": 2 })),
                    ],
                ),
                insp(
                    "/tmp/clip.mp4",
                    vec![marked(Status::NotApplicable, serde_json::Value::Null)],
                ),
                insp("/tmp/manifest-only.png", vec![]),
            ],
        )
    }

    #[test]
    fn table_tsv_has_one_line_per_file_and_seven_columns() {
        let t = render_table(&table_batch(), TableFormat::Tsv);
        let lines: Vec<Vec<&str>> = t.lines().map(|l| l.split('\t').collect()).collect();
        assert_eq!(lines.len(), 6);
        assert!(lines.iter().all(|l| l.len() == 7), "{t}");
        assert_eq!(lines[0], TABLE_COLUMNS);
        assert_eq!(lines[1][0], "navy_suit.png");
        assert_eq!(lines[1][1], "Trusted");
        assert_eq!(lines[1][6], "none", "XMP looked at, none there");
        assert_eq!(
            lines[2],
            [
                "edit.jpg",
                "Valid",
                "Example CA",
                "trainedAlgorithmicMedia one step back",
                "1",
                "c2pa.edited",
                "trainedAlgorithmicMedia"
            ],
            "a tab inside a value becomes a space; lineage terms show their distance"
        );
        assert_eq!(&lines[3][1..], ["remote", "-", "-", "-", "-", "present"]);
        assert_eq!(
            lines[4][6], "n/a",
            "the marking source does not read this format"
        );
        assert_eq!(
            &lines[5][1..],
            ["-", "-", "-", "-", "-", "n/a"],
            "source did not run"
        );
    }

    #[test]
    fn table_aligned_pads_columns_and_has_no_trailing_space() {
        let t = render_table(&table_batch(), TableFormat::Aligned);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 6);
        assert!(lines.iter().all(|l| !l.ends_with(' ') && !l.contains('\t')));
        // Every column starts at the same character position on every line.
        let starts = |l: &str, word: &str| l.find(word).unwrap();
        assert_eq!(starts(lines[0], "manifest"), starts(lines[1], "Trusted"));
        assert_eq!(starts(lines[0], "manifest"), starts(lines[2], "Valid"));
        assert_eq!(starts(lines[0], "xmp"), starts(lines[4], "n/a"));
        assert!(lines[0].starts_with("file "));
        let empty = render_table(&Batch::new(tool(), vec![]), TableFormat::Aligned);
        assert_eq!(empty.lines().count(), 1, "header only");
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
        assert!(
            js["summary"][1]["manifest"]
                .get("validation_state")
                .is_none()
        );
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
    fn fetched_manifest_is_marked() {
        let i = insp(
            "/tmp/ff.png",
            vec![ev(
                Layer::Manifest,
                "c2pa",
                Status::Present,
                json!({
                    "validation_state": "Trusted",
                    "issuer": "Adobe Inc.",
                    "fetched_from": "https://cai-manifests.adobe.com/manifests/urn-x"
                }),
            )],
        );
        let row = summarize(&i);
        assert_eq!(
            row.manifest.fetched_from.as_deref(),
            Some("https://cai-manifests.adobe.com/manifests/urn-x")
        );
        assert_eq!(row.manifest.remote_manifest_url, None);
        let b = Batch::new(tool(), vec![i]);
        let text = render_matrix(&b, |_| "x");
        assert!(text.contains("manifest Trusted"), "{text}");
        assert!(
            text.contains("fetched from cai-manifests.adobe.com"),
            "{text}"
        );
        assert!(!text.contains("not fetched"), "{text}");
    }

    #[test]
    fn embedded_manifest_row_has_no_fetched_from() {
        let i = insp(
            "/tmp/g.png",
            vec![ev(
                Layer::Manifest,
                "c2pa",
                Status::Present,
                json!({"validation_state": "Trusted"}),
            )],
        );
        let row = summarize(&i);
        assert_eq!(row.manifest.fetched_from, None);
        let v = serde_json::to_value(&row).unwrap();
        assert!(v["manifest"].get("fetched_from").is_none());
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

    #[test]
    fn generative_lineage_shows_in_the_manifest_terms() {
        let e2 = ManifestColumns {
            digital_source_type: vec!["algorithmicallyEnhanced".into()],
            declares_ai: Some(true),
            generative_depth: Some(1),
            lineage_digital_source_type: vec!["trainedAlgorithmicMedia".into()],
            ..Default::default()
        };
        assert_eq!(
            manifest_terms(&e2, ", "),
            "algorithmicallyEnhanced (lineage: trainedAlgorithmicMedia one step back)"
        );
        let e1 = ManifestColumns {
            digital_source_type: vec!["trainedAlgorithmicMedia".into()],
            generative_depth: Some(0),
            ..Default::default()
        };
        assert_eq!(manifest_terms(&e1, ", "), "trainedAlgorithmicMedia");
    }
}
