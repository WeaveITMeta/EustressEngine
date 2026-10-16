//! An instance's properties for a caller that is not the Properties panel (the
//! bridge's `properties.*` methods, so MCP agents): read and written through
//! the same functions the panel's rows and edits use, family by family.
//!
//! - **Field-table classes** (particle simulations and species, the terrain
//!   layers, and any class built on `FieldTable`): rows from the class's
//!   field table, edits through its own text parser and section save, undo
//!   as `ChangeClassField`, exactly what a panel edit records.
//! - **Lights, the sky classes (Sun, Moon, Sky, Atmosphere, Clouds, colour
//!   effects) and Sound**: the panels' own row builders and setters.
//! - **Everything else** (Part, Model, Humanoid, Camera, Beam, ...): the
//!   classes' `PropertyAccess` tables, written through `PropertyCommand`, the
//!   path a script's `Instance:Set` takes, with undo for the names the undo
//!   system replays.
//! - **Position, Rotation and Size**: the Move and Scale tools' own path
//!   (`Transform` and `BasePart` together, `ResizePartEvent` for a part's
//!   size, `TransformEntities` / `ScaleEntities` undo, the instance record
//!   saved as a drag saves it), so a terrain layer re-bakes and a road
//!   follows its points.
//!
//! Values are the panel's text form with lengths in metres, the stored form,
//! never the person's display unit. A change set is all or nothing: every
//! value is checked first, and if applying one still fails, the ones before
//! it are put back. It is one undo step.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use eustress_common::classes::{BasePart, ClassName, Instance, PropertyAccess, PropertyValue};
use eustress_common::realism::particle_sim::class::{field, format_value, parse_value, FieldKind, FieldTable};
use eustress_common::realism::particle_sim::{ParticleSimulation, ParticleSpecies};
use eustress_common::terrain::layer_instances::{
    TerrainFlattenPad, TerrainMaterialFill, TerrainNoise, TerrainScatter, TerrainSpline, TerrainSplinePoint,
    TerrainStamp, TerrainWaterBody,
};
use eustress_common::units::ENGINE_NATIVE_UNIT;

use crate::commands::PropertyCommand;
use crate::space::instance_loader::InstanceFile;
use crate::undo::Action;

/// One property, as a caller sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub category: String,
    /// bool, int, float, vec3, color, choice, string, rotation, or a
    /// `PropertyAccess` type name.
    pub kind: String,
    /// The text form, lengths in metres.
    pub value: String,
    pub editable: bool,
    /// SI unit, empty when dimensionless or unknown.
    pub unit: String,
    pub description: String,
    pub choices: Vec<String>,
}

impl Row {
    fn new(category: &str, name: &str, kind: &str, value: String, editable: bool) -> Self {
        Row {
            name: name.to_string(),
            category: category.to_string(),
            kind: kind.to_string(),
            value,
            editable,
            unit: String::new(),
            description: String::new(),
            choices: Vec::new(),
        }
    }

    pub fn json(&self) -> Value {
        let mut v = json!({
            "name": self.name,
            "category": self.category,
            "type": self.kind,
            "value": self.value,
            "editable": self.editable,
        });
        if !self.unit.is_empty() {
            v["unit"] = json!(self.unit);
        }
        if !self.description.is_empty() {
            v["description"] = json!(self.description);
        }
        if !self.choices.is_empty() {
            v["choices"] = json!(self.choices);
        }
        v
    }
}

