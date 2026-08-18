//! Arithmetic expressions for feature-tree values.
//!
//! A feature-tree value used to be one of two things: a literal
//! quantity (`"50 mm"`) or a bare variable name (`"length"`). That
//! forces whoever authors the tree — increasingly a model, not a
//! person — to pre-compute every coordinate. Wanting a hole centred
//! between two edges meant writing `0.04` and hoping the plate never
//! changed size. Nothing recorded WHY it was 0.04, so nothing could
//! keep it correct when `length` moved.
//!
//! This module adds the third form: `"length/2 - 10 mm"`. Intent
//! survives in the file, and the value tracks its inputs.
//!
//! ## Unit algebra
//!
//! [`Quantity`] can represent a length, an angle, a mass, a force, or
//! a dimensionless scalar — and notably NOT an area. That absence is
//! load-bearing: `length * width` has no representable result, so it
//! is rejected rather than silently reinterpreted. The rules are the
//! ones dimensional analysis gives:
//!
//! | Operation                | Result   |
//! |--------------------------|----------|
//! | length + length          | length   |
//! | length - length          | length   |
//! | length * scalar          | length   |
//! | scalar * length          | length   |
//! | length / scalar          | length   |
//! | length / length          | scalar   |
//! | length + scalar          | REJECTED |
//! | length * length          | REJECTED |
//!
//! Arithmetic happens in SI (metres, radians, kilograms, newtons), so
//! `"1 m + 500 mm"` is 1.5 m. The authored unit of the inputs is not
//! preserved through arithmetic — the result carries the SI base unit
//! for its dimension, which is what every consumer converts to anyway.
//!
//! ## Why this is a fallback, not a replacement
//!
//! [`crate::feature_tree::resolve_quantity`] tries a literal parse
//! first, then a variable lookup, and only then reaches this module.
//! Every value that resolved before still resolves the same way by the
//! same path, so no existing tree changes meaning. Only strings that
//! were previously errors — the ones containing operators — newly
//! become meaningful.

use std::collections::HashMap;

use crate::quantity::{AngleUnit, ForceUnit, LengthUnit, MassUnit, Quantity, Unit};

/// Guards mutually-recursive variables (`a = "b + 1"`, `b = "a * 2"`).
/// Mirrors the depth cap in `resolve_quantity_depth`.
const MAX_DEPTH: u8 = 32;

/// The dimension of an intermediate value. Deliberately coarser than
/// [`Unit`]: arithmetic runs in SI, so only the family matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dim {
    Length,
    Angle,
    Mass,
    Force,
    Scalar,
}

impl Dim {
    fn of(unit: Unit) -> Self {
        match unit {
            Unit::Length(_) => Dim::Length,
            Unit::Angle(_) => Dim::Angle,
            Unit::Mass(_) => Dim::Mass,
            Unit::Force(_) => Dim::Force,
            Unit::Scalar => Dim::Scalar,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Dim::Length => "length",
            Dim::Angle => "angle",
            Dim::Mass => "mass",
            Dim::Force => "force",
            Dim::Scalar => "scalar",
        }
    }

    /// The SI base unit a result of this dimension is reported in.
    fn si_unit(self) -> Unit {
        match self {
            Dim::Length => Unit::Length(LengthUnit::Meter),
            Dim::Angle => Unit::Angle(AngleUnit::Radian),
            Dim::Mass => Unit::Mass(MassUnit::Kilogram),
            Dim::Force => Unit::Force(ForceUnit::Newton),
            Dim::Scalar => Unit::Scalar,
        }
    }
}

/// An intermediate value: magnitude in SI plus its dimension.
#[derive(Debug, Clone, Copy)]
struct Val {
    si: f64,
    dim: Dim,
}

