//! # Rune language service
//!
//! A line-state lexer for highlighting, Rune's editing conventions, and the
//! static analyzer ([`super::analyzer`]) behind [`LanguageService`].
//!
//! The lexer carries, from one line to the next: an open block comment (with
//! its nesting depth and whether it is a doc comment), an open string or
//! template string (Rune strings may span lines), and the brace depth inside
//! a template's `${ }`. One level of template interpolation is tracked; a
//! template string nested inside an interpolation is lexed as its own string.
//!
//! Keywords match `infrastructure/extensions/lsp/vscode/syntaxes/rune.tmLanguage.json`
//! so the external extension and the editor agree.

use std::sync::Arc;

use super::analyzer::{self, SymbolIndex};
use super::language::{
    Analysis, Completion, CompletionContext, CompletionKind, Hover, LanguageId, LanguageService,
    LineState, Location, SignatureHelp, Token, TokenClass,
};

/// The Rune service. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct RuneService;

// ─── Line state ─────────────────────────────────────────────────────────
//
// bits 0..8    mode
// bits 8..16   block comment nesting depth
// bit  16      the open block comment is a doc comment
// bits 24..32  brace depth inside a template's `${ }` (0 outside one)

const MODE_MASK: u64 = 0xFF;
const NORMAL: u64 = 0;
const BLOCK_COMMENT: u64 = 1;
const STRING: u64 = 2;
const TEMPLATE: u64 = 3;

fn pack(mode: u64, depth: u64, doc: bool, interp: u64) -> LineState {
    mode | (depth.min(0xFF) << 8) | ((doc as u64) << 16) | (interp.min(0xFF) << 24)
}

const CONTROL_FLOW: &[&str] = &[
    "if", "else", "match", "while", "for", "loop", "break", "continue", "return", "yield",
    "select", "await",
];
const KEYWORDS: &[&str] = &[
    "fn", "let", "const", "struct", "enum", "impl", "mod", "use", "pub", "async", "in", "not",
    "as", "is", "move", "extern",
];
const BUILTINS: &[&str] = &["self", "Self", "super", "crate"];

const OPERATORS_3: &[&str] = &["..=", "<<=", ">>="];
const OPERATORS_2: &[&str] = &[
    "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "=>", "->", "::", "..", "+=", "-=", "*=",
    "/=", "%=", "^=", "&=", "|=",
];
const OPERATORS_1: &[u8] = b"+-*/%=<>!&|^~?";
const PUNCTUATION: &[u8] = b"()[]{},;:.";

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// End of the escape sequence starting at the backslash `b[i]`.
fn escape_end(line: &str, i: usize) -> usize {
    let b = line.as_bytes();
    let n = b.len();
    if i + 1 >= n {
        return n;
    }
    match b[i + 1] {
        b'x' => (i + 4).min(n),
        b'u' if i + 2 < n && b[i + 2] == b'{' => {
            b[i + 2..].iter().position(|&c| c == b'}').map(|p| i + 2 + p + 1).unwrap_or(n)
        }
        _ => i + 1 + line[i + 1..].chars().next().map_or(1, char::len_utf8),
    }
}

