//! Live-engine tools — drive the RUNNING engine over the TCP bridge.
//!
//! Unlike the filesystem-backed tools in `eustress-tools` (which write
//! `_instance.toml` for the file watcher to pick up), these four reach
//! the engine *while it runs*: they connect to `.eustress/engine.port`
//! and call the bridge methods `ecs.inspect`, `tool.equip`,
//! `selection.set`, and `state.get`.
//!
//! Each is a `ToolHandler` (same trait the shared registry uses) so it
//! plugs into `tools/list` + `tools/call` exactly like `QueryEntitiesTool`.
//! They live here in the MCP-server crate rather than in `eustress-tools`
//! because the bridge client is MCP-server-local (the engine talks to its
//! own ECS directly, in-process, and has no need for a TCP client of
//! itself).
//!
//! Pattern mirrors `eustress_tools::entity_tools::QueryEntitiesTool`:
//! `definition()` (modes General, `requires_approval: false`) +
//! `execute()` returning a `ToolResult` whose `content` is a compact
//! human summary and whose `structured_data` is the raw bridge result.

use eustress_tools::modes::WorkshopMode;
use eustress_tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use serde_json::Value;

use crate::bridge_client::call_engine;

/// Build a failed `ToolResult` with a bridge/engine error message. Shared
/// by all four tools so the "engine not running" surface is identical.
fn fail(tool_name: &str, message: String) -> ToolResult {
    ToolResult {
        tool_name: tool_name.to_string(),
        tool_use_id: String::new(),
        success: false,
        content: message,
        structured_data: None,
        stream_topic: None,
    }
}

/// Build a successful `ToolResult`: human `content` + raw bridge JSON in
/// `structured_data`.
fn ok(tool_name: &str, content: String, data: Value) -> ToolResult {
    ToolResult {
        tool_name: tool_name.to_string(),
        tool_use_id: String::new(),
        success: true,
        content,
        structured_data: Some(data),
        stream_topic: None,
    }
}

// ---------------------------------------------------------------------------
// inspect_scene  ->  ecs.inspect
// ---------------------------------------------------------------------------

pub struct InspectSceneTool;

impl ToolHandler for InspectSceneTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "inspect_scene",
            description: "Inspect the LIVE running engine's scene over the engine bridge. Returns per-entity class/mesh/material/color/transform/visibility/physics flags/parent/on-disk source plus current FPS — far richer than query_entities (which only reads files on disk). Use this to debug what the engine is ACTUALLY rendering right now (e.g. a wrong mesh asset). Requires the engine to be running. Read-only.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class":         { "type": "string",  "description": "Filter to an exact class name (e.g. \"Part\", \"Model\")." },
                    "name_contains": { "type": "string",  "description": "Case-insensitive substring filter on entity name." },
                    "offset":        { "type": "integer", "description": "Pagination offset (default 0)." },
                    "limit":         { "type": "integer", "description": "Max entities to return (default 200, engine caps at 5000)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        // Pass through only the params the bridge understands; omit any
        // the caller didn't supply so engine-side defaults apply.
        let mut params = serde_json::Map::new();
        for key in ["class", "name_contains", "offset", "limit"] {
            if let Some(v) = input.get(key) {
                if !v.is_null() {
                    params.insert(key.to_string(), v.clone());
                }
            }
        }

        match call_engine(&ctx.universe_root, "ecs.inspect", Value::Object(params)) {
            Ok(result) => {
                let total = result.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                let returned = result.get("returned").and_then(|v| v.as_u64()).unwrap_or(0);
                let fps = result.get("fps").and_then(|v| v.as_f64());
                let fps_note = match fps {
                    Some(f) => format!("fps={f:.1}"),
                    None => "fps=n/a".to_string(),
                };

                // Spell a few entities out in `content` — the AI reads
                // `content`, not `structured_data`, so a bare count would
                // leave it blind to names/classes/meshes.
                let mut lines: Vec<String> = Vec::new();
                if let Some(arr) = result.get("entities").and_then(|v| v.as_array()) {
                    for e in arr.iter().take(20) {
                        let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let class = e.get("class").and_then(|v| v.as_str()).unwrap_or("?");
                        let mesh = e.get("mesh").and_then(|v| v.as_str()).unwrap_or("-");
                        let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        lines.push(format!("  - {name} ({class}) mesh={mesh} id={id}"));
                    }
                }
                let more = if returned > 20 {
                    format!("\n  … and {} more (of {returned} returned)", returned - 20)
                } else {
                    String::new()
                };

                let summary = format!(
                    "{total} entities; {fps_note}\n{}{more}",
                    lines.join("\n")
                );
                ok("inspect_scene", summary, result)
            }
            Err(e) => fail("inspect_scene", e),
        }
    }
}

// ---------------------------------------------------------------------------
// equip_tool  ->  tool.equip
// ---------------------------------------------------------------------------

