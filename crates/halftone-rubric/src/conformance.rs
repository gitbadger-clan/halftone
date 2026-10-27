//! The C2PA conformance evaluator: every statement of a rubric evaluated once
//! against a whole crJSON document, each result classified as met, not met or a
//! warning, and the output in the reference's shape. Ported from
//! `c2pa_conformance_rubric_evaluator.py` (c2pa-org/conformance-public, CC BY 4.0).
//! It runs the integrity and conformance rubrics; the signals rubric has its own
//! per-manifest evaluator ([`crate::signals`]).
//!
//! Faithful to the reference, including where it is unforgiving: a document it
//! rejects outright (an exception outside its per-statement handling, such as a
//! manifest that is not an object) is an error here too, never a partial result.
//! A statement that fails is a "not met" row whose text starts with `Error:`; the
//! wording after it is this implementation's, not Python's.
//!
//! Not ported: the reference's pruning step, which relabels "fully GenAI" signals
//! by the branch they came from. It only acts on traits listed in `cut_off_traits`,
//! which is empty in the pinned evaluator, so it never runs; the fixture generator
//! refuses to run if a future evaluator fills that list.

use crate::formula::{Error, eval_to_string};
use crate::rubric::{Rubric, Statement};
use crate::value::{JValue, Map};

/// How a statement came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The check is met.
    Met,
    /// The check is not met (or could not be evaluated: see [`Row::error`]).
    NotMet,
    /// A warning (`warnIfMatched` statements).
    Warning,
}

/// One result row, in output order.
#[derive(Debug, Clone)]
pub struct Row {
    /// The trait as displayed: the statement id after its category prefix.
    pub trait_name: String,
    /// The output category (`validation`, `inceptions`, ...).
    pub category: String,
    /// How the statement came out.
    pub outcome: Outcome,
    /// The statement could not be evaluated (`outcome` is then `NotMet`).
    pub error: bool,
    /// The rubric's report text for this outcome, `{{matches}}` filled in.
    pub report_text: JValue,
    /// For a row about one manifest (a statement whose result is a list of
    /// manifests): its signer, as the reference reports it.
    pub asserted_by: Option<JValue>,
    /// For such a row: the manifest's label (the reference drops it from its
    /// output; Halftone keeps it to name the manifest).
    pub manifest_label: Option<JValue>,
}

/// The evaluator's result.
#[derive(Debug, Clone)]
pub struct ConformanceReport {
    /// The output document, in the reference's schema.
    pub output: JValue,
    /// The same rows, structured, in output order.
    pub rows: Vec<Row>,
}

fn crash(what: &str) -> Error {
    Error::eval(format!("the reference evaluator would stop here: {what}"))
}

/// Python's `str()` of a JSON value.
fn py_str(v: &JValue) -> String {
    match v {
        JValue::Str(s) => s.clone(),
        JValue::Null => "None".into(),
        JValue::Bool(b) => if *b { "True" } else { "False" }.into(),
        JValue::Int(_) | JValue::Float(_) => eval_to_string(v).unwrap_or_default(),
        JValue::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        JValue::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}: {}", py_repr(&JValue::Str(k.clone())), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python's `repr()` of a JSON value (strings quoted as Python quotes them).
fn py_repr(v: &JValue) -> String {
    match v {
        JValue::Str(s) => {
            let q = if s.contains('\'') && !s.contains('"') {
                '"'
            } else {
                '\''
            };
            let mut out = String::from(q);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c == q => {
                        out.push('\\');
                        out.push(c);
                    }
                    c => out.push(c),
                }
            }
            out.push(q);
            out
        }
        other => py_str(other),
    }
}

