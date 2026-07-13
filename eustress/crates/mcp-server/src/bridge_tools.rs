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
            description: "Inspect the LIVE running engine's scene over the engine bridge. Returns per-entity class/mesh/material/color/transform/visibility/physics flags/parent/on-disk source plus current FPS — far richer than query_entities (which only reads files on disk). Use this to debug what the engine is ACTUALLY rendering right now (e.g. a wrong mesh asset). For LARGE scenes, scope the query spatially: pass `cell` (a sim-cell-… id from scene_overview / partition_scene) or `region` ({min,max} AABB) so you page through one spatial cell at a time instead of the whole flat list. Requires the engine to be running. Read-only.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class":         { "type": "string",  "description": "Filter to an exact class name (e.g. \"Part\", \"Model\")." },
                    "name_contains": { "type": "string",  "description": "Case-insensitive substring filter on entity name." },
                    "cell":          { "type": "string",  "description": "Scope to one 256-stud Morton cell by its sim-cell-XXXXXXX-YYYYYYY-ZZZZZZZ id (get ids from scene_overview or partition_scene). Wins over region if both given." },
                    "region":        { "type": "object",  "description": "Scope to a world-space AABB: {min:[x,y,z], max:[x,y,z]}. Entities without a position never match.",
                                       "properties": { "min": { "type": "array", "items": { "type": "number" } },
                                                       "max": { "type": "array", "items": { "type": "number" } } } },
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
        for key in ["class", "name_contains", "cell", "region", "offset", "limit"] {
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
// scene_overview  ->  scene.overview
// ---------------------------------------------------------------------------

pub struct SceneOverviewTool;

impl ToolHandler for SceneOverviewTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "scene_overview",
            description: "Region-organized structured summary of the LIVE scene: entities bucketed into 256-stud Morton cells (the same grid the engine's streaming/HLOD/forge-SimCell systems share). Each cell has a stable sim-cell-… id, world bounds, entity count, and a class histogram, plus scene-wide totals/bounds. THIS is how to approach a large scene (10K+ entities): call this first to see WHERE things are, then drill into individual cells with inspect_scene {cell: <id>} — never page an unscoped list. Cells are sorted densest-first. Requires the engine to be running. Read-only.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "region":          { "type": "object",  "description": "Restrict the overview to a world-space AABB: {min:[x,y,z], max:[x,y,z]}.",
                                         "properties": { "min": { "type": "array", "items": { "type": "number" } },
                                                         "max": { "type": "array", "items": { "type": "number" } } } },
                    "max_cells":       { "type": "integer", "description": "Max cells to return (default 512, cap 4096); sparsest cells are dropped first." },
                    "classes_per_cell":{ "type": "integer", "description": "Histogram entries per cell (default 8)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let mut params = serde_json::Map::new();
        for key in ["region", "max_cells", "classes_per_cell"] {
            if let Some(v) = input.get(key) {
                if !v.is_null() {
                    params.insert(key.to_string(), v.clone());
                }
            }
        }

        match call_engine(&ctx.universe_root, "scene.overview", Value::Object(params)) {
            Ok(result) => {
                let total = result.get("total_entities").and_then(|v| v.as_u64()).unwrap_or(0);
                let cells_total = result.get("cells_total").and_then(|v| v.as_u64()).unwrap_or(0);
                let mut lines: Vec<String> = Vec::new();
                if let Some(arr) = result.get("cells").and_then(|v| v.as_array()) {
                    for c in arr.iter().take(15) {
                        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        let count = c.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                        let top_class = c
                            .get("classes")
                            .and_then(|v| v.as_array())
                            .and_then(|a| a.first())
                            .and_then(|e| e.get("class"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        lines.push(format!("  - {id}: {count} entities (mostly {top_class})"));
                    }
                    if arr.len() > 15 {
                        lines.push(format!("  … and {} more cells (see structured_data)", arr.len() - 15));
                    }
                }
                let summary = format!(
                    "{total} entities across {cells_total} occupied cells\n{}",
                    lines.join("\n")
                );
                ok("scene_overview", summary, result)
            }
            Err(e) => fail("scene_overview", e),
        }
    }
}

// ---------------------------------------------------------------------------
// partition_scene  ->  scene.overview + client-side Morton-order bin-packing
// ---------------------------------------------------------------------------

pub struct PartitionSceneTool;

impl ToolHandler for PartitionSceneTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "partition_scene",
            description: "Divide the LIVE scene into K spatially-contiguous, entity-balanced work units for multi-agent processing. Fetches the scene_overview cell digest, orders cells by Morton code (so each unit is a coherent spatial neighborhood, not scattered), and greedy-fills units to an even entity budget. Each unit lists its cell ids — hand ONE unit to each parallel agent, which then pages through its cells via inspect_scene {cell: <id>}. Unit cell ids are forge SimCell-compatible (sim-cell-…). Requires the engine to be running. Read-only.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "units":                 { "type": "integer", "description": "Target number of work units / agents (default 4, cap 64). Ignored if max_entities_per_unit is set." },
                    "max_entities_per_unit": { "type": "integer", "description": "Alternative sizing: cap each unit's entity count and emit as many units as needed." },
                    "region":                { "type": "object",  "description": "Restrict partitioning to a world-space AABB: {min:[x,y,z], max:[x,y,z]}.",
                                               "properties": { "min": { "type": "array", "items": { "type": "number" } },
                                                               "max": { "type": "array", "items": { "type": "number" } } } }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        // One engine round-trip: the full cell digest (engine cap 4096
        // cells). Everything after is pure JSON math in this process.
        let mut params = serde_json::Map::new();
        params.insert("max_cells".to_string(), serde_json::json!(4096));
        if let Some(r) = input.get("region") {
            if !r.is_null() {
                params.insert("region".to_string(), r.clone());
            }
        }
        let overview = match call_engine(&ctx.universe_root, "scene.overview", Value::Object(params)) {
            Ok(v) => v,
            Err(e) => return fail("partition_scene", e),
        };

        let cells: Vec<&Value> = overview
            .get("cells")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        let total_entities: u64 = cells
            .iter()
            .map(|c| c.get("count").and_then(|v| v.as_u64()).unwrap_or(0))
            .sum();
        if cells.is_empty() {
            return fail(
                "partition_scene",
                "scene has no positioned entities to partition (0 occupied cells)".to_string(),
            );
        }

        // Budget per unit: explicit cap, or total/K rounded up.
        let units_requested = input
            .get("units")
            .and_then(|v| v.as_u64())
            .unwrap_or(4)
            .clamp(1, 64);
        let budget = input
            .get("max_entities_per_unit")
            .and_then(|v| v.as_u64())
            .filter(|&b| b > 0)
            .unwrap_or_else(|| total_entities.div_ceil(units_requested).max(1));

        // Morton order = spatial contiguity: consecutive cells in this
        // order are near each other in the world, so a greedy fill yields
        // compact neighborhoods. The engine already computed each cell's
        // morton code — no coordinate math here.
        let mut ordered = cells;
        ordered.sort_by_key(|c| c.get("morton").and_then(|v| v.as_u64()).unwrap_or(0));

        let mut units: Vec<Value> = Vec::new();
        let mut current_cells: Vec<&Value> = Vec::new();
        let mut current_count: u64 = 0;
        let flush = |unit_cells: &[&Value], idx: usize, count: u64| -> Value {
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            let mut classes: std::collections::HashMap<String, u64> = Default::default();
            let mut ids: Vec<&str> = Vec::new();
            for c in unit_cells {
                if let Some(id) = c.get("id").and_then(|v| v.as_str()) {
                    ids.push(id);
                }
                for (axis, key) in ["min", "max"].iter().enumerate() {
                    if let Some(a) = c.get(*key).and_then(|v| v.as_array()) {
                        for (i, v) in a.iter().take(3).enumerate() {
                            let f = v.as_f64().unwrap_or(0.0);
                            if axis == 0 { min[i] = min[i].min(f); } else { max[i] = max[i].max(f); }
                        }
                    }
                }
                if let Some(hist) = c.get("classes").and_then(|v| v.as_array()) {
                    for e in hist {
                        if let (Some(class), Some(n)) = (
                            e.get("class").and_then(|v| v.as_str()),
                            e.get("count").and_then(|v| v.as_u64()),
                        ) {
                            *classes.entry(class.to_string()).or_default() += n;
                        }
                    }
                }
            }
            let mut class_pairs: Vec<(String, u64)> = classes.into_iter().collect();
            class_pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let classes_json: Vec<Value> = class_pairs
                .into_iter()
                .take(10)
                .map(|(class, count)| serde_json::json!({ "class": class, "count": count }))
                .collect();
            serde_json::json!({
                "unit_id":      format!("unit-{idx:02}"),
                "cell_ids":     ids,
                "cell_count":   unit_cells.len(),
                "entity_count": count,
                "bounds":       { "min": min, "max": max },
                "classes":      classes_json,
                "next_step":    "for each cell id, call inspect_scene {cell: <id>} and page with offset/limit",
            })
        };
        for c in ordered {
            let n = c.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            if !current_cells.is_empty() && current_count + n > budget {
                units.push(flush(&current_cells, units.len(), current_count));
                current_cells.clear();
                current_count = 0;
            }
            current_cells.push(c);
            current_count += n;
        }
        if !current_cells.is_empty() {
            units.push(flush(&current_cells, units.len(), current_count));
        }

        let lines: Vec<String> = units
            .iter()
            .map(|u| {
                format!(
                    "  - {}: {} entities in {} cell(s)",
                    u.get("unit_id").and_then(|v| v.as_str()).unwrap_or("?"),
                    u.get("entity_count").and_then(|v| v.as_u64()).unwrap_or(0),
                    u.get("cell_count").and_then(|v| v.as_u64()).unwrap_or(0),
                )
            })
            .collect();
        let summary = format!(
            "{} work units over {} entities (budget ≈{} per unit)\n{}",
            units.len(),
            total_entities,
            budget,
            lines.join("\n")
        );
        let cells_truncated = overview
            .get("has_more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        ok(
            "partition_scene",
            summary,
            serde_json::json!({
                "units":           units,
                "total_entities":  total_entities,
                "budget_per_unit": budget,
                // Honest coverage marker: true means the engine's 4096-cell
                // digest cap truncated the overview and some sparse cells
                // are NOT in any unit — narrow with `region` in that case.
                "cells_truncated": cells_truncated,
                "cell_size":       overview.get("cell_size").cloned().unwrap_or(Value::Null),
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// sim_bindings  ->  sim.bindings
// ---------------------------------------------------------------------------

pub struct SimBindingsTool;

impl ToolHandler for SimBindingsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "sim_bindings",
            description: "Forge gang-placement outcomes for this engine session's resident cells (SimOrchestrationPlugin ledger): cell id, members placed, whole-gang committed flag. Cell ids match scene_overview/partition_scene ids, so you can join placement state onto your spatial partition. Requires an engine built with the sim-orchestration feature (not in the default build) — errors gracefully otherwise. Read-only.",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "sim.bindings", serde_json::json!({})) {
            Ok(result) => {
                let count = result.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                let mut lines: Vec<String> = Vec::new();
                if let Some(arr) = result.get("bindings").and_then(|v| v.as_array()) {
                    for b in arr.iter().take(15) {
                        let id = b.get("cell_id").and_then(|v| v.as_str()).unwrap_or("?");
                        let members = b.get("placed_members").and_then(|v| v.as_u64()).unwrap_or(0);
                        let complete = b.get("complete").and_then(|v| v.as_bool()).unwrap_or(false);
                        lines.push(format!(
                            "  - {id}: {members} member(s), gang {}",
                            if complete { "COMMITTED" } else { "incomplete" }
                        ));
                    }
                }
                ok(
                    "sim_bindings",
                    format!("{count} placement record(s)\n{}", lines.join("\n")),
                    result,
                )
            }
            Err(e) => fail("sim_bindings", e),
        }
    }
}

// ---------------------------------------------------------------------------

pub struct OplogTailTool;

impl ToolHandler for OplogTailTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "oplog_tail",
            description: "Read the LIVE engine's causal op-log over the bridge — the most recent entity mutations (create/delete) in order, each with op/class/uuid/actor (provenance) and timestamp. This is the AI's 'what changed in the world, in what order, and why' audit trail, distinct from query_audit_log. Read-only; requires the engine running. NOTE (Phase 1): captures explicit create/delete today; disk/file-watcher-create coverage is pending.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max records, oldest-first (default 50, max 1000)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let mut params = serde_json::Map::new();
        if let Some(v) = input.get("limit") {
            if !v.is_null() {
                params.insert("limit".to_string(), v.clone());
            }
        }

        match call_engine(&ctx.universe_root, "oplog.tail", Value::Object(params)) {
            Ok(result) => {
                let count = result.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                // Spell the newest records into `content` (the AI reads content).
                let mut lines: Vec<String> = Vec::new();
                if let Some(arr) = result.get("mutations").and_then(|v| v.as_array()) {
                    for m in arr.iter().rev().take(20) {
                        let seq = m.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
                        let op = m.get("op").and_then(|v| v.as_str()).unwrap_or("?");
                        let class = m.get("class").and_then(|v| v.as_str()).unwrap_or("?");
                        let uuid = m.get("uuid").and_then(|v| v.as_str()).unwrap_or("?");
                        let actor = m.get("actor").and_then(|v| v.as_str()).unwrap_or("?");
                        lines.push(format!("  #{seq} {op} {class} uuid={uuid} by={actor}"));
                    }
                }
                let summary = format!(
                    "{count} op-log record(s) (newest first):\n{}",
                    lines.join("\n")
                );
                ok("oplog_tail", summary, result)
            }
            Err(e) => fail("oplog_tail", e),
        }
    }
}

