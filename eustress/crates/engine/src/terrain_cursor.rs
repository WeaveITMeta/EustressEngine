//! What the terrain tools draw at the cursor (see
//! `docs/design/TERRAIN_TOOLS_UX.md`, section 5): the footprint ring,
//! half-push ring and strength disc on the ground; the Draw volume's
//! wireframe, with an always-on-top copy showing the part inside the ground;
//! the locked plane; the local grid and height contours; the mirror planes
//! and ghost brushes; the stroke smoothing leash; Sea Level's rectangle,
//! water plane and shoreline; and the readout the Slint overlay shows beside
//! the cursor ([`TerrainCursorReadout`]).
//!
//! Everything is mesh-based: the Studio does not build Bevy's gizmo renderer
//! (`bevy_gizmos_render`), so `Gizmos` lines are recorded and never drawn.
//! Three meshes carry it all, each with per-vertex colours on the editor
//! overlay render layer (`view_grid::VIEW_GRID_LAYER`), so the AI camera's
//! captures never include them: depth-tested lines, always-on-top lines, and
//! blended triangles. The geometry comes from the pure builders in
//! `eustress_common::terrain::brush_cursor`, over the same hover and dab the
//! stroke uses, so what the cursor shows is where the dab lands.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use eustress_common::terrain::brush_cursor::{
    drape_loop, flat_loop, half_push_fraction, local_grid, mirror_lines, plane_disc, sea_level_box, strength_disc,
    contours, volume_wireframe, AlphaMesh, FadeSegment, GROUND_LIFT,
};
use eustress_common::terrain::{
    ground_at_world, height_at_world, material_at_world, mirror_centres, surface_data, BrushAction, BrushDab,
    BrushFamily, CsgShape, MirrorAxes, RegionMode, SeaLevelMode, TerrainBaked, TerrainBrush, TerrainBrushHover,
    TerrainConfig, TerrainData, TerrainMaterialSlots, TerrainMode, TerrainRoot, TerrainStroke,
};
use eustress_common::units::{convert_f32, DisplayUnit, Unit};

use crate::adornment_renderer::{AdornmentMaterial, AlwaysOnTopExtension};
use crate::terrain_region::{region_readout, RegionBox, RegionFace, RegionTool, RegionTransform};
use crate::terrain_sea_level::{sea_level_readout, SeaLevelTool};
use crate::view_grid::VIEW_GRID_LAYER;

/// Draws the terrain tools' cursor and keeps [`TerrainCursorReadout`].
pub struct TerrainCursorPlugin;

impl Plugin for TerrainCursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainCursorReadout>().add_systems(
            Update,
            update_terrain_cursor
                .after(eustress_common::terrain::terrain_paint_system)
                .after(eustress_common::terrain::apply_terrain_dirty_chunks),
        );
    }
}

/// What the readout beside the cursor shows (design section 5.1), for the
/// Slint overlay to draw.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct TerrainCursorReadout {
    /// The pill is shown: the terrain tools are on and the cursor is on the
    /// terrain or the locked plane, over the viewport.
    pub visible: bool,
    /// Cursor position in logical pixels from the viewport's top left.
    pub x: f32,
    pub y: f32,
    /// The line of text, e.g. `Grow · 16 m · 40% · Y 23.4 m · Grass`.
    pub text: String,
    /// The tool colour group, for the pill's accent dot.
    pub family: Option<BrushFamily>,
    pub plane: bool,
    pub snap: bool,
    pub contours: bool,
    pub mirror: bool,
}

/// The sRGB colour of a tool family: the theme token design section 5
/// assigns it (`cat-structure`, `accent-orange`, `accent-cyan`,
/// `cat-modify`, `text-accent`, `cat-parts`, Classic values).
pub fn family_rgb(family: BrushFamily) -> [f32; 3] {
    let hex = match family {
        BrushFamily::Build => 0x81c784,
        BrushFamily::Cut => 0xe8912d,
        BrushFamily::Shape => 0x00bcd4,
        BrushFamily::Surface => 0xba68c8,
        BrushFamily::Water => 0x4fc1ff,
        BrushFamily::Region => 0x64b5f6,
    };
    [((hex >> 16) & 0xff) as f32 / 255.0, ((hex >> 8) & 0xff) as f32 / 255.0, (hex & 0xff) as f32 / 255.0]
}

