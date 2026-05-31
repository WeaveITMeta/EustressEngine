//! Bridge to the shared `eustress-tools` crate.
//!
//! The MCP server's historical tool surface (see `tools.rs`) was 13
//! hand-rolled handlers with a function-pointer shape. The shared
//! `eustress-tools` crate now hosts ~40+ handlers under a trait-based
//! registry — the same registry the engine uses for Workshop — so
//! every tool in one surface is available in the other.
//!
//! This module exposes the shared registry's tools to MCP's
//! `tools/list` + `tools/call` by building a `ToolContext` from the
//! server's current Universe state on each invocation.

use eustress_tools::{ToolContext, ToolRegistry, ToolResult};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Process-wide registry built once at first use. Tool handlers are
/// stateless between calls, so sharing one registry is safe.
static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();

fn registry() -> &'static ToolRegistry {
    REGISTRY.get_or_init(|| {
        let mut r = eustress_tools::default_registry();
        // Live-engine tools (MCP-server-local): these drive the RUNNING
        // engine over the TCP bridge rather than writing files. They
        // live in this crate because the bridge client is local to the
        // MCP server — the engine reaches its own ECS in-process, with
        // no TCP client of itself. Registered on top of the shared
        // baseline so they flow through `tools/list` + `tools/call`
        // exactly like the filesystem tools.
        r.register(crate::bridge_tools::InspectSceneTool);
        r.register(crate::bridge_tools::EquipToolTool);
        r.register(crate::bridge_tools::SelectEntityTool);
        r.register(crate::bridge_tools::GetEditorStateTool);
        r.register(crate::bridge_tools::InvokeActionTool);
        r.register(crate::bridge_tools::CaptureViewportTool);
        // Independent AI camera — the AI's own off-screen eyes.
        r.register(crate::bridge_tools::AiCameraSetPoseTool);
        r.register(crate::bridge_tools::AiCameraOrbitTool);
        r.register(crate::bridge_tools::AiCameraFrameTool);
        r.register(crate::bridge_tools::AiCameraCaptureTool);
        // Binary-ECS entity CRUD: OVERRIDE the disk entity tools by name so
        // they operate on binary cores via the bridge when the engine is
        // live, and fall back to the disk tool (FileSystem rep) when it's
        // closed or the engine routes the op to disk. Registered AFTER the
        // baseline, so `register` (HashMap insert by name) replaces the disk
        // versions. The in-engine Workshop uses its OWN registry, so it is
        // unaffected by these MCP-server-local overrides.
        use crate::bridge_tools::BridgeEntityTool;
        r.register(BridgeEntityTool::new("entity.create", eustress_tools::entity_tools::CreateEntityTool));
        r.register(BridgeEntityTool::new("entity.update", eustress_tools::entity_tools::UpdateEntityTool));
        r.register(BridgeEntityTool::new("entity.delete", eustress_tools::entity_tools::DeleteEntityTool));
        r.register(BridgeEntityTool::new("entity.add_tag", eustress_tools::simulation_tools::AddTagTool));
        r.register(BridgeEntityTool::new("entity.remove_tag", eustress_tools::simulation_tools::RemoveTagTool));
        // Read/list over the bridge too, so the AI's habitual discovery tools
        // SEE binary parts (not just on-disk TOML). Both target the rich live
        // `ecs.inspect`; query_entities passes `class` straight through, while
        // find_entity remaps `query` → `name_contains` (dedicated wrapper).
        r.register(BridgeEntityTool::new("ecs.inspect", eustress_tools::entity_tools::QueryEntitiesTool));
        r.register(crate::bridge_tools::FindEntityBridgeTool::default());
        r
    })
}

/// Names of the live-engine ("bridge") tools registered above. These reach
/// the RUNNING engine over its TCP bridge, so they must target the Universe
/// that actually has a live `engine.port` — not the server's nominal default
/// Universe. The dispatcher uses this to pick
/// [`crate::universe::find_live_engine_universe`] for these tools only.
pub const BRIDGE_TOOL_NAMES: &[&str] = &[
    "inspect_scene",
    "equip_tool",
    "select_entity",
    "get_editor_state",
    "invoke_action",
    "capture_viewport",
    "ai_camera_set_pose",
    "ai_camera_orbit",
    "ai_camera_frame",
    "ai_camera_capture",
];

/// True if `name` is one of the live-engine bridge tools.
pub fn is_bridge_tool(name: &str) -> bool {
    BRIDGE_TOOL_NAMES.contains(&name)
}