// ---------------------------------------------------------------------------

pub struct SimStepTool;

impl ToolHandler for SimStepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "sim_step",
            description: "Deterministically advance the LIVE engine's physics simulation by N fixed-timestep ticks (1 tick = 1/60s), then return — the POMDP control primitive (observe -> act -> STEP -> observe). Pause the sim first (pause_simulation) so ONLY these steps advance the world; then sim_step(ticks) advances physics by exactly that many ticks, wall-clock-independent and reproducible. After stepping, read the new state with inspect_scene. Requires the engine running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "ticks": { "type": "integer", "description": "Number of 1/60s fixed ticks to advance (default 1, max 10000)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let mut params = serde_json::Map::new();
        if let Some(v) = input.get("ticks") {
            if !v.is_null() {
                params.insert("ticks".to_string(), v.clone());
            }
        }
        match call_engine(&ctx.universe_root, "sim.step", Value::Object(params)) {
            Ok(result) => {
                let stepped = result.get("stepped").and_then(|v| v.as_u64()).unwrap_or(0);
                let secs = result.get("sim_seconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let summary = format!(
                    "stepped {stepped} fixed tick(s) ({secs:.3}s sim time); read inspect_scene for the new state"
                );
                ok("sim_step", summary, result)
            }
            Err(e) => fail("sim_step", e),
        }
    }
}

