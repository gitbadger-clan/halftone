//! Per-file report. Renders an `Inspection` to HTML and, with the `pdf` feature, to
//! PDF, with the tool/pack versions, hashes, and the fixed notice that no layer's
//! output is sufficient evidence on its own. The report is what legal/newsroom buyers
//! file.
//!
//! Top to bottom: the file, notes on anything that changes how the evidence reads,
//! the evidence, the C2PA conformance measurements, the conditions of the check, the
//! notice.
//!
//! What a report says is built once, as [`model::Report`]; [`to_html`] and
//! `pdf::to_pdf` (with the `pdf` feature) are two drawings of that value, so the page
//! and the PDF cannot say different things.
//!
//! The page is one self-contained file. Its stylesheet (`report.css`), the mark and
//! the verdict glyphs are inlined, it carries no script, and its
//! Content-Security-Policy forbids loading anything, so it reads the same offline and
//! from an archive. The two measurement sections are collapsed on the page.
//!
//! The PDF is typeset in-process (no browser, no network), with its fonts built in.
//! The same inspection gives the same bytes, and the file is written as PDF/A-2b.

pub mod conditions;
pub mod model;
#[cfg(feature = "pdf")]
pub mod pdf;

use halftone_core::{Evidence, Inspection, Status};
use model::{Check, Conformance, EvidenceRow, LayerGroup, LegendItem, Report, Rich, Rubric, Span};

/// Fixed notice included in every report.
pub const NOTICE: &str = "Each layer is reported separately with its own calibration. \
No single verdict here is evidence of authorship on its own, and this report must not \
be the sole basis for any disciplinary, employment, or admissions decision.";

/// Introduces the notes on what differs from a default check.
pub const NOTES_LEAD: &str = "This check differs from a default one:";

/// Closes the legend: what a status is, and what it never is.
pub const LEGEND_CLOSE: &str = "A status answers that source's own question. It is \
never a statement about where the file as a whole comes from.";

/// What the conformance section is called.
pub const CONFORMANCE_HEADING: &str = "C2PA conformance rubrics";

/// What kind of statement the conformance section is.
pub const CONFORMANCE_CAVEAT: &str = "A measurement of the manifest, not a verdict.";

/// The stylesheet inlined into every page.
pub const CSS: &str = include_str!("report.css");

const MARK: &str = include_str!("../assets/halftone-mark.svg");
const GLYPH_PRESENT: &str = include_str!("../assets/verdict/present.svg");
const GLYPH_ABSENT: &str = include_str!("../assets/verdict/absent.svg");
const GLYPH_INCONCLUSIVE: &str = include_str!("../assets/verdict/inconclusive.svg");
const GLYPH_NOT_APPLICABLE: &str = include_str!("../assets/verdict/not_applicable.svg");

/// What a status is called in a report.
pub fn status_word(s: Status) -> &'static str {
    match s {
        Status::Present => "Present",
        Status::Absent => "Absent",
        Status::Inconclusive => "Inconclusive",
        Status::NotApplicable => "Not applicable",
    }
}

/// A status's row class and glyph on the page.
fn status_parts(s: Status) -> (&'static str, &'static str) {
    match s {
        Status::Present => ("present", GLYPH_PRESENT),
        Status::Absent => ("absent", GLYPH_ABSENT),
        Status::Inconclusive => ("inconclusive", GLYPH_INCONCLUSIVE),
        Status::NotApplicable => ("not_applicable", GLYPH_NOT_APPLICABLE),
    }
}

/// A glyph for the eye only: the status word stands next to it, so a screen reader
/// is not told the verdict twice.
fn glyph(svg: &str) -> String {
    format!(
        "<span class=\"glyph\" aria-hidden=\"true\">{}</span>",
        svg.trim()
    )
}

