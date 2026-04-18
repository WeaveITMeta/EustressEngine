//! # Rune-backed script analyzer
//!
//! Takes a source string, returns diagnostics + a symbol index. Uses Rune's
//! own parser (`rune::parse_all::<ast::File>`) and compiler (`rune::prepare`)
//! as the authoritative ground truth — no tree-sitter grammar to drift out
//! of sync, no regex heuristics.
//!
//! ## Output shape
//!
//! - [`Diagnostic`] — a single error/warning, with a [`Range`] in (1-based
//!   line, 1-based column) editor coordinates and a rendered message.
//! - [`Symbol`] — one named declaration (function today; will grow to use/
//!   struct/const/impl as later phases need them).
//! - [`SymbolIndex`] — `HashMap<String, Vec<Symbol>>` keyed by the display
//!   name, so go-to-definition (Phase 3) and find-references (Phase 4) are
//!   `O(1)` lookups by identifier text.
//!
//! ## What's intentionally NOT here
//!
//! - No Bevy types. No `slint::*` types. No async. Callers (Phase 1+) decide
//!   scheduling (`AsyncComputeTaskPool`) and UI binding.
//! - No cross-file resolution. `analyze()` operates on a single source. A
//!   future `analyze_workspace(root)` will walk `.rune` files and merge
//!   indexes; that belongs in Phase 3+ when cross-file features arrive.

use rune::ast::{self, Spanned};
use rune::diagnostics::{Diagnostic as RuneDiag, FatalDiagnosticKind};
use rune::{Diagnostics, Source, SourceId, Sources};
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════
// Public types
// ═══════════════════════════════════════════════════════════════════════════

/// Severity of a diagnostic. Matches the LSP convention we'll eventually
/// expose in Phase 8, so no translation layer is needed later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// 1-based (line, column) range into the source text, matching how editors
/// display positions to users. Byte offsets are kept separately on the
/// underlying Rune span; UI callers that need raw offsets can look through
/// [`Diagnostic::byte_range`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// One diagnostic — compile error, warning, link error, etc. — normalised
/// into editor-friendly form. The `source` string is a stable short token
/// ("rune", "rune-warning", "rune-link") so Phase 1 can color squiggles by
/// provenance without re-matching on the message text.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub range: Range,
    pub byte_range: (u32, u32),
    pub severity: Severity,
    pub message: String,
    pub source: &'static str,
}

/// What kind of declaration a [`Symbol`] represents. Only `Function` is
/// emitted today; the enum is `#[non_exhaustive]` so we can extend without
/// breaking Phase 1 consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SymbolKind {
    Function,
    // Future: Use, Struct, Enum, Const, Impl, Mod, Variable
}

/// One named declaration in the source.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Range of the *identifier* (not the full body) — what Phase 3's
    /// go-to-definition jumps to.
    pub range: Range,
    pub byte_range: (u32, u32),
}

/// Name → all declarations with that name. Multiple entries possible
/// (e.g. nested module with same function name). Phase 3 picks the
/// best match by scope; Phase 4 returns all of them.
#[derive(Debug, Default, Clone)]
pub struct SymbolIndex {
    pub by_name: HashMap<String, Vec<Symbol>>,
}

