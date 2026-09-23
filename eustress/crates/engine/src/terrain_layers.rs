//! Engine side of the terrain layer classes (`TerrainSpline`,
//! `TerrainSplinePoint`, `TerrainStamp`, `TerrainFlattenPad`, `TerrainNoise`,
//! `TerrainMaterialFill`, and `TerrainScatter` and `TerrainWaterBody`, which
//! bake nothing but are loaded, edited and inserted the same way). Their
//! components, field tables, the mapping onto the bake and the system that
//! re-bakes when one changes are shared with the Client in
//! `eustress_common::terrain::layer_instances`, scatter placement in
//! `eustress_common::terrain::scatter` and lakes and river water in
//! `eustress_common::terrain::water_bodies`; this module is the part that
//! needs the Space's files and the Studio:
//!
//! - **spawn**: attach the class component from its TOML section;
//! - **hot reload**: re-read the section when the file changes on disk;
//! - **undo**: replay a Properties edit (`Action::ChangeClassField`);
//! - **hierarchy**: a point belongs to the spline whose folder holds it;
//! - **Insert**: a new layer goes in `Workspace/Terrain/Layers` on the ground
//!   under the view, a new spline starts with two points, and a point goes
//!   into the selected spline past its last point. The Insert menu and the
//!   Terrain panel's Layers buttons both come here, through the same
//!   `insert:<Class>` action;
//! - **gizmos**: a selected spline, or the spline of a selected point, draws
//!   its points, centreline, bed and shoulders.
//!
//! Layers are not parented to the `TerrainRoot` (see the common module for
//! why): on disk they sit under the terrain directory, in the ECS beside the
//! Workspace's own children, and the Explorer lists them under the Terrain.
//! Properties edits their fields through the field-table rows of
//! `ui::particle_sim_panel`, which saves the section and records undo.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::ecs::component::Mutable;
use bevy::math::Ray3d;
use bevy::prelude::*;

use eustress_common::classes::ClassName;
use eustress_common::realism::particle_sim::class::FieldTable;
use eustress_common::terrain::layer_instances::{
    layers_dir, parse_layer_instance, section_name, LayerComponent, LayerQueries, TerrainFlattenPad,
    TerrainMaterialFill, TerrainNoise, TerrainScatter, TerrainSpline, TerrainSplinePoint, TerrainStamp,
    TerrainWaterBody, LAYER_SECTIONS,
};
use eustress_common::terrain::layers::{SplineLayer, SplineMode};
use eustress_common::terrain::road::{build_road_path_with, RoadPath, RoadStation};
use eustress_common::terrain::{
    height_at_world, raycast_terrain, surface_data, TerrainBaked, TerrainConfig, TerrainData, TerrainRoot,
};

use crate::selection_box::Selected;
use crate::space::instance_create::{create_instance, InstanceOverrides};
use crate::space::instance_loader::InstanceFile;

// ============================================================================
// Spawn, hot reload, undo
// ============================================================================

/// Attach the class component of a terrain layer instance from its flattened
/// TOML sections. Called by the instance loader for data-only instances;
/// unknown or bad keys keep their defaults and are logged, so a newer or
/// hand-edited file still loads. Any other class is left alone.
pub fn attach_class_component(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: ClassName,
    extra: &HashMap<String, toml::Value>,
    source: &Path,
) {
    let section = |name: &str| extra.get(name).and_then(|v| v.as_table());
    let Some((component, problems)) = LayerComponent::from_sections(class_name, section) else { return };
    for problem in &problems {
        warn!("{}: {problem}", source.display());
    }
    component.insert_into(ec);
}

/// True for the sections the layer classes own (kept out of the generic
/// Attributes fold, which would duplicate them as user attributes).
pub fn is_class_section(section: &str) -> bool {
    LAYER_SECTIONS.contains(&section)
}

