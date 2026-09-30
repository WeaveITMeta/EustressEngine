//! The Region tool (`docs/design/TERRAIN_TOOLS_UX.md`, sections 4.4 and
//! 10.1): a box of terrain to copy, cut, paste, duplicate, delete, move,
//! turn and fill.
//!
//! - **Select.** A left drag on the ground draws the box's footprint, snapped
//!   to the grid step when snap is on and to the volume lattice otherwise;
//!   its height spans the ground inside it plus a margin. A face handle
//!   dragged moves that face along its axis (on a plane through the axis
//!   facing the camera). A click without a drag clears the box. `Ctrl+C`
//!   copies what the box holds (`read_voxels` at the lattice spacing),
//!   `Ctrl+X` copies then empties it, `Ctrl+V` shows the copy as a ghost box
//!   under the cursor that a click places (`WriteVoxels`), `Ctrl+D` copies it
//!   one box width along X and selects the copy, and `Delete` empties it
//!   (`FillRegion` of Air). The bar's buttons do the same.
//! - **Transform.** The box's contents move with it: a left drag on the box
//!   slides it over the ground plane at its top, `PageUp`/`PageDown` raise
//!   and lower it by the snap step, and the bar's Rotate turns it a quarter
//!   turn about Y. `Enter` (or Apply) reads the source, empties it and writes
//!   the contents at the target as one undo entry; `Esc` (or Cancel) puts the
//!   box back.
//! - **Fill.** Fill fills the box with the brush's material; Replace swaps
//!   the source material for it inside the box.
//!
//! Every edit goes through the terrain API as one labelled undo entry, so
//! water comes and goes with the voxels it stands in. `terrain_cursor` draws
//! the box, its handles, the paste ghost and the transform target.

use bevy::ecs::schedule::common_conditions::resource_equals;
use bevy::prelude::*;
use eustress_common::terrain::api::{read_voxels, TerrainCommand, TerrainFill, MAX_SCRIPT_VOXELS};
use eustress_common::terrain::{
    ground_at_world, height_at_world, lattice_cell_size, ray_plane_hit, surface_data, RegionMode, TerrainBaked,
    TerrainBrush, TerrainBrushHover, TerrainConfig, TerrainData, TerrainMaterial, TerrainMode, TerrainPaintGate,
    TerrainRoot, TerrainTool,
};
use eustress_common::units::Unit;

use crate::keybindings::Action;
use crate::terrain_commands::{apply_terrain_commands, with_terrain_read, TerrainCommandOrigin};
use crate::terrain_cursor::compact_length;

/// The Region tool's input and edits.
pub struct TerrainRegionPlugin;

impl Plugin for TerrainRegionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RegionTool>()
            .add_systems(
                Update,
                region_input
                    .after(eustress_common::terrain::update_brush_hover)
                    .before(crate::terrain_cursor::update_terrain_cursor)
                    .run_if(resource_equals(TerrainMode::Editor)),
            )
            .add_systems(Update, region_keys)
            .add_systems(Update, apply_pending_region.after(region_input).after(region_keys));
    }
}

/// A box side shorter than this is a click, which clears the box.
pub const MIN_REGION_SIDE: f32 = 0.5;

/// One face of the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionFace {
    XMin,
    XMax,
    YMin,
    YMax,
    ZMin,
    ZMax,
}

impl RegionFace {
    pub const ALL: [RegionFace; 6] =
        [RegionFace::XMin, RegionFace::XMax, RegionFace::YMin, RegionFace::YMax, RegionFace::ZMin, RegionFace::ZMax];

    /// The axis the face moves along (0 X, 1 Y, 2 Z) and whether it is the
    /// box's far (max) side.
    pub fn axis(self) -> (usize, bool) {
        match self {
            RegionFace::XMin => (0, false),
            RegionFace::XMax => (0, true),
            RegionFace::YMin => (1, false),
            RegionFace::YMax => (1, true),
            RegionFace::ZMin => (2, false),
            RegionFace::ZMax => (2, true),
        }
    }

    /// The outward normal.
    pub fn normal(self) -> Vec3 {
        let (axis, far) = self.axis();
        let mut n = Vec3::ZERO;
        n[axis] = if far { 1.0 } else { -1.0 };
        n
    }
}

/// A world-axis box of terrain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionBox {
    pub min: Vec3,
    pub max: Vec3,
}

impl RegionBox {
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// The centre of `face`.
    pub fn face_center(&self, face: RegionFace) -> Vec3 {
        let (axis, far) = face.axis();
        let mut p = self.center();
        p[axis] = if far { self.max[axis] } else { self.min[axis] };
        p
    }

    /// The side of a face handle's square: a tenth of the box's smallest
    /// side, between 0.4 and 4 m.
    pub fn handle_size(&self) -> f32 {
        (self.size().min_element() * 0.1).clamp(0.4, 4.0)
    }

    /// The same box moved by `offset`.
    pub fn moved(&self, offset: Vec3) -> RegionBox {
        RegionBox { min: self.min + offset, max: self.max + offset }
    }

    /// The box that holds this one scaled and turned about its centre by
    /// `transform`, then moved by its offset.
    pub fn transformed(&self, transform: &RegionTransform) -> RegionBox {
        let extent = transform.extent_of(self.size());
        let center = self.center() + transform.offset;
        RegionBox { min: center - extent * 0.5, max: center + extent * 0.5 }
    }
}

