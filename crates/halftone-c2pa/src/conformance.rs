//! The C2PA's integrity and conformance rubrics over this file's manifest store.
//!
//! Halftone runs the published rubric files unmodified (`halftone-rubric`, held to
//! the C2PA's own evaluator by golden tests) over the crJSON that c2pa exports:
//! the Asset Integrity Rubric, which sorts the active manifest's validation codes
//! into well-formedness, validity and trust, and the Conformance 0.2 Spec 2.4
//! rubric, the Conformance Program's checks on how the manifest is written.
//!
//! These are measurements of the manifest, never of the file, and never part of
//! the verdict. Two limits are stated wherever they are shown: the integrity
//! rubric reads the validation codes in c2pa's export, which come from before
//! Halftone's own handling (the multi-asset fallback can find a file valid that
//! the export calls invalid); and a manifest can meet every check and still
//! describe content that was never what it claims.
//!
//! `details.conformance` is always written; [`render`] is the block `ht inspect
//! --conformance` prints. Pure: compiles and is tested without the `c2pa` feature.

use halftone_rubric::conformance::Outcome;
use halftone_rubric::{JValue, Rubric};
use serde_json::{Value, json};
use std::sync::OnceLock;

/// What every rendering of these results says about them.
pub const ABOUT: &str = "Measurements of the manifest against the C2PA's own rubrics: \
they describe how the manifest is written and what its validation codes say, not the \
file, and never change the verdict. The integrity rubric reads the validation codes in \
c2pa's crJSON export, from before Halftone's own handling (such as the multi-asset \
fallback).";

struct Loaded {
    id: &'static str,
    sha256: &'static str,
    spec_version: Option<&'static str>,
    rubric: Result<Rubric, String>,
}

/// The vendored rubrics, parsed once per process.
fn rubrics() -> &'static [Loaded] {
    static R: OnceLock<Vec<Loaded>> = OnceLock::new();
    R.get_or_init(|| {
        vec![
            Loaded {
                id: "integrity",
                sha256: halftone_rubric::INTEGRITY_SHA256,
                spec_version: None,
                rubric: halftone_rubric::integrity().map_err(|e| e.to_string()),
            },
            Loaded {
                id: "conformance-0.2-spec-2.4",
                sha256: halftone_rubric::CONFORMANCE_0_2_SPEC_2_4_SHA256,
                spec_version: Some("2.4"),
                rubric: halftone_rubric::conformance_0_2_spec_2_4().map_err(|e| e.to_string()),
            },
        ]
    })
}

/// `details.conformance` for one crJSON document.
pub fn details(crjson: &Value) -> Value {
    let doc = JValue::from_serde(crjson);
    let results: Vec<Value> = rubrics().iter().map(|l| one(l, &doc)).collect();
    json!({
        "about": ABOUT,
        "crjson_generator": crjson.get("jsonGenerator").cloned().unwrap_or(Value::Null),
        "rubrics": results,
    })
}

fn one(l: &Loaded, doc: &JValue) -> Value {
    let mut entry = json!({ "id": l.id, "sha256": l.sha256, "spec_version": l.spec_version });
    let set = |entry: &mut Value, k: &str, v: Value| {
        if let Some(m) = entry.as_object_mut() {
            m.insert(k.to_string(), v);
        }
    };
    let r = match &l.rubric {
        Ok(r) => r,
        Err(e) => {
            set(
                &mut entry,
                "error",
                json!(format!("the rubric did not load: {e}")),
            );
            return entry;
        }
    };
    set(&mut entry, "name", r.name.to_serde());
    set(&mut entry, "version", r.version.to_serde());
    let report = match r.evaluate_conformance(doc) {
        Ok(rep) => rep,
        Err(e) => {
            set(
                &mut entry,
                "error",
                json!(format!("the rubric could not be evaluated: {e}")),
            );
            return entry;
        }
    };
    let (mut met, mut not_met, mut warning, mut errors) = (0, 0, 0, 0);
    let rows: Vec<Value> = report
        .rows
        .iter()
        .map(|row| {
            let outcome = match (row.outcome, row.error) {
                (_, true) => {
                    errors += 1;
                    "error"
                }
                (Outcome::Met, _) => {
                    met += 1;
                    "met"
                }
                (Outcome::NotMet, _) => {
                    not_met += 1;
                    "not_met"
                }
                (Outcome::Warning, _) => {
                    warning += 1;
                    "warning"
                }
            };
            let mut v = json!({
                "trait": row.trait_name,
                "category": row.category,
                "outcome": outcome,
                "text": row.report_text.to_serde(),
            });
            if let Some(m) = v.as_object_mut() {
                if let Some(label) = &row.manifest_label {
                    m.insert("manifest".into(), label.to_serde());
                }
                if let Some(by) = &row.asserted_by {
                    m.insert("signer".into(), by.to_serde());
                }
            }
            v
        })
        .collect();
    set(
        &mut entry,
        "summary",
        json!({ "met": met, "not_met": not_met, "warning": warning, "error": errors }),
    );
    set(&mut entry, "results", Value::Array(rows));
    set(&mut entry, "output", report.output.to_serde());
    entry
}