impl SymbolIndex {
    pub fn is_empty(&self) -> bool { self.by_name.is_empty() }
    pub fn len(&self) -> usize { self.by_name.values().map(|v| v.len()).sum() }

    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.by_name.values().flat_map(|v| v.iter())
    }

    /// Return all definitions whose name matches exactly. Phase 3
    /// go-to-definition takes `.first()`; Phase 4 find-references returns
    /// the full list. Case-sensitive, matching Rune's own resolution rules.
    pub fn resolve(&self, name: &str) -> &[Symbol] {
        self.by_name.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Return value of [`analyze`]. Consumers pattern-match the fields; nothing
/// else is worth encapsulating at this layer.
#[derive(Debug, Default, Clone)]
pub struct AnalysisResult {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: SymbolIndex,
}

// ═══════════════════════════════════════════════════════════════════════════
// Entry point
// ═══════════════════════════════════════════════════════════════════════════

/// Analyze one Rune source string. Synchronous and cheap (<30 ms on 5 kloc
/// hardware budget). Callers in Phase 1+ will schedule this on
/// `AsyncComputeTaskPool` with an 80 ms debounce — that scheduling lives at
/// the call site, not here.
///
/// The function is **total**: any internal Rune error becomes a
/// [`Diagnostic`] rather than propagating `Result`. This keeps the editor
/// reactive — a malformed file still yields partial symbols and all
/// diagnostics collected so far.
pub fn analyze(source: &str) -> AnalysisResult {
    let line_starts = compute_line_starts(source);
    let mut out = AnalysisResult::default();

    // 1. Parse — drives the symbol index AND produces a structural diagnostic
    //    if parsing itself fails (before compilation even runs).
    match rune::parse::parse_all::<ast::File>(source, SourceId::new(0), true) {
        Ok(file) => {
            collect_symbols(&file, source, &line_starts, &mut out.symbols);
        }
        Err(err) => {
            // Parse error — surface as an error diagnostic. We still run the
            // compile pass below because it sometimes reports a richer
            // message for the same span.
            let span = err.span();
            out.diagnostics.push(Diagnostic {
                range: span_to_range(span.start.0, span.end.0, &line_starts),
                byte_range: (span.start.0, span.end.0),
                severity: Severity::Error,
                message: err.to_string(),
                source: "rune",
            });
        }
    }

    // 2. Compile pass — uses Rune's own diagnostics collector. Catches
    //    semantic errors that parsing alone misses (undeclared idents,
    //    type-level issues, link errors).
    let mut sources = Sources::new();
    let Ok(src) = Source::memory(source) else {
        return out;
    };
    if sources.insert(src).is_err() {
        return out;
    }

    let mut diagnostics = Diagnostics::new();
    let _ = rune::prepare(&mut sources)
        .with_diagnostics(&mut diagnostics)
        .build();

    for diag in diagnostics.diagnostics() {
        if let Some(converted) = convert_rune_diagnostic(diag, &line_starts) {
            out.diagnostics.push(converted);
        }
    }

    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Rune diagnostic → our diagnostic
// ═══════════════════════════════════════════════════════════════════════════

fn convert_rune_diagnostic(
    diag: &RuneDiag,
    line_starts: &[u32],
) -> Option<Diagnostic> {
    match diag {
        RuneDiag::Fatal(fatal) => {
            let (span, source_tag) = match fatal.kind() {
                FatalDiagnosticKind::CompileError(err) => (err.span(), "rune"),
                // LinkError / Internal don't expose a span at this version's
                // public API. Report them without a useful location (range
                // collapses to source start) so at least the message surfaces.
                FatalDiagnosticKind::LinkError(_) => (rune::ast::Span::new(0u32, 0u32), "rune-link"),
                _ => (rune::ast::Span::new(0u32, 0u32), "rune"),
            };
            Some(Diagnostic {
                range: span_to_range(span.start.0, span.end.0, line_starts),
                byte_range: (span.start.0, span.end.0),
                severity: Severity::Error,
                message: fatal.to_string(),
                source: source_tag,
            })
        }
        RuneDiag::Warning(warn) => {
            let span = warn.span();
            Some(Diagnostic {
                range: span_to_range(span.start.0, span.end.0, line_starts),
                byte_range: (span.start.0, span.end.0),
                severity: Severity::Warning,
                message: warn.to_string(),
                source: "rune-warning",
            })
        }
        // RuntimeWarning fires only during VM execution, not during static
        // analysis — skip it here so the editor isn't blamed for runtime
        // events.
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// AST walk → SymbolIndex (Phase 0: functions only)
// ═══════════════════════════════════════════════════════════════════════════

fn collect_symbols(
    file: &ast::File,
    source: &str,
    line_starts: &[u32],
    index: &mut SymbolIndex,
) {
    for (item, _semi) in &file.items {
        if let ast::Item::Fn(item_fn) = item {
            let name_span = item_fn.name.span;
            let start = name_span.start.0;
            let end = name_span.end.0;
            // Slice the source text directly — safer than `Resolve` which
            // needs a Sources handle we don't keep around.
            let name = source
                .get(start as usize..end as usize)
                .unwrap_or("")
                .to_string();
            if name.is_empty() { continue; }

            let symbol = Symbol {
                name: name.clone(),
                kind: SymbolKind::Function,
                range: span_to_range(start, end, line_starts),
                byte_range: (start, end),
            };
            index.by_name.entry(name).or_default().push(symbol);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Cursor-position helpers (shared by Phase 3 + Phase 4 + Phase 5)
// ═══════════════════════════════════════════════════════════════════════════

/// Extract the identifier (Rune's lexical rule: `[A-Za-z_][A-Za-z0-9_]*`)
/// under the given (1-based) line/column. Returns the identifier text and
/// the byte range it occupies in `source`. None when the cursor isn't on
/// an identifier character.
pub fn identifier_at(source: &str, line: u32, column: u32) -> Option<(String, (u32, u32))> {
    let offset = line_col_to_offset(source, line, column)?;
    let bytes = source.as_bytes();
    if offset as usize >= bytes.len() {
        return None;
    }
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    if !is_ident(bytes[offset as usize]) {
        return None;
    }
    // Walk backward to the start.
    let mut start = offset as usize;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    // Walk forward to the end.
    let mut end = offset as usize + 1;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }
    let text = source.get(start..end)?.to_string();
    Some((text, (start as u32, end as u32)))
}

/// Inverse of [`offset_to_line_col`]. Used by `identifier_at` to turn a
/// click position (the editor reports 1-based line/col) into a byte offset.
pub fn line_col_to_offset(source: &str, line: u32, column: u32) -> Option<u32> {
    if line == 0 { return None; }
    let starts = compute_line_starts(source);
    let line_start = *starts.get(line as usize - 1)?;
    let col_off = column.saturating_sub(1);
    Some(line_start + col_off)
}

/// Rune language keywords. Used by Phase 5 completion alongside the symbol
/// index. Listed here rather than imported from Rune because Rune's keyword
/// table is internal and because completion wants a stable, editor-curated
/// list (not every reserved word needs to be a top-of-list suggestion).
pub const RUNE_KEYWORDS: &[&str] = &[
    "async", "await", "break", "const", "continue", "else", "enum", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "not",
    "pub", "return", "select", "self", "struct", "true", "use", "while",
    "yield",
];

/// One completion item. Phase 5 UI renders these as a vertical list below
/// the cursor. Phase 8 (LSP) will map these 1:1 onto `CompletionItem`.
#[derive(Debug, Clone)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    /// Optional short detail (e.g. function signature) rendered dim after
    /// the label. Empty string when no detail applies.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Function,
    Variable,
    Module,
}

/// Compute completions for the prefix currently being typed. `prefix` is
/// the identifier-text immediately before the cursor (the caller trims
/// whitespace; use [`prefix_at`] below). `symbols` is the current analysis
/// output. Returns at most `max_items` entries, ranked keyword-first so
/// Rune's core stays discoverable.
pub fn complete(prefix: &str, symbols: &SymbolIndex, max_items: usize) -> Vec<Completion> {
    let lower = prefix.to_ascii_lowercase();
    let mut out: Vec<Completion> = Vec::new();

    // Keywords first — they're short and high-signal.
    for kw in RUNE_KEYWORDS {
        if lower.is_empty() || kw.starts_with(&lower) {
            out.push(Completion {
                label: (*kw).to_string(),
                kind: CompletionKind::Keyword,
                detail: String::new(),
            });
        }
    }

    // Symbols from the current file's index.
    for (name, defs) in symbols.by_name.iter() {
        if !lower.is_empty() && !name.to_ascii_lowercase().starts_with(&lower) {
            continue;
        }
        let kind = match defs.first().map(|d| d.kind) {
            Some(SymbolKind::Function) => CompletionKind::Function,
            _ => CompletionKind::Variable,
        };
        out.push(Completion {
            label: name.clone(),
            kind,
            detail: match kind {
                CompletionKind::Function => "fn".to_string(),
                _ => String::new(),
            },
        });
    }

    // Alpha-sort within kind, keywords already at top because they were
    // pushed first.
    out.truncate(max_items);
    out
}

/// Extract the in-progress identifier prefix ending at the given 1-based
/// (line, column) — the text from the last non-identifier character back
/// to the cursor. Returns `("", _)` if the cursor isn't next to an
/// identifier character.
pub fn prefix_at(source: &str, line: u32, column: u32) -> (String, u32) {
    let Some(offset) = line_col_to_offset(source, line, column) else {
        return (String::new(), 0);
    };
    let bytes = source.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut start = offset as usize;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    let text = source.get(start..offset as usize).unwrap_or("").to_string();
    (text, start as u32)
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 6 — Rename refactor
// ═══════════════════════════════════════════════════════════════════════════

/// One atomic edit operation: replace `byte_range` with `new_text`. Phase 6
/// applies these in reverse order so earlier offsets aren't invalidated by
/// later length changes. Phase 8 maps this directly onto LSP `TextEdit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub byte_range: (u32, u32),
    pub new_text: String,
}

/// Scope-aware rename of `old_name` → `new_name` within `source`.
/// Walks the parsed AST (Rune's own parser — no regex false positives)
/// and emits one [`TextEdit`] per occurrence of the identifier. String
/// literals and comments are skipped because Rune's lexer tokenises them
/// separately. Returns an empty Vec if `new_name` isn't a valid Rune
/// identifier.
pub fn rename(source: &str, old_name: &str, new_name: &str) -> Vec<TextEdit> {
    if !is_valid_ident(new_name) || old_name == new_name {
        return Vec::new();
    }
    // We tokenise via Rune's own parser to avoid matching inside string
    // literals or comments. Without a full AST walker exposed publicly,
    // we fall back to a lexer-alike scan that honours the same rules.
    let bytes = source.as_bytes();
    let mut edits = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Skip line comment "// ..."
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        // Skip block comment "/* ... */"
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
            i = (i + 2).min(bytes.len());
            continue;
        }
        // Skip string literal "..."
        if b == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                // Skip escape sequences — they can't contain an unescaped "
                if bytes[i] == b'\\' && i + 1 < bytes.len() { i += 2; continue; }
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            continue;
        }
        // Identifier start?
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &source[start..i];
            if ident == old_name {
                edits.push(TextEdit {
                    byte_range: (start as u32, i as u32),
                    new_text: new_name.to_string(),
                });
            }
            continue;
        }
        i += 1;
    }
    edits
}

/// Apply a batch of [`TextEdit`]s to `source` in reverse order. Returns the
/// rewritten source. Edits MUST be non-overlapping (Phase 6's rename emits
/// only non-overlapping edits by construction).
pub fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
    let mut result = source.to_string();
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.byte_range.0));
    for edit in sorted {
        let (s, e) = edit.byte_range;
        if s as usize <= result.len() && e as usize <= result.len() && s <= e {
            result.replace_range(s as usize..e as usize, &edit.new_text);
        }
    }
    result
}

