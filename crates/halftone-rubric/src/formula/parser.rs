// Portions of this file are ported from json-formula-py
// (https://github.com/adobe/json-formula-py, commit 327a8d7),
// Copyright 2026 Adobe, licensed under the Apache License, Version 2.0.
// Modified by the Halftone authors: translated from Python to Rust.
// This file is distributed under the Apache License, Version 2.0 only;
// see this crate's NOTICE.

//! Pratt parser, ported from json-formula-py's `parser.py` (Adobe, Apache-2.0).

use super::Error;
use super::lexer::{Cmp, Tok, tokenize};
use crate::value::JValue;

/// A parsed expression.
#[derive(Debug, Clone)]
pub enum Node {
    /// `"text"`.
    Str(String),
    /// `` `json` ``.
    Literal(JValue),
    /// A number with a fraction or exponent.
    Number(f64),
    /// An integer.
    Integer(i128),
    /// `name`: a field of the current value (and callable as a function).
    Field(String),
    /// `'quoted name'`: a field of the current value (never a function name).
    QuotedField(String),
    /// `!x`.
    Not(Box<Node>),
    /// `-x`.
    UnaryMinus(Box<Node>),
    /// `x.*` / `*`: the values of an object, each through `rhs`.
    ValueProjection(Box<Node>, Box<Node>),
    /// `x[*]`, `x[]`, slices: each element of an array through `rhs`.
    Projection {
        /// The array being projected.
        left: Box<Node>,
        /// Applied to each element.
        rhs: Box<Node>,
        /// Written as `[*]` (the reference only uses this for debug messages).
        wildcard: bool,
    },
    /// `x[]` before projection: one level of arrays merged.
    Flatten(Box<Node>),
    /// `x[? cond]`: the elements for which `cond` is truthy, each through `rhs`.
    Filter {
        /// The array being filtered.
        left: Box<Node>,
        /// Applied to each kept element.
        rhs: Box<Node>,
        /// Evaluated against each element; truthy keeps it.
        cond: Box<Node>,
    },
    /// `x[i]` / `x[a:b]`.
    Bracket(Box<Node>, Box<Node>),
    /// An index.
    Index(i128),
    /// A slice `[start:stop:step]`.
    Slice([Option<i128>; 3]),
    /// `[a, b]`.
    Array(Vec<Node>),
    /// `{k: v}`.
    Object(Vec<(String, Node)>),
    /// `@`.
    Current,
    /// `$name`.
    Global(String),
    /// `&expr`.
    Expref(Box<Node>),
    /// `a.b`.
    Chained(Box<Node>, Box<Node>),
    /// `a | b`.
    Pipe(Box<Node>, Box<Node>),
    /// `a || b`.
    Or(Box<Node>, Box<Node>),
    /// `a && b`.
    And(Box<Node>, Box<Node>),
    /// `a + b`.
    Add(Box<Node>, Box<Node>),
    /// `a - b`.
    Subtract(Box<Node>, Box<Node>),
    /// `a * b`.
    Multiply(Box<Node>, Box<Node>),
    /// `a / b`.
    Divide(Box<Node>, Box<Node>),
    /// `a ~ b`.
    Union(Box<Node>, Box<Node>),
    /// `a & b`.
    Concatenate(Box<Node>, Box<Node>),
    /// `name(args)`.
    Function(String, Vec<Node>),
    /// `a == b` and friends.
    Comparator(Cmp, Box<Node>, Box<Node>),
    /// The current value, unchanged.
    Identity,
}

/// What came before a token, for the reference's `*`-versus-multiply rule.
#[derive(Clone, Copy, PartialEq)]
enum Prev {
    None,
    /// An already-parsed expression; `true` if it is a comparison.
    Node(bool),
    /// A literal star token.
    Star,
}

/// Parse one expression.
pub fn parse(src: &str, allowed_globals: &[String]) -> Result<Node, Error> {
    let mut tokens = tokenize(src, allowed_globals)?;
    tokens.push(Tok::Eof);
    let mut p = Parser { tokens, index: 0 };
    let ast = p.expression(0)?;
    if p.peek(0) != &Tok::Eof {
        return Err(Error::syntax(format!("Unexpected token: {:?}", p.peek(0))));
    }
    Ok(ast)
}

