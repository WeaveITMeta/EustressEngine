//! Bridge to the shared `eustress-tools` crate.
//!
//! The MCP server's historical tool surface (see `tools.rs`) was 13
//! hand-rolled handlers with a function-pointer shape. The shared
//! `eustress-tools` crate now hosts ~40+ handlers under a trait-based
//! registry — the same registry the engine uses for Workshop — so
//! every tool in one surface is available in the other.
//!
//! This module exposes the shared registry's tools to MCP's
//! `tools/list` + `tools/call` by building a `ToolContext` from the
//! server's current Universe state on each invocation.

use eustress_tools::{ToolContext, ToolRegistry, ToolResult};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Process-wide registry built once at first use. Tool handlers are
/// stateless between calls, so sharing one registry is safe.
static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();

fn registry() -> &'static ToolRegistry {
    REGISTRY.get_or_init(|| eustress_tools::default_registry())
}

/// Build a `ToolContext` from the server's current Universe. When no
/// Universe is resolved yet, we return `None` — the caller can surface
/// a helpful error to the LLM rather than dispatching into a tool
/// that would write to a nonsensical path.
pub fn build_context(universe: Option<&PathBuf>) -> Option<ToolContext> {
    let universe = universe?.clone();

    // The MCP server doesn't know which Space inside the Universe is
    // "active" — there may be many. Default to the first Space under
    // `Spaces/`, falling back to the Universe root itself for tools
    // that don't care. Tools that need a specific Space must take
    // `space` as an input parameter.
    let spaces_dir = universe.join("Spaces");
    let space_root = std::fs::read_dir(&spaces_dir)
        .ok()
        .and_then(|mut it| it.find_map(|e| e.ok().map(|e| e.path())))
        .unwrap_or_else(|| universe.clone());

    Some(ToolContext {
        space_root,
        universe_root: universe,
        user_id: None,
        username: None,
    })
}

/// Definitions of every tool in the shared registry. Emitted alongside
/// the MCP server's hand-rolled `eustress_*` tools so `tools/list`
/// returns the full unified surface.
pub fn list_shared_tools() -> Vec<Value> {
    registry()
        .all_tools()
        .into_iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "description": d.description,
                "inputSchema": d.input_schema,
            })
        })
        .collect()
}

/// Attempt to dispatch a tool call through the shared registry.
/// Returns `None` if the tool isn't in the shared registry — the
/// caller should then try the hand-rolled `tools::all_tools()` list.
///
/// Returns `Some(result)` whether the tool succeeded or failed; the
/// caller shouldn't need to distinguish here.
pub fn try_dispatch(
    tool_name: &str,
    args: &Value,
    universe: Option<&PathBuf>,
) -> Option<ToolResult> {
    let r = registry();
    if !r.tool_names().contains(&tool_name) {
        return None;
    }
    let ctx = match build_context(universe) {
        Some(c) => c,
        None => {
            return Some(ToolResult {
                tool_name: tool_name.to_string(),
                tool_use_id: String::new(),
                success: false,
                content:
                    "No Universe resolved. Call `set_active_universe` with the absolute path to a Universe root, or set the `EUSTRESS_UNIVERSE` env var before starting the server, or launch the server from inside a Universe directory."
                        .to_string(),
                structured_data: None,
                stream_topic: None,
            });
        }
    };
    Some(r.dispatch(tool_name, "", args.clone(), &ctx))
}

/// Translate a `ToolResult` into the MCP-expected JSON envelope
/// (`content: [{ type, text }]` + `isError`) so responses match what
/// the hand-rolled tools already return.
pub fn to_mcp_json(result: ToolResult) -> Value {
    let body = if let Some(data) = result.structured_data.as_ref() {
        // Structured payload → JSON-pretty so the LLM sees the shape.
        serde_json::to_string_pretty(data)
            .unwrap_or_else(|_| result.content.clone())
    } else {
        result.content.clone()
    };

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": body,
        }],
        "isError": !result.success,
    })
}

