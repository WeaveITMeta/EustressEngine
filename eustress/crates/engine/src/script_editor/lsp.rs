//! # LSP adapter — expose the in-process analyzer to external editors
//!
//! Feature-gated behind `lsp`. The `eustress-lsp` binary loads this module
//! and runs a stdio `tower-lsp` server; external IDEs (Windsurf, VS Code,
//! Zed, Neovim via `nvim-lspconfig`) connect as they would to any other
//! language server.
//!
//! ## Architecture
//!
//! This file contains zero novel language-intelligence logic. Every
//! method on [`EustressLsp`] delegates to the same [`analyzer`] functions
//! the in-process editor uses. Type mapping is one-directional:
//!
//! ```text
//!   LSP request → lsp_types::Position → (1-based line, col)
//!                                     → analyzer API
//!                                     → analyzer::Diagnostic / Symbol / etc.
//!                                     → lsp_types response
//!                                     → client
//! ```
//!
//! ## Capabilities advertised
//!
//! - `textDocument/publishDiagnostics` (push on every `did_change`)
//! - `textDocument/definition`
//! - `textDocument/references`
//! - `textDocument/hover`
//! - `textDocument/completion`
//! - `textDocument/rename`
//! - `textDocument/codeAction`
//! - `textDocument/documentSymbol`
//! - `textDocument/semanticTokens` — deferred; the token-span renderer
//!   lives in our own editor and the mapping isn't free.
//!
//! ## Why it stays thin
//!
//! Everything that matters is in [`super::analyzer`]. If Rune gains a new
//! syntax tomorrow, we update analyzer.rs and every LSP method gets the
//! improvement automatically. The LSP layer's only job is protocol
//! translation — there's no second source of truth.

use super::analyzer;
use super::workspace::WorkspaceIndex;
use dashmap::DashMap;
// `Watcher` trait is required to call `.watch(...)` on the debouncer's
// inner `RecommendedWatcher` — without the trait in scope the method
// isn't found on the concrete type.
use notify::Watcher;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// The server instance. Holds one document buffer per open URI so
/// per-request analysis is O(parse) rather than O(re-read from disk),
/// plus a workspace-wide symbol index for cross-file goto-def, find-
/// references, and rename operations.
pub struct EustressLsp {
    pub client: Client,
    /// URI → current source text. Populated on `did_open`, mutated on
    /// `did_change`, removed on `did_close`.
    pub docs: Arc<DashMap<Url, String>>,
    /// Cross-file symbol index rooted at the Universe containing the
    /// LSP session. Populated lazily on `initialize` (once we know
    /// rootUri) and refreshed on `did_save`. Wrapped in `RwLock` so
    /// handlers that only need to read can run concurrently.
    pub workspace: Arc<RwLock<WorkspaceIndex>>,
    /// Root directory for synthetic native-API documentation files.
    /// Each native function / type gets one lazily-materialised
    /// `.md` file here so goto-def can return a valid `Location`
    /// that opens in any LSP client without custom URI-scheme
    /// registration. Cleared on `shutdown`.
    pub synthetic_docs_root: PathBuf,
    /// Filesystem watcher. Held so the debouncer thread lives as long
    /// as the LSP session; dropped on shutdown. Wrapped in `Mutex`
    /// because `Debouncer` isn't `Sync` but we only read/write it from
    /// `initialize` + `shutdown`.
    pub watcher: Arc<std::sync::Mutex<Option<
        notify_debouncer_full::Debouncer<notify::RecommendedWatcher, notify_debouncer_full::FileIdMap>
    >>>,
}