/// The locked plane's colour: selection cyan (`accent-cyan`).
const PLANE_RGB: [f32; 3] = [0.0, 0.737, 0.831];
const WHITE: [f32; 3] = [1.0, 1.0, 1.0];

/// Which of the three cursor meshes an entity carries.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum CursorLayer {
    /// Depth-tested lines: rings, grid, contours, wireframe, plane rim.
    Lines,
    /// Always-on-top lines: the wireframe's see-through copy.
    XRay,
    /// Blended triangles: the strength disc and the plane fill.
    Fills,
}

/// A line list or triangle list being built, with per-vertex colours.
#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Batch {
    fn line(&mut self, a: Vec3, b: Vec3, rgb: [f32; 3], alpha_a: f32, alpha_b: f32) {
        self.positions.push(a.to_array());
        self.positions.push(b.to_array());
        self.colors.push([rgb[0], rgb[1], rgb[2], alpha_a]);
        self.colors.push([rgb[0], rgb[1], rgb[2], alpha_b]);
    }

    fn segments(&mut self, segments: &[[Vec3; 2]], rgb: [f32; 3], alpha: f32) {
        for [a, b] in segments {
            self.line(*a, *b, rgb, alpha, alpha);
        }
    }

    fn faded(&mut self, segments: &[FadeSegment], rgb: [f32; 3]) {
        for s in segments {
            self.line(s.a, s.b, rgb, s.alpha_a, s.alpha_b);
        }
    }

    fn triangles(&mut self, mesh: &AlphaMesh, rgb: [f32; 3]) {
        let base = self.positions.len() as u32;
        for (p, a) in mesh.positions.iter().zip(&mesh.alphas) {
            self.positions.push(p.to_array());
            self.colors.push([rgb[0], rgb[1], rgb[2], *a]);
        }
        self.indices.extend(mesh.indices.iter().map(|i| base + i));
    }

    fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Write `batch` into `mesh` (keeping its topology), or report it empty.
fn upload(mesh: &mut Mesh, batch: Batch, triangles: bool) -> bool {
    if batch.is_empty() {
        return false;
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, batch.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, batch.colors);
    if triangles {
        mesh.insert_indices(Indices::U32(batch.indices));
    }
    true
}

/// The three cursor entities and their meshes, spawned on first use, and
/// which of them held geometry at the last rebuild.
#[derive(Default)]
pub struct CursorEntities {
    spawned: Option<[(Entity, Handle<Mesh>); 3]>,
    shown: [bool; 3],
}

/// The index of `layer` among the three cursor meshes.
fn layer_index(layer: CursorLayer) -> usize {
    match layer {
        CursorLayer::Lines => 0,
        CursorLayer::XRay => 1,
        CursorLayer::Fills => 2,
    }
}

/// Show the cursor meshes that hold geometry and hide the rest.
fn apply_visibility(visibility: &mut Query<(&CursorLayer, &mut Visibility)>, shown: [bool; 3]) {
    for (layer, mut visible) in visibility.iter_mut() {
        let want = if shown[layer_index(*layer)] { Visibility::Visible } else { Visibility::Hidden };
        if *visible != want {
            *visible = want;
        }
    }
}

fn spawn_cursor_entities(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    xray_materials: &mut Assets<AdornmentMaterial>,
) -> [(Entity, Handle<Mesh>); 3] {
    let base = StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    };
    let empty = |topology: PrimitiveTopology| {
        Mesh::new(topology, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3])
    };
    let common = || {
        (
            Transform::IDENTITY,
            Visibility::Hidden,
            RenderLayers::layer(VIEW_GRID_LAYER),
            NoFrustumCulling,
            NotShadowCaster,
            NotShadowReceiver,
            eustress_common::adornments::Adornment { meta: true },
        )
    };

    let lines_mesh = meshes.add(empty(PrimitiveTopology::LineList));
    let lines = commands
        .spawn((
            Name::new("Terrain Cursor Lines"),
            CursorLayer::Lines,
            Mesh3d(lines_mesh.clone()),
            MeshMaterial3d(materials.add(base.clone())),
            common(),
        ))
        .id();

    let xray_mesh = meshes.add(empty(PrimitiveTopology::LineList));
    let xray = commands
        .spawn((
            Name::new("Terrain Cursor X-Ray"),
            CursorLayer::XRay,
            Mesh3d(xray_mesh.clone()),
            MeshMaterial3d(xray_materials.add(ExtendedMaterial { base: base.clone(), extension: AlwaysOnTopExtension {} })),
            common(),
        ))
        .id();

    let fills_mesh = meshes.add(
        empty(PrimitiveTopology::TriangleList).with_inserted_indices(Indices::U32(vec![0, 1, 2])),
    );
    let fills = commands
        .spawn((
            Name::new("Terrain Cursor Fills"),
            CursorLayer::Fills,
            Mesh3d(fills_mesh.clone()),
            MeshMaterial3d(materials.add(base)),
            common(),
        ))
        .id();

    [(lines, lines_mesh), (xray, xray_mesh), (fills, fills_mesh)]
}

