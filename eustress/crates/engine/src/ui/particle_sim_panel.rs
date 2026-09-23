//! Properties panel for the classes whose properties are field tables:
//! `ParticleSimulation`, `ParticleSpecies` and the terrain layer classes.
//!
//! Rows are generated from the classes' field tables (one row per field,
//! grouped by category, with the unit in the tooltip), followed by a
//! read-only Runtime section of live measurements. Edits parse through the
//! same tables, apply to the component (live when the field allows it,
//! restarting the run when it is an initial condition), push an undo step
//! keyed by the instance file, and persist the section unless a Play
//! session is running: Stop restores the authored values, so play-time
//! tweaks never leak into the Space.
//!
//! The Runtime rows are refreshed in place a few times a second with
//! `set_row_data`, never by replacing the model, so a field being edited
//! keeps its caret.
//!
//! The terrain layer classes (`TerrainSpline`, `TerrainSplinePoint`,
//! `TerrainStamp`, `TerrainFlattenPad`, `TerrainNoise`,
//! `TerrainMaterialFill`, `TerrainScatter`, `TerrainWaterBody`) keep their properties in field tables of the same
//! shape and show here too: their field rows, then Position and Rotation,
//! whose edits fall through to the generic Transform writer. They have no
//! Runtime section, and their edits are saved even during Play, since
//! nothing restores layers on Stop.

use std::collections::HashSet;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use slint::{Model, SharedString};

use eustress_common::classes::{ClassName, Instance};
use eustress_common::realism::particle_sim::class::{
    format_number, format_value, FieldKind, FieldSpec, FieldTable, FieldValue,
};
use eustress_common::realism::particle_sim::{
    ParticleSimRuntime, ParticleSimulation, ParticleSpecies, SimStats, StatSpec, SIMULATION_STATS, SPECIES_STATS,
};
use eustress_common::terrain::layer_instances::{
    TerrainFlattenPad, TerrainMaterialFill, TerrainNoise, TerrainScatter, TerrainSpline, TerrainSplinePoint,
    TerrainStamp, TerrainWaterBody,
};

use super::slint_ui::{PropertyData, SlintUiState};
use crate::play_mode::PlayModeState;


/// Category of the live measurement rows.
pub const RUNTIME_CATEGORY: &str = "Runtime";
/// Category of a terrain layer's Position and Rotation rows.
const TRANSFORM_CATEGORY: &str = "Transform";

pub fn is_particle_class(class: ClassName) -> bool {
    matches!(class, ClassName::ParticleSimulation | ClassName::ParticleSpecies)
}

/// Classes whose Properties rows this panel builds: the particle simulation
/// classes and the terrain layer classes.
pub fn is_field_table_class(class: ClassName) -> bool {
    is_particle_class(class) || class.is_terrain_layer()
}

/// Read access for building rows.
#[derive(SystemParam)]
pub struct PanelQueries<'w, 's> {
    sims: Query<'w, 's, &'static ParticleSimulation>,
    species: Query<'w, 's, &'static ParticleSpecies>,
    runtimes: Query<'w, 's, &'static ParticleSimRuntime>,
    parents: Query<'w, 's, &'static ChildOf>,
    /// The terrain layer component of a layer entity (at most one is set).
    layers: Query<
        'w,
        's,
        (
            Option<&'static TerrainSpline>,
            Option<&'static TerrainSplinePoint>,
            Option<&'static TerrainStamp>,
            Option<&'static TerrainFlattenPad>,
            Option<&'static TerrainNoise>,
            Option<&'static TerrainMaterialFill>,
            Option<&'static TerrainScatter>,
            Option<&'static TerrainWaterBody>,
        ),
    >,
    /// A layer's pose, for its Position and Rotation rows.
    transforms: Query<'w, 's, &'static Transform>,
    /// The unit Position is shown in, the one the generic writer reads it in.
    display_unit: Option<Res<'w, eustress_common::units::DisplayUnit>>,
}

/// Write access for panel edits (queries and optional resources only, so it
/// can never make the drain system fail validation). The instance file
/// comes from the drain's own `InstanceFile` query, which already holds
/// that component mutably.
#[derive(SystemParam)]
pub struct EditQueries<'w, 's> {
    sims: Query<'w, 's, &'static mut ParticleSimulation>,
    species: Query<'w, 's, &'static mut ParticleSpecies>,
    play_state: Option<Res<'w, State<PlayModeState>>>,
    /// Terrain layer components. No `Transform` here: the drain system
    /// already writes it, and Position / Rotation edits are its own.
    layers: Query<
        'w,
        's,
        (
            Option<&'static mut TerrainSpline>,
            Option<&'static mut TerrainSplinePoint>,
            Option<&'static mut TerrainStamp>,
            Option<&'static mut TerrainFlattenPad>,
            Option<&'static mut TerrainNoise>,
            Option<&'static mut TerrainMaterialFill>,
            Option<&'static mut TerrainScatter>,
            Option<&'static mut TerrainWaterBody>,
        ),
    >,
}

