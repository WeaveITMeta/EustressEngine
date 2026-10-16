//! Git tools — commit, diff, log operations on the Universe repository.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

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

/// Run a git command in the nearest `.git`-rooted ancestor of the active
/// Space root. The actual repo lives at `Spaces/<Space>/.git` (BELOW the
/// Universe root), so we walk up from `ctx.space_root`, not
/// `ctx.universe_root` — an upward walk from the Universe root can never
/// reach a repo nested under it (Gap 9). Falls back to `ctx.space_root` if
/// no repo is found so the error clearly says "not a git repository".
///
/// Hooks are off ([`crate::sandbox::git_command`]): a hook is a program the
/// repository names, and a tool never runs one for its caller.
fn git(ctx: &ToolContext, args: &[&str]) -> Result<String, String> {
    let cwd = find_git_root(&ctx.space_root).unwrap_or_else(|| ctx.space_root.clone());
    let output = crate::sandbox::git_command(&cwd)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(format!("git {} failed (cwd={}): {}", args.join(" "), cwd.display(), stderr))
    }
}

/// [`git`] for a command that changes the repository or its working tree
/// (add, commit, a branch made, switched, deleted or merged): only in a
/// repository whose root is the Universe or inside it. The upward walk can
/// find a repository above the Universe, one that may hold the engine's own
/// sources or the person's other projects, where a switch or merge rewrites
/// the whole working tree; there tools read (status, log, diff) and leave
/// the changes to the person.
fn git_write(ctx: &ToolContext, args: &[&str]) -> Result<String, String> {
    let root = find_git_root(&ctx.space_root)
        .ok_or_else(|| format!("{} is not inside a git repository", ctx.space_root.display()))?;
    if !crate::sandbox::is_within(&root, &ctx.universe_root) {
        return Err(format!(
            "the repository at {} holds the Universe rather than sitting inside it; tools only read it \
             (status, log, diff), and commits, branches and merges there are the person's to make",
            root.display()
        ));
    }
    git(ctx, args)
}

// ---------------------------------------------------------------------------
// Git Status
// ---------------------------------------------------------------------------

pub struct GitStatusTool;

impl ToolHandler for GitStatusTool {
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
                git_write(ctx, &["add", "-A"])
            } else {
                let mut args = vec!["add", "--"];
                args.extend(paths);
                git_write(ctx, &args)
            }
        } else {
            git_write(ctx, &["add", "-A"])
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
        match git_write(ctx, &["commit", "-m", message]) {
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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
                    format!("{}...\n[diff truncated — {} bytes total]", crate::file_tools::utf8_prefix(&output, 8000), output.len())
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
            description: "List, create, switch, delete, or merge git branches in the Universe repository. Actions: 'list' (default), 'create', 'switch', 'delete', 'merge'. Experiment branches (exp/<name>-<ts>) are created by run_experiment; use merge to accept a winning configuration.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Branch action: list, create, switch, delete, merge", "default": "list" },
                    "name": { "type": "string", "description": "Branch name (required for create, switch, delete, merge)" }
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
        // The name reaches git as an argument, so it must be a branch name and
        // never an option such as `--orphan`.
        let name = if name.is_empty() || action == "list" {
            name
        } else {
            match crate::sandbox::git_branch_name(name) {
                Ok(n) => n,
                Err(e) => return ToolResult {
                    tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                    success: false, content: e, structured_data: None, stream_topic: None,
                },
            }
        };

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
                match git_write(ctx, &["checkout", "-b", name]) {
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
                // `--`: the name is a branch, never a file to restore.
                match git_write(ctx, &["checkout", name, "--"]) {
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
                match git_write(ctx, &["branch", "-d", name]) {
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
            "merge" => {
                if name.is_empty() {
                    return ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: false, content: "Branch name required for 'merge'".to_string(),
                        structured_data: None, stream_topic: None,
                    };
                }
                let msg = format!("Merge experiment branch '{}'", name);
                match git_write(ctx, &["merge", "--no-ff", name, "-m", &msg]) {
                    Ok(output) => ToolResult {
                        tool_name: "git_branch".to_string(), tool_use_id: String::new(),
                        success: true,
                        content: format!("Merged '{}' into current branch.\n{}", name, output.trim()),
                        structured_data: Some(serde_json::json!({ "action": "merge", "branch": name })),
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
                content: format!("Unknown action '{}'. Use: list, create, switch, delete, merge.", action),
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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
        // Each ref reaches git inside one argument, so neither may be read as
        // an option: `--output=<file>` would write a file.
        let (base, compare) = match (crate::sandbox::git_revision(base), crate::sandbox::git_revision(compare)) {
            (Ok(b), Ok(c)) => (b, c),
            (Err(e), _) | (_, Err(e)) => return ToolResult {
                tool_name: "feedback_diff".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: e,
                structured_data: None,
                stream_topic: None,
            },
        };
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
                    format!("{}...\n[diff truncated — {} bytes total]", crate::file_tools::utf8_prefix(&output, 12000), output.len())
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