/// Hot reload: re-read the class section of a layer instance whose file
/// changed on disk (MCP edits, git checkouts, a text editor). Queued as an
/// entity command so the file watcher needs no new system parameters. A file
/// without the class's section is not an edit back to the defaults, so it
/// changes nothing.
pub fn queue_section_reload(commands: &mut Commands, entity: Entity, toml_text: &str) {
    let Ok(doc) = toml_text.parse::<toml::Value>() else { return };
    let Some(class_name) = doc
        .get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(|v| v.as_str())
        .and_then(|s| ClassName::from_str(s).ok())
    else {
        return;
    };
    let section = |name: &str| doc.get(name).and_then(|v| v.as_table());
    if section_name(class_name).and_then(|name| section(name)).is_none() {
        return;
    }
    let Some((fresh, _)) = LayerComponent::from_sections(class_name, section) else { return };
    commands.entity(entity).queue(move |mut entity: EntityWorldMut| fresh.replace_in(&mut entity));
}

/// Set one field of a terrain layer class on `entity` from its text form and
/// save the section into `toml_path`: how the undo stack's `ChangeClassField`
/// replays a Properties edit. `None` when the entity has no layer component;
/// otherwise the parse result, and inside it the save result. Layer edits are
/// saved in a Play session too: nothing restores layers on Stop, so an
/// unsaved edit would silently part from the file.
pub fn set_field_text(
    world: &mut World,
    entity: Entity,
    toml_path: &Path,
    property: &str,
    text: &str,
) -> Option<Result<Option<Result<(), String>>, String>> {
    fn set<T: FieldTable + Component<Mutability = Mutable>>(
        world: &mut World,
        entity: Entity,
        toml_path: &Path,
        property: &str,
        text: &str,
    ) -> Option<Result<Option<Result<(), String>>, String>> {
        let mut component = world.get_mut::<T>(entity)?;
        Some(
            component
                .set_text(property, text)
                .map(|_| Some(crate::particles::bridge::save_class_section(toml_path, &*component))),
        )
    }
    if let Some(done) = set::<TerrainSpline>(world, entity, toml_path, property, text) {
        return Some(done);
    }
    if let Some(done) = set::<TerrainSplinePoint>(world, entity, toml_path, property, text) {
        return Some(done);
    }
    if let Some(done) = set::<TerrainStamp>(world, entity, toml_path, property, text) {
        return Some(done);
    }
    if let Some(done) = set::<TerrainFlattenPad>(world, entity, toml_path, property, text) {
        return Some(done);
    }
    if let Some(done) = set::<TerrainNoise>(world, entity, toml_path, property, text) {
        return Some(done);
    }
    if let Some(done) = set::<TerrainMaterialFill>(world, entity, toml_path, property, text) {
        return Some(done);
    }
    if let Some(done) = set::<TerrainScatter>(world, entity, toml_path, property, text) {
        return Some(done);
    }
    set::<TerrainWaterBody>(world, entity, toml_path, property, text)
}

// ============================================================================
// Hierarchy
// ============================================================================

/// Parent each spline point to the spline whose folder holds it. The file
/// watcher parents a new file to whatever its grandparent folder resolved to
/// at that instant; when a point lands before its spline (Insert writes a
/// spline and its first points in one go, undo restores a whole spline
/// folder) that is the Workspace. This repairs it once both exist.
pub fn adopt_points_by_folder(
    mut commands: Commands,
    new_points: Query<(), Added<TerrainSplinePoint>>,
    new_splines: Query<(), Added<TerrainSpline>>,
    points: Query<(Entity, &InstanceFile, Option<&ChildOf>), With<TerrainSplinePoint>>,
    splines: Query<(Entity, &InstanceFile), With<TerrainSpline>>,
) {
    if new_points.is_empty() && new_splines.is_empty() {
        return;
    }
    let by_folder: HashMap<PathBuf, Entity> = splines
        .iter()
        .filter_map(|(entity, file)| file.toml_path.parent().map(|dir| (dir.to_path_buf(), entity)))
        .collect();
    for (entity, file, parent) in &points {
        let Some(spline_dir) = file.toml_path.parent().and_then(|dir| dir.parent()) else { continue };
        let Some(&spline) = by_folder.get(spline_dir) else { continue };
        if parent.map(|p| p.parent()) != Some(spline) {
            commands.entity(entity).insert(ChildOf(spline));
        }
    }
}

// ============================================================================
// Insert
// ============================================================================

