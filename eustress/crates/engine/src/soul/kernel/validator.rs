//! # The Kernel gate — `validate_rune_rewrite`
//!
//! This is the single, non-bypassable choke-point for admitting a candidate Rune
//! **program** under the active [`UniverseLaws`]. Every RSI rewrite, every
//! command-bar one-shot, and every play-mode compile is meant to flow through
//! here (the `build_pipeline.rs` RSI loop already gates on the legacy adapter
//! [`validate_rune_script`], so making that adapter call this function gates the
//! whole loop with no new call site).
//!
//! ## What runs in the per-tick gate (deterministic, sub-ms)
//!
//! 1. **Tier 1 — Syntactic.** Rune parse via `parse_all::<ast::File>`. A parse
//!    failure is a fatal [`ViolationKind::Syntax`].
//! 2. **Tier 2 — Grammar / capability.**
//!    - Resolve every call symbol against the [`CapabilityCatalog`]; unknown =>
//!      [`ViolationKind::UnknownCapability`].
//!    - Check each resolved capability's class against the granted set; withheld
//!      => [`ViolationKind::WithheldCapability`].
//!    - Apply scope rules (path-traversal, immutable physics).
//!    - Check the entrypoint contract (presence + arity).
//!
//! Both tiers are pure and offline — NO Claude API call, NO RSI-learned
//! threshold — honoring constitutional CONST-004 (determinism). The verdict is a
//! pure function of `(source, laws)` and is reproducible given
//! `laws.registry_version`.
//!
//! ## What does NOT run here (deferred effect tier)
//!
//! The effect tier — fork a sandbox, run the candidate, reject on a law-violating
//! end-state — is explicitly excluded from the per-tick gate (it is seconds-slow
//! and cannot meet the 50ms cell tick deadline). It is the TODO seam
//! [`evaluate_effect_tier`], to be run as async post-hoc audit.
//!
//! ## Call-symbol extraction: scaffold approach
//!
//! Entrypoint detection + arity use the stable `ast::File.items` walk. Call-site
//! capability extraction currently uses a lexical token scan over the
//! parse-validated source (see [`extract_call_symbols`]). This is deliberate: a
//! deep recursive `ast::Expr` walk is version-fragile across Rune releases (the
//! analyzer notes the same fragility). The lexical pass is precise enough for the
//! capability grammar and is documented as a seam to upgrade to full AST call
//! resolution + `eustress_context` symbol resolution once the walk is pinned.

use rune::ast;

use super::capability::CapabilityCatalog;
use super::laws::{ScopeRule, UniverseLaws};
use super::verdict::{LawViolation, RewriteVerdict, ViolationKind, ViolationSeverity};

// ============================================================================
// Public gate
// ============================================================================

/// THE gate. Validate a candidate Rune program against the active universe laws
/// and return a structured [`RewriteVerdict`]. This is the only supported way to
/// admit a rewrite; there is no bypass path.
///
/// Pure and deterministic given `(source, laws)`.
pub fn validate_rune_rewrite(source: &str, laws: &UniverseLaws) -> RewriteVerdict {
    let mut violations: Vec<LawViolation> = Vec::new();

    // ---- Tier 1: syntactic --------------------------------------------------
    let parsed = parse_program(source);
    let file = match parsed {
        Ok(file) => file,
        Err(v) => {
            // Parse failure short-circuits — there is no AST to grammar-check.
            return RewriteVerdict::reject(vec![v]);
        }
    };

    // ---- Tier 2: grammar / capability --------------------------------------
    check_capabilities(source, &laws.catalog, laws, &mut violations);
    check_scope_rules(source, laws, &mut violations);
    check_entrypoints(&file, source, laws, &mut violations);

    // The effect tier is intentionally NOT invoked here (see module docs).
    // `evaluate_effect_tier` is the async post-hoc seam.

    RewriteVerdict::from_violations(violations)
}

