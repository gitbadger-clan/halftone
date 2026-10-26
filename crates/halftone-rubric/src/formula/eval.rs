// Portions of this file are ported from json-formula-py
// (https://github.com/adobe/json-formula-py, commit 327a8d7),
// Copyright 2026 Adobe, licensed under the Apache License, Version 2.0.
// Modified by the Halftone authors: translated from Python to Rust.
// This file is distributed under the Apache License, Version 2.0 only;
// see this crate's NOTICE.

//! Tree interpreter, argument coercion and built-ins, ported from json-formula-py's
//! `interpreter.py`, `match_type.py`, `runtime.py` and `functions.py`.

use super::Error;
use super::lexer::Cmp;
use super::parser::Node;
use crate::value::{JValue, Map, strict_deep_equal};
use std::collections::HashMap;

/// Global variables (`$name`), including `$args` while a helper expression runs.
pub type Globals = HashMap<String, JValue>;

/// A rubric-defined expression callable as a function.
struct UserFn {
    ast: Option<Node>,
    arity: usize,
}

/// Evaluates parsed expressions. Holds the rubric's helper expressions, which are
/// callable like functions and take precedence over built-ins of the same name.
#[derive(Default)]
pub struct Engine {
    user: HashMap<String, UserFn>,
}

impl Engine {
    /// An engine with only the built-in functions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a helper expression. `ast` is `None` if it failed to compile:
    /// calling it is then an error, as in the reference.
    pub fn define(&mut self, name: &str, ast: Option<Node>, arity: usize) {
        self.user.insert(name.to_string(), UserFn { ast, arity });
    }

    /// Evaluate `ast` against `data`.
    pub fn search(
        &self,
        ast: &Node,
        data: &JValue,
        globals: &mut Globals,
    ) -> Result<JValue, Error> {
        self.visit(ast, data, globals)
    }