/// What a copy holds: occupancy and material at the volume lattice spacing,
/// index = x + dims.x * (y + dims.y * z), and how far below the box's bottom
/// the ground at its centre stood, so a paste sits on the ground the same way.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionClip {
    pub dims: UVec3,
    pub resolution: f32,
    pub materials: Vec<TerrainFill>,
    pub occupancies: Vec<f32>,
    pub ground_above_bottom: f32,
}

impl RegionClip {
    pub fn size(&self) -> Vec3 {
        self.dims.as_vec3() * self.resolution
    }

    /// The copy turned a quarter turn about Y: +X goes to +Z, +Z to -X.
    pub fn turned(&self) -> RegionClip {
        let d = self.dims;
        let nd = UVec3::new(d.z, d.y, d.x);
        let count = self.materials.len();
        let mut materials = vec![TerrainFill::Air; count];
        let mut occupancies = vec![0.0; count];
        for z in 0..d.z {
            for y in 0..d.y {
                for x in 0..d.x {
                    let from = (x + d.x * (y + d.y * z)) as usize;
                    let (nx, ny, nz) = (d.z - 1 - z, y, x);
                    let to = (nx + nd.x * (ny + nd.y * nz)) as usize;
                    materials[to] = self.materials[from];
                    occupancies[to] = self.occupancies[from];
                }
            }
        }
        RegionClip { dims: nd, materials, occupancies, ..self.clone() }
    }

    /// The copy scaled by `transform.scale` and turned `transform.angle`
    /// degrees about Y, in a box that holds the result. A whole number of
    /// quarter turns at scale 1 moves the voxels exactly ([`Self::turned`]);
    /// anything else resamples each new voxel from the copy through the
    /// inverse transform: occupancy trilinear between the copy's voxel
    /// centres, material from the neighbour that holds the most of it.
    pub fn transformed(&self, transform: &RegionTransform) -> RegionClip {
        if let Some(turns) = transform.quarter_turns() {
            let mut clip = self.clone();
            for _ in 0..turns {
                clip = clip.turned();
            }
            return clip;
        }
        let res = self.resolution;
        let size = self.size();
        let extent = transform.extent_of(size);
        let dims = resampled_dims(extent, res);
        // A positive angle turns +X toward +Z, as a quarter turn does, which
        // is a rotation by minus the angle about +Y; its inverse is plus it.
        let inverse = Quat::from_rotation_y(transform.angle.to_radians());
        let scale = transform.scale.max(1e-3);
        let d = self.dims;
        let count = (dims.x * dims.y * dims.z) as usize;
        let mut materials = Vec::with_capacity(count);
        let mut occupancies = Vec::with_capacity(count);
        let source = |x: i64, y: i64, z: i64| -> Option<usize> {
            (x >= 0 && y >= 0 && z >= 0 && x < d.x as i64 && y < d.y as i64 && z < d.z as i64)
                .then(|| (x as u32 + d.x * (y as u32 + d.y * z as u32)) as usize)
        };
        for z in 0..dims.z {
            for y in 0..dims.y {
                for x in 0..dims.x {
                    // The new voxel's centre, from the new box's centre.
                    let p = (UVec3::new(x, y, z).as_vec3() + Vec3::splat(0.5)) * res - dims.as_vec3() * res * 0.5;
                    let q = inverse * p / scale;
                    // As a fractional index into the copy's voxel centres.
                    let u = (q + size * 0.5) / res - Vec3::splat(0.5);
                    let base = u.floor();
                    let f = u - base;
                    let (mut occupancy, mut best, mut best_weight) = (0.0f32, TerrainFill::Air, 0.0f32);
                    for corner in 0..8u32 {
                        let (cx, cy, cz) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
                        let w = (if cx == 1 { f.x } else { 1.0 - f.x })
                            * (if cy == 1 { f.y } else { 1.0 - f.y })
                            * (if cz == 1 { f.z } else { 1.0 - f.z });
                        let Some(index) =
                            source(base.x as i64 + cx as i64, base.y as i64 + cy as i64, base.z as i64 + cz as i64)
                        else {
                            continue;
                        };
                        let held = w * self.occupancies[index];
                        occupancy += held;
                        if held > best_weight {
                            best_weight = held;
                            best = self.materials[index];
                        }
                    }
                    materials.push(if occupancy > 0.0 { best } else { TerrainFill::Air });
                    occupancies.push(occupancy.clamp(0.0, 1.0));
                }
            }
        }
        RegionClip { dims, resolution: res, materials, occupancies, ground_above_bottom: self.ground_above_bottom * scale }
    }

    /// The command that writes the copy with its min corner at `corner`.
    pub fn write_at(&self, corner: Vec3) -> TerrainCommand {
        TerrainCommand::WriteVoxels {
            min: corner,
            resolution: self.resolution,
            size: self.dims,
            materials: self.materials.clone(),
            occupancies: self.occupancies.clone(),
        }
    }
}

/// How far a resampled box's side may pass a whole number of voxels and
/// still round down to it, as a fraction of a voxel: a turn a hair off a
/// quarter turn moves the corners by float dust, which must not add a row of
/// voxels the resampling would fill with slivers.
const RESAMPLE_SNAP_VOXELS: f32 = 0.01;

/// The voxel counts of a resampled box of `extent` metres at `resolution`:
/// whole voxels, rounded up past [`RESAMPLE_SNAP_VOXELS`], at least one.
fn resampled_dims(extent: Vec3, resolution: f32) -> UVec3 {
    (extent / resolution - Vec3::splat(RESAMPLE_SNAP_VOXELS)).ceil().max(Vec3::ONE).as_uvec3()
}

