//! A tiny, safe arithmetic evaluator for derived columns and HAVING.
//!
//! Grammar: `term ((+|-) term)*`, `term = factor ((*|/) factor)*`, `factor = number | name | (expr) | -factor`.
//! `name` is any field/calculation reference — a run of chars that are not operators/parens/space,
//! so `duration_ms`, `gen_ai.usage.cost_usd`, `P95(duration_ms)` and `SUM(x)/SUM(y)` all parse.
//! Names are looked up in a value map at eval time; nothing else (no calls we define, no strings) is
//! allowed, so an expression can never reach SQL or arbitrary code.

use crate::{QueryError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Num(f64),
    Name(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

pub fn lex(s: &str) -> Result<Vec<Tok>> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c == '+' { out.push(Tok::Plus); i += 1; }
        else if c == '-' { out.push(Tok::Minus); i += 1; }
        else if c == '*' { out.push(Tok::Star); i += 1; }
        else if c == '/' { out.push(Tok::Slash); i += 1; }
        else if c == '(' {
            // could be a function-call paren belonging to the preceding name (e.g. P95(x))
            if matches!(out.last(), Some(Tok::Name(_))) {
                // fold "name ( ... )" back into a single Name token by scanning to the matching )
                let start = i;
                let mut depth = 0;
                while i < b.len() {
                    match b[i] as char { '(' => depth += 1, ')' => { depth -= 1; if depth == 0 { i += 1; break; } }, _ => {} }
                    i += 1;
                }
                if depth != 0 { return Err(QueryError::Invalid("unbalanced parentheses in expression".into())); }
                if let Some(Tok::Name(n)) = out.last_mut() { n.push_str(&s[start..i]); }
            } else { out.push(Tok::LParen); i += 1; }
        }
        else if c == ')' { out.push(Tok::RParen); i += 1; }
        else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < b.len() && ((b[i] as char).is_ascii_digit() || b[i] as char == '.') { i += 1; }
            out.push(Tok::Num(s[start..i].parse().map_err(|_| QueryError::Invalid(format!("bad number '{}'", &s[start..i])))?));
        } else {
            // a name: letters, digits, and the punctuation that appears in field names
            let start = i;
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_whitespace() || matches!(ch, '+' | '-' | '*' | '/' | '(' | ')') { break; }
                i += 1;
            }
            out.push(Tok::Name(s[start..i].to_string()));
        }
    }
    Ok(out)
}

/// AST is not needed; we collect the referenced names and evaluate directly.
pub struct Expr {
    toks: Vec<Tok>,
}

impl Expr {
    pub fn parse(s: &str) -> Result<Self> {
        let toks = lex(s)?;
        if toks.is_empty() { return Err(QueryError::Invalid("empty expression".into())); }
        let e = Expr { toks };
        // validate by evaluating against an all-zero lookup
        e.eval(&mut |_| Some(0.0)).ok_or_else(|| QueryError::Invalid(format!("cannot parse expression '{s}'")))?;
        Ok(e)
    }

    /// Names referenced, for the UI and for pre-checking availability.
    pub fn names(&self) -> Vec<String> {
        self.toks.iter().filter_map(|t| if let Tok::Name(n) = t { Some(n.clone()) } else { None }).collect()
    }

    /// Evaluate; `lookup` returns the numeric value of a name (None = unknown → whole expr is None).
    pub fn eval(&self, lookup: &mut dyn FnMut(&str) -> Option<f64>) -> Option<f64> {
        let mut p = Parser { toks: &self.toks, pos: 0, lookup };
        let v = p.expr()?;
        if p.pos == p.toks.len() { Some(v) } else { None }
    }
}

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    lookup: &'a mut dyn FnMut(&str) -> Option<f64>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> { self.toks.get(self.pos) }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(t) = self.peek() {
            match t {
                Tok::Plus => { self.pos += 1; v += self.term()?; }
                Tok::Minus => { self.pos += 1; v -= self.term()?; }
                _ => break,
            }
        }
        Some(v)
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.factor()?;
        while let Some(t) = self.peek() {
            match t {
                Tok::Star => { self.pos += 1; v *= self.factor()?; }
                Tok::Slash => { self.pos += 1; let d = self.factor()?; v = if d == 0.0 { 0.0 } else { v / d }; }
                _ => break,
            }
        }
        Some(v)
    }
    fn factor(&mut self) -> Option<f64> {
        match self.peek()?.clone() {
            Tok::Num(n) => { self.pos += 1; Some(n) }
            Tok::Name(n) => { self.pos += 1; (self.lookup)(&n) }
            Tok::Minus => { self.pos += 1; Some(-self.factor()?) }
            Tok::LParen => { self.pos += 1; let v = self.expr()?; if matches!(self.peek(), Some(Tok::RParen)) { self.pos += 1; Some(v) } else { None } }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ev(s: &str, vars: &[(&str, f64)]) -> Option<f64> {
        Expr::parse(s).unwrap().eval(&mut |n| vars.iter().find(|(k, _)| *k == n).map(|(_, v)| *v))
    }
    #[test]
    fn arithmetic() {
        assert_eq!(ev("1 + 2 * 3", &[]), Some(7.0));
        assert_eq!(ev("(1 + 2) * 3", &[]), Some(9.0));
        assert_eq!(ev("10 / 4", &[]), Some(2.5));
        assert_eq!(ev("x / 0", &[("x", 5.0)]), Some(0.0)); // divide-by-zero → 0, not NaN
    }
    #[test]
    fn field_names() {
        assert_eq!(ev("duration_ms / 1000", &[("duration_ms", 2000.0)]), Some(2.0));
        assert_eq!(ev("SUM(cost) / SUM(tokens)", &[("SUM(cost)", 6.0), ("SUM(tokens)", 3.0)]), Some(2.0));
        assert_eq!(ev("gen_ai.usage.cost_usd * 1000000", &[("gen_ai.usage.cost_usd", 0.0001)]), Some(100.0));
    }
    #[test]
    fn unknown_name_is_none() {
        assert_eq!(ev("a + b", &[("a", 1.0)]), None);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    #[test]
    fn division_by_zero_keeps_parsing() {
        let e = Expr::parse("SUM(a) / SUM(b) * 1000").unwrap();
        assert_eq!(e.eval(&mut |n| Some(if n == "SUM(a)" { 6.0 } else { 3.0 })), Some(2000.0));
        assert_eq!(e.eval(&mut |_| Some(0.0)), Some(0.0));
    }
}
