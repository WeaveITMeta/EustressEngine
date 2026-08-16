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

// ════════════════════════════════════════════════════════════════════
// Read path — how an agent perceives geometry it authored.
//
// Everything below is read-only: parse a feature tree, evaluate it, and
// report what the kernel already computed. Nothing writes to disk or
// touches the ECS, so these are safe to call between every authoring
// step — which is the point. CAD failures are silent and partial (a
// boolean can "succeed" and hand back a solid with holes), so the
// failure has to be probed for after each edit rather than inferred
// from a final export.
// ════════════════════════════════════════════════════════════════════

use eustress_cad::{mass_properties, min_distance, topology, EvalMesh, EvalOutput, FeatureTree};

/// Resolve a CadPart folder or `features.toml` path and read the source.
fn read_features(
    ctx: &ToolContext,
    raw: &str,
    tool: &str,
) -> Result<(std::path::PathBuf, String), ToolResult> {
    if raw.trim().is_empty() {
        return Err(err(tool, "path required (CadPart folder or features.toml, Space-relative)"));
    }
    let mut path = resolve_space_path(ctx, raw).map_err(|e| err(tool, e))?;
    if path.is_dir() {
        path = path.join("features.toml");
    }
    if !path.is_file() {
        return Err(err(
            tool,
            format!("features.toml not found at {}", path.display()),
        ));
    }
    let src = std::fs::read_to_string(&path).map_err(|e| err(tool, format!("read: {e}")))?;
    Ok((path, src))
}

// The geometry itself lives in the kernel (`eustress_cad::measure`),
// where it is unit-tested against analytic solids and reusable by the
// Studio panel and exporter. This module only formats the results.

// ── Unit handling ────────────────────────────────────────────────────

/// Resolve a length-valued expression to metres.
///
/// `Quantity::parse` accepts a bare number and calls it `Scalar`, which
/// is exactly the silent 1000x-error the unit system exists to prevent —
/// so a bare number is rejected here rather than assumed to be metres.
fn length_meters(expr: &str, vars: &std::collections::HashMap<String, String>) -> Result<f64, String> {
    match eustress_cad::feature_tree::resolve_quantity(expr, vars) {
        Some(q) => match q.unit {
            eustress_cad::Unit::Length(_) => Ok(q.to_si()),
            eustress_cad::Unit::Scalar => Err(format!(
                "'{expr}' has no unit — write a unit string like \"0.02 m\" or \"20 mm\""
            )),
            other => Err(format!("'{expr}' is {other:?}, expected a length")),
        },
        None => Err(format!("could not resolve '{expr}' to a quantity")),
    }
}

/// Parse a density string such as `"7850 kg/m^3"` or `"7.85 g/cm^3"`
/// into kg/m³. Bare numbers are rejected for the same reason lengths are.
fn parse_density(s: &str) -> Result<f64, String> {
    let raw = s.trim();
    let (mass_part, vol_part) = raw
        .split_once('/')
        .ok_or_else(|| format!("density '{raw}' must look like \"7850 kg/m^3\""))?;
    let q = eustress_cad::Quantity::parse(mass_part.trim())
        .ok_or_else(|| format!("could not parse mass '{}' in density", mass_part.trim()))?;
    let kg = match q.unit {
        eustress_cad::Unit::Mass(_) => q.to_si(),
        _ => {
            return Err(format!(
                "density numerator '{}' needs a mass unit (kg, g, lb)",
                mass_part.trim()
            ))
        }
    };
    let v = vol_part.trim().to_ascii_lowercase().replace('^', "");
    let per_m3 = match v.as_str() {
        "m3" => 1.0,
        "cm3" => 1.0e-6,
        "mm3" => 1.0e-9,
        "l" | "liter" | "litre" => 1.0e-3,
        other => {
            return Err(format!(
                "unsupported density volume unit '{other}' (use m^3, cm^3, mm^3, or L)"
            ))
        }
    };
    Ok(kg / per_m3)
}

// ── Shared report builders ───────────────────────────────────────────

fn solve_status_str(s: eustress_cad::SolveStatus) -> &'static str {
    match s {
        eustress_cad::SolveStatus::UnderConstrained => "under_constrained",
        eustress_cad::SolveStatus::FullyConstrained => "fully_constrained",
        eustress_cad::SolveStatus::OverConstrained => "over_constrained",
        eustress_cad::SolveStatus::Failed => "failed",
    }
}

/// Feature kind + display name for one tree entry, without needing a
/// match arm per `Feature` variant (both enums are serde-tagged, so the
/// tag round-trips through a `toml::Value`).
fn entry_kind(entry: &eustress_cad::FeatureEntry) -> (&'static str, String, Option<String>) {
    match entry {
        eustress_cad::FeatureEntry::Sketch { name, .. } => ("sketch", name.clone(), None),
        eustress_cad::FeatureEntry::Feature { name, body } => {
            let op = toml::Value::try_from(body)
                .ok()
                .and_then(|v| v.get("op").and_then(|o| o.as_str().map(|s| s.to_string())));
            ("feature", name.clone(), op)
        }
        eustress_cad::FeatureEntry::Suppressed { name, body } => {
            let op = body.get("op").and_then(|o| o.as_str().map(|s| s.to_string()));
            ("suppressed", name.clone(), op)
        }
    }
}

fn mesh_json(mesh: Option<&EvalMesh>, weld_eps: f64) -> serde_json::Value {
    let Some(mesh) = mesh else {
        return serde_json::json!({
            "is_empty": true,
            "triangle_count": 0,
            "vertex_count": 0,
        });
    };
    let mp = mass_properties(mesh);
    let topo = topology(mesh, weld_eps);
    serde_json::json!({
        "is_empty": mesh.indices.is_empty(),
        "triangle_count": mp.triangles,
        "vertex_count": mesh.positions.len(),
        "welded_vertex_count": topo.welded_vertices,
        "bbox_min_m": mp.min,
        "bbox_max_m": mp.max,
        "bbox_size_m": mp.size(),
        "volume_m3": mp.signed_volume.abs(),
        "surface_area_m2": mp.surface_area,
        "centroid_m": mp.centroid,
        "is_watertight": topo.boundary_edges == 0 && !mesh.indices.is_empty(),
        "is_manifold": topo.boundary_edges == 0
            && topo.nonmanifold_edges == 0
            && !mesh.indices.is_empty(),
        "boundary_edges": topo.boundary_edges,
        "nonmanifold_edges": topo.nonmanifold_edges,
        "degenerate_triangles": topo.degenerate_triangles,
        "winding_inverted": mp.signed_volume < 0.0,
    })
}

