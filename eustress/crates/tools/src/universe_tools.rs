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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_scripts",
            description: "List every script source (.rune / .lua / .luau / .soul) in the active Space's services (SoulService, ServerScriptService, StarterPlayerScripts, ReplicatedStorage and the rest), as Space-relative paths. Workspace is left out; use search_universe there.",
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
        let scripts: Vec<String> = space_script_sources(&ctx.space_root)
            .iter()
            .map(|p| space_relative(&ctx.space_root, p))
            .collect();

        ToolResult {
            tool_name: "list_scripts".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("{} script(s): {}", scripts.len(), scripts.join(", ")),
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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_script",
            description: "Read a script's source by name (\"GameDirector\") or by the Space-relative path list_scripts returns. Searches every service in the active Space.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Script name without extension, or a Space-relative source path" }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let fail = |content: String| ToolResult {
            tool_name: "read_script".to_string(), tool_use_id: String::new(),
            success: false, content,
            structured_data: None, stream_topic: None,
        };
        let Some(name) = input.get("name").and_then(|v| v.as_str()).map(str::trim) else {
            return fail("Missing 'name' argument".into());
        };
        let wanted = name.replace('\\', "/");

        // A script matches by its Space-relative path, by its file stem
        // (Rojo suffix dropped), or by its folder's name.
        let matches: Vec<std::path::PathBuf> = space_script_sources(&ctx.space_root)
            .into_iter()
            .filter(|p| {
                let folder = p.parent().and_then(|d| d.file_name()).and_then(|n| n.to_str()).unwrap_or_default();
                space_relative(&ctx.space_root, p) == wanted || script_stem(p) == wanted || folder == wanted
            })
            .collect();

        let path = match matches.as_slice() {
            [] => return fail(format!("Script '{}' not found in this Space's services; list_scripts shows what exists", name)),
            [one] => one.clone(),
            many => return fail(format!(
                "'{}' matches {} scripts; pass one of these paths: {}",
                name, many.len(),
                many.iter().map(|p| space_relative(&ctx.space_root, p)).collect::<Vec<_>>().join(", "),
            )),
        };
        match std::fs::read_to_string(&path) {
            Ok(src) => ToolResult {
                tool_name: "read_script".to_string(), tool_use_id: String::new(),
                success: true,
                content: src.clone(),
                structured_data: Some(serde_json::json!({
                    "path": path.to_string_lossy(),
                    "relative_path": space_relative(&ctx.space_root, &path),
                    "extension": path.extension().and_then(|e| e.to_str()).unwrap_or_default(),
                    "bytes": src.len(),
                })),
                stream_topic: None,
            },
            Err(e) => fail(format!("Read failed: {}", e)),
        }
    }
}

// ---------------------------------------------------------------------------
// List Assets
// ---------------------------------------------------------------------------

pub struct ListAssetsTool;

impl ToolHandler for ListAssetsTool {
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
            description: "Create a script as a SoulScript folder: <Parent>/<Name>/_instance.toml, <Name>/<Name>.<kind>.luau or <Name>.rune (source code), <Name>/<Name>.md (human summary). The file watcher hot-loads it and Play runs it. language=\"rune\" (default) or \"luau\". For Luau, kind picks where it runs: \"server\" (default), \"client\" (each player, for HUD and input) or \"module\" (returned by require). parent is a Space-relative folder; it defaults to SoulService for server scripts, StarterPlayerScripts for client scripts and ReplicatedStorage for modules.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name":     { "type": "string", "description": "Script name (folder name + display name)" },
                    "code":     { "type": "string", "description": "Script source code" },
                    "language": { "type": "string", "enum": ["rune", "luau", "lua"], "default": "rune" },
                    "kind":     { "type": "string", "enum": ["server", "client", "module"], "default": "server", "description": "Luau only. server runs once on the server, client runs for each player, module is loaded with require()." },
                    "parent":   { "type": "string", "description": "Space-relative folder to create the script in, e.g. \"ServerScriptService\" or \"ReplicatedStorage/Shared\". Omitted: SoulService (server), StarterPlayerScripts (client), ReplicatedStorage (module)." },
                    "summary":  { "type": "string", "description": "Optional short markdown summary for <Name>.md (1-3 sentences). Omitted → a placeholder is generated." }
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
        let summary  = input.get("summary").and_then(|v| v.as_str());
        let fail = |content: String| ToolResult {
            tool_name: "create_script".to_string(), tool_use_id: String::new(),
            success: false, content,
            structured_data: None, stream_topic: None,
        };