    fn visit(&self, node: &Node, value: &JValue, g: &mut Globals) -> Result<JValue, Error> {
        use JValue as V;
        Ok(match node {
            Node::Field(name) | Node::QuotedField(name) => field(value, name),
            Node::Chained(l, r) => {
                let left = self.visit(l, value, g)?;
                self.visit(r, &left, g)?
            }
            Node::Bracket(l, r) => {
                let left = self.visit(l, value, g)?;
                self.visit(r, &left, g)?
            }
            Node::Index(i) => match value {
                V::Array(a) => {
                    let idx = if *i < 0 { a.len() as i128 + i } else { *i };
                    usize::try_from(idx)
                        .ok()
                        .and_then(|k| a.get(k))
                        .cloned()
                        .unwrap_or(V::Null)
                }
                _ => V::Null,
            },
            Node::Slice(parts) => match value {
                V::Array(a) => slice(a, *parts)?,
                _ => V::Null,
            },
            Node::Projection { left, rhs, .. } => match self.visit(left, value, g)? {
                V::Array(items) => V::Array(self.map(rhs, &items, g)?),
                _ => V::Null,
            },
            Node::ValueProjection(l, rhs) => match self.visit(l, value, g)? {
                V::Object(m) => {
                    let items: Vec<JValue> = m.iter().map(|(_, v)| v.clone()).collect();
                    V::Array(self.map(rhs, &items, g)?)
                }
                _ => V::Null,
            },
            Node::Filter { left, rhs, cond } => match self.visit(left, value, g)? {
                V::Array(items) => {
                    let mut kept = Vec::new();
                    for it in items {
                        if to_boolean(&self.visit(cond, &it, g)?) {
                            kept.push(it);
                        }
                    }
                    V::Array(self.map(rhs, &kept, g)?)
                }
                _ => V::Null,
            },
            Node::Comparator(op, l, r) => {
                let a = self.visit(l, value, g)?;
                let b = self.visit(r, value, g)?;
                V::Bool(compare(*op, &a, &b))
            }
            Node::Flatten(x) => match self.visit(x, value, g)? {
                V::Array(items) => {
                    let mut merged = Vec::new();
                    for it in items {
                        match it {
                            V::Array(inner) => merged.extend(inner),
                            other => merged.push(other),
                        }
                    }
                    V::Array(merged)
                }
                _ => V::Null,
            },
            Node::Identity | Node::Current => value.clone(),
            Node::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.visit(it, value, g)?);
                }
                V::Array(out)
            }
            Node::Object(pairs) => {
                let mut m = Map::new();
                for (k, v) in pairs {
                    let x = self.visit(v, value, g)?;
                    m.insert(k.clone(), x);
                }
                V::Object(m)
            }
            Node::Or(l, r) => {
                let first = self.visit(l, value, g)?;
                if to_boolean(&first) {
                    first
                } else {
                    self.visit(r, value, g)?
                }
            }
            Node::And(l, r) => {
                let first = self.visit(l, value, g)?;
                if !to_boolean(&first) {
                    first
                } else {
                    self.visit(r, value, g)?
                }
            }
            Node::Not(x) => V::Bool(!to_boolean(&self.visit(x, value, g)?)),
            Node::UnaryMinus(x) => {
                let v = self.visit(x, value, g)?;
                let n = to_number(&v).map_err(|_| {
                    Error::ty(format!("Failed to convert \"{}\" to number", py_repr(&v)))
                })?;
                arith(&n, &V::Int(-1), '*')?
            }
            Node::Add(l, r) | Node::Subtract(l, r) | Node::Multiply(l, r) | Node::Divide(l, r) => {
                let op = match node {
                    Node::Add(..) => '+',
                    Node::Subtract(..) => '-',
                    Node::Multiply(..) => '*',
                    _ => '/',
                };
                let (a, b) = balance(self.visit(l, value, g)?, self.visit(r, value, g)?);
                apply_operator(&a, &b, op)?
            }
            Node::Concatenate(l, r) => {
                let (a, b) = balance(self.visit(l, value, g)?, self.visit(r, value, g)?);
                let side = |v: JValue| {
                    let t = if is_array_ty(get_type(&v)) {
                        Ty::ArrayString
                    } else {
                        Ty::String
                    };
                    match_type(&[t], v, "concatenate")
                };
                apply_operator(&side(a)?, &side(b)?, '&')?
            }
            Node::Union(l, r) => {
                let side = |v: JValue| -> Result<Vec<JValue>, Error> {
                    if v.is_null() {
                        return Ok(vec![V::Null]);
                    }
                    match match_type(&[Ty::Array], v, "union")? {
                        V::Array(a) => Ok(a),
                        other => Ok(vec![other]),
                    }
                };
                let mut a = side(self.visit(l, value, g)?)?;
                a.extend(side(self.visit(r, value, g)?)?);
                V::Array(a)
            }
            Node::Str(s) => V::Str(s.clone()),
            Node::Literal(v) => v.clone(),
            Node::Number(n) => V::Float(*n),
            Node::Integer(i) => V::Int(*i),
            Node::Pipe(l, r) => {
                let left = self.visit(l, value, g)?;
                self.visit(r, &left, g)?
            }
            Node::Global(name) => g.get(name).cloned().unwrap_or(V::Null),
            Node::Function(name, args) => {
                if name == "if" && !self.user.contains_key(name) {
                    return Err(Error::function("if() is not implemented"));
                }
                let mut resolved = Vec::with_capacity(args.len());
                for a in args {
                    resolved.push(self.visit(a, value, g)?);
                }
                self.call(name, resolved, value, g)?
            }
            Node::Expref(_) => {
                return Err(Error::eval("expression references are not implemented"));
            }
        })
    }

    fn map(&self, rhs: &Node, items: &[JValue], g: &mut Globals) -> Result<Vec<JValue>, Error> {
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            out.push(self.visit(rhs, it, g)?);
        }
        Ok(out)
    }

    fn call(
        &self,
        name: &str,
        args: Vec<JValue>,
        data: &JValue,
        g: &mut Globals,
    ) -> Result<JValue, Error> {
        if let Some(f) = self.user.get(name) {
            // Signature: `arity` arguments of any type.
            if f.arity == 0 && !args.is_empty() {
                return Err(Error::function(format!(
                    "{name}() does not accept parameters"
                )));
            }
            if f.arity > 0 && args.len() != f.arity {
                return Err(Error::function(format!(
                    "{name}() takes {} arguments but received {}",
                    f.arity,
                    args.len()
                )));
            }
            let ast = f
                .ast
                .as_ref()
                .ok_or_else(|| Error::eval(format!("Expression '{name}' failed to compile")))?;
            if f.arity == 0 {
                // `$args` is inherited from the caller, unchanged.
                return self.visit(ast, data, g);
            }
            let saved = g.remove("$args");
            g.insert("$args".into(), JValue::Array(args));
            let r = self.visit(ast, data, g);
            g.remove("$args");
            if let Some(s) = saved.filter(|s| !s.is_null()) {
                g.insert("$args".into(), s);
            }
            return r;
        }
        let sig: &[&[Ty]] = match name {
            "length" => &[&[Ty::String, Ty::Array, Ty::Object]],
            "contains" => &[&[Ty::String, Ty::Array], &[Ty::Any]],
            "startsWith" => &[
                &[Ty::String, Ty::ArrayString],
                &[Ty::String, Ty::ArrayString],
            ],
            _ => return Err(Error::function(format!("No such function: {name}()"))),
        };
        if args.len() != sig.len() {
            return Err(Error::function(format!(
                "{name}() takes {} arguments but received {}",
                sig.len(),
                args.len()
            )));
        }
        let mut a = Vec::with_capacity(args.len());
        for (v, types) in args.into_iter().zip(sig) {
            a.push(match_type(types, v, name)?);
        }
        match name {
            "length" => Ok(JValue::Int(match &a[0] {
                JValue::Str(s) => s.chars().count() as i128,
                JValue::Array(x) => x.len() as i128,
                JValue::Object(m) => m.len() as i128,
                _ => 0,
            })),
            "contains" => match &a[0] {
                JValue::Array(items) => Ok(JValue::Bool(
                    items.iter().any(|it| strict_deep_equal(it, &a[1])),
                )),
                subject => {
                    if get_type(&a[1]) != Ty::String {
                        return Err(Error::ty(
                            "contains() requires a string search value for string subjects",
                        ));
                    }
                    Ok(JValue::Bool(
                        to_string(subject)?.contains(to_string(&a[1])?.as_str()),
                    ))
                }
            },
            _ => broadcast(&a, &|row: &[JValue]| {
                Ok(JValue::Bool(
                    to_string(&row[0])?.starts_with(to_string(&row[1])?.as_str()),
                ))
            }),
        }
    }
}