fn push_check(out: &mut Vec<serde_json::Value>, name: &str, pass: bool, detail: String) {
    out.push(serde_json::json!({
        "check": name,
        "pass": pass,
        "detail": detail,
    }));
}

fn entry_status_json(eval: &EvalOutput) -> Vec<serde_json::Value> {
    eval.entry_status
        .iter()
        .enumerate()
        .map(|(i, s)| {
            serde_json::json!({
                "index": i,
                "name": s.name,
                "ok": s.ok,
                "message": s.message,
            })
        })
        .collect()
}

// ── cad_describe_part ────────────────────────────────────────────────

pub struct CadDescribePartTool;

impl ToolHandler for CadDescribePartTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_describe_part",
            description: "Read a CadPart's feature tree: variables (with units and resolved metres), ordered features with per-feature evaluation status, sketch constraint status (SolveStatus + remaining DOF), and mesh statistics. This is the primary way to inspect geometry you created — call it after each authoring step, because CAD failures are silent and partial. Succeeds even when the tree fails to evaluate, reporting why.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "CadPart folder or features.toml, relative to the Space (e.g. Workspace/MyPart)"
                    },
                    "include_sketches": {
                        "type": "boolean",
                        "description": "Solve and report each sketch's constraint status + free DOF",
                        "default": true
                    },
                    "include_mesh_stats": {
                        "type": "boolean",
                        "description": "Evaluate the tree and report triangle/vertex counts, bbox, volume, manifoldness",
                        "default": true
                    }
                },
                "required": ["path"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_describe_part"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "cad_describe_part";
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let want_sketches = input
            .get("include_sketches")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let want_mesh = input
            .get("include_mesh_stats")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let (path, src) = match read_features(ctx, path_s, TOOL) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let tree: FeatureTree = match eustress_cad::parse_tree(&src) {
            Ok(t) => t,
            Err(e) => return err(TOOL, format!("parse: {e}")),
        };

        // Variables, each resolved to metres where it is a length. A
        // bare number is reported as such rather than silently treated
        // as metres — that mistake is invisible in the output mesh.
        let mut variables = Vec::new();
        for (name, expr) in &tree.variables {
            let q = eustress_cad::feature_tree::resolve_quantity(expr, &tree.variables);
            let (unit, meters, unitless) = match q {
                Some(q) => match q.unit {
                    eustress_cad::Unit::Length(u) => {
                        (format!("{u:?}"), Some(q.to_si()), false)
                    }
                    eustress_cad::Unit::Scalar => ("scalar".to_string(), None, true),
                    other => (format!("{other:?}"), None, false),
                },
                None => ("unresolved".to_string(), None, false),
            };
            variables.push(serde_json::json!({
                "name": name,
                "expression": expr,
                "unit": unit,
                "meters": meters,
                "unitless": unitless,
            }));
        }
        variables.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

        // Ordered entries. Index is the number to pass to a future
        // suppress/delete, and the number an error message should name.
        let mut features = Vec::new();
        let mut sketches = Vec::new();
        for (i, entry) in tree.entries.iter().enumerate() {
            let (kind, name, op) = entry_kind(entry);
            features.push(serde_json::json!({
                "index": i,
                "kind": kind,
                "name": name,
                "op": op,
                "suppressed": kind == "suppressed",
            }));

            if want_sketches {
                if let eustress_cad::FeatureEntry::Sketch { name, body } = entry {
                    let mut row = serde_json::json!({
                        "index": i,
                        "name": name,
                        "plane": body.plane,
                        "entity_count": body.entities.len(),
                        "constraint_count": body.constraints.len(),
                        "dimension_count": body.dimensions.len(),
                    });
                    match eustress_cad::solve_sketch(body, &tree.variables) {
                        Ok(rep) => {
                            row["solve_status"] = solve_status_str(rep.status).into();
                            row["free_dof"] = rep.free_dof.into();
                            row["converged"] = rep.converged.into();
                            row["residual_norm"] = rep.residual_norm.into();
                            row["iterations"] = rep.iterations.into();
                        }
                        Err(e) => {
                            row["solve_status"] = "error".into();
                            row["solve_error"] = e.to_string().into();
                        }
                    }
                    sketches.push(row);
                }
            }
        }

        // Evaluation is attempted last and its failure is reported, not
        // raised: a tree that will not evaluate is exactly when an agent
        // most needs to see the variables and feature list.
        let mut evaluates = true;
        let mut eval_error: Option<String> = None;
        let mut mesh_stats = serde_json::Value::Null;
        let mut per_feature = Vec::new();
        if want_mesh {
            match eustress_cad::evaluate_tree(&tree) {
                Ok(out) => {
                    let tol = tree
                        .metadata
                        .mesh_tolerance
                        .unwrap_or(eustress_cad::DEFAULT_MESH_TOLERANCE);
                    mesh_stats = mesh_json(out.mesh.as_ref(), tol);
                    per_feature = entry_status_json(&out);
                }
                Err(e) => {
                    evaluates = false;
                    eval_error = Some(e.to_string());
                }
            }
        }

        let summary = if !evaluates {
            format!(
                "{} — DOES NOT EVALUATE: {}",
                path.display(),
                eval_error.clone().unwrap_or_default()
            )
        } else {
            let head = format!(
                "{} — {} variables, {} entries",
                path.display(),
                variables.len(),
                tree.entries.len()
            );
            match mesh_stats.get("triangle_count").and_then(|v| v.as_u64()) {
                Some(tri) => format!("{head}, {tri} tris"),
                // Mesh stats were skipped, so say nothing about the mesh
                // rather than reporting a zero we never measured.
                None => head,
            }
        };

        ok(
            TOOL,
            summary,
            serde_json::json!({
                "ok": true,
                "path": path.to_string_lossy(),
                "evaluates": evaluates,
                "eval_error": eval_error,
                "variables": variables,
                "features": features,
                "sketches": sketches,
                "feature_status": per_feature,
                "mesh": mesh_stats,
            }),
        )
    }
}

