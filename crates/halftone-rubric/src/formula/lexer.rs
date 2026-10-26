// Portions of this file are ported from json-formula-py
// (https://github.com/adobe/json-formula-py, commit 327a8d7),
// Copyright 2026 Adobe, licensed under the Apache License, Version 2.0.
// Modified by the Halftone authors: translated from Python to Rust.
// This file is distributed under the Apache License, Version 2.0 only;
// see this crate's NOTICE.

//! Tokenizer, ported from json-formula-py's `lexer.py` (Adobe, Apache-2.0).
//! Works on Unicode scalar values, as Python indexes strings by code point.

use super::Error;
use crate::value::JValue;

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    /// `==` (also `=`).
    Eq,
    /// `!=` (also `<>`).
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
}

/// One token.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Identifier(String),
    QuotedIdentifier(String),
    Global(String),
    Int(i128),
    Number(f64),
    Str(String),
    Json(JValueEq),
    Dot,
    Comma,
    Colon,
    LBrace,
    RBrace,
    RBracket,
    LParen,
    RParen,
    Current,
    UnaryMinus,
    LBracket,
    Filter,
    Flatten,
    Not,
    Comparator(Cmp),
    And,
    Or,
    Pipe,
    Expref,
    Concatenate,
    Union,
    Add,
    Subtract,
    Star,
    Multiply,
    Divide,
    Eof,
}

/// A JSON literal inside a token (wrapper so tokens can derive `PartialEq`).
#[derive(Debug, Clone)]
pub struct JValueEq(pub JValue);

impl PartialEq for JValueEq {
    fn eq(&self, other: &Self) -> bool {
        crate::value::strict_deep_equal(&self.0, &other.0)
    }
}

impl Tok {
    /// The token kind for the parser's binding-power table; `None` where the
    /// reference has no entry (using such a token as an operator is an error).
    pub fn binding_power(&self) -> Option<u8> {
        Some(match self {
            Tok::Eof
            | Tok::Identifier(_)
            | Tok::QuotedIdentifier(_)
            | Tok::RBracket
            | Tok::RParen
            | Tok::Comma
            | Tok::RBrace
            | Tok::Number(_)
            | Tok::Int(_)
            | Tok::Current
            | Tok::Global(_)
            | Tok::Expref => 0,
            Tok::Pipe => 1,
            Tok::Or => 2,
            Tok::And => 3,
            Tok::Comparator(_) => 4,
            Tok::Concatenate => 5,
            Tok::Add | Tok::Subtract | Tok::Union => 6,
            Tok::Multiply | Tok::Divide => 7,
            Tok::Not | Tok::UnaryMinus => 8,
            Tok::Flatten => 10,
            Tok::Star => 20,
            Tok::Filter => 21,
            Tok::Dot => 40,
            Tok::LBrace => 50,
            Tok::LBracket => 55,
            Tok::LParen => 60,
            Tok::Str(_) | Tok::Json(_) | Tok::Colon => return None,
        })
    }
}