/// Legacy-shaped adapter. Keeps the exact `Result<(), Vec<String>>` signature
/// that `build_pipeline.rs` and other existing callers consume, so wiring the
/// real gate in requires no call-site changes there. Uses the active universe's
/// laws (currently the permissive core default — see
/// [`UniverseLaws::load_for_active_universe`] TODO seam).
///
/// This REPLACES the previous no-op stub at `rune_api.rs:483`. Re-export it from
/// `rune_api` / `soul::mod` so the public path `crate::soul::validate_rune_script`
/// resolves here.
pub fn validate_rune_script(source: &str) -> Result<(), Vec<String>> {
    let laws = UniverseLaws::load_for_active_universe();
    validate_rune_rewrite(source, &laws).into_legacy_result()
}

// ============================================================================
// Tier 1 — syntactic
// ============================================================================

/// Parse the source into a Rune AST `File`. On failure, produce a localized
/// syntax violation.
fn parse_program(source: &str) -> Result<ast::File, LawViolation> {
    match rune::parse::parse_all::<ast::File>(source, rune::SourceId::new(0), true) {
        Ok(file) => Ok(file),
        Err(err) => {
            use rune::ast::Spanned;
            let span = err.span();
            let (line, column) = byte_offset_to_linecol(source, span.start.0 as usize);
            Err(LawViolation {
                law_id: "SYNTAX".into(),
                message: format!("Rune parse error: {}", err),
                line,
                column,
                severity: ViolationSeverity::Fatal,
                kind: ViolationKind::Syntax,
            })
        }
    }
}

// ============================================================================
// Tier 2 — capability grammar
// ============================================================================

/// One extracted call site: the symbol called and its byte offset.
#[derive(Debug, Clone)]
struct CallSite {
    /// The call symbol, normalized to Rune path form (e.g. `set_sim_value`,
    /// `Instance::new`). Roblox-style dotted receivers (`Instance.new`,
    /// `task.wait`) are normalized `.` -> `::`.
    symbol: String,
    /// True if this is a receiver/method-style call (had a `.` or `::`
    /// separator). Method dispatch on a dynamic value is runtime-resolved and is
    /// OUT of scope for the static capability grammar, so an *unrecognized*
    /// receiver call is NOT flagged as an unknown capability — only a withheld
    /// match (when the normalized path IS catalogued) is enforced.
    is_receiver_call: bool,
    /// Byte offset of the symbol start, for line:col localization.
    offset: usize,
}

/// Walk the call sites and resolve each against the capability catalog,
/// recording unknown-capability and withheld-capability violations.
fn check_capabilities(
    source: &str,
    catalog: &CapabilityCatalog,
    laws: &UniverseLaws,
    out: &mut Vec<LawViolation>,
) {
    for call in extract_call_symbols(source) {
        match catalog.lookup(&call.symbol) {
            None => {
                // A receiver/method-style call that doesn't resolve in the
                // catalog is method dispatch on a dynamic value (e.g.
                // `result.is_some()`, `vec.x`) — runtime-resolved and OUT of the
                // static grammar's scope. Do not flag it.
                if call.is_receiver_call {
                    continue;
                }
                // A bare free-function call that is neither a local helper
                // (filtered in `extract_call_symbols`) nor catalogued IS an
                // unknown capability.
                let (line, column) = byte_offset_to_linecol(source, call.offset);
                out.push(LawViolation {
                    law_id: "UNKNOWN_CAPABILITY".into(),
                    message: format!(
                        "call to `{}` is not in the universe vocabulary",
                        call.symbol
                    ),
                    line,
                    column,
                    severity: ViolationSeverity::Fatal,
                    kind: ViolationKind::UnknownCapability,
                });
            }
            Some(cap) => {
                if !laws.grants(cap.class) {
                    let (line, column) = byte_offset_to_linecol(source, call.offset);
                    out.push(LawViolation {
                        law_id: format!("WITHHELD_CAPABILITY:{}", cap.class.as_str()),
                        message: format!(
                            "capability `{}` ({}) is withheld by universe `{}`",
                            cap.name,
                            cap.class.as_str(),
                            laws.universe
                        ),
                        line,
                        column,
                        severity: ViolationSeverity::Fatal,
                        kind: ViolationKind::WithheldCapability,
                    });
                }
            }
        }
    }
}