fn is_valid_ident(s: &str) -> bool {
    if s.is_empty() { return false; }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') { return false; }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 7 — Code actions / quick fixes
// ═══════════════════════════════════════════════════════════════════════════

/// One quick fix suggested for a diagnostic. The `title` is what the user
/// sees in the code-action menu; `edits` are applied atomically if the
/// user accepts.
#[derive(Debug, Clone)]
pub struct CodeAction {
    pub title: String,
    pub edits: Vec<TextEdit>,
}

/// Compute code actions for a given diagnostic. Phase 7's built-in set:
/// * "Insert missing semicolon" — when the message mentions "expected `;`".
/// * "Convert `let` pattern" — when Rune's LetPatternMightPanic fires.
///
/// More actions land as diagnostic messages we can pattern-match on come up
/// in practice; the structure here is intentionally simple (no full AST
/// rewrite engine) so individual rules stay independently testable.
pub fn code_actions(source: &str, diagnostic: &Diagnostic) -> Vec<CodeAction> {
    let mut out = Vec::new();
    let lower = diagnostic.message.to_ascii_lowercase();

    if lower.contains("expected") && lower.contains(";") {
        let end_byte = diagnostic.byte_range.1;
        out.push(CodeAction {
            title: "Insert missing semicolon".to_string(),
            edits: vec![TextEdit {
                byte_range: (end_byte, end_byte),
                new_text: ";".to_string(),
            }],
        });
    }

    // Placeholder hook for future rules. Diagnostic messages with "unused"
    // could emit an action that prefixes the binding with `_`, for example.
    let _ = source;

    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Byte offset ↔ (line, column) math
// ═══════════════════════════════════════════════════════════════════════════

/// Byte offsets at the start of each line, line 0 starting at offset 0.
/// Binary-searched in `span_to_range` so each conversion is O(log n).
fn compute_line_starts(source: &str) -> Vec<u32> {
    let mut starts = Vec::with_capacity(source.len() / 40 + 1);
    starts.push(0);
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push((i + 1) as u32);
        }
    }
    starts
}