// ── cad_validate_part ────────────────────────────────────────────────

pub struct CadValidatePartTool;

impl ToolHandler for CadValidatePartTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_validate_part",
            description: "Check a CadPart for geometric defects: parse/evaluation failure, per-feature errors, empty body, open (non-watertight) surface, non-manifold edges, degenerate triangles, inverted winding, and zero/negative volume. Returns a pass/fail per check, naming the offending feature index where the kernel knows it.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "CadPart folder or features.toml, relative to the Space"
                    },
                    "tolerance": {
                        "type": "string",
                        "description": "Vertex-weld tolerance as a unit string (e.g. \"0.001 mm\"). Supported length units: m, mm, cm, km, in, ft, yd, stud. Defaults to the tree's mesh tolerance. A bare number is read as metres."
                    }
                },
                "required": ["path"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_validate_part"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "cad_validate_part";
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let (path, src) = match read_features(ctx, path_s, TOOL) {
            Ok(v) => v,
            Err(e) => return e,
        };

        let mut checks: Vec<serde_json::Value> = Vec::new();

        let tree = match eustress_cad::parse_tree(&src) {
            Ok(t) => {
                push_check(&mut checks, "parses", true, "features.toml parsed".into());
                t
            }
            Err(e) => {
                push_check(&mut checks, "parses", false, e.to_string());
                return ok(
                    TOOL,
                    format!("{} — FAIL: does not parse", path.display()),
                    serde_json::json!({
                        "ok": true, "valid": false,
                        "path": path.to_string_lossy(),
                        "checks": checks,
                    }),
                );
            }
        };

        // Weld tolerance: explicit override → tree metadata → default.
        let default_tol = tree
            .metadata
            .mesh_tolerance
            .unwrap_or(eustress_cad::DEFAULT_MESH_TOLERANCE);
        let weld_eps = match input.get("tolerance") {
            Some(serde_json::Value::String(s)) => match length_meters(s, &tree.variables) {
                Ok(m) => m,
                Err(e) => return err(TOOL, e),
            },
            Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(default_tol),
            _ => default_tol,
        };

        let eval = match eustress_cad::evaluate_tree(&tree) {
            Ok(out) => {
                push_check(&mut checks, "evaluates", true, "feature tree evaluated".into());
                out
            }
            Err(e) => {
                push_check(&mut checks, "evaluates", false, e.to_string());
                return ok(
                    TOOL,
                    format!("{} — FAIL: {}", path.display(), e),
                    serde_json::json!({
                        "ok": true, "valid": false,
                        "path": path.to_string_lossy(),
                        "checks": checks,
                    }),
                );
            }
        };

        // Per-feature status — this is what names the failing index.
        let failed: Vec<String> = eval
            .entry_status
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.ok)
            .map(|(i, s)| format!("feature[{i}] {}: {}", s.name, s.message))
            .collect();
        push_check(
            &mut checks,
            "all_features_ok",
            failed.is_empty(),
            if failed.is_empty() {
                format!("{} feature(s) evaluated cleanly", eval.entry_status.len())
            } else {
                failed.join("; ")
            },
        );

        let mesh = eval.mesh.as_ref();
        let empty = mesh.map(|m| m.indices.is_empty()).unwrap_or(true);
        push_check(
            &mut checks,
            "non_empty_body",
            !empty,
            if empty {
                "evaluation produced no triangles — the body is empty".into()
            } else {
                format!("{} triangles", mesh.map(|m| m.indices.len() / 3).unwrap_or(0))
            },
        );

        if let (Some(mesh), false) = (mesh, empty) {
            let mp = mass_properties(mesh);
            let topo = topology(mesh, weld_eps);

            push_check(
                &mut checks,
                "watertight",
                topo.boundary_edges == 0,
                if topo.boundary_edges == 0 {
                    "closed surface — every edge shared by exactly 2 triangles".into()
                } else {
                    format!(
                        "{} boundary edge(s) — surface is open, so volume is not trustworthy",
                        topo.boundary_edges
                    )
                },
            );
            push_check(
                &mut checks,
                "manifold",
                topo.nonmanifold_edges == 0,
                if topo.nonmanifold_edges == 0 {
                    "no edge shared by 3+ triangles".into()
                } else {
                    format!(
                        "{} non-manifold edge(s) — typically a self-intersecting boolean",
                        topo.nonmanifold_edges
                    )
                },
            );
            push_check(
                &mut checks,
                "no_degenerate_triangles",
                topo.degenerate_triangles == 0,
                if topo.degenerate_triangles == 0 {
                    "no zero-area or collapsed triangles".into()
                } else {
                    format!("{} degenerate triangle(s)", topo.degenerate_triangles)
                },
            );
            let vol = mp.signed_volume;
            push_check(
                &mut checks,
                "positive_volume",
                vol.abs() > 1.0e-15 && vol > 0.0,
                if vol.abs() <= 1.0e-15 {
                    "volume is zero — the body has no interior".to_string()
                } else if vol < 0.0 {
                    format!("volume is negative ({vol:.9} m³) — surface winding is inverted")
                } else {
                    format!("{vol:.9} m³")
                },
            );
        }

        let valid = checks.iter().all(|c| c["pass"].as_bool().unwrap_or(false));
        let failed_names: Vec<&str> = checks
            .iter()
            .filter(|c| !c["pass"].as_bool().unwrap_or(false))
            .filter_map(|c| c["check"].as_str())
            .collect();

        ok(
            TOOL,
            if valid {
                format!("{} — VALID ({} checks passed)", path.display(), checks.len())
            } else {
                format!("{} — INVALID: {}", path.display(), failed_names.join(", "))
            },
            serde_json::json!({
                "ok": true,
                "valid": valid,
                "path": path.to_string_lossy(),
                "weld_tolerance_m": weld_eps,
                "checks": checks,
                "failed": failed_names,
            }),
        )
    }
}

// ── cad_measure ──────────────────────────────────────────────────────

pub struct CadMeasureTool;