/// Lex one line from `state`. Tokens never overlap and lie within the line;
/// whitespace is left out.
pub fn tokenize_rune_line(line: &str, state: LineState) -> (Vec<Token>, LineState) {
    let b = line.as_bytes();
    let n = b.len();
    let mut out: Vec<Token> = Vec::new();
    let mut mode = state & MODE_MASK;
    let mut depth = (state >> 8) & 0xFF;
    let mut doc = (state >> 16) & 1 == 1;
    let mut interp = (state >> 24) & 0xFF;
    let mut i = 0usize;

    fn push(out: &mut Vec<Token>, s: usize, e: usize, class: TokenClass) {
        if e > s {
            out.push(Token { start: s as u32, len: (e - s) as u32, class });
        }
    }

    // Context for classifying identifiers: the previous significant token.
    let mut prev_dot = false;
    let mut prev_word: Option<(usize, usize)> = None;

    while i < n {
        match mode {
            BLOCK_COMMENT => {
                let s = i;
                while i < n {
                    if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
                        i += 2;
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            mode = NORMAL;
                            break;
                        }
                    } else if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
                        i += 2;
                        depth += 1;
                    } else {
                        i += 1;
                    }
                }
                push(&mut out, s, i, if doc { TokenClass::DocComment } else { TokenClass::Comment });
                if mode == NORMAL {
                    doc = false;
                }
            }
            STRING | TEMPLATE => {
                let close = if mode == STRING { b'"' } else { b'`' };
                let mut s = i;
                loop {
                    if i >= n {
                        push(&mut out, s, i, TokenClass::String);
                        break;
                    }
                    let c = b[i];
                    if c == b'\\' {
                        push(&mut out, s, i, TokenClass::String);
                        let e = escape_end(line, i);
                        push(&mut out, i, e, TokenClass::StringEscape);
                        i = e;
                        s = i;
                    } else if c == close {
                        i += 1;
                        push(&mut out, s, i, TokenClass::String);
                        mode = NORMAL;
                        break;
                    } else if mode == TEMPLATE && c == b'$' && i + 1 < n && b[i + 1] == b'{' {
                        push(&mut out, s, i, TokenClass::String);
                        push(&mut out, i, i + 2, TokenClass::Interpolation);
                        i += 2;
                        interp = 1;
                        mode = NORMAL;
                        break;
                    } else {
                        i += 1;
                    }
                }
                prev_dot = false;
                prev_word = None;
            }
            _ => {
                let c = b[i];
                if c.is_ascii_whitespace() {
                    i += 1;
                    continue;
                }
                let next = b.get(i + 1).copied();

                // Comments.
                if c == b'/' && next == Some(b'/') {
                    let third = b.get(i + 2).copied();
                    let is_doc = (third == Some(b'/') && b.get(i + 3) != Some(&b'/')) || third == Some(b'!');
                    push(&mut out, i, n, if is_doc { TokenClass::DocComment } else { TokenClass::Comment });
                    i = n;
                    continue;
                }
                if c == b'/' && next == Some(b'*') {
                    let third = b.get(i + 2).copied();
                    doc = (third == Some(b'*') && b.get(i + 3) != Some(&b'/')) || third == Some(b'!');
                    push(&mut out, i, i + 2, if doc { TokenClass::DocComment } else { TokenClass::Comment });
                    i += 2;
                    depth = 1;
                    mode = BLOCK_COMMENT;
                    continue;
                }

                // Strings.
                if c == b'"' || c == b'`' {
                    push(&mut out, i, i + 1, TokenClass::String);
                    i += 1;
                    mode = if c == b'"' { STRING } else { TEMPLATE };
                    continue;
                }
                if c == b'\'' {
                    if let Some(e) = char_literal_end(line, i) {
                        lex_char_literal(line, i, e, &mut out);
                        i = e;
                    } else {
                        // A loop label: `'outer`.
                        let s = i;
                        i += 1;
                        while i < n {
                            let ch = line[i..].chars().next().unwrap();
                            if !is_ident_continue(ch) {
                                break;
                            }
                            i += ch.len_utf8();
                        }
                        push(&mut out, s, i, TokenClass::Variable);
                    }
                    prev_dot = false;
                    prev_word = None;
                    continue;
                }

                // Numbers.
                if c.is_ascii_digit() {
                    let s = i;
                    i = number_end(b, i);
                    push(&mut out, s, i, TokenClass::Number);
                    prev_dot = false;
                    prev_word = None;
                    continue;
                }

                // Attributes: `#[...]` and `#![...]`, to the matching `]`.
                if c == b'#' && (next == Some(b'[') || (next == Some(b'!') && b.get(i + 2) == Some(&b'['))) {
                    let s = i;
                    let mut level = 0i32;
                    while i < n {
                        match b[i] {
                            b'[' => level += 1,
                            b']' => {
                                level -= 1;
                                if level == 0 {
                                    i += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    push(&mut out, s, i, TokenClass::Attribute);
                    prev_dot = false;
                    prev_word = None;
                    continue;
                }

                // Identifiers and keywords.
                let ch = line[i..].chars().next().unwrap();
                if is_ident_start(ch) {
                    let s = i;
                    while i < n {
                        let ch = line[i..].chars().next().unwrap();
                        if !is_ident_continue(ch) {
                            break;
                        }
                        i += ch.len_utf8();
                    }
                    let word = &line[s..i];
                    // Byte strings and byte characters: `b"..."`, `b'x'`.
                    if word == "b" && b.get(i) == Some(&b'"') {
                        push(&mut out, s, i + 1, TokenClass::String);
                        i += 1;
                        mode = STRING;
                        continue;
                    }
                    if word == "b" && b.get(i) == Some(&b'\'') {
                        if let Some(e) = char_literal_end(line, i) {
                            push(&mut out, s, i, TokenClass::String);
                            lex_char_literal(line, i, e, &mut out);
                            i = e;
                            prev_dot = false;
                            prev_word = None;
                            continue;
                        }
                    }
                    let before = prev_word.map(|(a, z)| &line[a..z]);
                    let class = classify_word(word, prev_dot, before, &line[i..]);
                    push(&mut out, s, i, class);
                    prev_dot = false;
                    prev_word = Some((s, i));
                    continue;
                }

                // Template interpolation braces.
                if interp > 0 && (c == b'{' || c == b'}') {
                    if c == b'{' {
                        interp += 1;
                        push(&mut out, i, i + 1, TokenClass::Punctuation);
                    } else {
                        interp -= 1;
                        if interp == 0 {
                            push(&mut out, i, i + 1, TokenClass::Interpolation);
                            i += 1;
                            mode = TEMPLATE;
                            continue;
                        }
                        push(&mut out, i, i + 1, TokenClass::Punctuation);
                    }
                    i += 1;
                    prev_dot = false;
                    prev_word = None;
                    continue;
                }

                // Operators, longest first.
                let rest = &line[i..];
                if let Some(op) = OPERATORS_3.iter().chain(OPERATORS_2.iter()).find(|op| rest.starts_with(**op)) {
                    push(&mut out, i, i + op.len(), TokenClass::Operator);
                    i += op.len();
                    prev_dot = false;
                    prev_word = None;
                    continue;
                }
                if OPERATORS_1.contains(&c) {
                    push(&mut out, i, i + 1, TokenClass::Operator);
                    i += 1;
                    prev_dot = false;
                    prev_word = None;
                    continue;
                }
                if PUNCTUATION.contains(&c) {
                    push(&mut out, i, i + 1, TokenClass::Punctuation);
                    i += 1;
                    prev_dot = c == b'.';
                    prev_word = None;
                    continue;
                }

                // Anything else: one character of plain text.
                let len = ch.len_utf8();
                push(&mut out, i, i + len, TokenClass::Text);
                i += len;
                prev_dot = false;
                prev_word = None;
            }
        }
    }

    (out, pack(mode, depth, doc, interp))
}

/// End of a character literal starting at the quote `b[i]`, or `None` when
/// the quote opens a loop label.
fn char_literal_end(line: &str, i: usize) -> Option<usize> {
    let b = line.as_bytes();
    let mut j = i + 1;
    if j >= b.len() {
        return None;
    }
    if b[j] == b'\\' {
        j = escape_end(line, j);
    } else {
        j += line[j..].chars().next()?.len_utf8();
    }
    (b.get(j) == Some(&b'\'')).then_some(j + 1)
}

fn lex_char_literal(line: &str, s: usize, e: usize, out: &mut Vec<Token>) {
    let b = line.as_bytes();
    if b.get(s + 1) == Some(&b'\\') {
        out.push(Token { start: s as u32, len: 1, class: TokenClass::String });
        out.push(Token { start: (s + 1) as u32, len: (e - 1 - (s + 1)) as u32, class: TokenClass::StringEscape });
        out.push(Token { start: (e - 1) as u32, len: 1, class: TokenClass::String });
    } else {
        out.push(Token { start: s as u32, len: (e - s) as u32, class: TokenClass::String });
    }
}

/// End of a number literal starting at the digit `b[i]`. A `.` belongs to
/// the number only when a digit follows it, so `1..10` stays a range.
fn number_end(b: &[u8], mut i: usize) -> usize {
    let n = b.len();
    if b[i] == b'0' && i + 1 < n && matches!(b[i + 1], b'x' | b'o' | b'b') {
        i += 2;
        while i < n && (b[i].is_ascii_hexdigit() || b[i] == b'_') {
            i += 1;
        }
    } else {
        while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
            i += 1;
        }
        if i + 1 < n && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            i += 1;
            while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
                i += 1;
            }
        }
        if i < n && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < n && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            if j < n && b[j].is_ascii_digit() {
                i = j;
                while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
                    i += 1;
                }
            }
        }
    }
    // Type suffix: `10u8`, `1.5f64`.
    while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
        i += 1;
    }
    i
}