impl EustressLsp {
    pub fn new(client: Client) -> Self {
        // One root per process — multiple concurrent LSP servers (e.g.
        // split-screen Universes) each own a unique subdir so they
        // don't clobber each other.
        let synthetic_docs_root = std::env::temp_dir()
            .join("eustress-lsp-api")
            .join(format!("{}", std::process::id()));
        Self {
            client,
            docs: Arc::new(DashMap::new()),
            workspace: Arc::new(RwLock::new(WorkspaceIndex::default())),
            synthetic_docs_root,
            watcher: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Start a filesystem watcher rooted at `universe` that refreshes
    /// [`Self::workspace`] whenever a `.rune` file changes on disk.
    /// Covers the cross-file-staleness case that `didSave` alone
    /// misses (git checkout, external editor, mass rename). Idempotent
    /// — replaces any prior watcher so switching Universes doesn't
    /// leak handles.
    fn start_watcher(&self, universe: PathBuf) {
        let workspace = Arc::clone(&self.workspace);
        let debouncer = notify_debouncer_full::new_debouncer(
            std::time::Duration::from_millis(150),
            None,
            move |res: notify_debouncer_full::DebounceEventResult| {
                let events = match res {
                    Ok(ev) => ev,
                    Err(_) => return,
                };
                for event in events {
                    for path in &event.event.paths {
                        // Only `.rune` files invalidate the workspace
                        // index; everything else is noise.
                        if path.extension().and_then(|e| e.to_str()) != Some("rune") {
                            continue;
                        }
                        if let Ok(mut w) = workspace.write() {
                            if path.exists() {
                                w.update_file(path);
                            } else {
                                // File was deleted — a full refresh is
                                // the simplest way to prune stale
                                // entries across the index.
                                w.refresh_if_stale();
                            }
                        }
                    }
                }
            },
        );
        if let Ok(mut d) = debouncer {
            // Watch the Universe recursively so nested Spaces are
            // covered. Debouncer coalesces save-as-rename bursts from
            // editors (VS Code writes `.tmp` then renames).
            if d.watcher()
                .watch(&universe, notify::RecursiveMode::Recursive)
                .is_ok()
            {
                if let Ok(mut slot) = self.watcher.lock() {
                    *slot = Some(d);
                }
            }
        }
    }

    /// Materialise a read-only markdown file describing a native API
    /// entry and return a `Location` pointing at it. Idempotent — the
    /// file is only written the first time goto-def asks for it.
    /// Returns `None` if the temp dir can't be created or the file
    /// can't be written (in which case the caller falls through to
    /// "no definition available").
    fn synthetic_api_location(
        &self,
        entry: &crate::workshop::api_reference::ApiEntry,
    ) -> Option<Location> {
        let _ = std::fs::create_dir_all(&self.synthetic_docs_root);
        let path = self.synthetic_docs_root.join(format!("{}.md", entry.name));
        if !path.exists() {
            let header = format!(
                "<!-- Eustress native API — auto-generated, read-only.\n     \
                 Source: rune_ecs_module.rs + SCRIPTING_API_CHECKLIST.md. -->\n\n\
                 # `{}`\n\n",
                entry.name,
            );
            let body = analyzer::render_api_hover(entry);
            if std::fs::write(&path, format!("{}{}", header, body)).is_err() {
                return None;
            }
        }
        Some(Location {
            uri: Url::from_file_path(&path).ok()?,
            range: Range::default(),
        })
    }

    /// Re-analyze `uri` and push diagnostics to the client. Called on every
    /// `did_change` / `did_open`. Analysis is synchronous here — the LSP
    /// runtime runs us on its own task pool, so we don't need to hop.
    async fn refresh(&self, uri: Url) {
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return };
        let result = analyzer::analyze(&source);
        let diags: Vec<Diagnostic> = result.diagnostics.iter()
            .map(|d| to_lsp_diagnostic(d))
            .collect();
        self.client
            .publish_diagnostics(uri, diags, None)
            .await;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Type conversions
// ═══════════════════════════════════════════════════════════════════════════

fn to_lsp_range(r: analyzer::Range) -> Range {
    Range {
        // LSP uses 0-based line/character; our analyzer's Range is 1-based.
        start: Position {
            line: r.start_line.saturating_sub(1),
            character: r.start_column.saturating_sub(1),
        },
        end: Position {
            line: r.end_line.saturating_sub(1),
            character: r.end_column.saturating_sub(1),
        },
    }
}

fn to_lsp_diagnostic(d: &analyzer::Diagnostic) -> Diagnostic {
    Diagnostic {
        range: to_lsp_range(d.range),
        severity: Some(match d.severity {
            analyzer::Severity::Error   => DiagnosticSeverity::ERROR,
            analyzer::Severity::Warning => DiagnosticSeverity::WARNING,
            analyzer::Severity::Info    => DiagnosticSeverity::INFORMATION,
            analyzer::Severity::Hint    => DiagnosticSeverity::HINT,
        }),
        code: None,
        code_description: None,
        source: Some(d.source.to_string()),
        message: d.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}

fn lsp_pos_to_line_col(p: Position) -> (u32, u32) {
    // LSP is 0-based; analyzer APIs are 1-based.
    (p.line + 1, p.character + 1)
}

/// Turn the `InitializeParams` handed to us by the LSP client into the
/// Universe root we should index. Preference order:
///
///   1. `workspaceFolders[0].uri` — explicit per-window root (VS Code's
///      primary mechanism since 3.6.0).
///   2. `rootUri` (deprecated but still widely used).
///   3. `rootPath` (legacy fallback).
///
/// Whichever we get, we walk up to find the nearest ancestor that
/// contains a `Spaces/` subdirectory — so opening a file deep inside
/// a Space still indexes the whole Universe. `None` if the path
/// doesn't look like it's under a Universe, in which case the
/// workspace index stays empty and goto-def falls back to in-file.
fn resolve_workspace_root(params: &InitializeParams) -> Option<PathBuf> {
    let start = params
        .workspace_folders
        .as_ref()
        .and_then(|fs| fs.first())
        .and_then(|f| f.uri.to_file_path().ok())
        .or_else(|| {
            #[allow(deprecated)]
            params
                .root_uri
                .as_ref()
                .and_then(|u| u.to_file_path().ok())
                .or_else(|| {
                    #[allow(deprecated)]
                    params.root_path.as_ref().map(PathBuf::from)
                })
        })?;
    walk_up_for_universe(&start)
}

// ───────────────────────────────────────────────────────────────────
// Semantic tokens
// ───────────────────────────────────────────────────────────────────

/// LSP semantic-token legend. Indices into this array are sent to the
/// client; keep the order stable once published so existing clients
/// don't re-colour wrongly after a server upgrade. Returns by value
/// rather than `&'static [...]` because `SemanticTokenType` constants
/// aren't `const`-eligible in the current `tower-lsp` build, so a
/// static initialiser fails to type-check.
fn semantic_token_types() -> Vec<SemanticTokenType> {
    // Subset tailored to Rune — keywords, user-defined functions,
    // Eustress API calls (set apart from user fns for visual distinction),
    // strings, numbers, comments, types.
    vec![
        SemanticTokenType::KEYWORD,    // 0
        SemanticTokenType::FUNCTION,   // 1 — user-defined functions
        SemanticTokenType::METHOD,     // 2 — Eustress API functions
        SemanticTokenType::TYPE,       // 3 — Eustress API types (Vector3, etc.)
        SemanticTokenType::STRING,     // 4
        SemanticTokenType::NUMBER,     // 5
        SemanticTokenType::COMMENT,    // 6
        SemanticTokenType::VARIABLE,   // 7
    ]
}

/// Token type indices — match the legend above. A tiny newtype would
/// be cleaner but the constants' call sites are already obvious.
const TT_KEYWORD: u32  = 0;
const TT_FUNCTION: u32 = 1;
const TT_API_FN: u32   = 2;
const TT_API_TYPE: u32 = 3;
const TT_STRING: u32   = 4;
const TT_NUMBER: u32   = 5;
const TT_COMMENT: u32  = 6;

/// Raw semantic token before delta-encoding. Sorted by byte offset so
/// the encoder can compute line/character deltas in one pass.
struct RawToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
}

/// Compute semantic tokens for a Rune source. Returns the LSP-encoded
/// delta format (5 integers per token: deltaLine, deltaStart, length,
/// tokenType, tokenModifiers).
///
/// The scanner is hand-rolled rather than AST-based so it tolerates
/// partially-broken input (common while typing). For each position we
/// classify the token: keyword, API function/type (catalog lookup),
/// user function (local symbol index), string, number, or comment.
fn compute_semantic_tokens(source: &str) -> Vec<SemanticToken> {
    let bytes = source.as_bytes();
    let mut raw: Vec<RawToken> = Vec::new();

    // Line starts so we can convert byte offsets back to line/character.
    let mut line_starts: Vec<usize> = vec![0];
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let byte_to_pos = |offset: usize| -> (u32, u32) {
        let line = line_starts.partition_point(|&s| s <= offset).saturating_sub(1);
        let line_start = line_starts[line];
        // LSP semantic tokens use UTF-16 code units, but for ASCII-heavy
        // Rune source this is identical to byte offsets; we accept the
        // small loss for non-ASCII identifiers rather than walking
        // character-by-character on every token.
        (line as u32, (offset - line_start) as u32)
    };

    // Build a fast lookup of catalog names once per request so the scan
    // below is O(N) over source bytes with O(1) classification.
    let mut api_fn_names: std::collections::HashSet<&'static str> =
        std::collections::HashSet::new();
    let mut api_type_names: std::collections::HashSet<&'static str> =
        std::collections::HashSet::new();
    for entry in analyzer::api_starts_with("") {
        match entry.name.chars().next() {
            Some(c) if c.is_ascii_uppercase() => {
                api_type_names.insert(entry.name.as_str());
            }
            _ => {
                api_fn_names.insert(entry.name.as_str());
            }
        }
    }

    // Symbol index for user-defined functions.
    let result = analyzer::analyze(source);
    let user_fns: std::collections::HashSet<String> = result
        .symbols
        .iter()
        .filter(|s| matches!(s.kind, analyzer::SymbolKind::Function))
        .map(|s| s.name.clone())
        .collect();

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        // Line comment — //...
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            let (line, col) = byte_to_pos(start);
            raw.push(RawToken {
                line,
                start_char: col,
                length: (i - start) as u32,
                token_type: TT_COMMENT,
            });
            continue;
        }

        // Block comment — /* ... */ (no nesting, matches Rune's lexer).
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
            }
            // Block comments can span lines — emit one token per line
            // so the client highlights them correctly.
            let mut cur = start;
            while cur < i {
                let line_end = match bytes[cur..i].iter().position(|&b| b == b'\n') {
                    Some(off) => cur + off,
                    None => i,
                };
                let (line, col) = byte_to_pos(cur);
                raw.push(RawToken {
                    line,
                    start_char: col,
                    length: (line_end - cur) as u32,
                    token_type: TT_COMMENT,
                });
                cur = line_end.saturating_add(1);
                if cur > i { break; }
            }
            continue;
        }