        // Every script is a SoulScript folder whose `[script]` block names its
        // source file and run context, the same shape Studio's Insert writes.
        // Play classifies Luau by the source's Rojo suffix, so the suffix and
        // `run_context` always agree.
        let is_luau = matches!(language, "luau" | "lua");
        let kind = input.get("kind").and_then(|v| v.as_str()).unwrap_or("server");
        let (suffix, run_context) = match (is_luau, kind) {
            (true, "server")  => (".server", "Server"),
            (true, "client")  => (".client", "Client"),
            (true, "module")  => (".module", "Module"),
            (false, "server") => ("", "Rune"),
            (false, _) => return fail(format!(
                "kind '{}' applies to Luau scripts; pass language=\"luau\" for a client or module script", kind,
            )),
            (true, other) => return fail(format!(
                "Unknown kind '{}': use server, client or module", other,
            )),
        };
        let ext = if is_luau { "luau" } else { "rune" };

        // The parent folder: the caller's Space-relative path, or the service
        // where this kind of script belongs.
        let parent_dir = match input.get("parent").and_then(|v| v.as_str()).map(str::trim).filter(|p| !p.is_empty()) {
            Some(rel) => {
                let rel_path = std::path::Path::new(rel);
                let stays_inside = rel_path.components().all(|c| matches!(c, std::path::Component::Normal(_)));
                if !stays_inside {
                    return fail(format!(
                        "parent '{}' must be a folder inside the Space, like \"ServerScriptService\" or \"ReplicatedStorage/Shared\"", rel,
                    ));
                }
                ctx.space_root.join(rel_path)
            }
            None => match run_context {
                "Client" => {
                    // Some Spaces have a top-level StarterPlayerScripts
                    // service; the others hold it as a folder inside
                    // StarterPlayer, and Play runs client scripts from both.
                    let top = ctx.space_root.join("StarterPlayerScripts");
                    if top.is_dir() {
                        top
                    } else {
                        ctx.space_root.join("StarterPlayer").join("StarterPlayerScripts")
                    }
                }
                "Module" => ctx.space_root.join("ReplicatedStorage"),
                _ => ctx.space_root.join("SoulService"),
            },
        };

        // 1. Materialise the script folder via the canonical pipeline.
        //    Patches `[metadata].name` to the user-supplied name; the
        //    `[script]` section is rewritten below (the helper's overrides
        //    don't cover arbitrary section fields).
        let class_name = "SoulScript";
        let overrides = eustress_common::instance_create::InstanceOverrides {
            display_name: Some(name.to_string()),
            ..Default::default()
        };
        let created = match eustress_common::instance_create::create_instance(
            &parent_dir,
            class_name,
            Some(name),
            overrides,
        ) {
            Ok(c) => c,
            Err(e) => return ToolResult {
                tool_name: "create_script".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Materialise script folder failed: {}", e),
                structured_data: None, stream_topic: None,
            },
        };

        // 2. Point `[script]` at the source file we're about to drop next to
        //    it, with the run context its suffix implies. Files are named
        //    after the folder so two siblings never share a filename.
        let source_filename = format!("{}{}.{}", created.folder_name, suffix, ext);
        let script_language = if is_luau { "luau" } else { "rune" };
        if let Err(e) = patch_script_block(&created.toml_path, &source_filename, run_context, script_language) {
            return ToolResult {
                tool_name: "create_script".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Patch _instance.toml failed: {}", e),
                structured_data: None, stream_topic: None,
            };
        }

        // 3. Write the source code alongside `_instance.toml`.
        let source_path = created.folder_path.join(&source_filename);
        if let Err(e) = std::fs::write(&source_path, code) {
            return ToolResult {
                tool_name: "create_script".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Write source failed: {}", e),
                structured_data: None, stream_topic: None,
            };
        }

        // 4. `<Name>.md` summary, the cover sheet Studio's script tab opens:
        //    either the caller's text or a sensible placeholder.
        let readme_path = created.folder_path.join(format!("{}.md", created.folder_name));
        let readme = match summary {
            Some(s) if !s.trim().is_empty() => format!(
                "# {}\n\n{}\n\n*Language: {}, runs as: {}*\n",
                name, s.trim(), script_language, run_context,
            ),
            _ => format!(
                "# {}\n\n*Describe what this script does here.*\n\n*Language: {}, runs as: {}*\n",
                name, script_language, run_context,
            ),
        };
        if let Err(e) = std::fs::write(&readme_path, readme) {
            // Non-fatal — the script itself is on disk; a missing summary
            // shouldn't fail the tool call.
            tracing::warn!("create_script: failed to write {}: {}", readme_path.display(), e);
        }