fn classify_word(word: &str, prev_dot: bool, before: Option<&str>, rest: &str) -> TokenClass {
    if word == "true" || word == "false" {
        return TokenClass::Constant;
    }
    if BUILTINS.contains(&word) {
        return TokenClass::Builtin;
    }
    if CONTROL_FLOW.contains(&word) {
        return TokenClass::ControlFlow;
    }
    if KEYWORDS.contains(&word) && !prev_dot {
        return TokenClass::Keyword;
    }
    match before {
        Some("fn") => return TokenClass::Function,
        Some("struct" | "enum" | "impl") => return TokenClass::Type,
        _ => {}
    }
    let after = rest.trim_start();
    let is_macro = after.starts_with('!') && !after.starts_with("!=");
    if after.starts_with('(') || is_macro {
        return if prev_dot { TokenClass::Method } else { TokenClass::Function };
    }
    if prev_dot {
        return TokenClass::Property;
    }
    let first = word.chars().next().unwrap_or('_');
    if first.is_uppercase() {
        let all_caps = word.len() > 1
            && word.chars().any(char::is_alphabetic)
            && word.chars().all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_');
        return if all_caps { TokenClass::Constant } else { TokenClass::Type };
    }
    TokenClass::Variable
}

/// The last token of `line` that is not a comment.
fn last_code_token(line: &str) -> Option<Token> {
    let (tokens, _) = tokenize_rune_line(line, 0);
    tokens
        .into_iter()
        .rev()
        .find(|t| !matches!(t.class, TokenClass::Comment | TokenClass::DocComment))
}