/// Distance from a new spline's centre to each of its first two points.
const NEW_SPLINE_HALF_LENGTH: f32 = 20.0;
/// How far a point added to a spline lands past its last point.
const POINT_SPACING: f32 = 20.0;
/// Where a new layer lands when the view does not meet the ground: this far
/// ahead of the camera, on the ground.
const FALLBACK_FOCUS_DISTANCE: f32 = 40.0;
/// Longest view ray tested against the ground, and its step, metres.
const FOCUS_RAY_LENGTH: f32 = 4_000.0;
const FOCUS_RAY_STEP: f32 = 2.0;

/// Where and how Insert creates a terrain layer instance (see
/// [`plan_insert`]).
#[derive(Clone, Debug)]
pub struct LayerInsert {
    /// Folder the instance is created in.
    pub dir: PathBuf,
    /// Its pose.
    pub overrides: InstanceOverrides,
    /// A new point's `Index`: one past the spline's last.
    point_index: Option<f64>,
    /// A new spline's first points, positions under the spline.
    spline_points: Vec<Vec3>,
}

/// Plan an Insert of `class_name` when it is a terrain layer class, `None`
/// otherwise. A layer goes in `Workspace/Terrain/Layers`, whatever is
/// selected, on the ground where the view meets it; a spline also gets two
/// points across the view. A point goes into the selected spline (or the
/// spline of the selected point) past its last point, and without a spline
/// selected the Insert is refused with the reason. `selected` is the
/// selected instance's class and the folder the generic Insert would use for
/// it (its own folder); `terrain` is the ground the view shows (the layer
/// bake through `surface_data`), so a new layer lands on the ground the user
/// sees, a road's cut included.
pub fn plan_insert<'a>(
    class_name: &str,
    space_root: &Path,
    selected: Option<(ClassName, &Path)>,
    cameras: impl IntoIterator<Item = (&'a Camera, &'a GlobalTransform)>,
    terrain: Option<(&TerrainConfig, &TerrainData)>,
) -> Option<Result<LayerInsert, String>> {
    let class = ClassName::from_str(class_name).ok().filter(ClassName::is_terrain_layer)?;
    if class == ClassName::TerrainSplinePoint {
        return Some(plan_point_insert(selected));
    }
    let view = view_focus(cameras, terrain);
    let spline_points = if class == ClassName::TerrainSpline {
        match view {
            Some((focus, right)) => [-1.0f32, 1.0]
                .into_iter()
                .map(|side| {
                    let at = focus + right * (side * NEW_SPLINE_HALF_LENGTH);
                    let ground = terrain.map_or(focus.y, |(config, data)| height_at_world(config, data, at.x, at.z));
                    Vec3::new(at.x, ground, at.z) - focus
                })
                .collect(),
            None => vec![Vec3::NEG_X * NEW_SPLINE_HALF_LENGTH, Vec3::X * NEW_SPLINE_HALF_LENGTH],
        }
    } else {
        Vec::new()
    };
    Some(Ok(LayerInsert {
        dir: layers_dir(space_root),
        overrides: InstanceOverrides { position: view.map(|(focus, _)| focus), ..Default::default() },
        point_index: None,
        spline_points,
    }))
}

fn plan_point_insert(selected: Option<(ClassName, &Path)>) -> Result<LayerInsert, String> {
    let refused = || "Select a TerrainSpline, or one of its points, to add a point to it.".to_string();
    let spline_dir = match selected {
        Some((ClassName::TerrainSpline, dir)) => dir.to_path_buf(),
        Some((ClassName::TerrainSplinePoint, dir)) => dir.parent().map(Path::to_path_buf).ok_or_else(refused)?,
        _ => return Err(refused()),
    };
    let points = read_spline_points(&spline_dir);
    let index = points.last().map_or(0.0, |(last, _)| last.floor() + 1.0);
    Ok(LayerInsert {
        overrides: InstanceOverrides { position: Some(next_point_position(&points)), ..Default::default() },
        dir: spline_dir,
        point_index: Some(index),
        spline_points: Vec::new(),
    })
}