        let relative_folder = created.folder_path.strip_prefix(&ctx.space_root)
            .unwrap_or(&created.folder_path)
            .to_string_lossy()
            .replace('\\', "/");
        ToolResult {
            tool_name: "create_script".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!(
                "Created {} script '{}' at {} ({} source bytes, language={}). It runs the next time Play starts.",
                run_context, name, relative_folder, code.len(), script_language,
            ),
            structured_data: Some(serde_json::json!({
                "class": class_name,
                "name": created.folder_name,
                "folder": created.folder_path.to_string_lossy(),
                "instance_toml": created.toml_path.to_string_lossy(),
                "source_file": source_path.to_string_lossy(),
                "readme": readme_path.to_string_lossy(),
                "language": script_language,
                "run_context": run_context,
                "bytes": code.len(),
            })),
            stream_topic: Some("workshop.tool.create_script".to_string()),
        }
    }
}

/// Rewrite the `[script]` block (source, run context, enabled) and
/// `[properties].language` on a freshly-templated script `_instance.toml`
/// so the file_loader knows which sibling file holds the source code and
/// where it runs. Created next to `CreateScriptTool` because no
/// other surface needs this in-place edit; the canonical instance
/// pipeline's override struct intentionally doesn't expose arbitrary
/// section fields.
fn patch_script_block(
    toml_path: &std::path::Path,
    source_filename: &str,
    run_context: &str,
    language: &str,
) -> Result<(), String> {
    let raw = std::fs::read_to_string(toml_path)
        .map_err(|e| format!("read {}: {}", toml_path.display(), e))?;
    let mut doc: toml::Value = raw.parse()
        .map_err(|e| format!("parse {}: {}", toml_path.display(), e))?;
    if let Some(root) = doc.as_table_mut() {
        let script = root.entry("script".to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let Some(t) = script.as_table_mut() {
            t.insert("source".to_string(), toml::Value::String(source_filename.to_string()));
            t.insert("run_context".to_string(), toml::Value::String(run_context.to_string()));
            t.insert("enabled".to_string(), toml::Value::Boolean(true));
        }
        let properties = root.entry("properties".to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let Some(t) = properties.as_table_mut() {
            t.insert("language".to_string(), toml::Value::String(language.to_string()));
        }
    }
    let out = toml::to_string_pretty(&doc)
        .map_err(|e| format!("serialize {}: {}", toml_path.display(), e))?;
    std::fs::write(toml_path, out)
        .map_err(|e| format!("write {}: {}", toml_path.display(), e))?;
    Ok(())
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
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

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
// New Universe
// ---------------------------------------------------------------------------

pub struct NewUniverseTool;

impl ToolHandler for NewUniverseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "new_universe",
            description: "Create a brand-new Universe folder (with its `Spaces/` subdirectory and `.eustress/` asset dirs) under the Eustress documents root. Fails if the target already exists — never overwrites.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Universe folder name (e.g. 'Universe2')." },
                    "parent_path": { "type": "string", "description": "Optional path, relative to the Eustress documents root, under which to create the Universe. Defaults to the Eustress root itself. No `..` allowed." }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "new_universe";
        let Some(name) = input.get("name").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'name'".into());
        };
        if let Err(e) = validate_path_component(name) {
            return err_result(TOOL, e);
        }

        let eustress_root = ctx
            .universe_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_eustress_root);

        let parent_dir = match input.get("parent_path").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => {
                let cleaned = p.replace('\\', "/");
                if cleaned.contains("..") {
                    return err_result(TOOL, "Invalid parent_path: '..' is not allowed.".into());
                }
                let resolved = eustress_root.join(&cleaned);
                if !resolved.starts_with(&eustress_root) {
                    return err_result(TOOL, "Invalid parent_path: escapes the Eustress root.".into());
                }
                resolved
            }
            _ => eustress_root.clone(),
        };

        let universe_root = parent_dir.join(name);
        if universe_root.exists() {
            return err_result(TOOL, format!("Universe '{}' already exists at {}", name, universe_root.display()));
        }

        if let Err(e) = std::fs::create_dir_all(&universe_root) {
            return err_result(TOOL, format!("Failed to create Universe directory: {}", e));
        }
        for dir in [
            universe_root.join("Spaces"),
            universe_root.join(".eustress").join("assets").join("parts"),
            universe_root.join(".eustress").join("assets").join("meshes"),
            universe_root.join(".eustress").join("knowledge"),
        ] {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return err_result(TOOL, format!("Failed to create {}: {}", dir.display(), e));
            }
        }

        ToolResult {
            tool_name: TOOL.to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Created Universe '{}' at {}", name, universe_root.display()),
            structured_data: Some(serde_json::json!({
                "universe": name,
                "path": universe_root.to_string_lossy(),
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// New Space
// ---------------------------------------------------------------------------

pub struct NewSpaceTool;

impl ToolHandler for NewSpaceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "new_space",
            description: "Create a brand-new Space inside `<universe>/Spaces/<name>/`, scaffolded with the same folder + TOML structure the Studio 'New Space' flow produces (service folders, space.toml, simulation.toml, .eustress manifests, Baseplate + WelcomeCube parts, and Lighting children). Fails if the Space already exists.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "universe": { "type": "string", "description": "Universe folder name the Space is created inside." },
                    "name": { "type": "string", "description": "Space folder name (e.g. 'Space2')." }
                },
                "required": ["universe", "name"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "new_space";
        let Some(universe) = input.get("universe").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'universe'".into());
        };
        let Some(name) = input.get("name").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'name'".into());
        };
        if let Err(e) = validate_path_component(universe) {
            return err_result(TOOL, e);
        }
        if let Err(e) = validate_path_component(name) {
            return err_result(TOOL, e);
        }

        let eustress_root = ctx
            .universe_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_eustress_root);
        let universe_root = eustress_root.join(universe);
        if !universe_root.exists() {
            return err_result(TOOL, format!("Universe '{}' not found at {}", universe, universe_root.display()));
        }

        let spaces_dir = universe_root.join("Spaces");
        let space_root = spaces_dir.join(name);
        if space_root.exists() {
            return err_result(TOOL, format!("Space '{}' already exists at {}", name, space_root.display()));
        }

        let author = ctx.username.clone().unwrap_or_else(|| "Eustress User".to_string());
        match scaffold_new_space(&spaces_dir, name, &author) {
            Ok(created) => ToolResult {
                tool_name: TOOL.to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Created Space '{}' in Universe '{}' at {}", name, universe, created.display()),
                structured_data: Some(serde_json::json!({
                    "universe": universe,
                    "space": name,
                    "path": created.to_string_lossy(),
                })),
                stream_topic: None,
            },
            Err(e) => err_result(TOOL, format!("Failed to scaffold Space: {}", e)),
        }
    }
}