/// 1-based line and byte column of `offset`, as the analyzer takes them.
fn line_col(source: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(source.len());
    let before = &source.as_bytes()[..offset];
    let line = before.iter().filter(|&&c| c == b'\n').count() as u32 + 1;
    let line_start = before.iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
    (line, (offset - line_start) as u32 + 1)
}

/// The symbol index `analyze` stored, when this analysis came from Rune.
fn symbols_of(analysis: &Analysis) -> Option<&SymbolIndex> {
    analysis.service_data.as_ref()?.downcast_ref::<SymbolIndex>()
}

impl LanguageService for RuneService {
    fn id(&self) -> LanguageId {
        LanguageId::Rune
    }

    fn initial_state(&self) -> LineState {
        0
    }

    fn tokenize_line(&self, line: &str, state: LineState) -> (Vec<Token>, LineState) {
        tokenize_rune_line(line, state)
    }

    fn line_comment(&self) -> Option<&'static str> {
        Some("//")
    }

    fn auto_close_pairs(&self) -> &'static [(char, char)] {
        &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')]
    }

    fn indent_after(&self, line: &str) -> bool {
        last_code_token(line).is_some_and(|t| {
            t.class == TokenClass::Punctuation
                && matches!(&line[t.start as usize..(t.start + t.len) as usize], "{" | "(" | "[")
        })
    }

    fn dedent_line(&self, line: &str) -> bool {
        matches!(line.trim_start().chars().next(), Some('}' | ')' | ']'))
    }

    fn analyze(&self, source: &str) -> Analysis {
        let result = analyzer::analyze(source);
        Analysis {
            diagnostics: result.diagnostics,
            symbols: result.symbols.iter().cloned().collect(),
            service_data: Some(Arc::new(result.symbols)),
        }
    }

    fn complete(&self, cx: &CompletionContext) -> Vec<Completion> {
        // Member completion after `.` needs types (P2). Until then the list is
        // keywords, the Eustress API and the file's own functions, from a
        // lexer pass rather than a compile.
        if cx.access.is_some() || cx.in_string.is_some() {
            return Vec::new();
        }
        let mut index = SymbolIndex::default();
        let mut state = 0;
        for line in cx.source.lines() {
            let (tokens, next) = tokenize_rune_line(line, state);
            state = next;
            for t in tokens.iter().filter(|t| t.class == TokenClass::Function) {
                let name = &line[t.start as usize..(t.start + t.len) as usize];
                let is_decl = tokens.iter().any(|k| {
                    k.class == TokenClass::Keyword
                        && k.start + k.len <= t.start
                        && &line[k.start as usize..(k.start + k.len) as usize] == "fn"
                });
                if is_decl {
                    index.by_name.entry(name.to_string()).or_default().push(analyzer::Symbol {
                        name: name.to_string(),
                        kind: analyzer::SymbolKind::Function,
                        range: analyzer::Range { start_line: 0, start_column: 0, end_line: 0, end_column: 0 },
                        byte_range: (0, 0),
                    });
                }
            }
        }
        analyzer::complete(cx.prefix, &index, 50)
            .into_iter()
            .map(|c| Completion {
                insert_text: c.label.clone(),
                label: c.label,
                kind: match c.kind {
                    analyzer::CompletionKind::Keyword => CompletionKind::Keyword,
                    analyzer::CompletionKind::Function => CompletionKind::Function,
                    analyzer::CompletionKind::Variable => CompletionKind::Variable,
                    analyzer::CompletionKind::Module => CompletionKind::Module,
                },
                detail: c.detail,
                documentation: None,
            })
            .collect()
    }

    fn signature_help(&self, source: &str, offset: usize) -> Option<SignatureHelp> {
        let (line, col) = line_col(source, offset);
        let (entry, active) = analyzer::signature_help_at(source, line, col)?;
        let mut label = format!("{}(", entry.name);
        let mut parameters = Vec::with_capacity(entry.params.len());
        for (k, p) in entry.params.iter().enumerate() {
            if k > 0 {
                label.push_str(", ");
            }
            let start = label.len() as u32;
            label.push_str(&p.name);
            if !p.typ.is_empty() {
                label.push_str(": ");
                label.push_str(&p.typ);
            }
            if p.optional {
                label.push('?');
            }
            parameters.push((start, label.len() as u32));
        }
        label.push(')');
        if !entry.return_type.is_empty() {
            label.push_str(" -> ");
            label.push_str(&entry.return_type);
        }
        Some(SignatureHelp {
            label,
            parameters,
            active_parameter: active,
            documentation: (!entry.doc.is_empty()).then(|| entry.doc.clone()),
        })
    }

    fn hover(&self, source: &str, offset: usize, analysis: &Analysis) -> Option<Hover> {
        let (line, col) = line_col(source, offset);
        let empty = SymbolIndex::default();
        let symbols = symbols_of(analysis).unwrap_or(&empty);
        let info = analyzer::hover(source, line, col, symbols, &analysis.diagnostics)?;
        Some(Hover { markdown: info.markdown, byte_range: info.byte_range })
    }

    fn definition(&self, source: &str, offset: usize, analysis: &Analysis) -> Option<Location> {
        let (line, col) = line_col(source, offset);
        let (name, _) = analyzer::identifier_at(source, line, col)?;
        let symbol = symbols_of(analysis)?.resolve(&name).first()?;
        Some(Location { path: None, range: symbol.range, byte_range: symbol.byte_range })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TokenClass::*;

    /// `(text, class)` for each token of one line lexed from `state`.
    fn lex(line: &str, state: LineState) -> (Vec<(&str, TokenClass)>, LineState) {
        let (tokens, end) = tokenize_rune_line(line, state);
        let mut last_end = 0;
        for t in &tokens {
            assert!(t.start >= last_end, "tokens overlap in {line:?}");
            last_end = t.start + t.len;
            assert!(last_end as usize <= line.len());
        }
        (
            tokens
                .iter()
                .map(|t| (&line[t.start as usize..(t.start + t.len) as usize], t.class))
                .collect(),
            end,
        )
    }

    #[test]
    fn a_function_header() {
        let (t, end) = lex("pub fn main() {", 0);
        assert_eq!(
            t,
            [("pub", Keyword), ("fn", Keyword), ("main", Function), ("(", Punctuation), (")", Punctuation), ("{", Punctuation)]
        );
        assert_eq!(end, 0);
    }

    #[test]
    fn numbers_and_ranges() {
        let (t, _) = lex("let x = 0x1F + 3.5e2 + 10u8; for i in 1..10 {}", 0);
        let numbers: Vec<_> = t.iter().filter(|(_, c)| *c == Number).map(|(s, _)| *s).collect();
        assert_eq!(numbers, ["0x1F", "3.5e2", "10u8", "1", "10"]);
        assert!(t.contains(&("..", Operator)));
        assert!(t.contains(&("for", ControlFlow)));
    }

    #[test]
    fn comments_and_doc_comments() {
        assert_eq!(lex("// note", 0).0, [("// note", Comment)]);
        assert_eq!(lex("/// doc", 0).0, [("/// doc", DocComment)]);
        assert_eq!(lex("//! inner", 0).0, [("//! inner", DocComment)]);
        assert_eq!(lex("//// rule", 0).0, [("//// rule", Comment)]);
        assert_eq!(lex("x // tail", 0).0, [("x", Variable), ("// tail", Comment)]);
    }

    #[test]
    fn block_comments_span_lines_and_nest() {
        let (t, s1) = lex("a /* one /* two", 0);
        assert_eq!(t[0], ("a", Variable));
        assert!(t[1..].iter().all(|(_, c)| *c == Comment));
        let (t, s2) = lex("still */ inside */ b", s1);
        assert_eq!(t.last(), Some(&("b", Variable)));
        assert_eq!(s2, 0, "both levels closed");
        let (_, s3) = lex("x", s1);
        assert_eq!(s3, s1, "a line inside a comment keeps the state");
    }

    #[test]
    fn strings_escapes_and_multiline() {
        let (t, _) = lex(r#"let s = "a\n\u{1F600}b";"#, 0);
        let strings: Vec<_> = t.iter().filter(|(_, c)| matches!(c, String | StringEscape)).copied().collect();
        assert_eq!(
            strings,
            [("\"", String), ("a", String), ("\\n", StringEscape), ("\\u{1F600}", StringEscape), ("b\"", String)]
        );
        let (_, s1) = lex("let s = \"abc", 0);
        let (t, s2) = lex("def\" + x", s1);
        assert_eq!(t[0], ("def\"", String));
        assert_eq!(t.last(), Some(&("x", Variable)));
        assert_eq!(s2, 0);
    }

    #[test]
    fn template_strings_interpolate() {
        let (t, end) = lex("`x ${a + 1} y`", 0);
        assert_eq!(
            t,
            [
                ("`", String),
                ("x ", String),
                ("${", Interpolation),
                ("a", Variable),
                ("+", Operator),
                ("1", Number),
                ("}", Interpolation),
                (" y`", String),
            ]
        );
        assert_eq!(end, 0);
        // A block inside the interpolation keeps its braces.
        let (t, _) = lex("`${ if c { 1 } else { 2 } }`", 0);
        assert_eq!(t.iter().filter(|(_, c)| *c == Interpolation).count(), 2);
    }

    #[test]
    fn members_methods_types_and_constants() {
        let (t, _) = lex("obj.method(1).field; Vec::new(); MAX_HP; println!(\"hi\");", 0);
        assert!(t.contains(&("method", Method)));
        assert!(t.contains(&("field", Property)));
        assert!(t.contains(&("Vec", Type)));
        assert!(t.contains(&("new", Function)));
        assert!(t.contains(&("MAX_HP", Constant)));
        assert!(t.contains(&("println", Function)));
        assert_eq!(lex("self.hp", 0).0, [("self", Builtin), (".", Punctuation), ("hp", Property)]);
    }

    #[test]
    fn attributes_chars_and_labels() {
        assert_eq!(lex("#[test]", 0).0, [("#[test]", Attribute)]);
        let (t, _) = lex(r"let c = 'a'; let n = '\n'; 'outer: loop {}", 0);
        assert!(t.contains(&("'a'", String)));
        assert!(t.contains(&("\\n", StringEscape)));
        assert!(t.contains(&("'outer", Variable)));
        assert!(t.contains(&("loop", ControlFlow)));
    }

    #[test]
    fn unicode_text_stays_on_char_boundaries() {
        let (t, _) = lex("let héllo = \"ünï\"; // ✓", 0);
        assert!(t.contains(&("héllo", Variable)));
        assert!(t.contains(&("// ✓", Comment)));
    }

    #[test]
    fn a_file_round_trips_its_line_states() {
        let src = "/* a\n b */ fn f() {\n let s = `t ${x}\n more`;\n}\n";
        let mut state = 0;
        for line in src.lines() {
            state = tokenize_rune_line(line, state).1;
        }
        assert_eq!(state, 0, "every construct closes by the end of the file");
    }

    #[test]
    fn indentation_follows_braces() {
        let s = RuneService;
        assert!(s.indent_after("fn f() {"));
        assert!(s.indent_after("let v = [ // list"));
        assert!(!s.indent_after("let x = 1; // {"));
        assert!(s.dedent_line("    }"));
        assert!(!s.dedent_line("    x"));
    }

    #[test]
    fn positions_convert_to_the_analyzers_line_and_column() {
        assert_eq!(line_col("ab\ncd", 0), (1, 1));
        assert_eq!(line_col("ab\ncd", 4), (2, 2));
    }
}
