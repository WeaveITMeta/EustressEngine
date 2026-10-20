//! `particle_simulation`: author and inspect `ParticleSimulation` instances
//! from an agent. Works on the Space's files (the engine's file watcher
//! hot-reloads every edit), validated by the same field tables the
//! Properties panel uses, so a property an agent sets is exactly one a
//! person could set.
//!
//! Live control of a running simulation (reset, pause, retuning a field
//! mid-run without saving) goes through sim values instead:
//! `set_sim_value psim.<Simulation>.<Property>` and `get_sim_value
//! psim.<Simulation>.<Stat>`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use eustress_common::realism::particle_sim::class::{self as class, FieldKind, FieldSpec, FieldTable};
use eustress_common::realism::particle_sim::{ParticleSimulation, ParticleSpecies, SIMULATION_STATS, SPECIES_STATS};

use crate::modes::WorkshopMode;
use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};

const TOOL: &str = "particle_simulation";

pub struct ParticleSimulationTool;

fn result(success: bool, content: String, data: Option<Value>) -> ToolResult {
    ToolResult {
        tool_name: TOOL.to_string(),
        tool_use_id: String::new(),
        success,
        content,
        structured_data: data,
        stream_topic: success.then(|| format!("workshop.tool.{TOOL}")),
    }
}

fn kind_json(kind: FieldKind) -> Value {
    match kind {
        FieldKind::Bool => json!("bool"),
        FieldKind::Int => json!("int"),
        FieldKind::Float => json!("float"),
        FieldKind::Vector3 => json!("vec3"),
        FieldKind::Color3 => json!("color (r, g, b 0-255)"),
        FieldKind::Choice(options) => json!({ "choice": options }),
        FieldKind::Text => json!("text"),
    }
}

fn fields_json(fields: &[FieldSpec], defaults: &dyn Fn(&str) -> Option<String>) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|f| {
                json!({
                    "name": f.name,
                    "type": kind_json(f.kind),
                    "category": f.category,
                    "unit": f.unit,
                    "default": defaults(f.name),
                    "restarts_run": f.restarts,
                    "description": f.description,
                })
            })
            .collect(),
    )
}

/// A JSON value as the text the field parsers take.
fn json_to_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::Number(n) => Some(n.to_string()),
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect::<Option<Vec<String>>>()
            .map(|p| p.join(", ")),
        _ => None,
    }
}

fn apply_properties<T: FieldTable>(component: &mut T, props: Option<&serde_json::Map<String, Value>>) -> Result<Vec<String>, String> {
    let mut set = Vec::new();
    for (key, value) in props.into_iter().flatten() {
        if key.eq_ignore_ascii_case("name") {
            continue;
        }
        let text = json_to_text(value).ok_or_else(|| format!("{key}: unsupported value {value}"))?;
        let spec = component.set_text(key, &text).map_err(|e| format!("{key}: {e}"))?;
        set.push(format!("{} = {}", spec.name, component.text(spec.name).unwrap_or_default()));
    }
    Ok(set)
}

fn read_doc(path: &Path) -> Result<toml::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    text.parse().map_err(|e| format!("parse {}: {e}", path.display()))
}

fn load<T: FieldTable + Default>(doc: &toml::Value) -> T {
    class::from_section::<T>(doc.get(T::SECTION).and_then(|v| v.as_table())).0
}

fn write_section<T: FieldTable>(path: &Path, component: &T) -> Result<(), String> {
    let mut doc = read_doc(path)?;
    let root = doc.as_table_mut().ok_or_else(|| format!("{} is not a TOML table", path.display()))?;
    root.insert(T::SECTION.to_string(), toml::Value::Table(component.to_toml_table()));
    let out = toml::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))
}

