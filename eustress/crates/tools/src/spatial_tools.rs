//! Spatial reasoning tools — natural language queries about the 3D world.
//!
//! Bridges the spatial-llm crate's capabilities to the AI agent via MCP tools.
//! The agent can ask spatial questions about the scene, measure distances,
//! and query entity relationships.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

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
            description: "List entities inside a Space directory. Call with no `path` (or empty string) to see the top-level overview — services + top-level Workspace entities. Call with `path` set to a relative folder (e.g. \"Workspace/V-Cell\", \"Workspace/V-Cell/V1\") to drill into that folder or Model and see ONLY its direct children. Drilling is the cheap way to inspect a container without dumping the entire scene into context.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path from the Space root. Empty → top-level overview. Example: \"Workspace/V-Cell\" lists V-Cell's direct children."
                    }
                }
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let rel_path = input.get("path").and_then(|v| v.as_str()).unwrap_or("").trim();

        // Drill-down branch: when `path` is non-empty, list only the
        // direct children of that folder/Model. This avoids the
        // token-heavy "dump everything" behaviour when Claude already
        // knows which container it cares about.
        if !rel_path.is_empty() {
            return list_container_children(rel_path, ctx);
        }

        // Top-level branch: services + top-level Workspace entities.
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
        for (name, pos, class, has_children) in scan_entities_in_dir(&workspace) {
            entities.push(serde_json::json!({
                "name": name,
                "position": pos,
                "class_name": class,
                "has_children": has_children,
            }));
        }

        // Inline summary so the LLM actually sees service + entity
        // names. structured_data is shown to the UI, not fed back to
        // Claude — prior versions of this tool returned only counts
        // and the agent couldn't find anything by name.
        let service_lines: Vec<String> = services
            .iter()
            .map(|s| format!(
                "  - {} ({} files)",
                s.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                s.get("files").and_then(|v| v.as_u64()).unwrap_or(0),
            ))
            .collect();
        let entity_lines: Vec<String> = entities
            .iter()
            .map(|e| {
                let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let class = e.get("class_name").and_then(|v| v.as_str()).unwrap_or("Part");
                let has_children = e.get("has_children").and_then(|v| v.as_bool()).unwrap_or(false);
                let pos = e.get("position").and_then(|p| p.as_array());
                let coords = pos.and_then(|a| {
                    let x = a.first().and_then(|v| v.as_f64())?;
                    let y = a.get(1).and_then(|v| v.as_f64())?;
                    let z = a.get(2).and_then(|v| v.as_f64())?;
                    Some(format!("[{:.2}, {:.2}, {:.2}]", x, y, z))
                }).unwrap_or_default();
                // Note: container classes (Model, Folder, Configuration)
                // get a "▸" drill-down hint + the exact path to pass back
                // in the next `list_space_contents` call. Keeps the
                // agent from guessing paths.
                if has_children {
                    format!("  ▸ {} [{}] {} — drill: path=\"Workspace/{}\"", name, class, coords, name)
                } else {
                    format!("  - {} [{}] {}", name, class, coords)
                }
            })
            .collect();
        let body = format!(
            "Services ({}):\n{}\n\nWorkspace entities ({}):\n{}\n\n\
             Tip: container entities marked with ▸ can be drilled into \
             by calling this tool again with `path=\"Workspace/<name>\"`.",
            services.len(),
            if service_lines.is_empty() { "  (none)".to_string() } else { service_lines.join("\n") },
            entities.len(),
            if entity_lines.is_empty() { "  (none)".to_string() } else { entity_lines.join("\n") },
        );

        ToolResult {
            tool_name: "list_space_contents".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: body,
            structured_data: Some(serde_json::json!({
                "services": services,
                "entities": entities,
            })),
            stream_topic: None,
        }
    }
}

