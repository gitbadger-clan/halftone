//! What the file says about itself: the fields the metadata sources read, as tables.
//!
//! The evidence table gives each source's verdict and a rationale in words. This
//! section lists what stands behind five of those rationales, field by field:
//!
//! | Source | Table |
//! |---|---|
//! | `marking_metadata` | the IPTC digital source type |
//! | `data_mining_reservation` | the PLUS data-mining term |
//! | `vendor_ai_flag` | a vendor's own AI field or credit phrase |
//! | `rights_statements` | copyright, licence, credit and usage statements |
//! | `embedded_identifiers` | names, contacts, devices, documents, places |
//!
//! Nothing here is a finding about the file. Every field is unsigned: whoever wrote
//! or last saved the file could put anything in it. The section says so once, above
//! the tables.
//!
//! Free-text values, which can disclose something about a person, stay masked unless
//! the inspection was run with `--reveal`. This module decides nothing about that: a source writes
//! either a `value` or `masked` with a `length` into its `details`, and a row shows
//! which of the two it was given.
//!
//! `details` is not part of the verdict schema, so every read is defensive: a row or
//! table whose keys are not there is left out, and the report is still written. A
//! test in the CLI crate runs the real sources and checks the tables, so a renamed
//! key fails a test instead of silently emptying a section.

use halftone_core::Evidence;
use serde::Serialize;
use serde_json::Value;

use crate::html_escape;
use crate::model::{Rich, Span, text};

/// What the section is called.
pub const HEADING: &str = "What the file says about itself";

/// What kind of statement every row is.
pub const CAVEAT: &str = "These fields are unsigned. Whoever wrote or last saved the file \
could put anything in them or take them out, and nothing in this report verifies them. They \
are what the file says, not findings about it.";

/// Said when the sources ran and found none of their fields.
pub const NOTHING: &str = "nothing in the fields these sources read";

/// One source's fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Table {
    /// What the table lists.
    pub title: String,
    /// The source whose `details` it comes from.
    pub source: String,
    /// Column labels.
    pub columns: Vec<String>,
    /// One row per field found, a cell per column.
    pub rows: Vec<Vec<Rich>>,
    /// Sentences that belong under the table (copies that disagree, a free-text
    /// constraint).
    pub notes: Vec<String>,
}

/// The whole section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Declared {
    /// What the section is called.
    pub heading: String,
    /// What it holds, in one line.
    pub summary: String,
    /// What kind of statement every row is.
    pub caveat: String,
    /// The tables that have at least one row or note.
    pub tables: Vec<Table>,
}

fn code(s: &str) -> Rich {
    vec![Span::Code(s.to_string())]
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn list<'a>(details: &'a Value, key: &str) -> &'a [Value] {
    details
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// `copyright_notice` as `copyright notice`.
fn words(kind: &str) -> String {
    kind.replace('_', " ")
}

/// A value as its source gave it: the text, or that it is masked and how long it is.
/// `None` when the entry has neither, which is not a row.
fn value_cell(entry: &Value) -> Option<Rich> {
    if let Some(v) = str_of(entry, "value") {
        return Some(text(v));
    }
    let length = entry.get("length").and_then(Value::as_u64)?;
    let unit = if length == 1 {
        "character"
    } else {
        "characters"
    };
    Some(text(format!("masked, {length} {unit}")))
}

/// A term from a controlled vocabulary: the code, and the URI or text it was read from.
fn term_rows(details: &Value) -> Vec<Vec<Rich>> {
    list(details, "values")
        .iter()
        .filter_map(|v| {
            let term = str_of(v, "code")?;
            let raw = str_of(v, "raw").unwrap_or(term);
            let mut written = code(raw);
            if v.get("namespace_bound").and_then(Value::as_bool) == Some(false) {
                written.push(Span::Text(
                    " (its prefix is not bound to the standard's namespace)".to_string(),
                ));
            }
            Some(vec![code(term), written])
        })
        .collect()
}

fn source_type(details: &Value) -> Table {
    Table {
        title: "Digital source type (IPTC)".to_string(),
        source: "marking_metadata".to_string(),
        columns: vec!["Term".to_string(), "As written".to_string()],
        rows: term_rows(details),
        notes: Vec::new(),
    }
}