/// One line per rubric: "C2PA Asset Integrity Rubric 1.0: 6 of 6 met".
pub fn summary_lines(details: &Value) -> Vec<String> {
    details["rubrics"]
        .as_array()
        .map(|rs| {
            rs.iter()
                .map(|r| {
                    let name = format!(
                        "{} {}",
                        r["name"]
                            .as_str()
                            .unwrap_or(r["id"].as_str().unwrap_or("rubric")),
                        r["version"].as_str().unwrap_or("")
                    );
                    let name = name.trim_end();
                    if let Some(e) = r["error"].as_str() {
                        return format!("{name}: {e}");
                    }
                    let n = |k: &str| r["summary"][k].as_u64().unwrap_or(0);
                    let total = n("met") + n("not_met") + n("warning") + n("error");
                    let mut s = format!("{name}: {} of {total} met", n("met"));
                    for (k, word) in [
                        ("not_met", "not met"),
                        ("warning", "warning"),
                        ("error", "could not be evaluated"),
                    ] {
                        let c = n(k);
                        if c > 0 {
                            let plural = if c > 1 && k == "warning" { "s" } else { "" };
                            s.push_str(&format!(", {c} {word}{plural}"));
                        }
                    }
                    s
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The block `ht inspect --conformance` prints under a file: one summary line per
/// rubric, then only what is not met, warned about, or could not be evaluated.
pub fn render(details: &Value) -> String {
    let mut out = String::from(
        "  conformance (C2PA rubrics; they describe the manifest, not the file, and are not a verdict)\n",
    );
    let lines = summary_lines(details);
    let rubrics = details["rubrics"].as_array().cloned().unwrap_or_default();
    for (line, r) in lines.iter().zip(&rubrics) {
        out.push_str(&format!("    {line}\n"));
        for row in r["results"].as_array().into_iter().flatten() {
            let word = match row["outcome"].as_str() {
                Some("not_met") => "not met",
                Some("warning") => "warning",
                Some("error") => "error  ",
                _ => continue,
            };
            let text = row["text"]
                .as_str()
                .map_or_else(|| row["text"].to_string(), str::to_string);
            let first = text.lines().next().unwrap_or("");
            out.push_str(&format!(
                "      {word}  {}: {first}\n",
                row["trait"].as_str().unwrap_or("?")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sample(name: &str) -> Value {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/rubric-conformance/in")
            .join(format!("{name}.json"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn both_rubrics_run_and_are_identified() {
        let d = details(&sample("adobe-capture-then-ai-edits"));
        let rs = d["rubrics"].as_array().unwrap();
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0]["id"], "integrity");
        assert_eq!(rs[0]["sha256"], halftone_rubric::INTEGRITY_SHA256);
        assert_eq!(rs[1]["spec_version"], "2.4");
        assert_eq!(
            rs[0]["summary"],
            json!({ "met": 6, "not_met": 0, "warning": 0, "error": 0 })
        );
        assert_eq!(
            rs[1]["summary"],
            json!({ "met": 29, "not_met": 2, "warning": 0, "error": 0 })
        );
        assert!(
            d["about"]
                .as_str()
                .unwrap()
                .contains("never change the verdict")
        );
    }

    #[test]
    fn summaries_and_the_block_list_only_what_needs_attention() {
        let d = details(&sample("v-legacy-parameters"));
        assert_eq!(
            summary_lines(&d),
            [
                "C2PA Asset Integrity Rubric 1.0: 6 of 6 met",
                "C2PA Asset Conformance 0.2 Spec 2.4 Rubric 1.0: 28 of 31 met, 2 not met, 1 warning",
            ]
        );
        let block = render(&d);
        assert!(
            block.contains(
                "not met  mandatory_all_actions_included: Missing mandatory allActionsIncluded"
            ),
            "{block}"
        );
        assert!(
            block.contains("warning  discouraged_legacy_action_parameters:"),
            "{block}"
        );
        assert!(
            !block.contains("well_formed_success"),
            "met checks are counted, not listed:\n{block}"
        );
        assert_eq!(block.lines().count(), 1 + 2 + 3, "{block}");
    }

    #[test]
    fn a_document_the_reference_rejects_is_reported_per_rubric() {
        let d = details(&json!({ "manifests": ["x"] }));
        for r in d["rubrics"].as_array().unwrap() {
            assert!(
                r["error"]
                    .as_str()
                    .unwrap()
                    .starts_with("the rubric could not be evaluated"),
                "{r}"
            );
            assert!(r.get("results").is_none());
        }
        assert!(summary_lines(&d)[0].contains("could not be evaluated"));
    }

    #[test]
    fn integrity_failures_name_their_codes() {
        let d = details(&sample("v-integrity-failures"));
        let block = render(&d);
        assert!(
            summary_lines(&d)[0].ends_with("3 of 6 met, 3 not met"),
            "{:?}",
            summary_lines(&d)
        );
        assert!(block.contains("assertion.dataHash.mismatch"), "{block}");
    }
}