impl ToolHandler for CadMeasureTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_measure",
            description: "Compute mass properties of a CadPart: volume, surface area, centre of mass, and axis-aligned bounding box. Optionally give a density unit string to also get mass, and a second part to get the exact minimum distance between the two surfaces plus whether their bounds overlap.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "CadPart folder or features.toml, relative to the Space"
                    },
                    "against": {
                        "type": "string",
                        "description": "Optional second CadPart for clearance — returns exact minimum surface-to-surface distance"
                    },
                    "density": {
                        "type": "string",
                        "description": "Optional density unit string, e.g. \"7850 kg/m^3\" or \"7.85 g/cm^3\", to also return mass"
                    }
                },
                "required": ["path"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_measure"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "cad_measure";
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let (path, src) = match read_features(ctx, path_s, TOOL) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let tree = match eustress_cad::parse_tree(&src) {
            Ok(t) => t,
            Err(e) => return err(TOOL, format!("parse: {e}")),
        };
        let eval = match eustress_cad::evaluate_tree(&tree) {
            Ok(o) => o,
            Err(e) => return err(TOOL, format!("eval: {e}")),
        };
        let Some(mesh) = eval.mesh.as_ref().filter(|m| !m.indices.is_empty()) else {
            return err(
                TOOL,
                "evaluation produced an empty body — nothing to measure (run cad_validate_part)",
            );
        };

        let tol = tree
            .metadata
            .mesh_tolerance
            .unwrap_or(eustress_cad::DEFAULT_MESH_TOLERANCE);
        let mp = mass_properties(mesh);
        let topo = topology(mesh, tol);
        let volume = mp.signed_volume.abs();
        let watertight = topo.boundary_edges == 0;

        let mut data = serde_json::json!({
            "ok": true,
            "path": path.to_string_lossy(),
            "volume_m3": volume,
            "surface_area_m2": mp.surface_area,
            "center_of_mass_m": mp.centroid,
            "bbox_min_m": mp.min,
            "bbox_max_m": mp.max,
            "bbox_size_m": mp.size(),
            "triangle_count": mp.triangles,
            // Volume and centre of mass come from the divergence
            // theorem: on an open surface they are meaningless, so the
            // caller is told rather than left to trust the number.
            "watertight": watertight,
            "volume_trustworthy": watertight,
            "winding_inverted": mp.signed_volume < 0.0,
        });

        if let Some(d) = input.get("density").and_then(|v| v.as_str()) {
            match parse_density(d) {
                Ok(kg_m3) => {
                    data["density_kg_m3"] = kg_m3.into();
                    data["mass_kg"] = (kg_m3 * volume).into();
                }
                Err(e) => return err(TOOL, e),
            }
        }

        if let Some(other_s) = input.get("against").and_then(|v| v.as_str()) {
            let (other_path, other_src) = match read_features(ctx, other_s, TOOL) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let other_tree = match eustress_cad::parse_tree(&other_src) {
                Ok(t) => t,
                Err(e) => return err(TOOL, format!("parse '{other_s}': {e}")),
            };
            let other_eval = match eustress_cad::evaluate_tree(&other_tree) {
                Ok(o) => o,
                Err(e) => return err(TOOL, format!("eval '{other_s}': {e}")),
            };
            let Some(other_mesh) = other_eval.mesh.as_ref().filter(|m| !m.indices.is_empty())
            else {
                return err(TOOL, format!("'{other_s}' evaluated to an empty body"));
            };
            let omp = mass_properties(other_mesh);
            let (dist, exact) = min_distance(mesh, other_mesh);
            let overlap = (0..3).all(|k| mp.min[k] <= omp.max[k] && omp.min[k] <= mp.max[k]);

            data["against"] = serde_json::json!({
                "path": other_path.to_string_lossy(),
                "volume_m3": omp.signed_volume.abs(),
                "bbox_min_m": omp.min,
                "bbox_max_m": omp.max,
                "min_distance_m": dist,
                "min_distance_exact": exact,
                "bounds_overlap": overlap,
                // Interference VOLUME needs a mesh-mesh boolean, which
                // the kernel only exposes for oriented primitives — so
                // it is deliberately absent rather than approximated.
                "interference_volume_m3": serde_json::Value::Null,
            });
        }

        let mut summary = format!(
            "{} — volume {:.9} m³, area {:.6} m², {} tris",
            path.display(),
            volume,
            mp.surface_area,
            mp.triangles
        );
        if !watertight {
            summary.push_str(" (OPEN surface — volume not trustworthy)");
        }
        if let Some(m) = data.get("mass_kg").and_then(|v| v.as_f64()) {
            summary.push_str(&format!(", mass {m:.6} kg"));
        }
        if let Some(d) = data
            .get("against")
            .and_then(|a| a.get("min_distance_m"))
            .and_then(|v| v.as_f64())
        {
            summary.push_str(&format!(", min distance {d:.9} m"));
        }

        ok(TOOL, summary, data)
    }
}


// ════════════════════════════════════════════════════════════════════
// Authoring path — build a feature tree without hand-writing TOML.
//
// Every mutating tool here re-parses, re-evaluates and returns the new
// validity state in the SAME response, so a caller never has to follow
// a write with a read to find out whether the write was sound. That
// matters more in CAD than elsewhere: a feature can be accepted, alter
// the body, and still leave it defective, and the defect is not visible
// from the write's own return code.
//
// Writes go through `tree_to_toml` (serde round-trip), never string
// splicing, so anything this emits is by construction something
// `parse_tree` accepts.
// ════════════════════════════════════════════════════════════════════