/// How the note on a masked Other Constraints text begins.
const CONSTRAINTS_MASKED: &str = "Other constraints: masked, ";

/// The Other Constraints text of a data-mining reservation, as a note under its
/// table: quoted when the source ran revealed, its length when the source masked it.
/// A bare string is how a document written before the source masked this text
/// carries it; it is quoted, since the document holds it already.
fn other_constraints(details: &Value) -> Option<String> {
    let entry = details.get("other_constraints")?;
    if let Some(c) = entry.as_str().or_else(|| str_of(entry, "value")) {
        return Some(format!("Other constraints, as written: {c}"));
    }
    let length = entry.get("length").and_then(Value::as_u64)?;
    let unit = if length == 1 {
        "character"
    } else {
        "characters"
    };
    Some(format!("{CONSTRAINTS_MASKED}{length} {unit}"))
}

fn data_mining(details: &Value) -> Table {
    let notes = other_constraints(details).into_iter().collect();
    Table {
        title: "Data-mining reservation (PLUS)".to_string(),
        source: "data_mining_reservation".to_string(),
        columns: vec!["Term".to_string(), "As written".to_string()],
        rows: term_rows(details),
        notes,
    }
}

fn vendor_flag(details: &Value) -> Table {
    let rows = list(details, "flags")
        .iter()
        .filter_map(|f| {
            let field = str_of(f, "field")?;
            let name = match str_of(f, "prefix") {
                Some(prefix) if prefix != field => format!("{prefix}:{field}"),
                _ => field.to_string(),
            };
            let says = f.get("says").and_then(Value::as_bool);
            let reads = match (says, str_of(f, "states")) {
                (Some(true), Some("edited")) => "the content was edited with AI",
                (Some(true), _) => "the content is AI-generated",
                (Some(false), _) => "the file does not contain AI-generated content",
                (None, _) => "a value this source does not recognise",
            };
            Some(vec![
                code(&name),
                text(str_of(f, "value").unwrap_or("")),
                text(reads),
            ])
        })
        .collect();
    Table {
        title: "Vendor AI field".to_string(),
        source: "vendor_ai_flag".to_string(),
        columns: vec![
            "Field".to_string(),
            "Value".to_string(),
            "States that".to_string(),
        ],
        rows,
        notes: Vec::new(),
    }
}

/// Rows of field, kind and value, for the two sources that list free text.
fn field_rows(details: &Value, key: &str) -> Vec<Vec<Rich>> {
    list(details, key)
        .iter()
        .filter_map(|e| {
            Some(vec![
                code(str_of(e, "field")?),
                text(words(str_of(e, "kind").unwrap_or(""))),
                value_cell(e)?,
            ])
        })
        .collect()
}

fn rights(details: &Value) -> Table {
    let notes = list(details, "disagreements")
        .iter()
        .filter_map(|d| {
            let fields: Vec<&str> = list(d, "copies")
                .iter()
                .filter_map(|c| str_of(c, "field"))
                .collect();
            Some(format!(
                "The copies of the {} disagree: {}. They are meant to be identical, and this \
                 report does not pick which one counts.",
                str_of(d, "statement")?,
                fields.join("; ")
            ))
        })
        .collect();
    Table {
        title: "Rights statements".to_string(),
        source: "rights_statements".to_string(),
        columns: vec!["Field".to_string(), "Kind".to_string(), "Value".to_string()],
        rows: field_rows(details, "statements"),
        notes,
    }
}

fn identifiers(details: &Value) -> Table {
    Table {
        title: "Identifiers".to_string(),
        source: "embedded_identifiers".to_string(),
        columns: vec!["Field".to_string(), "Kind".to_string(), "Value".to_string()],
        rows: field_rows(details, "identifiers"),
        notes: Vec::new(),
    }
}

/// Turns one source's `details` into its table.
type Reader = fn(&Value) -> Table;

/// The readers, in the order the tables appear.
const READERS: [(&str, Reader); 5] = [
    ("marking_metadata", source_type),
    ("data_mining_reservation", data_mining),
    ("vendor_ai_flag", vendor_flag),
    ("rights_statements", rights),
    ("embedded_identifiers", identifiers),
];