// ---------------------------------------------------------------------------

pub struct SceneRaycastTool;

impl ToolHandler for SceneRaycastTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "scene_raycast",
            description: "Cast a world-space ray against the LIVE engine's Avian colliders and return the hits (entity / name / distance / point), nearest first — the POMDP 'sense' primitive. Supply `origin` [x,y,z] and `direction` [x,y,z] (default straight down); optional `max_distance`, `max_hits`. Requires the engine running. Read-only.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "origin": { "type": "array", "items": {"type":"number"}, "description": "Ray origin [x,y,z] in meters (default [0,0,0])." },
                    "direction": { "type": "array", "items": {"type":"number"}, "description": "Ray direction [x,y,z] (need not be normalized; default [0,-1,0])." },
                    "max_distance": { "type": "number", "description": "Max ray length in meters (default 1000)." },
                    "max_hits": { "type": "integer", "description": "Max hits to return (default 8, max 256)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let mut params = serde_json::Map::new();
        for key in ["origin", "direction", "max_distance", "max_hits"] {
            if let Some(v) = input.get(key) {
                if !v.is_null() {
                    params.insert(key.to_string(), v.clone());
                }
            }
        }
        match call_engine(&ctx.universe_root, "scene.raycast", Value::Object(params)) {
            Ok(result) => {
                let n = result.get("hit_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let mut lines = Vec::new();
                if let Some(arr) = result.get("hits").and_then(|v| v.as_array()) {
                    for h in arr.iter().take(10) {
                        let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let dist = h.get("distance").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let ent = h.get("entity").and_then(|v| v.as_str()).unwrap_or("?");
                        lines.push(format!("  {name} ({ent}) @ {dist:.3}m"));
                    }
                }
                let summary = format!("{n} hit(s):\n{}", lines.join("\n"));
                ok("scene_raycast", summary, result)
            }
            Err(e) => fail("scene_raycast", e),
        }
    }
}

