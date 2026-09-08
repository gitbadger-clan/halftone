//! `digitalSourceType` declarations inside a C2PA manifest.
//!
//! Where the signal lives: C2PA reuses the IPTC Digital Source Type vocabulary in
//! two places, both inside the signed claim: the `digitalSourceType` field of each
//! action in a `c2pa.actions` / `c2pa.actions.v2` assertion, and the
//! `Iptc4xmpExt:DigitalSourceType` property of a `stds.iptc` /
//! `stds.iptc.photo-metadata` assertion. Unlike the unsigned XMP copy the container
//! layer reads, these are covered by the manifest signature, so they say what the
//! signer asserted, verified by the validation state the layer reports alongside.
//!
//! A manifest is a history, not a single field: an actions list may legitimately
//! declare `digitalCapture` for `c2pa.created` and `compositeWithTrainedAlgorithmicMedia`
//! for a later `c2pa.edited`. Every occurrence is reported with its path; the
//! summary flag `declares_ai` is true if any occurrence is a generative term.
//!
//! This module reads the `serde_json::Value` produced by `c2pa::Reader::json()` and
//! depends on nothing else, so it compiles and is tested without the `c2pa` feature.

use halftone_core::dst::{code_of, lookup, TermKind};
use serde::Serialize;

/// One `digitalSourceType` occurrence found in a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceTypeHit {
    /// JSON path to the value within the active manifest, e.g.
    /// `assertions[0].data.actions[1].digitalSourceType`.
    pub path: String,
    /// Assertion label the value sits under, if it is inside an assertion.
    pub assertion: Option<String>,
    /// Action name (`c2pa.created`, `c2pa.edited`, …) if the value is on an action.
    pub action: Option<String>,
    /// The value exactly as written.
    pub raw: String,
    /// Term code: the last path segment of `raw`.
    pub code: String,
    /// Interpretation from the shared vocabulary; `None` for unknown terms.
    pub kind: Option<TermKind>,
}

/// Summary of the declarations in one manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SourceTypeSummary {
    /// Every occurrence, in document order.
    pub hits: Vec<SourceTypeHit>,
    /// Distinct term codes, in first-seen order.
    pub codes: Vec<String>,
    /// Any occurrence is a generative term.
    pub declares_ai: bool,
    /// Codes not in the vocabulary, reported verbatim.
    pub unknown: Vec<String>,
}

/// Find every `digitalSourceType` declaration in a manifest JSON value.
pub fn find(manifest: &serde_json::Value) -> SourceTypeSummary {
    let mut out = SourceTypeSummary::default();
    walk(manifest, "", None, None, &mut out.hits);
    for h in &out.hits {
        if !out.codes.contains(&h.code) {
            out.codes.push(h.code.clone());
        }
        match h.kind {
            Some(TermKind::Generative) => out.declares_ai = true,
            Some(_) => {}
            None => {
                if !out.unknown.contains(&h.code) {
                    out.unknown.push(h.code.clone());
                }
            }
        }
    }
    out
}

/// One or two sentences for the rationale, or an empty string if nothing is declared.
pub fn describe(s: &SourceTypeSummary) -> String {
    if s.codes.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    for code in &s.codes {
        match lookup(code) {
            Some(t) => parts.push(format!("{} ({})", t.code, t.label)),
            None => parts.push(format!("{code} (not in the known vocabulary)")),
        }
    }
    let list = parts.join(", ");
    if s.declares_ai {
        format!(
            " The manifest declares digitalSourceType = {list}: the signer asserts the \
             content is generated or composited with generative AI."
        )
    } else if !s.unknown.is_empty() && s.unknown.len() == s.codes.len() {
        format!(" The manifest declares digitalSourceType = {list}; reported verbatim.")
    } else {
        format!(
            " The manifest declares digitalSourceType = {list}: the signer asserts a \
             non-generative source."
        )
    }
}

fn is_dst_key(k: &str) -> bool {
    k == "digitalSourceType" || k.ends_with(":DigitalSourceType") || k == "DigitalSourceType"
}