fn blank_row(category: &str, collapsed: &HashSet<String>) -> PropertyData {
    PropertyData {
        name: SharedString::default(),
        value: SharedString::default(),
        property_type: SharedString::default(),
        category: category.into(),
        editable: false,
        options: slint::ModelRc::default(),
        is_header: false,
        section_collapsed: collapsed.contains(category),
        x_value: SharedString::default(),
        y_value: SharedString::default(),
        z_value: SharedString::default(),
        x_scale: SharedString::default(),
        x_offset: SharedString::default(),
        y_scale: SharedString::default(),
        y_offset: SharedString::default(),
        color_value: slint::Color::from_rgb_u8(0x80, 0x80, 0x80),
        description: SharedString::default(),
        learn_url: SharedString::default(),
        is_attribute: false,
        attribute_type: SharedString::default(),
        slider_min: 0.0,
        slider_max: 1.0,
        slider_display: SharedString::default(),
    }
}

fn header(category: &str, collapsed: &HashSet<String>) -> PropertyData {
    PropertyData { is_header: true, ..blank_row(category, collapsed) }
}

fn with_unit(description: &str, unit: &str) -> String {
    if unit.is_empty() { description.to_string() } else { format!("{description} ({unit})") }
}

fn field_row(spec: &FieldSpec, value: &FieldValue, collapsed: &HashSet<String>) -> PropertyData {
    let mut row = blank_row(spec.category, collapsed);
    row.name = spec.name.into();
    row.value = format_value(value).into();
    row.editable = true;
    row.description = with_unit(spec.description, spec.unit).into();
    row.property_type = match spec.kind {
        FieldKind::Bool => "bool",
        FieldKind::Int => "int",
        FieldKind::Float => "float",
        FieldKind::Vector3 => "vec3",
        FieldKind::Color3 => "color",
        FieldKind::Choice(_) => "choice",
        FieldKind::Text => "string",
    }
    .into();
    match (spec.kind, value) {
        (FieldKind::Vector3, FieldValue::Vec3(a)) => {
            row.x_value = format_number(a[0]).into();
            row.y_value = format_number(a[1]).into();
            row.z_value = format_number(a[2]).into();
        }
        (FieldKind::Color3, FieldValue::Color(c)) => {
            let b = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
            row.color_value = slint::Color::from_rgb_u8(b[0], b[1], b[2]);
        }
        (FieldKind::Choice(options), _) => {
            let opts: Vec<SharedString> = options.iter().map(|o| SharedString::from(*o)).collect();
            row.options = slint::ModelRc::new(slint::VecModel::from(opts));
        }
        _ => {}
    }
    row
}

fn stat_text<T>(spec: &StatSpec<T>, source: &T) -> String {
    let v = (spec.get)(source);
    if spec.unit.is_empty() { format_number(v) } else { format!("{} {}", format_number(v), spec.unit) }
}

fn stat_row<T>(spec: &StatSpec<T>, source: Option<&T>, collapsed: &HashSet<String>) -> PropertyData {
    let mut row = blank_row(RUNTIME_CATEGORY, collapsed);
    row.name = spec.name.into();
    row.value = source.map(|s| stat_text(spec, s)).unwrap_or_else(|| "-".into()).into();
    row.property_type = "string".into();
    row.description = spec.description.into();
    row
}

/// Emit a component's field rows, one header per category in table order.
fn push_fields<T: FieldTable>(out: &mut Vec<PropertyData>, component: &T, collapsed: &HashSet<String>) {
    let mut current = "";
    for spec in T::FIELDS {
        if spec.category != current {
            current = spec.category;
            out.push(header(current, collapsed));
        }
        if let Some(v) = component.get_field(spec.name) {
            out.push(field_row(spec, &v, collapsed));
        }
    }
}