// ---------------------------------------------------------------------------
// new_universe / new_space — DISK tools (no engine needed). They scaffold the
// on-disk world containers so an agent can spin up a fresh sandbox to test in.
// They operate on the MCP session's active Universe (ctx.universe_root), NOT the
// live engine port — so they are NOT in BRIDGE_TOOL_NAMES.
// ---------------------------------------------------------------------------

/// Recursively copy a directory tree (used to scaffold a Space from the service
/// templates).
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

pub struct NewUniverseTool;

impl ToolHandler for NewUniverseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "new_universe",
            description: "Create a new Universe (a top-level world container with a Spaces/ folder) as a sibling of the active Universe under the Eustress documents root. Disk-based — writes the directory tree; pair with new_space to scaffold a first Space. Returns the new Universe path.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Universe folder name (e.g. 'TestWorld')." }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let name = match input.get("name").and_then(|v| v.as_str()) {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => return fail("new_universe", "missing or empty 'name'".to_string()),
        };
        // Universes are siblings under the Eustress documents root (the active
        // Universe's parent).
        let root = match ctx.universe_root.parent() {
            Some(p) => p.to_path_buf(),
            None => return fail("new_universe", "cannot resolve the documents root".to_string()),
        };
        let uni = root.join(&name);
        if uni.exists() {
            return fail("new_universe", format!("universe '{name}' already exists at {}", uni.display()));
        }
        for sub in [".eustress/assets/meshes", ".eustress/assets/parts", ".eustress/knowledge", "Spaces"] {
            if let Err(e) = std::fs::create_dir_all(uni.join(sub)) {
                return fail("new_universe", format!("create {}: {e}", uni.join(sub).display()));
            }
        }
        ok(
            "new_universe",
            format!("created Universe '{name}' at {}", uni.display()),
            serde_json::json!({ "name": name, "path": uni.to_string_lossy() }),
        )
    }
}