/// `_evaluate`: apply `f` element-wise when any argument is an array.
fn broadcast(
    args: &[JValue],
    f: &dyn Fn(&[JValue]) -> Result<JValue, Error>,
) -> Result<JValue, Error> {
    if !args.iter().any(|a| matches!(a, JValue::Array(_))) {
        return f(args);
    }
    let max = args
        .iter()
        .map(|a| if let JValue::Array(x) = a { x.len() } else { 0 })
        .max()
        .unwrap_or(0);
    let mut rows = Vec::with_capacity(max);
    for r in 0..max {
        let row: Vec<JValue> = args
            .iter()
            .map(|a| match a {
                JValue::Array(x) => x.get(r).cloned().unwrap_or(JValue::Null),
                other => other.clone(),
            })
            .collect();
        rows.push(broadcast(&row, f)?);
    }
    Ok(JValue::Array(rows))
}

/// A field of an object, or (as Python's `get_property` does) an element of an array
/// addressed by an all-digit name. Anything else is `null`.
fn field(value: &JValue, name: &str) -> JValue {
    match value {
        JValue::Object(m) => m.get(name).cloned().unwrap_or(JValue::Null),
        JValue::Array(a) if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) => name
            .parse::<usize>()
            .ok()
            .and_then(|i| a.get(i))
            .cloned()
            .unwrap_or(JValue::Null),
        _ => JValue::Null,
    }
}

/// json-formula truthiness: `null`, `false`, 0, `""`, `[]`, `{}` are false.
pub(crate) fn to_boolean(v: &JValue) -> bool {
    v.py_truthy()
}

fn compare(op: Cmp, a: &JValue, b: &JValue) -> bool {
    match op {
        Cmp::Eq => return strict_deep_equal(a, b),
        Cmp::Ne => return !strict_deep_equal(a, b),
        _ => {}
    }
    let container = |v: &JValue| matches!(v, JValue::Array(_) | JValue::Object(_));
    if container(a) || container(b) {
        return false;
    }
    let ord = if let (JValue::Str(x), JValue::Str(y)) = (a, b) {
        x.partial_cmp(y)
    } else {
        match (to_number(a), to_number(b)) {
            (Ok(JValue::Int(x)), Ok(JValue::Int(y))) => x.partial_cmp(&y),
            (Ok(x), Ok(y)) => as_f64(&x).partial_cmp(&as_f64(&y)),
            _ => return false,
        }
    };
    use std::cmp::Ordering::*;
    match (op, ord) {
        (_, None) => false,
        (Cmp::Lt, Some(o)) => o == Less,
        (Cmp::Le, Some(o)) => o != Greater,
        (Cmp::Gt, Some(o)) => o == Greater,
        (Cmp::Ge, Some(o)) => o != Less,
        _ => false,
    }
}

