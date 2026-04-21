//! File system tools — read/write files within the Universe sandbox.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;
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
// List Directory
// ---------------------------------------------------------------------------

pub struct ListDirectoryTool;

impl ToolHandler for ListDirectoryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_directory",
            description: "Raw filesystem listing of a directory inside the Universe folder. Unlike `list_space_contents` (which understands entity TOMLs), this returns every file and subfolder as-is — ideal for inspecting services like SoulService, Workshop artifacts, or asset folders where file TYPE matters more than entity semantics. Returns one entry per line prefixed with [DIR] or [FILE]. Path is relative to the Universe root; empty string lists the Universe root itself.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path within the Universe folder. Examples: \"\", \"Space1/SoulService\", \"Space1/Workshop\". Do not use absolute paths or `..`."
                    }
                },
                "required": ["path"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or("").trim();
        // Empty path → list the Universe root itself.
        let resolved = if path_str.is_empty() {
            ctx.universe_root.clone()
        } else {
            match resolve_sandboxed_path(ctx, path_str) {
                Some(p) => p,
                None => return err_result(
                    "list_directory",
                    format!("Path '{}' is outside the Universe sandbox (or contains `..`).", path_str),
                ),
            }
        };

        if !resolved.exists() {
            return err_result("list_directory", format!("Path '{}' does not exist.", path_str));
        }
        if !resolved.is_dir() {
            return err_result(
                "list_directory",
                format!("Path '{}' is a file, not a directory. Use read_file instead.", path_str),
            );
        }

        let entries = match std::fs::read_dir(&resolved) {
            Ok(r) => r,
            Err(e) => return err_result("list_directory", format!("read_dir('{}'): {}", path_str, e)),
        };

        // Split dirs + files, sort each alphabetically, and attach file
        // sizes. Dot-files are still listed so the LLM can see
        // `.eustress/` sidecars, but we don't hide anything.
        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<(String, u64)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let ft = match entry.file_type() { Ok(t) => t, Err(_) => continue };
            if ft.is_dir() {
                dirs.push(name);
            } else if ft.is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push((name, size));
            }
        }
        dirs.sort();
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let mut lines: Vec<String> = Vec::with_capacity(dirs.len() + files.len() + 1);
        for d in &dirs {
            lines.push(format!("[DIR]  {}/", d));
        }
        for (name, size) in &files {
            lines.push(format!("[FILE] {} ({} bytes)", name, size));
        }

        let header = if path_str.is_empty() {
            format!("Contents of Universe root ({} entries):", dirs.len() + files.len())
        } else {
            format!("Contents of \"{}\" ({} entries):", path_str, dirs.len() + files.len())
        };
        let body = if lines.is_empty() {
            format!("{}\n  (empty)", header)
        } else {
            format!("{}\n{}", header, lines.join("\n"))
        };

        ToolResult {
            tool_name: "list_directory".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: body,
            structured_data: Some(serde_json::json!({
                "path": path_str,
                "directories": dirs,
                "files": files.iter().map(|(n, s)| serde_json::json!({ "name": n, "size": s })).collect::<Vec<_>>(),
            })),
            stream_topic: None,
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