/// Enumerate instance entries directly inside `dir`. Returns
/// `(name, position, class_name, has_children)` tuples, including
/// folders that contain an `_instance.toml` and flat `.toml` variants
/// (`.part.toml`, `.glb.toml`, plain `.toml`). A folder whose
/// `_instance.toml` is surrounded by other sub-folders (e.g. a Model's
/// children) reports `has_children = true` so the overview can flag
/// it as drill-worthy.
fn scan_entities_in_dir(dir: &std::path::Path) -> Vec<(String, [f64; 3], String, bool)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if fname.starts_with('.') || fname == "_instance.toml" { continue; }
        let (toml_path, has_children) = if path.is_dir() {
            let inst = path.join("_instance.toml");
            if !inst.exists() { continue; }
            // A folder has children when any sibling inside it (besides
            // the _instance.toml itself) is another entity folder or
            // flat instance file.
            let children = std::fs::read_dir(&path).map(|rd| {
                rd.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n != "_instance.toml" && !n.starts_with('.') && (
                        e.path().is_dir()
                        || n.ends_with(".part.toml")
                        || n.ends_with(".glb.toml")
                        || (n.ends_with(".toml") && n != "_instance.toml")
                    )
                })
            }).unwrap_or(false);
            (inst, children)
        } else if fname.ends_with(".part.toml") || fname.ends_with(".glb.toml") || fname.ends_with(".toml") {
            (path.clone(), false)
        } else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&toml_path) else { continue };
        let Ok(val) = toml::from_str::<toml::Value>(&content) else { continue };
        let stem = fname.split('.').next().unwrap_or(&fname).to_string();
        let name = val.get("metadata").and_then(|m| m.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(&stem)
            .to_string();
        let class = val.get("metadata").and_then(|m| m.get("class_name"))
            .and_then(|c| c.as_str())
            .unwrap_or("Part")
            .to_string();
        let pos = val.get("transform").and_then(|t| t.get("position")).and_then(|p| p.as_array())
            .map(|a| {
                let x = a.first().and_then(|v| v.as_float()).unwrap_or(0.0);
                let y = a.get(1).and_then(|v| v.as_float()).unwrap_or(0.0);
                let z = a.get(2).and_then(|v| v.as_float()).unwrap_or(0.0);
                [x, y, z]
            })
            .unwrap_or([0.0, 0.0, 0.0]);
        out.push((name, pos, class, has_children));
    }
    out
}

/// Drill-down: list direct children of the folder `rel_path` relative
/// to the Space root. Used when Claude knows which container it wants
/// to inspect so we don't dump the entire scene into context.
fn list_container_children(rel_path: &str, ctx: &ToolContext) -> ToolResult {
    // Guard against `..` path traversal so a malformed tool call can't
    // wander outside the Space root.
    if rel_path.contains("..") {
        return ToolResult {
            tool_name: "list_space_contents".to_string(),
            tool_use_id: String::new(),
            success: false,
            content: format!("Invalid path \"{}\" — `..` is not allowed.", rel_path),
            structured_data: None,
            stream_topic: None,
        };
    }
    let target = ctx.space_root.join(rel_path.replace('\\', "/"));
    if !target.exists() || !target.is_dir() {
        return ToolResult {
            tool_name: "list_space_contents".to_string(),
            tool_use_id: String::new(),
            success: false,
            content: format!(
                "Path \"{}\" is not a directory. Call this tool with no `path` to see the top-level overview.",
                rel_path
            ),
            structured_data: None,
            stream_topic: None,
        };
    }

    let children = scan_entities_in_dir(&target);
    let mut entities_json = Vec::with_capacity(children.len());
    let mut lines = Vec::with_capacity(children.len());
    for (name, pos, class, has_children) in &children {
        entities_json.push(serde_json::json!({
            "name": name,
            "position": pos,
            "class_name": class,
            "has_children": has_children,
        }));
        let coords = format!("[{:.2}, {:.2}, {:.2}]", pos[0], pos[1], pos[2]);
        if *has_children {
            lines.push(format!("  ▸ {} [{}] {} — drill: path=\"{}/{}\"", name, class, coords, rel_path, name));
        } else {
            lines.push(format!("  - {} [{}] {}", name, class, coords));
        }
    }

    let body = format!(
        "Children of \"{}\" ({}):\n{}",
        rel_path,
        children.len(),
        if lines.is_empty() { "  (empty)".to_string() } else { lines.join("\n") },
    );

    ToolResult {
        tool_name: "list_space_contents".to_string(),
        tool_use_id: String::new(),
        success: true,
        content: body,
        structured_data: Some(serde_json::json!({
            "path": rel_path,
            "entities": entities_json,
        })),
        stream_topic: None,
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
