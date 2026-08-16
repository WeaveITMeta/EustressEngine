// Hand-rolled tool registry for the Eustress MCP server. Each tool is a pair
// of (JSON Schema description, handler). Schema is what the client sees in
// `tools/list`; handler runs on `tools/call`.
//
// This surface is deliberately down to a single tool — see `all_tools()` for
// why `set_active_universe` can't live in the shared registry. Everything
// else is served from `eustress-tools` via `shared_registry.rs`.

use serde_json::{json, Value};
use std::path::PathBuf;

/// Mutable state held by main.rs and passed to each handler.
pub struct ServerState {
    pub current_universe: Option<PathBuf>,
    pub search_roots: Vec<PathBuf>,
}

/// Tool result shape. Matches the MCP CallToolResult schema: `content` is a
/// list of text blocks; `isError` flips the error flag the client renders.
pub struct ToolResult {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok_json(v: &impl serde::Serialize) -> Self {
        let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into());
        Self {
            content: vec![json!({ "type": "text", "text": text })],
            is_error: false,
        }
    }
    pub fn err(msg: impl AsRef<str>) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": format!("Error: {}", msg.as_ref()) })],
            is_error: true,
        }
    }
    pub fn to_json(&self) -> Value {
        json!({
            "content": self.content,
            "isError": self.is_error,
        })
    }
}

pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    pub handler: fn(&Value, &mut ServerState) -> ToolResult,
}

// ─── eustress_set_default_universe ──────────────────────────────────────

fn set_default_universe_schema() -> Value {
    json!({
        "type": "object",
        "required": ["universe"],
        "properties": {
            "universe": {
                "type": "string",
                "description": "Absolute path to a Universe root (a folder containing `Spaces/`)."
            }
        }
    })
}

fn set_default_universe_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let requested = match args.get("universe").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => u,
        _ => return ToolResult::err("`universe` is required"),
    };
    let abs = PathBuf::from(requested);
    if !abs.join("Spaces").is_dir() {
        return ToolResult::err(format!(
            "'{}' is not a Universe (no Spaces/ directory found).",
            abs.display()
        ));
    }
    let previous = state.current_universe.clone();
    state.current_universe = Some(abs.clone());
    let changed = previous.as_ref() != Some(&abs);
    ToolResult::ok_json(&json!({
        "previous": previous.map(|p| p.display().to_string()),
        "current": state.current_universe.as_ref().map(|p| p.display().to_string()),
        "changed": changed,
    }))
}

pub fn all_tools() -> Vec<ToolDescriptor> {
    // Only ONE hand-rolled tool remains: `set_active_universe`. Every
    // other listing / read / write / search tool that used to live
    // here as `eustress_*` has been migrated to the shared
    // `eustress-tools` registry (see `shared_registry.rs`), so
    // exposing both surfaces produced ~30 duplicate schemas in
    // `tools/list` — bloating the LLM's context and splitting its
    // routing heuristic across two equivalent tools per operation.
    //
    // `set_active_universe` stays hand-rolled because it's the only
    // tool that needs to *mutate* the server's live
    // `ServerState.current_universe`. The shared-registry tools
    // receive an immutable `ToolContext` per call and can't reach
    // the MCP server's session state. The shared crate's
    // `set_next_launch_universe` writes the on-disk sentinel for the
    // next engine launch — complementary, but doesn't change what
    // this MCP session resolves paths against.
    vec![
        ToolDescriptor {
            name: "set_active_universe",
            description: "Switch the MCP session's active Universe. Every subsequent tool call that resolves paths against the Universe (read_file, list_directory, list_space_contents, query_entities, git_*, run_bash, etc.) will operate on the new Universe immediately. Use this when the server's startup-resolved Universe isn't the one you want to work in — the alternative `set_next_launch_universe` only writes a sentinel file read by the ENGINE on its next launch, and has no effect on the current MCP session.",
            input_schema: set_default_universe_schema,
            handler: set_default_universe_handler,
        },
    ]
}
