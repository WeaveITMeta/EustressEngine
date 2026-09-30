//! The Luau service's parser: every syntax error in a file, with the bytes
//! it covers, from full-moon. This is the only module that names full-moon.

use eustress_common::luau::lang::{lexer, TokenClass};
use full_moon::{parse_fallible, LuaVersion};

/// Deeper than this, full-moon is not asked: its parser recurses once per
/// level, and a file nested thousands deep (generated, or hostile) would
/// overflow the analysis thread's stack and take Studio down with it. The
/// VM's compiler, which has its own recursion limit, still checks such a
/// file. Hand-written code stays far below this.
pub const MAX_PARSE_DEPTH: usize = 128;

/// How deeply `source` nests brackets and blocks at its deepest point.
pub fn nesting_depth(source: &str) -> usize {
    let (mut depth, mut max) = (0usize, 0usize);
    let mut state = lexer::initial_state();
    for line in source.lines() {
        let (tokens, end) = lexer::tokenize_line(line, state);
        state = end;
        for t in tokens {
            if !matches!(t.class, TokenClass::Keyword | TokenClass::ControlFlow | TokenClass::Punctuation) {
                continue;
            }
            match &line[t.start as usize..(t.start + t.len) as usize] {
                "(" | "{" | "[" | "function" | "do" | "then" | "repeat" => {
                    depth += 1;
                    max = max.max(depth);
                }
                ")" | "}" | "]" | "end" | "until" => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    max
}

/// A syntax error and the bytes of the source it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// Every syntax error in `source`, in order. The parser recovers after each
/// one, so a file with three mistakes reports three. A file nested deeper
/// than [`MAX_PARSE_DEPTH`] is not parsed here and reports none; the VM's
/// compiler checks it.
pub fn syntax_errors(source: &str) -> Vec<SyntaxError> {
    if nesting_depth(source) > MAX_PARSE_DEPTH {
        return Vec::new();
    }
    let result = parse_fallible(source, LuaVersion::luau());
    let mut out: Vec<SyntaxError> = result
        .errors()
        .iter()
        .map(|e| {
            let (from, to) = e.range();
            let start = floor_char(source, from.bytes().min(source.len()));
            let mut end = floor_char(source, to.bytes().min(source.len())).max(start);
            if end == start {
                // A point error (a missing `end` at the end of the file):
                // cover the character there, or the last one.
                end = source[start..].chars().next().map_or(start, |c| start + c.len_utf8());
            }
            SyntaxError { message: e.error_message().into_owned(), start, end }
        })
        .collect();
    out.sort_by_key(|e| (e.start, e.end));
    out.dedup();
    out
}

/// The nearest character boundary at or before `i`.
fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luau_syntax_parses_clean() {
        let src = "local t: {number} = {1, 2}\n\
                   local s = `total {#t}`\n\
                   for _, v in t do\n  if v == 1 then continue end\n  v += 1\nend\n\
                   type Pad<T> = { price: number, owner: T? }\n\
                   local function id<T>(x: T): T return x end\n\
                   local n = if #t > 1 then 1 else 2\n";
        assert_eq!(syntax_errors(src), vec![]);
    }

    #[test]
    fn every_mistake_is_reported_where_it_is() {
        let src = "local x = \nlocal y = 2\nprint(y\n";
        let errors = syntax_errors(src);
        assert!(!errors.is_empty(), "a dangling `=` and an unclosed call are errors");
        for e in &errors {
            assert!(e.start <= e.end && e.end <= src.len(), "{e:?}");
            assert!(src.is_char_boundary(e.start) && src.is_char_boundary(e.end));
        }
    }

    #[test]
    fn a_file_nested_thousands_deep_is_left_to_the_compiler() {
        // Thousands of nested parentheses: parsing this recursively would
        // overflow the stack. It must come back at once, with nothing.
        let deep = format!("local x = {}1{}\n", "(".repeat(5000), ")".repeat(5000));
        assert!(nesting_depth(&deep) > MAX_PARSE_DEPTH);
        assert_eq!(syntax_errors(&deep), vec![]);
        // Ordinary code is far below the limit.
        let usual = "local function f(t)\n  for _, v in t do\n    if v then print((v)) end\n  end\nend\n";
        assert_eq!(nesting_depth(usual), 5);
    }

    #[test]
    fn multibyte_text_near_an_error_keeps_ranges_on_characters() {
        let src = "local s = \"héllo\"\nlocal = ✓\n";
        for e in syntax_errors(src) {
            assert!(src.is_char_boundary(e.start) && src.is_char_boundary(e.end), "{e:?}");
        }
    }
}
