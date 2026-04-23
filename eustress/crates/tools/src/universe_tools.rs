//! Universe / Space / Script browsing tools — the 10 `eustress_*`
//! tools historically exposed only through the MCP server. Moving them
//! into the shared registry means Workshop gets them too (gated behind
//! `WorkshopMode::UniverseBrowsing`), and adding a new browsing tool
//! requires one change, not two.
//!
//! These are all filesystem-only and therefore run identically whether
//! invoked in-process by the Workshop agent or out-of-process by an
//! external MCP client. The Universe directory is discovered via
//! `ToolContext.universe_root` — the MCP server resolves it from
//! `~/Documents/Eustress/` + optional env override; the engine
//! passes its current `SpaceRoot` parent.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// List Universes
// ---------------------------------------------------------------------------

pub struct ListUniversesTool;

impl ToolHandler for ListUniversesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_universes",
            description: "List every Universe under the user's Eustress documents folder. Returns directory names ready to pass as `universe` arguments to other tools.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        // Walk up from the current universe_root to find the parent
        // "Eustress" directory that holds all Universes. Fallback:
        // use the standard Documents/Eustress location.
        let eustress_root = ctx
            .universe_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_eustress_root);

        let mut universes = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&eustress_root) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        // Only treat directories that look like Universe folders
                        // — contain at least one of Spaces/, Workspace/, .eustress/.
                        let p = entry.path();
                        if p.join("Spaces").exists()
                            || p.join("Workspace").exists()
                            || p.join(".eustress").exists()
                        {
                            universes.push(name.to_string());
                        }
                    }
                }
            }
        }
        universes.sort();

        ToolResult {
            tool_name: "list_universes".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Found {} universe(s): {}", universes.len(), universes.join(", ")),
            structured_data: Some(serde_json::json!({
                "universes": universes,
                "eustress_root": eustress_root.to_string_lossy(),
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// List Spaces in a Universe
// ---------------------------------------------------------------------------

pub struct ListSpacesTool;

impl ToolHandler for ListSpacesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_spaces",
            description: "List the Spaces in a Universe. Defaults to the active Universe if `universe` is omitted.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "universe": { "type": "string", "description": "Universe name (e.g. 'Universe1'). Defaults to the active Universe." }
                }
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let universe_name = input.get("universe").and_then(|v| v.as_str());
        let universe_root = match universe_name {
            Some(name) => {
                ctx.universe_root
                    .parent()
                    .unwrap_or(ctx.universe_root.as_path())
                    .join(name)
            }
            None => ctx.universe_root.clone(),
        };

        let spaces_dir = universe_root.join("Spaces");
        let mut spaces = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&spaces_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        spaces.push(name.to_string());
                    }
                }
            }
        }
        spaces.sort();

        ToolResult {
            tool_name: "list_spaces".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("{} space(s) in {}: {}", spaces.len(), universe_root.file_name().and_then(|n| n.to_str()).unwrap_or("Universe"), spaces.join(", ")),
            structured_data: Some(serde_json::json!({
                "spaces": spaces,
                "universe_root": universe_root.to_string_lossy(),
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// List Scripts in the active Space
// ---------------------------------------------------------------------------

pub struct ListScriptsTool;

impl ToolHandler for ListScriptsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_scripts",
            description: "List every Soul script (.rune / .lua / .luau / .soul) in the active Space's SoulService directory.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let soul_dir = ctx.space_root.join("SoulService");
        let mut scripts = Vec::new();
        collect_scripts(&soul_dir, &mut scripts);
        scripts.sort();

        ToolResult {
            tool_name: "list_scripts".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("{} script(s) in SoulService: {}", scripts.len(), scripts.join(", ")),
            structured_data: Some(serde_json::json!({ "scripts": scripts })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Read Script
// ---------------------------------------------------------------------------

pub struct ReadScriptTool;

impl ToolHandler for ReadScriptTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_script",
            description: "Read the source of a Soul script by name (without extension). Searches SoulService for a matching file.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Script name without extension" }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(name) = input.get("name").and_then(|v| v.as_str()) else {
            return ToolResult {
                tool_name: "read_script".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing 'name' argument".into(),
                structured_data: None, stream_topic: None,
            };
        };
        let soul_dir = ctx.space_root.join("SoulService");
        // Try each extension in turn. First match wins.
        for ext in &["rune", "soul", "lua", "luau"] {
            let path = soul_dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(src) => return ToolResult {
                        tool_name: "read_script".to_string(), tool_use_id: String::new(),
                        success: true,
                        content: src.clone(),
                        structured_data: Some(serde_json::json!({
                            "path": path.to_string_lossy(),
                            "extension": ext,
                            "bytes": src.len(),
                        })),
                        stream_topic: None,
                    },
                    Err(e) => return ToolResult {
                        tool_name: "read_script".to_string(), tool_use_id: String::new(),
                        success: false, content: format!("Read failed: {}", e),
                        structured_data: None, stream_topic: None,
                    },
                }
            }
        }
        ToolResult {
            tool_name: "read_script".to_string(), tool_use_id: String::new(),
            success: false, content: format!("Script '{}' not found in SoulService", name),
            structured_data: None, stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// List Assets
// ---------------------------------------------------------------------------

pub struct ListAssetsTool;

impl ToolHandler for ListAssetsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_assets",
            description: "List asset files (meshes, textures, materials) in the active Space's MaterialService and Workspace directories.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let mut assets = Vec::new();
        for dir in ["MaterialService", "Workspace"] {
            collect_assets(&ctx.space_root.join(dir), &mut assets);
        }
        assets.sort();
        assets.truncate(500); // keep payloads manageable
        // Include the asset paths in `content` — structured_data is
        // UI-only; Claude reads this field.
        let body = if assets.is_empty() {
            "No assets found in MaterialService or Workspace.".to_string()
        } else {
            let lines: Vec<String> = assets.iter().map(|a| format!("  - {}", a)).collect();
            format!("Found {} asset file(s):\n{}", assets.len(), lines.join("\n"))
        };
        ToolResult {
            tool_name: "list_assets".to_string(), tool_use_id: String::new(),
            success: true,
            content: body,
            structured_data: Some(serde_json::json!({ "assets": assets })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Find Entity by name (on-disk scan of _instance.toml files)
// ---------------------------------------------------------------------------

pub struct FindEntityTool;

impl ToolHandler for FindEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "find_entity",
            description: "Search the active Space's filesystem for entity definitions whose folder name matches (case-insensitive substring). Returns every matching _instance.toml path.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Substring to match against entity folder names" }
                },
                "required": ["query"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(query) = input.get("query").and_then(|v| v.as_str()).map(|s| s.to_lowercase()) else {
            return ToolResult {
                tool_name: "find_entity".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing 'query'".into(),
                structured_data: None, stream_topic: None,
            };
        };

        let mut hits = Vec::new();
        walk_for_instance_toml(&ctx.space_root, &query, &mut hits);
        hits.sort();
        hits.truncate(200);

        ToolResult {
            tool_name: "find_entity".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{} match(es) for '{}'", hits.len(), query),
            structured_data: Some(serde_json::json!({ "matches": hits })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Search Universe (basic on-disk grep across all text files)
// ---------------------------------------------------------------------------

pub struct SearchUniverseTool;

impl ToolHandler for SearchUniverseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_universe",
            description: "Search across every Space in the active Universe for text matching a query. Scans .toml, .rune, .lua, .md files. Returns paths + line numbers of matches.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Text to search for (case-sensitive substring)" }
                },
                "required": ["query"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(query) = input.get("query").and_then(|v| v.as_str()) else {
            return ToolResult {
                tool_name: "search_universe".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing 'query'".into(),
                structured_data: None, stream_topic: None,
            };
        };

        let spaces_dir = ctx.universe_root.join("Spaces");
        let mut matches = Vec::new();
        grep_tree(&spaces_dir, query, &mut matches, 500);

        ToolResult {
            tool_name: "search_universe".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{} hit(s) for '{}' across Universe", matches.len(), query),
            structured_data: Some(serde_json::json!({ "matches": matches })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Create Script (thin wrapper — delegates to filesystem)
// ---------------------------------------------------------------------------

pub struct CreateScriptTool;

impl ToolHandler for CreateScriptTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "create_script",
            description: "Create a new Soul script file (.rune by default) in the active Space's SoulService directory. The engine's file watcher picks it up on save.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Script name without extension" },
                    "code": { "type": "string", "description": "Script source code" },
                    "language": { "type": "string", "enum": ["rune", "lua"], "default": "rune" }
                },
                "required": ["name", "code"]
            }),
            modes: &[WorkshopMode::UniverseBrowsing, WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.create_script"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(name) = input.get("name").and_then(|v| v.as_str()) else {
            return ToolResult {
                tool_name: "create_script".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing 'name'".into(),
                structured_data: None, stream_topic: None,
            };
        };
        let Some(code) = input.get("code").and_then(|v| v.as_str()) else {
            return ToolResult {
                tool_name: "create_script".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing 'code'".into(),
                structured_data: None, stream_topic: None,
            };
        };
        let language = input.get("language").and_then(|v| v.as_str()).unwrap_or("rune");
        let ext = match language { "lua" => "lua", _ => "rune" };

        let soul_dir = ctx.space_root.join("SoulService");
        let _ = std::fs::create_dir_all(&soul_dir);
        let path = soul_dir.join(format!("{}.{}", name, ext));

        match std::fs::write(&path, code) {
            Ok(_) => ToolResult {
                tool_name: "create_script".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Wrote {} ({} bytes)", path.display(), code.len()),
                structured_data: Some(serde_json::json!({
                    "path": path.to_string_lossy(),
                    "language": language,
                    "bytes": code.len(),
                })),
                stream_topic: Some("workshop.tool.create_script".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "create_script".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Write failed: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Set Default Universe
// ---------------------------------------------------------------------------

pub struct SetDefaultUniverseTool;

impl ToolHandler for SetDefaultUniverseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            // Renamed from `set_default_universe` because the former
            // name collided semantically with the MCP server's
            // session-mutating `eustress_set_default_universe` /
            // `set_active_universe` — clients picked this one by
            // default and got the sentinel-only behavior. This tool
            // ONLY writes the sentinel file the engine reads at
            // startup; it does not change which Universe the current
            // tool-call session resolves paths against.
            name: "set_next_launch_universe",
            description: "Write the given Universe name to the sentinel file the engine reads at startup, so it opens that Universe on its NEXT launch. Does NOT change the universe the current MCP session operates against — call `set_active_universe` for that.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "universe": { "type": "string", "description": "Universe folder name under ~/Documents/Eustress/" }
                },
                "required": ["universe"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(name) = input.get("universe").and_then(|v| v.as_str()) else {
            return ToolResult {
                tool_name: "set_next_launch_universe".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing 'universe'".into(),
                structured_data: None, stream_topic: None,
            };
        };
        // Sentinel lives next to the Eustress documents root so it
        // survives per-Universe resets.
        let sentinel = ctx
            .universe_root
            .parent()
            .unwrap_or(ctx.universe_root.as_path())
            .join(".default_universe");
        match std::fs::write(&sentinel, name) {
            Ok(_) => ToolResult {
                tool_name: "set_next_launch_universe".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Set default Universe to '{}'", name),
                structured_data: Some(serde_json::json!({
                    "sentinel": sentinel.to_string_lossy(),
                    "universe": name,
                })),
                stream_topic: None,
            },
            Err(e) => ToolResult {
                tool_name: "set_next_launch_universe".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Write failed: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Get Conversation (placeholder — reads stored Workshop conversation)
// ---------------------------------------------------------------------------

pub struct GetConversationTool;

impl ToolHandler for GetConversationTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_conversation",
            description: "Read a Workshop conversation by session id from the on-disk conversation store.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return ToolResult {
                tool_name: "get_conversation".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing 'session_id'".into(),
                structured_data: None, stream_topic: None,
            };
        };
        // Workshop conversations are persisted under
        // `<universe>/.eustress/workshop/sessions/<id>.json` —
        // convention from `workshop/persistence.rs`.
        let path = ctx.universe_root
            .join(".eustress")
            .join("workshop")
            .join("sessions")
            .join(format!("{}.json", session_id));
        match std::fs::read_to_string(&path) {
            Ok(body) => ToolResult {
                tool_name: "get_conversation".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Loaded conversation '{}' ({} bytes)", session_id, body.len()),
                structured_data: Some(serde_json::json!({
                    "session_id": session_id,
                    "json": body,
                })),
                stream_topic: None,
            },
            Err(e) => ToolResult {
                tool_name: "get_conversation".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Not found: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_eustress_root() -> std::path::PathBuf {
    if let Ok(env_path) = std::env::var("EUSTRESS_ROOT") {
        return std::path::PathBuf::from(env_path);
    }
    if let Some(docs) = dirs::document_dir() {
        return docs.join("Eustress");
    }
    std::path::PathBuf::from(".")
}

fn collect_scripts(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // recurse shallowly so scripts-in-folders are found too
            collect_scripts(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "rune" | "lua" | "luau" | "soul") {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    out.push(name.to_string());
                }
            }
        }
    }
}

fn collect_assets(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_assets(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(
                ext,
                "glb" | "gltf" | "obj" | "fbx" | "png" | "jpg" | "jpeg" | "ktx2" | "dds"
            ) {
                if let Some(rel) = path.strip_prefix(dir.parent().unwrap_or(dir)).ok() {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
        }
    }
}

fn walk_for_instance_toml(
    dir: &std::path::Path,
    query: &str,
    out: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.to_lowercase().contains(query) && path.join("_instance.toml").exists() {
                    out.push(path.join("_instance.toml").to_string_lossy().to_string());
                }
            }
            walk_for_instance_toml(&path, query, out);
        }
    }
}

fn grep_tree(
    dir: &std::path::Path,
    query: &str,
    out: &mut Vec<serde_json::Value>,
    budget: usize,
) {
    if out.len() >= budget {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if out.len() >= budget {
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            grep_tree(&path, query, out, budget);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if !matches!(ext, "toml" | "rune" | "lua" | "luau" | "md" | "txt") {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&path) else { continue };
            for (idx, line) in body.lines().enumerate() {
                if line.contains(query) {
                    out.push(serde_json::json!({
                        "path": path.to_string_lossy(),
                        "line": idx + 1,
                        "text": line.trim(),
                    }));
                    if out.len() >= budget { return; }
                }
            }
        }
    }
}
