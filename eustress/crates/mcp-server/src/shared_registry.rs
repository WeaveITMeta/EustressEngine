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
        // Large-scene orchestration surface: the Morton-cell digest an
        // orchestrator reads first, and the partitioner that turns it
        // into balanced per-agent work units (cell ids are forge
        // SimCell-compatible). Detail-paging stays in inspect_scene via
        // its new `cell`/`region` params — one query tool, spatially
        // scoped, not a parallel one.
        r.register(crate::bridge_tools::SceneOverviewTool);
        r.register(crate::bridge_tools::PartitionSceneTool);
        // Forge gang-placement visibility (engine must be built with
        // sim-orchestration; the bridge errors gracefully otherwise).
        r.register(crate::bridge_tools::SimBindingsTool);
        r.register(crate::bridge_tools::DataBindTool);
        r.register(crate::bridge_tools::DataBindingsTool);
        r.register(crate::bridge_tools::DataUnbindTool);
        r.register(crate::bridge_tools::EquipToolTool);
        r.register(crate::bridge_tools::SelectEntityTool);
        r.register(crate::bridge_tools::GetEditorStateTool);
        r.register(crate::bridge_tools::InvokeActionTool);
        r.register(crate::bridge_tools::CaptureViewportTool);
        // Causal op-log read surface (Phase 1, Way 8) — the AI's audit trail.
        r.register(crate::bridge_tools::OplogTailTool);
        // POMDP control primitive (Phase 2) — deterministic stepped simulation.
        r.register(crate::bridge_tools::SimStepTool);
        // POMDP sense primitive (Phase 2) — world-space raycast.
        r.register(crate::bridge_tools::SceneRaycastTool);
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
        // Phase 3.5 — explicit promote (binary→disk TOML folder) / demote
        // (disk→binary). Bridge-only (no disk fallback): changing representation
        // is an in-process op on the live World + single-writer DB.
        r.register(crate::bridge_tools::PromoteEntityTool);
        r.register(crate::bridge_tools::DemoteEntityTool);
        // Bulk DB → TOML dump. Distinct from promote: it copies the database
        // out for reading rather than changing what owns an entity, and it
        // covers streamed-out entities that promote can't touch.
        r.register(crate::bridge_tools::ExportInstancesTomlTool);
        // Disk world-container tools — create Universes / Spaces (no engine).
        r.register(crate::bridge_tools::NewUniverseTool);
        r.register(crate::bridge_tools::NewSpaceTool);
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
    "scene_overview",
    "partition_scene",
    "sim_bindings",
    "data_bind",
    "data_bindings",
    "data_unbind",
    "equip_tool",
    "select_entity",
    "get_editor_state",
    "invoke_action",
    "capture_viewport",
    "oplog_tail",
    "sim_step",
    "scene_raycast",
    "ai_camera_set_pose",
    "ai_camera_orbit",
    "ai_camera_frame",
    "ai_camera_capture",
    "export_instances_toml",
    // Bridge-only too: both fail outright without a live engine, so pointing
    // them at the server's nominal default Universe rather than the running
    // one just produces "engine is not running" against a Universe that was
    // never the target.
    "promote_entity",
    "demote_entity",
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
        cancelled: None,
        // CMMC AC.L1-3.1.2. The MCP server is an out-of-process surface
        // reached by external clients (IDEs, agents). It gets the standard
        // set — Read and Write — so an MCP client cannot invoke `run_bash`,
        // `execute_luau`, or `delete_entity`. Raising this is a deliberate
        // act that should follow peer authentication, not precede it.
        permissions: eustress_tools::Permissions::standard().for_principal("mcp-client"),
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
///
/// Sorted by name: `ToolRegistry` stores handlers in a `HashMap`, whose
/// iteration order is reseeded per process, so an unsorted list shuffles on
/// every server restart. Since `tools/list` feeds the model's system prompt,
/// that shuffle invalidates prompt caches and makes the surface impossible to
/// diff between runs.
pub fn list_shared_tools() -> Vec<Value> {
    let mut annotated = registry().all_tools_annotated();
    annotated.sort_by_key(|(d, _)| d.name);
    annotated
        .into_iter()
        .map(|(d, read_only)| {
            serde_json::json!({
                "name": d.name,
                "description": d.description,
                "inputSchema": d.input_schema,
                // MCP tool annotations (2025-06-18), both sourced from what
                // the handler itself declares — `ToolHandler::read_only` and
                // `requires_approval` — so a new tool cannot drift out of
                // sync with a list maintained somewhere else.
                "annotations": {
                    "readOnlyHint": read_only,
                    "destructiveHint": d.requires_approval,
                    // Everything here targets the local Universe or the local
                    // engine; `http_request` is the one tool that reaches the
                    // open internet.
                    "openWorldHint": d.name == "http_request",
                },
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
    cancelled: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Option<ToolResult> {
    let r = registry();
    if !r.tool_names().contains(&tool_name) {
        return None;
    }
    let ctx = match build_context(universe) {
        Some(mut c) => {
            // Long-polling tools (`await_simulation`, `run_experiment`) read
            // this between polls, so a client cancel stops the wait instead of
            // running out the full `timeout_s`.
            c.cancelled = cancelled;
            c
        }
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

/// Keys a tool can set in `structured_data` to return an actual image.
/// `to_mcp_json` lifts them out into an MCP `image` content block, so the
/// bytes reach clients that have no filesystem access. Underscore-prefixed
/// so they never collide with a real engine field.
pub const IMAGE_B64_KEY: &str = "_mcp_image_base64";
pub const IMAGE_MIME_KEY: &str = "_mcp_image_mime";

/// Cap on the raw-JSON block appended after the human summary. `ecs.inspect`
/// over a 5 000-entity scene pretty-prints to megabytes; emitting that
/// verbatim buries the summary and can blow the client's context window.
/// Past the cap we drop to compact JSON, then truncate with an explicit
/// marker — never a silent trim.
const MAX_STRUCTURED_BYTES: usize = 120 * 1024;

/// Render `structured_data` for the trailing text block, bounded by
/// [`MAX_STRUCTURED_BYTES`].
fn render_structured(data: &Value) -> String {
    let pretty = serde_json::to_string_pretty(data).unwrap_or_default();
    if pretty.len() <= MAX_STRUCTURED_BYTES {
        return pretty;
    }
    let compact = serde_json::to_string(data).unwrap_or_default();
    if compact.len() <= MAX_STRUCTURED_BYTES {
        return compact;
    }
    // Cut on a char boundary so the JSON stays valid UTF-8, and say so.
    let mut end = MAX_STRUCTURED_BYTES;
    while end > 0 && !compact.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n[structured payload truncated at {} of {} bytes — narrow the query \
         (class / name_contains / cell / region / limit) for the full result]",
        &compact[..end],
        end,
        compact.len()
    )
}

/// Translate a `ToolResult` into the MCP-expected `CallToolResult` envelope.
///
/// Emits the handler's human summary FIRST, then the structured payload as a
/// separate block. The previous version returned the payload *instead of* the
/// summary whenever `structured_data` was present — which silently discarded
/// every summary the bridge tools build (`inspect_scene` spells out entity
/// names/classes/mesh ids precisely because a reader reads `content`), and
/// replaced a two-line digest with the whole raw result.
pub fn to_mcp_json(result: ToolResult) -> Value {
    let mut content: Vec<Value> = Vec::new();

    if !result.content.trim().is_empty() {
        content.push(serde_json::json!({ "type": "text", "text": result.content }));
    }

    let mut structured = result.structured_data;

    // Lift an embedded image (screenshot tools) into a real MCP image block.
    if let Some(Value::Object(map)) = structured.as_mut() {
        if let Some(b64) = map
            .remove(IMAGE_B64_KEY)
            .and_then(|v| v.as_str().map(str::to_owned))
        {
            let mime = map
                .remove(IMAGE_MIME_KEY)
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "image/png".to_string());
            content.push(serde_json::json!({
                "type": "image",
                "data": b64,
                "mimeType": mime,
            }));
        }
    }

    if let Some(data) = structured.as_ref().filter(|d| !d.is_null()) {
        content.push(serde_json::json!({
            "type": "text",
            "text": render_structured(data),
        }));
    }

    // A CallToolResult with no content at all is legal but unhelpful.
    if content.is_empty() {
        content.push(serde_json::json!({ "type": "text", "text": "(no output)" }));
    }

    let mut envelope = serde_json::json!({
        "content": content,
        "isError": !result.success,
    });

    // MCP 2025-06-18 `structuredContent`, for clients that prefer parsing over
    // scraping. Objects only — the spec types this field as an object, so an
    // array or scalar payload stays in the text block alone.
    if let Some(Value::Object(_)) = structured.as_ref() {
        envelope["structuredContent"] = structured.unwrap();
    }

    envelope
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_with(content: &str, data: Option<Value>) -> ToolResult {
        ToolResult {
            tool_name: "t".into(),
            tool_use_id: String::new(),
            success: true,
            content: content.into(),
            structured_data: data,
            stream_topic: None,
        }
    }

    #[test]
    fn summary_survives_structured_data() {
        let out = to_mcp_json(result_with(
            "3 entities; fps=60.0",
            Some(serde_json::json!({ "total": 3 })),
        ));
        let blocks = out["content"].as_array().unwrap();
        assert_eq!(blocks[0]["text"], "3 entities; fps=60.0");
        assert!(blocks[1]["text"].as_str().unwrap().contains("\"total\""));
        assert_eq!(out["structuredContent"]["total"], 3);
    }

    #[test]
    fn image_becomes_an_image_block() {
        let out = to_mcp_json(result_with(
            "shot saved",
            Some(serde_json::json!({
                "path": "/tmp/a.png",
                IMAGE_B64_KEY: "QUJD",
                IMAGE_MIME_KEY: "image/png",
            })),
        ));
        let blocks = out["content"].as_array().unwrap();
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["data"], "QUJD");
        // The base64 must not also be duplicated into the JSON block.
        assert!(!blocks[2]["text"].as_str().unwrap().contains("QUJD"));
    }

    #[test]
    fn oversized_payload_is_marked_truncated() {
        let big: Vec<u64> = (0..200_000).collect();
        let out = to_mcp_json(result_with("big", Some(serde_json::json!({ "v": big }))));
        let text = out["content"].as_array().unwrap()[1]["text"].as_str().unwrap();
        assert!(text.contains("truncated at"));
    }

    fn annotation(tools: &[Value], name: &str, key: &str) -> Value {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is registered"))["annotations"][key]
            .clone()
    }

    #[test]
    fn read_only_hint_comes_from_the_handler() {
        let tools = list_shared_tools();
        // Bridge query, disk query, and a bridge-wrapped disk query all
        // declare themselves read-only.
        assert_eq!(annotation(&tools, "inspect_scene", "readOnlyHint"), true);
        assert_eq!(annotation(&tools, "read_file", "readOnlyHint"), true);
        assert_eq!(annotation(&tools, "query_entities", "readOnlyHint"), true);
        assert_eq!(annotation(&tools, "find_entity", "readOnlyHint"), true);
        // Mutating tools do not, and approval-gated ones are flagged.
        assert_eq!(annotation(&tools, "delete_entity", "readOnlyHint"), false);
        assert_eq!(annotation(&tools, "delete_entity", "destructiveHint"), true);
        assert_eq!(annotation(&tools, "write_file", "readOnlyHint"), false);
        assert_eq!(annotation(&tools, "run_bash", "destructiveHint"), true);
        assert_eq!(annotation(&tools, "http_request", "openWorldHint"), true);
        assert_eq!(annotation(&tools, "read_file", "openWorldHint"), false);
    }

    #[test]
    fn export_tool_is_registered_and_gated() {
        let tools = list_shared_tools();
        assert!(tools.iter().any(|t| t["name"] == "export_instances_toml"));
        // It writes a directory tree, so it must not read as a safe query.
        assert_eq!(
            annotation(&tools, "export_instances_toml", "readOnlyHint"),
            false
        );
        assert_eq!(
            annotation(&tools, "export_instances_toml", "destructiveHint"),
            true
        );
        // Every bridge tool must resolve against the LIVE engine's Universe.
        for name in ["export_instances_toml", "promote_entity", "demote_entity"] {
            assert!(is_bridge_tool(name), "{name} should be a bridge tool");
        }
    }

    #[test]
    fn tool_list_order_is_stable() {
        let a: Vec<_> = list_shared_tools()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(a, sorted);
    }
}