/// Position and Rotation rows of a terrain layer: where a stamp or pad sits
/// (a pad's top is its Position Y), which way it turns, where a spline point
/// is. Edits fall through to the generic Transform writer, which records
/// undo and saves the file; Position is shown in the display unit, the one
/// that writer reads it in, and Rotation in the Euler XYZ degrees it takes.
fn push_transform_rows(out: &mut Vec<PropertyData>, entity: Entity, queries: &PanelQueries, collapsed: &HashSet<String>) {
    let Ok(transform) = queries.transforms.get(entity) else { return };
    let native = eustress_common::units::ENGINE_NATIVE_UNIT;
    let unit = queries.display_unit.as_ref().map_or(native, |u| u.0);
    let position = eustress_common::units::convert_vec3_f32(transform.translation.to_array(), native, unit);
    let (rx, ry, rz) = transform.rotation.to_euler(EulerRot::XYZ);
    let row = |name: &str, kind: &str, v: [f32; 3], decimals: usize, description: &str| {
        let mut row = blank_row(TRANSFORM_CATEGORY, collapsed);
        let [x, y, z] = v.map(|c| format!("{:.*}", decimals, c));
        row.name = name.into();
        row.property_type = kind.into();
        row.editable = true;
        row.description = description.into();
        row.value = format!("{x}, {y}, {z}").into();
        row.x_value = x.into();
        row.y_value = y.into();
        row.z_value = z.into();
        row
    };
    out.push(header(TRANSFORM_CATEGORY, collapsed));
    out.push(row("Position", "vec3", position, 3, "Where the layer sits. A flatten pad's top, and a stamp's level for Max, Min and Replace, are its Y."));
    out.push(row(
        "Rotation",
        "rotation",
        [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
        2,
        "Turn about Y heads the layer; a ridge, pad, noise or fill turns with it.",
    ));
}

/// Where a species' measurements come from: its simulation's runtime and
/// its index there.
fn species_stats<'a>(
    entity: Entity,
    queries: &'a PanelQueries,
) -> Option<(&'a ParticleSimRuntime, usize)> {
    let parent = queries.parents.get(entity).ok()?.parent();
    let rt = queries.runtimes.get(parent).ok()?;
    let index = rt.species_entities.iter().position(|e| *e == entity)?;
    Some((rt, index))
}

/// All rows for a selected simulation, species or terrain layer.
pub fn build_rows(
    entity: Entity,
    instance: &Instance,
    queries: &PanelQueries,
    collapsed: &HashSet<String>,
) -> Vec<PropertyData> {
    let mut out = Vec::new();
    match instance.class_name {
        ClassName::ParticleSimulation => {
            if let Ok(sim) = queries.sims.get(entity) {
                push_fields(&mut out, sim, collapsed);
            }
            out.push(header(RUNTIME_CATEGORY, collapsed));
            let stats: Option<&SimStats> = queries.runtimes.get(entity).ok().map(|rt| rt.stats());
            for spec in SIMULATION_STATS {
                out.push(stat_row(spec, stats, collapsed));
            }
        }
        ClassName::ParticleSpecies => {
            if let Ok(sp) = queries.species.get(entity) {
                push_fields(&mut out, sp, collapsed);
            }
            out.push(header(RUNTIME_CATEGORY, collapsed));
            let found = species_stats(entity, queries);
            let stats = found.and_then(|(rt, i)| rt.stats().species.get(i));
            for spec in SPECIES_STATS {
                out.push(stat_row(spec, stats, collapsed));
            }
        }
        class if class.is_terrain_layer() => {
            if let Ok((spline, point, stamp, pad, noise, fill, scatter, water)) = queries.layers.get(entity) {
                if let Some(c) = spline {
                    push_fields(&mut out, c, collapsed);
                }
                if let Some(c) = point {
                    push_fields(&mut out, c, collapsed);
                }
                if let Some(c) = stamp {
                    push_fields(&mut out, c, collapsed);
                }
                if let Some(c) = pad {
                    push_fields(&mut out, c, collapsed);
                }
                if let Some(c) = noise {
                    push_fields(&mut out, c, collapsed);
                }
                if let Some(c) = fill {
                    push_fields(&mut out, c, collapsed);
                }
                if let Some(c) = scatter {
                    push_fields(&mut out, c, collapsed);
                }
                if let Some(c) = water {
                    push_fields(&mut out, c, collapsed);
                }
            }
            push_transform_rows(&mut out, entity, queries, collapsed);
        }
        _ => {}
    }
    out.push(header("Metadata", collapsed));
    let mut class_row = blank_row("Metadata", collapsed);
    class_row.name = "ClassName".into();
    class_row.value = format!("{:?}", instance.class_name).into();
    class_row.property_type = "string".into();
    out.push(class_row);
    let mut name_row = blank_row("Metadata", collapsed);
    name_row.name = "Name".into();
    name_row.value = instance.name.clone().into();
    name_row.property_type = "string".into();
    name_row.editable = true;
    out.push(name_row);
    out
}