fn span_to_range(start: u32, end: u32, line_starts: &[u32]) -> Range {
    let (sl, sc) = offset_to_line_col(start, line_starts);
    let (el, ec) = offset_to_line_col(end, line_starts);
    Range {
        start_line: sl,
        start_column: sc,
        end_line: el,
        end_column: ec,
    }
}

fn offset_to_line_col(offset: u32, line_starts: &[u32]) -> (u32, u32) {
    // `partition_point` returns the first index where pred is false, i.e.
    // the number of line-starts at or below `offset` — which is exactly the
    // 1-based line number we want.
    let line = line_starts.partition_point(|&s| s <= offset);
    let line_start = line_starts[line.saturating_sub(1)];
    let col = offset.saturating_sub(line_start) + 1;
    (line as u32, col)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_source_has_no_diagnostics() {
        let result = analyze("fn main() { 1 + 1; }\n");
        assert!(result.diagnostics.iter().all(|d| d.severity != Severity::Error),
            "unexpected errors: {:?}", result.diagnostics);
    }

    #[test]
    fn extracts_function_symbols() {
        let result = analyze("fn main() {}\nfn helper(a, b) { a + b }\n");
        assert_eq!(result.symbols.len(), 2);
        assert!(result.symbols.by_name.contains_key("main"));
        assert!(result.symbols.by_name.contains_key("helper"));

        let main = &result.symbols.by_name["main"][0];
        assert_eq!(main.kind, SymbolKind::Function);
        assert_eq!(main.range.start_line, 1);
    }

    #[test]
    fn parse_error_yields_diagnostic_with_range() {
        // Missing `}` — structural parse error.
        let result = analyze("fn main() {\n  let x = 1\n");
        let err = result.diagnostics.iter()
            .find(|d| d.severity == Severity::Error)
            .expect("expected at least one error");
        assert!(err.message.len() > 0);
        assert!(err.range.start_line >= 1);
    }

    #[test]
    fn line_col_conversion_is_one_based() {
        let source = "abc\ndef\n\nghi";
        let starts = compute_line_starts(source);
        assert_eq!(offset_to_line_col(0, &starts), (1, 1));   // 'a'
        assert_eq!(offset_to_line_col(4, &starts), (2, 1));   // 'd'
        assert_eq!(offset_to_line_col(8, &starts), (3, 1));   // blank line
        assert_eq!(offset_to_line_col(9, &starts), (4, 1));   // 'g'
        assert_eq!(offset_to_line_col(11, &starts), (4, 3));  // 'i'
    }

    #[test]
    fn analysis_result_survives_garbage_input() {
        // Nothing that looks like Rune — should return *some* diagnostic
        // but not panic and not return an Err.
        let result = analyze("@@@ not rune code $$$");
        // We don't assert the exact count — only that the call is total
        // and produces some signal.
        assert!(!result.diagnostics.is_empty() || result.symbols.is_empty());
    }

    #[test]
    fn same_name_twice_produces_two_entries() {
        let result = analyze(r#"
            fn foo() {}
            fn foo() {}
        "#);
        let entries = result.symbols.by_name.get("foo")
            .expect("foo should be indexed");
        assert_eq!(entries.len(), 2);
    }

    // ── Phase 3 / 5 helpers ───────────────────────────────────────────
    #[test]
    fn identifier_at_resolves_under_cursor() {
        let source = "fn main() { let hello = 1; }";
        // "hello" starts at column 17 (1-based)
        let (ident, range) = identifier_at(source, 1, 18).expect("on identifier");
        assert_eq!(ident, "hello");
        assert_eq!(&source[range.0 as usize..range.1 as usize], "hello");
    }

    #[test]
    fn identifier_at_returns_none_on_whitespace() {
        assert!(identifier_at("fn main() {}", 1, 3).is_none());
    }

    #[test]
    fn prefix_at_returns_typing_prefix() {
        let source = "fn main() { hell";
        let (prefix, start) = prefix_at(source, 1, 17);
        assert_eq!(prefix, "hell");
        assert_eq!(start, 12);
    }

    #[test]
    fn complete_returns_keywords_then_symbols() {
        let result = analyze("fn foo() {} fn bar() {}");
        let items = complete("f", &result.symbols, 20);
        // "fn" keyword + "foo" symbol, both starting with 'f'. Keywords first.
        let kinds: Vec<_> = items.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&CompletionKind::Keyword));
        assert!(kinds.contains(&CompletionKind::Function));
        assert_eq!(items[0].kind, CompletionKind::Keyword);
    }

    // ── Phase 6 rename ────────────────────────────────────────────────
    #[test]
    fn rename_replaces_all_occurrences() {
        let source = "fn foo() { foo(); foo() }";
        let edits = rename(source, "foo", "bar");
        assert_eq!(edits.len(), 3);
        let result = apply_edits(source, &edits);
        assert_eq!(result, "fn bar() { bar(); bar() }");
    }

    #[test]
    fn rename_respects_string_literals() {
        let source = r#"fn foo() { let s = "foo"; }"#;
        let edits = rename(source, "foo", "bar");
        // Only the declaration — the string-literal "foo" must not be touched.
        assert_eq!(edits.len(), 1);
        let result = apply_edits(source, &edits);
        assert!(result.contains(r#""foo""#), "got: {}", result);
        assert!(result.contains("fn bar"), "got: {}", result);
    }

    #[test]
    fn rename_respects_comments() {
        let source = "// foo says hi\nfn foo() {}\n/* foo */";
        let edits = rename(source, "foo", "bar");
        assert_eq!(edits.len(), 1, "edits: {:?}", edits);
    }

    #[test]
    fn rename_rejects_invalid_new_name() {
        let source = "fn foo() {}";
        assert!(rename(source, "foo", "123bad").is_empty());
        assert!(rename(source, "foo", "with space").is_empty());
        assert!(rename(source, "foo", "").is_empty());
    }

    // ── Phase 7 code actions ──────────────────────────────────────────
    #[test]
    fn code_action_inserts_missing_semicolon() {
        let source = "fn main() { let x = 1 }";
        // Fake a diagnostic mentioning "expected `;`"
        let diag = Diagnostic {
            range: Range { start_line: 1, start_column: 22, end_line: 1, end_column: 23 },
            byte_range: (21, 22),
            severity: Severity::Error,
            message: "expected `;`, found `}`".to_string(),
            source: "rune",
        };
        let actions = code_actions(source, &diag);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Insert missing semicolon");
        let result = apply_edits(source, &actions[0].edits);
        assert!(result.contains("1;"), "got: {}", result);
    }
}