/// `_instance.toml` of an instance addressed relative to the Space
/// ("Workspace/Tank/Water") or to Workspace ("Tank/Water").
fn resolve(ctx: &ToolContext, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim().trim_matches('/');
    if rel.is_empty() || rel.contains("..") {
        return Err(format!("invalid instance path '{rel}'"));
    }
    for base in [ctx.space_root.clone(), ctx.space_root.join("Workspace")] {
        let candidate = base.join(rel).join("_instance.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("no instance at '{rel}' (looked in the Space root and Workspace)"))
}

fn class_of(doc: &toml::Value) -> String {
    doc.get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

fn describe() -> ToolResult {
    let sim = ParticleSimulation::default();
    let sp = ParticleSpecies::default();
    let data = json!({
        "ParticleSimulation": fields_json(class::SIMULATION_FIELDS, &|n| sim.text(n)),
        "ParticleSpecies": fields_json(class::SPECIES_FIELDS, &|n| sp.text(n)),
        "simulation_stats": SIMULATION_STATS.iter().map(|s| json!({"name": s.name, "unit": s.unit, "description": s.description})).collect::<Vec<_>>(),
        "species_stats": SPECIES_STATS.iter().map(|s| json!({"name": s.name, "unit": s.unit, "description": s.description})).collect::<Vec<_>>(),
        "live_control": {
            "read": "get_sim_value psim.<Simulation>.<Stat> or psim.<Simulation>.<Species>.<Stat>",
            "write": "set_sim_value psim.<Simulation>.<Property> (bool/int/float; vectors as .<Property>.x|y|z), applied live and reverted on Stop",
            "reset": "set_sim_value psim.<Simulation>.Reset 1",
        },
        "presets": {
            "fluid_tank": {"simulation": {}, "species": [{"name": "Water"}]},
            "copper_wire": {
                "simulation": {"DomainSize": [2e-8, 2e-8, 2e-8], "DisplayScale": 1e8, "TimeScale": 1e-13,
                    "BoundaryX": "Periodic", "BoundaryY": "Periodic", "BoundaryZ": "Periodic",
                    "Gravity": [0, 0, 0], "ElectricField": [1e7, 0, 0], "ColorMode": "Speed"},
                "species": [{"name": "Electrons", "Particle": "Electron", "Conductor": "Copper", "Count": 20000,
                    "Arrangement": "Random", "RegionMax": [1, 1, 1]}],
            },
        },
    });
    result(
        true,
        format!(
            "ParticleSimulation has {} properties, ParticleSpecies {}; see structured data for names, types, units and defaults.",
            class::SIMULATION_FIELDS.len(),
            class::SPECIES_FIELDS.len()
        ),
        Some(data),
    )
}

fn create(input: &Value, ctx: &ToolContext) -> Result<ToolResult, String> {
    let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("ParticleSimulation");
    let parent = input.get("parent").and_then(|v| v.as_str()).unwrap_or("").trim().trim_matches('/');
    if parent.contains("..") {
        return Err("invalid parent path".into());
    }
    let base = if parent.is_empty() { ctx.space_root.join("Workspace") } else { ctx.space_root.join("Workspace").join(parent) };
    let position = input.get("position").and_then(|v| v.as_array()).and_then(|a| {
        let f: Vec<f32> = a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
        (f.len() == 3).then(|| [f[0], f[1], f[2]])
    });

    // Validate everything before writing anything.
    let mut sim = ParticleSimulation::default();
    let sim_set = apply_properties(&mut sim, input.get("simulation").and_then(|v| v.as_object()))?;
    let species_in: Vec<Value> = match input.get("species") {
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err("species must be an array of objects".into()),
        None => vec![json!({ "name": "Water" })],
    };
    let mut species = Vec::new();
    for s in &species_in {
        let obj = s.as_object().ok_or("each species must be an object")?;
        let mut sp = ParticleSpecies::default();
        let set = apply_properties(&mut sp, Some(obj))?;
        let sp_name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("Species").to_string();
        species.push((sp_name, sp, set));
    }

    let overrides = eustress_common::instance_create::InstanceOverrides {
        position,
        ..Default::default()
    };
    let created = eustress_common::instance_create::create_instance(&base, "ParticleSimulation", Some(name), overrides)
        .map_err(|e| format!("create ParticleSimulation: {e}"))?;
    write_section(&created.toml_path, &sim)?;
    let mut species_out = Vec::new();
    for (sp_name, sp, set) in &species {
        let c = eustress_common::instance_create::create_instance(
            &created.folder_path,
            "ParticleSpecies",
            Some(sp_name.as_str()),
            Default::default(),
        )
        .map_err(|e| format!("create ParticleSpecies {sp_name}: {e}"))?;
        write_section(&c.toml_path, sp)?;
        species_out.push(json!({ "name": c.folder_name, "file": c.toml_path.to_string_lossy(), "set": set }));
    }
    Ok(result(
        true,
        format!(
            "Created ParticleSimulation '{}' with {} species ({}).",
            created.folder_name,
            species_out.len(),
            if sim_set.is_empty() { "default properties".to_string() } else { sim_set.join(", ") }
        ),
        Some(json!({ "name": created.folder_name, "file": created.toml_path.to_string_lossy(), "set": sim_set, "species": species_out })),
    ))
}

fn set(input: &Value, ctx: &ToolContext) -> Result<ToolResult, String> {
    let rel = input.get("path").and_then(|v| v.as_str()).ok_or("path is required")?;
    let path = resolve(ctx, rel)?;
    let doc = read_doc(&path)?;
    let props = input.get("properties").and_then(|v| v.as_object());
    let set = match class_of(&doc).as_str() {
        "ParticleSimulation" => {
            let mut c: ParticleSimulation = load(&doc);
            let set = apply_properties(&mut c, props)?;
            write_section(&path, &c)?;
            set
        }
        "ParticleSpecies" => {
            let mut c: ParticleSpecies = load(&doc);
            let set = apply_properties(&mut c, props)?;
            write_section(&path, &c)?;
            set
        }
        other => return Err(format!("'{rel}' is a {other}, not a ParticleSimulation or ParticleSpecies")),
    };
    Ok(result(true, format!("{rel}: {}", set.join(", ")), Some(json!({ "file": path.to_string_lossy(), "set": set }))))
}

fn get(input: &Value, ctx: &ToolContext) -> Result<ToolResult, String> {
    let rel = input.get("path").and_then(|v| v.as_str()).ok_or("path is required")?;
    let path = resolve(ctx, rel)?;
    let doc = read_doc(&path)?;
    let class_name = class_of(&doc);
    let props: serde_json::Map<String, Value> = match class_name.as_str() {
        "ParticleSimulation" => {
            let c: ParticleSimulation = load(&doc);
            class::SIMULATION_FIELDS.iter().map(|f| (f.name.to_string(), json!(c.text(f.name)))).collect()
        }
        "ParticleSpecies" => {
            let c: ParticleSpecies = load(&doc);
            class::SPECIES_FIELDS.iter().map(|f| (f.name.to_string(), json!(c.text(f.name)))).collect()
        }
        other => return Err(format!("'{rel}' is a {other}, not a ParticleSimulation or ParticleSpecies")),
    };
    Ok(result(true, format!("{rel} ({class_name}): {} properties", props.len()), Some(json!({ "class": class_name, "properties": props }))))
}

impl ToolHandler for ParticleSimulationTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: TOOL,
            description: "Author particle simulations (SPH fluids, electrons and ions in E/B fields, Drude conduction with Joule heating, plasma). \
                action=describe lists every property with type, unit, default and presets; create makes a ParticleSimulation with species; \
                set changes properties of a simulation or species (by path like 'Tank' or 'Tank/Water'); get reads them. \
                Files are hot-reloaded by the engine. For live control of a running simulation use set_sim_value/get_sim_value on psim.<Name>.<Property|Stat>.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["describe", "create", "set", "get"], "default": "describe" },
                    "name": { "type": "string", "description": "create: simulation name" },
                    "parent": { "type": "string", "description": "create: folder under Workspace to create in" },
                    "position": { "type": "array", "items": { "type": "number" }, "description": "create: world position [x, y, z] (m)" },
                    "simulation": { "type": "object", "description": "create: ParticleSimulation properties by name, e.g. {\"DomainSize\": [0.5, 0.5, 0.5], \"ColorMode\": \"Speed\"}" },
                    "species": { "type": "array", "items": { "type": "object" }, "description": "create: species objects with a name plus ParticleSpecies properties; omitted = one Water species" },
                    "path": { "type": "string", "description": "set/get: instance path relative to Workspace (or the Space)" },
                    "properties": { "type": "object", "description": "set: properties by name, values as numbers, booleans, [x, y, z] arrays or names" }
                }
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.particle_simulation"],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("describe");
        let outcome = match action {
            "describe" => Ok(describe()),
            "create" => create(&input, ctx),
            "set" => set(&input, ctx),
            "get" => get(&input, ctx),
            other => Err(format!("unknown action '{other}' (describe, create, set, get)")),
        };
        outcome.unwrap_or_else(|e| result(false, e, None))
    }
}
