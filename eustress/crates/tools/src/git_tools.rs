//! Git tools — commit, diff, log operations on the Universe repository.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;
use std::process::Command;

/// Walk upward from `start` looking for a `.git` directory, returning
/// the first ancestor that contains one. Universe folders are usually
/// children of a larger repo (e.g. `~/Documents/Eustress/Universe1` is
/// inside a personal notes repo, or the Eustress installer repo) —
/// running `git` directly in the Universe folder errors with
/// "not a git repository" even when the Universe is under version
/// control from a higher-level root.
fn find_git_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = start;
    loop {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        match current.parent() {
            Some(p) => current = p,
            None    => return None,
        }
    }
}

/// Run a git command in the nearest `.git`-rooted ancestor of the
/// Universe root. Falls back to `ctx.universe_root` if no repo is
/// found so the error message clearly says "not a git repository"
/// rather than bubbling up a misleading path.
fn git(ctx: &ToolContext, args: &[&str]) -> Result<String, String> {
    let cwd = find_git_root(&ctx.universe_root).unwrap_or_else(|| ctx.universe_root.clone());
    let output = Command::new("git")
        .args(args)
        .current_dir(&cwd)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(format!("git {} failed (cwd={}): {}", args.join(" "), cwd.display(), stderr))
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

// ---------------------------------------------------------------------------
// Git Branch
// ---------------------------------------------------------------------------

pub struct GitBranchTool;

impl ToolHandler for GitBranchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "git_branch",
            description: "List, create, switch, or delete git branches in the Universe repository. Actions: 'list' (default), 'create', 'switch', 'delete'. Prototype lattice branches (v0001, v0002, ...) are managed here.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Branch action: list, create, switch, delete", "default": "list" },
                    "name": { "type": "string", "description": "Branch name (required for create, switch, delete)" }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &["workshop.tool.git_branch"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");

        match action {
            "list" => {
                match git(ctx, &["branch", "-a", "--no-color"]) {
                    Ok(output) => {
                        let branches: Vec<&str> = output.lines().collect();
                        let current = branches.iter()
                            .find(|b| b.starts_with('*'))
                            .map(|b| b.trim_start_matches("* ").trim())
                            .unwrap_or("(detached)");
                        ToolResult {
                            tool_name: "git_branch".to_string(),
                            tool_use_id: String::new(),
                            success: true,
                            content: format!("Current: {}\n{} branch(es):\n{}", current, branches.len(), output),
                            structured_data: Some(serde_json::json!({
                                "current": current,
                                "branches": branches.iter().map(|b| b.trim()).collect::<Vec<_>>(),
                                "count": branches.len(),
                            })),
                            stream_topic: None,
                        }
                    }
                    Err(e) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: e, structured_data: None, stream_topic: None,
                    },
                }
            }
            "create" => {
                if name.is_empty() {
                    return ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: "Branch name required for 'create'".to_string(),
                        structured_data: None, stream_topic: None,
                    };
                }
                match git(ctx, &["checkout", "-b", name]) {
                    Ok(output) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: true,
                        content: format!("Created and switched to branch '{}'\n{}", name, output.trim()),
                        structured_data: Some(serde_json::json!({ "action": "create", "branch": name })),
                        stream_topic: Some("workshop.tool.git_branch".to_string()),
                    },
                    Err(e) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: e, structured_data: None, stream_topic: None,
                    },
                }
            }
            "switch" => {
                if name.is_empty() {
                    return ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: "Branch name required for 'switch'".to_string(),
                        structured_data: None, stream_topic: None,
                    };
                }
                match git(ctx, &["checkout", name]) {
                    Ok(output) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: true,
                        content: format!("Switched to branch '{}'\n{}", name, output.trim()),
                        structured_data: Some(serde_json::json!({ "action": "switch", "branch": name })),
                        stream_topic: Some("workshop.tool.git_branch".to_string()),
                    },
                    Err(e) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: e, structured_data: None, stream_topic: None,
                    },
                }
            }
            "delete" => {
                if name.is_empty() {
                    return ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: "Branch name required for 'delete'".to_string(),
                        structured_data: None, stream_topic: None,
                    };
                }
                match git(ctx, &["branch", "-d", name]) {
                    Ok(output) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: true,
                        content: format!("Deleted branch '{}'\n{}", name, output.trim()),
                        structured_data: Some(serde_json::json!({ "action": "delete", "branch": name })),
                        stream_topic: Some("workshop.tool.git_branch".to_string()),
                    },
                    Err(e) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: e, structured_data: None, stream_topic: None,
                    },
                }
            }
            _ => ToolResult {
                tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                success: false,
                content: format!("Unknown action '{}'. Use: list, create, switch, delete.", action),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Feedback Diff — parameterized comparison between branches, commits, or paths
// ---------------------------------------------------------------------------

pub struct FeedbackDiffTool;

impl ToolHandler for FeedbackDiffTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "feedback_diff",
            description: "Compare two git refs (branches, commits, tags) or file paths to produce a structured diff. Use for prototype lattice comparisons (e.g. v0001 vs v0003), scenario branch evaluation, or before/after analysis of simulation parameter changes.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "base": { "type": "string", "description": "Base ref — branch name, commit hash, or tag (e.g. 'v0001', 'main', 'HEAD~3')" },
                    "compare": { "type": "string", "description": "Compare ref — branch name, commit hash, or tag (e.g. 'v0003', 'HEAD')" },
                    "path": { "type": "string", "description": "Optional path filter — only show diff for files under this path (relative to repo root)" },
                    "stat_only": { "type": "boolean", "description": "If true, return only file-level change summary (insertions/deletions) instead of full diff", "default": false }
                },
                "required": ["base", "compare"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let base = input.get("base").and_then(|v| v.as_str()).unwrap_or("HEAD~1");
        let compare = input.get("compare").and_then(|v| v.as_str()).unwrap_or("HEAD");
        let path = input.get("path").and_then(|v| v.as_str());
        let stat_only = input.get("stat_only").and_then(|v| v.as_bool()).unwrap_or(false);

        let range = format!("{}..{}", base, compare);
        let mut args = if stat_only {
            vec!["diff", "--stat", &range]
        } else {
            vec!["diff", &range]
        };
        if let Some(p) = path {
            args.push("--");
            args.push(p);
        }

        match git(ctx, &args) {
            Ok(output) => {
                let truncated = if output.len() > 12000 {
                    format!("{}...\n[diff truncated — {} bytes total]", &output[..12000], output.len())
                } else {
                    output.clone()
                };

                // Parse stat summary if available
                let files_changed = output.lines()
                    .filter(|l| l.contains("| ") || l.starts_with("diff --git"))
                    .count();

                ToolResult {
                    tool_name: "feedback_diff".to_string(),
                    tool_use_id: String::new(),
                    success: true,
                    content: if truncated.is_empty() {
                        format!("No differences between {} and {}", base, compare)
                    } else {
                        format!("Diff {} → {} ({} file(s) changed):\n{}", base, compare, files_changed, truncated)
                    },
                    structured_data: Some(serde_json::json!({
                        "base": base,
                        "compare": compare,
                        "files_changed": files_changed,
                        "stat_only": stat_only,
                    })),
                    stream_topic: None,
                }
            }
            Err(e) => ToolResult {
                tool_name: "feedback_diff".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("feedback_diff {} → {} failed: {}", base, compare, e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}