/// A length in the display unit, compact: whole units from 100, one
/// decimal from 10, two below, trailing zeros dropped.
pub fn compact_length(meters: f32, unit: Unit) -> String {
    let value = convert_f32(meters, Unit::Meter, unit);
    let magnitude = value.abs();
    let text = if magnitude >= 100.0 {
        format!("{value:.0}")
    } else if magnitude >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text };
    format!("{text} {}", unit.symbol())
}

/// The name of material slot `slot`, or "Material N" for one the table does
/// not define.
fn material_name(slots: Option<&TerrainMaterialSlots>, slot: u8) -> String {
    slots.and_then(|slots| slots.get(slot)).map_or_else(|| format!("Material {slot}"), |def| def.name.clone())
}

/// Rebuild the cursor meshes and the readout from this frame's hover,
/// stroke and brush. Hidden whenever the terrain tools are off or the
/// cursor is off the terrain and the plane.
#[allow(clippy::too_many_arguments)]
pub fn update_terrain_cursor(
    mut commands: Commands,
    mode: Res<TerrainMode>,
    hover: Res<TerrainBrushHover>,
    stroke: Res<TerrainStroke>,
    brush: Res<TerrainBrush>,
    terrain: Query<(Ref<TerrainData>, &TerrainConfig, Option<&TerrainBaked>), With<TerrainRoot>>,
    (slots, display_unit, sea_level, region): (
        Option<Res<TerrainMaterialSlots>>,
        Option<Res<DisplayUnit>>,
        Option<Res<SeaLevelTool>>,
        Option<Res<RegionTool>>,
    ),
    (windows, viewport_bounds): (
        Query<&Window, With<PrimaryWindow>>,
        Option<Res<crate::ui::ViewportBounds>>,
    ),
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut xray_materials: ResMut<Assets<AdornmentMaterial>>,
    mut visibility: Query<(&CursorLayer, &mut Visibility)>,
    mut readout: ResMut<TerrainCursorReadout>,
    mut entities: Local<CursorEntities>,
) {
    let editing = *mode == TerrainMode::Editor;
    let terrain = terrain.single().ok();

    // Sea Level draws its rectangle and water plane, not a brush.
    let water_mode = match hover.action {
        Some(BrushAction::SeaLevel(water_mode)) if editing => Some(water_mode),
        _ => None,
    };
    if let (Some(water_mode), Some(tool)) = (water_mode, sea_level.as_ref()) {
        let Some(terrain) = terrain else {
            apply_visibility(&mut visibility, [false; 3]);
            if readout.visible {
                readout.visible = false;
            }
            return;
        };
        let unit = display_unit.as_deref().map_or(Unit::Meter, DisplayUnit::get);
        let anchor = readout_anchor(&windows, viewport_bounds.as_deref());
        update_sea_level_cursor(
            &mut commands,
            water_mode,
            tool,
            terrain,
            &hover,
            &brush,
            unit,
            anchor,
            (&mut *meshes, &mut *materials, &mut *xray_materials),
            &mut visibility,
            &mut readout,
            &mut entities,
        );
        return;
    }

    // Region draws its box, handles, paste ghost and transform target.
    let region_mode = match hover.action {
        Some(BrushAction::Region(region_mode)) if editing => Some(region_mode),
        _ => None,
    };
    if let (Some(region_mode), Some(tool)) = (region_mode, region.as_ref()) {
        let Some(terrain) = terrain else {
            apply_visibility(&mut visibility, [false; 3]);
            if readout.visible {
                readout.visible = false;
            }
            return;
        };
        let unit = display_unit.as_deref().map_or(Unit::Meter, DisplayUnit::get);
        let anchor = readout_anchor(&windows, viewport_bounds.as_deref());
        update_region_cursor(
            &mut commands,
            region_mode,
            tool,
            terrain,
            &hover,
            &brush,
            unit,
            anchor,
            (&mut *meshes, &mut *materials, &mut *xray_materials),
            &mut visibility,
            &mut readout,
            &mut entities,
        );
        return;
    }

    let action = hover.action.filter(|action| action.is_brush());
    let target = stroke.active.as_ref().and_then(|open| open.centre).or(hover.target);
    let showing = editing && action.is_some() && target.is_some() && terrain.is_some();

    // Nothing to draw: hide what is shown and stop, leaving the meshes as
    // they are for the next time the cursor lands on the ground.
    if !showing {
        apply_visibility(&mut visibility, [false; 3]);
        if readout.visible {
            readout.visible = false;
        }
        return;
    }
    let (Some(action), Some(target), Some((data, config, baked))) = (action, target, terrain) else { return };

    let unit = display_unit.as_deref().map_or(Unit::Meter, DisplayUnit::get);
    let surface = surface_data(&data, baked);
    update_readout(
        &mut readout,
        &hover,
        &brush,
        action,
        target,
        config,
        surface,
        slots.as_deref(),
        unit,
        &windows,
        viewport_bounds.as_deref(),
    );

    // Rebuild only when something the cursor shows changed, or on the first
    // frame the meshes exist. Otherwise show again what the last rebuild
    // drew (the tools were left and re-entered with the cursor in place).
    let first = entities.spawned.is_none();
    let changed = first || hover.is_changed() || stroke.is_changed() || brush.is_changed() || data.is_changed();
    let handles = entities
        .spawned
        .get_or_insert_with(|| spawn_cursor_entities(&mut commands, &mut meshes, &mut materials, &mut xray_materials))
        .clone();
    if !changed {
        apply_visibility(&mut visibility, entities.shown);
        return;
    }
    let Some(dab) = stroke.active.as_ref().map(|open| open.dab).or_else(|| brush.dab(action)) else { return };

    let ground = |p: Vec2| ground_height(config, surface, p);
    let batches = build_cursor(&brush, &dab, action, target, &hover, &stroke, &ground);
    show_batches(&mut meshes, &handles, &mut entities, &mut visibility, batches);
}