fn fmt(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

fn vec3_text(v: [f32; 3]) -> String {
    format!("{}, {}, {}", fmt(v[0]), fmt(v[1]), fmt(v[2]))
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Every property of `entity` this service can read, in the panel's
/// categories. `Err` when it is not an instance.
pub fn describe(world: &mut World, entity: Entity) -> Result<Vec<Row>, String> {
    let instance = world.get::<Instance>(entity).cloned().ok_or("not an instance")?;
    let class = instance.class_name;
    let mut rows = vec![
        Row {
            description: "Read-only here: rename it in the Explorer.".to_string(),
            ..Row::new("Metadata", "Name", "string", instance.name.clone(), false)
        },
        Row::new("Metadata", "ClassName", "string", class.as_str().to_string(), false),
    ];
    rows.extend(transform_rows(world, entity));

    let family = match field_table_rows(world, entity) {
        Some(rows) => Some(rows),
        None => panel_rows(world, class, entity),
    };
    match family {
        Some(family) => rows.extend(family),
        None => rows.extend(generic_rows(world, entity, &rows)),
    }
    Ok(rows)
}

/// Position and Rotation for anything with a pose, and Size for a part.
fn transform_rows(world: &World, entity: Entity) -> Vec<Row> {
    let Some(t) = world.get::<Transform>(entity) else { return Vec::new() };
    let (rx, ry, rz) = t.rotation.to_euler(EulerRot::XYZ);
    let mut rows = vec![
        Row { unit: "m".into(), description: "Where it sits, in metres.".into(), ..Row::new("Transform", "Position", "vec3", vec3_text(t.translation.to_array()), true) },
        Row {
            unit: "deg".into(),
            description: "Euler XYZ, in degrees.".into(),
            ..Row::new("Transform", "Rotation", "vec3", vec3_text([rx.to_degrees(), ry.to_degrees(), rz.to_degrees()]), true)
        },
    ];
    if let Some(bp) = world.get::<BasePart>(entity) {
        rows.push(Row { unit: "m".into(), description: "The part's size, in metres.".into(), ..Row::new("Transform", "Size", "vec3", vec3_text(bp.size.to_array()), true) });
    }
    rows
}

fn kind_name(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Bool => "bool",
        FieldKind::Int => "int",
        FieldKind::Float => "float",
        FieldKind::Vector3 => "vec3",
        FieldKind::Color3 => "color",
        FieldKind::Choice(_) => "choice",
        FieldKind::Text => "string",
    }
}

fn table_rows<T: FieldTable>(component: &T) -> Vec<Row> {
    T::FIELDS
        .iter()
        .filter_map(|spec| {
            let value = component.get_field(spec.name)?;
            Some(Row {
                unit: spec.unit.to_string(),
                description: spec.description.to_string(),
                choices: match spec.kind {
                    FieldKind::Choice(options) => options.iter().map(|o| o.to_string()).collect(),
                    _ => Vec::new(),
                },
                ..Row::new(spec.category, spec.name, kind_name(spec.kind), format_value(&value), true)
            })
        })
        .collect()
}

macro_rules! each_field_table {
    ($world:expr, $entity:expr, $f:ident) => {{
        let w: &World = $world;
        None.or_else(|| w.get::<ParticleSimulation>($entity).map($f))
            .or_else(|| w.get::<ParticleSpecies>($entity).map($f))
            .or_else(|| w.get::<TerrainSpline>($entity).map($f))
            .or_else(|| w.get::<TerrainSplinePoint>($entity).map($f))
            .or_else(|| w.get::<TerrainStamp>($entity).map($f))
            .or_else(|| w.get::<TerrainFlattenPad>($entity).map($f))
            .or_else(|| w.get::<TerrainNoise>($entity).map($f))
            .or_else(|| w.get::<TerrainMaterialFill>($entity).map($f))
            .or_else(|| w.get::<TerrainScatter>($entity).map($f))
            .or_else(|| w.get::<TerrainWaterBody>($entity).map($f))
    }};
}

fn field_table_rows(world: &World, entity: Entity) -> Option<Vec<Row>> {
    each_field_table!(world, entity, table_rows)
}

fn rows_of(rows: Option<Vec<(&'static str, &'static str, String, &'static str)>>) -> Option<Vec<Row>> {
    rows.map(|rows| rows.into_iter().map(|(category, name, value, kind)| Row::new(category, name, kind, value, true)).collect())
}

/// A light's, sky class's or Sound's rows, from its panel's own row builder,
/// with lengths in metres. `None` for any other class.
fn panel_rows(world: &mut World, class: ClassName, entity: Entity) -> Option<Vec<Row>> {
    let light = {
        let mut state = SystemState::<crate::ui::light_panel::PanelQueries<'static, 'static>>::new(world);
        let Ok(q) = state.get(world) else { return None };
        rows_of(crate::ui::light_panel::light_rows(class, entity, ENGINE_NATIVE_UNIT, &q))
    };
    if light.is_some() {
        return light;
    }
    let celestial = {
        let mut state = SystemState::<crate::ui::celestial_panel::PanelQueries<'static, 'static>>::new(world);
        let Ok(q) = state.get(world) else { return None };
        rows_of(crate::ui::celestial_panel::celestial_rows(class, entity, ENGINE_NATIVE_UNIT, &q))
    };
    if celestial.is_some() {
        return celestial;
    }
    let mut state = SystemState::<crate::ui::sound_panel::PanelQueries<'static, 'static>>::new(world);
    let Ok(q) = state.get(world) else { return None };
    rows_of(crate::ui::sound_panel::sound_rows(class, entity, ENGINE_NATIVE_UNIT, &q))
}

/// Every `PropertyAccess` component the generic path reads and writes, in
/// `PropertyCommand`'s order.
macro_rules! each_property_component {
    ($m:ident) => {{
        use eustress_common::classes::*;
        $m!(Instance);
        $m!(BasePart);
        $m!(Part);
        $m!(Model);
        $m!(Humanoid);
        $m!(EustressCamera);
        $m!(Attachment);
        $m!(WeldConstraint);
        $m!(Motor6D);
        $m!(ParticleEmitter);
        $m!(Beam);
        $m!(SpecialMesh);
        $m!(Decal);
        $m!(Animator);
        $m!(KeyframeSequence);
        $m!(Terrain);
        $m!(UnionOperation);
        $m!(BillboardGui);
        $m!(TextLabel);
        $m!(Folder);
    }};
}

/// Names this service handles itself, so the generic rows never repeat them.
const OWN_NAMES: [&str; 6] = ["Name", "ClassName", "Position", "Rotation", "Orientation", "Size"];

fn generic_rows(world: &World, entity: Entity, already: &[Row]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    macro_rules! read {
        ($ty:ty) => {
            if let Some(c) = world.get::<$ty>(entity) {
                for d in c.list_properties() {
                    if OWN_NAMES.contains(&d.name.as_str())
                        || already.iter().chain(rows.iter()).any(|r| r.name == d.name)
                    {
                        continue;
                    }
                    let Some(value) = c.get_property(&d.name) else { continue };
                    let Some(text) = value_text(&value) else { continue };
                    let category = if d.category.is_empty() { "Properties".to_string() } else { d.category.clone() };
                    rows.push(Row::new(&category, &d.name, &d.property_type, text, !d.read_only));
                }
            }
        };
    }
    each_property_component!(read);
    rows
}

/// A `PropertyValue` in its text form, lengths in metres. `None` for a whole
/// `Transform`, which is read through Position and Rotation instead.
pub fn value_text(value: &PropertyValue) -> Option<String> {
    Some(match value {
        PropertyValue::String(s) | PropertyValue::Enum(s) => s.clone(),
        PropertyValue::Float(f) => fmt(*f),
        PropertyValue::Int(i) => i.to_string(),
        PropertyValue::Bool(b) => b.to_string(),
        PropertyValue::Vector2(v) => format!("{}, {}", fmt(v[0]), fmt(v[1])),
        PropertyValue::Vector3(v) => vec3_text(v.to_array()),
        PropertyValue::UDim2(u) => format!("{}, {}, {}, {}", fmt(u.x.scale), fmt(u.x.offset), fmt(u.y.scale), fmt(u.y.offset)),
        PropertyValue::Color(c) => {
            let s = c.to_srgba();
            format!("{}, {}, {}, {}", fmt(s.red), fmt(s.green), fmt(s.blue), fmt(s.alpha))
        }
        PropertyValue::Color3(c) => vec3_text(*c),
        PropertyValue::Material(m) => format!("{m:?}"),
        PropertyValue::Transform(_) => return None,
    })
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// A JSON value in the panel's text form: a string as it is, a number or
/// bool as written, an array of numbers as "a, b, c".
pub fn json_text(value: &Value) -> Result<String, String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Array(items) => items
            .iter()
            .map(|v| match v {
                Value::Number(n) => Ok(n.to_string()),
                other => Err(format!("{other} is not a number")),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join(", ")),
        other => Err(format!("{other} has no text form; give a string, number, bool or array of numbers")),
    }
}

fn numbers(text: &str, n: usize) -> Result<Vec<f32>, String> {
    let parts: Vec<f32> = text
        .split(',')
        .map(|s| s.trim().parse::<f32>().map_err(|_| format!("{:?} is not a number", s.trim())))
        .collect::<Result<_, _>>()?;
    if parts.len() != n || parts.iter().any(|v| !v.is_finite()) {
        return Err(format!("expected {n} finite numbers separated by commas, got {text:?}"));
    }
    Ok(parts)
}

/// `text` as a value of the same shape as `current`.
fn parse_like(current: &PropertyValue, text: &str) -> Result<PropertyValue, String> {
    let t = text.trim();
    Ok(match current {
        PropertyValue::String(_) => PropertyValue::String(text.to_string()),
        PropertyValue::Enum(_) => PropertyValue::Enum(t.to_string()),
        PropertyValue::Float(_) => PropertyValue::Float(numbers(t, 1)?[0]),
        PropertyValue::Int(_) => PropertyValue::Int(t.parse().map_err(|_| format!("{t:?} is not a whole number"))?),
        PropertyValue::Bool(_) => PropertyValue::Bool(match t {
            "true" | "True" => true,
            "false" | "False" => false,
            _ => return Err(format!("{t:?} is not true or false")),
        }),
        PropertyValue::Vector2(_) => {
            let v = numbers(t, 2)?;
            PropertyValue::Vector2([v[0], v[1]])
        }
        PropertyValue::Vector3(_) => {
            let v = numbers(t, 3)?;
            PropertyValue::Vector3(Vec3::new(v[0], v[1], v[2]))
        }
        PropertyValue::UDim2(_) => {
            let v = numbers(t, 4)?;
            PropertyValue::UDim2(eustress_common::ui_types::UDim2::new(v[0], v[1], v[2], v[3]))
        }
        PropertyValue::Color(_) | PropertyValue::Color3(_) => {
            let parts = t.split(',').count();
            let v = numbers(t, if parts == 4 { 4 } else { 3 })?;
            // 0-255 channels when any exceeds 1, as the panel accepts both.
            let scale = if v.iter().any(|c| *c > 1.0) { 255.0 } else { 1.0 };
            let c: Vec<f32> = v.iter().map(|c| (c / scale).clamp(0.0, 1.0)).collect();
            match current {
                PropertyValue::Color3(_) => PropertyValue::Color3([c[0], c[1], c[2]]),
                _ => PropertyValue::Color(Color::srgba(c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0))),
            }
        }
        PropertyValue::Material(_) => {
            return Err("this Material is set by name through a part's `Material` property".to_string())
        }
        PropertyValue::Transform(_) => return Err("set Position and Rotation instead".to_string()),
    })
}

/// Which way one change is applied.
enum Plan {
    /// A field-table class's field.
    Table { file: PathBuf, text: String },
    /// A light's, sky class's or Sound's field, through its panel's setter.
    Panel { file: PathBuf, text: String },
    Position(Vec3),
    Rotation(Quat),
    Size(Vec3),
    Generic(PropertyValue),
}

struct Change {
    entity: Entity,
    name: String,
    plan: Plan,
}

/// What was done, so it can be put back if a later change fails.
enum Done {
    Text { entity: Entity, file: PathBuf, name: String, old: String, table: bool },
    Pose { entity: Entity, translation: Vec3, rotation: Quat },
    Size { entity: Entity, translation: Vec3, old: Vec3, new: Vec3 },
    Generic { entity: Entity, name: String, old: PropertyValue, new: PropertyValue },
}

fn record_file(world: &World, entity: Entity) -> Option<PathBuf> {
    if let Some(f) = world.get::<InstanceFile>(entity) {
        return Some(f.toml_path.clone());
    }
    crate::ui::sound_panel::sound_file(None, world.get::<crate::space::file_loader::LoadedFromFile>(entity))
}

/// The stored text of a field-table field.
fn table_text(world: &World, entity: Entity, name: &str) -> Option<String> {
    fn text_of<T: FieldTable>(c: &T, name: &str) -> Option<String> {
        field(T::FIELDS, name).and_then(|spec| c.text(spec.name))
    }
    None.or_else(|| world.get::<ParticleSimulation>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<ParticleSpecies>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainSpline>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainSplinePoint>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainStamp>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainFlattenPad>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainNoise>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainMaterialFill>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainScatter>(entity).and_then(|c| text_of(c, name)))
        .or_else(|| world.get::<TerrainWaterBody>(entity).and_then(|c| text_of(c, name)))
}

/// Check one change and decide how it is applied, changing nothing.
fn plan(world: &mut World, entity: Entity, name: &str, value: &Value, panel: &[Row]) -> Result<Change, String> {
    let class = world.get::<Instance>(entity).map(|i| i.class_name.as_str().to_string()).unwrap_or_default();
    let change = |plan| Ok(Change { entity, name: name.to_string(), plan });
    match name {
        "Name" => return Err("Name is not set here yet: rename the instance in the Explorer".to_string()),
        "ClassName" => return Err("ClassName cannot change".to_string()),
        "Position" if world.get::<Transform>(entity).is_some() => {
            let v = numbers(&json_text(value)?, 3)?;
            return change(Plan::Position(Vec3::new(v[0], v[1], v[2])));
        }
        "Rotation" | "Orientation" if world.get::<Transform>(entity).is_some() => {
            let v = numbers(&json_text(value)?, 3)?;
            let r = Quat::from_euler(EulerRot::XYZ, v[0].to_radians(), v[1].to_radians(), v[2].to_radians());
            return change(Plan::Rotation(r));
        }
        "Size" if world.get::<BasePart>(entity).is_some() => {
            let v = numbers(&json_text(value)?, 3)?;
            if v.iter().any(|c| *c <= 0.0) {
                return Err("Size must be positive on every axis".to_string());
            }
            return change(Plan::Size(Vec3::new(v[0], v[1], v[2])));
        }
        _ => {}
    }

    // A field-table class's field: parsed by its own table now.
    let kind = {
        fn spec_kind<T: FieldTable>(_: &T, name: &str) -> Option<FieldKind> {
            field(T::FIELDS, name).map(|s| s.kind)
        }
        let name = name.to_string();
        let w: &World = world;
        None.or_else(|| w.get::<ParticleSimulation>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<ParticleSpecies>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainSpline>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainSplinePoint>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainStamp>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainFlattenPad>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainNoise>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainMaterialFill>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainScatter>(entity).and_then(|c| spec_kind(c, &name)))
            .or_else(|| w.get::<TerrainWaterBody>(entity).and_then(|c| spec_kind(c, &name)))
    };
    if let Some(kind) = kind {
        let text = json_text(value)?;
        parse_value(kind, &text).map_err(|e| format!("{name}: {e}"))?;
        let file = record_file(world, entity).ok_or_else(|| format!("{class} has no record to save {name} into"))?;
        return change(Plan::Table { file, text });
    }

    // A light's, sky class's or Sound's field: its panel's rows name them.
    if panel.iter().any(|r| r.name == name && r.editable) {
        let text = json_text(value)?;
        let file = record_file(world, entity).ok_or_else(|| format!("{class} has no record to save {name} into"))?;
        return change(Plan::Panel { file, text });
    }
    if !panel.is_empty() {
        return Err(format!("{class} has no property {name}"));
    }

    // Everything else: the class's `PropertyAccess` table.
    let current = PropertyCommand::read_property(world, entity, name).ok_or_else(|| format!("{class} has no property {name}"))?;
    let text = json_text(value)?;
    let new = parse_like(&current, &text).map_err(|e| format!("{name}: {e}"))?;
    change(Plan::Generic(new))
}

/// Set one field-table or panel field from its text, through the same setter
/// undo replays it with, and report a parse or save error.
fn set_text(world: &mut World, entity: Entity, file: &Path, name: &str, text: &str, table: bool) -> Result<(), String> {
    let flat = |r: Option<Result<Option<Result<(), String>>, String>>| -> Option<Result<(), String>> {
        r.map(|outer| outer.and_then(|saved| saved.unwrap_or(Ok(()))))
    };
    if table {
        if let Some(mut c) = world.get_mut::<ParticleSimulation>(entity) {
            c.set_text(name, text)?;
            return crate::particles::bridge::save_class_section(file, &*c);
        }
        if let Some(mut c) = world.get_mut::<ParticleSpecies>(entity) {
            c.set_text(name, text)?;
            return crate::particles::bridge::save_class_section(file, &*c);
        }
        if let Some(r) = flat(crate::terrain_layers::set_field_text(world, entity, file, name, text)) {
            return r;
        }
        return Err(format!("no field table holds {name}"));
    }
    if let Some(r) = flat(crate::ui::light_panel::set_field_text(world, entity, file, name, text)) {
        return r;
    }
    if let Some(r) = flat(crate::ui::celestial_panel::set_field_text(world, entity, file, name, text)) {
        return r;
    }
    if let Some(mut sound) = world.get::<eustress_common::classes::Sound>(entity).cloned() {
        crate::ui::sound_panel::set_text(&mut sound, name, text)?;
        if crate::ui::sound_panel::replay_field(world, file, name, text) {
            return Ok(());
        }
    }
    Err(format!("no panel setter takes {name}"))
}

/// The stored text of a light's, sky class's or Sound's field.
fn panel_text(world: &mut World, entity: Entity, name: &str) -> Option<String> {
    let class = world.get::<Instance>(entity)?.class_name;
    panel_rows(world, class, entity)?.into_iter().find(|r| r.name == name).map(|r| r.value)
}

/// Save an instance's pose into its record, as a Move-tool drag does.
fn save_pose(world: &World, entity: Entity) -> Result<(), String> {
    let (Some(file), Some(t)) = (world.get::<InstanceFile>(entity), world.get::<Transform>(entity)) else {
        return Ok(()); // a binary instance: the database mirror keeps its pose
    };
    let stamp = world.get_resource::<crate::auth::AuthState>().and_then(crate::space::instance_loader::current_stamp);
    let mut def = crate::space::instance_loader::load_instance_definition(&file.toml_path)?;
    crate::space::instance_loader::set_authored_transform(&mut def, t.translation, t.rotation, None);
    crate::space::instance_loader::write_instance_definition_signed(&file.toml_path, &mut def, stamp.as_ref())
}

fn set_pose(world: &mut World, entity: Entity, translation: Vec3, rotation: Quat) -> Result<(), String> {
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
        t.translation = translation;
        t.rotation = rotation;
    }
    if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
        bp.cframe.translation = translation;
        bp.cframe.rotation = rotation;
    }
    save_pose(world, entity)
}