pub struct NewSpaceTool;

impl ToolHandler for NewSpaceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "new_space",
            description: "Create a new Space inside a Universe by scaffolding the standard service folders (Workspace, Lighting, MaterialService, ...) from the service templates. Disk-based; the engine creates the world database on first open. Defaults to the active Universe. Returns the new Space path + the services created.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Space folder name (e.g. 'Sandbox')." },
                    "universe": { "type": "string", "description": "Universe name or absolute path. Defaults to the active Universe." }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let name = match input.get("name").and_then(|v| v.as_str()) {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => return fail("new_space", "missing or empty 'name'".to_string()),
        };
        // Resolve the target Universe: explicit path, explicit name (sibling of
        // the active Universe), or the active Universe.
        let universe = match input.get("universe").and_then(|v| v.as_str()) {
            Some(u) if u.contains('/') || u.contains('\\') => std::path::PathBuf::from(u),
            Some(u) => ctx
                .universe_root
                .parent()
                .map(|p| p.join(u))
                .unwrap_or_else(|| ctx.universe_root.clone()),
            None => ctx.universe_root.clone(),
        };
        let space = universe.join("Spaces").join(&name);
        if space.exists() {
            return fail("new_space", format!("space '{name}' already exists at {}", space.display()));
        }
        let templates = eustress_common::service_templates_dir();
        if !templates.is_dir() {
            return fail("new_space", format!("service templates not found at {}", templates.display()));
        }
        if let Err(e) = std::fs::create_dir_all(space.join(".eustress")) {
            return fail("new_space", format!("create {}: {e}", space.display()));
        }
        // Copy every service-template folder into the new Space.
        let mut services = Vec::new();
        let entries = match std::fs::read_dir(&templates) {
            Ok(e) => e,
            Err(e) => return fail("new_space", format!("read service templates: {e}")),
        };
        for entry in entries.flatten() {
            let src = entry.path();
            if src.is_dir() {
                let svc = entry.file_name().to_string_lossy().to_string();
                if let Err(e) = copy_dir_all(&src, &space.join(&svc)) {
                    return fail("new_space", format!("copy service {svc}: {e}"));
                }
                services.push(svc);
            }
        }
        services.sort();
        ok(
            "new_space",
            format!(
                "created Space '{name}' at {} with {} services ({})",
                space.display(),
                services.len(),
                services.join(", ")
            ),
            serde_json::json!({ "name": name, "path": space.to_string_lossy(), "services": services }),
        )
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