pub struct EquipToolTool;

impl ToolHandler for EquipToolTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "equip_tool",
            description: "Set the LIVE engine's active editor tool (select/move/scale/rotate) over the engine bridge — the AI equivalent of pressing the Alt+Z/X/C/V tool shortcuts. Affects gizmos in the running viewport. Requires the engine to be running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tool": {
                        "type": "string",
                        "enum": ["select", "move", "scale", "rotate"],
                        "description": "Which editor tool to equip."
                    }
                },
                "required": ["tool"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let tool = input.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        if tool.is_empty() {
            return fail(
                "equip_tool",
                "Missing required `tool` (expected select|move|scale|rotate).".to_string(),
            );
        }
        let params = serde_json::json!({ "tool": tool });
        match call_engine(&ctx.universe_root, "tool.equip", params) {
            Ok(result) => {
                let equipped = result
                    .get("equipped")
                    .and_then(|v| v.as_str())
                    .unwrap_or(tool);
                ok("equip_tool", format!("Equipped '{equipped}' tool."), result)
            }
            Err(e) => fail("equip_tool", e),
        }
    }
}

// ---------------------------------------------------------------------------
// select_entity  ->  selection.set
// ---------------------------------------------------------------------------

pub struct SelectEntityTool;

impl ToolHandler for SelectEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "select_entity",
            description: "Replace the LIVE engine's current selection with one or more entities over the engine bridge — drives gizmos + the Properties panel exactly like a click-select. Pass entity ids as returned by inspect_scene (\"<index>v<generation>\", e.g. \"123v0\"). An empty `ids` clears the selection. Requires the engine to be running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Entity ids to select (\"<index>v<generation>\"). Empty array clears the selection."
                    },
                    "id": {
                        "type": "string",
                        "description": "Convenience: a single entity id to select (use this OR `ids`)."
                    }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        // Forward whichever of {ids, id} the caller gave; the bridge
        // accepts either form. If neither is present, send an empty
        // `ids` (explicit clear) so behaviour is well-defined.
        let params = if input.get("ids").is_some() {
            serde_json::json!({ "ids": input.get("ids").cloned().unwrap_or(Value::Null) })
        } else if let Some(id) = input.get("id") {
            serde_json::json!({ "id": id.clone() })
        } else {
            serde_json::json!({ "ids": [] })
        };

        match call_engine(&ctx.universe_root, "selection.set", params) {
            Ok(result) => {
                let count = result.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                let selected: Vec<String> = result
                    .get("selected")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let summary = if count == 0 {
                    "Selection cleared.".to_string()
                } else {
                    format!("Selected {count}: {}", selected.join(", "))
                };
                ok("select_entity", summary, result)
            }
            Err(e) => fail("select_entity", e),
        }
    }
}

// ---------------------------------------------------------------------------
// get_editor_state  ->  state.get
// ---------------------------------------------------------------------------

pub struct GetEditorStateTool;

impl ToolHandler for GetEditorStateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_editor_state",
            description: "Read the LIVE engine's editor state (active tool + current selection) over the engine bridge, so the AI can verify the result of its own equip_tool / select_entity actions. Requires the engine to be running. Read-only.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "state.get", serde_json::json!({})) {
            Ok(result) => {
                let tool = result
                    .get("current_tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let count = result
                    .get("selected_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let selected: Vec<String> = result
                    .get("selected")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let sel_note = if count == 0 {
                    "nothing selected".to_string()
                } else {
                    format!("{count} selected: {}", selected.join(", "))
                };
                ok(
                    "get_editor_state",
                    format!("tool={tool}; {sel_note}"),
                    result,
                )
            }
            Err(e) => fail("get_editor_state", e),
        }
    }
}

// ---------------------------------------------------------------------------
// invoke_action  ->  action.invoke
// ---------------------------------------------------------------------------

pub struct InvokeActionTool;

impl ToolHandler for InvokeActionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "invoke_action",
            description: "Invoke any LIVE engine editor action by name over the bridge — the AI equivalent of pressing its keyboard shortcut. Runs the SAME handler a real key press does. Examples: Copy, Cut, Paste, Duplicate, Group, Ungroup, Delete, SelectAll, Undo, Redo, SaveScene, and tool switches SelectTool/MoveTool/ScaleTool/RotateTool. Combine with select_entity (to set the operand) + inspect_scene/get_editor_state (to verify the effect) for end-to-end editor testing. Requires the engine to be running. NOTE: some actions (Delete, Cut) are destructive.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "The Action enum variant name, e.g. \"Copy\", \"Group\", \"Undo\", \"SaveScene\", \"MoveTool\"."
                    }
                },
                "required": ["action"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        if action.is_empty() {
            return fail(
                "invoke_action",
                "Missing required `action` (e.g. Copy, Group, Undo, SaveScene, MoveTool).".to_string(),
            );
        }
        match call_engine(
            &ctx.universe_root,
            "action.invoke",
            serde_json::json!({ "action": action }),
        ) {
            Ok(result) => ok("invoke_action", format!("Invoked action '{action}'."), result),
            Err(e) => fail("invoke_action", e),
        }
    }
}