fn as_f64(v: &JValue) -> f64 {
    match v {
        JValue::Int(i) => *i as f64,
        JValue::Float(f) => *f,
        _ => f64::NAN,
    }
}

fn slice(a: &[JValue], parts: [Option<i128>; 3]) -> Result<JValue, Error> {
    let len = a.len() as i128;
    let step = parts[2].unwrap_or(1);
    if step == 0 {
        return Err(Error::eval("Invalid slice, step cannot be 0"));
    }
    let neg = step < 0;
    let cap = |actual: i128| -> i128 {
        if actual < 0 {
            let x = actual + len;
            if x < 0 { if neg { -1 } else { 0 } } else { x }
        } else if actual >= len {
            if neg { len - 1 } else { len }
        } else {
            actual
        }
    };
    let start = parts[0].map_or(if neg { len - 1 } else { 0 }, cap);
    let stop = parts[1].map_or(if neg { -1 } else { len }, cap);
    let mut out = Vec::new();
    let mut i = start;
    while (step > 0 && i < stop) || (step < 0 && i > stop) {
        if let Some(v) = usize::try_from(i).ok().and_then(|k| a.get(k)) {
            out.push(v.clone());
        }
        i += step;
    }
    Ok(JValue::Array(out))
}

fn balance(a: JValue, b: JValue) -> (JValue, JValue) {
    match (a, b) {
        (JValue::Array(mut x), JValue::Array(mut y)) => {
            let n = x.len().max(y.len());
            x.resize(n, JValue::Null);
            y.resize(n, JValue::Null);
            (JValue::Array(x), JValue::Array(y))
        }
        pair => pair,
    }
}

fn apply_operator(a: &JValue, b: &JValue, op: char) -> Result<JValue, Error> {
    match (a, b) {
        (JValue::Array(x), JValue::Array(y)) => Ok(JValue::Array(
            x.iter()
                .zip(y)
                .map(|(p, q)| apply_operator(p, q, op))
                .collect::<Result<_, _>>()?,
        )),
        (JValue::Array(x), _) => Ok(JValue::Array(
            x.iter()
                .map(|p| apply_operator(p, b, op))
                .collect::<Result<_, _>>()?,
        )),
        (_, JValue::Array(y)) => Ok(JValue::Array(
            y.iter()
                .map(|q| apply_operator(a, q, op))
                .collect::<Result<_, _>>()?,
        )),
        _ if op == '&' => Ok(JValue::Str(to_string(a)? + &to_string(b)?)),
        _ => arith(&to_number(a)?, &to_number(b)?, op),
    }
}

/// Python arithmetic on int/float: int op int stays int (except `/`).
fn arith(a: &JValue, b: &JValue, op: char) -> Result<JValue, Error> {
    if let (JValue::Int(x), JValue::Int(y)) = (a, b) {
        let r = match op {
            '+' => x.checked_add(*y),
            '-' => x.checked_sub(*y),
            '*' => x.checked_mul(*y),
            _ => None,
        };
        if let Some(r) = r {
            return Ok(JValue::Int(r));
        }
    }
    let (x, y) = (as_f64(a), as_f64(b));
    let r = match op {
        '+' => x + y,
        '-' => x - y,
        '*' => x * y,
        _ => {
            if y == 0.0 {
                return Err(Error::eval("Division by zero"));
            }
            let r = x / y;
            if !r.is_finite() {
                return Err(Error::eval("Division by zero"));
            }
            r
        }
    };
    Ok(JValue::Float(r))
}

/// json-formula's `toNumber` with the strict string conversion the engine uses.
fn to_number(v: &JValue) -> Result<JValue, Error> {
    match v {
        JValue::Null => Ok(JValue::Float(0.0)),
        JValue::Array(_) => Err(Error::ty("Failed to convert array to number")),
        JValue::Int(_) | JValue::Float(_) => Ok(v.clone()),
        JValue::Str(s) => strict_string_to_number(s).map(JValue::Float),
        JValue::Bool(b) => Ok(JValue::Float(if *b { 1.0 } else { 0.0 })),
        JValue::Object(_) => Err(Error::ty("Failed to convert object to number")),
    }
}