        // String literal — "..." with simple escape handling.
        if b == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < bytes.len() {
                i += 1;
            }
            let (line, col) = byte_to_pos(start);
            // Strings can span lines via \n escapes — we still emit one
            // token per line for the same reason block comments do.
            let mut cur = start;
            while cur < i {
                let line_end = match bytes[cur..i].iter().position(|&b| b == b'\n') {
                    Some(off) => cur + off,
                    None => i,
                };
                let (l, c) = byte_to_pos(cur);
                raw.push(RawToken {
                    line: l,
                    start_char: c,
                    length: (line_end - cur) as u32,
                    token_type: TT_STRING,
                });
                cur = line_end.saturating_add(1);
                if cur > i { break; }
            }
            let _ = (line, col); // silence unused warning on the branch
            continue;
        }

        // Number literal — digits + optional decimal + optional
        // exponent. Simple pattern; good enough for Rune.
        if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'_') {
                i += 1;
            }
            let (line, col) = byte_to_pos(start);
            raw.push(RawToken {
                line,
                start_char: col,
                length: (i - start) as u32,
                token_type: TT_NUMBER,
            });
            continue;
        }

        // Identifier / keyword / catalog lookup.
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let text = &source[start..i];
            let token_type = if analyzer::RUNE_KEYWORDS.contains(&text) {
                TT_KEYWORD
            } else if api_fn_names.contains(text) {
                TT_API_FN
            } else if api_type_names.contains(text) {
                TT_API_TYPE
            } else if user_fns.contains(text) {
                TT_FUNCTION
            } else {
                i = i; // no highlight — skip emitting a token rather than
                       // flooding the client with VARIABLE tokens for
                       // every identifier, which crushes perf and
                       // clashes with the TextMate grammar.
                continue;
            };
            let (line, col) = byte_to_pos(start);
            raw.push(RawToken {
                line,
                start_char: col,
                length: (i - start) as u32,
                token_type,
            });
            continue;
        }

        i += 1;
    }

    // Delta-encode to the LSP wire format. Sort defensively by
    // (line, start_char) in case any branch emitted out of order.
    raw.sort_by_key(|t| (t.line, t.start_char));
    let mut out: Vec<SemanticToken> = Vec::with_capacity(raw.len());
    let mut prev_line: u32 = 0;
    let mut prev_char: u32 = 0;
    for t in raw {
        let delta_line = t.line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            t.start_char.saturating_sub(prev_char)
        } else {
            t.start_char
        };
        out.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: t.token_type,
            token_modifiers_bitset: 0,
        });
        prev_line = t.line;
        prev_char = t.start_char;
    }
    out
}