/// An instance file's text: the WorldDb's copy when a DB is active (it is
/// authoritative on migrated Spaces), else the disk's.
pub(crate) fn instance_text(toml_path: &Path) -> Option<String> {
    crate::space::active_db::get_instance_text(toml_path).or_else(|| std::fs::read_to_string(toml_path).ok())
}

/// The points in a spline's folder as `(Index, position under the spline)`,
/// in Index order. Each file is read through [`instance_text`].
fn read_spline_points(spline_dir: &Path) -> Vec<(f64, Vec3)> {
    let Ok(entries) = std::fs::read_dir(spline_dir) else { return Vec::new() };
    let mut points: Vec<(f64, Vec3)> = entries
        .flatten()
        .filter_map(|entry| {
            let toml_path = entry.path().join("_instance.toml");
            let text = instance_text(&toml_path)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let (point, _) = parse_layer_instance(&text, &name).ok()??;
            match point.component {
                LayerComponent::Point(p) => Some((p.index, point.transform.translation)),
                _ => None,
            }
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points
}

/// Where a point added to a spline with `points` (in Index order) goes: past
/// the last one, the way the spline leaves it, at the last one's height.
fn next_point_position(points: &[(f64, Vec3)]) -> Vec3 {
    match points {
        [] => Vec3::ZERO,
        [(_, only)] => *only + Vec3::X * POINT_SPACING,
        [.., (_, before), (_, last)] => {
            let heading = Vec3::new(last.x - before.x, 0.0, last.z - before.z).try_normalize().unwrap_or(Vec3::X);
            *last + heading * POINT_SPACING
        }
    }
}

/// Where the view meets the ground, the scene camera's forward ray against
/// `terrain`, else a point ahead of the camera on the ground; and the view's
/// right hand flattened onto the ground. `None` without a scene camera.
fn view_focus<'a>(
    cameras: impl IntoIterator<Item = (&'a Camera, &'a GlobalTransform)>,
    terrain: Option<(&TerrainConfig, &TerrainData)>,
) -> Option<(Vec3, Vec3)> {
    let (_, view) = cameras.into_iter().find(|(camera, _)| camera.order == 0)?;
    let origin = view.translation();
    let forward = view.forward();
    let hit = terrain.and_then(|(config, data)| {
        raycast_terrain(config, data, Ray3d::new(origin, forward), FOCUS_RAY_LENGTH, FOCUS_RAY_STEP)
    });
    let focus = hit.unwrap_or_else(|| {
        let ahead = origin + *forward * FALLBACK_FOCUS_DISTANCE;
        let ground = terrain.map_or(ahead.y, |(config, data)| height_at_world(config, data, ahead.x, ahead.z));
        Vec3::new(ahead.x, ground, ahead.z)
    });
    let right = view.right();
    let right = Vec3::new(right.x, 0.0, right.z).try_normalize().unwrap_or(Vec3::X);
    Some((focus, right))
}

/// Finish an Insert that [`plan_insert`] planned, once the instance exists at
/// `folder`: write a new point's `Index`, and give a new spline its first
/// points (created the canonical way, so each has its own uuid and undo
/// trashes them with the spline's folder).
pub fn finish_insert(plan: &LayerInsert, folder: &Path) {
    if let Some(index) = plan.point_index {
        write_point_index(&folder.join("_instance.toml"), index);
    }
    for position in &plan.spline_points {
        if let Err(e) = append_spline_point(folder, *position) {
            warn!("Insert TerrainSpline: a first point failed: {e}");
        }
    }
}

/// Create a point at the end of the spline whose folder is `spline_dir`, at
/// `position` under the spline, through the canonical `instance_create`
/// path: its `Index` is one past the spline's last point (0 for the first)
/// and its name follows from it. Returns the new point's folder. The file
/// watcher spawns it; a spline created in the same breath may still be
/// spawning, and `adopt_points_by_folder` parents the point once it has.
pub fn append_spline_point(spline_dir: &Path, position: Vec3) -> Result<PathBuf, String> {
    let index = read_spline_points(spline_dir).last().map_or(0.0, |(last, _)| last.floor() + 1.0);
    let name = format!("Point{}", index.max(0.0) as u64 + 1);
    let overrides = InstanceOverrides { position: Some(position), ..Default::default() };
    let created = create_instance(spline_dir, ClassName::TerrainSplinePoint.as_str(), Some(name.as_str()), overrides)
        .map_err(|e| e.to_string())?;
    write_point_index(&created.toml_path, index);
    Ok(created.folder_path)
}

fn write_point_index(toml_path: &Path, index: f64) {
    // The template's own Index needs no second write.
    if index == TerrainSplinePoint::default().index {
        return;
    }
    if let Err(e) = crate::particles::bridge::save_class_section(toml_path, &TerrainSplinePoint { index }) {
        warn!("{}: point Index not written: {e}", toml_path.display());
    }
}

// ============================================================================
// Gizmos
// ============================================================================

/// Most stations a spline gizmo draws along its length; a longer line is
/// drawn through every n-th station.
const GIZMO_MAX_STATIONS: usize = 512;
/// Station spacing of a spline drawn through its points rather than along its
/// bake, metres, before [`GIZMO_MAX_STATIONS`] widens it.
const GIZMO_PREVIEW_SPACING: f32 = 2.0;
/// How far the lines float above the ground, metres: enough to clear the
/// terrain mesh between two stations.
const GIZMO_LIFT: f32 = 0.25;
/// A control point marker's radius per metre from the camera, so it keeps
/// about one size on screen, and the limits of that radius, metres.
const GIZMO_POINT_SCALE: f32 = 0.012;
const GIZMO_POINT_MIN: f32 = 0.25;
const GIZMO_POINT_MAX: f32 = 8.0;
/// A selected point is drawn this much larger, in the selection colour.
const GIZMO_SELECTED_POINT_SCALE: f32 = 1.6;
/// Circle resolution of the point markers: a marker is a few pixels across.
const GIZMO_POINT_RESOLUTION: u32 = 12;
/// The Studio's selection cyan.
const SELECTED_POINT_COLOR: Color = Color::srgb(0.0, 0.737, 0.831);
/// A disabled spline's lines, whatever its mode.
const DISABLED_RGB: (f32, f32, f32) = (0.6, 0.6, 0.6);

/// The colour a spline's mode draws in.
fn mode_rgb(mode: SplineMode) -> (f32, f32, f32) {
    match mode {
        SplineMode::Road => (1.0, 0.75, 0.2),
        SplineMode::Path => (0.85, 0.7, 0.45),
        SplineMode::River => (0.25, 0.6, 1.0),
        SplineMode::Canyon => (0.95, 0.45, 0.25),
        SplineMode::Embankment => (0.45, 0.9, 0.4),
    }
}

fn marker_radius(eye: Vec3, at: Vec3) -> f32 {
    (eye.distance(at) * GIZMO_POINT_SCALE).clamp(GIZMO_POINT_MIN, GIZMO_POINT_MAX)
}

/// Draw every selected `TerrainSpline`, and the spline of every selected
/// `TerrainSplinePoint`: its control points (a selected one larger, in the
/// selection colour) each with a stem to the ground under it, the straight
/// polygon through them, then the centreline, the bed edges and the outer
/// edge of the shoulders in its mode's colour (grey while disabled). The
/// corridor follows the stations the bake carved it along while the bake
/// holds the spline as it stands; a disabled spline, or one whose throttled
/// bake has not caught up with a drag yet, is drawn along a path through its
/// points as they stand. The lines lie on the ground the view shows. The
/// scene camera (order 0) is the view: without one nothing is drawn, and the
/// markers keep their screen size against its distance.
pub fn draw_spline_gizmos(
    mut gizmos: Gizmos,
    selected_splines: Query<Entity, (With<TerrainSpline>, With<Selected>)>,
    selected_points: Query<(Entity, &ChildOf), (With<TerrainSplinePoint>, With<Selected>)>,
    layers: LayerQueries,
    terrain: Query<(&TerrainConfig, &TerrainData, Option<&TerrainBaked>), With<TerrainRoot>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
) {
    if selected_splines.is_empty() && selected_points.is_empty() {
        return;
    }
    let Some((_, view)) = cameras.iter().find(|(camera, _)| camera.order == 0) else { return };
    let eye = view.translation();
    let mut splines: Vec<Entity> = selected_splines.iter().collect();
    for (_, child_of) in &selected_points {
        if !splines.contains(&child_of.parent()) {
            splines.push(child_of.parent());
        }
    }
    let terrain = terrain.iter().next();
    let ground = terrain.map(|(config, base, baked)| (config, surface_data(base, baked)));
    let bake = terrain.and_then(|(_, _, baked)| baked);
    for entity in splines {
        let Some((id, enabled, layer)) = layers.spline(entity) else { continue };
        // A disabled spline is not in the bake, and one whose points moved
        // since the last hand-over is baked where they were.
        let carved = bake
            .and_then(|bake| bake.baked_spline(id))
            .filter(|(baked, _)| **baked == layer)
            .map(|(_, stations)| stations);
        draw_spline(&mut gizmos, &layer, enabled, carved, ground, eye);
    }
    for (point, _) in &selected_points {
        let at = layers.world_pose(point).translation;
        gizmos
            .sphere(Isometry3d::from_translation(at), marker_radius(eye, at) * GIZMO_SELECTED_POINT_SCALE, SELECTED_POINT_COLOR)
            .resolution(GIZMO_POINT_RESOLUTION);
    }
}

/// Draw one spline (see [`draw_spline_gizmos`]). `carved` is the stations the
/// bake carved it along, when they match it; `ground` the ground the view
/// shows.
fn draw_spline(
    gizmos: &mut Gizmos,
    layer: &SplineLayer,
    enabled: bool,
    carved: Option<&[Vec3]>,
    ground: Option<(&TerrainConfig, &TerrainData)>,
    eye: Vec3,
) {
    let (r, g, b) = if enabled { mode_rgb(layer.mode) } else { DISABLED_RGB };
    let strong = Color::srgba(r, g, b, 0.95);
    let soft = Color::srgba(r, g, b, 0.6);
    let faint = Color::srgba(r, g, b, 0.3);
    // The lifted ground height at `x, z`, or `fallback` without a terrain.
    let ground_at = |x: f32, z: f32, fallback: f32| {
        ground.map_or(fallback, |(config, data)| height_at_world(config, data, x, z)) + GIZMO_LIFT
    };

    // The profile runs through each point's own height, which may sit above
    // or below the ground; the stem shows by how much.
    for &point in &layer.points {
        gizmos
            .sphere(Isometry3d::from_translation(point), marker_radius(eye, point), strong)
            .resolution(GIZMO_POINT_RESOLUTION);
        gizmos.line(point, Vec3::new(point.x, ground_at(point.x, point.z, point.y), point.z), faint);
    }
    gizmos.linestrip(layer.points.iter().copied(), faint);

    let Some(path) = gizmo_path(layer, carved) else { return };
    let half = (layer.width * 0.5).max(0.0);
    let reach = half + layer.shoulder_width.max(0.0);
    // The ground `offset` metres to the left of a station (negative: right),
    // the side `build_ribbon_mesh` calls left.
    let beside = |station: &RoadStation, offset: f32| {
        let side = Vec2::new(-station.tangent.y, station.tangent.x) * offset;
        let (x, z) = (station.pos.x + side.x, station.pos.z + side.y);
        Vec3::new(x, ground_at(x, z, station.pos.y), z)
    };
    gizmos.linestrip(path.stations.iter().map(|station| beside(station, 0.0)), strong);
    for offset in [half, -half] {
        gizmos.linestrip(path.stations.iter().map(|station| beside(station, offset)), soft);
    }
    if reach > half {
        for offset in [reach, -reach] {
            gizmos.linestrip(path.stations.iter().map(|station| beside(station, offset)), faint);
        }
    }
    // Close both ends across the corridor, down into a carved bed and out.
    for station in [path.stations.first(), path.stations.last()].into_iter().flatten() {
        gizmos.linestrip([reach, half, 0.0, -half, -reach].map(|offset| beside(station, offset)), soft);
    }
}

/// The stations to draw a spline along: `carved` when the bake holds it,
/// else a path through its points whose profile runs through their heights
/// alone (the bake's extra knots read ground the preview cannot know), thinned
/// to at most [`GIZMO_MAX_STATIONS`]. `None` with fewer than two points.
fn gizmo_path(layer: &SplineLayer, carved: Option<&[Vec3]>) -> Option<RoadPath> {
    let preview: Vec<Vec3>;
    let stations: &[Vec3] = match carved {
        Some(stations) => stations,
        None => {
            let length: f32 = layer
                .points
                .windows(2)
                .map(|pair| Vec2::new(pair[1].x - pair[0].x, pair[1].z - pair[0].z).length())
                .sum();
            let spacing = (length / GIZMO_MAX_STATIONS as f32).max(GIZMO_PREVIEW_SPACING);
            // A knot spacing no stretch between two points reaches: no extra
            // knot, so the ground sampler is never called.
            let path = build_road_path_with(&layer.points, spacing, f32::MAX, |_, _| 0.0)?;
            preview = path.stations.iter().map(|station| station.pos).collect();
            &preview
        }
    };
    let step = stations.len().div_ceil(GIZMO_MAX_STATIONS).max(1);
    let mut thinned: Vec<Vec3> = stations.iter().copied().step_by(step).collect();
    // The line always reaches the spline's end.
    if let Some(&last) = stations.last() {
        if thinned.last() != Some(&last) {
            thinned.push(last);
        }
    }
    RoadPath::from_positions(&thinned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_layer_classes_are_planned_and_layers_go_under_the_terrain() {
        let root = Path::new("Space");
        let no_cameras: Vec<(&Camera, &GlobalTransform)> = Vec::new();
        assert!(plan_insert("Part", root, None, no_cameras.clone(), None).is_none());
        let plan = plan_insert("TerrainStamp", root, None, no_cameras.clone(), None).expect("a layer").expect("planned");
        assert_eq!(plan.dir, layers_dir(root));
        assert!(plan.spline_points.is_empty());
        let spline = plan_insert("TerrainSpline", root, None, no_cameras.clone(), None).expect("a layer").expect("planned");
        assert_eq!(spline.spline_points.len(), 2, "a new spline starts with two points");
        let point = plan_insert("TerrainSplinePoint", root, None, no_cameras, None).expect("a layer class");
        assert!(point.is_err(), "a point needs a spline selected");
    }

    #[test]
    fn a_spline_gizmo_follows_its_carved_line_or_its_points_thinned() {
        let layer = SplineLayer {
            points: vec![Vec3::new(0.0, 1.0, 0.0), Vec3::new(100.0, 5.0, 0.0)],
            width: 8.0,
            shoulder_width: 4.0,
            ..SplineLayer::default()
        };
        // Through the points: it starts and ends on them, at their heights.
        let preview = gizmo_path(&layer, None).expect("two points make a path");
        let first = preview.stations.first().expect("a station").pos;
        let last = preview.stations.last().expect("a station").pos;
        assert!((first - layer.points[0]).length() < 1e-3, "starts at {first}");
        assert!((last - layer.points[1]).length() < 1e-3, "ends at {last}");

        // A long carved line is thinned and still reaches its end.
        let carved: Vec<Vec3> = (0..=2000).map(|i| Vec3::new(i as f32 * 0.5, 0.0, 0.0)).collect();
        let thinned = gizmo_path(&layer, Some(carved.as_slice())).expect("a path");
        assert!(thinned.stations.len() <= GIZMO_MAX_STATIONS + 1, "{} stations", thinned.stations.len());
        assert_eq!(thinned.stations.last().map(|s| s.pos), carved.last().copied());

        let lone = SplineLayer { points: vec![Vec3::ZERO], ..layer.clone() };
        assert!(gizmo_path(&lone, None).is_none(), "one point is no path");
    }

    #[test]
    fn a_new_point_extends_the_spline_past_its_last_point() {
        assert_eq!(next_point_position(&[]), Vec3::ZERO);
        assert_eq!(next_point_position(&[(0.0, Vec3::new(1.0, 2.0, 3.0))]), Vec3::new(21.0, 2.0, 3.0));
        let along_z = [(0.0, Vec3::new(5.0, 0.0, 0.0)), (1.0, Vec3::new(5.0, 4.0, 10.0))];
        assert_eq!(next_point_position(&along_z), Vec3::new(5.0, 4.0, 30.0));
    }
}
