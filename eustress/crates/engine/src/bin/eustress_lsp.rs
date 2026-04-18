//! # eustress-lsp — stdio LSP server for the Rune scripting language
//!
//! Tiny binary that plumbs stdin/stdout into [`EustressLsp`]. All actual
//! language intelligence lives in `eustress_engine::script_editor::analyzer`
//! — this file is just the protocol shell.
//!
//! ## Running
//!
//! ```bash
//! cargo build --bin eustress-lsp --features lsp
//! # Then point your editor at the resulting binary. For Windsurf / VS Code:
//! #   "rune.serverPath": "<path>/eustress-lsp"
//! ```
//!
//! The server reads LSP JSON-RPC from stdin and writes to stdout. Logs go
//! through the client via `client.log_message(...)` so they surface in
//! the editor's language-server output pane.
#![cfg(feature = "lsp")]

use eustress_engine::script_editor::lsp::EustressLsp;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    // tracing is intentionally not wired here — LSP clients don't read
    // stderr in a structured way, so we rely on `log_message` to the
    // client for operator-visible signals.
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(EustressLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