/// What a panel edit did, for the Output panel and the undo stack.
pub struct EditOutcome {
    pub message: String,
    pub undo: Option<crate::undo::Action>,
}

/// Apply a Properties edit if `key` is a field of the selected entity's
/// particle or terrain layer class. `None` means "not ours" (e.g. Name, or a
/// layer's Position): the generic handler takes it.
pub fn handle_edit(
    entity: Entity,
    key: &str,
    raw: &str,
    toml_path: Option<&std::path::Path>,
    queries: &mut EditQueries,
) -> Option<Result<EditOutcome, String>> {
    let playing = crate::particles::bridge::is_playing(queries.play_state.as_deref());
    if let Ok(mut sim) = queries.sims.get_mut(entity) {
        return apply_field(&mut *sim, toml_path, key, raw, playing);
    }
    if let Ok(mut sp) = queries.species.get_mut(entity) {
        return apply_field(&mut *sp, toml_path, key, raw, playing);
    }
    // Terrain layers save and record undo even during Play: nothing restores
    // them on Stop, so an unsaved edit would silently part from the file.
    if let Ok((spline, point, stamp, pad, noise, fill, scatter, water)) = queries.layers.get_mut(entity) {
        if let Some(mut c) = spline {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
        if let Some(mut c) = point {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
        if let Some(mut c) = stamp {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
        if let Some(mut c) = pad {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
        if let Some(mut c) = noise {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
        if let Some(mut c) = fill {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
        if let Some(mut c) = scatter {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
        if let Some(mut c) = water {
            return apply_field(&mut *c, toml_path, key, raw, false);
        }
    }
    None
}

fn apply_field<T: FieldTable>(
    component: &mut T,
    toml_path: Option<&std::path::Path>,
    key: &str,
    raw: &str,
    playing: bool,
) -> Option<Result<EditOutcome, String>> {
    let spec = eustress_common::realism::particle_sim::class::field(T::FIELDS, key)?;
    let old_text = component.text(spec.name).unwrap_or_default();
    if let Err(e) = component.set_text(spec.name, raw) {
        return Some(Err(format!("{}: {e}", spec.name)));
    }
    let new_text = component.text(spec.name).unwrap_or_default();
    if new_text == old_text {
        return Some(Ok(EditOutcome { message: String::new(), undo: None }));
    }
    let mut message = format!("{} = {new_text}", spec.name);
    if spec.restarts {
        message.push_str(" (restarts the run)");
    }
    let mut undo = None;
    match toml_path {
        Some(path) if !playing => {
            if let Err(e) = crate::particles::bridge::save_class_section(path, component) {
                return Some(Err(format!("{} changed but was not saved: {e}", spec.name)));
            }
            undo = Some(crate::undo::Action::ChangeClassField {
                toml_path: path.to_path_buf(),
                property: spec.name.to_string(),
                old_text,
                new_text,
            });
        }
        Some(_) => message.push_str(" (Play session: reverts on Stop)"),
        None => {}
    }
    Some(Ok(EditOutcome { message, undo }))
}

/// Refresh the Runtime rows of the open panel in place, about four times
/// a second, when the selection is a simulation or a species.
pub fn refresh_runtime_rows(
    slint_context: Option<NonSend<SlintUiState>>,
    explorer: Option<Res<super::slint_ui::UnifiedExplorerState>>,
    queries: PanelQueries,
    time: Res<Time>,
    mut elapsed: Local<f32>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 0.25 {
        return;
    }
    *elapsed = 0.0;
    let (Some(ctx), Some(explorer)) = (slint_context, explorer) else { return };
    let super::slint_ui::SelectedItem::Entity(entity) = &explorer.selected else { return };
    let entity = *entity;
    let texts: Vec<(&'static str, String)> = if let Ok(rt) = queries.runtimes.get(entity) {
        SIMULATION_STATS.iter().map(|s| (s.name, stat_text(s, rt.stats()))).collect()
    } else if let Some((rt, i)) = species_stats(entity, &queries) {
        match rt.stats().species.get(i) {
            Some(sp) => SPECIES_STATS.iter().map(|s| (s.name, stat_text(s, sp))).collect(),
            None => return,
        }
    } else {
        return;
    };
    let model = ctx.window.get_entity_properties();
    for i in 0..model.row_count() {
        let Some(mut row) = model.row_data(i) else { continue };
        if row.is_header || row.category.as_str() != RUNTIME_CATEGORY {
            continue;
        }
        if let Some((_, text)) = texts.iter().find(|(n, _)| row.name.as_str() == *n) {
            if row.value.as_str() != text {
                row.value = text.as_str().into();
                model.set_row_data(i, row);
            }
        }
    }
}