/// Sea Level's part of [`update_terrain_cursor`]: its readout and its
/// rectangle, plane and shore ([`build_sea_level`]), rebuilt when the hover,
/// the tool, the brush or the ground changed.
#[allow(clippy::too_many_arguments)]
fn update_sea_level_cursor(
    commands: &mut Commands,
    water_mode: SeaLevelMode,
    tool: &Res<SeaLevelTool>,
    (data, config, baked): (Ref<TerrainData>, &TerrainConfig, Option<&TerrainBaked>),
    hover: &Res<TerrainBrushHover>,
    brush: &Res<TerrainBrush>,
    unit: Unit,
    anchor: Option<(f32, f32)>,
    (meshes, materials, xray_materials): (&mut Assets<Mesh>, &mut Assets<StandardMaterial>, &mut Assets<AdornmentMaterial>),
    visibility: &mut Query<(&CursorLayer, &mut Visibility)>,
    readout: &mut TerrainCursorReadout,
    entities: &mut CursorEntities,
) {
    match anchor {
        Some((x, y)) => {
            let next = TerrainCursorReadout {
                visible: true,
                x,
                y,
                text: sea_level_readout(tool, water_mode, unit),
                family: Some(BrushFamily::Water),
                plane: false,
                snap: brush.snap,
                contours: brush.contours,
                mirror: false,
            };
            if *readout != next {
                *readout = next;
            }
        }
        None => {
            if readout.visible {
                readout.visible = false;
            }
        }
    }

    let first = entities.spawned.is_none();
    let changed = first || hover.is_changed() || tool.is_changed() || brush.is_changed() || data.is_changed();
    let handles = entities
        .spawned
        .get_or_insert_with(|| spawn_cursor_entities(&mut *commands, &mut *meshes, &mut *materials, &mut *xray_materials))
        .clone();
    if !changed {
        apply_visibility(visibility, entities.shown);
        return;
    }
    let surface = surface_data(&data, baked);
    let ground = |p: Vec2| ground_height(config, surface, p);
    let batches = build_sea_level(tool, brush, hover, &ground);
    show_batches(meshes, &handles, entities, visibility, batches);
}

