//! Spatial reasoning tools — natural language queries about the 3D world.
//!
//! Bridges the spatial-llm crate's capabilities to the AI agent via MCP tools.
//! The agent can ask spatial questions about the scene, measure distances,
//! and query entity relationships.

use super::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Measure Distance
// ---------------------------------------------------------------------------

pub struct MeasureDistanceTool;

impl ToolHandler for MeasureDistanceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "measure_distance",
            description: "Calculate the Euclidean distance between two 3D points in world coordinates. Returns distance in studs (1 stud = 1 meter).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "from": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] start point" },
                    "to": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] end point" }
                },
                "required": ["from", "to"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let from = parse_vec3(&input, "from");
        let to = parse_vec3(&input, "to");

        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let dz = to[2] - from[2];
        let distance = (dx * dx + dy * dy + dz * dz).sqrt();

        ToolResult {
            tool_name: "measure_distance".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Distance: {:.3} studs ({:.3} meters)", distance, distance),
            structured_data: Some(serde_json::json!({
                "distance": distance,
                "from": from,
                "to": to,
                "delta": [dx, dy, dz],
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// List Space Contents
// ---------------------------------------------------------------------------

pub struct ListSpaceContentsTool;

impl ToolHandler for ListSpaceContentsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_space_contents",
            description: "List all services and top-level entities in the current Space. Returns service names with file counts, and Workspace entity names with positions. Provides a high-level overview of the 3D scene structure.",
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
        let mut services = Vec::new();
        let mut entities = Vec::new();

        // Scan top-level directories (services)
        if let Ok(entries) = std::fs::read_dir(&ctx.space_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if path.is_dir() && !name.starts_with('.') && name != "meshes" {
                    let file_count = std::fs::read_dir(&path)
                        .map(|rd| rd.flatten().count())
                        .unwrap_or(0);
                    services.push(serde_json::json!({ "name": name, "files": file_count }));
                }
            }
        }

        // Scan Workspace entities
        let workspace = ctx.space_root.join("Workspace");
        if let Ok(entries) = std::fs::read_dir(&workspace) {
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Resolve TOML: folder/_instance.toml or flat .part.toml/.glb.toml
                let toml_path = if path.is_dir() {
                    let inst = path.join("_instance.toml");
                    if inst.exists() { inst } else { continue; }
                } else if fname.ends_with(".part.toml") || fname.ends_with(".glb.toml") {
                    path.clone()
                } else {
                    continue;
                };
                if let Ok(content) = std::fs::read_to_string(&toml_path) {
                    if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                        let name = val.get("metadata").and_then(|m| m.get("name")).and_then(|n| n.as_str()).unwrap_or(fname);
                        let pos = val.get("transform").and_then(|t| t.get("position")).and_then(|p| p.as_array())
                            .map(|a| {
                                let x = a.get(0).and_then(|v| v.as_float()).unwrap_or(0.0);
                                let y = a.get(1).and_then(|v| v.as_float()).unwrap_or(0.0);
                                let z = a.get(2).and_then(|v| v.as_float()).unwrap_or(0.0);
                                [x, y, z]
                            })
                            .unwrap_or([0.0, 0.0, 0.0]);
                        entities.push(serde_json::json!({ "name": name, "position": pos }));
                    }
                }
            }
        }

        ToolResult {
            tool_name: "list_space_contents".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("{} services, {} workspace entities", services.len(), entities.len()),
            structured_data: Some(serde_json::json!({
                "services": services,
                "entities": entities,
            })),
            stream_topic: None,
        }
    }
}

fn parse_vec3(input: &serde_json::Value, key: &str) -> [f64; 3] {
    input.get(key).and_then(|v| v.as_array()).map(|a| {
        [
            a.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0),
            a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
            a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
        ]
    }).unwrap_or([0.0, 0.0, 0.0])
}