/// `build_provenance_dag` builds a graph the evaluator only uses for its (inactive)
/// pruning, but building it can raise: reproduce exactly those failures.
fn check_document(doc: &JValue) -> Result<(), Error> {
    let manifests = match doc {
        JValue::Object(m) => match m.get("manifests") {
            None => return Ok(()),
            Some(v) => v,
        },
        // `"manifests" not in data` on a list is a membership test; on a string, a
        // substring test followed by string indexing; on anything else a TypeError.
        JValue::Array(a) if !a.iter().any(|x| x.as_str() == Some("manifests")) => return Ok(()),
        JValue::Str(s) if !s.contains("manifests") => return Ok(()),
        _ => return Err(crash("the document is not an object")),
    };
    let JValue::Array(list) = manifests else {
        return match manifests {
            JValue::Object(m) if m.is_empty() => Ok(()),
            JValue::Str(s) if s.is_empty() => Ok(()),
            _ => Err(crash("`manifests` is not a list")),
        };
    };
    for m in list {
        let JValue::Object(m) = m else {
            return Err(crash("a manifest is not an object"));
        };
        if !m.get("label").is_some_and(JValue::py_truthy) {
            continue;
        }
        let assertions = match m.get("assertions") {
            None => continue,
            Some(JValue::Object(a)) => a,
            Some(_) => return Err(crash("`assertions` is not an object")),
        };
        for (key, value) in assertions.iter() {
            if !key.starts_with("c2pa.ingredient") {
                continue;
            }
            let JValue::Object(v) = value else {
                return Err(crash("an ingredient is not an object"));
            };
            let active = match v.get("activeManifest") {
                None => continue,
                Some(JValue::Object(a)) => a,
                Some(_) => return Err(crash("`activeManifest` is not an object")),
            };
            match active.get("url") {
                Some(u) if u.py_truthy() && u.as_str().is_none() => {
                    return Err(crash("an ingredient url is not a string"));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// `dict.get(key, {})` chained over a value that must be a dict.
fn get_or_empty<'a>(v: &'a JValue, key: &str, empty: &'a JValue) -> Result<&'a JValue, String> {
    match v {
        JValue::Object(m) => Ok(m.get(key).unwrap_or(empty)),
        _ => Err(format!(
            "'{}' object has no attribute 'get'",
            py_type_name(v)
        )),
    }
}

fn py_type_name(v: &JValue) -> &'static str {
    match v {
        JValue::Null => "NoneType",
        JValue::Bool(_) => "bool",
        JValue::Int(_) => "int",
        JValue::Float(_) => "float",
        JValue::Str(_) => "str",
        JValue::Array(_) => "list",
        JValue::Object(_) => "dict",
    }
}

/// The report text for `txt_val`, as the reference looks it up. `Err` where the
/// reference raises (inside its per-statement handling: an error row).
fn report_text(dict: &JValue, txt_val: &str) -> Result<JValue, String> {
    let pick = |v: &JValue| match v {
        JValue::Object(m) => Ok(m.get("en").cloned().unwrap_or(JValue::Str(String::new()))),
        other => Ok(JValue::Str(py_str(other))),
    };
    match dict {
        JValue::Object(m) => {
            if let Some(v) = m.get(txt_val) {
                pick(v)
            } else if let Some(v) = m.get("default") {
                pick(v)
            } else {
                Ok(JValue::Str(String::new()))
            }
        }
        JValue::Array(a) => {
            let has = |k: &str| a.iter().any(|x| x.as_str() == Some(k));
            if has(txt_val) || has("default") {
                Err("list indices must be integers or slices, not str".into())
            } else {
                Ok(JValue::Str(String::new()))
            }
        }
        JValue::Str(s) => {
            if s.contains(txt_val) || s.contains("default") {
                Err("string indices must be integers, not 'str'".into())
            } else {
                Ok(JValue::Str(String::new()))
            }
        }
        other => Err(format!(
            "argument of type '{}' is not iterable",
            py_type_name(other)
        )),
    }
}

impl Rubric {
    /// Evaluate this rubric over a crJSON document, as the reference conformance
    /// evaluator does. For the conformance rubrics, load the rubric with its expected
    /// spec version (see `halftone_rubric::conformance_0_2_spec_2_4`).
    pub fn evaluate_conformance(&self, doc: &JValue) -> Result<ConformanceReport, Error> {
        check_document(doc)?;
        let empty = JValue::Object(Map::new());
        let mut rows: Vec<(u8, Row)> = Vec::new();
        for s in &self.statements {
            self.evaluate_statement(s, doc, &empty, &mut rows)?;
        }
        rows.sort_by(|a, b| (a.0, &a.1.trait_name).cmp(&(b.0, &b.1.trait_name)));
        let rows: Vec<Row> = rows.into_iter().map(|(_, r)| r).collect();

        // Categories come from every statement with an `id`, evaluated or not.
        let mut categories: Vec<String> = Vec::new();
        for s in &self.statements {
            let Some(id) = s.raw.get("id") else { continue };
            let cat = match id {
                JValue::Str(t) => match t.split_once(':') {
                    Some((c, _)) => map_category(c),
                    None => "general".to_string(),
                },
                // `":" in id` is a membership test on a list or a dict.
                JValue::Array(a) if !a.iter().any(|x| x.as_str() == Some(":")) => {
                    "general".to_string()
                }
                JValue::Object(m) if !m.contains_key(":") => "general".to_string(),
                _ => return Err(crash("a statement id is not a string")),
            };
            if !categories.contains(&cat) {
                categories.push(cat);
            }
        }
        if categories.is_empty() {
            categories.push("general".into());
        }
        categories.sort();
        let bucket = || {
            let mut m = Map::new();
            for c in &categories {
                m.insert(c.clone(), JValue::Array(Vec::new()));
            }
            m
        };
        let (mut met, mut not_met, mut warned) = (bucket(), bucket(), bucket());
        for r in &rows {
            let target = match r.outcome {
                Outcome::Met => &mut met,
                Outcome::Warning => &mut warned,
                Outcome::NotMet => &mut not_met,
            };
            let mut row = Map::new();
            row.insert("trait", JValue::Str(r.trait_name.clone()));
            row.insert("reportText", r.report_text.clone());
            if let Some(a) = &r.asserted_by {
                row.insert("asserted_by", a.clone());
            }
            let mut list = match target.get(&r.category) {
                Some(JValue::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            list.push(JValue::Object(row));
            target.insert(r.category.clone(), JValue::Array(list));
        }
        let mut output = Map::new();
        output.insert("rubricName", self.name.clone());
        output.insert("rubricVersion", self.version.clone());
        output.insert("true", JValue::Object(met));
        output.insert("false", JValue::Object(not_met));
        output.insert("warning", JValue::Object(warned));
        Ok(ConformanceReport {
            output: JValue::Object(output),
            rows,
        })
    }

    fn evaluate_statement(
        &self,
        s: &Statement,
        doc: &JValue,
        empty: &JValue,
        rows: &mut Vec<(u8, Row)>,
    ) -> Result<(), Error> {
        let raw = &s.raw;
        let stmt_id = match raw.get("id") {
            Some(v) if v.py_truthy() => v.clone(),
            _ => JValue::Str("unnamed_trait".into()),
        };
        let JValue::Str(stmt_id) = stmt_id else {
            return Err(crash("a statement id is not a string"));
        };
        let report_dict = raw
            .get("reportText")
            .cloned()
            .unwrap_or_else(|| empty.clone());
        let fail_if = raw.get("failIfMatched").is_some_and(JValue::py_truthy);
        let warn_if = raw.get("warnIfMatched").is_some_and(JValue::py_truthy);

        let mut trait_name = stmt_id.clone();
        if stmt_id == "c2pa:signal_compliance" {
            // Outside the reference's error handling: a non-dict here stops it.
            let t = get_or_empty(&report_dict, "true", empty).map_err(|e| crash(&e))?;
            let en = get_or_empty(t, "en", &JValue::Null).map_err(|e| crash(&e))?;
            if let Some(text) = en.as_str().filter(|t| !t.is_empty()) {
                trait_name = text.strip_prefix("Contains ").unwrap_or(text).to_string();
            } else if en.py_truthy() {
                return Err(crash("a compliance report text is not a string"));
            }
        }
        let (category, display) = match trait_name.split_once(':') {
            Some((c, d)) => (c.to_string(), d.to_string()),
            None => ("general".to_string(), trait_name.clone()),
        };
        let category = map_category(&category);
        if s.ast.is_none() {
            return Ok(());
        }

        let error_row = |msg: String| Row {
            trait_name: display.clone(),
            category: category.clone(),
            outcome: Outcome::NotMet,
            error: true,
            report_text: JValue::Str(format!("Error: {msg}")),
            asserted_by: None,
            manifest_label: None,
        };
        let val = match self.run(s, doc) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(()),
            Err(e) => {
                rows.push((2, error_row(e.to_string())));
                return Ok(());
            }
        };

        let mut matches: Vec<JValue> = Vec::new();
        let state: &str = if fail_if || warn_if {
            match &val {
                JValue::Array(a) if !a.is_empty() => {
                    matches = a.clone();
                    if fail_if { "false" } else { "warn" }
                }
                _ => "true",
            }
        } else {
            match &val {
                JValue::Array(a) => {
                    let manifests = a.first().is_some_and(|f| {
                        matches!(f, JValue::Object(m) if m.contains_key("label") || m.contains_key("signature"))
                    });
                    if manifests {
                        matches = a.clone();
                        "true"
                    } else if a.is_empty() {
                        "false"
                    } else {
                        "true"
                    }
                }
                JValue::Bool(b) => {
                    if *b {
                        "true"
                    } else {
                        "false"
                    }
                }
                JValue::Int(i) => {
                    if *i > 0 {
                        "true"
                    } else {
                        "false"
                    }
                }
                JValue::Float(f) => {
                    if *f > 0.0 {
                        "true"
                    } else {
                        "false"
                    }
                }
                JValue::Null => "false",
                _ => "true",
            }
        };
        let txt_val = match state {
            "warn" if !matches!(&report_dict, JValue::Object(m) if m.contains_key("warn")) => {
                "false"
            }
            other => other,
        };
        // Everything below is inside the reference's per-statement handling.
        let mut text = match report_text(&report_dict, txt_val) {
            Ok(t) => t,
            Err(msg) => {
                rows.push((2, error_row(msg)));
                return Ok(());
            }
        };
        if matches.first().is_some_and(|f| matches!(f, JValue::Str(_))) {
            let joined = matches.iter().map(py_str).collect::<Vec<_>>().join(", ");
            match &text {
                JValue::Str(t) => text = JValue::Str(t.replace("{{matches}}", &joined)),
                other => {
                    let msg = format!(
                        "'{}' object has no attribute 'replace'",
                        py_type_name(other)
                    );
                    rows.push((2, error_row(msg)));
                    return Ok(());
                }
            }
        }
        let outcome = match state {
            "true" => Outcome::Met,
            "warn" => Outcome::Warning,
            _ => Outcome::NotMet,
        };
        let priority = match outcome {
            Outcome::Met => 0,
            Outcome::Warning => 1,
            Outcome::NotMet => 2,
        };
        if !matches.is_empty() && !fail_if && !warn_if {
            // One row per manifest, each with its signer. A non-dict along the way
            // raises inside the handler: the rows so far stay, plus an error row.
            for m in &matches {
                let subject = get_or_empty(m, "signature", empty)
                    .and_then(|s| get_or_empty(s, "certificateInfo", empty))
                    .and_then(|c| get_or_empty(c, "subject", empty));
                let subject = match subject {
                    Ok(s) => s,
                    Err(msg) => {
                        rows.push((2, error_row(msg)));
                        return Ok(());
                    }
                };
                let field = |k: &str, default: &str| match subject {
                    JValue::Object(sm) => sm.get(k).cloned().unwrap_or(JValue::Str(default.into())),
                    _ => JValue::Str(default.into()),
                };
                let JValue::Object(_) = subject else {
                    rows.push((
                        2,
                        error_row(format!(
                            "'{}' object has no attribute 'get'",
                            py_type_name(subject)
                        )),
                    ));
                    return Ok(());
                };
                let mut by = Map::new();
                by.insert("CN", field("CN", "Unknown CN"));
                by.insert("O", field("O", "Unknown O"));
                let ou = field("OU", "");
                if ou.py_truthy() && subject.get("OU").is_some() {
                    by.insert("OU", ou);
                }
                rows.push((
                    0,
                    Row {
                        trait_name: display.clone(),
                        category: category.clone(),
                        outcome: Outcome::Met,
                        error: false,
                        report_text: text.clone(),
                        asserted_by: Some(JValue::Object(by)),
                        manifest_label: Some(m.get("label").cloned().unwrap_or(JValue::Null)),
                    },
                ));
            }
        } else {
            rows.push((
                priority,
                Row {
                    trait_name: display,
                    category,
                    outcome,
                    error: false,
                    report_text: text,
                    asserted_by: None,
                    manifest_label: None,
                },
            ));
        }
        Ok(())
    }
}

fn map_category(c: &str) -> String {
    match c {
        "inception" => "inceptions".into(),
        "transformation" => "transformations".into(),
        other => other.into(),
    }
}