/// Build a `ToolContext` from the server's current Universe. When no
/// Universe is resolved yet, we return `None` — the caller can surface
/// a helpful error to the LLM rather than dispatching into a tool
/// that would write to a nonsensical path.
pub fn build_context(universe: Option<&PathBuf>) -> Option<ToolContext> {
    let universe = universe?.clone();

    // Target the engine's CURRENT Space (its persisted `last_space_path`),
    // NOT the first Space under `Spaces/`. The MCP server is out-of-process,
    // so disk-write tools (`create_entity`, `write_file`, …) must land in
    // the Space the user is actually viewing. The old "first Space" default
    // wrote everything into one Space (e.g. `Universe1/Space1`) regardless
    // of where the user was — a part created while viewing Finance/"Game
    // Economics" ended up bleeding into `Universe1/Space1`. The engine
    // auto-saves `last_space_path` on every Space switch, so reading it here
    // is the cross-process handshake. When it's absent/stale (engine never
    // run this session) we fall back to the first-Space behavior.
    let (space_root, universe_root) = match engine_current_space() {
        Some(space) if space.is_dir() => {
            // Derive the Universe from the Space (…/<Universe>/Spaces/<Space>)
            // so the context's universe agrees with its space.
            let uni = space
                .parent()
                .and_then(|p| p.parent())
                .filter(|u| u.join("Spaces").is_dir())
                .map(|u| u.to_path_buf())
                .unwrap_or_else(|| universe.clone());
            (space, uni)
        }
        _ => {
            let spaces_dir = universe.join("Spaces");
            let space_root = std::fs::read_dir(&spaces_dir)
                .ok()
                .and_then(|mut it| it.find_map(|e| e.ok().map(|e| e.path())))
                .unwrap_or_else(|| universe.clone());
            (space_root, universe)
        }
    };

    Some(ToolContext {
        space_root,
        universe_root,
        user_id: None,
        username: None,
        luau_executor: None,
        // MCP server runs out-of-process from the engine — it doesn't
        // observe the engine's `DisplayUnit` resource. Callers passing
        // sizes via the MCP boundary must declare their `unit` arg
        // explicitly; Space-default unit then acts as the fallback.
        display_unit: None,
    })
}

/// Read the engine's currently-open Space from its persisted editor
/// settings (`~/.eustress_engine/settings.json` → `last_space_path`, with
/// the legacy `~/.eustress_studio/` location as fallback). The engine
/// writes this on every Space switch (auto-saved via change detection), so
/// the out-of-process MCP server can follow the user's active Space here
/// instead of guessing the first Space under a default Universe. `None`
/// when unset / unreadable (engine never run, or a non-UI open path).
fn engine_current_space() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let current = home.join(".eustress_engine").join("settings.json");
    let legacy = home.join(".eustress_studio").join("settings.json");
    let path = if current.exists() { current } else { legacy };
    let contents = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let last = json.get("last_space_path")?.as_str()?;
    Some(PathBuf::from(last))
}

/// Definitions of every tool in the shared registry. Emitted alongside
/// the MCP server's hand-rolled `eustress_*` tools so `tools/list`
/// returns the full unified surface.
pub fn list_shared_tools() -> Vec<Value> {
    registry()
        .all_tools()
        .into_iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "description": d.description,
                "inputSchema": d.input_schema,
            })
        })
        .collect()
}

/// Attempt to dispatch a tool call through the shared registry.
/// Returns `None` if the tool isn't in the shared registry — the
/// caller should then try the hand-rolled `tools::all_tools()` list.
///
/// Returns `Some(result)` whether the tool succeeded or failed; the
/// caller shouldn't need to distinguish here.
pub fn try_dispatch(
    tool_name: &str,
    args: &Value,
    universe: Option<&PathBuf>,
) -> Option<ToolResult> {
    let r = registry();
    if !r.tool_names().contains(&tool_name) {
        return None;
    }
    let ctx = match build_context(universe) {
        Some(c) => c,
        None => {
            return Some(ToolResult {
                tool_name: tool_name.to_string(),
                tool_use_id: String::new(),
                success: false,
                content:
                    "No Universe resolved. Call `set_active_universe` with the absolute path to a Universe root, or set the `EUSTRESS_UNIVERSE` env var before starting the server, or launch the server from inside a Universe directory."
                        .to_string(),
                structured_data: None,
                stream_topic: None,
            });
        }
    };
    Some(r.dispatch(tool_name, "", args.clone(), &ctx))
}

/// Translate a `ToolResult` into the MCP-expected JSON envelope
/// (`content: [{ type, text }]` + `isError`) so responses match what
/// the hand-rolled tools already return.
pub fn to_mcp_json(result: ToolResult) -> Value {
    let body = if let Some(data) = result.structured_data.as_ref() {
        // Structured payload → JSON-pretty so the LLM sees the shape.
        serde_json::to_string_pretty(data)
            .unwrap_or_else(|_| result.content.clone())
    } else {
        result.content.clone()
    };

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": body,
        }],
        "isError": !result.success,
    })
}