impl Val {
    fn to_quantity(self) -> Quantity {
        Quantity {
            value: self.si,
            unit: self.dim.si_unit(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    /// A bare word: either a variable name or a unit suffix, decided
    /// by position rather than by spelling.
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => { out.push(Tok::Plus); i += 1; }
            '-' => { out.push(Tok::Minus); i += 1; }
            '*' => { out.push(Tok::Star); i += 1; }
            '/' => { out.push(Tok::Slash); i += 1; }
            '(' => { out.push(Tok::LParen); i += 1; }
            ')' => { out.push(Tok::RParen); i += 1; }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                let mut seen_dot = false;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || (chars[i] == '.' && !seen_dot))
                {
                    if chars[i] == '.' {
                        seen_dot = true;
                    }
                    i += 1;
                }
                // Scientific notation: 1e-3, 2.5E+4.
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let save = i;
                    let mut j = i + 1;
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        while j < chars.len() && chars[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    } else {
                        i = save;
                    }
                }
                let text: String = chars[start..i].iter().collect();
                let v: f64 = text
                    .parse()
                    .map_err(|_| format!("'{text}' is not a number"))?;
                out.push(Tok::Num(v));
            }
            _ if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            other => {
                return Err(format!(
                    "unexpected character '{other}' — expressions use + - * / ( ) with numbers, \
                     units and variable names"
                ))
            }
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    vars: &'a HashMap<String, String>,
    depth: u8,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// expr := term (('+' | '-') term)*
    fn expr(&mut self) -> Result<Val, String> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => '+',
                Some(Tok::Minus) => '-',
                _ => break,
            };
            self.pos += 1;
            let rhs = self.term()?;
            // Addition demands identical dimensions. Adding a bare
            // number to a length is the single most likely authoring
            // slip, and silently treating the number as metres is
            // exactly the 1000x error the unit system exists to stop.
            if lhs.dim != rhs.dim {
                return Err(format!(
                    "cannot {} {} and {} — both sides of '{}' must have the same dimension \
                     (did you forget a unit, e.g. '10 mm' instead of '10'?)",
                    if op == '+' { "add" } else { "subtract" },
                    lhs.dim.name(),
                    rhs.dim.name(),
                    op
                ));
            }
            lhs = Val {
                si: if op == '+' { lhs.si + rhs.si } else { lhs.si - rhs.si },
                dim: lhs.dim,
            };
        }
        Ok(lhs)
    }

    /// term := factor (('*' | '/') factor)*
    fn term(&mut self) -> Result<Val, String> {
        let mut lhs = self.factor()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => '*',
                Some(Tok::Slash) => '/',
                _ => break,
            };
            self.pos += 1;
            let rhs = self.factor()?;
            lhs = match op {
                '*' => match (lhs.dim, rhs.dim) {
                    (Dim::Scalar, d) => Val { si: lhs.si * rhs.si, dim: d },
                    (d, Dim::Scalar) => Val { si: lhs.si * rhs.si, dim: d },
                    (a, b) => {
                        return Err(format!(
                            "cannot multiply {} by {} — the result has no representable unit \
                             (there is no area or volume unit); multiply by a plain number instead",
                            a.name(),
                            b.name()
                        ))
                    }
                },
                _ => {
                    if rhs.si == 0.0 {
                        return Err("division by zero".to_string());
                    }
                    match (lhs.dim, rhs.dim) {
                        (d, Dim::Scalar) => Val { si: lhs.si / rhs.si, dim: d },
                        (a, b) if a == b => Val { si: lhs.si / rhs.si, dim: Dim::Scalar },
                        (a, b) => {
                            return Err(format!(
                                "cannot divide {} by {} — dividing unlike dimensions has no \
                                 representable unit",
                                a.name(),
                                b.name()
                            ))
                        }
                    }
                }
            };
        }
        Ok(lhs)
    }

    /// factor := ('-' | '+') factor | primary
    fn factor(&mut self) -> Result<Val, String> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.pos += 1;
                let v = self.factor()?;
                Ok(Val { si: -v.si, dim: v.dim })
            }
            Some(Tok::Plus) => {
                self.pos += 1;
                self.factor()
            }
            _ => self.primary(),
        }
    }

    /// primary := number [unit] | ident | '(' expr ')'
    fn primary(&mut self) -> Result<Val, String> {
        match self.next() {
            Some(Tok::Num(v)) => {
                // A following word MIGHT be a unit suffix ("10 mm") or
                // might be an unrelated variable in a malformed
                // expression. Decide by asking Quantity whether the
                // pair parses, rather than keeping a duplicate list of
                // unit spellings here that could drift.
                if let Some(Tok::Ident(word)) = self.peek().cloned() {
                    if let Some(q) = Quantity::parse(&format!("{v} {word}")) {
                        if !matches!(q.unit, Unit::Scalar) {
                            self.pos += 1;
                            return Ok(Val { si: q.to_si(), dim: Dim::of(q.unit) });
                        }
                    }
                }
                Ok(Val { si: v, dim: Dim::Scalar })
            }
            Some(Tok::Ident(name)) => {
                if self.depth >= MAX_DEPTH {
                    return Err(format!(
                        "variable nesting too deep at '{name}' — check for a reference cycle"
                    ));
                }
                let Some(raw) = self.vars.get(&name) else {
                    let mut known: Vec<&str> = self.vars.keys().map(|s| s.as_str()).collect();
                    known.sort_unstable();
                    return Err(format!(
                        "unknown variable '{name}'{}",
                        if known.is_empty() {
                            " — this tree declares no variables".to_string()
                        } else {
                            format!(" — declared variables are: {}", known.join(", "))
                        }
                    ));
                };
                // The variable's own value may itself be an expression.
                let q = eval_depth(raw, self.vars, self.depth + 1)
                    .map_err(|e| format!("while resolving '{name}': {e}"))?;
                Ok(Val { si: q.to_si(), dim: Dim::of(q.unit) })
            }
            Some(Tok::LParen) => {
                let v = self.expr()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(v),
                    _ => Err("unbalanced parentheses — missing ')'".to_string()),
                }
            }
            Some(t) => Err(format!("unexpected {t:?} where a value was expected")),
            None => Err("expression ended where a value was expected".to_string()),
        }
    }
}

