//! Memory tools — store and recall persistent facts across Workshop sessions.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

pub struct RememberTool;

impl ToolHandler for RememberTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "remember",
            description: "Store a persistent memory across sessions. Use for user preferences, project decisions, material choices. Universe-scoped, synced to Cloudflare.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key (e.g. 'preferred_material')" },
                    "value": { "type": "string", "description": "Memory value" },
                    "category": { "type": "string", "description": "Category: preference, fact, project, contact", "default": "preference" }
                },
                "required": ["key", "value"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.memory.updated"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let value = input.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let category = input.get("category").and_then(|v| v.as_str()).unwrap_or("preference");

        if key.is_empty() || value.is_empty() {
            return ToolResult {
                tool_name: "remember".to_string(), tool_use_id: String::new(),
                success: false, content: "key and value required".into(), structured_data: None, stream_topic: None,
            };
        }

        ToolResult {
            tool_name: "remember".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Stored: [{}] {} = {}", category, key, value),
            structured_data: Some(serde_json::json!({ "action": "remember", "key": key, "value": value, "category": category })),
            stream_topic: Some("workshop.memory.updated".to_string()),
        }
    }
}

pub struct RecallTool;

impl ToolHandler for RecallTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "recall",
            description: "Recall stored memories matching a query. Searches keys, values, and categories.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        ToolResult {
            tool_name: "recall".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Searching memories for: '{}'", query),
            structured_data: Some(serde_json::json!({ "action": "recall", "query": query })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// List Rules
// ---------------------------------------------------------------------------

pub struct ListRulesTool;

impl ToolHandler for ListRulesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_rules",
            description: "List all Workshop rules. Two scopes: global rules (Universe-wide, in .eustress/rules/*.md at Universe root, synced to Cloudflare for cross-device access) and local rules (Space-specific, in {Space}/.rules/*.md). Both are .md files injected into the AI system prompt. Returns scope, filename, and content preview for each rule.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scope": { "type": "string", "description": "Filter by scope: all, global, local", "default": "all" }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let scope = input.get("scope").and_then(|v| v.as_str()).unwrap_or("all");
        let mut rules = Vec::new();

        // Global rules: Universe-level (.eustress/rules/ at Universe root)
        if scope == "all" || scope == "global" {
            let global_dir = ctx.universe_root.join(".eustress").join("rules");
            scan_rules_dir(&global_dir, "global", &mut rules);
        }

        // Local rules: Space-level ({Space}/.rules/)
        if scope == "all" || scope == "local" {
            let local_dir = ctx.space_root.join(".rules");
            scan_rules_dir(&local_dir, "local", &mut rules);
        }

        let global_count = rules.iter().filter(|r| r["scope"] == "global").count();
        let local_count = rules.iter().filter(|r| r["scope"] == "local").count();

        ToolResult {
            tool_name: "list_rules".to_string(), tool_use_id: String::new(),
            success: true,
            content: if rules.is_empty() {
                "No rules found. Create .md files in .eustress/rules/ (global) or {Space}/.rules/ (local).".to_string()
            } else {
                format!("{} rules ({} global, {} local)", rules.len(), global_count, local_count)
            },
            structured_data: Some(serde_json::json!({ "rules": rules, "count": rules.len(), "global": global_count, "local": local_count })),
            stream_topic: None,
        }
    }
}

fn scan_rules_dir(dir: &std::path::Path, scope: &str, rules: &mut Vec<serde_json::Value>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let preview = if content.len() > 200 {
                    format!("{}...", &content[..200])
                } else {
                    content.clone()
                };
                rules.push(serde_json::json!({
                    "scope": scope,
                    "filename": filename,
                    "preview": preview,
                    "size_bytes": content.len(),
                    "path": path.to_string_lossy(),
                }));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// List Workflows
// ---------------------------------------------------------------------------

pub struct ListWorkflowsTool;

impl ToolHandler for ListWorkflowsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_workflows",
            description: "List all Workshop workflows available as /run slash commands. Workflows are .md files in SoulService/.Workflows/ or .eustress/workflows/ that define multi-step instruction sequences the AI can execute.",
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
        let dirs = [
            ctx.space_root.join("SoulService").join(".Workflows"),
            ctx.universe_root.join(".eustress").join("workflows"),
        ];

        let mut workflows = Vec::new();
        for dir in &dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "md").unwrap_or(false) {
                        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                        let command = format!("/run {}", stem);
                        let content = std::fs::read_to_string(&path).unwrap_or_default();
                        let first_line = content.lines().next().unwrap_or("").trim_start_matches('#').trim();
                        workflows.push(serde_json::json!({
                            "name": stem,
                            "command": command,
                            "description": first_line,
                            "path": path.to_string_lossy(),
                        }));
                    }
                }
            }
        }

        ToolResult {
            tool_name: "list_workflows".to_string(), tool_use_id: String::new(),
            success: true,
            content: if workflows.is_empty() {
                "No workflows found. Create .md files in SoulService/.Workflows/ to define slash-command workflows.".to_string()
            } else {
                let cmds: Vec<String> = workflows.iter()
                    .map(|w| w["command"].as_str().unwrap_or("").to_string())
                    .collect();
                format!("{} workflows: {}", workflows.len(), cmds.join(", "))
            },
            structured_data: Some(serde_json::json!({ "workflows": workflows, "count": workflows.len() })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Query Stream Events
// ---------------------------------------------------------------------------

pub struct QueryStreamEventsTool;

impl ToolHandler for QueryStreamEventsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_stream_events",
            description: "Query recent Eustress Stream events from the live simulation. Returns the last N events with topic, summary, and timestamp. Events include entity changes, tool executions, simulation state changes, memory updates, and diff staging.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "count": { "type": "integer", "description": "Number of recent events to return (default: 20, max: 50)", "default": 20 },
                    "topic_filter": { "type": "string", "description": "Filter events by topic prefix (e.g. 'workshop.tool' or 'workshop.simulation')" }
                }
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(20).min(50) as usize;
        let topic_filter = input.get("topic_filter").and_then(|v| v.as_str());

        // Intent — processed by Workshop system that reads StreamAwareContext
        ToolResult {
            tool_name: "query_stream_events".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Querying last {} stream events{}",
                count,
                topic_filter.map(|t| format!(" (topic: {})", t)).unwrap_or_default()),
            structured_data: Some(serde_json::json!({
                "action": "query_stream_events",
                "count": count,
                "topic_filter": topic_filter,
            })),
            stream_topic: None,
        }
    }
}
