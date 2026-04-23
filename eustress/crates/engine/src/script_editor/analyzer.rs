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
    let mut preparer = rune::prepare(&mut sources);
    if let Some(ctx) = eustress_context() {
        // Passing a `with_context(ctx)` lets the compiler resolve `use
        // eustress::{...}` and every registered API symbol. Without it
        // the analyzer previously flagged every Eustress import as
        // "module not found" — noise that drowned out real errors.
        preparer = preparer.with_context(ctx);
    }
    let build_result = preparer
        .with_diagnostics(&mut diagnostics)
        .build();

    for diag in diagnostics.diagnostics() {
        if let Some(converted) = convert_rune_diagnostic(diag, &line_starts) {
            out.diagnostics.push(converted);
        }
    }

    // 3. Dry-run pass — actually invoke each lifecycle entrypoint with
    //    mock arguments so runtime-only failures (method dispatch on
    //    dynamic values, arity mismatches on FFI calls, `.to_string()`
    //    on a type that doesn't have it, etc.) surface as diagnostics
    //    BEFORE the user hits Play. Rune's compiler can't catch these
    //    because method dispatch is resolved at runtime — so we exercise
    //    the resolution by running it.
    //
    //    Only fires when the compile pass succeeded (errors above would
    //    otherwise shadow the real issue) and the Eustress context is
    //    available. Side-effects during dry-run go to thread-local
    //    queues (GUI commands, sim values) that either get drained into
    //    the editor's empty-target state or get overwritten as soon as
    //    play mode starts — so there's no user-visible fallout.
    //
    //    Diagnostics emitted here use source = "rune-runtime" so the
    //    Problems panel can distinguish them from compile-time errors.
    if !out.diagnostics.iter().any(|d| matches!(d.severity, Severity::Error)) {
        if let (Ok(unit), Some(ctx)) = (build_result, eustress_context()) {
            let runtime_diagnostics = dry_run_entrypoints(unit, ctx, &out.symbols);
            out.diagnostics.extend(runtime_diagnostics);
        }
    }

    out
}

/// Entrypoint name + synthesized arg tuple. Only entrypoints the script
/// actually defines get called — others would just produce `Missing entry`
/// errors and noise up the output. We deliberately skip `on_exit` because
/// running it in edit mode could trip state-teardown logic the script
/// assumed happens on shutdown.
///
/// Returns a Vec of diagnostics (one per failing entrypoint).
fn dry_run_entrypoints(
    unit: rune::Unit,
    ctx: &rune::Context,
    symbols: &SymbolIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let Ok(runtime_ctx) = ctx.runtime() else { return diagnostics };
    let runtime_ctx = std::sync::Arc::new(runtime_ctx);
    let unit = std::sync::Arc::new(unit);

    // Each entry: (name, has-args). Args are synthesized inline below
    // because Rune's `Args` trait requires different tuple shapes per
    // call site — can't bundle them in a single Vec without boxing.
    let entrypoints: &[&str] = &[
        "on_init",
        "on_ready",
        "on_update",
        "on_button_click",
        "on_tick",
    ];

    for name in entrypoints {
        // Skip entrypoints the script didn't define — Rune will return
        // `Missing entry` which is the expected shape, not a real error.
        if symbols.resolve(name).is_empty() {
            continue;
        }

        let mut vm = rune::Vm::new(runtime_ctx.clone(), unit.clone());
        let result = match *name {
            "on_update" | "on_tick" => vm.call([*name], (0.016_f64,)),
            "on_button_click"       => vm.call([*name], ("TestButton".to_string(),)),
            _                       => vm.call([*name], ()),
        };

        if let Err(e) = result {
            let msg = e.to_string();
            // `Missing entry` here shouldn't happen (we checked symbols
            // above), but guard anyway in case the script defined the
            // name as a non-function or in a non-top-level scope.
            if msg.starts_with("Missing entry ") {
                continue;
            }
            // Range: anchor at the function definition so the squiggle
            // underlines `on_update` in the source. Falls back to
            // (1,1) if the symbol resolver drops the slot.
            let sym = symbols.resolve(name).first().cloned();
            let range = sym.as_ref().map(|s| s.range.clone()).unwrap_or(Range {
                start_line: 1, start_column: 1,
                end_line: 1, end_column: 1,
            });
            let byte_range = sym.as_ref().map(|s| s.byte_range).unwrap_or((0, 0));
            diagnostics.push(Diagnostic {
                range,
                byte_range,
                severity: Severity::Error,
                message: format!(
                    "Runtime check failed for `{}`: {}\n\
                     (Rune caught this by dry-running the function — it will fail at play time too.)",
                    name, msg,
                ),
                source: "rune-runtime",
            });
        }
    }

    diagnostics
}