/// Persist a generic property as the panel does, when its record has a key
/// for it; the live change has happened either way.
fn save_generic(world: &World, entity: Entity, name: &str, value: &PropertyValue) -> Result<(), String> {
    // A binary core has no text record (its synthetic `__bin_` path names
    // none); the database mirror persists its components.
    let Some(file) = world
        .get::<InstanceFile>(entity)
        .map(|f| f.toml_path.clone())
        .filter(|p| !p.to_string_lossy().contains("__bin_") && p.is_file())
    else {
        return Ok(());
    };
    let Some(text) = crate::ui::slint_ui::property_value_to_toml_string(value) else { return Ok(()) };
    let current = world.get::<Transform>(entity).map(|t| (t.translation.to_array(), t.rotation.to_array(), t.scale.to_array()));
    crate::ui::slint_ui::apply_property_to_toml_value(&file, name, &text, current).map(|_| ())
}

fn apply(world: &mut World, change: &Change) -> Result<Done, String> {
    let entity = change.entity;
    match &change.plan {
        Plan::Table { file, text } | Plan::Panel { file, text } => {
            let table = matches!(change.plan, Plan::Table { .. });
            let old = if table { table_text(world, entity, &change.name) } else { panel_text(world, entity, &change.name) }
                .ok_or_else(|| format!("{} could not be read before the change", change.name))?;
            set_text(world, entity, file, &change.name, text, table).map_err(|e| format!("{}: {e}", change.name))?;
            Ok(Done::Text { entity, file: file.clone(), name: change.name.clone(), old, table })
        }
        Plan::Position(p) => {
            let t = *world.get::<Transform>(entity).ok_or("no pose")?;
            let p = crate::space::instance_loader::safe_translation(*p, t.translation);
            set_pose(world, entity, p, t.rotation)?;
            Ok(Done::Pose { entity, translation: t.translation, rotation: t.rotation })
        }
        Plan::Rotation(r) => {
            let t = *world.get::<Transform>(entity).ok_or("no pose")?;
            set_pose(world, entity, t.translation, *r)?;
            Ok(Done::Pose { entity, translation: t.translation, rotation: t.rotation })
        }
        Plan::Size(size) => {
            let bp = world.get::<BasePart>(entity).ok_or("not a part")?;
            let (old_size, translation) = (bp.size, bp.cframe.translation);
            world.write_message(crate::scale_tool::ResizePartEvent { entity, new_size: *size });
            Ok(Done::Size { entity, translation, old: old_size, new: *size })
        }
        Plan::Generic(new) => {
            let old = PropertyCommand::read_property(world, entity, &change.name).ok_or("no longer readable")?;
            PropertyCommand::new(entity, change.name.clone(), old.clone(), new.clone()).execute(world)?;
            save_generic(world, entity, &change.name, new)?;
            Ok(Done::Generic { entity, name: change.name.clone(), old, new: new.clone() })
        }
    }
}