// ---------------------------------------------------------------------------
// Rename Space
// ---------------------------------------------------------------------------

pub struct RenameSpaceTool;

impl ToolHandler for RenameSpaceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "rename_space",
            description: "Rename `<universe>/Spaces/<old_name>/` to `<universe>/Spaces/<new_name>/` on disk (plain filesystem rename — no git operation). If the Space is open in a running engine (locked WorldDb / file handles), the rename fails atomically and this reports that clearly instead of a raw OS error. Also updates the `name` self-reference inside the renamed Space's space.toml and .eustress/project.toml.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "universe": { "type": "string", "description": "Universe folder name." },
                    "old_name": { "type": "string", "description": "Current Space folder name." },
                    "new_name": { "type": "string", "description": "New Space folder name." }
                },
                "required": ["universe", "old_name", "new_name"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "rename_space";
        let Some(universe) = input.get("universe").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'universe'".into());
        };
        let Some(old_name) = input.get("old_name").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'old_name'".into());
        };
        let Some(new_name) = input.get("new_name").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'new_name'".into());
        };
        for component in [universe, old_name, new_name] {
            if let Err(e) = validate_path_component(component) {
                return err_result(TOOL, e);
            }
        }

        let eustress_root = ctx
            .universe_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_eustress_root);
        let spaces_dir = eustress_root.join(universe).join("Spaces");
        let old_path = spaces_dir.join(old_name);
        let new_path = spaces_dir.join(new_name);

        if !old_path.exists() {
            return err_result(TOOL, format!("Space '{}' not found at {}", old_name, old_path.display()));
        }
        if new_path.exists() {
            return err_result(TOOL, format!("A Space named '{}' already exists — refusing to overwrite.", new_name));
        }

        if let Err(e) = std::fs::rename(&old_path, &new_path) {
            return err_result(
                TOOL,
                format!(
                    "Rename failed ({e}). The Space appears to be open in a running engine \
                     (a locked WorldDb file or other open handle inside {}) — close it and retry.",
                    old_path.display()
                ),
            );
        }

        let mut patched = Vec::new();
        if patch_toml_name_field(&new_path.join("space.toml"), "space", old_name, new_name) {
            patched.push("space.toml".to_string());
        }
        if patch_toml_name_field(&new_path.join(".eustress").join("project.toml"), "project", old_name, new_name) {
            patched.push(".eustress/project.toml".to_string());
        }

        ToolResult {
            tool_name: TOOL.to_string(), tool_use_id: String::new(),
            success: true,
            content: format!(
                "Renamed Space '{}' → '{}' in Universe '{}'. Updated self-references in: {}",
                old_name, new_name, universe,
                if patched.is_empty() { "(none found)".to_string() } else { patched.join(", ") }
            ),
            structured_data: Some(serde_json::json!({
                "universe": universe,
                "old_name": old_name,
                "new_name": new_name,
                "path": new_path.to_string_lossy(),
                "patched_files": patched,
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Rename Universe
// ---------------------------------------------------------------------------

pub struct RenameUniverseTool;

impl ToolHandler for RenameUniverseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "rename_universe",
            description: "Rename a Universe folder on disk (plain filesystem rename — no git operation). If the Universe is open in a running engine, the rename fails atomically and this reports that clearly instead of a raw OS error. Also updates the `.default_universe` next-launch sentinel if it pointed at the old name.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "old_name": { "type": "string", "description": "Current Universe folder name." },
                    "new_name": { "type": "string", "description": "New Universe folder name." }
                },
                "required": ["old_name", "new_name"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::UniverseBrowsing],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "rename_universe";
        let Some(old_name) = input.get("old_name").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'old_name'".into());
        };
        let Some(new_name) = input.get("new_name").and_then(|v| v.as_str()) else {
            return err_result(TOOL, "Missing 'new_name'".into());
        };
        for component in [old_name, new_name] {
            if let Err(e) = validate_path_component(component) {
                return err_result(TOOL, e);
            }
        }

        let eustress_root = ctx
            .universe_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_eustress_root);
        let old_path = eustress_root.join(old_name);
        let new_path = eustress_root.join(new_name);

        if !old_path.exists() {
            return err_result(TOOL, format!("Universe '{}' not found at {}", old_name, old_path.display()));
        }
        if new_path.exists() {
            return err_result(TOOL, format!("A Universe named '{}' already exists — refusing to overwrite.", new_name));
        }

        if let Err(e) = std::fs::rename(&old_path, &new_path) {
            return err_result(
                TOOL,
                format!(
                    "Rename failed ({e}). The Universe appears to be open in a running engine \
                     (a locked WorldDb file or other open handle inside {}) — close it and retry.",
                    old_path.display()
                ),
            );
        }

        // The only on-disk "last opened Universe" state found in the codebase
        // is the `.default_universe` next-launch sentinel written by
        // `SetDefaultUniverseTool` / the engine's own picker (see
        // `eustress-engine/src/space/mod.rs` and `engine_bridge/port_file.rs`).
        // Per-user MRU state (`workshop/mention.rs`) lives *inside* each
        // Space's `.eustress/` tree, so it moves with the rename automatically
        // and needs no separate patch.
        let sentinel = eustress_root.join(".default_universe");
        let mut sentinel_updated = false;
        if let Ok(content) = std::fs::read_to_string(&sentinel) {
            if content.trim() == old_name {
                if std::fs::write(&sentinel, new_name).is_ok() {
                    sentinel_updated = true;
                }
            }
        }

        ToolResult {
            tool_name: TOOL.to_string(), tool_use_id: String::new(),
            success: true,
            content: format!(
                "Renamed Universe '{}' → '{}'.{}",
                old_name, new_name,
                if sentinel_updated {
                    " Updated .default_universe next-launch sentinel.".to_string()
                } else {
                    String::new()
                }
            ),
            structured_data: Some(serde_json::json!({
                "old_name": old_name,
                "new_name": new_name,
                "path": new_path.to_string_lossy(),
                "sentinel_updated": sentinel_updated,
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn err_result(tool: &str, msg: String) -> ToolResult {
    ToolResult {
        tool_name: tool.to_string(), tool_use_id: String::new(),
        success: false, content: msg, structured_data: None, stream_topic: None,
    }
}

/// Validate a single path component (Universe name, Space name, …):
/// non-empty, no path separators, and no `..` traversal. Mirrors the
/// `.contains("..")` convention used by `file_tools`/`entity_tools`/
/// `spatial_tools`, extended to also reject embedded separators since
/// these names must stay single path components.
fn validate_path_component(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if trimmed.contains("..") || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(format!("Invalid name '{}': must be a single path component (no '..', '/', or '\\\\').", name));
    }
    if eustress_common::instance_create::is_eep_reserved_name(trimmed) {
        return Err(format!("'{}' is a reserved EEP name and cannot be used.", trimmed));
    }
    Ok(())
}

/// Rewrite `[<table>].name` in a TOML file at `path` from `old_name` to
/// `new_name`, but only when it currently equals `old_name` — never
/// clobbers a name the user already customized independently. Returns
/// `true` if the file was patched.
fn patch_toml_name_field(path: &std::path::Path, table: &str, old_name: &str, new_name: &str) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else { return false };
    let Ok(mut doc) = raw.parse::<toml::Value>() else { return false };
    let Some(root) = doc.as_table_mut() else { return false };
    let Some(section) = root.get_mut(table).and_then(|v| v.as_table_mut()) else { return false };
    let matches_old = section.get("name").and_then(|v| v.as_str()) == Some(old_name);
    if !matches_old {
        return false;
    }
    section.insert("name".to_string(), toml::Value::String(new_name.to_string()));
    let Ok(out) = toml::to_string_pretty(&doc) else { return false };
    std::fs::write(path, out).is_ok()
}

/// Service folder names + class every new Space receives. Mirrors
/// `eustress-engine::space::space_ops::SERVICE_FOLDERS` — kept as a
/// separate copy here because the tools crate (used by the MCP server
/// and the Workshop agent) cannot depend on the engine crate (the
/// engine depends on `eustress-tools`, not the other way around).
const SERVICE_FOLDERS: &[(&str, &str)] = &[
    ("Workspace", "Workspace"),
    ("Lighting", "Lighting"),
    ("Players", "Players"),
    ("StarterGui", "StarterGui"),
    ("StarterPack", "StarterPack"),
    ("StarterPlayerScripts", "StarterPlayerScripts"),
    ("StarterCharacterScripts", "StarterCharacterScripts"),
    ("ReplicatedStorage", "ReplicatedStorage"),
    ("ServerStorage", "ServerStorage"),
    ("ServerScriptService", "ServerScriptService"),
    ("SoulService", "SoulService"),
    ("MaterialService", "MaterialService"),
    ("SoundService", "SoundService"),
    ("AdornmentService", "AdornmentService"),
    ("DataService", "DataService"),
    ("Teams", "Teams"),
    ("Chat", "Chat"),
];

const SIMULATION_TOML_CONTENT: &str = "# Simulation configuration -- SIMULATION_SYSTEM.md\n\
# Controls tick-based time compression for physics and product simulations.\n\
\n\
[simulation]\n\
tick_rate_hz = 60.0\n\
time_scale = 1.0\n\
max_ticks_per_frame = 10\n\
auto_start = false\n\
\n\
[simulation.recording]\n\
enabled = false\n\
output_dir = \".eustress/knowledge/recordings\"\n\
format = \"both\"\n\
auto_export = false\n";

const GITIGNORE: &str = "# Eustress — gitignore\n\
.eustress/local/\n\
.DS_Store\n\
Thumbs.db\n\
desktop.ini\n\
target/\n";

/// Scaffold a brand-new Space at `parent_dir/<space_name>/`, matching the
/// folder + TOML structure `eustress-engine::space::space_ops::scaffold_new_space`
/// produces for the Studio "New Space" flow: `.eustress/` manifests, every
/// service folder (`_service.toml` copied from the shared
/// `common/assets/service_templates/`), `space.toml`, `simulation.toml`,
/// `.gitignore`, a Baseplate + WelcomeCube Part (via the canonical
/// `instance_create::create_instance` pipeline — same one `CreateScriptTool`
/// and `create_entity` use), and Lighting children (Atmosphere/Clouds/Moon/
/// Sky/Sun, also via `create_instance`, each from its class_schema template).
///
/// Returns the new Space's root path on success.
fn scaffold_new_space(parent_dir: &std::path::Path, space_name: &str, author: &str) -> Result<std::path::PathBuf, String> {
    use eustress_common::{
        AssetIndexManifest, PackageIndexManifest, ProjectManifest, ProjectSettingsManifest,
        PublishJournalManifest, PublishManifest, SyncManifest, save_toml_file,
    };

    let space_root = parent_dir.join(space_name);
    if space_root.exists() {
        return Err(format!("Space '{}' already exists at {:?}", space_name, space_root));
    }

    std::fs::create_dir_all(&space_root).map_err(|e| format!("create {:?}: {}", space_root, e))?;
    std::fs::create_dir_all(space_root.join(".eustress").join("local")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(space_root.join(".eustress").join("knowledge")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(space_root.join("src")).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().to_rfc3339();

    save_toml_file(&ProjectManifest::new(space_name, author, &now), &space_root.join(".eustress").join("project.toml"))
        .map_err(|e| format!("write project.toml: {}", e))?;
    save_toml_file(&ProjectSettingsManifest::default(), &space_root.join(".eustress").join("settings.toml"))
        .map_err(|e| format!("write settings.toml: {}", e))?;
    save_toml_file(&SyncManifest::default(), &space_root.join(".eustress").join("sync.toml"))
        .map_err(|e| format!("write sync.toml: {}", e))?;
    save_toml_file(&AssetIndexManifest::default(), &space_root.join(".eustress").join("asset-index.toml"))
        .map_err(|e| format!("write asset-index.toml: {}", e))?;
    save_toml_file(&PackageIndexManifest::default(), &space_root.join(".eustress").join("package-index.toml"))
        .map_err(|e| format!("write package-index.toml: {}", e))?;
    save_toml_file(&PublishManifest::default(), &space_root.join(".eustress").join("publish.toml"))
        .map_err(|e| format!("write publish.toml: {}", e))?;
    save_toml_file(&PublishJournalManifest::new(&now), &space_root.join(".eustress").join("publish-journal.toml"))
        .map_err(|e| format!("write publish-journal.toml: {}", e))?;

    std::fs::write(space_root.join(".gitignore"), GITIGNORE).map_err(|e| e.to_string())?;
    std::fs::write(
        space_root.join("space.toml"),
        format!(
            "# EEP Space metadata\n[space]\nname = \"{}\"\nauthor = \"{}\"\nversion = \"0.1.0\"\ncreated_with = \"Eustress Engine\"\n\n[metadata]\ncreated = \"{}\"\nlast_modified = \"{}\"\n",
            space_name, author, now, now,
        ),
    ).map_err(|e| e.to_string())?;
    std::fs::write(space_root.join("simulation.toml"), SIMULATION_TOML_CONTENT).map_err(|e| e.to_string())?;

    // Service folders — _service.toml copied from the shared templates.
    let svc_template_dir = eustress_common::service_templates_dir();
    for (name, class) in SERVICE_FOLDERS {
        let svc_dir = space_root.join(name);
        std::fs::create_dir_all(&svc_dir).map_err(|e| e.to_string())?;
        let template_path = svc_template_dir.join(name).join("_service.toml");
        if let Ok(content) = std::fs::read_to_string(&template_path) {
            std::fs::write(svc_dir.join("_service.toml"), content).map_err(|e| e.to_string())?;
        } else {
            let toml = format!(
                "# EEP _service.toml — marks this folder as a Service container.\n[service]\nclass_name = \"{class}\"\ncan_have_children = true\n\n[metadata]\nid = \"{id}-service\"\ncreated = \"{now}\"\nlast_modified = \"{now}\"\n",
                class = class, id = class.to_lowercase(), now = now,
            );
            std::fs::write(svc_dir.join("_service.toml"), toml).map_err(|e| e.to_string())?;
        }
    }

    // Baseplate + WelcomeCube via the canonical create-instance pipeline.
    let workspace_dir = space_root.join("Workspace");
    eustress_common::instance_create::create_instance(
        &workspace_dir, "Part", Some("Baseplate"),
        eustress_common::instance_create::InstanceOverrides {
            position: Some([0.0, -0.5, 0.0]),
            scale: Some([512.0, 1.0, 512.0]),
            color_rgba: Some([0.388, 0.373, 0.384, 1.0]),
            reflectance: Some(0.1),
            anchored: Some(true),
            can_collide: Some(true),
            asset_mesh: Some("parts/block.glb".to_string()),
            ..Default::default()
        },
    ).map_err(|e| format!("create Baseplate: {}", e))?;
    eustress_common::instance_create::create_instance(
        &workspace_dir, "Part", Some("WelcomeCube"),
        eustress_common::instance_create::InstanceOverrides {
            position: Some([0.0, 2.0, 0.0]),
            scale: Some([4.0, 4.0, 4.0]),
            color_rgba: Some([0.388, 0.706, 1.0, 1.0]),
            reflectance: Some(0.2),
            anchored: Some(true),
            can_collide: Some(true),
            asset_mesh: Some("parts/block.glb".to_string()),
            ..Default::default()
        },
    ).map_err(|e| format!("create WelcomeCube: {}", e))?;

    // Lighting children, each from its class_schema template, as Studio's
    // scaffold writes them. The Sun is class Star (its [star] section), named
    // Sun. Clouds is the fair-weather layer: with no Clouds object a Space
    // has no clouds (Roblox's rule).
    let lighting_dir = space_root.join("Lighting");
    for (class, name) in [
        ("Atmosphere", "Atmosphere"),
        ("Clouds", "Clouds"),
        ("Moon", "Moon"),
        ("Sky", "Sky"),
        ("Star", "Sun"),
    ] {
        eustress_common::instance_create::create_instance(
            &lighting_dir, class, Some(name),
            eustress_common::instance_create::InstanceOverrides::default(),
        ).map_err(|e| format!("create Lighting/{}: {}", name, e))?;
    }

    tracing::info!("new_space: scaffolded '{}' at {:?}", space_name, space_root);
    Ok(space_root)
}

fn default_eustress_root() -> std::path::PathBuf {
    if let Ok(env_path) = std::env::var("EUSTRESS_ROOT") {
        return std::path::PathBuf::from(env_path);
    }
    if let Some(docs) = dirs::document_dir() {
        return docs.join("Eustress");
    }
    std::path::PathBuf::from(".")
}

fn collect_scripts(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Scripts are folders (`<Name>/_instance.toml` + source), so recurse.
            collect_scripts(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "rune" | "lua" | "luau" | "soul") {
                out.push(path);
            }
        }
    }
}

/// Every script source file in a Space's services, as absolute paths.
/// Workspace is left out: imported worlds can hold hundreds of thousands of
/// folders there, and game scripts live in the services.
fn space_script_sources(space_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(space_root) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !path.is_dir() || name.starts_with('.') || name == "Workspace" || name == "src" {
            continue;
        }
        collect_scripts(&path, &mut out);
    }
    out.sort();
    out
}

/// `GameDirector.server.luau` → `GameDirector`; `script.rune` → `script`.
fn script_stem(path: &std::path::Path) -> String {
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let without_ext = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    for suffix in [".server", ".client", ".module"] {
        if let Some(s) = without_ext.strip_suffix(suffix) {
            return s.to_string();
        }
    }
    without_ext.to_string()
}

fn space_relative(space_root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(space_root).unwrap_or(path).to_string_lossy().replace('\\', "/")
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(dir: &std::path::Path) -> ToolContext {
        // Constructed field by field: ToolContext does not implement Default.
        ToolContext {
            space_root: dir.to_path_buf(),
            universe_root: dir.to_path_buf(),
            user_id: None,
            username: None,
            luau_executor: None,
            display_unit: None,
            cancelled: None,
            permissions: crate::capability::Permissions::default(),
        }
    }

    #[test]
    fn create_script_places_each_kind_where_it_runs_and_read_script_finds_it() {
        let space = std::env::temp_dir().join(format!("eus_create_script_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&space);
        std::fs::create_dir_all(space.join("StarterPlayerScripts")).unwrap();
        let c = ctx(&space);

        let r = CreateScriptTool.execute(json!({ "name": "Hud", "code": "print('hud')", "language": "luau", "kind": "client" }), &c);
        assert!(r.success, "{}", r.content);
        let hud = space.join("StarterPlayerScripts").join("Hud");
        assert!(hud.join("Hud.client.luau").is_file());
        assert!(hud.join("Hud.md").is_file());
        let toml: toml::Value = std::fs::read_to_string(hud.join("_instance.toml")).unwrap().parse().unwrap();
        assert_eq!(toml["script"]["source"].as_str(), Some("Hud.client.luau"));
        assert_eq!(toml["script"]["run_context"].as_str(), Some("Client"));
        assert_eq!(toml["metadata"]["class_name"].as_str(), Some("SoulScript"));

        let r = CreateScriptTool.execute(json!({ "name": "Config", "code": "return {}", "language": "luau", "kind": "module", "parent": "ReplicatedStorage/Shared" }), &c);
        assert!(r.success, "{}", r.content);
        assert!(space.join("ReplicatedStorage/Shared/Config/Config.module.luau").is_file());

        let r = CreateScriptTool.execute(json!({ "name": "Rules", "code": "pub fn main() {}" }), &c);
        assert!(r.success, "{}", r.content);
        assert!(space.join("SoulService/Rules/Rules.rune").is_file());

        let r = CreateScriptTool.execute(json!({ "name": "Nope", "code": "", "kind": "client" }), &c);
        assert!(!r.success, "a Rune client script must be refused");
        let r = CreateScriptTool.execute(json!({ "name": "Nope", "code": "", "language": "luau", "parent": "../Outside" }), &c);
        assert!(!r.success, "a parent outside the Space must be refused");

        let listed = ListScriptsTool.execute(json!({}), &c);
        let scripts = listed.structured_data.unwrap()["scripts"].as_array().unwrap().len();
        assert_eq!(scripts, 3);
        let read = ReadScriptTool.execute(json!({ "name": "Hud" }), &c);
        assert!(read.success, "{}", read.content);
        assert_eq!(read.content, "print('hud')");
        let read = ReadScriptTool.execute(json!({ "name": "ReplicatedStorage/Shared/Config/Config.module.luau" }), &c);
        assert_eq!(read.content, "return {}");

        let _ = std::fs::remove_dir_all(&space);
    }
}