/// How well the kernel actually supports a feature op today.
///
/// Deliberately finer-grained than "supported / blocked". The dangerous
/// case is the middle one: `Approximate` ops succeed, mutate the body,
/// and report OK — but the result is not what the name promises (a
/// mesh-crease fillet leaves BRep topology untouched, so `cad_measure`
/// reports the UNFILLETED volume). Silently returning "ok" there would
/// reproduce exactly the class of failure this tool surface exists to
/// eliminate, so the caveat rides along in the response.
enum OpSupport {
    Working,
    Approximate(&'static str),
    NotImplemented(&'static str),
}

fn op_support(op: &str) -> Option<OpSupport> {
    use OpSupport::*;
    Some(match op {
        "extrude" | "revolve" | "hole" | "mirror" | "pattern" | "boolean" | "split"
        | "sweep" | "plane" => Working,
        "fillet" => Approximate(
            "fillet is a post-tessellation mesh-crease soften, NOT a BRep fillet: the solid \
             topology is unchanged, so volume/area from cad_measure will NOT reflect it",
        ),
        "chamfer" => Approximate(
            "chamfer is a post-tessellation mesh-crease soften, NOT a BRep chamfer: the solid \
             topology is unchanged, so volume/area from cad_measure will NOT reflect it",
        ),
        "shell" => Approximate(
            "shell is open-top only (the inner cut protrudes through +Z); a fully enclosed \
             cavity needs offset surfaces the kernel does not have yet",
        ),
        "loft" => NotImplemented(
            "loft needs a multi-profile interpolation + guide-curve solver that truck-modeling \
             0.6 does not provide (it has homotopy for two surfaces only)",
        ),
        _ => return None,
    })
}

const KNOWN_OPS: &[&str] = &[
    "extrude", "revolve", "hole", "mirror", "pattern", "boolean", "split", "sweep",
    "fillet", "chamfer", "shell", "loft", "plane",
];

/// Enclosed volume of a tree's evaluated body, or `None` when it does
/// not evaluate. Used to measure what an edit actually did.
fn tree_volume(tree: &FeatureTree) -> Option<f64> {
    let out = eustress_cad::evaluate_tree(tree).ok()?;
    let mesh = out.mesh?;
    if mesh.indices.is_empty() {
        return None;
    }
    Some(eustress_cad::mass_properties(&mesh).volume())
}

/// Load + parse a tree for mutation, capturing the pre-edit volume so
/// the edit's actual effect can be measured rather than assumed.
fn load_tree_for_edit(
    ctx: &ToolContext,
    raw: &str,
    tool: &str,
) -> Result<(std::path::PathBuf, FeatureTree, Option<f64>), ToolResult> {
    let (path, src) = read_features(ctx, raw, tool)?;
    let tree = eustress_cad::parse_tree(&src)
        .map_err(|e| err(tool, format!("parse {}: {e}", path.display())))?;
    let before = tree_volume(&tree);
    Ok((path, tree, before))
}

/// Names of the entries in declaration order — the vocabulary a caller
/// must use for `sketch` / `features` / `target` references.
fn entry_names(tree: &FeatureTree) -> Vec<String> {
    tree.entries.iter().map(|e| entry_kind(e).1).collect()
}

/// Write the tree back, but only after proving it still serializes and
/// parses. The re-evaluation result is returned rather than gating the
/// write: refusing to write a tree that does not yet evaluate would
/// strand a caller mid-build (adding a sketch and the feature that uses
/// it is two edits, and the first one alone is legitimately incomplete).
/// The response says plainly when the result is broken, and why.
fn commit_tree(
    path: &std::path::Path,
    tree: &FeatureTree,
    tool: &str,
    verb: &str,
    before_volume: Option<f64>,
) -> Result<serde_json::Value, ToolResult> {
    let toml_src = eustress_cad::tree_to_toml(tree)
        .map_err(|e| err(tool, format!("{verb}: serialize failed: {e}")))?;
    let reparsed = eustress_cad::parse_tree(&toml_src).map_err(|e| {
        err(
            tool,
            format!("{verb}: result would not parse back ({e}); nothing written"),
        )
    })?;

    let (evaluates, eval_error, mesh, per_feature) = match eustress_cad::evaluate_tree(&reparsed) {
        Ok(out) => {
            let tol = reparsed
                .metadata
                .mesh_tolerance
                .unwrap_or(eustress_cad::DEFAULT_MESH_TOLERANCE);
            (
                true,
                None,
                mesh_json(out.mesh.as_ref(), tol),
                entry_status_json(&out),
            )
        }
        Err(e) => (false, Some(e.to_string()), serde_json::Value::Null, Vec::new()),
    };

    std::fs::write(path, toml_src)
        .map_err(|e| err(tool, format!("{verb}: write failed: {e}")))?;

    // Measure what the edit actually did to the solid.
    //
    // A feature can be accepted, report `ok`, pass every validity check
    // and still change NOTHING — the kernel's `finish_combine` swallows
    // a failed union (`boolean_or(...).unwrap_or_else(|| new_body)`) and
    // reports success, so a boolean that quietly failed is
    // indistinguishable from one that worked unless the body is
    // measured before and after. Surfacing the delta is what turns that
    // silent no-op into something a caller can act on.
    let after_volume = mesh
        .get("volume_m3")
        .and_then(|v| v.as_f64())
        .filter(|_| evaluates);
    let mut out = serde_json::json!({
        "evaluates": evaluates,
        "eval_error": eval_error,
        "feature_status": per_feature,
        "mesh": mesh,
        "volume_before_m3": before_volume,
        "volume_after_m3": after_volume,
    });
    if let (Some(b), Some(a)) = (before_volume, after_volume) {
        let delta = a - b;
        out["volume_delta_m3"] = delta.into();
        // Relative to the part, not absolute: 1e-9 m³ is nothing on a
        // plate and everything on a watch component.
        if b > 0.0 && (delta.abs() / b) < 1.0e-9 {
            out["no_geometric_effect"] = true.into();
            out["warning"] = format!(
                "{verb} changed the tree but the solid is unchanged (volume {a:.12} m³ \
                 before and after). The feature was accepted and reports ok, so this is \
                 most likely a boolean that failed and was swallowed, or a feature whose \
                 combine mode cannot express what you asked for."
            )
            .into();
        }
    }
    Ok(out)
}

/// Build a `FeatureEntry` from a caller-supplied op + arguments by
/// round-tripping through the kernel's own serde derive, so field names
/// and enum spellings are validated by the same code that will later
/// read the file back.
fn build_feature_entry(
    name: &str,
    op: &str,
    args: &serde_json::Map<String, serde_json::Value>,
    tool: &str,
) -> Result<eustress_cad::FeatureEntry, ToolResult> {
    let mut obj = serde_json::Map::new();
    obj.insert("kind".into(), serde_json::Value::String("feature".into()));
    obj.insert("name".into(), serde_json::Value::String(name.to_string()));
    obj.insert("op".into(), serde_json::Value::String(op.to_string()));
    for (k, v) in args {
        // These four are the tool's own envelope, not feature fields.
        if matches!(k.as_str(), "path" | "name" | "op" | "index") {
            continue;
        }
        obj.insert(k.clone(), v.clone());
    }
    serde_json::from_value::<eustress_cad::FeatureEntry>(serde_json::Value::Object(obj)).map_err(
        |e| {
            err(
                tool,
                format!(
                    "op '{op}': {e}. Supplied fields: {}. Call cad_list_templates for worked examples.",
                    args.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            )
        },
    )
}

fn auto_feature_name(tree: &FeatureTree, op: &str) -> String {
    let mut chars = op.chars();
    let base = match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Feature".to_string(),
    };
    let existing = entry_names(tree);
    let mut n = 1;
    loop {
        let candidate = format!("{base}{n}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Fold a `commit_tree` state object into a response payload.
fn merge_state(data: &mut serde_json::Value, state: serde_json::Value) {
    if let serde_json::Value::Object(m) = state {
        for (k, v) in m {
            data[k] = v;
        }
    }
}

fn tri_count(data: &serde_json::Value) -> u64 {
    data["mesh"]["triangle_count"].as_u64().unwrap_or(0)
}

// ── cad_list_templates ───────────────────────────────────────────────

pub struct CadListTemplatesTool;

impl ToolHandler for CadListTemplatesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_list_templates",
            description: "List the built-in CadPart templates with their driving variables, plus which feature ops the kernel supports and at what fidelity. Call this before cad_create_part or cad_add_feature instead of guessing template or op names.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "include_source": {
                        "type": "boolean",
                        "description": "Include each template's full features.toml as a worked example",
                        "default": false
                    }
                },
                "required": []
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_list_templates"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "cad_list_templates";
        let want_src = input
            .get("include_source")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut templates = Vec::new();
        for (name, src) in eustress_cad::templates::all() {
            let mut row = serde_json::json!({ "name": name });
            match eustress_cad::parse_tree(src) {
                Ok(tree) => {
                    let mut vars: Vec<serde_json::Value> = tree
                        .variables
                        .iter()
                        .map(|(k, v)| serde_json::json!({ "name": k, "value": v }))
                        .collect();
                    vars.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                    row["variables"] = serde_json::Value::Array(vars);
                    row["entries"] = serde_json::Value::Array(
                        tree.entries
                            .iter()
                            .enumerate()
                            .map(|(i, e)| {
                                let (kind, ename, op) = entry_kind(e);
                                serde_json::json!({
                                    "index": i, "kind": kind, "name": ename, "op": op
                                })
                            })
                            .collect(),
                    );
                }
                Err(e) => {
                    row["parse_error"] = e.to_string().into();
                }
            }
            if want_src {
                row["source"] = (*src).into();
            }
            templates.push(row);
        }

        // Op support, so a caller stops guessing which features are real.
        let ops: Vec<serde_json::Value> = KNOWN_OPS
            .iter()
            .map(|op| match op_support(op) {
                Some(OpSupport::Working) => serde_json::json!({ "op": op, "support": "working" }),
                Some(OpSupport::Approximate(note)) => {
                    serde_json::json!({ "op": op, "support": "approximate", "caveat": note })
                }
                Some(OpSupport::NotImplemented(note)) => {
                    serde_json::json!({ "op": op, "support": "not_implemented", "reason": note })
                }
                None => serde_json::json!({ "op": op, "support": "unknown" }),
            })
            .collect();

        let names: Vec<&str> = eustress_cad::templates::all().iter().map(|(n, _)| *n).collect();
        ok(
            TOOL,
            format!(
                "{} templates ({}); {} feature ops",
                templates.len(),
                names.join(", "),
                ops.len()
            ),
            serde_json::json!({
                "ok": true,
                "templates": templates,
                "feature_ops": ops,
                "note": "cad_create_part accepts plate | box | cylinder. Other templates are reachable by authoring their entries with cad_add_feature.",
            }),
        )
    }
}

// ── cad_add_feature ──────────────────────────────────────────────────

pub struct CadAddFeatureTool;

impl ToolHandler for CadAddFeatureTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_add_feature",
            description: "Append or insert a feature into a CadPart's tree. op = extrude | revolve | hole | mirror | pattern | boolean | split | sweep | fillet | chamfer | shell (loft is rejected: not implemented). Op-specific fields pass through to the kernel — extrude: sketch, depth, end_condition, combine, both_sides; hole: sketch_point, diameter, depth; pattern: pattern_kind, features, count, spacing/direction/axis/angle; boolean: target, boolean_op; mirror: plane, features; split: plane. Lengths and angles MUST be unit strings (\"20 mm\", \"90 deg\") or variable names. Returns the re-evaluated validity state so the edit's soundness is visible immediately.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "CadPart folder or features.toml, Space-relative" },
                    "op": { "type": "string", "description": "extrude | revolve | hole | mirror | pattern | boolean | split | sweep | fillet | chamfer | shell" },
                    "name": { "type": "string", "description": "Feature name; auto-generated (e.g. Extrude2) when omitted" },
                    "index": { "type": "integer", "description": "Insert position; appends when omitted" },
                    "sketch": { "type": "string", "description": "extrude/revolve: name of the sketch entry used as profile" },
                    "depth": { "type": "string", "description": "extrude/hole: unit string or variable name" },
                    "end_condition": { "type": "string", "description": "blind | through_all | to_plane | to_surface | mid_plane | up_to_next" },
                    "combine": { "type": "string", "description": "new_body | add | subtract | intersect" },
                    "both_sides": { "type": "boolean" },
                    "draft_angle": { "type": "string" },
                    "sketch_point": { "type": "string", "description": "hole: e.g. Sketch1/point-0" },
                    "diameter": { "type": "string", "description": "hole: unit string" },
                    "axis": { "type": "string" },
                    "angle": { "type": "string" },
                    "plane": { "type": "string", "description": "mirror/split: plane reference" },
                    "features": { "type": "array", "items": { "type": "string" }, "description": "mirror/pattern: feature names to operate on" },
                    "pattern_kind": { "type": "string", "description": "linear | circular | path | sketch" },
                    "count": { "type": "integer" },
                    "spacing": { "type": "string", "description": "pattern: unit string or variable" },
                    "direction": { "type": "array", "items": { "type": "number" }, "description": "pattern linear: [x,y,z]" },
                    "target": { "type": "string", "description": "boolean: name of the feature whose body is the operand" },
                    "boolean_op": { "type": "string", "description": "union | difference | intersect" },
                    "radius": { "type": "string", "description": "fillet" },
                    "distance": { "type": "string", "description": "chamfer" },
                    "edges": { "type": "array", "items": { "type": "string" } },
                    "wall_thickness": { "type": "string", "description": "shell" },
                    "open_faces": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["path", "op"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_add_feature"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "cad_add_feature";
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let op = input
            .get("op")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if op.is_empty() {
            return err(TOOL, format!("op required — one of: {}", KNOWN_OPS.join(", ")));
        }

        // Reject what the kernel cannot do BEFORE touching the file, so
        // an unsupported request costs nothing and says exactly why.
        let support = match op_support(&op) {
            Some(s) => s,
            None => {
                return err(
                    TOOL,
                    format!("unknown op '{op}' — expected one of: {}", KNOWN_OPS.join(", ")),
                )
            }
        };
        if let OpSupport::NotImplemented(reason) = support {
            return err(TOOL, format!("op '{op}' is not implemented: {reason}"));
        }

        let (path, mut tree, before_vol) = match load_tree_for_edit(ctx, path_s, TOOL) {
            Ok(v) => v,
            Err(e) => return e,
        };

        let name = match input.get("name").and_then(|v| v.as_str()) {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => auto_feature_name(&tree, &op),
        };
        if entry_names(&tree).contains(&name) {
            return err(TOOL, format!("an entry named '{name}' already exists in this tree"));
        }

        let args = input.as_object().cloned().unwrap_or_default();
        let entry = match build_feature_entry(&name, &op, &args, TOOL) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let at = match input.get("index").and_then(|v| v.as_u64()) {
            Some(i) => (i as usize).min(tree.entries.len()),
            None => tree.entries.len(),
        };
        tree.entries.insert(at, entry);

        let state = match commit_tree(&path, &tree, TOOL, "add_feature", before_vol) {
            Ok(s) => s,
            Err(e) => return e,
        };

        let mut data = serde_json::json!({
            "ok": true,
            "path": path.to_string_lossy(),
            "added": { "name": name, "op": op, "index": at },
            "entries": entry_names(&tree),
        });
        merge_state(&mut data, state);
        if let OpSupport::Approximate(caveat) = support {
            data["approximation"] = caveat.into();
        }

        let evaluates = data["evaluates"].as_bool().unwrap_or(false);
        let mut summary = if evaluates {
            format!(
                "Added {op} '{name}' at index {at} — tree evaluates, {} tris",
                tri_count(&data)
            )
        } else {
            format!(
                "Added {op} '{name}' at index {at} — WRITTEN BUT DOES NOT EVALUATE: {}",
                data["eval_error"].as_str().unwrap_or("unknown")
            )
        };
        if let OpSupport::Approximate(caveat) = support {
            summary.push_str(&format!("  [approximation: {caveat}]"));
        }
        ok(TOOL, summary, data)
    }
}

