//! The C2PA signals evaluator: one node per manifest in a crJSON document (plus
//! synthesized nodes for flat ingredients that declare a source type), every rubric
//! statement evaluated against each node on its own, and the result in the
//! reference's output schema (`asset-rubric-signals-local-output.schema.json`).
//! Ported from `c2pa_signals_rubric_evaluator.py` (c2pa-org/conformance-public,
//! CC BY 4.0).

use crate::formula::Error;
use crate::rubric::Rubric;
use crate::value::{JValue, Map};

/// A statement that failed on a node: the reference prints these to stderr and
/// reports no signal; here they are kept for the caller.
#[derive(Debug, Clone)]
pub struct StatementError {
    /// The node's index in the output's `manifests` array.
    pub node: usize,
    /// The statement's id.
    pub trait_id: String,
    /// Why it failed.
    pub error: Error,
}

/// The evaluator's result.
#[derive(Debug, Clone)]
pub struct SignalsReport {
    /// The output document, in the reference's schema and key order.
    pub output: JValue,
    /// Statements that failed (each counts as "no signal").
    pub errors: Vec<StatementError>,
    /// The manifest label of each entry in `output.manifests`, in the same order:
    /// the crJSON label for a manifest, the synthesized `urn:c2pa:pseudo:...` label
    /// for a flat ingredient. The output's own ingredient `index` values point
    /// into this order, which follows the crJSON input and differs between
    /// exporters, so labels are the stable way to name a node. Kept outside
    /// `output` so the document stays exactly the reference's.
    pub labels: Vec<String>,
}

struct Edge {
    parent: JValue,
    relationship: JValue,
}

struct DagNode {
    urn: String,
    manifest: JValue,
    edges: Vec<Edge>,
    signals: Vec<JValue>,
    is_pseudo: bool,
    parent_node_id: Option<String>,
    mime_type: Option<JValue>,
}

fn crash(what: &str) -> Error {
    Error::eval(format!("the reference evaluator would stop here: {what}"))
}

/// `dict.get(key, default)` on a value that must be a dict (else the reference crashes).
fn get_or<'a>(
    v: &'a JValue,
    key: &str,
    default: &'a JValue,
    what: &str,
) -> Result<&'a JValue, Error> {
    match v {
        JValue::Object(m) => Ok(m.get(key).unwrap_or(default)),
        _ => Err(crash(what)),
    }
}