/// Where a Transform takes the box's contents: moved by `offset`, turned
/// `angle` degrees about the vertical through the box's centre, and scaled
/// by `scale` about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionTransform {
    pub offset: Vec3,
    pub angle: f32,
    pub scale: f32,
}

impl Default for RegionTransform {
    fn default() -> Self {
        Self { offset: Vec3::ZERO, angle: 0.0, scale: 1.0 }
    }
}

/// The smallest scale a Transform takes, and the largest.
pub const REGION_SCALE_RANGE: (f32, f32) = (0.1, 10.0);

impl RegionTransform {
    /// Moves, turns and scales nothing.
    pub fn is_identity(&self) -> bool {
        *self == RegionTransform::default()
    }

    /// The angle as whole quarter turns (0 to 3) when it is one and the
    /// scale is 1, so the voxels move exactly.
    pub fn quarter_turns(&self) -> Option<u8> {
        let turns = self.angle / 90.0;
        let whole = turns.round();
        ((turns - whole).abs() < 1e-4 && (self.scale - 1.0).abs() < 1e-6).then(|| whole.rem_euclid(4.0) as u8)
    }

    /// The size of the box that holds a box of `size` scaled and turned by
    /// this transform.
    pub fn extent_of(&self, size: Vec3) -> Vec3 {
        let scaled = size * self.scale;
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (sin, cos) = (sin.abs(), cos.abs());
        // Clear the float dust of a quarter turn, so 90 degrees swaps the
        // sides exactly.
        let clean = |v: f32| if v < 1e-6 { 0.0 } else if v > 1.0 - 1e-6 { 1.0 } else { v };
        let (sin, cos) = (clean(sin), clean(cos));
        Vec3::new(cos * scaled.x + sin * scaled.z, scaled.y, sin * scaled.x + cos * scaled.z)
    }
}

/// An edit or command the Region tool runs at the next update.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RegionOp {
    Copy,
    Cut,
    /// Start pasting: the ghost follows the cursor.
    Paste,
    /// Place the paste with its min corner here.
    Place(Vec3),
    Duplicate,
    Delete,
    Fill,
    Replace,
    Rotate,
    ApplyTransform,
    Cancel,
}

/// The Region tool's box, clipboard, paste and transform.
#[derive(Resource, Debug, Clone, Default)]
pub struct RegionTool {
    pub region: Option<RegionBox>,
    pub clipboard: Option<RegionClip>,
    /// While pasting: the ghost's min corner under the cursor.
    pub paste_at: Option<Vec3>,
    /// While transforming: where the contents go.
    pub transform: Option<RegionTransform>,
    /// The face handle under the cursor, or being dragged.
    pub hovered_face: Option<RegionFace>,
    pub pending: Option<RegionOp>,
    drag: Option<RegionDrag>,
}

impl RegionTool {
    /// A drag is drawing, resizing or moving the box.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Where a Transform would put the box, while one is open.
    pub fn target(&self) -> Option<RegionBox> {
        let (region, transform) = (self.region?, self.transform?);
        Some(region.transformed(&transform))
    }

    /// The paste ghost's box, while pasting.
    pub fn paste_box(&self) -> Option<RegionBox> {
        let (corner, clip) = (self.paste_at?, self.clipboard.as_ref()?);
        Some(RegionBox { min: corner, max: corner + clip.size() })
    }

    /// Drop the box, any paste, transform and drag; the clipboard stays.
    pub fn clear(&mut self) {
        self.region = None;
        self.paste_at = None;
        self.transform = None;
        self.hovered_face = None;
        self.drag = None;
        self.pending = None;
    }

    /// Move a Transform's target up or down by `delta` metres, when one is open.
    pub fn nudge_target(&mut self, delta: f32) {
        if let Some(transform) = self.transform.as_mut() {
            transform.offset.y += delta;
        }
    }

    /// Something `Esc` would cancel: a paste, a transform or the box itself.
    pub fn has_open_state(&self) -> bool {
        self.paste_at.is_some() || self.transform.is_some() || self.region.is_some() || self.drag.is_some()
    }

