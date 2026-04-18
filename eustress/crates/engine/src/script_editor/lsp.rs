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
use dashmap::DashMap;
use std::sync::Arc;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// The server instance. Holds one document buffer per open URI so
/// per-request analysis is O(parse) rather than O(re-read from disk).
pub struct EustressLsp {
    pub client: Client,
    /// URI → current source text. Populated on `did_open`, mutated on
    /// `did_change`, removed on `did_close`.
    pub docs: Arc<DashMap<Url, String>>,
}

impl EustressLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            docs: Arc::new(DashMap::new()),
        }
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
    async fn initialize(&self, _params: InitializeParams) -> LspResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // '.' could be a trigger once we grow member completion
                    // (Phase 5 is currently prefix-only).
                    trigger_characters: None,
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
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

    async fn shutdown(&self) -> LspResult<()> { Ok(()) }

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
        self.refresh(params.text_document.uri).await;
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
        let result = analyzer::analyze(&source);
        let defs = result.symbols.resolve(&ident);
        if defs.is_empty() { return Ok(None) }

        let locations: Vec<Location> = defs.iter().map(|s| Location {
            uri: uri.clone(),
            range: to_lsp_range(s.range),
        }).collect();

        Ok(Some(GotoDefinitionResponse::Array(locations)))
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);
        let Some((ident, _)) = analyzer::identifier_at(&source, line, col) else {
            return Ok(None);
        };
        let result = analyzer::analyze(&source);
        let refs = result.symbols.resolve(&ident);

        let locations: Vec<Location> = refs.iter().map(|s| Location {
            uri: uri.clone(),
            range: to_lsp_range(s.range),
        }).collect();
        Ok(Some(locations))
    }

    // ── Informational ─────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let result = analyzer::analyze(&source);

        // Prefer a diagnostic under the cursor (Phase 2 hover tooltip
        // content), falling back to the symbol definition if no diagnostic.
        let (line, col) = lsp_pos_to_line_col(pos);
        for d in &result.diagnostics {
            if d.range.start_line <= line && line <= d.range.end_line
                && d.range.start_column <= col && col <= d.range.end_column
            {
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(
                        format!("**{:?}**: {}", d.severity, d.message)
                    )),
                    range: Some(to_lsp_range(d.range)),
                }));
            }
        }

        if let Some((ident, _)) = analyzer::identifier_at(&source, line, col) {
            let defs = result.symbols.resolve(&ident);
            if let Some(def) = defs.first() {
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(
                        format!("`{}` — {:?}", ident, def.kind)
                    )),
                    range: Some(to_lsp_range(def.range)),
                }));
            }
        }
        Ok(None)
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(source) = self.docs.get(&uri).map(|s| s.clone()) else { return Ok(None) };

        let (line, col) = lsp_pos_to_line_col(pos);
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
        let edits = analyzer::rename(&source, &old_name, &new_name);
        if edits.is_empty() { return Ok(None) }

        let lsp_edits: Vec<TextEdit> = edits.iter().map(|e| TextEdit {
            range: byte_range_to_lsp(&source, e.byte_range),
            new_text: e.new_text.clone(),
        }).collect();

        let mut changes = std::collections::HashMap::new();
        changes.insert(uri, lsp_edits);

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
