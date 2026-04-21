//! Git tools — commit, diff, log operations on the Universe repository.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;
use std::process::Command;

/// Run a git command in the Universe root directory.
fn git(ctx: &ToolContext, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(&ctx.universe_root)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(format!("git {} failed: {}", args.join(" "), stderr))
    }
}

// ---------------------------------------------------------------------------
// Git Status
// ---------------------------------------------------------------------------

pub struct GitStatusTool;

impl ToolHandler for GitStatusTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "git_status",
            description: "Show the current git status of the Universe repository. Returns modified, staged, and untracked files.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        match git(ctx, &["status", "--porcelain"]) {
            Ok(output) => {
                let lines: Vec<&str> = output.lines().collect();
                ToolResult {
                    tool_name: "git_status".to_string(),
                    tool_use_id: String::new(),
                    success: true,
                    content: if lines.is_empty() {
                        "Working tree clean — no changes".to_string()
                    } else {
                        format!("{} changed files:\n{}", lines.len(), output)
                    },
                    structured_data: Some(serde_json::json!({ "files": lines, "count": lines.len() })),
                    stream_topic: None,
                }
            }
            Err(e) => ToolResult {
                tool_name: "git_status".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: e,
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Git Commit
// ---------------------------------------------------------------------------

pub struct GitCommitTool;

impl ToolHandler for GitCommitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "git_commit",
            description: "Stage all changes and create a git commit in the Universe repository. Generates a descriptive commit message based on the provided summary. Stages all modified and new files before committing.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Commit message describing what changed and why" },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Specific files to stage (if empty, stages all changes)" }
                },
                "required": ["message"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &["workshop.tool.git_commit"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let message = input.get("message").and_then(|v| v.as_str()).unwrap_or("Workshop changes");
        let files = input.get("files").and_then(|v| v.as_array());

        // Stage files
        let stage_result = if let Some(file_list) = files {
            let paths: Vec<&str> = file_list.iter().filter_map(|v| v.as_str()).collect();
            if paths.is_empty() {
                git(ctx, &["add", "-A"])
            } else {
                let mut args = vec!["add", "--"];
                args.extend(paths);
                git(ctx, &args)
            }
        } else {
            git(ctx, &["add", "-A"])
        };

        if let Err(e) = stage_result {
            return ToolResult {
                tool_name: "git_commit".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("Failed to stage: {}", e),
                structured_data: None,
                stream_topic: None,
            };
        }

        // Commit
        match git(ctx, &["commit", "-m", message]) {
            Ok(output) => ToolResult {
                tool_name: "git_commit".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: format!("Committed: {}\n{}", message, output.lines().next().unwrap_or("")),
                structured_data: Some(serde_json::json!({ "message": message })),
                stream_topic: Some("workshop.tool.git_commit".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "git_commit".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: e,
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Git Log
// ---------------------------------------------------------------------------

pub struct GitLogTool;

impl ToolHandler for GitLogTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "git_log",
            description: "Show recent git commit history for the Universe repository. Returns commit hash, author, date, and message for the last N commits.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "count": { "type": "integer", "description": "Number of recent commits to show (default: 10, max: 50)", "default": 10 }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(10).min(50);
        let count_str = format!("-{}", count);

        match git(ctx, &["log", &count_str, "--oneline", "--no-decorate"]) {
            Ok(output) => ToolResult {
                tool_name: "git_log".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: if output.is_empty() { "No commits yet".to_string() } else { output },
                structured_data: None,
                stream_topic: None,
            },
            Err(e) => ToolResult {
                tool_name: "git_log".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: e,
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Git Diff
// ---------------------------------------------------------------------------

pub struct GitDiffTool;

impl ToolHandler for GitDiffTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "git_diff",
            description: "Show the current uncommitted changes in the Universe repository as a unified diff. Optionally filter to a specific file path.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional file path to diff (relative to Universe root)" }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path = input.get("path").and_then(|v| v.as_str());

        let args = if let Some(p) = path {
            vec!["diff", "--", p]
        } else {
            vec!["diff"]
        };

        match git(ctx, &args) {
            Ok(output) => {
                let truncated = if output.len() > 8000 {
                    format!("{}...\n[diff truncated — {} bytes total]", &output[..8000], output.len())
                } else {
                    output
                };
                ToolResult {
                    tool_name: "git_diff".to_string(),
                    tool_use_id: String::new(),
                    success: true,
                    content: if truncated.is_empty() { "No uncommitted changes".to_string() } else { truncated },
                    structured_data: None,
                    stream_topic: None,
                }
            }
            Err(e) => ToolResult {
                tool_name: "git_diff".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: e,
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}