/// The Region tool's batches: the box (dimmed while a Transform shows its
/// target) with a see-through copy, its face handles (cyan under the
/// cursor), a Transform's target box and a line to it, the paste ghost, and
/// a mark at the cursor before a box is drawn.
fn build_region(
    tool: &RegionTool,
    brush: &TerrainBrush,
    hover: &TerrainBrushHover,
    ground: &dyn Fn(Vec2) -> Option<f32>,
) -> (Batch, Batch, Batch) {
    let (mut lines, mut xray, mut fills) = (Batch::default(), Batch::default(), Batch::default());
    let rgb = family_rgb(BrushFamily::Region);
    let wire = |region: &RegionBox| {
        volume_wireframe(&CsgShape::AxisBox { center: region.center(), half_extents: region.size() * 0.5 }, None)
    };
    let target = tool.target().filter(|_| tool.transform.is_some_and(|t| t != RegionTransform::default()));
    if let Some(region) = tool.region {
        let edges = wire(&region);
        lines.segments(&edges, rgb, if target.is_some() { 0.35 } else { 0.9 });
        xray.segments(&edges, rgb, 0.25);
        if tool.transform.is_none() && tool.paste_at.is_none() {
            let half = region.handle_size() * 0.5;
            for face in RegionFace::ALL {
                let (axis, _) = face.axis();
                let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
                let center = region.face_center(face) + face.normal() * 0.03;
                let offset = |sb: f32, sc: f32| {
                    let mut p = center;
                    p[b] += sb * half;
                    p[c] += sc * half;
                    p
                };
                let corners = [offset(-1.0, -1.0), offset(1.0, -1.0), offset(1.0, 1.0), offset(-1.0, 1.0)];
                let hovered = tool.hovered_face == Some(face);
                let (colour, alpha) = if hovered { (PLANE_RGB, 0.75) } else { (rgb, 0.4) };
                fills.triangles(
                    &AlphaMesh { positions: corners.to_vec(), alphas: vec![alpha; 4], indices: vec![0, 1, 2, 0, 2, 3] },
                    colour,
                );
                for i in 0..4 {
                    lines.line(corners[i], corners[(i + 1) % 4], colour, 0.9, 0.9);
                }
            }
        }
    }
    if let (Some(target), Some(region)) = (target, tool.region) {
        let edges = wire(&target);
        lines.segments(&edges, rgb, 1.0);
        xray.segments(&edges, rgb, 0.35);
        lines.line(region.center(), target.center(), WHITE, 0.4, 0.4);
    }
    if let Some(ghost) = tool.paste_box() {
        let edges = wire(&ghost);
        lines.segments(&edges, WHITE, 0.8);
        xray.segments(&edges, WHITE, 0.3);
    }
    if let (None, None, Some(hit), false) = (tool.region, tool.paste_at, hover.surface, tool.dragging()) {
        let step = brush.active_snap();
        let snap = |v: f32| step.map_or(v, |step| (v / step).round() * step);
        let at = Vec2::new(snap(hit.x), snap(hit.z));
        let mark = Vec3::new(at.x, ground(at).unwrap_or(hit.y) + GROUND_LIFT, at.y);
        lines.line(mark - Vec3::X * 0.6, mark + Vec3::X * 0.6, WHITE, 0.7, 0.7);
        lines.line(mark - Vec3::Z * 0.6, mark + Vec3::Z * 0.6, WHITE, 0.7, 0.7);
        if brush.snap {
            let patch = (brush.snap_step * 12.0).clamp(4.0, 64.0);
            lines.faded(&local_grid(at, patch, brush.snap_step, None, 0.35, 0.7, ground), WHITE);
        }
    }
    (lines, xray, fills)
}