impl Rubric {
    /// Evaluate the signals rubric over a crJSON document.
    pub fn evaluate_signals(&self, crjson: &JValue) -> Result<SignalsReport, Error> {
        let empty_obj = JValue::Object(Map::new());
        let none = JValue::Null;
        let manifests: Vec<&JValue> = match crjson.get("manifests") {
            Some(JValue::Array(a)) => a.iter().collect(),
            Some(_) => return Err(crash("`manifests` is not a list")),
            None => Vec::new(),
        };

        // create_index_mapping
        let mut index_mapping: Vec<(String, String)> = Vec::new();
        let label_of = |m: &JValue| -> Result<Option<String>, Error> {
            match get_or(m, "label", &none, "a manifest is not an object")? {
                JValue::Str(s) if !s.is_empty() => Ok(Some(s.clone())),
                v if !v.py_truthy() => Ok(None),
                _ => Err(crash("a manifest label is not a string")),
            }
        };
        for (idx, m) in manifests.iter().enumerate() {
            if let Some(label) = label_of(m)? {
                match index_mapping.iter_mut().find(|(l, _)| *l == label) {
                    Some(slot) => slot.1 = idx.to_string(),
                    None => index_mapping.push((label, idx.to_string())),
                }
            }
        }
        let map_label = |l: &str| {
            index_mapping
                .iter()
                .find(|(x, _)| x == l)
                .map(|(_, i)| i.clone())
        };

        // build_provenance_dag: an insertion-ordered map node_id -> node.
        let mut dag: Vec<(String, DagNode)> = Vec::new();
        fn node_mut<'a>(dag: &'a mut [(String, DagNode)], id: &str) -> Option<&'a mut DagNode> {
            dag.iter_mut().find(|(k, _)| k == id).map(|(_, n)| n)
        }
        for m in &manifests {
            let Some(label) = label_of(m)? else { continue };
            let node_id = map_label(&label).unwrap_or_else(|| "Unknown".into());
            let mut node = DagNode {
                urn: label.clone(),
                manifest: (*m).clone(),
                edges: Vec::new(),
                signals: Vec::new(),
                is_pseudo: false,
                parent_node_id: None,
                mime_type: None,
            };
            let claim = [m.get("claim.v2"), m.get("claim")]
                .into_iter()
                .flatten()
                .find(|c| c.py_truthy())
                .unwrap_or(&empty_obj);
            let mime = get_or(claim, "dc:format", &none, "a claim is not an object")?;
            if mime.py_truthy() {
                node.mime_type = Some(mime.clone());
            }
            match dag.iter_mut().find(|(k, _)| *k == node_id) {
                Some(slot) => slot.1 = node,
                None => dag.push((node_id, node)),
            }
        }
        let mut next_node_index = manifests.len();
        for m in &manifests {
            let Some(label) = label_of(m)? else { continue };
            let node_id = map_label(&label).unwrap_or_else(|| "Unknown".into());
            let assertions =
                match get_or(m, "assertions", &empty_obj, "a manifest is not an object")? {
                    JValue::Object(a) => a.clone(),
                    _ => return Err(crash("`assertions` is not an object")),
                };
            for (key, value) in assertions.iter() {
                if key.starts_with("c2pa.ingredient") {
                    let active = get_or(
                        value,
                        "activeManifest",
                        &none,
                        "an ingredient is not an object",
                    )?;
                    let url = if active.py_truthy() {
                        get_or(active, "url", &none, "activeManifest is not an object")?.clone()
                    } else {
                        JValue::Null
                    };
                    let relationship = get_or(value, "relationship", &none, "")?.clone();
                    if url.py_truthy() {
                        let JValue::Str(url) = url else {
                            return Err(crash("an ingredient url is not a string"));
                        };
                        let mut parent_urn = url.clone();
                        if let Some(p) = url.find("urn:c2pa:") {
                            parent_urn = url[p..].to_string();
                            if let Some(slash) = parent_urn.find('/') {
                                parent_urn.truncate(slash);
                            }
                        }
                        let parent_id = map_label(&parent_urn).unwrap_or(parent_urn);
                        if let Some(n) = node_mut(&mut dag, &node_id) {
                            n.edges.push(Edge {
                                parent: JValue::Str(parent_id.clone()),
                                relationship,
                            });
                        }
                        let mime = get_or(value, "dc:format", &none, "")?;
                        if mime.py_truthy() {
                            if let Some(p) = node_mut(&mut dag, &parent_id) {
                                if p.mime_type.is_none() && !p.is_pseudo {
                                    p.mime_type = Some(mime.clone());
                                }
                            }
                        }
                    } else {
                        let mut dst = get_or(value, "digitalSourceType", &none, "")?.clone();
                        if !dst.py_truthy() {
                            'scan: for (a_key, a_val) in assertions.iter() {
                                if !a_key.starts_with("c2pa.actions") {
                                    continue;
                                }
                                let empty = JValue::Array(Vec::new());
                                let JValue::Array(actions) = get_or(
                                    a_val,
                                    "actions",
                                    &empty,
                                    "an actions assertion is not an object",
                                )?
                                else {
                                    return Err(crash("`actions` is not a list"));
                                };
                                for act in actions {
                                    let a =
                                        get_or(act, "action", &none, "an action is not an object")?;
                                    let d = get_or(act, "digitalSourceType", &none, "")?;
                                    if a.as_str() == Some("c2pa.opened") && d.py_truthy() {
                                        dst = d.clone();
                                        break 'scan;
                                    }
                                }
                            }
                        }
                        if dst.py_truthy() {
                            let pseudo_id = next_node_index.to_string();
                            next_node_index += 1;
                            let default_title = JValue::Str("flat-ingredient".into());
                            let JValue::Str(title) = get_or(value, "dc:title", &default_title, "")?
                            else {
                                return Err(crash("an ingredient title is not a string"));
                            };
                            let safe: String = title
                                .chars()
                                .map(|c| {
                                    if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                                        c
                                    } else {
                                        '_'
                                    }
                                })
                                .collect();
                            let pseudo_label =
                                format!("urn:c2pa:pseudo:{label}:{safe}_{pseudo_id}");
                            let mut action = Map::new();
                            action.insert("action", JValue::Str("c2pa.created".into()));
                            action.insert("digitalSourceType", dst);
                            let mut actions = Map::new();
                            actions.insert("allActionsIncluded", JValue::Bool(true));
                            actions.insert("actions", JValue::Array(vec![JValue::Object(action)]));
                            let mut asserts = Map::new();
                            asserts.insert("c2pa.actions.v2", JValue::Object(actions));
                            let mut pm = Map::new();
                            pm.insert("label", JValue::Str(pseudo_label.clone()));
                            pm.insert("assertions", JValue::Object(asserts));
                            dag.push((
                                pseudo_id.clone(),
                                DagNode {
                                    urn: pseudo_label,
                                    manifest: JValue::Object(pm),
                                    edges: Vec::new(),
                                    signals: Vec::new(),
                                    is_pseudo: true,
                                    parent_node_id: Some(node_id.clone()),
                                    mime_type: Some(get_or(value, "dc:format", &none, "")?.clone()),
                                },
                            ));
                            if let Some(n) = node_mut(&mut dag, &node_id) {
                                n.edges.push(Edge {
                                    parent: JValue::Str(pseudo_id),
                                    relationship,
                                });
                            }
                        }
                    }
                } else if key.starts_with("c2pa.thumbnail") {
                    let mime = get_or(
                        value,
                        "format",
                        &none,
                        "a thumbnail assertion is not an object",
                    )?;
                    if mime.py_truthy() {
                        if let Some(n) = node_mut(&mut dag, &node_id) {
                            if n.mime_type.is_none() && !n.is_pseudo {
                                n.mime_type = Some(mime.clone());
                            }
                        }
                    }
                }
            }
        }

        // Node-local evaluation.
        let mut failures: Vec<(String, String, Error)> = Vec::new();
        for (node_id, node) in dag.iter_mut() {
            for s in &self.statements {
                let trait_str = match &s.id {
                    JValue::Str(t) => t.clone(),
                    _ => continue,
                };
                let v = match self.run(s, &node.manifest) {
                    Ok(None) => continue,
                    Ok(Some(v)) => v,
                    Err(e) => {
                        failures.push((node_id.clone(), trait_str, e));
                        continue;
                    }
                };
                let (fires, multiple) = match &v {
                    JValue::Array(a) => (!a.is_empty(), a.len() > 1),
                    JValue::Bool(b) => (*b, false),
                    JValue::Int(i) => (*i > 0, false),
                    JValue::Float(f) => (*f > 0.0, false),
                    JValue::Null => (false, false),
                    _ => (true, false),
                };
                if !fires {
                    continue;
                }
                let report = match s.raw.get("reportText") {
                    None => JValue::Str(trait_str.clone()),
                    Some(JValue::Object(rt)) => match rt.get("true") {
                        None => JValue::Str(trait_str.clone()),
                        Some(JValue::Object(t)) => t
                            .get("en")
                            .cloned()
                            .unwrap_or(JValue::Str(trait_str.clone())),
                        Some(_) => {
                            failures.push((
                                node_id.clone(),
                                trait_str,
                                crash("reportText.true is not an object"),
                            ));
                            continue;
                        }
                    },
                    Some(_) => {
                        failures.push((
                            node_id.clone(),
                            trait_str,
                            crash("reportText is not an object"),
                        ));
                        continue;
                    }
                };
                let mut sig = Map::new();
                sig.insert("trait", JValue::Str(trait_str));
                sig.insert("reportText", report);
                sig.insert("multiple", JValue::Bool(multiple));
                node.signals.push(JValue::Object(sig));
            }
        }

        // Output: real manifests first, then pseudo nodes, each group by numeric id.
        let mut order: Vec<usize> = (0..dag.len()).collect();
        let num = |id: &str| {
            id.parse::<i128>()
                .map_err(|_| crash("a node id is not a number"))
        };
        let mut keys = Vec::with_capacity(dag.len());
        for (i, (id, n)) in dag.iter().enumerate() {
            keys.push((u8::from(n.is_pseudo), num(id)?, i));
        }
        order.sort_by_key(|&i| (keys[i].0, keys[i].1));
        let new_index = |id: &str| order.iter().position(|&i| dag[i].0 == id);

        let subject_of = |m: &JValue| -> Result<JValue, Error> {
            let sig = get_or(m, "signature", &empty_obj, "a manifest is not an object")?;
            let ci = get_or(
                sig,
                "certificateInfo",
                &empty_obj,
                "`signature` is not an object",
            )?;
            let subj = get_or(
                ci,
                "subject",
                &empty_obj,
                "`certificateInfo` is not an object",
            )?;
            let cn_default = JValue::Str("Unknown CN".into());
            let o_default = JValue::Str("Unknown O".into());
            let mut s = Map::new();
            s.insert(
                "CN",
                get_or(subj, "CN", &cn_default, "`subject` is not an object")?.clone(),
            );
            s.insert("O", get_or(subj, "O", &o_default, "")?.clone());
            let ou = get_or(subj, "OU", &none, "")?;
            if ou.py_truthy() {
                s.insert("OU", ou.clone());
            }
            let mut a = Map::new();
            a.insert("subject", JValue::Object(s));
            Ok(JValue::Object(a))
        };

        let mut out_manifests = Vec::with_capacity(order.len());
        for &i in &order {
            let (_, n) = &dag[i];
            let asserted_by = if n.is_pseudo {
                let pid = n.parent_node_id.as_deref().unwrap_or_default();
                let parent = dag
                    .iter()
                    .find(|(k, _)| k == pid)
                    .map(|(_, p)| &p.manifest)
                    .ok_or_else(|| crash("a pseudo node has no parent"))?;
                subject_of(parent)?
            } else {
                subject_of(&n.manifest)?
            };
            let (mut inc, mut tr) = (Vec::new(), Vec::new());
            for s in &n.signals {
                let t = s.get("trait").and_then(JValue::as_str).unwrap_or_default();
                if t.starts_with("inception:") {
                    inc.push(s.clone());
                } else if t.starts_with("transformation:") {
                    tr.push(s.clone());
                }
            }
            let mut ingredients = Vec::new();
            for e in &n.edges {
                let pid = match &e.parent {
                    JValue::Str(s) => s.clone(),
                    other => crate::formula::eval_to_string(other).unwrap_or_default(),
                };
                let index = match new_index(&pid) {
                    Some(x) => Some(JValue::Int(x as i128)),
                    None => py_int(&pid).map(JValue::Int),
                };
                if let Some(index) = index {
                    let mut ing = Map::new();
                    ing.insert("index", index);
                    ing.insert("relationship", e.relationship.clone());
                    ingredients.push(JValue::Object(ing));
                }
            }
            let mut entry = Map::new();
            entry.insert("assertedBy", asserted_by);
            entry.insert("dc:format", n.mime_type.clone().unwrap_or(JValue::Null));
            entry.insert("localInceptions", JValue::Array(inc));
            entry.insert("localTransformations", JValue::Array(tr));
            entry.insert("ingredients", JValue::Array(ingredients));
            if n.is_pseudo {
                entry.insert("pseudo", JValue::Bool(true));
            }
            out_manifests.push(JValue::Object(entry));
        }
        let mut output = Map::new();
        output.insert("rubricName", self.name.clone());
        output.insert("rubricVersion", self.version.clone());
        output.insert("manifests", JValue::Array(out_manifests));
        let labels = order.iter().map(|&i| dag[i].1.urn.clone()).collect();
        let errors = failures
            .into_iter()
            .map(|(id, trait_id, error)| StatementError {
                node: new_index(&id).unwrap_or(usize::MAX),
                trait_id,
                error,
            })
            .collect();
        Ok(SignalsReport {
            output: JValue::Object(output),
            errors,
            labels,
        })
    }
}

/// Python's `int(str)` for the strings that can reach it here: optional sign,
/// digits with single underscores between them, surrounding whitespace.
fn py_int(s: &str) -> Option<i128> {
    let t = s.trim();
    let (sign, body) = match t.strip_prefix('-') {
        Some(b) => (-1, b),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    if !body.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return None;
    }
    body.replace('_', "").parse::<i128>().ok().map(|v| sign * v)
}