/// `3 rights statements`, `1 identifier`: a table's row count with its own noun.
fn counted(table: &Table) -> String {
    let n = table.rows.len();
    let (one, many) = match table.source.as_str() {
        "marking_metadata" => ("digital source type", "digital source types"),
        "data_mining_reservation" => ("data-mining term", "data-mining terms"),
        "vendor_ai_flag" => ("vendor AI field", "vendor AI fields"),
        "rights_statements" => ("rights statement", "rights statements"),
        _ => ("identifier", "identifiers"),
    };
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The section for one inspection's evidence. `None` when none of the five sources
/// produced details: they did not run, or do not handle this kind of file, and an
/// empty section would read as a finding.
pub fn declared(evidence: &[Evidence]) -> Option<Declared> {
    let mut ran = false;
    let mut tables = Vec::new();
    for (name, read) in READERS {
        let Some(details) = evidence
            .iter()
            .find(|e| e.source.name == name)
            .map(|e| &e.details)
            .filter(|d| d.is_object())
        else {
            continue;
        };
        ran = true;
        let table = read(details);
        if !table.rows.is_empty() || !table.notes.is_empty() {
            tables.push(table);
        }
    }
    if !ran {
        return None;
    }
    let mut parts: Vec<String> = tables
        .iter()
        .filter(|t| !t.rows.is_empty())
        .map(counted)
        .collect();
    let any_masked = tables
        .iter()
        .flat_map(|t| &t.rows)
        .flatten()
        .any(|cell| matches!(cell.as_slice(), [Span::Text(t)] if t.starts_with("masked, ")))
        || tables
            .iter()
            .flat_map(|t| &t.notes)
            .any(|note| note.starts_with(CONSTRAINTS_MASKED));
    if any_masked {
        parts.push("free-text values masked".to_string());
    }
    Some(Declared {
        heading: HEADING.to_string(),
        summary: if parts.is_empty() {
            NOTHING.to_string()
        } else {
            parts.join(", ")
        },
        caveat: CAVEAT.to_string(),
        tables,
    })
}

/// The section as a collapsed part of the page.
pub(crate) fn declared_html(d: &Declared) -> String {
    use std::fmt::Write as _;
    let mut body = String::new();
    for t in &d.tables {
        // Writing to a String cannot fail.
        let _ = write!(
            body,
            "<h3>{}</h3><table class=\"declared\"><tr>",
            html_escape(&t.title)
        );
        for c in &t.columns {
            let _ = write!(body, "<th>{}</th>", html_escape(c));
        }
        body.push_str("</tr>");
        for row in &t.rows {
            body.push_str("<tr>");
            for cell in row {
                let _ = write!(body, "<td>{}</td>", crate::rich_html(cell));
            }
            body.push_str("</tr>");
        }
        body.push_str("</table>");
        for n in &t.notes {
            let _ = write!(body, "<p>{}</p>", html_escape(n));
        }
    }
    format!(
        "<details><summary>{}: {}.</summary><p>{}</p>{body}</details>",
        html_escape(&d.heading),
        html_escape(&d.summary),
        html_escape(&d.caveat),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use halftone_core::{Layer, SourceId, Status};
    use serde_json::json;

    fn evidence(name: &str, details: Value) -> Evidence {
        Evidence {
            layer: Layer::Container,
            source: SourceId {
                name: name.into(),
                version: "0.1.0".into(),
            },
            status: Status::Present,
            statistic: None,
            calibration: None,
            rationale: String::new(),
            details,
            duration_ms: 0,
        }
    }

    /// Details as the sources write them (copied from real output on the synthetic
    /// stratum), masked.
    fn masked() -> Vec<Evidence> {
        vec![
            evidence(
                "marking_metadata",
                json!({ "values": [{ "code": "trainedAlgorithmicMedia", "form": "element",
                    "namespace_bound": true, "prefix": "Iptc4xmpExt",
                    "raw": "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia" }] }),
            ),
            evidence(
                "data_mining_reservation",
                json!({ "other_constraints": { "field": "plus:OtherConstraints",
                        "length": 28, "masked": true },
                    "values": [{ "code": "DMI-PROHIBITED-SEECONSTRAINT", "namespace_bound": false,
                    "raw": "http://ns.useplus.org/ldf/vocab/DMI-PROHIBITED-SEECONSTRAINT" }] }),
            ),
            evidence(
                "vendor_ai_flag",
                json!({ "flags": [
                    { "field": "Credit", "prefix": "photoshop", "says": true,
                      "states": "edited", "value": "Edited with Google AI" },
                    { "field": "ContainsAiGeneratedContent",
                      "prefix": "ContainsAiGeneratedContent", "says": false,
                      "states": "generated", "value": "No" } ] }),
            ),
            evidence(
                "rights_statements",
                json!({ "values_masked": true,
                    "statements": [
                        { "field": "exif:Copyright", "kind": "copyright_notice", "length": 15, "masked": true },
                        { "field": "dc:rights", "kind": "copyright_notice", "length": 1, "masked": true } ],
                    "disagreements": [{ "statement": "copyright notice", "copies": [
                        { "field": "exif:Copyright", "length": 15, "masked": true },
                        { "field": "dc:rights", "length": 1, "masked": true } ] }] }),
            ),
            evidence(
                "embedded_identifiers",
                json!({ "values_masked": true, "identifiers": [
                    { "field": "exif:Artist", "kind": "person", "length": 11, "masked": true } ] }),
            ),
        ]
    }

    #[test]
    fn each_source_gets_its_table_in_order_with_its_rows() {
        let d = declared(&masked()).unwrap();
        let titles: Vec<&str> = d.tables.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Digital source type (IPTC)",
                "Data-mining reservation (PLUS)",
                "Vendor AI field",
                "Rights statements",
                "Identifiers"
            ]
        );
        assert_eq!(
            d.summary,
            "1 digital source type, 1 data-mining term, 2 vendor AI fields, \
             2 rights statements, 1 identifier, free-text values masked"
        );
        assert_eq!(d.tables[0].rows[0][0], code("trainedAlgorithmicMedia"));
        // A term whose prefix is not bound to the standard's namespace says so.
        assert_eq!(d.tables[1].rows[0][1].len(), 2);
        // The Other Constraints text is free text: masked like a rights value.
        assert_eq!(
            d.tables[1].notes,
            ["Other constraints: masked, 28 characters"]
        );
        // A vendor field by prefix and name, once when the two are the same word.
        assert_eq!(d.tables[2].rows[0][0], code("photoshop:Credit"));
        assert_eq!(
            d.tables[2].rows[0][2],
            text("the content was edited with AI")
        );
        assert_eq!(d.tables[2].rows[1][0], code("ContainsAiGeneratedContent"));
        assert_eq!(
            d.tables[2].rows[1][2],
            text("the file does not contain AI-generated content")
        );
        assert_eq!(d.tables[3].rows[0][1], text("copyright notice"));
        assert!(d.tables[3].notes[0].starts_with(
            "The copies of the copyright notice disagree: exif:Copyright; dc:rights."
        ));
    }

    #[test]
    fn a_masked_value_shows_its_length_and_a_revealed_one_its_text() {
        let d = declared(&masked()).unwrap();
        assert_eq!(d.tables[3].rows[0][2], text("masked, 15 characters"));
        assert_eq!(d.tables[3].rows[1][2], text("masked, 1 character"));
        assert_eq!(d.tables[4].rows[0][2], text("masked, 11 characters"));

        let revealed = vec![evidence(
            "embedded_identifiers",
            json!({ "values_masked": false, "identifiers": [
                { "field": "exif:Artist", "kind": "person", "value": "Ada <b>Example</b>" } ] }),
        )];
        let d = declared(&revealed).unwrap();
        assert_eq!(d.tables[0].rows[0][2], text("Ada <b>Example</b>"));
        assert_eq!(d.summary, "1 identifier");
        // On the page the value is text, whatever it holds.
        let h = declared_html(&d);
        assert!(h.contains("<td>Ada &lt;b&gt;Example&lt;/b&gt;</td>"), "{h}");
        assert!(h.contains("<td><code>exif:Artist</code></td><td>person</td>"));
    }

    #[test]
    fn sources_that_found_nothing_give_one_line_and_sources_that_did_not_run_give_none() {
        let empty = vec![
            evidence(
                "rights_statements",
                json!({ "statements": [], "disagreements": [] }),
            ),
            evidence("embedded_identifiers", json!({ "identifiers": [] })),
        ];
        let d = declared(&empty).unwrap();
        assert!(d.tables.is_empty());
        assert_eq!(d.summary, NOTHING);
        let h = declared_html(&d);
        assert!(
            h.starts_with(
                "<details><summary>What the file says about itself: nothing in the fields \
                 these sources read.</summary><p>These fields are unsigned."
            ),
            "{h}"
        );
        // Not applicable (no details) and other sources only: no section at all.
        let none = vec![
            evidence("rights_statements", Value::Null),
            evidence("jpeg_quant", json!({ "quality": 85 })),
        ];
        assert_eq!(declared(&none), None);
        assert_eq!(declared(&[]), None);
    }

    #[test]
    fn details_of_another_shape_drop_rows_and_never_the_section() {
        let odd = vec![
            evidence("rights_statements", json!({ "statements": "not a list" })),
            evidence(
                "embedded_identifiers",
                json!({ "identifiers": [
                    { "kind": "person", "length": 4 },
                    { "field": "exif:Artist", "kind": "person" },
                    { "field": "dc:creator", "length": 3 },
                    42 ] }),
            ),
            evidence("vendor_ai_flag", json!({ "flags": [{ "value": "Yes" }] })),
            evidence("marking_metadata", json!({ "values": [{ "raw": "x" }] })),
        ];
        let d = declared(&odd).unwrap();
        // Only the identifier with a field and a length is a row; its kind is unknown.
        assert_eq!(d.tables.len(), 1);
        assert_eq!(
            d.tables[0].rows,
            [vec![
                code("dc:creator"),
                text(""),
                text("masked, 3 characters")
            ]]
        );
    }

    #[test]
    fn the_page_section_is_collapsed_and_opens_with_the_caveat() {
        let h = declared_html(&declared(&masked()).unwrap());
        assert!(h.starts_with("<details><summary>What the file says about itself: 1 digital"));
        assert!(!h.contains("<details open"));
        assert!(h.contains(&format!(
            "</summary><p>{CAVEAT}</p><h3>Digital source type (IPTC)</h3>"
        )));
        assert!(h.contains(
            "<table class=\"declared\"><tr><th>Field</th><th>Kind</th><th>Value</th></tr>\
             <tr><td><code>exif:Artist</code></td><td>person</td><td>masked, 11 characters</td></tr></table>"
        ), "{h}");
        assert!(h.contains("<p>Other constraints: masked, 28 characters</p>"));
        assert!(!h.contains("Research use only"));
    }

    #[test]
    fn the_other_constraints_text_is_quoted_only_when_the_source_revealed_it() {
        let note = |entry: Value| {
            let d = declared(&[evidence(
                "data_mining_reservation",
                json!({ "other_constraints": entry, "values": [{
                    "code": "DMI-PROHIBITED-SEECONSTRAINT", "namespace_bound": true,
                    "raw": "http://ns.useplus.org/ldf/vocab/DMI-PROHIBITED-SEECONSTRAINT" }] }),
            )])
            .unwrap();
            (d.tables[0].notes.clone(), d.summary)
        };
        let field = "plus:OtherConstraints";
        assert_eq!(
            note(json!({ "field": field, "value": "Ask <b>first</b>" })),
            (
                vec!["Other constraints, as written: Ask <b>first</b>".to_string()],
                "1 data-mining term".to_string()
            )
        );
        // Masked: its length, and the summary says that something is masked.
        assert_eq!(
            note(json!({ "field": field, "masked": true, "length": 1 })),
            (
                vec!["Other constraints: masked, 1 character".to_string()],
                "1 data-mining term, free-text values masked".to_string()
            )
        );
        // A document from before the source masked it holds the text as a string.
        assert_eq!(
            note(json!("Ask first")).0,
            ["Other constraints, as written: Ask first"]
        );
        // No text in the file, or an entry of another shape: no note.
        assert!(note(Value::Null).0.is_empty());
        assert!(note(json!({ "field": field })).0.is_empty());
        assert!(note(json!(7)).0.is_empty());
    }
}
