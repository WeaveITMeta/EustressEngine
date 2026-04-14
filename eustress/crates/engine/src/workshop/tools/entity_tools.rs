//! Entity management tools — create and query entities in the Space.

use super::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Create Entity
// ---------------------------------------------------------------------------

pub struct CreateEntityTool;

impl ToolHandler for CreateEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "create_entity",
            description: "Create a new 3D entity in the current Space's Workspace. Supported classes: Part, Model. Writes a .part.toml file that the engine hot-reloads.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class": { "type": "string", "description": "Entity class: Part (3D primitive or custom mesh via asset path), Model (group of Parts)", "default": "Part" },
                    "name": { "type": "string", "description": "Entity name" },
                    "position": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] world position" },
                    "size": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] size in studs" },
                    "material": { "type": "string", "description": "Material preset: Plastic, SmoothPlastic, Wood, WoodPlanks, Metal, CorrodedMetal, DiamondPlate, Foil, Grass, Concrete, Brick, Granite, Marble, Slate, Sand, Fabric, Glass, Neon, Ice" },
                    "color": { "type": "array", "items": { "type": "number" }, "description": "[r, g, b] color (0-1 range)" }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.create_entity"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let class = input.get("class").and_then(|v| v.as_str()).unwrap_or("Part");
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("NewPart");
        let position = parse_vec3(&input, "position", [0.0, 0.0, 0.0]);
        let size = parse_vec3(&input, "size", [1.0, 1.0, 1.0]);
        let material = input.get("material").and_then(|v| v.as_str()).unwrap_or("Plastic");
        let color = parse_vec3(&input, "color", [0.639, 0.635, 0.647]);

        let safe_name = name.replace(' ', "_").replace('/', "_");
        let workspace_dir = ctx.space_root.join("Workspace");
        let instance_dir = workspace_dir.join(&safe_name);
        let _ = std::fs::create_dir_all(&instance_dir);
        let filepath = instance_dir.join("_instance.toml");

        let toml_content = format!(
r#"[metadata]
class_name = "{class}"
name = "{name}"

[transform]
position = [{}, {}, {}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [{}, {}, {}]

[properties]
material = "{material}"
color = [{}, {}, {}, 1.0]
transparency = 0.0
anchored = true
can_collide = true
"#,
            position[0], position[1], position[2],
            size[0], size[1], size[2],
            color[0], color[1], color[2],
        );

        match std::fs::write(&filepath, &toml_content) {
            Ok(_) => ToolResult {
                tool_name: "create_entity".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: format!("Created {} '{}' at [{:.1}, {:.1}, {:.1}]", class, name, position[0], position[1], position[2]),
                structured_data: Some(serde_json::json!({ "class": class, "name": name, "file": filepath.to_string_lossy() })),
                stream_topic: Some("workshop.tool.create_entity".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "create_entity".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("Failed to create entity: {}", e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Query Entities
// ---------------------------------------------------------------------------

pub struct QueryEntitiesTool;

impl ToolHandler for QueryEntitiesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_entities",
            description: "Query entities in the current Space's Workspace. Optionally filter by class. Returns names, classes, and file paths.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class": { "type": "string", "description": "Filter by entity class: Part or Model" }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let class_filter = input.get("class").and_then(|v| v.as_str());
        let workspace_dir = ctx.space_root.join("Workspace");
        let mut entities = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&workspace_dir) {
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
                        let class = val.get("metadata").and_then(|m| m.get("class_name")).and_then(|c| c.as_str()).unwrap_or("Unknown");
                        let name = val.get("metadata").and_then(|m| m.get("name")).and_then(|n| n.as_str()).unwrap_or(fname);
                        if let Some(f) = class_filter { if class != f { continue; } }
                        entities.push(serde_json::json!({ "name": name, "class": class, "file": fname }));
                    }
                }
            }
        }

        ToolResult {
            tool_name: "query_entities".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Found {} entities{}", entities.len(), class_filter.map(|c| format!(" of class '{}'", c)).unwrap_or_default()),
            structured_data: Some(serde_json::json!({ "entities": entities })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Update Entity
// ---------------------------------------------------------------------------

pub struct UpdateEntityTool;

impl ToolHandler for UpdateEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "update_entity",
            description: "Update properties of an existing entity in the Workspace. Reads the .part.toml or .glb.toml file, modifies specified properties, and writes back. The engine hot-reloads the changes.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Entity name (matches [metadata].name in the TOML)" },
                    "position": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] new position" },
                    "size": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] new size" },
                    "material": { "type": "string", "description": "New material preset" },
                    "color": { "type": "array", "items": { "type": "number" }, "description": "[r, g, b] new color (0-1)" },
                    "transparency": { "type": "number", "description": "Transparency (0.0 = opaque, 1.0 = invisible)" },
                    "anchored": { "type": "boolean", "description": "Whether the entity is anchored (immovable)" },
                    "can_collide": { "type": "boolean", "description": "Whether the entity participates in collision" }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.update_entity"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let safe_name = name.replace(' ', "_").replace('/', "_");
        let workspace = ctx.space_root.join("Workspace");

        // Find the entity file: folder/_instance.toml or legacy flat files
        let folder_path = workspace.join(&safe_name).join("_instance.toml");
        let candidates = [
            folder_path,
            workspace.join(format!("{}.part.toml", safe_name)),
            workspace.join(format!("{}.glb.toml", safe_name)),
        ];
        let filepath = match candidates.iter().find(|p| p.exists()) {
            Some(p) => p.clone(),
            None => return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Entity '{}' not found in Workspace", name),
                structured_data: None, stream_topic: None,
            },
        };

        // Read existing TOML
        let content = match std::fs::read_to_string(&filepath) {
            Ok(c) => c,
            Err(e) => return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to read {}: {}", filepath.display(), e),
                structured_data: None, stream_topic: None,
            },
        };

        let mut doc: toml::Value = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to parse TOML: {}", e),
                structured_data: None, stream_topic: None,
            },
        };

        let mut changes = Vec::new();

        // Apply position
        if let Some(pos) = input.get("position").and_then(|v| v.as_array()) {
            if let Some(transform) = doc.get_mut("transform").and_then(|t| t.as_table_mut()) {
                let arr: Vec<toml::Value> = pos.iter().map(|v| toml::Value::Float(v.as_f64().unwrap_or(0.0))).collect();
                transform.insert("position".to_string(), toml::Value::Array(arr));
                changes.push("position");
            }
        }

        // Apply size (stored as scale in transform)
        if let Some(size) = input.get("size").and_then(|v| v.as_array()) {
            if let Some(transform) = doc.get_mut("transform").and_then(|t| t.as_table_mut()) {
                let arr: Vec<toml::Value> = size.iter().map(|v| toml::Value::Float(v.as_f64().unwrap_or(1.0))).collect();
                transform.insert("scale".to_string(), toml::Value::Array(arr));
                changes.push("size");
            }
        }

        // Apply properties
        if let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) {
            if let Some(mat) = input.get("material").and_then(|v| v.as_str()) {
                props.insert("material".to_string(), toml::Value::String(mat.to_string()));
                changes.push("material");
            }
            if let Some(color) = input.get("color").and_then(|v| v.as_array()) {
                let mut arr: Vec<toml::Value> = color.iter().map(|v| toml::Value::Float(v.as_f64().unwrap_or(0.5))).collect();
                if arr.len() == 3 { arr.push(toml::Value::Float(1.0)); }
                props.insert("color".to_string(), toml::Value::Array(arr));
                changes.push("color");
            }
            if let Some(t) = input.get("transparency").and_then(|v| v.as_f64()) {
                props.insert("transparency".to_string(), toml::Value::Float(t));
                changes.push("transparency");
            }
            if let Some(a) = input.get("anchored").and_then(|v| v.as_bool()) {
                props.insert("anchored".to_string(), toml::Value::Boolean(a));
                changes.push("anchored");
            }
            if let Some(c) = input.get("can_collide").and_then(|v| v.as_bool()) {
                props.insert("can_collide".to_string(), toml::Value::Boolean(c));
                changes.push("can_collide");
            }
        }

        if changes.is_empty() {
            return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: true, content: format!("No changes specified for '{}'", name),
                structured_data: None, stream_topic: None,
            };
        }

        // Write back
        let new_content = toml::to_string_pretty(&doc).unwrap_or_default();
        match std::fs::write(&filepath, &new_content) {
            Ok(_) => ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Updated '{}': {}", name, changes.join(", ")),
                structured_data: Some(serde_json::json!({ "name": name, "changed": changes })),
                stream_topic: Some("workshop.tool.update_entity".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to write: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Delete Entity
// ---------------------------------------------------------------------------

pub struct DeleteEntityTool;

impl ToolHandler for DeleteEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "delete_entity",
            description: "Delete an entity from the Workspace by removing its .part.toml or .glb.toml file. The engine will despawn the entity on next file-watcher cycle.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Entity name to delete" }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &["workshop.tool.delete_entity"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let safe_name = name.replace(' ', "_").replace('/', "_");
        let workspace = ctx.space_root.join("Workspace");

        // Try folder-based first, then legacy flat files
        let folder_path = workspace.join(&safe_name);
        if folder_path.is_dir() && folder_path.join("_instance.toml").exists() {
            match std::fs::remove_dir_all(&folder_path) {
                Ok(_) => return ToolResult {
                    tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                    success: true,
                    content: format!("Deleted entity '{}' (folder)", name),
                    structured_data: Some(serde_json::json!({ "name": name, "file": folder_path.to_string_lossy() })),
                    stream_topic: Some("workshop.tool.delete_entity".to_string()),
                },
                Err(e) => return ToolResult {
                    tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                    success: false, content: format!("Failed to delete folder: {}", e),
                    structured_data: None, stream_topic: None,
                },
            }
        }

        // Legacy flat file fallback
        let legacy_candidates = [
            workspace.join(format!("{}.part.toml", safe_name)),
            workspace.join(format!("{}.glb.toml", safe_name)),
        ];
        for path in &legacy_candidates {
            if path.exists() {
                match std::fs::remove_file(path) {
                    Ok(_) => return ToolResult {
                        tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                        success: true,
                        content: format!("Deleted entity '{}' ({})", name, path.file_name().unwrap_or_default().to_string_lossy()),
                        structured_data: Some(serde_json::json!({ "name": name, "file": path.to_string_lossy() })),
                        stream_topic: Some("workshop.tool.delete_entity".to_string()),
                    },
                    Err(e) => return ToolResult {
                        tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                        success: false, content: format!("Failed to delete: {}", e),
                        structured_data: None, stream_topic: None,
                    },
                }
            }
        }

        ToolResult {
            tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
            success: false, content: format!("Entity '{}' not found in Workspace", name),
            structured_data: None, stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_vec3(input: &serde_json::Value, key: &str, default: [f32; 3]) -> [f32; 3] {
    input.get(key).and_then(|v| v.as_array()).map(|a| {
        [
            a.get(0).and_then(|v| v.as_f64()).unwrap_or(default[0] as f64) as f32,
            a.get(1).and_then(|v| v.as_f64()).unwrap_or(default[1] as f64) as f32,
            a.get(2).and_then(|v| v.as_f64()).unwrap_or(default[2] as f64) as f32,
        ]
    }).unwrap_or(default)
}