/// The evidence as a group of rows per layer, in execution order.
pub fn evidence_groups(evidence: &[Evidence]) -> Vec<LayerGroup> {
    let mut groups: Vec<LayerGroup> = Vec::new();
    let mut layer = None;
    for e in evidence {
        if layer != Some(e.layer) {
            groups.push(LayerGroup {
                layer: format!("{:?}", e.layer),
                rows: Vec::new(),
            });
            layer = Some(e.layer);
        }
        if let Some(group) = groups.last_mut() {
            group.rows.push(EvidenceRow {
                status: e.status,
                word: status_word(e.status).to_string(),
                source: e.source.name.clone(),
                version: e.source.version.clone(),
                rationale: e.rationale.clone(),
            });
        }
    }
    groups
}

/// The evidence as one table: in each row the status (glyph and word), the source and
/// its rationale.
#[cfg(test)]
fn evidence_html(evidence: &[Evidence]) -> String {
    groups_html(&evidence_groups(evidence))
}

fn groups_html(groups: &[LayerGroup]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from(
        "<table class=\"evidence\"><thead><tr><th>Status</th><th>Source</th>\
<th>Rationale</th></tr></thead>",
    );
    for g in groups {
        // Writing to a String cannot fail.
        let _ = write!(
            out,
            "<tbody><tr class=\"layer\"><th colspan=\"3\" scope=\"rowgroup\">{}</th></tr>",
            html_escape(&g.layer)
        );
        for r in &g.rows {
            let (class, svg) = status_parts(r.status);
            let _ = write!(
                out,
                "<tr class=\"{class}\"><td class=\"status\">{}{}</td>\
<td class=\"source\"><code>{}</code><small>{}</small></td><td>{}</td></tr>",
                glyph(svg),
                html_escape(&r.word),
                html_escape(&r.source),
                html_escape(&r.version),
                html_escape(&r.rationale)
            );
        }
        out.push_str("</tbody>");
    }
    out.push_str("</table>");
    out
}

/// What the four statuses mean. A status answers the one question its source asks;
/// [`LEGEND_CLOSE`] keeps a reader from adding them up.
pub fn legend() -> Vec<LegendItem> {
    [
        (Status::Present, "the source found what it looks for."),
        (
            Status::Absent,
            "it did not; the rationale names what that leaves open.",
        ),
        (Status::Inconclusive, "it could not decide."),
        (
            Status::NotApplicable,
            "the source does not handle this kind of file, or could not run.",
        ),
    ]
    .into_iter()
    .map(|(status, text)| LegendItem {
        status,
        word: status_word(status).to_string(),
        text: text.to_string(),
    })
    .collect()
}

/// [`legend`] as the paragraph under the evidence.
fn legend_html() -> String {
    let items: String = legend()
        .iter()
        .map(|i| {
            format!(
                "{}<b>{}</b>: {} ",
                glyph(status_parts(i.status).1),
                html_escape(&i.word),
                html_escape(&i.text)
            )
        })
        .collect();
    format!("<p class=\"legend\">{items}{LEGEND_CLOSE}</p>")
}

/// `s` as the inside of a CSS string. Everything but plain letters, digits and a few
/// marks becomes a hex escape, so a file name can neither end the string nor the
/// `<style>` element it sits in.
fn css_string(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | ':' | ',') {
            out.push(c);
        } else {
            let _ = write!(out, "\\{:x} ", u32::from(c));
        }
    }
    out
}

/// The line printed at the foot of every page: which file the page belongs to. Loose
/// pages of two reports can then be told apart.
fn page_footer_css(footer: &str) -> String {
    format!(
        "@page{{@bottom-left{{content:\"{}\";\
font:8pt Charter,\"Bitstream Charter\",Cambria,Georgia,serif;color:#1B3A5C}}}}",
        css_string(footer),
    )
}

/// Render to one self-contained HTML document.
pub fn to_html(insp: &Inspection) -> String {
    report_html(&Report::new(insp))
}

