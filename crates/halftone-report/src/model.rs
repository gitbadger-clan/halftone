//! What a report says, apart from how it looks.
//!
//! [`Report::new`] turns an inspection into the words, rows and tables of its report.
//! The HTML page and the PDF are two drawings of this one value, so they cannot say
//! different things: a sentence is written once, here or in the module that owns its
//! part of the report ([`crate::conditions`] for the file block, the notes and the
//! conditions of the check).
//!
//! The model is plain data and serializes to JSON; the PDF template reads it in that
//! form. Text is never markup: a file name or a rationale reaches both renderers as a
//! string, and each escapes it for its own format.

use halftone_core::{Inspection, Status};
use serde::Serialize;

/// A run of text, part of a [`Rich`] value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum Span {
    /// Ordinary words.
    Text(String),
    /// A literal machine value: a hash, a commit, a command-line option.
    Code(String),
    /// A line break between two entries of one cell.
    Break,
}

/// A cell's content: words with literal values set apart.
pub type Rich = Vec<Span>;

/// Plain words as a [`Rich`] value.
pub fn text(s: impl Into<String>) -> Rich {
    vec![Span::Text(s.into())]
}

/// One labelled row of a two-column table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    /// The row's label.
    pub label: String,
    /// Its value.
    pub value: Rich,
}

impl Field {
    /// A row from a label and a value.
    pub fn new(label: &str, value: Rich) -> Self {
        Self {
            label: label.to_string(),
            value,
        }
    }
}

/// One source's row in the evidence table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceRow {
    /// The verdict; picks the glyph.
    pub status: Status,
    /// The verdict as the report words it.
    pub word: String,
    /// Source name.
    pub source: String,
    /// Source version.
    pub version: String,
    /// The source's rationale, as written.
    pub rationale: String,
}

/// The sources of one layer, in execution order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerGroup {
    /// The layer's name.
    pub layer: String,
    /// Its sources.
    pub rows: Vec<EvidenceRow>,
}

/// One entry of the legend under the evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegendItem {
    /// The status explained; picks the glyph.
    pub status: Status,
    /// The status as the report words it.
    pub word: String,
    /// What it means.
    pub text: String,
}

/// One check of a conformance rubric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// The rubric's name for the check.
    pub name: String,
    /// `met`, `not met`, `warning` or `not evaluated`.
    pub result: String,
    /// The rubric's report text.
    pub text: String,
}

/// One rubric: a heading with its counts, and its checks, those not met first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rubric {
    /// Name, version and counts, or why it could not be evaluated.
    pub heading: String,
    /// The checks; empty when the rubric could not be evaluated.
    pub checks: Vec<Check>,
}

/// The C2PA conformance measurements of the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Conformance {
    /// The rubrics could not be run; why.
    NotMeasured {
        /// What the section is called.
        heading: String,
        /// The reason given.
        reason: String,
    },
    /// The rubrics ran.
    Measured {
        /// What the section is called.
        heading: String,
        /// Every rubric's heading in one line.
        summary: String,
        /// What kind of statement the section is.
        caveat: String,
        /// What the measurements are, in the rubric evaluator's words.
        about: String,
        /// The rubrics.
        rubrics: Vec<Rubric>,
    },
}

/// One trust input in the conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrustRow {
    /// What it is used for.
    pub used_for: String,
    /// Where this copy came from.
    pub copy: String,
    /// File name.
    pub file: String,
    /// Its version, or `not stated`.
    pub version: String,
    /// Certificate count; empty for the key-usage policy.
    pub certificates: String,
    /// SHA-256 of the bytes as loaded.
    pub sha256: String,
}

/// The trust lists of a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Trust {
    /// The inspection carries no run record.
    NotRecorded {
        /// Said in place of the lists.
        sentence: String,
    },
    /// No list was loaded.
    NoneLoaded {
        /// That none was loaded, and why.
        sentence: String,
    },
    /// The lists that were loaded.
    Lists {
        /// One row per input.
        rows: Vec<TrustRow>,
    },
}

/// The conditions of the check: the collapsed section of the page, an appendix of the
/// PDF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conditions {
    /// What the section is called.
    pub heading: String,
    /// The record in one line.
    pub summary: String,
    /// Build, options and packs.
    pub rows: Vec<Field>,
    /// The trust lists.
    pub trust: Trust,
    /// Whose statement the section is.
    pub statement: String,
}

/// Everything a report says about one inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The document's title.
    pub title: String,
    /// The file's name, without its directory.
    pub file_name: String,
    /// The file's SHA-256.
    pub sha256: String,
    /// When the check ran and with which version, as a sentence.
    pub checked: String,
    /// When the check ran (RFC 3339); the PDF's own date.
    pub created_at: String,
    /// The tool's name and version.
    pub tool: String,
    /// The file block.
    pub file: Vec<Field>,
    /// What differs from a default check. Empty for a default one.
    pub notes: Vec<String>,
    /// The sentence that introduces the notes.
    pub notes_lead: String,
    /// The evidence, a group per layer.
    pub evidence: Vec<LayerGroup>,
    /// The layers that were asked for and had no source, as one sentence. `None`
    /// when every layer asked for reported, or when the inspection does not record
    /// what was asked for.
    pub empty_layers: Option<String>,
    /// What the statuses mean.
    pub legend: Vec<LegendItem>,
    /// The legend's closing sentences.
    pub legend_close: String,
    /// What the file says about itself, when a metadata source ran.
    pub declared: Option<crate::declared::Declared>,
    /// The conformance measurements, when the manifest source made any.
    pub conformance: Option<Conformance>,
    /// The conditions of the check.
    pub conditions: Conditions,
    /// The fixed notice.
    pub notice: String,
    /// The line at the foot of every printed page: which file the page belongs to.
    pub page_footer: String,
}

impl Report {
    /// The report of one inspection.
    pub fn new(insp: &Inspection) -> Self {
        let file_name = crate::conditions::file_name(insp);
        Self {
            title: format!("Halftone report: {file_name}"),
            checked: format!(
                "Checked {} with {} {}.",
                insp.created_at, insp.tool.name, insp.tool.version
            ),
            created_at: insp.created_at.clone(),
            tool: format!("{} {}", insp.tool.name, insp.tool.version),
            file: crate::conditions::file_fields(insp),
            notes: crate::conditions::notes(insp),
            notes_lead: crate::NOTES_LEAD.to_string(),
            evidence: crate::evidence_groups(&insp.evidence),
            empty_layers: halftone_core::empty_layers_note(&insp.empty_layers()),
            legend: crate::legend(),
            legend_close: crate::LEGEND_CLOSE.to_string(),
            declared: crate::declared::declared(&insp.evidence),
            conformance: insp
                .evidence
                .iter()
                .find(|e| e.source.name == "c2pa")
                .and_then(|e| e.details.get("conformance"))
                .and_then(crate::conformance),
            conditions: crate::conditions::conditions(insp),
            notice: crate::NOTICE.to_string(),
            page_footer: format!(
                "Halftone report: {}, SHA-256 {}",
                clipped(&file_name, 48),
                clipped(&insp.asset.sha256, 17)
            ),
            sha256: insp.asset.sha256.clone(),
            file_name,
        }
    }
}

/// At most `max` characters of `s`, the rest shown as an ellipsis.
pub fn clipped(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}