/// `strict_string_to_number`: the regex `^\s*[-+]?\d*(\.\d+)?(e[+-]?\d+)?\s*$`,
/// then Python's `float()` (which rejects what the regex lets through, like "" or "e5").
fn strict_string_to_number(s: &str) -> Result<f64, Error> {
    let fail = || Error::ty(format!("Failed to convert \"{s}\" to number"));
    let t = s.trim();
    let c: Vec<char> = t.chars().collect();
    let mut i = 0;
    if matches!(c.first(), Some('+' | '-')) {
        i += 1;
    }
    while c.get(i).is_some_and(char::is_ascii_digit) {
        i += 1;
    }
    if c.get(i) == Some(&'.') {
        let j = i + 1;
        let mut k = j;
        while c.get(k).is_some_and(char::is_ascii_digit) {
            k += 1;
        }
        if k == j {
            return Err(fail());
        }
        i = k;
    }
    if matches!(c.get(i), Some('e' | 'E')) {
        let mut k = i + 1;
        if matches!(c.get(k), Some('+' | '-')) {
            k += 1;
        }
        let d = k;
        while c.get(k).is_some_and(char::is_ascii_digit) {
            k += 1;
        }
        if k == d {
            return Err(fail());
        }
        i = k;
    }
    if i != c.len() {
        return Err(fail());
    }
    let f: f64 = t.parse().map_err(|_| fail())?;
    if f.is_nan() {
        return Err(fail());
    }
    Ok(f)
}

/// json-formula's `toString`: Python `str()` for numbers, `"true"`/`"false"`,
/// `""` for null; arrays and objects are errors.
pub(crate) fn to_string(v: &JValue) -> Result<String, Error> {
    match v {
        JValue::Null => Ok(String::new()),
        JValue::Array(_) => Err(Error::ty("Failed to convert array to string")),
        JValue::Object(_) => Err(Error::ty("Failed to convert object to string")),
        JValue::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        JValue::Int(i) => Ok(i.to_string()),
        JValue::Float(f) => Ok(py_float_repr(*f)),
        JValue::Str(s) => Ok(s.clone()),
    }
}

fn py_repr(v: &JValue) -> String {
    to_string(v).unwrap_or_else(|_| "<container>".into())
}

/// Python's `repr(float)`: shortest round-trip digits; fixed notation for decimal
/// exponents -4..16, otherwise `d.ddde+XX`.
pub(crate) fn py_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.into();
    }
    let sci = format!("{:e}", f.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let sign = if f.is_sign_negative() { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let body = if exp >= 0 {
            let e = exp as usize;
            let int = if digits.len() > e {
                digits[..=e].to_string()
            } else {
                format!("{digits:0<width$}", width = e + 1)
            };
            let frac = if digits.len() > e + 1 {
                &digits[e + 1..]
            } else {
                "0"
            };
            format!("{int}.{frac}")
        } else {
            format!("0.{}{digits}", "0".repeat((-exp - 1) as usize))
        };
        format!("{sign}{body}")
    } else {
        let m = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits.clone()
        };
        let es = if exp < 0 { '-' } else { '+' };
        format!("{sign}{m}e{es}{:02}", exp.abs())
    }
}

/// json-formula's type tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    Number,
    Any,
    String,
    Array,
    Object,
    Boolean,
    Null,
    ArrayNumber,
    ArrayString,
    ArrayArray,
    EmptyArray,
}

fn is_array_ty(t: Ty) -> bool {
    matches!(
        t,
        Ty::Array | Ty::ArrayNumber | Ty::ArrayString | Ty::ArrayArray | Ty::EmptyArray
    )
}

fn flatten_all(items: &[JValue], out: &mut Vec<JValue>) {
    for it in items {
        match it {
            JValue::Array(inner) => flatten_all(inner, out),
            other => out.push(other.clone()),
        }
    }
}