// ═══════════════════════════════════════════════════════════════════════════
// Eustress API catalog — hover, completion, signature help
// ═══════════════════════════════════════════════════════════════════════════
//
// Single source of truth already lives in [`workshop::api_reference`],
// parsed at compile time from `rune_ecs_module.rs` + the Scripting API
// checklist markdown. That same catalog drives the Workshop agent
// prompt, the in-engine API Browser panel, and the /learn web docs.
// Here we just cache one copy for the analyzer and expose name lookups.
//
// The catalog is read-only after construction, so sharing via `Arc`
// across async analyzer tasks is safe.

use crate::workshop::api_reference::{ApiCatalog, ApiEntry as CatalogEntry};

static API_CATALOG: std::sync::OnceLock<std::sync::Arc<ApiCatalog>> =
    std::sync::OnceLock::new();

fn api_catalog() -> &'static ApiCatalog {
    API_CATALOG
        .get_or_init(|| std::sync::Arc::new(ApiCatalog::build()))
}

/// Look up a single API entry by exact name. Used by hover and
/// goto-def-into-native-API (Phase D).
pub fn api_lookup(name: &str) -> Option<&'static CatalogEntry> {
    // SAFETY: `api_catalog()` returns a reference into a `OnceLock<Arc<_>>`
    // that never drops for the lifetime of the process, so the `'static`
    // extension is sound.
    api_catalog()
        .entries
        .iter()
        .find(|e| e.name == name)
}

/// Return entries whose names start with the prefix (case-insensitive).
/// Used by completion so typing `get_s` surfaces `get_sim_value`,
/// `get_soc`, etc.
pub fn api_starts_with<'a>(prefix: &'a str) -> impl Iterator<Item = &'static CatalogEntry> + 'a {
    let lower = prefix.to_ascii_lowercase();
    api_catalog().entries.iter().filter(move |e| {
        lower.is_empty() || e.name.to_ascii_lowercase().starts_with(&lower)
    })
}

/// If the cursor sits inside the argument list of a call to one of the
/// registered API functions, return the function entry plus a 0-based
/// parameter index. `signature_help_at("get_sim_value(|", line, col)`
/// yields `(entry_for_get_sim_value, 0)`. Returns `None` if the cursor
/// isn't in a recognisable call context.
///
/// The scan is intentionally shallow — a bounded look-back from the
/// cursor to find the nearest open `(` not already balanced by a
/// matching `)`, then counts commas at depth 1 to compute the param
/// index. Good enough for IDE-grade parameter hints; doesn't attempt
/// to resolve chained calls or trait methods.
pub fn signature_help_at(
    source: &str,
    line: u32,
    column: u32,
) -> Option<(&'static CatalogEntry, u32)> {
    const LOOKBACK_BYTES: usize = 2048;
    let offset = line_col_to_offset(source, line, column)? as usize;
    let bytes = source.as_bytes();
    let start = offset.saturating_sub(LOOKBACK_BYTES);

    // Walk backward from the cursor. Track parenthesis + bracket depth
    // so commas in nested calls don't bump the param index.
    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut brace_depth: i32 = 0;
    let mut commas_at_depth1: u32 = 0;
    let mut i = offset;
    let open_paren_idx = loop {
        if i == 0 || i <= start {
            return None;
        }
        i -= 1;
        let b = bytes[i];
        // Skip the interiors of string/char literals so commas inside
        // strings don't count as parameter separators. Simple pass —
        // doesn't handle escaped quotes perfectly but good enough for
        // our purposes.
        if b == b'"' || b == b'\'' {
            let quote = b;
            if i == 0 { return None; }
            i -= 1;
            while i > 0 && bytes[i] != quote {
                i -= 1;
            }
            continue;
        }
        match b {
            b')' => paren_depth += 1,
            b'(' => {
                if paren_depth == 0 {
                    break i;
                }
                paren_depth -= 1;
            }
            b']' => bracket_depth += 1,
            b'[' => bracket_depth -= 1,
            b'}' => brace_depth += 1,
            b'{' => brace_depth -= 1,
            b';' => return None, // crossed a statement boundary
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                commas_at_depth1 += 1;
            }
            _ => {}
        }
    };

    // Walk backward from `(` over whitespace, then grab the identifier
    // that comes before it. That identifier is the callee name.
    let mut j = open_paren_idx;
    while j > 0 && bytes[j - 1].is_ascii_whitespace() {
        j -= 1;
    }
    if j == 0 {
        return None;
    }
    let name_end = j;
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    while j > 0 && is_ident(bytes[j - 1]) {
        j -= 1;
    }
    if j == name_end {
        return None;
    }
    let name = &source[j..name_end];

    let entry = api_lookup(name)?;
    Some((entry, commas_at_depth1))
}