/// Region's part of [`update_terrain_cursor`], as [`update_sea_level_cursor`]
/// is Sea Level's.
#[allow(clippy::too_many_arguments)]
fn update_region_cursor(
    commands: &mut Commands,
    region_mode: RegionMode,
    tool: &Res<RegionTool>,
    (data, config, baked): (Ref<TerrainData>, &TerrainConfig, Option<&TerrainBaked>),
    hover: &Res<TerrainBrushHover>,
    brush: &Res<TerrainBrush>,
    unit: Unit,
    anchor: Option<(f32, f32)>,
    (meshes, materials, xray_materials): (&mut Assets<Mesh>, &mut Assets<StandardMaterial>, &mut Assets<AdornmentMaterial>),
    visibility: &mut Query<(&CursorLayer, &mut Visibility)>,
    readout: &mut TerrainCursorReadout,
    entities: &mut CursorEntities,
) {
    match anchor {
        Some((x, y)) => {
            let next = TerrainCursorReadout {
                visible: true,
                x,
                y,
                text: region_readout(tool, region_mode, unit),
                family: Some(BrushFamily::Region),
                plane: false,
                snap: brush.snap,
                contours: false,
                mirror: false,
            };
            if *readout != next {
                *readout = next;
            }
        }
        None => {
            if readout.visible {
                readout.visible = false;
            }
        }
    }

    let first = entities.spawned.is_none();
    let changed = first || hover.is_changed() || tool.is_changed() || brush.is_changed() || data.is_changed();
    let handles = entities
        .spawned
        .get_or_insert_with(|| spawn_cursor_entities(&mut *commands, &mut *meshes, &mut *materials, &mut *xray_materials))
        .clone();
    if !changed {
        apply_visibility(visibility, entities.shown);
        return;
    }
    let surface = surface_data(&data, baked);
    let ground = |p: Vec2| ground_height(config, surface, p);
    let batches = build_region(tool, brush, hover, &ground);
    show_batches(meshes, &handles, entities, visibility, batches);
}

/// The ground's height at world XZ `p`, `None` off the terrain's footprint
/// or over a hole.
fn ground_height(config: &TerrainConfig, surface: &TerrainData, p: Vec2) -> Option<f32> {
    let (lo, hi) = config.footprint_xz();
    (p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y && ground_at_world(config, surface, p.x, p.y))
        .then(|| height_at_world(config, surface, p.x, p.y))
}

/// Upload the three batches into the cursor meshes, and show the ones that
/// hold geometry.
fn show_batches(
    meshes: &mut Assets<Mesh>,
    handles: &[(Entity, Handle<Mesh>); 3],
    entities: &mut CursorEntities,
    visibility: &mut Query<(&CursorLayer, &mut Visibility)>,
    (lines, xray, fills): (Batch, Batch, Batch),
) {
    let mut set = |index: usize, batch: Batch, triangles: bool| -> bool {
        let (_, handle) = &handles[index];
        match meshes.get_mut(handle) {
            Some(mut mesh) => upload(&mut mesh, batch, triangles),
            None => false,
        }
    };
    let shown = [set(0, lines, false), set(1, xray, false), set(2, fills, true)];
    entities.shown = shown;
    apply_visibility(visibility, shown);
}

