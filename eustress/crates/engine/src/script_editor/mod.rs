//! # Script Editor — IDE-grade analysis for Rune scripts
//!
//! Foundation layer (**Phase 0**) of the editor's language-intelligence stack.
//! Everything under this module is *pure Rust, UI-free, Bevy-free* so it can
//! be:
//! 1. Unit-tested without a running editor.
//! 2. Re-used by the eventual LSP adapter (Phase 8) without rewriting.
//! 3. Scheduled on `bevy::tasks::AsyncComputeTaskPool` without lifetime games.
//!
//! ## Design
//!
//! The goals — squiggles, hover, go-to-definition, find references, completion,
//! rename, code actions — all read from two data structures that this module
//! produces:
//!
//! ```text
//!   source text ──► analyze() ──► AnalysisResult {
//!                                     diagnostics: Vec<Diagnostic>,
//!                                     symbols: SymbolIndex,
//!                                 }
//! ```
//!
//! The split means the UI layer never talks to the Rune compiler directly;
//! every later phase just reads from `AnalysisResult`.
//!
//! ## Why not LSP
//!
//! LSP (`tower-lsp`, stdio JSON-RPC) makes sense when the editor and analyzer
//! are separate processes. We own both sides in one binary; every LSP request
//! would serialise to JSON just to deserialise back into the same address
//! space. All Phase 1-7 features call `analyze()` directly. An LSP shim over
//! the same API can be added later (Phase 8) for external editors.

pub mod analyzer;
pub mod plugin;
pub mod runtime_snapshot;
pub mod workspace;
#[cfg(feature = "lsp")]
pub mod lsp;

pub use analyzer::{
    analyze, api_lookup, api_starts_with, apply_edits, code_actions, complete,
    hover, identifier_at, line_col_to_offset, prefix_at, render_api_hover,
    rename, signature_help_at,
    AnalysisResult, CodeAction, Completion, CompletionKind, Diagnostic,
    HoverInfo, HoverSource, Range, Severity, Symbol, SymbolIndex, SymbolKind,
    TextEdit, RUNE_KEYWORDS,
};
pub use plugin::{ScriptAnalysis, ScriptAnalysisPlugin};
pub use workspace::{WorkspaceIndex, WorkspaceSymbol};