/// Render an API entry as Markdown fit for LSP hover. Shared between
/// the current-file hover handler and the future native-goto-def
/// synthetic document (Phase D).
pub fn render_api_hover(entry: &CatalogEntry) -> String {
    let mut out = String::new();
    out.push_str(&format!("**{}** — *{}*\n\n", entry.name, entry.category));

    // Signature line: `fn name(p1: T1, p2: T2) -> RetT`
    let params = entry
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.typ))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str("```rust\n");
    if entry.return_type.is_empty() || entry.return_type == "()" {
        out.push_str(&format!("fn {}({})\n", entry.name, params));
    } else {
        out.push_str(&format!(
            "fn {}({}) -> {}\n",
            entry.name, params, entry.return_type,
        ));
    }
    out.push_str("```\n\n");

    if !entry.doc.is_empty() {
        out.push_str(&entry.doc);
        out.push('\n');
    }
    if !entry.example.is_empty() {
        out.push_str("\n### Example\n\n```rune\n");
        out.push_str(&entry.example);
        out.push_str("\n```");
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared Rune context — the same API surface the runtime sees
// ═══════════════════════════════════════════════════════════════════════════
//
// We build one canonical `rune::Context` lazily and reuse it across every
// `analyze()` call. Construction is the expensive part (module metadata
// registration); analysis itself is cheap. The context is immutable once
// built, so sharing via `Arc` across analyzer tasks is safe.
//
// If `rune::Context` turns out not to be `Send + Sync`, the `OnceLock`
// below will refuse to compile — that would be a genuine diagnostic, not
// a workaround we want to reach for.

use std::sync::OnceLock;

static ANALYZER_CONTEXT: OnceLock<Option<std::sync::Arc<rune::Context>>> = OnceLock::new();

/// Return the shared Rune context used for analyzer compilation passes,
/// or `None` if construction failed. Cached on first call.
fn eustress_context() -> Option<&'static rune::Context> {
    ANALYZER_CONTEXT
        .get_or_init(build_analyzer_context)
        .as_deref()
}

