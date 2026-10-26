//! Loading a C2PA asset rubric (YAML) into an evaluator, as the reference does in
//! `create_json_formula_engine` (c2pa-org/conformance-public, CC BY 4.0): rubric
//! variables become globals, named expressions become functions whose arity is
//! inferred from the text, and each statement is an expression with an id.

use crate::formula::{Engine, Error, Globals, Node, parse};
use crate::value::{JValue, Map};
use yaml_rust2::{Yaml, YamlLoader};

/// One rubric statement.
#[derive(Debug)]
pub struct Statement {
    /// The trait id, e.g. `inception:signal_capturedMedia`.
    pub id: JValue,
    /// The statement as written.
    pub raw: Map,
    /// The parsed expression, or why it does not parse (the statement then never
    /// fires, as in the reference).
    pub(crate) ast: Option<Result<Node, Error>>,
}

/// A loaded rubric, ready to evaluate.
pub struct Rubric {
    /// `rubric_metadata.name`.
    pub name: JValue,
    /// `rubric_metadata.version`.
    pub version: JValue,
    /// The statements, in rubric order.
    pub statements: Vec<Statement>,
    pub(crate) engine: Engine,
    pub(crate) globals: Globals,
}

/// Python's `str()` of a value read from YAML (strings are themselves).
fn py_str(v: &JValue) -> String {
    match v {
        JValue::Str(s) => s.clone(),
        JValue::Null => "None".into(),
        JValue::Bool(b) => if *b { "True" } else { "False" }.into(),
        other => crate::formula::eval_to_string(other).unwrap_or_default(),
    }
}

