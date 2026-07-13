//! CAD tools — create parametric feature-tree parts via the filesystem.
//!
//! Writes a folder with `_instance.toml` + `features.toml`. The engine's
//! `CadPlugin` attaches CadPart when it sees `features.toml` next to
//! an instance and regenerates the mesh.

use crate::modes::WorkshopMode;
use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};

fn ok(name: &str, content: impl Into<String>, data: serde_json::Value) -> ToolResult {
    ToolResult {
        tool_name: name.to_string(),
        tool_use_id: String::new(),
        success: true,
        content: content.into(),
        structured_data: Some(data),
        stream_topic: None,
    }
}

fn err(name: &str, msg: impl Into<String>) -> ToolResult {
    ToolResult {
        tool_name: name.to_string(),
        tool_use_id: String::new(),
        success: false,
        content: msg.into(),
        structured_data: None,
        stream_topic: None,
    }
}

/// Resolve a user-supplied path against the Space sandbox (the same
/// contract as `file_tools::resolve_sandboxed_path`, but rooted at
/// `space_root` since CadParts live inside the current Space).
///
/// `PathBuf::join` REPLACES the base when handed an absolute or rooted
/// path, so absolute inputs must be rejected before joining — and `..`
/// must be rejected separately because `starts_with` is lexical and
/// would accept `<root>/../elsewhere`.
fn resolve_space_path(
    ctx: &ToolContext,
    raw: &str,
) -> Result<std::path::PathBuf, String> {
    let cleaned = raw.trim().replace('\\', "/");
    if cleaned.contains("..") {
        return Err(format!("path must not contain '..' (got '{raw}')"));
    }
    let resolved = ctx.space_root.join(&cleaned);
    if resolved.starts_with(&ctx.space_root) {
        Ok(resolved)
    } else {
        Err(format!(
            "path must be relative to the Space, not absolute (got '{raw}')"
        ))
    }
}

pub struct CadCreatePartTool;

impl ToolHandler for CadCreatePartTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_create_part",
            description: "Create a parametric CadPart (feature-tree solid) in Workspace. Writes _instance.toml + features.toml. Templates: plate (default), box, cylinder. The engine attaches CadPart and tessellates the feature tree into a live mesh.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Part folder name (default CadPart)"
                    },
                    "template": {
                        "type": "string",
                        "description": "plate | box | cylinder",
                        "default": "plate"
                    },
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "[x,y,z] meters"
                    },
                    "parent": {
                        "type": "string",
                        "description": "Path relative to Workspace/ for nesting under a Model"
                    }
                },
                "required": []
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_create_part"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("CadPart");
        let template = input
            .get("template")
            .and_then(|v| v.as_str())
            .unwrap_or("plate")
            .to_ascii_lowercase();
        let pos = parse_vec3(&input, "position", [0.0, 0.5, 0.0]);
        let parent = input
            .get("parent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        let features = match template.as_str() {
            "box" => eustress_cad::templates::BOX_TOML,
            "cylinder" | "cyl" => eustress_cad::templates::CYLINDER_TOML,
            _ => eustress_cad::templates::PLATE_TOML,
        };

        let rel = if parent.is_empty() {
            "Workspace".to_string()
        } else {
            format!("Workspace/{parent}")
        };
        let dir = match resolve_space_path(ctx, &rel) {
            Ok(d) => d,
            Err(e) => return err("cad_create_part", e),
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return err("cad_create_part", format!("mkdir: {e}"));
        }

        let mut folder = name.to_string();
        let mut n = 0u32;
        while dir.join(&folder).exists() {
            n += 1;
            folder = format!("{name}-{n}");
        }
        let instance_dir = dir.join(&folder);
        if let Err(e) = std::fs::create_dir_all(&instance_dir) {
            return err("cad_create_part", format!("mkdir instance: {e}"));
        }

        let uuid = eustress_common::instance_create::fresh_uuid_for_create();
        let instance_toml = format!(
            r#"[metadata]
class_name = "Part"
archivable = true
name = "{folder}"
uuid = "{uuid}"

[transform]
position = [{}, {}, {}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [0.1, 0.01, 0.06]

[properties]
color = [140, 158, 184]
transparency = 0.0
anchored = true
can_collide = true
cast_shadow = true
reflectance = 0.0
material = "Plastic"
locked = false

[asset]
mesh = "parts/block.glb"
scene = "Scene0"
"#,
            pos[0], pos[1], pos[2]
        );

        let toml_path = instance_dir.join("_instance.toml");
        let features_path = instance_dir.join("features.toml");
        if let Err(e) = std::fs::write(&toml_path, instance_toml) {
            return err("cad_create_part", format!("write instance: {e}"));
        }
        if let Err(e) = std::fs::write(&features_path, features) {
            return err("cad_create_part", format!("write features: {e}"));
        }

        ok(
            "cad_create_part",
            format!("Created CadPart '{folder}' ({template}) at {}", instance_dir.display()),
            serde_json::json!({
                "ok": true,
                "name": folder,
                "template": template,
                "path": instance_dir.to_string_lossy(),
                "features": features_path.to_string_lossy(),
            }),
        )
    }
}

pub struct CadSetVariableTool;