fn walk(
    v: &serde_json::Value,
    path: &str,
    assertion: Option<&str>,
    action: Option<&str>,
    out: &mut Vec<SourceTypeHit>,
) {
    match v {
        serde_json::Value::Object(o) => {
            // An assertion object carries its own label; an action carries `action`.
            let here_assertion = o
                .get("label")
                .and_then(|l| l.as_str())
                .filter(|_| o.contains_key("data"))
                .or(assertion);
            let here_action = o.get("action").and_then(|a| a.as_str()).or(action);
            for (k, child) in o {
                let child_path = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if is_dst_key(k) {
                    if let Some(raw) = child.as_str() {
                        let code = code_of(raw);
                        out.push(SourceTypeHit {
                            path: child_path,
                            assertion: here_assertion.map(str::to_string),
                            action: here_action.map(str::to_string),
                            raw: raw.to_string(),
                            code: code.clone(),
                            kind: lookup(&code).map(|t| t.kind),
                        });
                        continue;
                    }
                }
                walk(child, &child_path, here_assertion, here_action, out);
            }
        }
        serde_json::Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                walk(child, &format!("{path}[{i}]"), assertion, action, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn actions_manifest(actions: serde_json::Value) -> serde_json::Value {
        json!({
            "claim_generator": "ChatGPT",
            "title": "image.png",
            "assertions": [
                {"label": "c2pa.actions.v2", "data": {"actions": actions}}
            ]
        })
    }

    #[test]
    fn generative_action_declares_ai_with_path() {
        let m = actions_manifest(json!([
            {"action": "c2pa.created",
             "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"}
        ]));
        let s = find(&m);
        assert!(s.declares_ai);
        assert_eq!(s.codes, ["trainedAlgorithmicMedia"]);
        assert_eq!(s.hits[0].assertion.as_deref(), Some("c2pa.actions.v2"));
        assert_eq!(s.hits[0].action.as_deref(), Some("c2pa.created"));
        assert_eq!(
            s.hits[0].path,
            "assertions[0].data.actions[0].digitalSourceType"
        );
        assert!(describe(&s).contains("trainedAlgorithmicMedia (created using generative AI)"));
    }

    #[test]
    fn history_with_capture_then_generative_edit_is_ai_not_conflict() {
        let m = actions_manifest(json!([
            {"action": "c2pa.created",
             "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/digitalCapture"},
            {"action": "c2pa.edited",
             "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/compositeWithTrainedAlgorithmicMedia"}
        ]));
        let s = find(&m);
        assert!(s.declares_ai);
        assert_eq!(
            s.codes,
            ["digitalCapture", "compositeWithTrainedAlgorithmicMedia"]
        );
        let d = describe(&s);
        assert!(d.contains("digitalCapture"));
        assert!(d.contains("generated or composited"));
    }

    #[test]
    fn capture_only_is_not_ai() {
        let m = actions_manifest(json!([
            {"action": "c2pa.created",
             "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/digitalCapture"}
        ]));
        let s = find(&m);
        assert!(!s.declares_ai);
        assert!(describe(&s).contains("non-generative source"));
    }

    #[test]
    fn iptc_assertion_is_found_by_prefixed_key() {
        let m = json!({
            "assertions": [
                {"label": "stds.iptc.photo-metadata",
                 "data": {"Iptc4xmpExt:DigitalSourceType":
                          "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"}}
            ]
        });
        let s = find(&m);
        assert!(s.declares_ai);
        assert_eq!(
            s.hits[0].assertion.as_deref(),
            Some("stds.iptc.photo-metadata")
        );
        assert_eq!(s.hits[0].action, None);
    }

    #[test]
    fn unknown_term_is_reported_verbatim() {
        let m = actions_manifest(json!([
            {"action": "c2pa.created", "digitalSourceType": "trainedalgorithmicmedia"}
        ]));
        let s = find(&m);
        assert!(!s.declares_ai);
        assert_eq!(s.unknown, ["trainedalgorithmicmedia"]);
        assert!(describe(&s).contains("reported verbatim"));
    }

    #[test]
    fn nothing_declared_is_empty() {
        let m = actions_manifest(json!([{"action": "c2pa.created"}]));
        let s = find(&m);
        assert!(s.codes.is_empty());
        assert!(!s.declares_ai);
        assert_eq!(describe(&s), "");
    }

    #[test]
    fn substring_in_unrelated_string_does_not_count() {
        // The old `json_contains` heuristic would have flagged this.
        let m = json!({"title": "trainedAlgorithmicMedia-study.png"});
        let s = find(&m);
        assert!(s.codes.is_empty());
    }
}