/// Put one change back, best effort: the path that made it, in reverse.
fn restore(world: &mut World, done: &Done) {
    let result = match done {
        Done::Text { entity, file, name, old, table } => set_text(world, *entity, file, name, old, *table),
        Done::Pose { entity, translation, rotation } => set_pose(world, *entity, *translation, *rotation),
        Done::Size { entity, old, .. } => {
            world.write_message(crate::scale_tool::ResizePartEvent { entity: *entity, new_size: *old });
            Ok(())
        }
        Done::Generic { entity, name, old, new } => PropertyCommand::new(*entity, name.clone(), new.clone(), old.clone())
            .execute(world)
            .and_then(|_| save_generic(world, *entity, name, old)),
    };
    if let Err(e) = result {
        warn!("properties.set: could not put back a change after a failure: {e}");
    }
}

/// The undo actions for what was done: the ones the panel and the Move and
/// Scale tools record for the same changes.
fn undo_actions(world: &mut World, done: &[Done]) -> (Vec<Action>, Vec<String>) {
    let mut actions = Vec::new();
    let mut not_undoable = Vec::new();
    let mut poses: Vec<(Entity, Vec3, Quat)> = Vec::new();
    let mut sizes: Vec<(Entity, Vec3, Vec3, Vec3)> = Vec::new();
    for d in done {
        match d {
            Done::Text { entity, file, name, old, table } => {
                let new = if *table { table_text(world, *entity, name) } else { panel_text(world, *entity, name) };
                match new {
                    Some(new_text) => actions.push(Action::ChangeClassField {
                        toml_path: file.clone(),
                        property: name.clone(),
                        old_text: old.clone(),
                        new_text,
                    }),
                    None => not_undoable.push(name.clone()),
                }
            }
            Done::Pose { entity, translation, rotation } => {
                // An entity's first recorded pose is its pose before the set.
                if !poses.iter().any(|(e, ..)| e == entity) {
                    poses.push((*entity, *translation, *rotation));
                }
            }
            Done::Size { entity, translation, old, new } => match sizes.iter_mut().find(|(e, ..)| e == entity) {
                Some(entry) => entry.3 = *new,
                None => sizes.push((*entity, *translation, *old, *new)),
            },
            Done::Generic { entity, name, old, new } => {
                let snaps = crate::ui::slint_ui::is_undo_wired_property(name)
                    .then(|| {
                        crate::ui::slint_ui::property_value_to_undo_snapshot(name, old)
                            .zip(crate::ui::slint_ui::property_value_to_undo_snapshot(name, new))
                    })
                    .flatten();
                match (snaps, world.get::<Instance>(*entity).map(|i| i.id).filter(|id| *id != 0)) {
                    (Some((old_value, new_value)), Some(id)) => actions.push(Action::ChangeProperty {
                        id,
                        property: name.clone(),
                        old_value,
                        new_value,
                    }),
                    _ => not_undoable.push(name.clone()),
                }
            }
        }
    }
    if !poses.is_empty() {
        let (old_transforms, new_transforms) = poses
            .iter()
            .filter_map(|(e, t, r)| {
                let now = world.get::<Transform>(*e)?;
                Some(((e.to_bits(), t.to_array(), r.to_array()), (e.to_bits(), now.translation.to_array(), now.rotation.to_array())))
            })
            .unzip();
        actions.push(Action::TransformEntities { old_transforms, new_transforms });
    }
    if !sizes.is_empty() {
        // The resize lands on the next frame; the states are the ones asked for.
        let (old_states, new_states) = sizes
            .iter()
            .map(|(e, t, old, new)| ((e.to_bits(), t.to_array(), old.to_array()), (e.to_bits(), t.to_array(), new.to_array())))
            .unzip();
        actions.push(Action::ScaleEntities { old_states, new_states });
    }
    (actions, not_undoable)
}

