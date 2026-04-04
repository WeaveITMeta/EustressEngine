//! File system tools — read/write files within the Universe sandbox.

use super::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;
use std::path::PathBuf;

fn resolve_sandboxed_path(ctx: &ToolContext, relative_path: &str) -> Option<PathBuf> {
    let cleaned = relative_path.replace('\\', "/");
    if cleaned.contains("..") { return None; }
    let resolved = ctx.universe_root.join(&cleaned);
    if resolved.starts_with(&ctx.universe_root) { Some(resolved) } else { None }
}

fn err_result(tool: &str, msg: String) -> ToolResult {
    ToolResult { tool_name: tool.to_string(), tool_use_id: String::new(), success: false, content: msg, structured_data: None, stream_topic: None }
}

// ---------------------------------------------------------------------------
// Read File
// ---------------------------------------------------------------------------

pub struct ReadFileTool;

impl ToolHandler for ReadFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file",
            description: "Read a file from the Universe folder. Path is relative to Universe root. Binary files not supported.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Relative path within Universe folder" } },
                "required": ["path"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path_str = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return err_result("read_file", "Missing: path".into()),
        };
        let resolved = match resolve_sandboxed_path(ctx, path_str) {
            Some(p) => p,
            None => return err_result("read_file", format!("Path '{}' outside sandbox", path_str)),
        };
        match std::fs::read_to_string(&resolved) {
            Ok(content) => {
                let truncated = if content.len() > 10_000 {
                    format!("{}...\n[truncated — {} bytes]", &content[..10_000], content.len())
                } else { content };
                ToolResult {
                    tool_name: "read_file".to_string(), tool_use_id: String::new(), success: true,
                    content: truncated,
                    structured_data: Some(serde_json::json!({ "path": resolved.to_string_lossy() })),
                    stream_topic: None,
                }
            }
            Err(e) => err_result("read_file", format!("Failed to read '{}': {}", path_str, e)),
        }
    }
}

// ---------------------------------------------------------------------------
// Write File
// ---------------------------------------------------------------------------

pub struct WriteFileTool;

impl ToolHandler for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file",
            description: "Write content to a file in the Universe folder. Creates parent dirs. Engine hot-reloads TOML files.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within Universe folder" },
                    "content": { "type": "string", "description": "File content" }
                },
                "required": ["path", "content"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.write_file"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path_str = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return err_result("write_file", "Missing: path".into()),
        };
        let content = match input.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return err_result("write_file", "Missing: content".into()),
        };
        let resolved = match resolve_sandboxed_path(ctx, path_str) {
            Some(p) => p,
            None => return err_result("write_file", format!("Path '{}' outside sandbox", path_str)),
        };
        if let Some(parent) = resolved.parent() { let _ = std::fs::create_dir_all(parent); }
        match std::fs::write(&resolved, content) {
            Ok(_) => ToolResult {
                tool_name: "write_file".to_string(), tool_use_id: String::new(), success: true,
                content: format!("Wrote {} bytes to {}", content.len(), resolved.display()),
                structured_data: Some(serde_json::json!({ "path": resolved.to_string_lossy(), "size": content.len() })),
                stream_topic: Some("workshop.tool.write_file".to_string()),
            },
            Err(e) => err_result("write_file", format!("Failed: {}", e)),
        }
    }
}