/// A report as one self-contained HTML document.
pub fn report_html(r: &Report) -> String {
    let notes = conditions::notes_list_html(&r.notes);
    let notes = if notes.is_empty() {
        notes
    } else {
        format!(
            "<aside class=\"notes\"><p>{}</p>{notes}</aside>",
            html_escape(&r.notes_lead)
        )
    };
    let mut file = String::new();
    for f in &r.file {
        use std::fmt::Write as _;
        let _ = write!(
            file,
            "<tr><th>{}</th><td>{}</td></tr>",
            html_escape(&f.label),
            rich_html(&f.value)
        );
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\
<title>{}</title><style>{CSS}</style><style>{}</style></head><body>\
<header><span class=\"mark\" aria-hidden=\"true\">{}</span><div><h1>Halftone report</h1>\
<p>{}</p></div></header><main><h2>File</h2><table class=\"file\">{file}</table>{notes}\
<h2>Evidence</h2>{}{}{}{}</main><footer><p>{}</p></footer></body></html>",
        html_escape(&r.title),
        page_footer_css(&r.page_footer),
        MARK.trim(),
        html_escape(&r.checked),
        groups_html(&r.evidence),
        legend_html(),
        r.conformance
            .as_ref()
            .map(conformance_model_html)
            .unwrap_or_default(),
        conditions::conditions_model_html(&r.conditions),
        html_escape(&r.notice)
    )
}

/// The C2PA conformance measurements (`details.conformance` of the c2pa source): one
/// line with every rubric's counts, and per rubric its checks, those not met first.
/// `None` when there is nothing to show.
pub fn conformance(details: &serde_json::Value) -> Option<Conformance> {
    use std::fmt::Write as _;
    let Some(rubrics) = details.get("rubrics").and_then(|r| r.as_array()) else {
        return details
            .get("error")
            .and_then(|e| e.as_str())
            .map(|e| Conformance::NotMeasured {
                heading: CONFORMANCE_HEADING.to_string(),
                reason: e.to_string(),
            });
    };
    let text = |v: &serde_json::Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    let mut summary = Vec::new();
    let mut out = Vec::new();
    for r in rubrics {
        let name = format!("{} {}", text(&r["name"]), text(&r["version"]));
        let name = if r["name"].is_null() {
            text(&r["id"])
        } else {
            name
        };
        if let Some(e) = r["error"].as_str() {
            let line = format!("{name}: {e}");
            summary.push(line.clone());
            out.push(Rubric {
                heading: line,
                checks: Vec::new(),
            });
            continue;
        }
        let n = |k: &str| r["summary"][k].as_u64().unwrap_or(0);
        let total = n("met") + n("not_met") + n("warning") + n("error");
        let mut line = format!("{name}: {} of {total} met", n("met"));
        for (k, word) in [
            ("not_met", "not met"),
            ("warning", "warning"),
            ("error", "not evaluated"),
        ] {
            if n(k) > 0 {
                let _ = write!(line, ", {} {word}", n(k));
            }
        }
        summary.push(line.clone());
        let results = r["results"].as_array().cloned().unwrap_or_default();
        let mut checks = Vec::new();
        for want in [["not_met", "error"], ["warning", "warning"], ["met", "met"]] {
            for row in results
                .iter()
                .filter(|x| want.contains(&x["outcome"].as_str().unwrap_or("")))
            {
                let result = match row["outcome"].as_str() {
                    Some("not_met") => "not met",
                    Some("error") => "not evaluated",
                    Some(o) => o,
                    None => "?",
                };
                checks.push(Check {
                    name: text(&row["trait"]),
                    result: result.to_string(),
                    text: text(&row["text"]),
                });
            }
        }
        out.push(Rubric {
            heading: line,
            checks,
        });
    }
    Some(Conformance::Measured {
        heading: CONFORMANCE_HEADING.to_string(),
        summary: summary.join("; "),
        caveat: CONFORMANCE_CAVEAT.to_string(),
        about: details["about"].as_str().unwrap_or("").to_string(),
        rubrics: out,
    })
}

/// [`conformance`] as a collapsed section: one summary line with the counts, the
/// checks inside. Empty when there is nothing to show. A measurement of the manifest,
/// so it sits below the evidence table and says it is not a verdict.
pub fn conformance_html(details: &serde_json::Value) -> String {
    conformance(details)
        .as_ref()
        .map(conformance_model_html)
        .unwrap_or_default()
}