/// If the cursor sits inside a double-quoted string literal that is
/// the first argument to `get_sim_value`, `set_sim_value`, or
/// `list_sim_values`, return the key text — stripped of its quotes.
/// Used by Phase-E hover to append live values from the runtime
/// snapshot.
///
/// Intentionally a small surface-level scan, not a full AST walk:
/// the hover path runs on every cursor movement and the regex would
/// blow past an 80 ms budget on a large file. We scan backward a
/// bounded number of bytes looking for one of the known getters, then
/// forward-match a quoted string. Good enough for the common case;
/// misses exotic formatting (split across lines inside parens etc.).
fn sim_key_at_cursor(source: &str, line: u32, col: u32) -> Option<String> {
    const TARGETS: &[&str] = &["get_sim_value", "set_sim_value", "list_sim_values"];
    const LOOKBACK_BYTES: usize = 256;

    let offset = analyzer::line_col_to_offset(source, line, col)? as usize;
    let bytes = source.as_bytes();

    // Find the opening quote that encloses the cursor. If the cursor
    // isn't inside a string literal, bail.
    let (quote_start, key_start) = {
        // Walk back to the nearest unescaped `"`.
        let mut i = offset;
        while i > 0 {
            if bytes[i - 1] == b'\n' || bytes[i - 1] == b';' {
                return None; // crossed a statement boundary
            }
            if bytes[i - 1] == b'"' {
                break;
            }
            i -= 1;
        }
        if i == 0 || bytes[i - 1] != b'"' {
            return None;
        }
        (i - 1, i)
    };

    // Forward-scan from `key_start` to the closing quote.
    let mut key_end = key_start;
    while key_end < bytes.len() && bytes[key_end] != b'"' && bytes[key_end] != b'\n' {
        key_end += 1;
    }
    if key_end >= bytes.len() || bytes[key_end] != b'"' {
        return None;
    }
    if offset > key_end {
        return None; // cursor was past the string
    }

    // Walk backwards from the opening quote past whitespace + `(` until
    // we find an identifier. If that identifier is one of our targets,
    // return the key text.
    let preamble_start = quote_start.saturating_sub(LOOKBACK_BYTES);
    let preamble = &source[preamble_start..quote_start];
    let trimmed = preamble.trim_end();
    let without_paren = trimmed.trim_end_matches(|c: char| c == '(' || c.is_whitespace());
    for target in TARGETS {
        if without_paren.ends_with(target) {
            let key = &source[key_start..key_end];
            return Some(key.to_string());
        }
    }
    None
}