/// Apply scope rules that go beyond raw capability grant/withhold.
fn check_scope_rules(source: &str, laws: &UniverseLaws, out: &mut Vec<LawViolation>) {
    for rule in &laws.scope_rules {
        match rule {
            ScopeRule::NoPathTraversal => {
                // Reject filesystem calls whose first string literal argument
                // escapes the space root. Scaffold heuristic: scan for
                // read_space_file / write_space_file followed by a string
                // literal containing `..` or a leading `/` or drive prefix. A
                // future version resolves the literal from the AST argument.
                detect_path_traversal(source, out);
            }
            ScopeRule::ImmutablePhysics => {
                // World-law mutation forbidden regardless of class grant.
                for call in extract_call_symbols(source) {
                    if let Some(cap) = laws.catalog.lookup(&call.symbol) {
                        if matches!(cap.class, super::capability::CapabilityClass::WorldLaw) {
                            let (line, column) = byte_offset_to_linecol(source, call.offset);
                            out.push(LawViolation {
                                law_id: "SCOPE:immutable_physics".into(),
                                message: format!(
                                    "`{}` mutates a world law, forbidden in immutable-physics universe `{}`",
                                    cap.name, laws.universe
                                ),
                                line,
                                column,
                                severity: ViolationSeverity::Fatal,
                                kind: ViolationKind::Scope,
                            });
                        }
                    }
                }
            }
            ScopeRule::ForbiddenPattern(_pattern) => {
                // TODO(kernel-scope): compile forbidden patterns to a matcher and
                // emit Scope violations. Not enforced by the first deliverable.
            }
        }
    }
}

/// Check the entrypoint contract: every required-present-context program must
/// define at least one legal entrypoint, and any defined entrypoint must have
/// the right arity.
///
/// Entrypoint names are recovered by slicing `source` at each function's name
/// span — the same approach `script_editor::analyzer::collect_symbols` uses,
/// which is the stable way to read an `ast::Ident`'s text without holding a
/// `Sources` handle.
fn check_entrypoints(
    file: &ast::File,
    source: &str,
    laws: &UniverseLaws,
    out: &mut Vec<LawViolation>,
) {
    use rune::ast::Spanned;
    let contract = &laws.entrypoints;
    let mut found_any_recognized = false;

    for (item, _semi) in &file.items {
        let ast::Item::Fn(item_fn) = item else { continue };
        let name_span = item_fn.name.span();
        let start = name_span.start.0 as usize;
        let end = name_span.end.0 as usize;
        let name = source.get(start..end).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let arity = item_fn.args.len();

        if let Some((_, expected_arity)) =
            contract.allowed.iter().find(|(n, _)| *n == name)
        {
            found_any_recognized = true;
            if arity != *expected_arity {
                let (line, column) = byte_offset_to_linecol(source, start);
                out.push(LawViolation {
                    law_id: "ENTRYPOINT_ARITY".into(),
                    message: format!(
                        "entrypoint `{}` has arity {} but the contract requires {}",
                        name, arity, expected_arity
                    ),
                    line,
                    column,
                    severity: ViolationSeverity::Fatal,
                    kind: ViolationKind::EntrypointSignature,
                });
            }
        }
    }

    if contract.require_at_least_one && !found_any_recognized {
        let legal: Vec<String> = contract
            .allowed
            .iter()
            .map(|(n, a)| format!("{}/{}", n, a))
            .collect();
        out.push(LawViolation {
            law_id: "NO_ENTRYPOINT".into(),
            message: format!(
                "program defines no recognized entrypoint; expected one of: {}",
                legal.join(", ")
            ),
            line: 0,
            column: 0,
            severity: ViolationSeverity::Fatal,
            kind: ViolationKind::MissingEntrypoint,
        });
    }
}