struct Parser {
    tokens: Vec<Tok>,
    index: usize,
}

fn bx(n: Node) -> Box<Node> {
    Box::new(n)
}

impl Parser {
    fn peek(&self, n: usize) -> &Tok {
        self.tokens.get(self.index + n).unwrap_or(&Tok::Eof)
    }

    /// `_lookahead_token`: a star becomes multiplication unless what precedes it
    /// is nothing or a comparison (the reference compares the previous AST node's
    /// type with token type names; `Comparator` is the only name both share).
    fn token(&self, prev: Prev) -> Tok {
        let t = self.peek(0).clone();
        if t == Tok::Star && !matches!(prev, Prev::None | Prev::Node(true)) {
            return Tok::Multiply;
        }
        t
    }

    fn advance(&mut self) {
        self.index += 1;
    }

    fn bp(t: &Tok) -> Result<u8, Error> {
        t.binding_power()
            .ok_or_else(|| Error::syntax(format!("Unexpected token: {t:?}")))
    }

    fn expression(&mut self, rbp: u8) -> Result<Node, Error> {
        let left_token = self.token(Prev::None);
        self.advance();
        let mut left = self.nud(left_token)?;
        loop {
            let current = self.token(Prev::Node(matches!(left, Node::Comparator(..))));
            if rbp >= Self::bp(&current)? {
                break;
            }
            self.advance();
            left = self.led(current, left)?;
        }
        Ok(left)
    }

    fn look_ahead_index(&self) -> bool {
        let mut i = 0;
        if self.peek(i) == &Tok::UnaryMinus {
            i += 1;
        }
        if matches!(self.peek(i), Tok::Int(_)) {
            i += 1;
        }
        matches!(self.peek(i), Tok::RBracket | Tok::Colon)
    }

    fn nud(&mut self, t: Tok) -> Result<Node, Error> {
        Ok(match t {
            Tok::Str(s) => Node::Str(s),
            Tok::Json(v) => Node::Literal(v.0),
            Tok::Number(n) => Node::Number(n),
            Tok::Int(i) => Node::Integer(i),
            Tok::Identifier(n) => Node::Field(n),
            Tok::QuotedIdentifier(n) => Node::QuotedField(n),
            Tok::Not => Node::Not(bx(self.expression(8)?)),
            Tok::UnaryMinus => Node::UnaryMinus(bx(self.expression(8)?)),
            Tok::Star => {
                let rhs = if self.peek(0) == &Tok::RBracket {
                    Node::Identity
                } else {
                    self.projection_rhs(20)?
                };
                Node::ValueProjection(bx(Node::Identity), bx(rhs))
            }
            Tok::Filter => self.led(Tok::Filter, Node::Identity)?,
            Tok::LBrace => self.object_expression()?,
            Tok::Flatten => {
                let rhs = self.projection_rhs(10)?;
                Node::Projection {
                    left: bx(Node::Flatten(bx(Node::Identity))),
                    rhs: bx(rhs),
                    wildcard: false,
                }
            }
            Tok::LBracket => {
                if self.look_ahead_index() {
                    let right = self.index_expression()?;
                    self.project_if_slice(Node::Identity, right)?
                } else if self.peek(0) == &Tok::Star && self.peek(1) == &Tok::RBracket {
                    self.advance();
                    self.advance();
                    let rhs = self.projection_rhs(20)?;
                    Node::Projection {
                        left: bx(Node::Identity),
                        rhs: bx(rhs),
                        wildcard: true,
                    }
                } else {
                    self.array_expression()?
                }
            }
            Tok::Current => Node::Current,
            Tok::Global(n) => Node::Global(n),
            Tok::Expref => Node::Expref(bx(self.expression(0)?)),
            Tok::LParen => {
                let mut args = Vec::new();
                while self.peek(0) != &Tok::RParen {
                    args.push(self.expression(0)?);
                }
                self.expect(&Tok::RParen)?;
                args.into_iter()
                    .next()
                    .ok_or_else(|| Error::syntax("Empty parenthesized expression"))?
            }
            other => return Err(Error::syntax(format!("Unexpected token: {other:?}"))),
        })
    }