/// Like [`sim_key_at_cursor`] but intended for completion — the cursor
/// is inside an *incomplete* string literal where only the opening
/// quote exists. Returns the already-typed prefix (possibly empty).
fn sim_key_prefix_at_cursor(source: &str, line: u32, col: u32) -> Option<String> {
    const TARGETS: &[&str] = &["get_sim_value", "set_sim_value", "list_sim_values"];
    const LOOKBACK_BYTES: usize = 256;

    let offset = analyzer::line_col_to_offset(source, line, col)? as usize;
    let bytes = source.as_bytes();

    // Walk back looking for the opening quote; stop on statement
    // boundaries (`;` or `\n`) — those mean we're not inside a string.
    let mut i = offset;
    let mut newlines_seen = 0;
    while i > 0 {
        match bytes[i - 1] {
            b';' => return None,
            b'\n' => {
                newlines_seen += 1;
                if newlines_seen > 1 { return None; }
            }
            b'"' => break,
            _ => {}
        }
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'"' {
        return None;
    }
    let key_start = i;
    let prefix = &source[key_start..offset];

    // Preamble must end with one of the target function names.
    let preamble_start = i.saturating_sub(1 + LOOKBACK_BYTES);
    let preamble = &source[preamble_start..i.saturating_sub(1)];
    let trimmed = preamble.trim_end();
    let without_paren = trimmed.trim_end_matches(|c: char| c == '(' || c.is_whitespace());
    for target in TARGETS {
        if without_paren.ends_with(target) {
            return Some(prefix.to_string());
        }
    }
    None
}

/// Turn a concrete filesystem path back into an `lsp_types::Url`.
/// Silently drops paths that can't be converted (usually non-UTF-8),
/// letting callers fall through rather than propagating an error
/// through an LSP response.
fn file_path_to_uri(path: &std::path::Path) -> Option<Url> {
    Url::from_file_path(path).ok()
}

fn walk_up_for_universe(start: &std::path::Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    for _ in 0..16 {
        if cur.join("Spaces").is_dir() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
    None
}

/// Byte offset (from analyzer TextEdit) back to an LSP Range by re-deriving
/// line/col against the current source. Small helper so rename can stay
/// pure-offset internally.
fn byte_range_to_lsp(source: &str, range: (u32, u32)) -> Range {
    let line_starts: Vec<u32> = {
        let mut v = Vec::with_capacity(source.len() / 40 + 1);
        v.push(0u32);
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' { v.push((i + 1) as u32); }
        }
        v
    };
    let at = |offset: u32| {
        let line = line_starts.partition_point(|&s| s <= offset);
        let line_start = line_starts[line.saturating_sub(1)];
        Position {
            line: line.saturating_sub(1) as u32,
            character: offset.saturating_sub(line_start),
        }
    };
    Range { start: at(range.0), end: at(range.1) }
}

// ═══════════════════════════════════════════════════════════════════════════
// LanguageServer trait
// ═══════════════════════════════════════════════════════════════════════════

#[tower_lsp::async_trait]
impl LanguageServer for EustressLsp {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        // Root the workspace index at whatever the client handed us.
        // We walk up looking for a Universe (`Spaces/` subdirectory)
        // so a `.rune` opened anywhere inside a Space still indexes
        // the whole Universe, not just the opened folder.
        let root = resolve_workspace_root(&params);
        if let Some(root) = root {
            let idx = WorkspaceIndex::build(&root);
            if let Ok(mut w) = self.workspace.write() {
                *w = idx;
            }
            // Kick off a filesystem watcher so the index stays in sync
            // with external edits (git checkout, `sed -i` across the
            // tree, another editor's save). Without this the Phase-C
            // index only refreshes on `didSave` from the active client.
            self.start_watcher(root);
        }
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // `"` triggers string-literal completion inside
                    // `get_sim_value("…")`. `.` reserved for future
                    // member-access completion.
                    trigger_characters: Some(vec!["\"".into(), ".".into()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    // `(` opens the hint; `,` advances the active param.
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: Some(vec![",".into()]),
                    work_done_progress_options: Default::default(),
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: Default::default(),
                            legend: SemanticTokensLegend {
                                token_types: semantic_token_types(),
                                token_modifiers: vec![],
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "eustress-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "eustress-lsp ready")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        // Drop the filesystem watcher first — stops its background
        // thread before we remove the synthetic-docs directory so we
        // don't race deletes against inotify events.
        if let Ok(mut slot) = self.watcher.lock() {
            *slot = None;
        }
        // Clean up the per-process synthetic-API docs directory so a
        // long-running developer machine doesn't accumulate orphan
        // `<temp>/eustress-lsp-api/<pid>/` trees across crashes.
        let _ = std::fs::remove_dir_all(&self.synthetic_docs_root);
        Ok(())
    }

    // ── Document lifecycle ────────────────────────────────────────────

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        self.docs.insert(uri.clone(), params.text_document.text);
        self.refresh(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // We advertised FULL sync, so there's exactly one change containing
        // the entire new text.
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().next() {
            self.docs.insert(uri.clone(), change.text);
        }
        self.refresh(uri).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // Re-analyze on save in case on-disk state matters for future rules.
        let uri = params.text_document.uri.clone();
        self.refresh(uri.clone()).await;

        // Re-index the saved file in the workspace map. Cheap (single-
        // file walk) and keeps cross-file goto-def / refs / rename
        // fresh as the user edits.
        if let Ok(path) = uri.to_file_path() {
            if let Ok(mut w) = self.workspace.write() {
                w.update_file(&path);
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.remove(&params.text_document.uri);
    }

    // ── Navigation ────────────────────────────────────────────────────

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);
        let Some((ident, _)) = analyzer::identifier_at(&source, line, col) else {
            return Ok(None);
        };

        // In-file lookup first — if the symbol is defined in the same
        // document, jump there (identical to Phase 0 behavior).
        let result = analyzer::analyze(&source);
        let in_file = result.symbols.resolve(&ident);
        if !in_file.is_empty() {
            let locations: Vec<Location> = in_file.iter().map(|s| Location {
                uri: uri.clone(),
                range: to_lsp_range(s.range),
            }).collect();
            return Ok(Some(GotoDefinitionResponse::Array(locations)));
        }

        // Workspace fallback — cross-file definitions. The index may be
        // empty if the LSP couldn't resolve a Universe root; in that
        // case we simply fall through to the native-API lookup below.
        let workspace_hits = self
            .workspace
            .read()
            .ok()
            .map(|w| {
                w.resolve_name(&ident)
                    .iter()
                    .filter_map(|s| file_path_to_uri(&s.file).map(|u| Location {
                        uri: u,
                        range: to_lsp_range(s.range),
                    }))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !workspace_hits.is_empty() {
            return Ok(Some(GotoDefinitionResponse::Array(workspace_hits)));
        }

        // Phase-D fallback: native API. Generate (or reuse) a read-only
        // markdown document describing the function / type and return a
        // location pointing at it. Feels like goto-def even though the
        // "definition" is Rust on the engine side.
        if let Some(entry) = analyzer::api_lookup(&ident) {
            if let Some(loc) = self.synthetic_api_location(entry) {
                return Ok(Some(GotoDefinitionResponse::Array(vec![loc])));
            }
        }

        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);
        let Some((ident, _)) = analyzer::identifier_at(&source, line, col) else {
            return Ok(None);
        };

        let mut locations: Vec<Location> = Vec::new();

        // In-file declarations — same-document matches keep their source URI.
        let result = analyzer::analyze(&source);
        for s in result.symbols.resolve(&ident) {
            locations.push(Location {
                uri: uri.clone(),
                range: to_lsp_range(s.range),
            });
        }

        // Workspace-wide declarations — other `.rune` files with the
        // same symbol name. Uses the cached index so we don't re-parse
        // every file on each request.
        if let Ok(w) = self.workspace.read() {
            for s in w.resolve_name(&ident) {
                if let Some(u) = file_path_to_uri(&s.file) {
                    // Skip duplicates of the current file (already added above).
                    if u == uri { continue; }
                    locations.push(Location {
                        uri: u,
                        range: to_lsp_range(s.range),
                    });
                }
            }
        }

        Ok(Some(locations))
    }

    // ── Informational ─────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);
        let result = analyzer::analyze(&source);

        // Unified hover resolution — API catalog → local symbol → diagnostic.
        // Uses the analyzer's markdown renderer so signatures and example
        // blocks render consistently across LSP clients.
        let Some(mut info) = analyzer::hover(&source, line, col, &result.symbols, &result.diagnostics) else {
            return Ok(None);
        };

        // Phase-E live value: if the hover is on a string literal that
        // sits as an argument to `get_sim_value`, `set_sim_value`, or
        // `list_sim_values`, consult the runtime snapshot and append
        // the current value. Gives cross-process "this is the live
        // number right now" insight the regular catalog can't provide.
        if let Some(key) = sim_key_at_cursor(&source, line, col) {
            if let Some(universe) = uri.to_file_path().ok().and_then(|p| walk_up_for_universe(&p)) {
                if let Some(snap) = super::runtime_snapshot::read_snapshot(&universe) {
                    let live_line = match snap.sim_values.get(&key) {
                        Some(v) => format!(
                            "\n\n---\n\n**Live value** (`{}`): `{}`{}",
                            key,
                            v,
                            if snap.play_state == super::runtime_snapshot::PlayState::Playing {
                                "  _(simulation running)_"
                            } else {
                                "  _(snapshot from last play session)_"
                            },
                        ),
                        None => format!(
                            "\n\n---\n\n**Live value** (`{}`): _key not present in snapshot_",
                            key,
                        ),
                    };
                    info.markdown.push_str(&live_line);
                }
            }
        }

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: info.markdown,
            }),
            range: Some(to_lsp_range(info.range)),
        }))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let tokens = compute_semantic_tokens(&source);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);
        let Some((entry, active_param)) = analyzer::signature_help_at(&source, line, col) else {
            return Ok(None);
        };

        // Build one `SignatureInformation` describing the callee. Each
        // parameter gets its own `ParameterInformation` so the client
        // can bold the active one.
        let label = {
            let params_str = entry
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.typ))
                .collect::<Vec<_>>()
                .join(", ");
            if entry.return_type.is_empty() || entry.return_type == "()" {
                format!("fn {}({})", entry.name, params_str)
            } else {
                format!("fn {}({}) -> {}", entry.name, params_str, entry.return_type)
            }
        };

        let parameters = entry
            .params
            .iter()
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(format!("{}: {}", p.name, p.typ)),
                documentation: None,
            })
            .collect();

        let doc = if entry.doc.is_empty() {
            None
        } else {
            Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: entry.doc.clone(),
            }))
        };

        // Clamp — the API might have been called with more commas than
        // parameters (mistake, or trailing comma). Cap at last param.
        let active_index = std::cmp::min(
            active_param as usize,
            entry.params.len().saturating_sub(1),
        ) as u32;

        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label,
                documentation: doc,
                parameters: Some(parameters),
                active_parameter: Some(active_index),
            }],
            active_signature: Some(0),
            active_parameter: Some(active_index),
        }))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);

        // Phase-E string-literal completion: if the cursor sits inside
        // the key argument of `get_sim_value("...")` / `set_sim_value`,
        // replace the normal identifier completion with the live keys
        // from the runtime snapshot. Typing `get_sim_value("bat` now
        // surfaces `battery.voltage`, `battery.soc`, etc.
        if let Some(prefix) = sim_key_prefix_at_cursor(&source, line, col) {
            if let Some(universe) = uri.to_file_path().ok().and_then(|p| walk_up_for_universe(&p)) {
                if let Some(snap) = super::runtime_snapshot::read_snapshot(&universe) {
                    let lower = prefix.to_ascii_lowercase();
                    let items: Vec<CompletionItem> = snap
                        .sim_values
                        .keys()
                        .filter(|k| k.to_ascii_lowercase().starts_with(&lower))
                        .take(50)
                        .map(|k| {
                            let v = snap.sim_values.get(k).copied().unwrap_or(0.0);
                            CompletionItem {
                                label: k.clone(),
                                kind: Some(CompletionItemKind::CONSTANT),
                                detail: Some(format!("current value: {}", v)),
                                ..Default::default()
                            }
                        })
                        .collect();
                    return Ok(Some(CompletionResponse::Array(items)));
                }
            }
        }

        let (prefix, _start) = analyzer::prefix_at(&source, line, col);
        let result = analyzer::analyze(&source);
        let items = analyzer::complete(&prefix, &result.symbols, 50);

        let lsp_items: Vec<CompletionItem> = items.into_iter().map(|c| CompletionItem {
            label: c.label.clone(),
            kind: Some(match c.kind {
                analyzer::CompletionKind::Keyword  => CompletionItemKind::KEYWORD,
                analyzer::CompletionKind::Function => CompletionItemKind::FUNCTION,
                analyzer::CompletionKind::Variable => CompletionItemKind::VARIABLE,
                analyzer::CompletionKind::Module   => CompletionItemKind::MODULE,
            }),
            detail: if c.detail.is_empty() { None } else { Some(c.detail) },
            ..Default::default()
        }).collect();

        Ok(Some(CompletionResponse::Array(lsp_items)))
    }

    // ── Refactor ──────────────────────────────────────────────────────

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let new_name = params.new_name;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);
        let Some((old_name, _)) = analyzer::identifier_at(&source, line, col) else {
            return Ok(None);
        };

        let mut changes: std::collections::HashMap<Url, Vec<TextEdit>> =
            std::collections::HashMap::new();

        // In-file rename — the comment/string-aware analyzer already
        // handles this correctly per-document.
        let edits = analyzer::rename(&source, &old_name, &new_name);
        if !edits.is_empty() {
            let lsp_edits: Vec<TextEdit> = edits.iter().map(|e| TextEdit {
                range: byte_range_to_lsp(&source, e.byte_range),
                new_text: e.new_text.clone(),
            }).collect();
            changes.insert(uri.clone(), lsp_edits);
        }

        // Workspace-wide rename — walk every other `.rune` file that
        // declares or mentions this name. Each file re-runs the same
        // comment/string-aware pass so we don't blindly text-replace
        // through literal "foo" strings that happen to share the
        // identifier.
        if let Ok(w) = self.workspace.read() {
            // Collect unique file paths that mention the symbol so we
            // don't re-read a file once per occurrence.
            let mut touched: std::collections::HashSet<PathBuf> =
                std::collections::HashSet::new();
            for sym in w.resolve_name(&old_name) {
                touched.insert(sym.file.clone());
            }
            for path in touched {
                // Skip the current file — already handled above from
                // the in-memory buffer which may have unsaved edits.
                let path_uri = file_path_to_uri(&path);
                if path_uri.as_ref() == Some(&uri) { continue; }
                let Some(other_uri) = path_uri else { continue };
                let Ok(other_src) = std::fs::read_to_string(&path) else { continue };
                let other_edits = analyzer::rename(&other_src, &old_name, &new_name);
                if other_edits.is_empty() { continue; }
                let lsp_edits: Vec<TextEdit> = other_edits.iter().map(|e| TextEdit {
                    range: byte_range_to_lsp(&other_src, e.byte_range),
                    new_text: e.new_text.clone(),
                }).collect();
                changes.insert(other_uri, lsp_edits);
            }
        }

        if changes.is_empty() { return Ok(None) }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> LspResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let result = analyzer::analyze(&source);
        let mut actions: Vec<CodeActionOrCommand> = Vec::new();

        // Offer actions for every diagnostic whose range intersects the
        // client's requested range. LSP's `context.diagnostics` list also
        // includes relevant diagnostics — we use our own full set so stale
        // client state doesn't hide fresh fixes.
        for d in &result.diagnostics {
            if !ranges_overlap(to_lsp_range(d.range), params.range) { continue }
            for a in analyzer::code_actions(&source, d) {
                let lsp_edits: Vec<TextEdit> = a.edits.iter().map(|e| TextEdit {
                    range: byte_range_to_lsp(&source, e.byte_range),
                    new_text: e.new_text.clone(),
                }).collect();

                let mut changes = std::collections::HashMap::new();
                changes.insert(uri.clone(), lsp_edits);

                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: a.title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![to_lsp_diagnostic(d)]),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        document_changes: None,
                        change_annotations: None,
                    }),
                    command: None,
                    is_preferred: Some(true),
                    disabled: None,
                    data: None,
                }));
            }
        }
        Ok(Some(actions))
    }

    // ── Outline ───────────────────────────────────────────────────────

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let Some(source) = self.docs.get(&params.text_document.uri).map(|s| s.clone()) else {
            return Ok(None);
        };
        let result = analyzer::analyze(&source);

        let symbols: Vec<DocumentSymbol> = result.symbols.iter().map(|s| {
            #[allow(deprecated)]
            DocumentSymbol {
                name: s.name.clone(),
                detail: None,
                kind: match s.kind {
                    analyzer::SymbolKind::Function => SymbolKind::FUNCTION,
                },
                tags: None,
                deprecated: None,
                range: to_lsp_range(s.range),
                selection_range: to_lsp_range(s.range),
                children: None,
            }
        }).collect();

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}