// ---------------------------------------------------------------------------
// capture_viewport  ->  viewport.capture
// ---------------------------------------------------------------------------

pub struct CaptureViewportTool;

impl ToolHandler for CaptureViewportTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "capture_viewport",
            description: "Screenshot the LIVE engine's viewport (3D scene + UI overlay = exactly what a human sees) to a PNG on disk, and return its path. This is the AI's EYES: after calling it, READ the returned file path to view the frame. Use it to see the result of your actions and to debug VISUAL issues (gizmo hidden behind a part, wrong placement, render glitches) that state queries can't catch. Requires the engine to be running; allow a brief moment after the call before reading (the PNG lands 1-2 frames later).",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "viewport.capture", serde_json::json!({})) {
            Ok(result) => {
                let path = result
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                ok(
                    "capture_viewport",
                    format!(
                        "Screenshot saved to {path} — read that file path to view it \
                         (allow ~1 frame for the PNG to land)."
                    ),
                    result,
                )
            }
            Err(e) => fail("capture_viewport", e),
        }
    }
}

// ---------------------------------------------------------------------------
// AI camera — the AI's OWN independent off-screen camera (not the user's view)
// ---------------------------------------------------------------------------

pub struct AiCameraSetPoseTool;

impl ToolHandler for AiCameraSetPoseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_set_pose",
            description: "Place the AI's OWN independent off-screen camera (separate from the user's viewport — it never displaces what the human sees). Params: position [x,y,z] (required), plus either look_at [x,y,z] or rotation [x,y,z,w]. Then call ai_camera_capture to see from this pose. This is how you move your own eyes around the world on your own timeline. Requires the engine running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "position": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] world position of the AI camera." },
                    "look_at":  { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] point to look at (use this OR rotation)." },
                    "rotation": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z,w] quaternion orientation (use this OR look_at)." }
                },
                "required": ["position"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "ai_camera.set_pose", input) {
            Ok(r) => ok("ai_camera_set_pose", "AI camera repositioned.".to_string(), r),
            Err(e) => fail("ai_camera_set_pose", e),
        }
    }
}

pub struct AiCameraOrbitTool;

impl ToolHandler for AiCameraOrbitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_orbit",
            description: "Orbit the AI's own off-screen camera around a point — convenient for circling what you're working on. Params: center [x,y,z] (default origin), distance (default 15), yaw_deg (default 45), pitch_deg (default 30). Then ai_camera_capture to view it. Requires the engine running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "center":    { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] point to orbit around (default [0,0,0])." },
                    "distance":  { "type": "number", "description": "Distance from center (default 15)." },
                    "yaw_deg":   { "type": "number", "description": "Horizontal angle in degrees (default 45)." },
                    "pitch_deg": { "type": "number", "description": "Vertical angle in degrees (default 30)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "ai_camera.orbit", input) {
            Ok(r) => ok("ai_camera_orbit", "AI camera orbited.".to_string(), r),
            Err(e) => fail("ai_camera_orbit", e),
        }
    }
}

pub struct AiCameraFrameTool;

impl ToolHandler for AiCameraFrameTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_frame",
            description: "Point the AI's own off-screen camera at a named entity and back off to a sensible distance for its size — 'go look at that part'. Param: name (the entity's Name, e.g. \"House_Roof\"). Then ai_camera_capture to view it. Requires the engine running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the entity to frame (e.g. \"House_Floor\")." }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        match call_engine(&ctx.universe_root, "ai_camera.frame", input) {
            Ok(r) => ok("ai_camera_frame", format!("AI camera framed '{name}'."), r),
            Err(e) => fail("ai_camera_frame", e),
        }
    }
}

pub struct AiCameraCaptureTool;

impl ToolHandler for AiCameraCaptureTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_capture",
            description: "Render the AI's OWN independent camera to a PNG and return its path — your own eyes, separate from capture_viewport (which is the human's window). After calling, READ the returned file path to view your frame. Use ai_camera_set_pose/orbit/frame first to aim it. On-demand: the off-screen camera powers up only for this shot, so it doesn't tax the user's framerate. Allow ~3 frames before reading. Requires the engine running.",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "ai_camera.capture", serde_json::json!({})) {
            Ok(r) => {
                let path = r.get("path").and_then(|v| v.as_str()).unwrap_or("(unknown)");
                ok(
                    "ai_camera_capture",
                    format!(
                        "AI camera frame saved to {path} — read that file path to view it \
                         (allow ~3 frames for the PNG to land)."
                    ),
                    r,
                )
            }
            Err(e) => fail("ai_camera_capture", e),
        }
    }
}