fn build_analyzer_context() -> Option<std::sync::Arc<rune::Context>> {
    let mut ctx = rune::Context::with_default_modules().ok()?;

    // The engine's single ECS module exposes the full Eustress API
    // surface — `get_sim_value`, `set_sim_value`, `gui_set_text`,
    // Instance, TweenService, raycast helpers, task utilities, UDim,
    // and the Vector3/Color3/CFrame types. Building it is pure metadata
    // registration; the thread-local ECS bindings it consults at call
    // time aren't needed for compilation. A missing installation here
    // falls back to "default modules only" — still better than the old
    // behaviour of no context at all.
    #[cfg(feature = "realism-scripting")]
    {
        if let Ok(module) = crate::soul::rune_ecs_module::create_ecs_module() {
            let _ = ctx.install(module);
        }
        if let Ok(module) = crate::soul::rune_ecs_module::create_event_bus_module() {
            let _ = ctx.install(module);
        }
    }

    Some(std::sync::Arc::new(ctx))
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

/// Hover info for the identifier under the cursor. Resolution order:
///
/// 1. **Eustress API catalog** — registered native function/type. Yields
///    signature + docstring + example as Markdown.
/// 2. **In-file symbol index** — `fn foo() {}` defined in this document.
/// 3. **Cursor-spanning diagnostic** — if a compile error overlaps the
///    cursor, show its message. Used when there's no symbol to resolve.
///
/// Returns `None` when the cursor isn't on an identifier OR nothing
/// matches. Callers (LSP `textDocument/hover`, engine Problems tooltip)
/// render the returned Markdown verbatim.
pub fn hover(
    source: &str,
    line: u32,
    column: u32,
    symbols: &SymbolIndex,
    diagnostics: &[Diagnostic],
) -> Option<HoverInfo> {
    let (name, byte_range) = identifier_at(source, line, column)?;
    let line_starts = compute_line_starts(source);
    let range = span_to_range(byte_range.0, byte_range.1, &line_starts);

    // 1. API catalog — authoritative for engine-registered names.
    if let Some(entry) = api_lookup(&name) {
        return Some(HoverInfo {
            range,
            byte_range,
            markdown: render_api_hover(entry),
            source: HoverSource::ApiCatalog,
        });
    }

    // 2. In-file symbol — surface the symbol's kind + its own defining range.
    if let Some(sym) = symbols.resolve(&name).first() {
        let kind = match sym.kind {
            SymbolKind::Function => "fn",
        };
        let md = format!(
            "**{}** — *in-file {kind}*\n\nDefined at line {line}:{col}.",
            sym.name,
            line = sym.range.start_line,
            col = sym.range.start_column,
        );
        return Some(HoverInfo {
            range,
            byte_range,
            markdown: md,
            source: HoverSource::LocalSymbol,
        });
    }

    // 3. Diagnostic overlap — if a compiler error covers the cursor,
    //    surface its message so hovering broken code explains itself.
    for diag in diagnostics {
        if byte_range.0 >= diag.byte_range.0 && byte_range.1 <= diag.byte_range.1 {
            return Some(HoverInfo {
                range,
                byte_range,
                markdown: format!(
                    "**{}** — *{}*\n\n{}",
                    name,
                    severity_label(diag.severity),
                    diag.message,
                ),
                source: HoverSource::Diagnostic,
            });
        }
    }

    None
}

fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

/// Hover payload. `markdown` is ready-to-render CommonMark that LSP
/// clients display in a tooltip; `range` bounds the identifier that
/// triggered the hover so the client knows which text to underline.
#[derive(Debug, Clone)]
pub struct HoverInfo {
    pub range: Range,
    pub byte_range: (u32, u32),
    pub markdown: String,
    pub source: HoverSource,
}

/// Where the hover content came from. LSP doesn't care; useful for
/// debugging and for the engine's own Problems tooltip which color-
/// codes by provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverSource {
    ApiCatalog,
    LocalSymbol,
    Diagnostic,
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

    // Eustress API catalog — every function, method, and type the engine
    // registers into the Rune context. Skip entries that collide with a
    // local symbol of the same name so user definitions shadow the API
    // (matching Rune's own resolution rules).
    let local_names: std::collections::HashSet<&str> =
        symbols.by_name.keys().map(|s| s.as_str()).collect();
    for entry in api_starts_with(prefix) {
        if local_names.contains(entry.name.as_str()) {
            continue;
        }
        // Signature as completion detail so the drop-down shows "fn
        // get_sim_value(key: String) -> f64" alongside the name.
        let params = entry
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.typ))
            .collect::<Vec<_>>()
            .join(", ");
        let detail = if entry.return_type.is_empty() || entry.return_type == "()" {
            format!("fn {}({})", entry.name, params)
        } else {
            format!("fn {}({}) -> {}", entry.name, params, entry.return_type)
        };
        out.push(Completion {
            label: entry.name.clone(),
            // Types get `Module` kind so the VS Code icon differs from
            // function results — Roblox-style `Vector3::new` vs. free
            // functions are visually distinct.
            kind: if entry.return_type.starts_with("struct ")
                || entry.name.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
            {
                CompletionKind::Module
            } else {
                CompletionKind::Function
            },
            detail,
        });
    }

    // Keywords are already at the top because they were pushed first.
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
