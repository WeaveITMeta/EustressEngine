//! Entity management tools — create and query entities in the Space.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Create Entity
// ---------------------------------------------------------------------------

pub struct CreateEntityTool;

/// Map a primitive shape id (or class name) to its shared-mesh asset
/// path. Must stay in sync with `toolbox::get_mesh_catalog` on the
/// engine side — the engine's file-watcher resolves this path via
/// `PRIMITIVE_MESHES` + `material_loader::resolve_material`.
///
/// Without an `[asset]` section the file watcher's `spawn_instance`
/// hits its `asset.is_none()` branch and attaches only a bare
/// `Instance + Transform + Visibility` (no `BasePart`, `Part`,
/// `Mesh3d`, `MeshMaterial3d`, `Collider`) — which is why the
/// previous version of this tool produced entities that were
/// invisible and unselectable in the viewport.
fn primitive_mesh_path(shape: &str) -> Option<&'static str> {
    match shape.to_ascii_lowercase().as_str() {
        "block" | "part" | "cube"                 => Some("assets/parts/block.glb"),
        "ball" | "sphere"                         => Some("assets/parts/ball.glb"),
        "cylinder"                                => Some("assets/parts/cylinder.glb"),
        "wedge"                                   => Some("assets/parts/wedge.glb"),
        "corner_wedge" | "corner" | "cornerwedge" => Some("assets/parts/corner_wedge.glb"),
        "cone"                                    => Some("assets/parts/cone.glb"),
        _                                         => None,
    }
}