// ---------------------------------------------------------------------------
// Binary-ECS entity CRUD — bridge-backed OVERRIDES of the disk entity tools
// ---------------------------------------------------------------------------
//
// Phase 1 made the Insert default to binary-ECS cores (no TOML on disk), so
// the disk entity tools (which read/write `_instance.toml`) are blind to
// user-created parts. These wrappers route create/update/delete/tag through
// the engine bridge (which owns the live World + Fjall DB) when the engine is
// running, and fall back to the original disk tool when it's closed (offline
// authoring of FileSystem-representation entities). Each reuses the disk
// tool's name + schema (so it OVERRIDES it by name in the registry) and its
// `execute` as the offline / filesystem-routed fallback.

/// Generic bridge-backed wrapper. `definition()` delegates to the wrapped
/// disk tool (identical name + schema → registry override). `execute()`
/// calls the bridge `method`; on `routed:"filesystem"` (the engine's router
/// decided this belongs on disk — custom mesh / non-Part / TOML entity) OR
/// on "engine not running" it falls back to the disk tool; any other bridge
/// error is surfaced (engine up but the op failed — don't silently diverge).
pub struct BridgeEntityTool {
    method: &'static str,
    disk: Box<dyn ToolHandler>,
}

impl BridgeEntityTool {
    pub fn new(method: &'static str, disk: impl ToolHandler) -> Self {
        Self { method, disk: Box::new(disk) }
    }
}

impl ToolHandler for BridgeEntityTool {
    fn definition(&self) -> ToolDefinition {
        self.disk.definition()
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let name = self.disk.definition().name;
        match call_engine(&ctx.universe_root, self.method, input.clone()) {
            Ok(result) => {
                if result.get("routed").and_then(|v| v.as_str()) == Some("filesystem") {
                    // The engine routed this to the filesystem representation;
                    // perform the equivalent disk op so it actually lands.
                    return self.disk.execute(input, ctx);
                }
                let summary = format!(
                    "{name} via engine bridge (binary-ECS): {}",
                    serde_json::to_string(&result).unwrap_or_default()
                );
                ok(name, summary, result)
            }
            // Engine offline → disk tool (FileSystem representation).
            Err(e) if e.contains("is not running") => self.disk.execute(input, ctx),
            // Engine up but the RPC failed — surface it (don't write a
            // divergent disk copy behind the user's back).
            Err(e) => fail(name, e),
        }
    }
}

/// `find_entity` over the bridge: maps the disk tool's `query` param to
/// `ecs.inspect`'s `name_contains`, so it finds LIVE entities (including
/// binary-ECS parts that have no TOML on disk) by name — not just on-disk
/// folders. Falls back to the disk folder-name search when the engine is
/// offline. (Resident-only: a streamed-out part won't match by name, since
/// only uuid/path/class are indexed — use those via the engine for those.)
pub struct FindEntityBridgeTool {
    disk: eustress_tools::universe_tools::FindEntityTool,
}

impl Default for FindEntityBridgeTool {
    fn default() -> Self {
        Self { disk: eustress_tools::universe_tools::FindEntityTool }
    }
}

