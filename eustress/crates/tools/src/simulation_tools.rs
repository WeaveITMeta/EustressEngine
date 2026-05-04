//! Simulation bridge tools — expose simulation APIs to the AI agent.
//!
//! These tools bridge the gap between the Rune scripting API and the MCP
//! tool interface. They return structured intents that the Workshop system
//! processes against Bevy ECS resources (WatchPointRegistry, CollectionService).
//!
//! Tools:
//! - get_sim_value: read a simulation watchpoint
//! - set_sim_value: write a simulation watchpoint
//! - list_sim_values: list all active watchpoints with values
//! - get_tagged_entities: find entities by CollectionService tag
//! - raycast: cast a ray into the 3D scene and return hit results

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Get Simulation Value
// ---------------------------------------------------------------------------

pub struct GetSimValueTool;

impl ToolHandler for GetSimValueTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_sim_value",
            description: "Read a simulation watchpoint value from the running simulation. Watchpoints are named numeric values tracked during simulation. Common keys: voltage, soc, temperature, pressure, dendrite_risk, cycle_count, capacity_wh, efficiency.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Watchpoint key name" }
                },
                "required": ["key"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        match read_sim_snapshot(ctx) {
            Ok(snap) => {
                if let Some(val) = snap.sim_values.get(key) {
                    ToolResult {
                        tool_name: "get_sim_value".to_string(),
                        tool_use_id: String::new(),
                        success: true,
                        content: format!(
                            "{} = {} (play_state={}, snapshot {}ms old)",
                            key, val, snap.play_state, snap.age_ms,
                        ),
                        structured_data: Some(serde_json::json!({
                            "key": key,
                            "value": val,
                            "play_state": snap.play_state,
                            "snapshot_age_ms": snap.age_ms,
                        })),
                        stream_topic: None,
                    }
                } else {
                    ToolResult {
                        tool_name: "get_sim_value".to_string(),
                        tool_use_id: String::new(),
                        success: false,
                        content: format!(
                            "No watchpoint named '{}' in current snapshot. Known keys: {}",
                            key,
                            snap.sim_values.keys().cloned().collect::<Vec<_>>().join(", "),
                        ),
                        structured_data: Some(serde_json::json!({
                            "known_keys": snap.sim_values.keys().collect::<Vec<_>>(),
                        })),
                        stream_topic: None,
                    }
                }
            }
            Err(e) => ToolResult {
                tool_name: "get_sim_value".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("Runtime snapshot unavailable: {}. Is the engine running?", e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Set Simulation Value
// ---------------------------------------------------------------------------

pub struct SetSimValueTool;

impl ToolHandler for SetSimValueTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "set_sim_value",
            description: "Write a simulation watchpoint value. Injects a value into the simulation that Rune scripts can read via get_sim_value(). Use to set initial conditions, override parameters, or inject test data.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Watchpoint key name" },
                    "value": { "type": "number", "description": "Numeric value to set" }
                },
                "required": ["key", "value"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.set_sim_value"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let value = input.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);

        // Queue the command by writing to `<universe>/.eustress/sim-commands.jsonl`
        // — one JSON-line entry per pending mutation. The engine drains
        // this file on its next sim tick and applies the write to
        // `SimValuesResource`, then truncates. Keeps the write path
        // identical in-process and out-of-process.
        let cmd = serde_json::json!({
            "op": "set_sim_value",
            "key": key,
            "value": value,
            "queued_at": chrono::Utc::now().to_rfc3339(),
        });
        let path = ctx.universe_root.join(".eustress").join("sim-commands.jsonl");
        let write_result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .create(true).append(true).open(&path)?;
            writeln!(f, "{}", cmd)?;
            Ok(())
        })();

        match write_result {
            Ok(()) => ToolResult {
                tool_name: "set_sim_value".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: format!(
                    "Queued set_sim_value({} = {}). Engine will apply on next sim tick.",
                    key, value,
                ),
                structured_data: Some(serde_json::json!({
                    "queue_path": path.to_string_lossy(),
                    "key": key,
                    "value": value,
                })),
                stream_topic: Some("workshop.tool.set_sim_value".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "set_sim_value".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("Failed to queue set_sim_value: {}", e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// List All Simulation Values
// ---------------------------------------------------------------------------

pub struct ListSimValuesTool;

impl ToolHandler for ListSimValuesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_sim_values",
            description: "List all active simulation watchpoints with their current, min, max, and average values. Returns every watchpoint registered in the WatchPointRegistry.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        match read_sim_snapshot(ctx) {
            Ok(snap) => {
                let body = if snap.sim_values.is_empty() {
                    format!("No watchpoints in current snapshot (play_state={}).", snap.play_state)
                } else {
                    let lines: Vec<String> = snap.sim_values.iter()
                        .map(|(k, v)| format!("  - {} = {}", k, v))
                        .collect();
                    format!(
                        "{} watchpoint(s) (play_state={}, snapshot {}ms old):\n{}",
                        snap.sim_values.len(), snap.play_state, snap.age_ms, lines.join("\n"),
                    )
                };
                ToolResult {
                    tool_name: "list_sim_values".to_string(),
                    tool_use_id: String::new(),
                    success: true,
                    content: body,
                    structured_data: Some(serde_json::json!({
                        "sim_values": snap.sim_values,
                        "play_state": snap.play_state,
                        "snapshot_age_ms": snap.age_ms,
                    })),
                    stream_topic: None,
                }
            }
            Err(e) => ToolResult {
                tool_name: "list_sim_values".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("Runtime snapshot unavailable: {}. Is the engine running?", e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Get Tagged Entities
// ---------------------------------------------------------------------------

pub struct GetTaggedEntitiesTool;

impl ToolHandler for GetTaggedEntitiesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_tagged_entities",
            description: "Find all entities with a specific CollectionService tag. Tags are assigned by Rune scripts via collection_add_tag() or Luau scripts via CollectionService:AddTag(). Returns entity IDs and names.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tag": { "type": "string", "description": "Tag name to search for" }
                },
                "required": ["tag"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let tag = input.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        // Filesystem-backed: walk Workspace TOMLs, look for a `[tags]`
        // section containing the requested tag. CollectionService tags
        // are materialised to TOML on save by the engine, so this
        // picks them up even when the engine isn't currently running.
        let workspace = ctx.space_root.join("Workspace");
        let mut hits: Vec<serde_json::Value> = Vec::new();
        walk_tagged(&workspace, tag, &mut hits);

        let body = if hits.is_empty() {
            format!("No entities tagged '{}' in Workspace.", tag)
        } else {
            let lines: Vec<String> = hits.iter()
                .map(|h| format!(
                    "  - {} ({})",
                    h.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                    h.get("file").and_then(|v| v.as_str()).unwrap_or("?"),
                ))
                .collect();
            format!("{} entit{} tagged '{}':\n{}",
                hits.len(),
                if hits.len() == 1 { "y" } else { "ies" },
                tag,
                lines.join("\n"),
            )
        };

        ToolResult {
            tool_name: "get_tagged_entities".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: body,
            structured_data: Some(serde_json::json!({ "entities": hits, "tag": tag })),
            stream_topic: None,
        }
    }
}

fn walk_tagged(dir: &std::path::Path, tag: &str, out: &mut Vec<serde_json::Value>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Recurse into subdirectories (Part folders with _instance.toml).
        if path.is_dir() {
            walk_tagged(&path, tag, out);
            // Check for _instance.toml at this level.
            let inst = path.join("_instance.toml");
            if inst.exists() {
                if toml_has_tag(&inst, tag) {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                    out.push(serde_json::json!({
                        "name": name,
                        "file": inst.to_string_lossy(),
                    }));
                }
            }
            continue;
        }
        // Flat Part TOMLs.
        if fname.ends_with(".part.toml") || fname.ends_with(".glb.toml") || fname.ends_with(".instance.toml") {
            if toml_has_tag(&path, tag) {
                out.push(serde_json::json!({
                    "name": fname.trim_end_matches(".toml"),
                    "file": path.to_string_lossy(),
                }));
            }
        }
    }
}

fn toml_has_tag(path: &std::path::Path, tag: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else { return false };
    let Ok(val) = toml::from_str::<toml::Value>(&content) else { return false };
    val.get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some(tag)))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Raycast
// ---------------------------------------------------------------------------

pub struct RaycastTool;

impl ToolHandler for RaycastTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "raycast",
            description: "Cast a ray into the 3D scene and return the first entity hit. Specify origin point and direction vector. Returns hit position, surface normal, hit entity name, and distance. Uses the same raycast engine as Rune's workspace_raycast().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "origin": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] ray origin in world coordinates" },
                    "direction": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] ray direction (will be normalized)" },
                    "max_distance": { "type": "number", "description": "Maximum ray distance in studs (default: 1000)", "default": 1000 }
                },
                "required": ["origin", "direction"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let origin = input.get("origin").and_then(|v| v.as_array()).map(|a| {
            [
                a.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0),
                a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
            ]
        }).unwrap_or([0.0, 0.0, 0.0]);

        let direction = input.get("direction").and_then(|v| v.as_array()).map(|a| {
            [
                a.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0),
                a.get(1).and_then(|v| v.as_f64()).unwrap_or(-1.0),
                a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
            ]
        }).unwrap_or([0.0, -1.0, 0.0]);

        let max_distance = input.get("max_distance").and_then(|v| v.as_f64()).unwrap_or(1000.0);

        // Raycasting needs live Avian3D physics state — not available
        // from filesystem. We currently surface this honestly rather
        // than returning a fake "hit nothing" result. Once Engine Bridge
        // (`.eustress/engine.port`) grows an `ecs.raycast` RPC method,
        // this handler will switch to calling it.
        ToolResult {
            tool_name: "raycast".to_string(),
            tool_use_id: String::new(),
            success: false,
            content: format!(
                "raycast requires live physics — not yet wired to Engine Bridge. \
                 Your call was: origin=[{:.2}, {:.2}, {:.2}], direction=[{:.2}, {:.2}, {:.2}], \
                 max_distance={:.1}. Use `workspace_raycast()` from a Rune script instead \
                 (runs inline during simulation), or ask the user to wire the bridge \
                 `ecs.raycast` method to route this out-of-process call.",
                origin[0], origin[1], origin[2],
                direction[0], direction[1], direction[2],
                max_distance,
            ),
            structured_data: Some(serde_json::json!({
                "origin": origin,
                "direction": direction,
                "max_distance": max_distance,
                "reason": "no_live_physics_bridge",
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP Request
// ---------------------------------------------------------------------------

pub struct HttpRequestTool;

impl ToolHandler for HttpRequestTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "http_request",
            description: "Make an HTTP request to an external URL. Supports GET and POST methods. Returns status code, headers, and response body. Same capability as Rune's http_request_async() and Luau's HttpService:RequestAsync().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Full URL to request" },
                    "method": { "type": "string", "description": "HTTP method: GET, POST, PUT, DELETE", "default": "GET" },
                    "body": { "type": "string", "description": "Request body (for POST/PUT)" },
                    "headers": { "type": "object", "description": "Additional HTTP headers as key-value pairs" }
                },
                "required": ["url"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &["workshop.tool.http_request"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let url = match input.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return ToolResult {
                tool_name: "http_request".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing required parameter: url".to_string(),
                structured_data: None, stream_topic: None,
            },
        };
        let method = input.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
        let body = input.get("body").and_then(|v| v.as_str());

        let result = match method.to_uppercase().as_str() {
            "GET" => ureq::get(url).call(),
            "POST" => {
                let req = ureq::post(url);
                if let Some(b) = body {
                    req.set("Content-Type", "application/json").send_string(b)
                } else {
                    req.call()
                }
            }
            "PUT" => {
                let req = ureq::put(url);
                if let Some(b) = body {
                    req.set("Content-Type", "application/json").send_string(b)
                } else {
                    req.call()
                }
            }
            "DELETE" => ureq::delete(url).call(),
            _ => return ToolResult {
                tool_name: "http_request".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Unsupported method: {}. Use GET, POST, PUT, or DELETE.", method),
                structured_data: None, stream_topic: None,
            },
        };

        match result {
            Ok(resp) => {
                let status = resp.status();
                let body_text = resp.into_string().unwrap_or_default();
                let truncated = if body_text.len() > 8000 {
                    format!("{}...\n[truncated — {} bytes total]", &body_text[..8000], body_text.len())
                } else {
                    body_text
                };
                ToolResult {
                    tool_name: "http_request".to_string(),
                    tool_use_id: String::new(),
                    success: status < 400,
                    content: format!("HTTP {} {} — {} {}\n{}", method, url, status,
                        if status < 400 { "OK" } else { "Error" }, truncated),
                    structured_data: Some(serde_json::json!({ "status": status, "url": url, "method": method })),
                    stream_topic: Some("workshop.tool.http_request".to_string()),
                }
            }
            Err(e) => ToolResult {
                tool_name: "http_request".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("HTTP {} {} failed: {}", method, url, e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// DataStore Get
// ---------------------------------------------------------------------------

pub struct DataStoreGetTool;

impl ToolHandler for DataStoreGetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "datastore_get",
            description: "Read a value from a DataStore by key. DataStores are named key-value stores persisted to disk. Same API as Rune's datastore_get() and Luau's DataStore:GetAsync().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "store": { "type": "string", "description": "DataStore name" },
                    "key": { "type": "string", "description": "Key to look up" }
                },
                "required": ["store", "key"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let store = input.get("store").and_then(|v| v.as_str()).unwrap_or("");
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let path = datastore_path(ctx, store);
        let content_opt = std::fs::read_to_string(&path).ok()
            .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
            .and_then(|v| v.get(key).cloned());
        match content_opt {
            Some(val) => ToolResult {
                tool_name: "datastore_get".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("{}:{} = {}", store, key, val),
                structured_data: Some(serde_json::json!({ "store": store, "key": key, "value": val })),
                stream_topic: None,
            },
            None => ToolResult {
                tool_name: "datastore_get".to_string(), tool_use_id: String::new(),
                success: false,
                content: format!("No value for {}:{} (store file: {})", store, key, path.display()),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// DataStore Set
// ---------------------------------------------------------------------------

pub struct DataStoreSetTool;

impl ToolHandler for DataStoreSetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "datastore_set",
            description: "Write a value to a DataStore by key. DataStores are named key-value stores persisted to disk. Same API as Rune's datastore_set() and Luau's DataStore:SetAsync().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "store": { "type": "string", "description": "DataStore name" },
                    "key": { "type": "string", "description": "Key to write" },
                    "value": { "type": "string", "description": "Value to store (string-serialized)" }
                },
                "required": ["store", "key", "value"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.datastore_set"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let store = input.get("store").and_then(|v| v.as_str()).unwrap_or("");
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let value = input.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let path = datastore_path(ctx, store);
        // Read-modify-write the JSON store. Concurrent writers could
        // race; acceptable for the single-user Workshop/MCP case. For
        // multi-writer we'd switch to sled here.
        let mut blob: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = blob.as_object_mut() {
            obj.insert(key.to_string(), serde_json::Value::String(value.to_string()));
        } else {
            blob = serde_json::json!({ key: value });
        }
        let write_result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
            std::fs::write(&path, serde_json::to_string_pretty(&blob).unwrap_or_default())
        })();
        match write_result {
            Ok(()) => ToolResult {
                tool_name: "datastore_set".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Set {}:{} = {:?}", store, key, value),
                structured_data: Some(serde_json::json!({ "store": store, "key": key, "value": value, "file": path.to_string_lossy() })),
                stream_topic: Some("workshop.tool.datastore_set".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "datastore_set".to_string(), tool_use_id: String::new(),
                success: false,
                content: format!("Failed to persist {}:{}: {}", store, key, e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

/// Resolve the on-disk path for a named DataStore — shared by
/// `datastore_get` / `datastore_set` so both sides of the API write to
/// the same location regardless of which surface invokes the tool.
fn datastore_path(ctx: &ToolContext, store: &str) -> std::path::PathBuf {
    // Sanitise the store name so it can't traverse the filesystem. Only
    // ASCII alnum + `_` / `-`; everything else collapses to `_`.
    let safe: String = store.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    ctx.universe_root
        .join(".eustress")
        .join("datastore")
        .join(format!("{}.json", safe))
}

// ---------------------------------------------------------------------------
// Add Tag
// ---------------------------------------------------------------------------

pub struct AddTagTool;

impl ToolHandler for AddTagTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "add_tag",
            description: "Add a CollectionService tag to an entity. Tags are used to group entities for batch queries. Same API as Rune's collection_add_tag() and Luau's CollectionService:AddTag().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": { "type": "integer", "description": "Entity ID to tag" },
                    "tag": { "type": "string", "description": "Tag name to add" }
                },
                "required": ["entity_id", "tag"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.add_tag"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        // Filesystem-backed: the `entity_id` is expected to be the
        // entity folder/file name under Workspace (the LLM typically
        // already knows the name from `query_entities`). We ignore
        // numeric IDs — those don't survive a process restart. The
        // engine's CollectionService reloads from the TOML's [tags]
        // array on Space load.
        let entity_name = input.get("entity_id").and_then(|v| v.as_str())
            .or_else(|| input.get("entity").and_then(|v| v.as_str()))
            .unwrap_or("");
        let tag = input.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        match toggle_tag(&ctx.space_root, entity_name, tag, true) {
            Ok(path) => ToolResult {
                tool_name: "add_tag".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Tagged '{}' with '{}' (file: {})", entity_name, tag, path.display()),
                structured_data: Some(serde_json::json!({
                    "entity": entity_name, "tag": tag, "file": path.to_string_lossy(),
                })),
                stream_topic: Some("workshop.tool.add_tag".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "add_tag".to_string(), tool_use_id: String::new(),
                success: false,
                content: format!("add_tag failed: {}", e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Remove Tag
// ---------------------------------------------------------------------------

pub struct RemoveTagTool;

impl ToolHandler for RemoveTagTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "remove_tag",
            description: "Remove a CollectionService tag from an entity. Same API as Rune's collection_remove_tag() and Luau's CollectionService:RemoveTag().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": { "type": "integer", "description": "Entity ID to untag" },
                    "tag": { "type": "string", "description": "Tag name to remove" }
                },
                "required": ["entity_id", "tag"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.remove_tag"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let entity_name = input.get("entity_id").and_then(|v| v.as_str())
            .or_else(|| input.get("entity").and_then(|v| v.as_str()))
            .unwrap_or("");
        let tag = input.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        match toggle_tag(&ctx.space_root, entity_name, tag, false) {
            Ok(path) => ToolResult {
                tool_name: "remove_tag".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Removed tag '{}' from '{}' (file: {})", tag, entity_name, path.display()),
                structured_data: Some(serde_json::json!({
                    "entity": entity_name, "tag": tag, "file": path.to_string_lossy(),
                })),
                stream_topic: Some("workshop.tool.remove_tag".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "remove_tag".to_string(), tool_use_id: String::new(),
                success: false,
                content: format!("remove_tag failed: {}", e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

/// Shared tag-mutation helper. Finds the entity's TOML in Workspace,
/// parses it, adds or removes the tag from `[tags]`, and writes it
/// back. Works in-process (Workshop) and out-of-process (MCP).
fn toggle_tag(
    space_root: &std::path::Path,
    entity_name: &str,
    tag: &str,
    add: bool,
) -> Result<std::path::PathBuf, String> {
    if entity_name.is_empty() || tag.is_empty() {
        return Err("entity_id and tag must both be provided (as strings)".to_string());
    }
    let workspace = space_root.join("Workspace");

    // Folder-style entity: Workspace/<name>/_instance.toml
    let folder_toml = workspace.join(entity_name).join("_instance.toml");
    // Flat-file styles: .part.toml / .glb.toml / .instance.toml
    let candidates = [
        folder_toml.clone(),
        workspace.join(format!("{}.part.toml", entity_name)),
        workspace.join(format!("{}.glb.toml", entity_name)),
        workspace.join(format!("{}.instance.toml", entity_name)),
    ];
    let path = candidates.into_iter().find(|p| p.exists())
        .ok_or_else(|| format!("entity '{}' not found under Workspace", entity_name))?;

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let mut doc: toml::Value = toml::from_str(&content)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;

    // Normalise to an object root and an existing `tags` array.
    let tbl = doc.as_table_mut()
        .ok_or_else(|| "TOML root is not a table".to_string())?;
    let tags_entry = tbl.entry("tags".to_string())
        .or_insert_with(|| toml::Value::Array(Vec::new()));
    let tags = tags_entry.as_array_mut()
        .ok_or_else(|| "`tags` exists but is not an array".to_string())?;

    let already = tags.iter().any(|v| v.as_str() == Some(tag));
    if add && !already {
        tags.push(toml::Value::String(tag.to_string()));
    } else if !add {
        tags.retain(|v| v.as_str() != Some(tag));
    }

    std::fs::write(&path, toml::to_string_pretty(&doc).unwrap_or(content))
        .map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Tail Telemetry — stream recent telemetry from Eustress Streams
// ---------------------------------------------------------------------------

pub struct TailTelemetryTool;

impl ToolHandler for TailTelemetryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "tail_telemetry",
            description: "Tail recent telemetry events from Eustress Streams. Returns the last N simulation watchpoint samples with timestamps. Use for monitoring simulation health, detecting anomalies, and feeding the Repairman feedback loop. Reads from the runtime snapshot history log.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "count": { "type": "integer", "description": "Number of recent samples to return (default: 20, max: 100)", "default": 20 },
                    "keys": { "type": "array", "items": { "type": "string" }, "description": "Filter to specific watchpoint keys (e.g. ['battery.voltage', 'battery.soc']). Empty = all keys." },
                    "since_ms": { "type": "integer", "description": "Only return samples newer than this many milliseconds ago" }
                }
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(20).min(100) as usize;
        let key_filter: Vec<String> = input.get("keys")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let since_ms = input.get("since_ms").and_then(|v| v.as_u64());

        // Read the telemetry log — the engine appends one JSON line per
        // snapshot tick to `<universe>/.eustress/telemetry.jsonl`. Each
        // line: { "t": "<rfc3339>", "values": { "key": f64, ... } }
        let path = ctx.universe_root.join(".eustress").join("telemetry.jsonl");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => {
                // Fall back to reading current snapshot as a single sample
                match read_sim_snapshot(ctx) {
                    Ok(snap) => {
                        let filtered: std::collections::BTreeMap<String, f64> = if key_filter.is_empty() {
                            snap.sim_values
                        } else {
                            snap.sim_values.into_iter()
                                .filter(|(k, _)| key_filter.iter().any(|f| k.contains(f.as_str())))
                                .collect()
                        };
                        let lines: Vec<String> = filtered.iter()
                            .map(|(k, v)| format!("  {} = {:.4}", k, v))
                            .collect();
                        return ToolResult {
                            tool_name: "tail_telemetry".to_string(),
                            tool_use_id: String::new(),
                            success: true,
                            content: format!(
                                "Live snapshot (play_state={}, {}ms old):\n{}",
                                snap.play_state, snap.age_ms, lines.join("\n"),
                            ),
                            structured_data: Some(serde_json::json!({
                                "source": "snapshot",
                                "play_state": snap.play_state,
                                "values": filtered,
                                "sample_count": 1,
                            })),
                            stream_topic: None,
                        };
                    }
                    Err(e) => return ToolResult {
                        tool_name: "tail_telemetry".to_string(), tool_use_id: String::new(),
                        success: false,
                        content: format!("No telemetry log and no live snapshot: {}. Is the engine running a simulation?", e),
                        structured_data: None, stream_topic: None,
                    },
                }
            }
        };

        // Parse the last N lines from the JSONL log
        let now = chrono::Utc::now();
        let mut samples: Vec<serde_json::Value> = content.lines().rev()
            .take(count * 2) // over-read then filter
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|entry| {
                // Apply since_ms filter
                if let Some(max_age) = since_ms {
                    if let Some(ts) = entry.get("t").and_then(|v| v.as_str()) {
                        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
                            let age = now.signed_duration_since(parsed.with_timezone(&chrono::Utc));
                            return age.num_milliseconds() <= max_age as i64;
                        }
                    }
                }
                true
            })
            .filter(|entry| {
                // Apply key filter
                if key_filter.is_empty() { return true; }
                entry.get("values").and_then(|v| v.as_object()).map(|obj| {
                    key_filter.iter().any(|k| obj.contains_key(k))
                }).unwrap_or(false)
            })
            .take(count)
            .collect();
        samples.reverse(); // Chronological order

        let body = if samples.is_empty() {
            "No telemetry samples matching filters.".to_string()
        } else {
            let lines: Vec<String> = samples.iter().map(|s| {
                let ts = s.get("t").and_then(|v| v.as_str()).unwrap_or("?");
                let vals = s.get("values").and_then(|v| v.as_object())
                    .map(|obj| {
                        let mut filtered = obj.clone();
                        if !key_filter.is_empty() {
                            filtered.retain(|k, _| key_filter.iter().any(|f| k.contains(f.as_str())));
                        }
                        filtered.iter()
                            .map(|(k, v)| format!("{}={:.4}", k, v.as_f64().unwrap_or(0.0)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!("  [{}] {}", ts, vals)
            }).collect();
            format!("{} telemetry sample(s):\n{}", samples.len(), lines.join("\n"))
        };

        ToolResult {
            tool_name: "tail_telemetry".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: body,
            structured_data: Some(serde_json::json!({
                "source": "telemetry_log",
                "sample_count": samples.len(),
                "samples": samples,
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Query Audit Log — search Claude API call audit trail
// ---------------------------------------------------------------------------

pub struct QueryAuditLogTool;

impl ToolHandler for QueryAuditLogTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_audit_log",
            description: "Query the Claude API call audit trail for the current Space. Returns recent AI decisions, tool calls, token usage, and durations. Every Claude call is logged as a .log.toml file in SoulService/Logs/. Use to inspect the full chain of AI decisions, debug agent behavior, or audit costs.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "count": { "type": "integer", "description": "Number of recent audit entries to return (default: 10, max: 50)", "default": 10 },
                    "caller_filter": { "type": "string", "description": "Filter by caller subsystem (e.g. 'workshop', 'soul-build', 'summarize')" },
                    "include_prompt": { "type": "boolean", "description": "Include full prompt text in results (default: false — prompts can be very large)", "default": false },
                    "include_response": { "type": "boolean", "description": "Include full response text in results (default: false)", "default": false }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(10).min(50) as usize;
        let caller_filter = input.get("caller_filter").and_then(|v| v.as_str());
        let include_prompt = input.get("include_prompt").and_then(|v| v.as_bool()).unwrap_or(false);
        let include_response = input.get("include_response").and_then(|v| v.as_bool()).unwrap_or(false);

        let logs_dir = ctx.space_root.join("SoulService").join("Logs");
        if !logs_dir.exists() {
            return ToolResult {
                tool_name: "query_audit_log".to_string(), tool_use_id: String::new(),
                success: true,
                content: "No audit logs found — SoulService/Logs/ does not exist yet.".to_string(),
                structured_data: Some(serde_json::json!({ "entries": [], "count": 0 })),
                stream_topic: None,
            };
        }

        // Collect .log.toml files, sorted by name (which is timestamp-prefixed)
        let mut log_files: Vec<std::path::PathBuf> = std::fs::read_dir(&logs_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("toml"))
            .filter(|e| e.file_name().to_string_lossy().ends_with(".log.toml"))
            .map(|e| e.path())
            .collect();
        log_files.sort();
        log_files.reverse(); // Most recent first

        let mut entries: Vec<serde_json::Value> = Vec::new();
        let mut total_tokens_in: u64 = 0;
        let mut total_tokens_out: u64 = 0;
        let mut total_duration_ms: u64 = 0;

        for path in log_files {
            if entries.len() >= count { break; }

            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let Ok(doc) = toml::from_str::<toml::Value>(&content) else { continue };

            let call = doc.get("call");
            let caller = call.and_then(|c| c.get("caller")).and_then(|v| v.as_str()).unwrap_or("");

            // Apply caller filter
            if let Some(filter) = caller_filter {
                if !caller.contains(filter) { continue; }
            }

            let timestamp = call.and_then(|c| c.get("timestamp")).and_then(|v| v.as_str()).unwrap_or("?");
            let model = call.and_then(|c| c.get("model")).and_then(|v| v.as_str()).unwrap_or("?");
            let tokens_in = call.and_then(|c| c.get("tokens_input")).and_then(|v| v.as_integer()).unwrap_or(0) as u64;
            let tokens_out = call.and_then(|c| c.get("tokens_output")).and_then(|v| v.as_integer()).unwrap_or(0) as u64;
            let duration = call.and_then(|c| c.get("duration_ms")).and_then(|v| v.as_integer()).unwrap_or(0) as u64;

            total_tokens_in += tokens_in;
            total_tokens_out += tokens_out;
            total_duration_ms += duration;

            let mut entry = serde_json::json!({
                "timestamp": timestamp,
                "model": model,
                "caller": caller,
                "tokens_input": tokens_in,
                "tokens_output": tokens_out,
                "duration_ms": duration,
                "file": path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            });

            if include_prompt {
                let prompt = doc.get("prompt").and_then(|p| p.get("text")).and_then(|v| v.as_str()).unwrap_or("");
                let truncated = if prompt.len() > 2000 {
                    format!("{}...[truncated, {} chars total]", &prompt[..2000], prompt.len())
                } else {
                    prompt.to_string()
                };
                entry.as_object_mut().unwrap().insert("prompt".to_string(), serde_json::json!(truncated));
            }
            if include_response {
                let response = doc.get("response").and_then(|p| p.get("text")).and_then(|v| v.as_str()).unwrap_or("");
                let truncated = if response.len() > 2000 {
                    format!("{}...[truncated, {} chars total]", &response[..2000], response.len())
                } else {
                    response.to_string()
                };
                entry.as_object_mut().unwrap().insert("response".to_string(), serde_json::json!(truncated));
            }

            entries.push(entry);
        }

        let lines: Vec<String> = entries.iter().map(|e| {
            format!("  [{}] {} — {} (in:{} out:{} {}ms)",
                e.get("timestamp").and_then(|v| v.as_str()).unwrap_or("?"),
                e.get("caller").and_then(|v| v.as_str()).unwrap_or("?"),
                e.get("model").and_then(|v| v.as_str()).unwrap_or("?"),
                e.get("tokens_input").and_then(|v| v.as_u64()).unwrap_or(0),
                e.get("tokens_output").and_then(|v| v.as_u64()).unwrap_or(0),
                e.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
            )
        }).collect();

        let body = if entries.is_empty() {
            format!("No audit log entries{}", caller_filter.map(|f| format!(" matching caller '{}'", f)).unwrap_or_default())
        } else {
            format!(
                "{} audit log entr{}:\n{}\n\nTotals: {} tokens in, {} tokens out, {:.1}s total duration",
                entries.len(),
                if entries.len() == 1 { "y" } else { "ies" },
                lines.join("\n"),
                total_tokens_in,
                total_tokens_out,
                total_duration_ms as f64 / 1000.0,
            )
        };

        ToolResult {
            tool_name: "query_audit_log".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: body,
            structured_data: Some(serde_json::json!({
                "entries": entries,
                "count": entries.len(),
                "total_tokens_input": total_tokens_in,
                "total_tokens_output": total_tokens_out,
                "total_duration_ms": total_duration_ms,
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Simulation Control — run, stop, get state
// ---------------------------------------------------------------------------

pub struct RunSimulationTool;

impl ToolHandler for RunSimulationTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "run_simulation",
            description: "Start or resume the simulation (enter Play mode). Equivalent to pressing the Play button in the IDE. The simulation runs the electrochemistry tick, Rune scripts, physics, and all registered systems. Use with set_sim_value to configure initial conditions before running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "time_scale": { "type": "number", "description": "Time scale multiplier (1.0 = realtime, 10.0 = 10x speed, 0.1 = slow-mo). Default: 1.0", "default": 1.0 },
                    "duration_s": { "type": "number", "description": "Auto-stop after this many simulation-seconds. Omit for indefinite run." }
                }
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: true,
            stream_topics: &["workshop.simulation.started"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let time_scale = input.get("time_scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let duration_s = input.get("duration_s").and_then(|v| v.as_f64());

        // Queue the command via sim-commands.jsonl — the engine reads this
        // on its next frame and transitions PlayModeState accordingly.
        let cmd = serde_json::json!({
            "op": "run_simulation",
            "time_scale": time_scale,
            "duration_s": duration_s,
            "queued_at": chrono::Utc::now().to_rfc3339(),
        });
        match queue_sim_command(ctx, &cmd) {
            Ok(()) => ToolResult {
                tool_name: "run_simulation".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!(
                    "Simulation start queued (time_scale={:.1}x{}).",
                    time_scale,
                    duration_s.map(|d| format!(", auto-stop after {:.1}s", d)).unwrap_or_default(),
                ),
                structured_data: Some(serde_json::json!({
                    "action": "run", "time_scale": time_scale, "duration_s": duration_s,
                })),
                stream_topic: Some("workshop.simulation.started".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "run_simulation".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to queue: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

pub struct StopSimulationTool;

impl ToolHandler for StopSimulationTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "stop_simulation",
            description: "Stop the running simulation (return to Edit mode). Equivalent to pressing the Stop button in the IDE. Simulation state is preserved in watchpoints for post-analysis.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.simulation.stopped"],
        }
    }

    fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let cmd = serde_json::json!({
            "op": "stop_simulation",
            "queued_at": chrono::Utc::now().to_rfc3339(),
        });
        match queue_sim_command(ctx, &cmd) {
            Ok(()) => ToolResult {
                tool_name: "stop_simulation".to_string(), tool_use_id: String::new(),
                success: true,
                content: "Simulation stop queued.".to_string(),
                structured_data: Some(serde_json::json!({ "action": "stop" })),
                stream_topic: Some("workshop.simulation.stopped".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "stop_simulation".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to queue: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

pub struct GetSimulationStateTool;

impl ToolHandler for GetSimulationStateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_simulation_state",
            description: "Get the current simulation state: play mode (editing/playing/paused), all watchpoint values, simulation time, and snapshot freshness. The single tool for understanding what the simulation is doing right now.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        match read_sim_snapshot(ctx) {
            Ok(snap) => {
                let lines: Vec<String> = snap.sim_values.iter()
                    .map(|(k, v)| format!("  {} = {:.6}", k, v))
                    .collect();
                let body = format!(
                    "Play state: {}\nSnapshot age: {}ms\n{} watchpoint(s):\n{}",
                    snap.play_state, snap.age_ms, snap.sim_values.len(), lines.join("\n"),
                );
                ToolResult {
                    tool_name: "get_simulation_state".to_string(), tool_use_id: String::new(),
                    success: true,
                    content: body,
                    structured_data: Some(serde_json::json!({
                        "play_state": snap.play_state,
                        "snapshot_age_ms": snap.age_ms,
                        "watchpoint_count": snap.sim_values.len(),
                        "sim_values": snap.sim_values,
                    })),
                    stream_topic: None,
                }
            }
            Err(e) => ToolResult {
                tool_name: "get_simulation_state".to_string(), tool_use_id: String::new(),
                success: false,
                content: format!("Runtime snapshot unavailable: {}. Is the engine running?", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

/// Shared helper — append a JSON command to `<universe>/.eustress/sim-commands.jsonl`.
/// The engine drains this file on its next frame tick.
fn queue_sim_command(ctx: &ToolContext, cmd: &serde_json::Value) -> Result<(), String> {
    let path = ctx.universe_root.join(".eustress").join("sim-commands.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true).append(true).open(&path)
        .map_err(|e| format!("open: {}", e))?;
    writeln!(f, "{}", cmd).map_err(|e| format!("write: {}", e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Runtime-snapshot reader — feeds the sim-value tools
// ---------------------------------------------------------------------------
//
// The engine writes `<universe>/.eustress/runtime-snapshot.json` at 4 Hz
// (see `engine/src/script_editor/runtime_snapshot.rs`). Every sibling
// process — LSP, MCP, Workshop-in-engine — reads the same file, so live
// sim values are available identically in-process and out-of-process.
//
// We parse the JSON loosely with `serde_json::Value` here to avoid a
// structural dependency on the engine's `RuntimeSnapshot` type; the on-
// disk schema is: `{ generated_at, play_state, sim_values: {k: f64} }`.

struct SnapshotReading {
    sim_values: std::collections::BTreeMap<String, f64>,
    play_state: String,
    age_ms: u128,
}

fn read_sim_snapshot(ctx: &ToolContext) -> Result<SnapshotReading, String> {
    let path = ctx
        .universe_root
        .join(".eustress")
        .join("runtime-snapshot.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let val: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("parse snapshot: {}", e))?;

    let sim_values = val.get("sim_values")
        .and_then(|v| v.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default();

    let play_state = val.get("play_state")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    // `generated_at` is RFC-3339. Compute age against `now`; report 0 if
    // the timestamp is missing or unparseable — better than failing.
    let age_ms = val.get("generated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            let now = chrono::Utc::now();
            let diff = now.signed_duration_since(t.with_timezone(&chrono::Utc));
            diff.num_milliseconds().max(0) as u128
        })
        .unwrap_or(0);

    Ok(SnapshotReading { sim_values, play_state, age_ms })
}