impl ToolHandler for CreateEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "create_entity",
            description: "Create a new 3D entity in the current Space's Workspace. Writes a folder with _instance.toml so the engine's file watcher hot-spawns it with full rendering + selection components. Supported primitive classes (Part) use the shared mesh assets under `assets/parts/*.glb`; Model creates a container folder for grouped Parts.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class": {
                        "type": "string",
                        "description": "Entity class. `Part` (default) = visible 3D primitive. `Model` = folder container (no mesh). Any other string is passed through for service/script classes.",
                        "default": "Part"
                    },
                    "shape": {
                        "type": "string",
                        "description": "Primitive shape for Part class: block (default), ball, cylinder, wedge, corner_wedge, cone. Determines which shared mesh asset is referenced in the `[asset]` section.",
                        "default": "block"
                    },
                    "name":     { "type": "string",  "description": "Entity name (used as folder + Instance.name)" },
                    "position": { "type": "array",   "items": { "type": "number" }, "description": "[x, y, z] world position in studs" },
                    "size":     { "type": "array",   "items": { "type": "number" }, "description": "[x, y, z] size in studs (maps to Transform.scale)" },
                    "material": { "type": "string",  "description": "Material preset: Plastic, SmoothPlastic, Wood, WoodPlanks, Metal, CorrodedMetal, DiamondPlate, Foil, Grass, Concrete, Brick, Granite, Marble, Slate, Sand, Fabric, Glass, Neon, Ice" },
                    "color":    { "type": "array",   "items": { "type": "number" }, "description": "[r, g, b] color — either 0.0-1.0 floats or 0-255 integers" },
                    "anchored":     { "type": "boolean", "description": "Prevents physics from moving the part (default true)" },
                    "can_collide":  { "type": "boolean", "description": "Whether the part participates in collisions (default true)" }
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
        let shape = input.get("shape").and_then(|v| v.as_str()).unwrap_or("block");
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("NewPart");
        let position = parse_vec3(&input, "position", [0.0, 0.0, 0.0]);
        let size = parse_vec3(&input, "size", [1.0, 1.0, 1.0]);
        let material = input.get("material").and_then(|v| v.as_str()).unwrap_or("Plastic");
        let color = parse_vec3(&input, "color", [0.639, 0.635, 0.647]);
        let anchored = input.get("anchored").and_then(|v| v.as_bool()).unwrap_or(true);
        let can_collide = input.get("can_collide").and_then(|v| v.as_bool()).unwrap_or(true);

        let safe_name = name.replace(' ', "_").replace('/', "_");
        let workspace_dir = ctx.space_root.join("Workspace");
        let instance_dir = workspace_dir.join(&safe_name);
        let _ = std::fs::create_dir_all(&instance_dir);
        let filepath = instance_dir.join("_instance.toml");

        // Pick the mesh asset for the class. `Part` + any known
        // primitive shape gets a real `assets/parts/*.glb` reference.
        // `Model` / unknown classes skip the `[asset]` section (they
        // render as folder-containers, not meshes).
        let asset_section = if class.eq_ignore_ascii_case("Part") {
            let mesh = primitive_mesh_path(shape)
                .unwrap_or("assets/parts/block.glb");
            format!(
"[asset]\nmesh = \"{mesh}\"\nscene = \"Scene0\"\n\n"
            )
        } else if let Some(mesh) = primitive_mesh_path(class) {
            // Caller passed a shape name as `class` (e.g. "Ball") —
            // accept it so older prompts keep working.
            format!(
"[asset]\nmesh = \"{mesh}\"\nscene = \"Scene0\"\n\n"
            )
        } else {
            String::new()
        };

        let toml_content = format!(
r#"[metadata]
class_name = "{class}"
name = "{name}"
archivable = true

{asset_section}[transform]
position = [{px}, {py}, {pz}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [{sx}, {sy}, {sz}]

[properties]
material = "{material}"
color = [{cr}, {cg}, {cb}, 1.0]
transparency = 0.0
anchored = {anchored}
can_collide = {can_collide}
cast_shadow = true
reflectance = 0.0
locked = false
"#,
            px = position[0], py = position[1], pz = position[2],
            sx = size[0],     sy = size[1],     sz = size[2],
            cr = color[0],    cg = color[1],    cb = color[2],
        );

        match std::fs::write(&filepath, &toml_content) {
            Ok(_) => ToolResult {
                tool_name: "create_entity".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: format!(
                    "Created {} '{}' at [{:.1}, {:.1}, {:.1}] (shape={}, asset={})",
                    class, name, position[0], position[1], position[2], shape,
                    if asset_section.is_empty() { "none" } else { "attached" }
                ),
                structured_data: Some(serde_json::json!({
                    "class": class,
                    "shape": shape,
                    "name": name,
                    "file": filepath.to_string_lossy(),
                    "has_asset": !asset_section.is_empty(),
                })),
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
                        // Name fallback: strip the compound extension
                        // from the filename so `Grid.part.toml` yields
                        // "Grid", not the full filename. Folder-based
                        // entities use their parent directory name
                        // (`Block/` with `_instance.toml` → "Block").
                        let stem = if path.is_dir() {
                            path.file_name().and_then(|n| n.to_str()).unwrap_or(fname).to_string()
                        } else {
                            fname.split('.').next().unwrap_or(fname).to_string()
                        };
                        let name = val.get("metadata").and_then(|m| m.get("name")).and_then(|n| n.as_str()).unwrap_or(&stem);
                        if let Some(f) = class_filter { if class != f { continue; } }
                        entities.push(serde_json::json!({ "name": name, "class": class, "file": fname }));
                    }
                }
            }
        }

        // Claude reads `content`, not `structured_data`, so spell the
        // entities out in the summary instead of parking them in a
        // sidecar field the LLM never sees. Without this, the agent
        // reliably called `query_entities` and then asked "but I don't
        // know the names" because `content` only said "Found N".
        let filter_note = class_filter
            .map(|c| format!(" of class '{}'", c))
            .unwrap_or_default();
        let body = if entities.is_empty() {
            format!("No entities{} in Workspace.", filter_note)
        } else {
            let lines: Vec<String> = entities
                .iter()
                .map(|e| format!(
                    "  - {} ({})",
                    e.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                    e.get("class").and_then(|v| v.as_str()).unwrap_or("?"),
                ))
                .collect();
            format!(
                "Found {} entities{}:\n{}",
                entities.len(),
                filter_note,
                lines.join("\n")
            )
        };

        ToolResult {
            tool_name: "query_entities".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: body,
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