/// The cursor's three batches (see [`CursorLayer`]) for `action` at `target`.
fn build_cursor(
    brush: &TerrainBrush,
    dab: &BrushDab,
    action: BrushAction,
    target: Vec3,
    hover: &TerrainBrushHover,
    stroke: &TerrainStroke,
    ground: &dyn Fn(Vec2) -> Option<f32>,
) -> (Batch, Batch, Batch) {
    let (mut lines, mut xray, mut fills) = (Batch::default(), Batch::default(), Batch::default());
    let rgb = family_rgb(action.family());
    let stroking = stroke.active.is_some();
    let ring_alpha = if stroking { 0.9 } else { 0.7 };
    let plane = brush.plane_lock;
    let center = target.xz();

    // A footprint outline at `scale` of the radius around `c`, on the plane
    // when it is locked, else draped on the ground.
    let outline = |c: Vec2, scale: f32| -> Vec<[Vec3; 2]> {
        let points = dab.footprint(c).outline(scale);
        match plane {
            Some(y) => flat_loop(&points, y + GROUND_LIFT),
            None => drape_loop(&points, ground),
        }
    };

    // The footprint ring, the half-push ring and the strength disc (the
    // surface brushes; Draw shows its volume instead, with the ring as its
    // shadow on the ground).
    lines.segments(&outline(center, 1.0), rgb, ring_alpha);
    if !action.is_volume() {
        let half = half_push_fraction(dab.falloff);
        if half < 0.999 {
            lines.segments(&outline(center, half), rgb, 0.4);
        }
        fills.triangles(&strength_disc(dab.footprint(center), dab.strength, dab.falloff, plane, ground), rgb);
    }

    // The centre mark: a small cross on the ground and, for Draw, a tick up
    // to the volume's centre.
    let cross = (dab.radius * 0.08).clamp(0.15, 1.5);
    let base_y = plane.map_or_else(|| ground(center).unwrap_or(target.y), |y| y) + GROUND_LIFT;
    let at = Vec3::new(center.x, base_y, center.y);
    lines.line(at - Vec3::X * cross, at + Vec3::X * cross, WHITE, 0.6, 0.6);
    lines.line(at - Vec3::Z * cross, at + Vec3::Z * cross, WHITE, 0.6, 0.6);

    // Draw: the exact volume it adds or carves, clipped to the plane, plus a
    // see-through copy for the part inside the ground.
    let clip = dab.clip().map(|clip| (clip.y, clip.keep_below));
    if action.is_volume() {
        let wire = volume_wireframe(&dab.volume_shape(target), clip);
        lines.segments(&wire, rgb, 0.9);
        xray.segments(&wire, rgb, 0.25);
        let middle = dab.volume_center(target);
        lines.line(at, middle, WHITE, 0.6, 0.3);
    }

    // The locked plane.
    if let Some(y) = plane {
        let (fill, rim, grid) = plane_disc(center, y, dab.radius * 3.0, brush.snap_step, 0.12);
        fills.triangles(&fill, PLANE_RGB);
        lines.segments(&rim, PLANE_RGB, 0.6);
        lines.segments(&grid, PLANE_RGB, 0.22);
    }

    // The local grid while snap is on, and the contours.
    let patch = dab.radius * 2.5;
    if brush.snap {
        lines.faded(&local_grid(center, patch, brush.snap_step, plane, 0.35, 0.7, ground), WHITE);
    }
    if brush.contours {
        lines.faded(&contours(center, patch, brush.snap_step, 0.3, 0.6, ground), WHITE);
    }

    // Mirror planes and ghost brushes at the mirrored centres.
    if brush.mirror != MirrorAxes::Off {
        let reach = dab.radius * 3.0;
        lines.segments(
            &mirror_lines(center, brush.mirror_origin, reach, brush.mirror.mirror_x(), brush.mirror.mirror_z(), ground),
            rgb,
            0.5,
        );
        for ghost in mirror_centres(center, brush.mirror_origin, brush.mirror).into_iter().skip(1) {
            lines.segments(&outline(ghost, 1.0), rgb, ring_alpha * 0.5);
            if action.is_volume() {
                let y = plane.or_else(|| ground(ghost)).unwrap_or(target.y);
                let wire = volume_wireframe(&dab.volume_shape(Vec3::new(ghost.x, y, ghost.y)), clip);
                lines.segments(&wire, rgb, 0.45);
            }
        }
    }

    // The smoothing leash, from where the cursor points to where the brush
    // trails it.
    if let (Some(open), Some(pointer)) = (stroke.active.as_ref(), hover.target) {
        if let Some(centre) = open.centre {
            if centre.distance(pointer) > 1e-3 {
                lines.line(pointer + Vec3::Y * GROUND_LIFT, centre + Vec3::Y * GROUND_LIFT, WHITE, 0.4, 0.4);
            }
        }
    }
    (lines, xray, fills)
}

/// Fill the readout for `action` at `target` (design section 5.1).
#[allow(clippy::too_many_arguments)]
fn update_readout(
    readout: &mut TerrainCursorReadout,
    hover: &TerrainBrushHover,
    brush: &TerrainBrush,
    action: BrushAction,
    target: Vec3,
    config: &TerrainConfig,
    surface: &TerrainData,
    slots: Option<&TerrainMaterialSlots>,
    unit: Unit,
    windows: &Query<&Window, With<PrimaryWindow>>,
    viewport_bounds: Option<&crate::ui::ViewportBounds>,
) {
    let Some((x, y)) = readout_anchor(windows, viewport_bounds) else {
        if readout.visible {
            readout.visible = false;
        }
        return;
    };

    let mut parts = vec![action.label().to_string(), compact_length(brush.size(), unit)];
    if !action.is_volume() {
        parts.push(format!("{:.0}%", brush.strength() * 100.0));
    }
    parts.push(format!("Y {}", compact_length(target.y, unit)));
    let ground_material = hover
        .surface
        .and_then(|p| material_at_world(config, surface, p.x, p.z))
        .map(|sample| material_name(slots, sample.primary));
    match action {
        BrushAction::Add | BrushAction::Paint => parts.push(material_name(slots, brush.paint_material)),
        BrushAction::Replace => parts.push(format!(
            "{} to {}",
            material_name(slots, brush.source_material),
            material_name(slots, brush.paint_material)
        )),
        _ => parts.extend(ground_material),
    }

    let next = TerrainCursorReadout {
        visible: true,
        x,
        y,
        text: parts.join(" · "),
        family: Some(action.family()),
        plane: brush.plane_lock.is_some(),
        snap: brush.snap,
        contours: brush.contours,
        mirror: brush.mirror != MirrorAxes::Off,
    };
    if *readout != next {
        *readout = next;
    }
}