    fn led(&mut self, t: Tok, left: Node) -> Result<Node, Error> {
        Ok(match t {
            Tok::Concatenate => Node::Concatenate(bx(left), bx(self.expression(5)?)),
            Tok::Dot => {
                if self.peek(0) != &Tok::Star {
                    Node::Chained(bx(left), bx(self.dot_rhs(40)?))
                } else {
                    self.advance();
                    Node::ValueProjection(bx(left), bx(self.projection_rhs(40)?))
                }
            }
            Tok::Pipe => Node::Pipe(bx(left), bx(self.expression(1)?)),
            Tok::Or => Node::Or(bx(left), bx(self.expression(2)?)),
            Tok::And => Node::And(bx(left), bx(self.expression(3)?)),
            Tok::Add => Node::Add(bx(left), bx(self.expression(6)?)),
            Tok::Subtract => Node::Subtract(bx(left), bx(self.expression(6)?)),
            Tok::Multiply => Node::Multiply(bx(left), bx(self.expression(7)?)),
            Tok::Divide => Node::Divide(bx(left), bx(self.expression(7)?)),
            Tok::Union => Node::Union(bx(left), bx(self.expression(6)?)),
            Tok::LParen => match left {
                Node::Field(name) => Node::Function(name, self.function_args()?),
                _ => {
                    return Err(Error::syntax(
                        "Bad function syntax. Parenthesis must be preceded by an unquoted identifier",
                    ));
                }
            },
            Tok::Filter => {
                let cond = self.expression(0)?;
                self.expect(&Tok::RBracket)?;
                let rhs = self.projection_rhs(21)?;
                Node::Filter {
                    left: bx(left),
                    rhs: bx(rhs),
                    cond: bx(cond),
                }
            }
            Tok::Flatten => {
                let rhs = self.projection_rhs(10)?;
                Node::Projection {
                    left: bx(Node::Flatten(bx(left))),
                    rhs: bx(rhs),
                    wildcard: false,
                }
            }
            Tok::Comparator(op) => Node::Comparator(op, bx(left), bx(self.expression(4)?)),
            Tok::LBracket => {
                if self.peek(0) == &Tok::Star && self.peek(1) == &Tok::RBracket {
                    self.advance();
                    self.advance();
                    let rhs = self.projection_rhs(20)?;
                    Node::Projection {
                        left: bx(left),
                        rhs: bx(rhs),
                        wildcard: true,
                    }
                } else {
                    let right = self.index_expression()?;
                    self.project_if_slice(left, right)?
                }
            }
            other => return Err(Error::syntax(format!("Unexpected token: {other:?}"))),
        })
    }

    fn expect(&mut self, t: &Tok) -> Result<(), Error> {
        if self.peek(0) == t {
            self.advance();
            Ok(())
        } else {
            Err(Error::syntax(format!(
                "Expected {t:?}, got: {:?}",
                self.peek(0)
            )))
        }
    }

    fn function_args(&mut self) -> Result<Vec<Node>, Error> {
        let mut args = Vec::new();
        let mut first = true;
        while self.peek(0) != &Tok::RParen {
            if !first {
                self.expect(&Tok::Comma)?;
            }
            args.push(self.expression(0)?);
            first = false;
        }
        self.expect(&Tok::RParen)?;
        Ok(args)
    }

    fn signed_int(&mut self) -> Result<i128, Error> {
        match self.peek(0).clone() {
            Tok::UnaryMinus => {
                self.advance();
                match self.peek(0).clone() {
                    Tok::Int(i) => {
                        self.advance();
                        Ok(-i)
                    }
                    t => Err(Error::syntax(format!("Expected Integer, got: {t:?}"))),
                }
            }
            Tok::Int(i) => {
                self.advance();
                Ok(i)
            }
            t => Err(Error::syntax(format!("Unexpected token: {t:?}"))),
        }
    }