// ============================================================================
// Effect tier — DEFERRED (async post-hoc audit, NOT per-tick)
// ============================================================================

/// TODO(kernel-effect-tier): run the candidate program in a forked sandbox
/// (`common/src/sandbox`), measure the end-state, and reject on a law-violating
/// outcome (e.g. a program that only uses allowed capabilities but drives a
/// value out of a law's `domain_of_validity`). This is OUT of the per-tick
/// commit gate — it is seconds-slow and runs as async post-hoc audit. A failed
/// effect-tier audit should flag the already-committed rewrite for rollback, not
/// block the tick.
///
/// Returns `None` today (tier not implemented). When implemented, returns the
/// effect-tier sub-verdict to be merged with the static verdict by the audit
/// system.
pub fn evaluate_effect_tier(_source: &str, _laws: &UniverseLaws) -> Option<RewriteVerdict> {
    // Not part of the static, per-tick gate. See module docs.
    None
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract candidate Eustress-API call symbols from parse-validated source.
///
/// Scaffold strategy (documented in module header): a lexical scan that finds
/// `ident(`, `Type::method(`, and Roblox-style `Type.method(` / `recv.method(`
/// call shapes. It deliberately filters out:
/// - calls to functions the program defines itself (local helpers), by first
///   collecting `fn <name>` declarations and excluding them;
/// - language keywords and control-flow (`if`, `for`, `while`, `match`, `loop`,
///   `fn`, `return`).
///
/// Dotted receivers are normalized `.` -> `::` so a catalogued associated
/// constructor like `Instance::new` matches a script's `Instance.new(...)`.
/// Receiver-style calls that DON'T resolve are treated as runtime method
/// dispatch and are not flagged (see [`CallSite::is_receiver_call`]).
///
/// What remains for enforcement is the set of bare free-function call symbols —
/// neither local helpers nor keywords — that must resolve against the capability
/// catalog. That is how an unknown/withheld API call is caught.
///
/// TODO(kernel-ast-walk): replace this lexical pass with a full `ast::Expr`
/// traversal that resolves each `ExprCall` target through the cached
/// `script_editor::analyzer::eustress_context()` so method-style calls and
/// re-exported aliases resolve precisely. Kept lexical for now to avoid
/// version-fragile deep AST recursion (see analyzer.rs notes on Rune version
/// pinning).
fn extract_call_symbols(source: &str) -> Vec<CallSite> {
    let local_fns = collect_local_fn_names(source);
    let mut out = Vec::new();
    let bytes = source.as_bytes();

    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    // A path/segment char: ident, or a `:`/`.` separator.
    let is_path = |b: u8| is_ident(b) || b == b':' || b == b'.';

    let mut i = 0usize;
    while i < bytes.len() {
        // Find an open paren.
        if bytes[i] != b'(' {
            i += 1;
            continue;
        }
        // Walk back over whitespace.
        let mut j = i;
        while j > 0 && bytes[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        // Walk back over a path-ident (incl. `::` and `.`).
        let end = j;
        while j > 0 && is_path(bytes[j - 1]) {
            j -= 1;
        }
        if j < end {
            let raw = &source[j..end];
            let is_receiver_call = raw.contains('.') || raw.contains("::");
            // Normalize Roblox-dotted receivers to Rune path form and trim any
            // separator artifacts from the ends.
            let symbol = raw
                .replace('.', "::")
                .trim_matches(':')
                .to_string();
            if !symbol.is_empty()
                && !is_keyword(&symbol)
                && !local_fns.contains(&last_segment(&symbol).to_string())
                && symbol_head_is_ident(&symbol)
            {
                out.push(CallSite {
                    symbol,
                    is_receiver_call,
                    offset: j,
                });
            }
        }
        i += 1;
    }
    out
}

/// Collect names of functions the program defines, so calls to them are not
/// treated as unknown capabilities. Names are sliced from `source` at each
/// function's name span (the analyzer's stable approach).
fn collect_local_fn_names(source: &str) -> std::collections::HashSet<String> {
    use rune::ast::Spanned;
    let mut set = std::collections::HashSet::new();
    if let Ok(file) = rune::parse::parse_all::<ast::File>(source, rune::SourceId::new(0), true) {
        for (item, _) in &file.items {
            if let ast::Item::Fn(item_fn) = item {
                let span = item_fn.name.span();
                let (s, e) = (span.start.0 as usize, span.end.0 as usize);
                if let Some(name) = source.get(s..e) {
                    if !name.is_empty() {
                        set.insert(name.to_string());
                    }
                }
            }
        }
    }
    set
}

/// True if the symbol's first segment starts with an identifier char (filters
/// out numeric/operator artifacts caught by the back-scan).
fn symbol_head_is_ident(symbol: &str) -> bool {
    symbol
        .as_bytes()
        .first()
        .map(|b| b.is_ascii_alphabetic() || *b == b'_')
        .unwrap_or(false)
}

/// Last `::`-separated segment of a path symbol (`Instance::new` -> `new`).
fn last_segment(symbol: &str) -> &str {
    symbol.rsplit("::").next().unwrap_or(symbol)
}

/// Rune keywords / control-flow heads that can appear before `(` but are not
/// calls.
fn is_keyword(symbol: &str) -> bool {
    matches!(
        last_segment(symbol),
        "if" | "for" | "while" | "match" | "loop" | "fn" | "return" | "let" | "else" | "yield"
    )
}

/// Heuristic path-traversal detection for filesystem capability calls.
fn detect_path_traversal(source: &str, out: &mut Vec<LawViolation>) {
    for fname in ["read_space_file", "write_space_file"] {
        let mut search_from = 0usize;
        while let Some(rel) = source[search_from..].find(fname) {
            let call_at = search_from + rel;
            // Grab the parenthesized argument window (bounded look-ahead).
            // Clamp the end to a char boundary so slicing can't panic on UTF-8.
            let mut window_end = (call_at + 256).min(source.len());
            while window_end < source.len() && !source.is_char_boundary(window_end) {
                window_end += 1;
            }
            let window = &source[call_at..window_end];
            if let Some(open) = window.find('(') {
                let arg_region = &window[open..];
                // Find the first string literal inside the arg region.
                if let Some(lit) = first_string_literal(arg_region) {
                    if path_escapes_root(&lit) {
                        let (line, column) = byte_offset_to_linecol(source, call_at);
                        out.push(LawViolation {
                            law_id: "PATH_TRAVERSAL".into(),
                            message: format!(
                                "`{}` path `{}` escapes the space root",
                                fname, lit
                            ),
                            line,
                            column,
                            severity: ViolationSeverity::Fatal,
                            kind: ViolationKind::PathTraversal,
                        });
                    }
                }
            }
            search_from = call_at + fname.len();
        }
    }
}

/// Extract the first double-quoted string literal from a fragment (no escape
/// handling needed for the scaffold heuristic).
fn first_string_literal(fragment: &str) -> Option<String> {
    let start = fragment.find('"')? + 1;
    let rest = &fragment[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Whether a relative path escapes the space root.
fn path_escapes_root(path: &str) -> bool {
    path.contains("..")
        || path.starts_with('/')
        || path.starts_with('\\')
        // Windows drive prefix like `C:`
        || (path.len() >= 2 && path.as_bytes()[1] == b':')
}

/// 1-based (line, column) for a byte offset into `source`.
fn byte_offset_to_linecol(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