/// Evaluate an arithmetic expression against a variable table.
///
/// Returns the result in the SI base unit for its dimension. Callers
/// that only accept a length should check `unit` and reject anything
/// else — a caller asking for a depth and receiving a scalar means the
/// author wrote a bare number somewhere.
pub fn eval(src: &str, vars: &HashMap<String, String>) -> Result<Quantity, String> {
    eval_depth(src, vars, 0)
}

fn eval_depth(src: &str, vars: &HashMap<String, String>, depth: u8) -> Result<Quantity, String> {
    if depth > MAX_DEPTH {
        return Err("variable nesting too deep — check for a reference cycle".to_string());
    }
    // A plain literal or a bare variable name never reaches the parser
    // in normal operation (resolve_quantity tries those first), but
    // eval() is public and callers may hand us either.
    if let Some(q) = Quantity::parse(src) {
        return Ok(q);
    }
    let toks = tokenize(src)?;
    if toks.is_empty() {
        return Err("empty expression".to_string());
    }
    let mut p = Parser { toks, pos: 0, vars, depth };
    let v = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(format!(
            "trailing input after a complete expression (stopped at token {})",
            p.pos + 1
        ));
    }
    Ok(v.to_quantity())
}

/// True when the string looks like an expression rather than a literal
/// or a bare name — i.e. it contains an operator or a parenthesis.
/// Used to decide whether a failed resolution is worth reporting as an
/// expression error rather than an unknown-variable error.
pub fn looks_like_expression(s: &str) -> bool {
    s.contains(['+', '*', '/', '(', ')']) || s.trim_start_matches('-').contains('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn si(src: &str, v: &HashMap<String, String>) -> f64 {
        eval(src, v).unwrap().to_si()
    }

    #[test]
    fn literals_still_parse_unchanged() {
        let v = vars(&[]);
        assert!((si("50 mm", &v) - 0.05).abs() < 1e-12);
        assert!((si("0.1 m", &v) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn division_by_scalar_keeps_length() {
        let v = vars(&[("length", "0.1 m")]);
        assert!((si("length/2", &v) - 0.05).abs() < 1e-12);
    }

    #[test]
    fn mixed_units_add_in_si() {
        let v = vars(&[]);
        assert!((si("1 m + 500 mm", &v) - 1.5).abs() < 1e-12);
    }

    #[test]
    fn the_motivating_case() {
        // "half the plate, less a 10 mm margin"
        let v = vars(&[("length", "0.1 m")]);
        assert!((si("length/2 - 10 mm", &v) - 0.04).abs() < 1e-12);
    }

    #[test]
    fn parens_and_precedence() {
        let v = vars(&[("a", "0.1 m"), ("b", "0.06 m")]);
        assert!((si("(a - b)/2", &v) - 0.02).abs() < 1e-12);
        // * binds tighter than -
        assert!((si("a - b*2", &v) - (-0.02)).abs() < 1e-12);
    }

    #[test]
    fn length_over_length_is_scalar() {
        let v = vars(&[("a", "0.1 m"), ("b", "0.05 m")]);
        let q = eval("a/b", &v).unwrap();
        assert!(matches!(q.unit, Unit::Scalar));
        assert!((q.value - 2.0).abs() < 1e-12);
    }

    #[test]
    fn adding_a_bare_number_to_a_length_is_rejected() {
        let v = vars(&[("length", "0.1 m")]);
        let e = eval("length + 5", &v).unwrap_err();
        assert!(e.contains("same dimension"), "{e}");
    }

    #[test]
    fn multiplying_two_lengths_is_rejected() {
        let v = vars(&[("a", "0.1 m"), ("b", "0.06 m")]);
        let e = eval("a*b", &v).unwrap_err();
        assert!(e.contains("no representable unit"), "{e}");
    }

    #[test]
    fn unknown_variable_names_the_known_ones() {
        let v = vars(&[("length", "0.1 m"), ("width", "0.06 m")]);
        let e = eval("heigth/2", &v).unwrap_err();
        assert!(e.contains("unknown variable 'heigth'"), "{e}");
        assert!(e.contains("length") && e.contains("width"), "{e}");
    }

    #[test]
    fn variable_cycles_terminate() {
        let v = vars(&[("a", "b + 1 mm"), ("b", "a + 1 mm")]);
        assert!(eval("a", &v).is_err());
    }

    #[test]
    fn nested_variable_expressions_resolve() {
        let v = vars(&[("length", "0.1 m"), ("half", "length/2"), ("margin", "10 mm")]);
        assert!((si("half - margin", &v) - 0.04).abs() < 1e-12);
    }

    #[test]
    fn angles_work_too() {
        let v = vars(&[("full", "360 deg")]);
        let q = eval("full/4", &v).unwrap();
        assert!(matches!(q.unit, Unit::Angle(_)));
        assert!((q.to_si() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn unary_minus() {
        let v = vars(&[("length", "0.1 m")]);
        assert!((si("-length/2", &v) + 0.05).abs() < 1e-12);
    }

    #[test]
    fn division_by_zero_is_an_error() {
        let v = vars(&[("a", "0.1 m")]);
        assert!(eval("a/0", &v).is_err());
    }

    #[test]
    fn garbage_is_rejected_not_silently_zero() {
        let v = vars(&[]);
        assert!(eval("$$$", &v).is_err());
        assert!(eval("(1 m", &v).is_err());
        assert!(eval("1 m 2 m", &v).is_err());
    }
}
