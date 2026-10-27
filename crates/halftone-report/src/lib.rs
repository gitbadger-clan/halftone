//! Per-file report. Renders an `Inspection` to HTML (and later PDF) with the
//! tool/pack versions, hashes, and the fixed notice that no layer's output is
//! sufficient evidence on its own. The report is what legal/newsroom buyers file.

use halftone_core::Inspection;

/// Fixed notice included in every report.
pub const NOTICE: &str = "Each layer is reported separately with its own calibration. \
No single verdict here is evidence of authorship on its own, and this report must not \
be the sole basis for any disciplinary, employment, or admissions decision.";

/// Render to a minimal HTML string.
pub fn to_html(insp: &Inspection) -> String {
    use std::fmt::Write as _;
    let mut rows = String::new();
    for e in &insp.evidence {
        // Writing to a String cannot fail.
        let _ = write!(
            rows,
            "<tr><td>{:?}</td><td>{} {}</td><td>{:?}</td><td>{}</td></tr>",
            e.layer,
            e.source.name,
            e.source.version,
            e.status,
            html_escape(&e.rationale)
        );
    }
    format!(
        "<!doctype html><meta charset=utf-8><title>Halftone report</title>\
<h1>Halftone report</h1><p>{} {} · schema {} · {}</p><p>sha256 {}</p>\
<table><tr><th>Layer</th><th>Source</th><th>Status</th><th>Rationale</th></tr>{rows}</table>\
{}<p><small>{}</small></p>",
        insp.tool.name,
        insp.tool.version,
        insp.schema_version,
        insp.created_at,
        insp.asset.sha256,
        insp.evidence
            .iter()
            .find(|e| e.source.name == "c2pa")
            .and_then(|e| e.details.get("conformance"))
            .map(conformance_html)
            .unwrap_or_default(),
        NOTICE
    )
}

/// The C2PA conformance measurements (`details.conformance` of the c2pa source) as
/// a collapsed section: one summary line with the counts, the checks inside, those
/// not met first. Empty when there is nothing to show. A measurement of the
/// manifest, so it sits below the evidence table and says it is not a verdict.
pub fn conformance_html(details: &serde_json::Value) -> String {
    use std::fmt::Write as _;
    let Some(rubrics) = details.get("rubrics").and_then(|r| r.as_array()) else {
        return match details.get("error").and_then(|e| e.as_str()) {
            Some(e) => format!(
                "<p>C2PA conformance rubrics: not measured ({})</p>",
                html_escape(e)
            ),
            None => String::new(),
        };
    };
    let text = |v: &serde_json::Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    let mut summary = Vec::new();
    let mut rows = String::new();
    for r in rubrics {
        let name = format!("{} {}", text(&r["name"]), text(&r["version"]));
        let name = if r["name"].is_null() {
            text(&r["id"])
        } else {
            name
        };
        if let Some(e) = r["error"].as_str() {
            let line = format!("{name}: {e}");
            let _ = write!(
                rows,
                "<tr><th colspan=\"3\">{}</th></tr>",
                html_escape(&line)
            );
            summary.push(line);
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
        // One heading row per rubric, with its own counts; the checks below it.
        let _ = write!(
            rows,
            "<tr><th colspan=\"3\">{}</th></tr>",
            html_escape(&line)
        );
        summary.push(line);
        let results = r["results"].as_array().cloned().unwrap_or_default();
        for want in [["not_met", "error"], ["warning", "warning"], ["met", "met"]] {
            for row in results
                .iter()
                .filter(|x| want.contains(&x["outcome"].as_str().unwrap_or("")))
            {
                let outcome = match row["outcome"].as_str() {
                    Some("not_met") => "not met",
                    Some("error") => "not evaluated",
                    Some(o) => o,
                    None => "?",
                };
                let _ = write!(
                    rows,
                    "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                    html_escape(&text(&row["trait"])),
                    outcome,
                    html_escape(&text(&row["text"]))
                );
            }
        }
    }
    format!(
        "<details><summary>C2PA conformance rubrics: {}. A measurement of the manifest, \
not a verdict.</summary><p>{}</p><table><tr><th>Check</th><th>Result</th>\
<th>Report text</th></tr>{rows}</table></details>",
        html_escape(&summary.join("; ")),
        html_escape(details["about"].as_str().unwrap_or(""))
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
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