impl Rubric {
    /// Parse rubric YAML. Errors only for YAML that does not load at all; problems
    /// inside expressions surface as statements that never fire.
    pub fn from_yaml(text: &str) -> Result<Rubric, Error> {
        let docs = YamlLoader::load_from_str(text)
            .map_err(|e| Error::syntax(format!("rubric YAML: {e}")))?;
        let docs: Vec<JValue> = docs.iter().map(yaml_to_json).collect::<Result<_, _>>()?;
        let metadata = match docs.first() {
            Some(JValue::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        let section = |key: &str| match metadata.get(key) {
            Some(JValue::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        let variables = section("variables");
        let expressions = section("expressions");
        let rm = section("rubric_metadata");
        let name = rm
            .get("name")
            .cloned()
            .unwrap_or(JValue::Str("Unknown".into()));
        let version = rm
            .get("version")
            .cloned()
            .unwrap_or(JValue::Str("Unknown".into()));

        let bodies: Vec<(String, String)> = expressions
            .iter()
            .map(|(k, v)| (k.clone(), py_str(v)))
            .collect();
        let arities = infer_arities(&bodies);

        // Helpers compile with every `$name` any body mentions (and the variables)
        // allowed as globals; statements with the variables only.
        let mut helper_globals: Vec<String> = variables.iter().map(|(k, _)| k.clone()).collect();
        for (_, b) in &bodies {
            for d in dollar_names(b) {
                if !helper_globals.contains(&d) {
                    helper_globals.push(d);
                }
            }
        }
        let mut allowed = helper_globals.clone();
        allowed.extend(bodies.iter().map(|(k, _)| k.clone()));
        let mut engine = Engine::new();
        for (k, b) in &bodies {
            let ast = parse(b.trim(), &allowed).ok();
            engine.define(
                k,
                ast,
                arities.iter().find(|(n, _)| n == k).map_or(0, |(_, a)| *a),
            );
        }

        let statement_globals: Vec<String> = variables.iter().map(|(k, _)| k.clone()).collect();
        let mut statements = Vec::new();
        for doc in docs.iter().skip(1) {
            let items: Vec<&Map> = match doc {
                JValue::Array(list) => list
                    .iter()
                    .filter_map(|x| {
                        if let JValue::Object(m) = x {
                            Some(m)
                        } else {
                            None
                        }
                    })
                    .collect(),
                JValue::Object(m) if !m.contains_key("block") => vec![m],
                _ => Vec::new(),
            };
            for m in items {
                let expr = m.get("expression").cloned().unwrap_or_default();
                let id = m.get("id").cloned().unwrap_or_default();
                let ast = if expr.py_truthy() && id.py_truthy() {
                    Some(match &expr {
                        JValue::Str(s) => parse(s.trim(), &statement_globals),
                        _ => Err(Error::eval("expression is not a string")),
                    })
                } else {
                    None
                };
                statements.push(Statement {
                    id,
                    raw: m.clone(),
                    ast,
                });
            }
        }
        let mut globals = Globals::new();
        for (k, v) in variables.iter() {
            globals.insert(k.clone(), v.clone());
        }
        Ok(Rubric {
            name,
            version,
            statements,
            engine,
            globals,
        })
    }

    /// Evaluate statement `s` against one manifest: `Ok(Some(value))` if it ran,
    /// `Ok(None)` if the statement is skipped (no expression or id), `Err` if it
    /// failed (the reference then reports no signal).
    pub(crate) fn run(&self, s: &Statement, manifest: &JValue) -> Result<Option<JValue>, Error> {
        let Some(ast) = &s.ast else { return Ok(None) };
        let ast = ast.as_ref().map_err(Clone::clone)?;
        let mut g = self.globals.clone();
        self.engine.search(ast, manifest, &mut g).map(Some)
    }
}

/// `_infer_arities`: the most arguments any helper body passes to each helper, or
/// N+1 if its own body reads `$args[N]` (1 for a bare `$args`). Commas inside
/// strings count, exactly as the reference's text scan counts them.
pub(crate) fn infer_arities(bodies: &[(String, String)]) -> Vec<(String, usize)> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    for (name, _) in bodies {
        let mut arity = 0;
        for (_, body) in bodies {
            let chars: Vec<char> = body.chars().collect();
            let n: Vec<char> = name.chars().collect();
            let mut i = 0;
            while i + n.len() <= chars.len() {
                // `\b`: a word character on exactly one side of the position.
                let before = i > 0 && word(chars[i - 1]);
                let boundary = n.first().is_some_and(|c| word(*c) != before);
                if chars[i..i + n.len()] == n[..] && boundary {
                    let mut j = i + n.len();
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if chars.get(j) == Some(&'(') {
                        let rest = &chars[j + 1..];
                        let count = if rest.first() == Some(&')') {
                            0
                        } else {
                            let (mut depth, mut count) = (1, 1);
                            for &c in rest {
                                match c {
                                    '(' => depth += 1,
                                    ')' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            break;
                                        }
                                    }
                                    ',' if depth == 1 => count += 1,
                                    _ => {}
                                }
                            }
                            count
                        };
                        arity = arity.max(count);
                    }
                }
                i += 1;
            }
        }
        out.push((name.clone(), arity));
    }
    for (name, body) in bodies {
        let mut max_idx: Option<usize> = None;
        let mut rest = body.as_str();
        while let Some(p) = rest.find("$args[") {
            let after = &rest[p + 6..];
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() && after[digits.len()..].starts_with(']') {
                if let Ok(d) = digits.parse::<usize>() {
                    max_idx = Some(max_idx.map_or(d, |m| m.max(d)));
                }
            }
            rest = after;
        }
        let bare = {
            let mut found = false;
            let mut rest = body.as_str();
            while let Some(p) = rest.find("$args") {
                let after = &rest[p + 5..];
                if after.chars().next().is_none_or(|c| !word(c)) {
                    found = true;
                    break;
                }
                rest = after;
            }
            found
        };
        let slot = out.iter_mut().find(|(n, _)| n == name).map(|(_, a)| a);
        if let Some(a) = slot {
            if let Some(m) = max_idx {
                *a = (*a).max(m + 1);
            } else if bare {
                *a = (*a).max(1);
            }
        }
    }
    out
}

/// Every `$[a-zA-Z0-9_]+` in a body.
fn dollar_names(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j > i + 1 {
                out.push(body[i..j].to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Every document in a YAML text, converted as [`Rubric::from_yaml`] converts them.
/// Exposed so tests can compare the result with PyYAML's reading of the same file.
pub fn yaml_documents(text: &str) -> Result<Vec<JValue>, Error> {
    YamlLoader::load_from_str(text)
        .map_err(|e| Error::syntax(format!("rubric YAML: {e}")))?
        .iter()
        .map(yaml_to_json)
        .collect()
}

/// YAML (1.2, yaml-rust2) to the value PyYAML's `safe_load` produces for the same
/// text. The rubric fixtures check the two agree on the vendored rubric.
fn yaml_to_json(y: &Yaml) -> Result<JValue, Error> {
    Ok(match y {
        Yaml::Null => JValue::Null,
        Yaml::Boolean(b) => JValue::Bool(*b),
        Yaml::Integer(i) => JValue::Int(i128::from(*i)),
        Yaml::Real(s) => JValue::Float(
            y.as_f64()
                .ok_or_else(|| Error::syntax(format!("rubric YAML: bad number {s}")))?,
        ),
        Yaml::String(s) => JValue::Str(s.clone()),
        Yaml::Array(a) => JValue::Array(a.iter().map(yaml_to_json).collect::<Result<_, _>>()?),
        Yaml::Hash(h) => {
            let mut m = Map::new();
            for (k, v) in h {
                let key = match k {
                    Yaml::String(s) => s.clone(),
                    Yaml::Integer(i) => i.to_string(),
                    Yaml::Boolean(b) => b.to_string(),
                    Yaml::Real(s) => s.clone(),
                    Yaml::Null => "null".into(),
                    _ => return Err(Error::syntax("rubric YAML: unsupported mapping key")),
                };
                m.insert(key, yaml_to_json(v)?);
            }
            JValue::Object(m)
        }
        Yaml::Alias(_) | Yaml::BadValue => {
            return Err(Error::syntax("rubric YAML: aliases are not supported"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arities_match_the_reference_scan() {
        let b = |s: &[(&str, &str)]| {
            s.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect::<Vec<_>>()
        };
        let bodies = b(&[
            (
                "_manifest_signalActions",
                "$args[0].assertions.'c2pa.actions.v2'.actions",
            ),
            (
                "_manifest_createdWithDst",
                "length(_manifest_signalActions($args[0])[? digitalSourceType == $args[1]]) > 0",
            ),
            ("_signalActions", "_manifest_signalActions(@)"),
            (
                "_createdWithDst",
                "_manifest_createdWithDst($args[0], $args[1])",
            ),
            ("_bare", "$args"),
            ("_commaInString", "_signalActions(\"a,b\")"),
        ]);
        let a = infer_arities(&bodies);
        let get = |n: &str| a.iter().find(|(x, _)| x == n).unwrap().1;
        assert_eq!(get("_manifest_signalActions"), 1);
        assert_eq!(get("_manifest_createdWithDst"), 2);
        assert_eq!(
            get("_signalActions"),
            2,
            "the comma inside the string literal counts, as in the reference"
        );
        assert_eq!(get("_createdWithDst"), 2);
        assert_eq!(get("_bare"), 1);
    }
}