impl ToolHandler for FindEntityBridgeTool {
    fn definition(&self) -> ToolDefinition {
        self.disk.definition()
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        match call_engine(
            &ctx.universe_root,
            "ecs.inspect",
            serde_json::json!({ "name_contains": query }),
        ) {
            Ok(result) => {
                let total = result.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                let returned = result.get("returned").and_then(|v| v.as_u64()).unwrap_or(0);
                let mut lines: Vec<String> = Vec::new();
                if let Some(arr) = result.get("entities").and_then(|v| v.as_array()) {
                    for e in arr.iter().take(20) {
                        let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let class = e.get("class").and_then(|v| v.as_str()).unwrap_or("?");
                        let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        lines.push(format!("  - {name} ({class}) id={id}"));
                    }
                }
                ok(
                    "find_entity",
                    format!(
                        "{returned} live match(es) for '{query}' (of {total} scanned)\n{}",
                        lines.join("\n")
                    ),
                    result,
                )
            }
            Err(e) if e.contains("is not running") => self.disk.execute(input, ctx),
            Err(e) => fail("find_entity", e),
        }
    }
}

// ---------------------------------------------------------------------------
// promote_entity / demote_entity  ->  entity.promote / entity.demote (Phase 3.5)
// ---------------------------------------------------------------------------
//
// Bridge-ONLY (no disk fallback): "change representation" is an in-process
// operation on the live World + single-writer DB, so when the engine is down
// these fail with a clear message rather than falling back (unlike the CRUD
// tools). They write a folder + rewrite identity stores → `requires_approval`.

/// Materialize a binary-ECS entity into an on-disk TOML folder ("Export to disk").
pub struct PromoteEntityTool;

impl ToolHandler for PromoteEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "promote_entity",
            description: "Materialize a binary-ECS entity (a bare Part stored in the world DB, not on disk) into an on-disk Workspace/<Name>/_instance.toml FOLDER — making it path-addressable, hand-editable, and file-attachable while preserving its uuid and appearance. The explicit 'Export to disk' action. Identify the entity by `uuid` (preferred) or `name` (must be resident). Requires the engine to be running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "uuid": { "type": "string", "description": "32-char hex uuid of the entity (preferred)." },
                    "name": { "type": "string", "description": "Entity name (resident match) if the uuid is unknown." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let mut params = serde_json::Map::new();
        for key in ["uuid", "name"] {
            if let Some(v) = input.get(key) {
                if !v.is_null() {
                    params.insert(key.to_string(), v.clone());
                }
            }
        }
        match call_engine(&ctx.universe_root, "entity.promote", Value::Object(params)) {
            Ok(result) => {
                let path = result.get("path").and_then(|v| v.as_str()).unwrap_or("(folder)");
                ok("promote_entity", format!("Promoted to disk: {path}"), result)
            }
            Err(e) if e.contains("is not running") => fail(
                "promote_entity",
                "The engine must be running to promote an entity (binary→disk happens in-process).".to_string(),
            ),
            Err(e) => fail("promote_entity", e),
        }
    }
}

/// Fold a bare, artifact-free FileSystem entity back into a binary-ECS core.
pub struct DemoteEntityTool;

impl ToolHandler for DemoteEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "demote_entity",
            description: "Fold a bare, artifact-free FileSystem entity (a Workspace/<Name>/ TOML folder) back into a binary-ECS core and DELETE its disk folder — the reverse of promote_entity. Identify by `uuid` (preferred) or `name`. Fails if the folder still has attached files or the class/mesh isn't binary-eligible. Requires the engine to be running.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "uuid": { "type": "string", "description": "32-char hex uuid (preferred)." },
                    "name": { "type": "string", "description": "Entity name (resident match)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let mut params = serde_json::Map::new();
        for key in ["uuid", "name"] {
            if let Some(v) = input.get(key) {
                if !v.is_null() {
                    params.insert(key.to_string(), v.clone());
                }
            }
        }
        match call_engine(&ctx.universe_root, "entity.demote", Value::Object(params)) {
            Ok(result) => ok(
                "demote_entity",
                "Demoted to binary-ECS (disk folder removed).".to_string(),
                result,
            ),
            Err(e) if e.contains("is not running") => fail(
                "demote_entity",
                "The engine must be running to demote an entity.".to_string(),
            ),
            Err(e) => fail("demote_entity", e),
        }
    }
}
