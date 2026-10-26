//! The C2PA signals rubric over this file's manifest store.
//!
//! The C2PA's Asset Signals Rubric (Local) 1.0 labels each manifest in a provenance
//! chain with what its signer declared: inception signals (`capturedMedia`,
//! `fullyGenAIMedia`, `blankCanvas`, ...) and transformation signals
//! (`perceptibleGenAI`, `nonEditorial`, `ambiguousActions`, ...). Halftone runs the
//! published rubric file unmodified (`halftone-rubric`, held to the C2PA's own
//! evaluator by golden tests) over the crJSON that c2pa exports for the store.
//!
//! A signal classifies a declaration; it verifies nothing. The rubric reads every
//! manifest, including ones that failed validation, so `details.signals` is always
//! read next to `details.validation_state`, and it never changes the verdict.
//! Absence of a signal is not a finding either: the rubric only recognises exact
//! IPTC URIs in `c2pa.actions.v2`, so an older or loosely written manifest can
//! declare a generative step and still raise no signal.
//!
//! `details.signals.nodes` names every node by its manifest label; the rubric's
//! own output (`details.signals.output`, in the reference schema) numbers nodes by
//! position in the crJSON, which depends on the exporter. Pure: compiles and is
//! tested without the `c2pa` feature.

use halftone_rubric::{JValue, Rubric};
use serde_json::{Value, json};
use std::sync::OnceLock;

/// The vendored rubric, parsed once per process.
fn rubric() -> Result<&'static Rubric, String> {
    static RUBRIC: OnceLock<Result<Rubric, String>> = OnceLock::new();
    RUBRIC
        .get_or_init(|| halftone_rubric::signals_local().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

/// Which rubric ran: name and version from its metadata, and the file's SHA-256.
fn rubric_id(r: &Rubric) -> Value {
    json!({
        "name": r.name.to_serde(),
        "version": r.version.to_serde(),
        "sha256": halftone_rubric::SIGNALS_LOCAL_SHA256,
    })
}

/// Trait ids of a list of signals, e.g. `["inception:signal_capturedMedia"]`.
fn traits(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|s| s.get("trait").cloned()).collect())
        .unwrap_or_default()
}