fn is_alphanum(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_identifier_start(c: char) -> bool {
    c == '$' || c.is_alphabetic() || c == '_'
}

/// Tokenize `src`. `allowed_globals` are the `$names` read as globals; any other
/// `$name` is an ordinary identifier (a field lookup), as in the reference.
pub fn tokenize(src: &str, allowed_globals: &[String]) -> Result<Vec<Tok>, Error> {
    let s: Vec<char> = src.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let prev = out.last();
        if c == '$' && !matches!(prev, Some(Tok::Dot)) {
            let mut j = i + 1;
            while j < s.len() && (s[j] == '$' || is_alphanum(s[j])) {
                j += 1;
            }
            let name: String = s[i..j].iter().collect();
            if allowed_globals.contains(&name) {
                out.push(Tok::Global(name));
                i = j;
                continue;
            }
        }
        if is_identifier_start(c) {
            let mut j = i + 1;
            while j < s.len() && (s[j] == '$' || is_alphanum(s[j])) {
                j += 1;
            }
            out.push(Tok::Identifier(s[i..j].iter().collect()));
            i = j;
        } else if c.is_ascii_digit() || (c == '.' && s.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let (tok, len) = number(&s[i..])?;
            out.push(tok);
            i += len;
        } else if let Some(t) = basic(c) {
            out.push(t);
            i += 1;
        } else if c == '-'
            && !matches!(
                prev,
                Some(
                    Tok::Global(_)
                        | Tok::Current
                        | Tok::Number(_)
                        | Tok::Int(_)
                        | Tok::RParen
                        | Tok::Identifier(_)
                        | Tok::QuotedIdentifier(_)
                        | Tok::RBracket
                        | Tok::Json(_)
                        | Tok::Str(_)
                )
            )
        {
            out.push(Tok::UnaryMinus);
            i += 1;
        } else if c == '[' {
            match s.get(i + 1) {
                Some('?') => {
                    out.push(Tok::Filter);
                    i += 2;
                }
                Some(']') => {
                    out.push(Tok::Flatten);
                    i += 2;
                }
                _ => {
                    out.push(Tok::LBracket);
                    i += 1;
                }
            }
        } else if c == '\'' {
            let (v, len) = quoted_identifier(&s[i..])?;
            out.push(Tok::QuotedIdentifier(v));
            i += len;
        } else if c == '"' {
            let (v, len) = string_literal(&s[i..])?;
            out.push(Tok::Str(v));
            i += len;
        } else if c == '`' {
            let (v, len) = json_literal(&s[i..])?;
            out.push(Tok::Json(JValueEq(v)));
            i += len;
        } else if matches!(c, '<' | '>' | '=' | '!') {
            let next = s.get(i + 1).copied();
            let (t, len) = match (c, next) {
                ('!', Some('=')) => (Tok::Comparator(Cmp::Ne), 2),
                ('!', _) => (Tok::Not, 1),
                ('<', Some('=')) => (Tok::Comparator(Cmp::Le), 2),
                ('<', Some('>')) => (Tok::Comparator(Cmp::Ne), 2),
                ('<', _) => (Tok::Comparator(Cmp::Lt), 1),
                ('>', Some('=')) => (Tok::Comparator(Cmp::Ge), 2),
                ('>', _) => (Tok::Comparator(Cmp::Gt), 1),
                (_, Some('=')) => (Tok::Comparator(Cmp::Eq), 2),
                _ => (Tok::Comparator(Cmp::Eq), 1),
            };
            out.push(t);
            i += len;
        } else if matches!(c, ' ' | '\t' | '\n') {
            i += 1;
        } else if c == '&' {
            if s.get(i + 1) == Some(&'&') {
                out.push(Tok::And);
                i += 2;
            } else {
                let t = if matches!(prev, Some(Tok::Comma | Tok::LParen)) {
                    Tok::Expref
                } else {
                    Tok::Concatenate
                };
                out.push(t);
                i += 1;
            }
        } else if c == '|' {
            if s.get(i + 1) == Some(&'|') {
                out.push(Tok::Or);
                i += 2;
            } else {
                out.push(Tok::Pipe);
                i += 1;
            }
        } else {
            let t = match c {
                '~' => Tok::Union,
                '+' => Tok::Add,
                '-' => Tok::Subtract,
                '*' => Tok::Star,
                '/' => Tok::Divide,
                _ => return Err(Error::syntax(format!("Unknown character:{c}"))),
            };
            out.push(t);
            i += 1;
        }
    }
    Ok(out)
}

fn basic(c: char) -> Option<Tok> {
    Some(match c {
        '.' => Tok::Dot,
        ',' => Tok::Comma,
        ':' => Tok::Colon,
        '{' => Tok::LBrace,
        '}' => Tok::RBrace,
        ']' => Tok::RBracket,
        '(' => Tok::LParen,
        ')' => Tok::RParen,
        '@' => Tok::Current,
        _ => return None,
    })
}