/// What a change set did.
pub struct SetReport {
    pub changed: Vec<(Entity, String)>,
    /// Changed names the undo step cannot put back.
    pub not_undoable: Vec<String>,
    pub undo_steps: usize,
}

/// Apply every change in `targets` (an entity and its `{Name: value}` pairs),
/// all or nothing, as one undo step. See the module docs.
pub fn set(world: &mut World, targets: &[(Entity, Vec<(String, Value)>)]) -> Result<SetReport, String> {
    // Check every change first; nothing is touched until all pass.
    let mut changes = Vec::new();
    for (entity, props) in targets {
        let class = world.get::<Instance>(*entity).map(|i| i.class_name).ok_or("not an instance")?;
        let panel: Vec<Row> = panel_rows(world, class, *entity).unwrap_or_default();
        for (name, value) in props {
            changes.push(plan(world, *entity, name, value, &panel)?);
        }
    }

    // Apply in order; a failure puts back what came before it.
    let mut done = Vec::new();
    for change in &changes {
        match apply(world, change) {
            Ok(d) => done.push(d),
            Err(e) => {
                for d in done.iter().rev() {
                    restore(world, d);
                }
                return Err(e);
            }
        }
    }

    let (actions, not_undoable) = undo_actions(world, &done);
    let undo_steps = usize::from(!actions.is_empty());
    let label = format!("Set {} propert{}", changes.len(), if changes.len() == 1 { "y" } else { "ies" });
    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        match actions.len() {
            0 => {}
            1 => stack.push_labeled(label, actions.into_iter().next().expect("one action")),
            _ => stack.push_labeled(label, Action::Batch { actions }),
        }
    }
    Ok(SetReport {
        changed: changes.iter().map(|c| (c.entity, c.name.clone())).collect(),
        not_undoable,
        undo_steps,
    })
}