/// `get_type`, including the array subtypes (decided on the fully flattened
/// contents; an array whose flattening is empty counts as an array of numbers).
pub(crate) fn get_type(v: &JValue) -> Ty {
    match v {
        JValue::Null => Ty::Null,
        JValue::Bool(_) => Ty::Boolean,
        JValue::Int(_) | JValue::Float(_) => Ty::Number,
        JValue::Str(_) => Ty::String,
        JValue::Object(_) => Ty::Object,
        JValue::Array(items) => {
            if items.is_empty() {
                return Ty::EmptyArray;
            }
            let mut flat = Vec::new();
            flatten_all(items, &mut flat);
            if flat.iter().all(|x| get_type(x) == Ty::Number) {
                Ty::ArrayNumber
            } else if flat.iter().all(|x| get_type(x) == Ty::String) {
                Ty::ArrayString
            } else if items.iter().all(|x| is_array_ty(get_type(x))) {
                Ty::ArrayArray
            } else {
                Ty::Array
            }
        }
    }
}

fn supported_conversion(from: Ty, to: Ty) -> bool {
    use Ty::*;
    let targets: &[Ty] = match from {
        Number => &[String, Array, ArrayNumber, Boolean],
        Boolean => &[String, Number, Array],
        Array => &[Boolean, ArrayString, ArrayNumber],
        ArrayNumber => &[Boolean, ArrayString, Array],
        ArrayString => &[Boolean, ArrayNumber, Array],
        ArrayArray | EmptyArray | Object => &[Boolean],
        Null => &[String, Number, Boolean],
        String => &[Number, ArrayString, Array, Boolean],
        Any => &[],
    };
    targets.contains(&to)
}

/// `match_type`: accept the argument if its type is expected, otherwise apply the
/// single supported conversion; none, or more than one, is a type error.
pub(crate) fn match_type(expected: &[Ty], v: JValue, ctx: &str) -> Result<JValue, Error> {
    let actual = get_type(&v);
    let matches = |e: Ty| {
        e == actual
            || e == Ty::Any
            || (e == Ty::Array && is_array_ty(actual))
            || (is_array_ty(e) && actual == Ty::EmptyArray)
    };
    if expected.iter().any(|e| matches(*e)) {
        return Ok(v);
    }
    let filtered: Vec<Ty> = expected
        .iter()
        .copied()
        .filter(|e| supported_conversion(actual, *e))
        .collect();
    if filtered.len() != 1 {
        return Err(Error::ty(format!(
            "{ctx} cannot process type {actual:?}; expected one of {expected:?}"
        )));
    }
    let target = filtered[0];
    match (&v, target) {
        (JValue::Array(items), Ty::Boolean) => Ok(JValue::Bool(!items.is_empty())),
        (JValue::Array(items), Ty::ArrayString) => Ok(JValue::Array(
            items
                .iter()
                .map(|it| match it {
                    JValue::Array(inner) => inner
                        .iter()
                        .map(|x| to_string(x).map(JValue::Str))
                        .collect::<Result<Vec<_>, _>>()
                        .map(JValue::Array),
                    x => to_string(x).map(JValue::Str),
                })
                .collect::<Result<_, _>>()?,
        )),
        (JValue::Array(items), Ty::ArrayNumber) => Ok(JValue::Array(
            items
                .iter()
                .map(|it| match it {
                    JValue::Array(inner) => inner
                        .iter()
                        .map(to_number)
                        .collect::<Result<Vec<_>, _>>()
                        .map(JValue::Array),
                    x => to_number(x),
                })
                .collect::<Result<_, _>>()?,
        )),
        (JValue::Array(items), Ty::ArrayArray) => Ok(JValue::Array(
            items
                .iter()
                .map(|it| {
                    if let JValue::Array(_) = it {
                        it.clone()
                    } else {
                        JValue::Array(vec![it.clone()])
                    }
                })
                .collect(),
        )),
        (JValue::Object(m), Ty::Boolean) => Ok(JValue::Bool(!m.is_empty())),
        (JValue::Array(_) | JValue::Object(_), _) => Err(Error::ty(format!(
            "{ctx} expected argument to be type {target:?} but received type {actual:?}"
        ))),
        (_, Ty::ArrayString) => Ok(JValue::Array(vec![JValue::Str(to_string(&v)?)])),
        (_, Ty::ArrayNumber) => Ok(JValue::Array(vec![to_number(&v)?])),
        (_, Ty::Array) => Ok(JValue::Array(vec![v])),
        (_, Ty::Number) => to_number(&v),
        (_, Ty::String) => Ok(JValue::Str(to_string(&v)?)),
        (_, Ty::Boolean) => Ok(JValue::Bool(v.py_truthy())),
        _ => Err(Error::ty(format!(
            "{ctx} expected argument to be type {target:?} but received type {actual:?}"
        ))),
    }
}