/// `^[0-9]*\.?[0-9]+(?:[eE][-+]?[0-9]+)?`, with the regex's backtracking: `1.`
/// is the integer 1 followed by a dot.
fn number(s: &[char]) -> Result<(Tok, usize), Error> {
    let digits = |from: usize| s[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let a = digits(0);
    let mut end;
    if s.get(a) == Some(&'.') && digits(a + 1) > 0 {
        end = a + 1 + digits(a + 1);
    } else if a > 0 {
        end = a;
    } else {
        return Err(Error::syntax("Invalid number"));
    }
    if matches!(s.get(end), Some('e' | 'E')) {
        let mut j = end + 1;
        if matches!(s.get(j), Some('+' | '-')) {
            j += 1;
        }
        let d = digits(j);
        if d > 0 {
            end = j + d;
        }
    }
    let text: String = s[..end].iter().collect();
    if text.contains(['.', 'e', 'E']) {
        let f = text
            .parse::<f64>()
            .map_err(|_| Error::syntax(format!("Invalid number: {text}")))?;
        Ok((Tok::Number(f), end))
    } else {
        match text.parse::<i128>() {
            Ok(i) => Ok((Tok::Int(i), end)),
            Err(_) => Err(Error::syntax(format!("Invalid number: {text}"))),
        }
    }
}

/// Decode a body the way `json.loads('"' + body + '"')` does.
fn json_string(body: &str) -> Option<String> {
    serde_json::from_str::<String>(&format!("\"{body}\"")).ok()
}

/// `'...'`: a quoted identifier. `\\` and `\'` skip two characters; the body is
/// then JSON-decoded after unescaping `\'`.
fn quoted_identifier(s: &[char]) -> Result<(String, usize), Error> {
    let mut i = 1;
    while i < s.len() && s[i] != '\'' {
        if s[i] == '\\' && matches!(s.get(i + 1), Some('\\' | '\'')) {
            i += 2;
        } else {
            i += 1;
        }
    }
    if i >= s.len() {
        return Err(Error::syntax("Unterminated quoted identifier"));
    }
    let body: String = s[1..i].iter().collect();
    let body = body.replace("\\'", "'");
    let v = json_string(&body)
        .ok_or_else(|| Error::syntax(format!("Invalid quoted identifier: '{body}'")))?;
    Ok((v, i + 1))
}

/// `"..."`: a string literal, JSON-decoded.
fn string_literal(s: &[char]) -> Result<(String, usize), Error> {
    let mut i = 1;
    while i < s.len() && s[i] != '"' {
        if s[i] == '\\' && matches!(s.get(i + 1), Some('\\' | '"')) {
            i += 2;
        } else {
            i += 1;
        }
    }
    if i >= s.len() {
        return Err(Error::syntax("Unterminated string literal"));
    }
    let body: String = s[1..i].iter().collect();
    let v = json_string(&body)
        .ok_or_else(|| Error::syntax(format!("Invalid string literal: {body}")))?;
    Ok((v, i + 1))
}

/// `` `...` ``: a JSON literal. A backtick inside a JSON string is an error; `` \` ``
/// outside one is unescaped before parsing.
fn json_literal(s: &[char]) -> Result<(JValue, usize), Error> {
    let mut i = 1;
    let (mut in_string, mut escaped) = (false, false);
    while i < s.len() {
        let c = s[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '`' {
                return Err(Error::syntax("Unterminated JSON literal"));
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        } else if c == '`' {
            break;
        }
        i += 1;
    }
    if i >= s.len() {
        return Err(Error::syntax("Unterminated JSON literal"));
    }
    let text: String = s[1..i].iter().collect();
    let text = text.trim_start().replace("\\`", "`");
    let v = JValue::from_json_str(&text).map_err(|e| Error::syntax(e.to_string()))?;
    Ok((v, i + 1))
}