fn ranges_overlap(a: Range, b: Range) -> bool {
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character < b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character < a.start.character))
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests — pure type-mapping checks (no runtime, no tokio)
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzer_range_maps_to_zero_based_lsp() {
        let r = analyzer::Range {
            start_line: 5, start_column: 10, end_line: 5, end_column: 15,
        };
        let lsp = to_lsp_range(r);
        assert_eq!(lsp.start, Position { line: 4, character: 9 });
        assert_eq!(lsp.end,   Position { line: 4, character: 14 });
    }

    #[test]
    fn lsp_pos_maps_to_one_based_analyzer() {
        let (l, c) = lsp_pos_to_line_col(Position { line: 0, character: 0 });
        assert_eq!((l, c), (1, 1));
        let (l, c) = lsp_pos_to_line_col(Position { line: 10, character: 5 });
        assert_eq!((l, c), (11, 6));
    }

    #[test]
    fn byte_range_to_lsp_round_trip() {
        let source = "abc\ndef\nghij";
        // Byte 5 = 'e' on line 1, col 1 → LSP (line=1, char=1)
        let r = byte_range_to_lsp(source, (5, 7));
        assert_eq!(r.start, Position { line: 1, character: 1 });
        assert_eq!(r.end,   Position { line: 1, character: 3 });
    }

    #[test]
    fn diagnostic_severity_maps_correctly() {
        let d = analyzer::Diagnostic {
            range: analyzer::Range { start_line: 1, start_column: 1, end_line: 1, end_column: 2 },
            byte_range: (0, 1),
            severity: analyzer::Severity::Warning,
            message: "sample".into(),
            source: "rune",
        };
        let lsp = to_lsp_diagnostic(&d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(lsp.source.as_deref(), Some("rune"));
    }
}