    /// `Esc`: cancel a paste or a transform first, else drop the box.
    pub fn escape(&mut self) {
        if self.paste_at.is_some() || self.transform.is_some() || self.drag.is_some() {
            self.paste_at = None;
            self.transform = None;
            self.drag = None;
        } else {
            self.clear();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RegionDrag {
    /// Drawing the footprint from world XZ `from`, at ground height `height`.
    Draw { from: Vec2, height: f32 },
    /// Moving `face`: its coordinate was `start_value` when the pointer
    /// crossed the drag plane (through `point`, normal `normal`) at `start`.
    Face { face: RegionFace, start_value: f32, point: Vec3, normal: Vec3, start: Vec3 },
    /// Sliding a Transform's target over the plane at height `plane_y`: the
    /// offset was `start_offset` when the pointer crossed it at `start`.
    Move { plane_y: f32, start: Vec3, start_offset: Vec3 },
}

/// `value` snapped to `step`.
fn snapped(value: f32, step: f32) -> f32 {
    if step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

/// Where `ray` crosses the plane through `point` with `normal`, `None` when
/// it runs along it or the plane is behind it.
fn ray_plane(ray: Ray3d, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let direction = *ray.direction;
    let denominator = direction.dot(normal);
    if denominator.abs() < 1e-4 {
        return None;
    }
    let t = (point - ray.origin).dot(normal) / denominator;
    (t > 0.0).then(|| ray.origin + direction * t)
}

/// The distance along `ray` to where it enters `region`, `None` when it
/// misses (the slab test).
fn ray_box(ray: Ray3d, region: &RegionBox) -> Option<f32> {
    let direction = *ray.direction;
    let (mut near, mut far) = (0.0f32, f32::INFINITY);
    for axis in 0..3 {
        let (o, d) = (ray.origin[axis], direction[axis]);
        if d.abs() < 1e-9 {
            if o < region.min[axis] || o > region.max[axis] {
                return None;
            }
            continue;
        }
        let a = (region.min[axis] - o) / d;
        let b = (region.max[axis] - o) / d;
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// The face handle `ray` points at: the nearest face whose handle square it
/// crosses.
pub fn face_under_ray(ray: Ray3d, region: &RegionBox) -> Option<RegionFace> {
    let half = region.handle_size() * 0.5;
    let mut best: Option<(f32, RegionFace)> = None;
    for face in RegionFace::ALL {
        let center = region.face_center(face);
        let Some(hit) = ray_plane(ray, center, face.normal()) else { continue };
        let (axis, _) = face.axis();
        let within = (0..3).filter(|a| *a != axis).all(|a| (hit[a] - center[a]).abs() <= half);
        if !within {
            continue;
        }
        let distance = hit.distance(ray.origin);
        if best.map_or(true, |(d, _)| distance < d) {
            best = Some((distance, face));
        }
    }
    best.map(|(_, face)| face)
}

/// A plane through `point` that holds `axis` and faces `view` as squarely as
/// it can: its normal is the part of the view direction across the axis.
fn axis_drag_plane(axis: Vec3, view: Vec3) -> Vec3 {
    let across = view - axis * view.dot(axis);
    across.try_normalize().unwrap_or_else(|| axis.any_orthonormal_vector())
}

fn held(keys: &ButtonInput<KeyCode>, left: KeyCode, right: KeyCode) -> bool {
    keys.pressed(left) || keys.pressed(right)
}

/// The lowest and highest ground over the footprint `lo..hi`, sampled on a
/// 16 by 16 grid; `None` when none of it is over ground.
fn ground_span(config: &TerrainConfig, surface: &TerrainData, lo: Vec2, hi: Vec2) -> Option<(f32, f32)> {
    let n = 16;
    let mut span: Option<(f32, f32)> = None;
    for j in 0..=n {
        for i in 0..=n {
            let p = lo + (hi - lo) * Vec2::new(i as f32, j as f32) / n as f32;
            if !ground_at_world(config, surface, p.x, p.y) {
                continue;
            }
            let y = height_at_world(config, surface, p.x, p.y);
            span = Some(span.map_or((y, y), |(a, b)| (a.min(y), b.max(y))));
        }
    }
    span
}

/// Draw and resize the box, slide a Transform's target, and follow the
/// cursor with a paste (see the module docs). Runs only while the terrain
/// tools are on; a drag ends when Region stops being the tool.
#[allow(clippy::too_many_arguments)]
fn region_input(
    brush: Res<TerrainBrush>,
    hover: Res<TerrainBrushHover>,
    gate: Option<Res<TerrainPaintGate>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    terrain: Query<(&TerrainConfig, &TerrainData, Option<&TerrainBaked>), With<TerrainRoot>>,
    mut tool: ResMut<RegionTool>,
) {
    if brush.tool != TerrainTool::Region {
        if tool.drag.is_some() || tool.paste_at.is_some() || tool.hovered_face.is_some() {
            tool.drag = None;
            tool.paste_at = None;
            tool.hovered_face = None;
        }
        return;
    }
    let Ok((config, data, baked)) = terrain.single() else { return };
    let cell = lattice_cell_size(config);
    let step = brush.active_snap().unwrap_or(cell);
    let allowed = gate.map_or(true, |gate| gate.allowed);
    let alt = held(&keys, KeyCode::AltLeft, KeyCode::AltRight);
    let press = buttons.just_pressed(MouseButton::Left) && allowed && !alt;
    let transforming = brush.region_mode == RegionMode::Transform;

    // A Transform opens on the box as the mode is chosen, and closes when
    // another mode is.
    if transforming && tool.region.is_some() && tool.transform.is_none() {
        tool.transform = Some(RegionTransform::default());
    } else if !transforming && tool.transform.is_some() {
        tool.transform = None;
    }

    // Pasting: the ghost follows the ground under the cursor, sitting on it
    // the way the copy sat; a click places it.
    if tool.paste_at.is_some() {
        if let (Some(hit), Some(clip)) = (hover.surface, tool.clipboard.as_ref()) {
            let size = clip.size();
            let corner = Vec3::new(
                snapped(hit.x - size.x * 0.5, step),
                snapped(hit.y - clip.ground_above_bottom, step),
                snapped(hit.z - size.z * 0.5, step),
            );
            if tool.paste_at != Some(corner) {
                tool.paste_at = Some(corner);
            }
        }
        if press {
            if let Some(corner) = tool.paste_at {
                tool.pending = Some(RegionOp::Place(corner));
            }
        }
        return;
    }

    // The handle under the cursor, when no drag is open.
    if tool.drag.is_none() && !transforming {
        let face = match (tool.region, hover.ray) {
            (Some(region), Some(ray)) => face_under_ray(ray, &region),
            _ => None,
        };
        if tool.hovered_face != face {
            tool.hovered_face = face;
        }
    }

    if press {
        let view = hover.ray.map(|ray| *ray.direction).unwrap_or(Vec3::NEG_Y);
        if transforming {
            if let (Some(target), Some(ray), Some(transform)) = (tool.target(), hover.ray, tool.transform) {
                let plane_y = target.max.y;
                let on_target = ray_box(ray, &target).is_some();
                if let (true, Some(start)) = (on_target, ray_plane_hit(ray, plane_y)) {
                    tool.drag = Some(RegionDrag::Move { plane_y, start, start_offset: transform.offset });
                }
            }
        } else if let (Some(face), Some(region), Some(ray)) = (tool.hovered_face, tool.region, hover.ray) {
            let (axis, far) = face.axis();
            let point = region.face_center(face);
            let normal = axis_drag_plane(face.normal(), view);
            if let Some(start) = ray_plane(ray, point, normal) {
                let start_value = if far { region.max[axis] } else { region.min[axis] };
                tool.drag = Some(RegionDrag::Face { face, start_value, point, normal, start });
            }
        } else if let Some(hit) = hover.surface {
            let from = Vec2::new(snapped(hit.x, step), snapped(hit.z, step));
            tool.drag = Some(RegionDrag::Draw { from, height: hit.y });
            tool.region = None;
        }
    }

    if buttons.pressed(MouseButton::Left) {
        match tool.drag {
            Some(RegionDrag::Draw { from, height }) => {
                let at = hover.surface.or_else(|| hover.ray.and_then(|ray| ray_plane_hit(ray, height)));
                if let Some(at) = at {
                    let to = Vec2::new(snapped(at.x, step), snapped(at.z, step));
                    let (lo, hi) = (from.min(to), from.max(to));
                    let surface = surface_data(data, baked);
                    let (bottom, top) = ground_span(config, surface, lo, hi).unwrap_or((height, height));
                    let margin = (2.0 * cell).max(step);
                    let region = RegionBox {
                        min: Vec3::new(lo.x, snapped(bottom - margin, step), lo.y),
                        max: Vec3::new(hi.x, snapped(top + margin, step), hi.y),
                    };
                    if tool.region != Some(region) {
                        tool.region = Some(region);
                    }
                }
            }
            Some(RegionDrag::Face { face, start_value, point, normal, start }) => {
                if let (Some(ray), Some(mut region)) = (hover.ray, tool.region) {
                    if let Some(at) = ray_plane(ray, point, normal) {
                        let (axis, far) = face.axis();
                        let value = snapped(start_value + (at - start).dot(face.normal().abs()), step);
                        // A face never crosses the one opposite it.
                        if far {
                            region.max[axis] = value.max(region.min[axis] + step);
                        } else {
                            region.min[axis] = value.min(region.max[axis] - step);
                        }
                        if tool.region != Some(region) {
                            tool.region = Some(region);
                        }
                    }
                }
            }
            Some(RegionDrag::Move { plane_y, start, start_offset }) => {
                if let Some(at) = hover.ray.and_then(|ray| ray_plane_hit(ray, plane_y)) {
                    let delta = at - start;
                    let offset =
                        Vec3::new(snapped(start_offset.x + delta.x, step), start_offset.y, snapped(start_offset.z + delta.z, step));
                    if let Some(transform) = tool.transform.as_mut().filter(|t| t.offset != offset) {
                        transform.offset = offset;
                    }
                }
            }
            None => {}
        }
    } else if let Some(drag) = tool.drag {
        tool.drag = None;
        // A click without a drag clears the box.
        if let (RegionDrag::Draw { .. }, Some(region)) = (drag, tool.region) {
            let size = region.size();
            if size.x < MIN_REGION_SIDE || size.z < MIN_REGION_SIDE {
                tool.region = None;
            }
        }
        if matches!(drag, RegionDrag::Draw { .. }) && tool.region.is_none() {
            tool.hovered_face = None;
        }
    }

    // Enter applies a Transform, unless a text field has the keyboard.
    let enter = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    let typing = ui_focus.as_deref().is_some_and(|focus| focus.text_input_focused)
        || crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed);
    if enter && !typing && tool.drag.is_none() && tool.transform.is_some() {
        tool.pending = Some(RegionOp::ApplyTransform);
    }
}

/// Queue the Region shortcuts (`Ctrl` C, X, V, D and `Delete`, sent by the
/// keybinding table in the terrain context) while Region is the tool.
fn region_keys(
    mut menu: MessageReader<crate::ui::MenuActionEvent>,
    brush: Res<TerrainBrush>,
    mut tool: ResMut<RegionTool>,
) {
    for event in menu.read() {
        let op = match event.action {
            Action::TerrainRegionCopy => RegionOp::Copy,
            Action::TerrainRegionCut => RegionOp::Cut,
            Action::TerrainRegionPaste => RegionOp::Paste,
            Action::TerrainRegionDuplicate => RegionOp::Duplicate,
            Action::TerrainRegionDelete => RegionOp::Delete,
            _ => continue,
        };
        if brush.tool == TerrainTool::Region {
            tool.pending = Some(op);
        }
    }
}

fn notify(world: &mut World, message: impl Into<String>, warning: bool) {
    let message = message.into();
    info!("🧱 {message}");
    if let Some(mut n) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
        if warning {
            n.warning(message);
        } else {
            n.info(message);
        }
    }
}

/// Read the terrain inside `region` at the lattice spacing, `Err` naming why
/// when there is no terrain or the box holds too many voxels.
fn copy_region(world: &World, region: &RegionBox) -> Result<RegionClip, String> {
    with_terrain_read(world, |config, data, volume, water| {
        let resolution = lattice_cell_size(config);
        let dims = (region.size() / resolution).round().max(Vec3::ONE).as_uvec3();
        let count = dims.x as u64 * dims.y as u64 * dims.z as u64;
        if count > MAX_SCRIPT_VOXELS as u64 {
            return Err(format!(
                "the box holds {count} voxels of {resolution} m, over the {MAX_SCRIPT_VOXELS} one copy may; make it smaller"
            ));
        }
        let (materials, occupancies) = read_voxels(config, data, volume, water, region.min, resolution, dims);
        let center = region.center();
        let ground = if ground_at_world(config, data, center.x, center.z) {
            height_at_world(config, data, center.x, center.z)
        } else {
            region.min.y
        };
        Ok(RegionClip {
            dims,
            resolution,
            materials,
            occupancies,
            ground_above_bottom: (ground - region.min.y).clamp(0.0, region.size().y),
        })
    })
    .unwrap_or_else(|| Err("there is no terrain to copy".to_string()))
}

/// Run `commands` as one undo entry labelled `label`; `Err` with the first
/// refusal.
fn run(world: &mut World, label: &str, commands: Vec<TerrainCommand>) -> Result<(), String> {
    let results = apply_terrain_commands(world, commands, TerrainCommandOrigin::Tool { label: label.to_string() });
    match results.into_iter().find_map(Result::err) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// The command that empties `region`.
fn empty(region: &RegionBox) -> TerrainCommand {
    TerrainCommand::FillRegion { min: region.min, max: region.max, fill: TerrainFill::Air }
}

/// Run the queued Region edit (see the module docs).
fn apply_pending_region(world: &mut World) {
    let queued = world.get_resource::<RegionTool>().and_then(|tool| tool.pending);
    let Some(op) = queued else { return };
    world.resource_mut::<RegionTool>().pending = None;
    let (region, clipboard) = {
        let tool = world.resource::<RegionTool>();
        (tool.region, tool.clipboard.clone())
    };
    let brush = world.get_resource::<TerrainBrush>().cloned().unwrap_or_default();
    let need_box = "Region: drag a box on the ground first.";

    match op {
        RegionOp::Copy | RegionOp::Cut => {
            let Some(region) = region else { return notify(world, need_box, false) };
            match copy_region(world, &region) {
                Ok(clip) => {
                    world.resource_mut::<RegionTool>().clipboard = Some(clip);
                    if op == RegionOp::Cut {
                        if let Err(e) = run(world, "Cut Terrain", vec![empty(&region)]) {
                            notify(world, format!("Cut: {e}"), true);
                        }
                    }
                }
                Err(e) => notify(world, format!("Region: {e}"), true),
            }
        }
        RegionOp::Paste => {
            if clipboard.is_none() {
                return notify(world, "Region: copy something first (Ctrl+C).", false);
            }
            let mut tool = world.resource_mut::<RegionTool>();
            tool.transform = None;
            tool.paste_at = Some(region.map_or(Vec3::ZERO, |r| r.min));
        }
        RegionOp::Place(corner) => {
            let Some(clip) = clipboard else { return };
            match run(world, "Paste Terrain", vec![clip.write_at(corner)]) {
                Ok(()) => {
                    let mut tool = world.resource_mut::<RegionTool>();
                    tool.region = Some(RegionBox { min: corner, max: corner + clip.size() });
                    tool.paste_at = None;
                }
                Err(e) => notify(world, format!("Paste: {e}"), true),
            }
        }
        RegionOp::Duplicate => {
            let Some(region) = region else { return notify(world, need_box, false) };
            match copy_region(world, &region) {
                Ok(clip) => {
                    let corner = region.min + Vec3::X * clip.size().x;
                    match run(world, "Duplicate Terrain", vec![clip.write_at(corner)]) {
                        Ok(()) => {
                            let mut tool = world.resource_mut::<RegionTool>();
                            tool.region = Some(RegionBox { min: corner, max: corner + clip.size() });
                            tool.clipboard = Some(clip);
                        }
                        Err(e) => notify(world, format!("Duplicate: {e}"), true),
                    }
                }
                Err(e) => notify(world, format!("Region: {e}"), true),
            }
        }
        RegionOp::Delete => {
            let Some(region) = region else { return notify(world, need_box, false) };
            if let Err(e) = run(world, "Delete Terrain", vec![empty(&region)]) {
                notify(world, format!("Delete: {e}"), true);
            }
        }
        RegionOp::Fill => {
            let Some(region) = region else { return notify(world, need_box, false) };
            let Some(material) = TerrainMaterial::from_u8(brush.paint_material) else {
                return notify(world, "Fill: a custom material slot cannot fill yet; pick a built-in material.", false);
            };
            let fill = TerrainCommand::FillRegion { min: region.min, max: region.max, fill: TerrainFill::Material(material) };
            if let Err(e) = run(world, "Fill Terrain", vec![fill]) {
                notify(world, format!("Fill: {e}"), true);
            }
        }
        RegionOp::Replace => {
            let Some(region) = region else { return notify(world, need_box, false) };
            let (Some(from), Some(to)) =
                (TerrainMaterial::from_u8(brush.source_material), TerrainMaterial::from_u8(brush.paint_material))
            else {
                return notify(world, "Replace: custom material slots cannot be swapped yet; pick built-in materials.", false);
            };
            let swap = TerrainCommand::ReplaceMaterial { min: region.min, max: region.max, from, to };
            if let Err(e) = run(world, "Replace Terrain Material", vec![swap]) {
                notify(world, format!("Replace: {e}"), true);
            }
        }
        RegionOp::Rotate => {
            let mut tool = world.resource_mut::<RegionTool>();
            if tool.region.is_some() {
                let transform = tool.transform.get_or_insert_with(RegionTransform::default);
                transform.angle = (transform.angle + 90.0).rem_euclid(360.0);
            }
        }
        RegionOp::ApplyTransform => {
            let (Some(region), Some(transform)) = (region, world.resource::<RegionTool>().transform) else { return };
            if transform.is_identity() {
                return;
            }
            let clip = match copy_region(world, &region) {
                Ok(clip) => clip,
                Err(e) => return notify(world, format!("Transform: {e}"), true),
            };
            let target_dims = resampled_dims(transform.extent_of(clip.size()), clip.resolution);
            let target_count = target_dims.x as f64 * target_dims.y as f64 * target_dims.z as f64;
            if target_count > MAX_SCRIPT_VOXELS as f64 {
                return notify(
                    world,
                    format!(
                        "Transform: the result would hold {target_count:.0} voxels, over the {MAX_SCRIPT_VOXELS} one edit may; scale it down"
                    ),
                    true,
                );
            }
            let clip = clip.transformed(&transform);
            // The written box is the resampled copy's, centred where the
            // target's centre is.
            let corner = region.center() + transform.offset - clip.size() * 0.5;
            match run(world, "Transform Terrain", vec![empty(&region), clip.write_at(corner)]) {
                Ok(()) => {
                    let mut tool = world.resource_mut::<RegionTool>();
                    tool.region = Some(RegionBox { min: corner, max: corner + clip.size() });
                    tool.transform = if brush.region_mode == RegionMode::Transform {
                        Some(RegionTransform::default())
                    } else {
                        None
                    };
                }
                Err(e) => notify(world, format!("Transform: {e}"), true),
            }
        }
        RegionOp::Cancel => {
            let mut tool = world.resource_mut::<RegionTool>();
            tool.paste_at = None;
            if tool.transform.is_some() {
                tool.transform = Some(RegionTransform::default());
            }
        }
    }
}

/// The readout beside the cursor while Region is the tool: the mode, the
/// box's size, and what a key does next.
pub fn region_readout(tool: &RegionTool, mode: RegionMode, unit: Unit) -> String {
    let name = match mode {
        RegionMode::Select => "Region",
        RegionMode::Transform => "Transform",
        RegionMode::Fill => "Fill",
    };
    let mut parts = vec![name.to_string()];
    let size_text = |size: Vec3| {
        format!("{} × {} × {}", compact_length(size.x, unit), compact_length(size.y, unit), compact_length(size.z, unit))
    };
    if let Some(ghost) = tool.paste_box() {
        parts.push(format!("paste {}", size_text(ghost.size())));
        parts.push("click to place".to_string());
        return parts.join(" · ");
    }
    match tool.region {
        Some(region) => parts.push(size_text(region.size())),
        None => parts.push("drag a box".to_string()),
    }
    if let Some(transform) = tool.transform.filter(|t| !t.is_identity()) {
        if transform.angle.abs() > 1e-3 {
            parts.push(format!("{}°", trim_number(transform.angle)));
        }
        if (transform.scale - 1.0).abs() > 1e-4 {
            parts.push(format!("{}%", trim_number(transform.scale * 100.0)));
        }
        parts.push("Enter to apply".to_string());
    }
    parts.join(" · ")
}

/// `value` with at most one decimal, trailing zeros dropped.
fn trim_number(value: f32) -> String {
    let text = format!("{value:.1}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_turn_moves_every_voxel_and_four_come_back() {
        let dims = UVec3::new(3, 2, 2);
        let count = 12;
        let clip = RegionClip {
            dims,
            resolution: 1.0,
            materials: (0..count).map(|i| if i == 1 { TerrainFill::Material(TerrainMaterial::Rock) } else { TerrainFill::Air }).collect(),
            occupancies: (0..count).map(|i| i as f32).collect(),
            ground_above_bottom: 0.5,
        };
        let turned = clip.turned();
        assert_eq!(turned.dims, UVec3::new(2, 2, 3));
        assert_eq!(turned.size(), Vec3::new(2.0, 2.0, 3.0));
        // Voxel (1, 0, 0) goes to (dims.z - 1 - 0, 0, 1) = (1, 0, 1).
        let to = (1 + 2 * (0 + 2 * 1)) as usize;
        assert_eq!(turned.materials[to], TerrainFill::Material(TerrainMaterial::Rock));
        assert_eq!(turned.occupancies[to], 1.0);
        let back = turned.turned().turned().turned();
        assert_eq!(back, clip, "four quarter turns are no turn");
    }

    #[test]
    fn a_transformed_box_holds_the_turned_and_scaled_box() {
        let region = RegionBox { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(4.0, 2.0, 2.0) };
        let quarter = RegionTransform { offset: Vec3::new(10.0, 0.0, 0.0), angle: 90.0, scale: 1.0 };
        let turned = region.transformed(&quarter);
        assert_eq!(turned.size(), Vec3::new(2.0, 2.0, 4.0));
        assert_eq!(turned.center(), Vec3::new(12.0, 1.0, 1.0));
        assert_eq!(quarter.quarter_turns(), Some(1));
        let half = RegionTransform { angle: 180.0, ..RegionTransform::default() };
        assert_eq!(region.transformed(&half), region);
        assert_eq!(RegionTransform { angle: -90.0, ..RegionTransform::default() }.quarter_turns(), Some(3));
        let eighth = RegionTransform { angle: 45.0, scale: 2.0, ..RegionTransform::default() };
        assert_eq!(eighth.quarter_turns(), None);
        let size = region.transformed(&eighth).size();
        let diagonal = (4.0 + 2.0) * 2.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((size.x - diagonal).abs() < 1e-4 && (size.z - diagonal).abs() < 1e-4 && size.y == 4.0, "{size}");
        assert!(RegionTransform::default().is_identity());
        assert_eq!(region.moved(Vec3::Y).min, Vec3::Y);
    }

    /// A solid 4 x 2 x 2 block of Rock with one Sand voxel at its +X end.
    fn block() -> RegionClip {
        let dims = UVec3::new(4, 2, 2);
        let mut materials = vec![TerrainFill::Material(TerrainMaterial::Rock); 16];
        materials[3] = TerrainFill::Material(TerrainMaterial::Sand);
        RegionClip { dims, resolution: 1.0, materials, occupancies: vec![1.0; 16], ground_above_bottom: 1.0 }
    }

    #[test]
    fn resampling_follows_the_exact_quarter_turn() {
        let clip = block();
        let exact = clip.transformed(&RegionTransform { angle: 90.0, ..RegionTransform::default() });
        assert_eq!(exact, clip.turned(), "a quarter turn moves the voxels exactly");
        let near = clip.transformed(&RegionTransform { angle: 90.01, ..RegionTransform::default() });
        assert_eq!(near.dims, exact.dims, "a hair past a quarter turn holds the same voxels");
        for (i, (a, b)) in near.occupancies.iter().zip(&exact.occupancies).enumerate() {
            assert!((a - b).abs() < 0.05, "voxel {i}: {a} against {b}");
        }
        let sand = TerrainFill::Material(TerrainMaterial::Sand);
        assert_eq!(
            near.materials.iter().position(|m| *m == sand),
            exact.materials.iter().position(|m| *m == sand),
            "the Sand voxel lands where the exact turn puts it"
        );
    }

    #[test]
    fn scaling_up_keeps_a_solid_block_solid() {
        let clip = block();
        let big = clip.transformed(&RegionTransform { scale: 2.0, ..RegionTransform::default() });
        assert_eq!(big.dims, UVec3::new(8, 4, 4));
        assert!((big.ground_above_bottom - 2.0).abs() < 1e-6);
        // Inside the block every voxel is full; its corners read the edge.
        let at = |x: u32, y: u32, z: u32| (x + 8 * (y + 4 * z)) as usize;
        assert!((big.occupancies[at(3, 1, 1)] - 1.0).abs() < 1e-5);
        assert!(big.occupancies.iter().all(|o| (0.0..=1.0).contains(o)));
        assert_eq!(big.materials[at(7, 1, 1)], TerrainFill::Material(TerrainMaterial::Sand), "Sand stays at the +X end");
        assert_eq!(big.materials[at(0, 1, 1)], TerrainFill::Material(TerrainMaterial::Rock));
    }

    #[test]
    fn handles_are_found_under_the_ray_and_boxes_are_hit() {
        let region = RegionBox { min: Vec3::splat(-4.0), max: Vec3::splat(4.0) };
        let down = Ray3d::new(Vec3::new(0.0, 20.0, 0.0), Dir3::NEG_Y);
        assert_eq!(face_under_ray(down, &region), Some(RegionFace::YMax), "the top handle is nearest");
        let beside = Ray3d::new(Vec3::new(3.0, 20.0, 3.0), Dir3::NEG_Y);
        assert_eq!(face_under_ray(beside, &region), None, "off the handle squares");
        assert!((ray_box(down, &region).unwrap() - 16.0).abs() < 1e-4);
        let miss = Ray3d::new(Vec3::new(10.0, 20.0, 0.0), Dir3::NEG_Y);
        assert!(ray_box(miss, &region).is_none());
        let plane = axis_drag_plane(Vec3::X, Vec3::new(0.3, -0.5, 1.0).normalize());
        assert!(plane.dot(Vec3::X).abs() < 1e-5, "the drag plane holds its axis");
    }

    #[test]
    fn escape_cancels_a_paste_before_the_box() {
        let mut tool = RegionTool {
            region: Some(RegionBox { min: Vec3::ZERO, max: Vec3::ONE }),
            paste_at: Some(Vec3::X),
            ..RegionTool::default()
        };
        tool.escape();
        assert!(tool.paste_at.is_none() && tool.region.is_some());
        tool.escape();
        assert!(!tool.has_open_state());
    }

    #[test]
    fn the_readout_names_the_mode_and_size() {
        let mut tool = RegionTool::default();
        assert_eq!(region_readout(&tool, RegionMode::Select, Unit::Meter), "Region · drag a box");
        tool.region = Some(RegionBox { min: Vec3::ZERO, max: Vec3::new(24.0, 8.0, 16.0) });
        tool.transform = Some(RegionTransform { offset: Vec3::X, angle: 90.0, scale: 1.5 });
        assert_eq!(
            region_readout(&tool, RegionMode::Transform, Unit::Meter),
            "Transform · 24 m × 8 m × 16 m · 90° · 150% · Enter to apply"
        );
    }
}