fn conformance_model_html(c: &Conformance) -> String {
    use std::fmt::Write as _;
    let (heading, summary, caveat, about, rubrics) = match c {
        Conformance::NotMeasured { heading, reason } => {
            return format!(
                "<p>{}: not measured ({})</p>",
                html_escape(heading),
                html_escape(reason)
            );
        }
        Conformance::Measured {
            heading,
            summary,
            caveat,
            about,
            rubrics,
        } => (heading, summary, caveat, about, rubrics),
    };
    let mut rows = String::new();
    for r in rubrics {
        // One heading row per rubric, with its own counts; the checks below it.
        let _ = write!(
            rows,
            "<tr><th colspan=\"3\">{}</th></tr>",
            html_escape(&r.heading)
        );
        for check in &r.checks {
            let _ = write!(
                rows,
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&check.name),
                html_escape(&check.result),
                html_escape(&check.text)
            );
        }
    }
    format!(
        "<details><summary>{}: {}. {}</summary><p>{}</p><table><tr><th>Check</th>\
<th>Result</th><th>Report text</th></tr>{rows}</table></details>",
        html_escape(heading),
        html_escape(summary),
        html_escape(caveat),
        html_escape(about)
    )
}

/// A cell's content for the page: words escaped, literal values in `<code>`.
pub(crate) fn rich_html(rich: &Rich) -> String {
    rich.iter()
        .map(|span| match span {
            Span::Text(t) => html_escape(t),
            Span::Code(c) => format!("<code>{}</code>", html_escape(c)),
            Span::Break => "<br>".to_string(),
        })
        .collect()
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use halftone_core::{AssetInfo, Layer, Modality, SourceId, ToolInfo};
    use serde_json::json;

    fn details() -> serde_json::Value {
        json!({
            "about": "Measurements of the manifest <not the file>.",
            "rubrics": [
                { "id": "integrity", "name": "C2PA Asset Integrity Rubric", "version": "1.0",
                  "summary": { "met": 1, "not_met": 0, "warning": 0, "error": 0 },
                  "results": [{ "trait": "valid_success", "outcome": "met", "text": "No validity errors" }] },
                { "id": "conformance-0.2-spec-2.4", "name": "C2PA Asset Conformance 0.2 Spec 2.4 Rubric", "version": "1.0",
                  "summary": { "met": 1, "not_met": 1, "warning": 1, "error": 0 },
                  "results": [
                    { "trait": "active_manifest_urn", "outcome": "met", "text": "ok" },
                    { "trait": "discouraged_legacy_action_parameters", "outcome": "warning", "text": "legacy" },
                    { "trait": "mandatory_all_actions_included", "outcome": "not_met", "text": "Missing allActionsIncluded & more" }
                  ] }
            ]
        })
    }

    fn evidence(layer: Layer, name: &str, status: Status, rationale: &str) -> Evidence {
        Evidence {
            layer,
            source: SourceId {
                name: name.into(),
                version: "0.1.0".into(),
            },
            status,
            statistic: None,
            calibration: None,
            rationale: rationale.into(),
            details: serde_json::Value::Null,
            duration_ms: 0,
        }
    }

    fn inspection(file: &str) -> Inspection {
        Inspection {
            schema_version: "1.5.0".into(),
            tool: ToolInfo {
                name: "halftone".into(),
                version: "0.1.0".into(),
                build: None,
            },
            asset: AssetInfo {
                path: Some(file.into()),
                modality: Modality::Image,
                mime: "image/png".into(),
                sha256: "5c4081bb367965ab".repeat(4),
                size_bytes: 10,
            },
            evidence: vec![
                evidence(Layer::Manifest, "c2pa", Status::Absent, "No C2PA manifest."),
                evidence(Layer::Container, "png_writer", Status::Present, "a <b> & c"),
                evidence(
                    Layer::Container,
                    "jpeg_double",
                    Status::Inconclusive,
                    "too small",
                ),
                evidence(
                    Layer::Container,
                    "webp_writer",
                    Status::NotApplicable,
                    "not WebP",
                ),
                evidence(
                    Layer::Blind,
                    "feature_probe",
                    Status::NotApplicable,
                    "no pack",
                ),
            ],
            run: None,
            created_at: "2026-11-18T10:45:48Z".into(),
        }
    }

    #[test]
    fn the_report_is_one_file_that_loads_nothing_and_runs_nothing() {
        let h = to_html(&inspection("/cases/photo.png"));
        assert!(
            h.starts_with("<!doctype html><html lang=\"en\">"),
            "{}",
            &h[..80]
        );
        assert!(h.contains(
            "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">"
        ));
        // No script, and nothing that names another resource. The one `http` left is
        // the SVG namespace of the inlined mark and glyphs, which is a name, not a link.
        for banned in [
            "<script", "<link", "<img", "<iframe", " src=", " href=", "@import", "url(h",
            "url(\"h", "url('h",
        ] {
            assert!(!h.contains(banned), "{banned}");
        }
        assert_eq!(
            h.matches("http").count(),
            h.matches("http://www.w3.org/2000/svg").count() + 1
        );
        // The stylesheet is in the file, with the print rules that make a PDF whole.
        assert!(h.contains("<style>") && h.contains("--ink: #1B3A5C"));
        assert!(CSS.contains("@media print") && CSS.contains("details::details-content"));
        assert!(CSS.contains("size: A4") && CSS.contains("counter(pages)"));
        assert!(h.contains("<title>Halftone report: photo.png</title>"));
        assert!(h.contains(&format!("<footer><p>{NOTICE}</p></footer>")));
    }

    #[test]
    fn evidence_is_grouped_by_layer_with_the_status_in_shape_and_word() {
        let h = evidence_html(&inspection("a.png").evidence);
        let pos = |s: &str| h.find(s).unwrap_or_else(|| panic!("{s} not in {h}"));
        // One group per layer, in execution order; a layer with no source has none.
        assert_eq!(h.matches("<tr class=\"layer\">").count(), 3);
        assert!(pos(">Manifest</th>") < pos(">Container</th>"));
        assert!(pos(">Container</th>") < pos(">Blind</th>"));
        assert!(!h.contains(">Mark</th>"));
        assert_eq!(h.matches("<tbody>").count(), h.matches("</tbody>").count());
        // The status as a word, with a glyph the eye reads and a screen reader skips.
        assert!(h.contains("<tr class=\"present\"><td class=\"status\"><span class=\"glyph\" aria-hidden=\"true\"><svg"), "{h}");
        assert!(h.contains("</svg></span>Present</td>"));
        assert!(h.contains("</svg></span>Inconclusive</td>"));
        assert!(h.contains("</svg></span>Absent</td>"));
        assert_eq!(h.matches("</svg></span>Not applicable</td>").count(), 2);
        assert!(!h.contains("NotApplicable"));
        assert!(h.contains("<td class=\"source\"><code>png_writer</code><small>0.1.0</small></td><td>a &lt;b&gt; &amp; c</td>"), "{h}");
    }

    #[test]
    fn the_legend_says_what_a_status_is_and_is_not() {
        let l = legend_html();
        for word in [
            "<b>Present</b>",
            "<b>Absent</b>",
            "<b>Inconclusive</b>",
            "<b>Not applicable</b>",
        ] {
            assert!(l.contains(word), "{word}");
        }
        assert!(l.contains("never a statement about where the file as a whole comes from"));
        assert!(to_html(&inspection("a.png")).contains(&l));
    }

    #[test]
    fn a_file_name_cannot_break_out_of_the_title_or_the_page_footer() {
        let name = "x</style><script>alert(1)</script>\"}.png";
        let h = to_html(&inspection(name));
        assert!(!h.contains("<script"), "{h}");
        assert_eq!(
            h.matches("</style>").count(),
            2,
            "the two style elements end once each"
        );
        assert_eq!(css_string("a b-c_d.e:1"), "a b-c_d.e:1");
        assert_eq!(css_string("\"}<\\\n"), "\\22 \\7d \\3c \\5c \\a ");
        // The footer names the file and the start of its hash, clipped to fit a line.
        let footer = Report::new(&inspection("photo.png")).page_footer;
        assert_eq!(
            footer,
            "Halftone report: photo.png, SHA-256 5c4081bb367965ab\u{2026}"
        );
        let css = page_footer_css(&footer);
        assert!(
            css.contains("content:\"Halftone report: photo.png, SHA-256 5c4081bb367965ab\\2026 \""),
            "{css}"
        );
        assert_eq!(model::clipped("abcdef", 4), "abc\u{2026}");
        assert_eq!(model::clipped("abcd", 4), "abcd");
    }

    #[test]
    fn notes_get_their_heading_only_when_there_are_any() {
        // No build record and no run record: two notes.
        let h = to_html(&inspection("a.png"));
        assert!(h.contains("<aside class=\"notes\"><p>This check differs from a default one:</p><ul class=\"notes\"><li>"), "{h}");
        let mut plain = inspection("a.png");
        plain.tool.build = Some(halftone_core::BuildInfo {
            commit: Some("a".repeat(40)),
            modified: Some(false),
            features: vec!["c2pa".into()],
        });
        plain.run = Some(halftone_core::RunInfo {
            options: halftone_core::RunOptions {
                layers: vec![Layer::Manifest, Layer::Container, Layer::Mark, Layer::Blind],
                trust_list: "auto".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        assert!(!to_html(&plain).contains("<aside"));
    }

    #[test]
    fn the_section_is_collapsed_and_says_it_is_not_a_verdict() {
        let h = conformance_html(&details());
        assert!(h.starts_with("<details><summary>"), "{h}");
        assert!(!h.contains("<details open"));
        assert!(h.contains("C2PA Asset Integrity Rubric 1.0: 1 of 1 met; C2PA Asset Conformance 0.2 Spec 2.4 Rubric 1.0: 1 of 3 met, 1 not met, 1 warning"), "{h}");
        assert!(h.contains("not a verdict"));
    }

    #[test]
    fn checks_not_met_come_first_and_text_is_escaped() {
        let h = conformance_html(&details());
        let pos = |s: &str| h.find(s).unwrap();
        assert!(
            pos("mandatory_all_actions_included") < pos("discouraged_legacy_action_parameters")
        );
        assert!(pos("discouraged_legacy_action_parameters") < pos("active_manifest_urn"));
        assert!(h.contains("Missing allActionsIncluded &amp; more"));
        assert!(h.contains("&lt;not the file&gt;"));
    }

    #[test]
    fn each_rubric_is_a_heading_row_not_a_column() {
        let h = conformance_html(&details());
        let table = &h[h.find("<table>").unwrap()..];
        assert_eq!(
            table
                .matches("C2PA Asset Conformance 0.2 Spec 2.4 Rubric 1.0")
                .count(),
            1,
            "{table}"
        );
        assert!(
            table.contains(
                "<tr><th colspan=\"3\">C2PA Asset Integrity Rubric 1.0: 1 of 1 met</th></tr>"
            ),
            "{table}"
        );
        assert!(!table.contains("<th>Rubric</th>"));
        assert!(
            table.contains("<tr><td>mandatory_all_actions_included</td><td>not met</td>"),
            "{table}"
        );
    }

    #[test]
    fn a_rubric_that_could_not_be_evaluated_gets_its_heading_row() {
        let d = json!({ "about": "", "rubrics": [{ "id": "integrity", "name": "R", "version": "1.0", "error": "could not be evaluated" }] });
        assert!(
            conformance_html(&d)
                .contains("<tr><th colspan=\"3\">R 1.0: could not be evaluated</th></tr>")
        );
    }

    #[test]
    fn nothing_to_show_is_an_empty_string_and_an_export_error_is_one_line() {
        assert_eq!(conformance_html(&json!({})), "");
        let e = conformance_html(&json!({ "error": "no export" }));
        assert_eq!(
            e,
            "<p>C2PA conformance rubrics: not measured (no export)</p>"
        );
    }
}