    fn index_expression(&mut self) -> Result<Node, Error> {
        let old = self.index;
        if self.peek(0) == &Tok::Colon {
            return self.slice_expression();
        }
        let first = self.signed_int()?;
        if self.peek(0) == &Tok::Colon {
            self.index = old;
            return self.slice_expression();
        }
        self.expect(&Tok::RBracket)?;
        Ok(Node::Index(first))
    }

    fn project_if_slice(&mut self, left: Node, right: Node) -> Result<Node, Error> {
        let is_slice = matches!(right, Node::Slice(_));
        let index_expr = Node::Bracket(bx(left), bx(right));
        if is_slice {
            let rhs = self.projection_rhs(20)?;
            return Ok(Node::Projection {
                left: bx(index_expr),
                rhs: bx(rhs),
                wildcard: false,
            });
        }
        Ok(index_expr)
    }

    fn slice_expression(&mut self) -> Result<Node, Error> {
        let mut parts = [None, None, None];
        let mut i = 0;
        while self.peek(0) != &Tok::RBracket && i < 3 {
            if self.peek(0) == &Tok::Colon && i < 2 {
                i += 1;
                self.advance();
            } else {
                parts[i] = Some(self.signed_int()?);
                if !matches!(self.peek(0), Tok::Colon | Tok::RBracket) {
                    return Err(Error::syntax(format!(
                        "Unexpected token: {:?}",
                        self.peek(0)
                    )));
                }
            }
        }
        self.expect(&Tok::RBracket)?;
        Ok(Node::Slice(parts))
    }

    fn dot_rhs(&mut self, rbp: u8) -> Result<Node, Error> {
        match self.peek(0) {
            Tok::Identifier(_) | Tok::QuotedIdentifier(_) | Tok::Star => self.expression(rbp),
            Tok::LBracket => {
                self.advance();
                self.array_expression()
            }
            Tok::LBrace => {
                self.advance();
                self.object_expression()
            }
            _ => Err(Error::syntax(
                "Expecting one of: \"*\", \"[\", \"{\", name or quoted name after a dot",
            )),
        }
    }

    fn projection_rhs(&mut self, rbp: u8) -> Result<Node, Error> {
        let next = self.token(Prev::Star);
        if Self::bp(&next)? <= 10 {
            return Ok(Node::Identity);
        }
        match next {
            Tok::LBracket | Tok::Filter => self.expression(rbp),
            Tok::Dot => {
                self.advance();
                self.dot_rhs(rbp)
            }
            other => Err(Error::syntax(format!("Unexpected token: {other:?}"))),
        }
    }

    fn array_expression(&mut self) -> Result<Node, Error> {
        let mut items = Vec::new();
        while self.peek(0) != &Tok::RBracket {
            items.push(self.expression(0)?);
            if self.peek(0) == &Tok::Comma {
                self.advance();
                if self.peek(0) == &Tok::RBracket {
                    return Err(Error::syntax("Unexpected token Rbracket"));
                }
            }
        }
        self.expect(&Tok::RBracket)?;
        Ok(Node::Array(items))
    }

    fn object_expression(&mut self) -> Result<Node, Error> {
        if self.peek(0) == &Tok::RBrace {
            return Err(Error::syntax("An empty object expression is not allowed"));
        }
        let mut pairs = Vec::new();
        loop {
            let key = match self.peek(0).clone() {
                Tok::Identifier(k) | Tok::QuotedIdentifier(k) => k,
                t => {
                    return Err(Error::syntax(format!(
                        "Expecting an identifier token, got: {t:?}"
                    )));
                }
            };
            self.advance();
            self.expect(&Tok::Colon)?;
            pairs.push((key, self.expression(0)?));
            if self.peek(0) == &Tok::Comma {
                self.advance();
            } else if self.peek(0) == &Tok::RBrace {
                self.advance();
                break;
            }
        }
        Ok(Node::Object(pairs))
    }
}
