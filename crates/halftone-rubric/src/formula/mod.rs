// Portions of this file are ported from json-formula-py
// (https://github.com/adobe/json-formula-py, commit 327a8d7),
// Copyright 2026 Adobe, licensed under the Apache License, Version 2.0.
// Modified by the Halftone authors: translated from Python to Rust.
// This file is distributed under the Apache License, Version 2.0 only;
// see this crate's NOTICE.

//! A json-formula interpreter: the subset of Adobe's json-formula language the C2PA
//! asset rubrics use, with the exact semantics of the reference implementation the
//! C2PA's evaluators run on (json-formula-py, Apache-2.0): the same grammar, the same
//! argument coercions, the same lenient handling of `null`. Where Python and Rust
//! differ (float formatting, string comparison), the Python behaviour is reproduced.
//!
//! Implemented built-ins: `length`, `contains`, `startsWith`. Calling any other
//! function is an error, so a rubric that needs more fails loudly rather than
//! silently disagreeing with the reference.

mod eval;
mod lexer;
mod parser;

pub use eval::{Engine, Globals};
pub use parser::{Node, parse};

/// json-formula's `toString` (Python `str()` for numbers).
pub fn eval_to_string(v: &crate::value::JValue) -> Result<String, Error> {
    eval::to_string(v)
}

/// Why an expression failed. Kinds mirror json-formula's error classes; the
/// rubric evaluators treat every kind the same (the rule reports no signal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The class of error.
    pub kind: ErrorKind,
    /// What went wrong.
    pub message: String,
}

/// json-formula's error classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The expression does not parse.
    Syntax,
    /// An argument has a type the function cannot take.
    Type,
    /// Unknown function or wrong number of arguments.
    Function,
    /// Anything else at run time.
    Evaluation,
}

impl Error {
    pub(crate) fn syntax(m: impl Into<String>) -> Self {
        Error {
            kind: ErrorKind::Syntax,
            message: m.into(),
        }
    }
    pub(crate) fn ty(m: impl Into<String>) -> Self {
        Error {
            kind: ErrorKind::Type,
            message: m.into(),
        }
    }
    pub(crate) fn function(m: impl Into<String>) -> Self {
        Error {
            kind: ErrorKind::Function,
            message: m.into(),
        }
    }
    pub(crate) fn eval(m: impl Into<String>) -> Self {
        Error {
            kind: ErrorKind::Evaluation,
            message: m.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} error: {}", self.kind, self.message)
    }
}

impl std::error::Error for Error {}