impl ToolHandler for CadSetVariableTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_set_variable",
            description: "Set a feature-tree variable by rewriting features.toml (e.g. height = \"0.02 m\"). Path is the CadPart folder or features.toml file.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to features.toml or the CadPart folder, relative to the Space"
                    },
                    "name": { "type": "string", "description": "Variable name (e.g. height)" },
                    "value": { "type": "string", "description": "Quantity string (e.g. \"0.02 m\")" }
                },
                "required": ["path", "name", "value"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_set_variable"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = input.get("value").and_then(|v| v.as_str()).unwrap_or("");
        if path_s.is_empty() || name.is_empty() || value.is_empty() {
            return err("cad_set_variable", "path, name, and value required");
        }

        let mut path = match resolve_space_path(ctx, path_s) {
            Ok(p) => p,
            Err(e) => return err("cad_set_variable", e),
        };
        if path.is_dir() {
            path = path.join("features.toml");
        }
        if !path.is_file() {
            return err(
                "cad_set_variable",
                format!("features.toml not found at {}", path.display()),
            );
        }

        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return err("cad_set_variable", format!("read: {e}")),
        };

        let new_src = match patch_variable(&src, name, value) {
            Ok(s) => s,
            Err(e) => return err("cad_set_variable", e),
        };
        // Never write a features.toml the kernel can't parse back —
        // a bad patch would strand the part until hand-repaired.
        if let Err(e) = eustress_cad::parse_tree(&new_src) {
            return err(
                "cad_set_variable",
                format!("patched features.toml would not parse ({e}); write aborted"),
            );
        }
        if let Err(e) = std::fs::write(&path, new_src) {
            return err("cad_set_variable", format!("write: {e}"));
        }

        ok(
            "cad_set_variable",
            format!("Set {name} = {value} in {}", path.display()),
            serde_json::json!({
                "ok": true,
                "path": path.to_string_lossy(),
                "name": name,
                "value": value
            }),
        )
    }
}

fn patch_variable(src: &str, name: &str, value: &str) -> Result<String, String> {
    let mut lines: Vec<String> = src.lines().map(|l| l.to_string()).collect();
    let mut in_vars = false;
    let mut found = false;
    let key_prefix = format!("{name} ");
    let key_eq = format!("{name}=");
    for line in lines.iter_mut() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_vars = trimmed == "[variables]" || trimmed.starts_with("[variables]");
            continue;
        }
        if in_vars
            && (trimmed.starts_with(&key_prefix)
                || trimmed.starts_with(&key_eq)
                || trimmed.starts_with(&format!("{name}\t")))
        {
            *line = format!("{name} = \"{value}\"");
            found = true;
            break;
        }
    }
    if !found {
        if let Some(ix) = lines.iter().position(|l| l.trim() == "[variables]") {
            lines.insert(ix + 1, format!("{name} = \"{value}\""));
        } else {
            lines.insert(0, format!("[variables]\n{name} = \"{value}\""));
        }
    }
    Ok(lines.join("\n") + "\n")
}

fn parse_vec3(input: &serde_json::Value, key: &str, default: [f64; 3]) -> [f64; 3] {
    input
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            [
                a.first().and_then(|x| x.as_f64()).unwrap_or(default[0]),
                a.get(1).and_then(|x| x.as_f64()).unwrap_or(default[1]),
                a.get(2).and_then(|x| x.as_f64()).unwrap_or(default[2]),
            ]
        })
        .unwrap_or(default)
}

pub struct CadExportGlbTool;

impl ToolHandler for CadExportGlbTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_export_glb",
            description: "Export a CadPart features.toml to a binary glTF (.glb) file with parametric extras. Path is the features.toml or CadPart folder. Optionally set out= relative path for the .glb.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to features.toml or CadPart folder, relative to the Space"
                    },
                    "out": {
                        "type": "string",
                        "description": "Output .glb path relative to the Space (default: alongside features.toml as export.glb)"
                    }
                },
                "required": ["path"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_export_glb"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path_s.is_empty() {
            return err("cad_export_glb", "path required");
        }
        let mut path = match resolve_space_path(ctx, path_s) {
            Ok(p) => p,
            Err(e) => return err("cad_export_glb", e),
        };
        if path.is_dir() {
            path = path.join("features.toml");
        }
        if !path.is_file() {
            return err(
                "cad_export_glb",
                format!("features.toml not found at {}", path.display()),
            );
        }
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return err("cad_export_glb", format!("read: {e}")),
        };

        let out = match input.get("out").and_then(|v| v.as_str()) {
            Some(o) => match resolve_space_path(ctx, o) {
                Ok(p) => p,
                Err(e) => return err("cad_export_glb", e),
            },
            // Default lands alongside features.toml, which is already
            // inside the sandbox.
            None => path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("export.glb"),
        };
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let tree = match eustress_cad::parse_tree(&src) {
            Ok(t) => t,
            Err(e) => return err("cad_export_glb", format!("parse: {e}")),
        };
        let eval = match eustress_cad::evaluate_tree(&tree) {
            Ok(o) => o,
            Err(e) => return err("cad_export_glb", format!("eval: {e}")),
        };
        let Some(mesh) = eval.mesh.filter(|m| !m.indices.is_empty()) else {
            return err("cad_export_glb", "evaluation produced empty mesh");
        };
        let extras = serde_json::json!({
            "eustress": {
                "kind": "CadPart",
                "generator": "eustress-cad",
                "variables": tree.variables,
            }
        });
        if let Err(e) = eustress_cad::write_glb(&out, &mesh, Some(extras)) {
            return err("cad_export_glb", format!("write glb: {e}"));
        }

        ok(
            "cad_export_glb",
            format!(
                "Exported GLB → {} ({} tris)",
                out.display(),
                mesh.indices.len() / 3
            ),
            serde_json::json!({
                "ok": true,
                "out": out.to_string_lossy(),
                "features": path.to_string_lossy(),
                "triangles": mesh.indices.len() / 3,
                "vertices": mesh.positions.len(),
            }),
        )
    }
}