/// The cursor in logical pixels from the viewport's top left, `None` off the
/// window.
fn readout_anchor(
    windows: &Query<&Window, With<PrimaryWindow>>,
    viewport_bounds: Option<&crate::ui::ViewportBounds>,
) -> Option<(f32, f32)> {
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let scale = window.scale_factor().max(1e-4);
    let (ox, oy) = viewport_bounds.map_or((0.0, 0.0), |bounds| (bounds.x / scale, bounds.y / scale));
    Some((cursor.x - ox, cursor.y - oy))
}

/// The Sea Level tool's batches: its rectangle draped on the ground and,
/// with a level, the water plane (a see-through copy of its rim shows it
/// through hills), the corner posts and the shoreline the level would make;
/// a mark at the cursor while no drag is open, and the local grid and
/// contours around it when they are on.
fn build_sea_level(
    tool: &SeaLevelTool,
    brush: &TerrainBrush,
    hover: &TerrainBrushHover,
    ground: &dyn Fn(Vec2) -> Option<f32>,
) -> (Batch, Batch, Batch) {
    let (mut lines, mut xray, mut fills) = (Batch::default(), Batch::default(), Batch::default());
    let rgb = family_rgb(BrushFamily::Water);
    if let Some((lo, hi)) = tool.rect {
        let shown = sea_level_box(lo, hi, tool.level, 0.2, ground);
        lines.segments(&shown.outline, rgb, 0.9);
        lines.segments(&shown.rim, rgb, if tool.dragging_level() { 1.0 } else { 0.8 });
        lines.segments(&shown.posts, rgb, 0.6);
        lines.segments(&shown.shoreline, WHITE, 0.7);
        xray.segments(&shown.rim, rgb, 0.3);
        fills.triangles(&shown.fill, rgb);
    }
    if let (Some(hit), false) = (hover.surface, tool.dragging()) {
        let step = brush.active_snap();
        let snap = |v: f32| step.map_or(v, |step| (v / step).round() * step);
        let at = Vec2::new(snap(hit.x), snap(hit.z));
        let mark = Vec3::new(at.x, ground(at).unwrap_or(hit.y) + GROUND_LIFT, at.y);
        let arm = 0.6;
        lines.line(mark - Vec3::X * arm, mark + Vec3::X * arm, WHITE, 0.7, 0.7);
        lines.line(mark - Vec3::Z * arm, mark + Vec3::Z * arm, WHITE, 0.7, 0.7);
        let patch = (brush.snap_step * 12.0).clamp(4.0, 64.0);
        if brush.snap {
            lines.faded(&local_grid(at, patch, brush.snap_step, None, 0.35, 0.7, ground), WHITE);
        }
        if brush.contours {
            lines.faded(&contours(at, patch, brush.snap_step, 0.3, 0.6, ground), WHITE);
        }
    }
    (lines, xray, fills)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_lengths_drop_needless_digits() {
        assert_eq!(compact_length(16.0, Unit::Meter), "16 m");
        assert_eq!(compact_length(23.42, Unit::Meter), "23.4 m");
        assert_eq!(compact_length(0.5, Unit::Meter), "0.5 m");
        assert_eq!(compact_length(128.4, Unit::Meter), "128 m");
    }

    #[test]
    fn family_colours_are_the_theme_tokens() {
        assert_eq!(family_rgb(BrushFamily::Shape), [0.0, 188.0 / 255.0, 212.0 / 255.0]);
        let build = family_rgb(BrushFamily::Build);
        assert!((build[0] - 0x81 as f32 / 255.0).abs() < 1e-6);
    }
}