/// `details.signals` for one crJSON document: the rubric's result with every node
/// named by its manifest label, the reference-schema output verbatim (for
/// differential runs against the C2PA's evaluator), and any statement that failed
/// to evaluate (the reference reports those as no signal, and so does this).
pub fn details(crjson: &Value) -> Value {
    let r = match rubric() {
        Ok(r) => r,
        Err(e) => return json!({ "error": format!("the signals rubric did not load: {e}") }),
    };
    let generator = crjson.get("jsonGenerator").cloned().unwrap_or(Value::Null);
    let report = match r.evaluate_signals(&JValue::from_serde(crjson)) {
        Ok(rep) => rep,
        Err(e) => {
            return json!({
                "rubric": rubric_id(r),
                "crjson_generator": generator,
                "error": e.to_string(),
            });
        }
    };
    let output = report.output.to_serde();
    let entries = output
        .get("manifests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let label_at = |i: Option<u64>| {
        i.and_then(|i| usize::try_from(i).ok())
            .and_then(|i| report.labels.get(i))
            .map_or(Value::Null, |l| Value::String(l.clone()))
    };
    let nodes: Vec<Value> = entries
        .iter()
        .zip(&report.labels)
        .map(|(e, label)| {
            let ingredients: Vec<Value> = e
                .get("ingredients")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|ing| {
                            json!({
                                "label": label_at(ing.get("index").and_then(Value::as_u64)),
                                "relationship": ing.get("relationship").cloned().unwrap_or(Value::Null),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "label": label,
                "pseudo": e.get("pseudo").and_then(Value::as_bool).unwrap_or(false),
                "inception": traits(e.get("localInceptions")),
                "transformation": traits(e.get("localTransformations")),
                "ingredients": ingredients,
            })
        })
        .collect();
    let errors: Vec<Value> = report
        .errors
        .iter()
        .map(|e| {
            json!({
                "label": report.labels.get(e.node).cloned(),
                "trait": e.trait_id,
                "error": e.error.to_string(),
            })
        })
        .collect();
    json!({
        "rubric": rubric_id(r),
        "crjson_generator": generator,
        "nodes": nodes,
        "errors": errors,
        "output": output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: &str = "http://cv.iptc.org/newscodes/digitalsourcetype/";

    fn manifest(label: &str, actions: Value, extra: Value) -> Value {
        let mut assertions =
            json!({ "c2pa.actions.v2": { "actions": actions, "allActionsIncluded": true } });
        if let (Some(a), Some(x)) = (assertions.as_object_mut(), extra.as_object()) {
            a.extend(x.clone());
        }
        json!({
            "label": label,
            "assertions": assertions,
            "claim.v2": { "created_assertions": [{ "url": "self#jumbf=c2pa.assertions/c2pa.actions.v2" }] },
            "signature": { "certificateInfo": { "subject": { "CN": "t", "O": "Test" } } },
        })
    }

    /// c2pa-rs order: the active manifest first, its ingredient after it.
    fn edit_of_capture() -> Value {
        json!({
            "jsonGenerator": { "name": "c2pa-rs", "version": "0.91.1" },
            "manifests": [
                manifest(
                    "urn:c2pa:edit",
                    json!([{ "action": "c2pa.opened" },
                           { "action": "c2pa.edited", "digitalSourceType": format!("{D}trainedAlgorithmicMedia") }]),
                    json!({ "c2pa.ingredient.v3": { "relationship": "parentOf",
                        "activeManifest": { "url": "self#jumbf=/c2pa/urn:c2pa:cam/c2pa.assertions/c2pa.claim.v2" } } }),
                ),
                manifest("urn:c2pa:cam", json!([{ "action": "c2pa.created", "digitalSourceType": format!("{D}digitalCapture") }]), json!({})),
            ]
        })
    }

    #[test]
    fn nodes_are_named_by_label_and_ingredients_point_at_labels() {
        let d = details(&edit_of_capture());
        assert_eq!(d["rubric"]["version"], "1.0");
        assert_eq!(d["rubric"]["sha256"], halftone_rubric::SIGNALS_LOCAL_SHA256);
        assert_eq!(d["crjson_generator"]["name"], "c2pa-rs");
        assert_eq!(d["errors"], json!([]));
        let nodes = d["nodes"].as_array().unwrap();
        assert_eq!(nodes[0]["label"], "urn:c2pa:edit");
        assert_eq!(
            nodes[0]["transformation"],
            json!(["transformation:signal_perceptibleGenAI"])
        );
        assert_eq!(
            nodes[0]["ingredients"],
            json!([{ "label": "urn:c2pa:cam", "relationship": "parentOf" }])
        );
        assert_eq!(nodes[1]["label"], "urn:c2pa:cam");
        assert_eq!(
            nodes[1]["inception"],
            json!(["inception:signal_capturedMedia"])
        );
        assert_eq!(nodes[1]["pseudo"], false);
    }

    #[test]
    fn the_same_store_in_another_order_gives_the_same_nodes_by_label() {
        let mut other = edit_of_capture();
        other["manifests"].as_array_mut().unwrap().reverse();
        let by_label = |d: &Value| {
            let mut v: Vec<Value> = d["nodes"].as_array().unwrap().clone();
            v.sort_by_key(|n| n["label"].as_str().unwrap().to_string());
            v
        };
        let (a, b) = (details(&edit_of_capture()), details(&other));
        assert_eq!(by_label(&a), by_label(&b));
        // ...while the positional output differs: indices follow the input order.
        assert_ne!(a["output"], b["output"]);
    }

    #[test]
    fn a_flat_ingredient_becomes_a_labelled_pseudo_node() {
        let doc = json!({ "manifests": [manifest(
            "urn:c2pa:top",
            json!([{ "action": "c2pa.opened" }]),
            json!({ "c2pa.ingredient.v3": { "relationship": "parentOf", "dc:title": "photo.jpg",
                    "digitalSourceType": format!("{D}digitalCapture") } }),
        )] });
        let d = details(&doc);
        let nodes = d["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1]["pseudo"], true);
        assert_eq!(
            nodes[1]["label"],
            "urn:c2pa:pseudo:urn:c2pa:top:photo.jpg_1"
        );
        assert_eq!(nodes[0]["ingredients"][0]["label"], nodes[1]["label"]);
        assert_eq!(
            nodes[0]["inception"],
            json!(["inception:signal_mediaUnknownProvenance"])
        );
    }

    #[test]
    fn a_document_the_reference_would_reject_is_an_error_not_a_panic() {
        let d = details(&json!({ "manifests": { "not": "a list" } }));
        assert!(
            d["error"]
                .as_str()
                .is_some_and(|e| e.contains("not a list"))
        );
        assert_eq!(d["rubric"]["version"], "1.0");
    }
}