// ── cad_edit_feature ─────────────────────────────────────────────────

pub struct CadEditFeatureTool;

impl ToolHandler for CadEditFeatureTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_edit_feature",
            description: "Suppress, unsuppress, rename, or patch fields of the feature at a tree index. Suppression is how a bad tree gets bisected: suppress a feature, read the returned validity state, and see whether the defect disappears. Returns the re-evaluated state.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "CadPart folder or features.toml, Space-relative" },
                    "index": { "type": "integer", "description": "Tree index from cad_describe_part" },
                    "suppressed": { "type": "boolean", "description": "true suppresses (evaluator skips it), false restores" },
                    "rename": { "type": "string", "description": "New name for the entry" },
                    "set": {
                        "type": "object",
                        "description": "Field patch merged into the feature body, e.g. {\"depth\": \"25 mm\", \"combine\": \"subtract\"}. Lengths/angles must be unit strings or variable names.",
                        "additionalProperties": true
                    }
                },
                "required": ["path", "index"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_edit_feature"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "cad_edit_feature";
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let Some(index) = input.get("index").and_then(|v| v.as_u64()).map(|i| i as usize) else {
            return err(TOOL, "index required (see cad_describe_part)");
        };

        let (path, mut tree, before_vol) = match load_tree_for_edit(ctx, path_s, TOOL) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if index >= tree.entries.len() {
            return err(
                TOOL,
                format!("index {index} out of range — tree has {} entries", tree.entries.len()),
            );
        }

        // Round-trip the entry through TOML so suppress/restore and the
        // field patch operate on one uniform representation, and so a
        // restore is lossless — the kernel's `Suppressed` variant keeps
        // the body verbatim.
        let current = &tree.entries[index];
        let (cur_kind, cur_name, _) = entry_kind(current);
        let mut body = match toml::Value::try_from(current) {
            Ok(toml::Value::Table(t)) => t,
            Ok(other) => {
                return err(
                    TOOL,
                    format!("entry {index} serialized to {other:?}, expected a table"),
                )
            }
            Err(e) => return err(TOOL, format!("entry {index}: {e}")),
        };

        let mut actions: Vec<String> = Vec::new();

        if let Some(new_name) = input.get("rename").and_then(|v| v.as_str()) {
            let new_name = new_name.trim();
            if new_name.is_empty() {
                return err(TOOL, "rename must not be empty");
            }
            if entry_names(&tree)
                .iter()
                .enumerate()
                .any(|(i, n)| i != index && n == new_name)
            {
                return err(TOOL, format!("an entry named '{new_name}' already exists"));
            }
            body.insert("name".into(), toml::Value::String(new_name.to_string()));
            actions.push(format!("renamed to '{new_name}'"));
        }

        if let Some(serde_json::Value::Object(patch)) = input.get("set") {
            if cur_kind == "suppressed" {
                return err(
                    TOOL,
                    format!("entry {index} is suppressed — unsuppress it before patching fields"),
                );
            }
            for (k, v) in patch {
                let tv = match toml::Value::try_from(v) {
                    Ok(t) => t,
                    Err(e) => return err(TOOL, format!("field '{k}': {e}")),
                };
                body.insert(k.clone(), tv);
            }
            actions.push(format!(
                "set {}",
                patch.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }

        if let Some(want_suppressed) = input.get("suppressed").and_then(|v| v.as_bool()) {
            let is_suppressed = cur_kind == "suppressed";
            if want_suppressed && !is_suppressed {
                body.insert("kind".into(), toml::Value::String("suppressed".into()));
                actions.push("suppressed".into());
            } else if !want_suppressed && is_suppressed {
                // The preserved body still carries its `op`, so it goes
                // back to being a plain feature entry.
                let restored = if body.contains_key("op") { "feature" } else { "sketch" };
                body.insert("kind".into(), toml::Value::String(restored.into()));
                actions.push("unsuppressed".into());
            }
        }

        if actions.is_empty() {
            return err(TOOL, "nothing to do — pass suppressed, rename, and/or set");
        }

        let new_entry: eustress_cad::FeatureEntry = match toml::Value::Table(body).try_into() {
            Ok(e) => e,
            Err(e) => {
                return err(
                    TOOL,
                    format!("edit to entry {index} ('{cur_name}') is not a valid feature: {e}"),
                )
            }
        };
        tree.entries[index] = new_entry;

        let state = match commit_tree(&path, &tree, TOOL, "edit_feature", before_vol) {
            Ok(s) => s,
            Err(e) => return e,
        };

        let mut data = serde_json::json!({
            "ok": true,
            "path": path.to_string_lossy(),
            "index": index,
            "actions": actions,
            "entries": entry_names(&tree),
        });
        merge_state(&mut data, state);

        let evaluates = data["evaluates"].as_bool().unwrap_or(false);
        ok(
            TOOL,
            if evaluates {
                format!(
                    "entry {index} ('{cur_name}'): {} — tree evaluates, {} tris",
                    actions.join(", "),
                    tri_count(&data)
                )
            } else {
                format!(
                    "entry {index} ('{cur_name}'): {} — WRITTEN BUT DOES NOT EVALUATE: {}",
                    actions.join(", "),
                    data["eval_error"].as_str().unwrap_or("unknown")
                )
            },
            data,
        )
    }
}

// ── cad_delete_feature ───────────────────────────────────────────────

pub struct CadDeleteFeatureTool;

impl ToolHandler for CadDeleteFeatureTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cad_delete_feature",
            description: "Remove the feature at a tree index. Reports which downstream entries referenced it by name and are therefore invalidated. Deleting a referenced entry is refused unless force=true, because the resulting tree evaluates to a body that silently omits the dependents.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "CadPart folder or features.toml, Space-relative" },
                    "index": { "type": "integer", "description": "Tree index from cad_describe_part" },
                    "force": {
                        "type": "boolean",
                        "description": "Delete even when other entries reference it by name",
                        "default": false
                    }
                },
                "required": ["path", "index"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.cad_delete_feature"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "cad_delete_feature";
        let path_s = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let Some(index) = input.get("index").and_then(|v| v.as_u64()).map(|i| i as usize) else {
            return err(TOOL, "index required (see cad_describe_part)");
        };
        let force = input.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

        let (path, mut tree, before_vol) = match load_tree_for_edit(ctx, path_s, TOOL) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if index >= tree.entries.len() {
            return err(
                TOOL,
                format!("index {index} out of range — tree has {} entries", tree.entries.len()),
            );
        }

        let (_, victim_name, victim_op) = entry_kind(&tree.entries[index]);

        // Dependency scan: references are by NAME (sketch = "Sketch1",
        // features = ["Extrude1"], target = "…") and can be qualified
        // ("Sketch1/point-0"), so match the quoted name and any
        // "<name>/…" prefix anywhere in the serialized entry rather than
        // guessing which field carries the reference for each op.
        let mut dependents: Vec<serde_json::Value> = Vec::new();
        for (i, e) in tree.entries.iter().enumerate() {
            if i == index {
                continue;
            }
            let Ok(v) = toml::Value::try_from(e) else { continue };
            let text = v.to_string();
            if text.contains(&format!("\"{victim_name}\""))
                || text.contains(&format!("\"{victim_name}/"))
            {
                let (k, n, o) = entry_kind(e);
                dependents.push(serde_json::json!({
                    "index": i, "kind": k, "name": n, "op": o
                }));
            }
        }

        if !dependents.is_empty() && !force {
            let names: Vec<String> = dependents
                .iter()
                .map(|d| format!("[{}] {}", d["index"], d["name"].as_str().unwrap_or("?")))
                .collect();
            return err(
                TOOL,
                format!(
                    "entry {index} ('{victim_name}') is referenced by {}. Deleting it would \
                     invalidate them. Re-run with force=true to delete anyway, or repoint/delete \
                     the dependents first.",
                    names.join(", ")
                ),
            );
        }

        tree.entries.remove(index);

        let state = match commit_tree(&path, &tree, TOOL, "delete_feature", before_vol) {
            Ok(s) => s,
            Err(e) => return e,
        };

        let mut data = serde_json::json!({
            "ok": true,
            "path": path.to_string_lossy(),
            "deleted": { "index": index, "name": victim_name, "op": victim_op },
            "invalidated_dependents": dependents,
            "forced": force,
            "entries": entry_names(&tree),
        });
        merge_state(&mut data, state);

        let n_dep = data["invalidated_dependents"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        let evaluates = data["evaluates"].as_bool().unwrap_or(false);
        ok(
            TOOL,
            if evaluates {
                format!(
                    "Deleted entry {index} ('{victim_name}') — tree evaluates, {} tris{}",
                    tri_count(&data),
                    if n_dep == 0 {
                        String::new()
                    } else {
                        format!(", {n_dep} dependent(s) invalidated")
                    }
                )
            } else {
                format!(
                    "Deleted entry {index} ('{victim_name}') — DOES NOT EVALUATE: {}",
                    data["eval_error"].as_str().unwrap_or("unknown")
                )
            },
            data,
        )
    }
}