// ---------------------------------------------------------------------------
// Class methods
// ---------------------------------------------------------------------------

/// A class method's implementation: the instance and its arguments in, a
/// JSON result out.
pub type MethodFn = fn(&mut World, Entity, &Value) -> Result<Value, String>;

/// The engine side of `eustress_common::class_methods::METHODS`: each declared
/// method's implementation, registered by the plugin that owns its class.
#[derive(Resource, Default)]
pub struct ClassMethodImpls(Vec<(&'static str, &'static str, MethodFn)>);

impl ClassMethodImpls {
    /// Register `class:name` from a plugin's `build`. Declare the method in
    /// `class_methods::METHODS` first: the bridge refuses an undeclared one,
    /// and a debug build stops here.
    pub fn register(app: &mut App, class: &'static str, name: &'static str, call: MethodFn) {
        debug_assert!(
            eustress_common::class_methods::declared(class, name).is_some(),
            "{class}:{name} is registered but not declared in class_methods::METHODS"
        );
        let world = app.world_mut();
        if !world.contains_resource::<ClassMethodImpls>() {
            world.insert_resource(ClassMethodImpls::default());
        }
        world.resource_mut::<ClassMethodImpls>().0.push((class, name, call));
    }

    /// The implementation of `class:name`, when one is registered.
    pub fn get(&self, class: &str, name: &str) -> Option<MethodFn> {
        self.0.iter().find(|(c, n, _)| *c == class && *n == name).map(|(_, _, f)| *f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(world: &mut World, id: u32) -> Entity {
        world
            .spawn((
                Instance { name: format!("P{id}"), class_name: ClassName::Part, id, ..Default::default() },
                BasePart::default(),
                Transform::from_xyz(1.0, 2.0, 3.0),
            ))
            .id()
    }

    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(crate::undo::UndoStack::default());
        world
    }

    fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
        rows.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}: {rows:?}"))
    }

    #[test]
    fn text_forms_round_trip() {
        assert_eq!(json_text(&json!([1, 2.5, -3])).unwrap(), "1, 2.5, -3");
        assert_eq!(json_text(&json!(true)).unwrap(), "true");
        assert!(json_text(&json!({ "x": 1 })).is_err());
        assert_eq!(value_text(&PropertyValue::Float(0.5)).unwrap(), "0.5");
        assert_eq!(value_text(&PropertyValue::Vector3(Vec3::new(1.0, 0.0, -2.25))).unwrap(), "1, 0, -2.25");
        let c = parse_like(&PropertyValue::Color3([0.0; 3]), "255, 0, 128").unwrap();
        assert!(matches!(c, PropertyValue::Color3(v) if v[0] == 1.0 && v[1] == 0.0));
        assert!(parse_like(&PropertyValue::Bool(false), "yes").is_err());
        assert!(parse_like(&PropertyValue::Vector3(Vec3::ZERO), "1, 2").is_err());
    }

    /// A part shows its pose and size, metadata, and its classes' properties,
    /// each once.
    #[test]
    fn a_part_is_described() {
        let mut world = world();
        let e = part(&mut world, 7);
        let rows = describe(&mut world, e).unwrap();
        assert_eq!(row(&rows, "Position").value, "1, 2, 3");
        assert_eq!(row(&rows, "Position").unit, "m");
        assert!(row(&rows, "Size").editable);
        assert!(!row(&rows, "Name").editable, "renaming is not offered here");
        assert_eq!(row(&rows, "ClassName").value, "Part");
        let mut names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total, "a property listed twice");
    }

    /// Several parts change in one call, as one undo step; a pose change is
    /// the Move tool's TransformEntities.
    #[test]
    fn a_change_set_is_one_undo_step() {
        let mut world = world();
        let a = part(&mut world, 1);
        let b = part(&mut world, 2);
        let report = set(
            &mut world,
            &[
                (a, vec![("Position".to_string(), json!([5, 6, 7]))]),
                (b, vec![("Position".to_string(), json!([8, 9, 10])), ("Rotation".to_string(), json!([0, 90, 0]))]),
            ],
        )
        .unwrap();
        assert_eq!(report.changed.len(), 3);
        assert_eq!(report.undo_steps, 1);
        assert_eq!(world.get::<Transform>(a).unwrap().translation, Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(world.get::<BasePart>(b).unwrap().cframe.translation, Vec3::new(8.0, 9.0, 10.0));
        let stack = world.resource::<crate::undo::UndoStack>();
        assert_eq!(stack.current_index(), 1, "one undo step for the whole set");
    }

    /// One bad value refuses the whole set before anything changes.
    #[test]
    fn one_bad_value_changes_nothing() {
        let mut world = world();
        let a = part(&mut world, 3);
        for bad in [
            vec![("Position".to_string(), json!([5, 6, 7])), ("NoSuchProperty".to_string(), json!(1))],
            vec![("Position".to_string(), json!([5, 6, 7])), ("Size".to_string(), json!([1, 0, 1]))],
            vec![("Position".to_string(), json!([5, 6, 7])), ("Name".to_string(), json!("Renamed"))],
        ] {
            assert!(set(&mut world, &[(a, bad)]).is_err());
            assert_eq!(world.get::<Transform>(a).unwrap().translation, Vec3::new(1.0, 2.0, 3.0), "it moved");
        }
        assert_eq!(world.resource::<crate::undo::UndoStack>().current_index(), 0);
    }

    #[test]
    fn a_method_is_found_by_class_and_name() {
        fn noop(_: &mut World, _: Entity, _: &Value) -> Result<Value, String> {
            Ok(json!(null))
        }
        let impls = ClassMethodImpls(vec![("Road", "Split", noop as MethodFn)]);
        assert!(impls.get("Road", "Split").is_some());
        assert!(impls.get("Road", "Merge").is_none());
        assert!(impls.get("Part", "Split").is_none());
    }
}
