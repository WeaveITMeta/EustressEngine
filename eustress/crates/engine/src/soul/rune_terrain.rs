//! # `eustress::terrain`: Rune on the Space's terrain
//!
//! Fill, carve, sculpt and paint the terrain from a script in Play, and read
//! it back. Positions and lengths are world metres. Materials are Roblox
//! terrain material names in any case (`"Grass"`, `"rock"`,
//! `"Enum.Material.Sand"`); `"Air"` carves, and `"Water"` raises the water
//! surface over the shape's footprint. Numbers are floats: `4.0`, not `4`.
//!
//! ```rune
//! use eustress::Vector3;
//! use eustress::terrain;
//!
//! pub fn on_init() {
//!     // A crater, and a rock pillar standing in it.
//!     terrain::fill_ball(Vector3::new(0.0, 10.0, 0.0), 12.0, "Air");
//!     terrain::fill_block(Vector3::new(0.0, 5.0, 0.0), Vector3::new(4.0, 20.0, 4.0), "Rock");
//! }
//!
//! pub fn on_update(dt) {
//!     let ground = terrain::height_at(20.0, 0.0);
//!     if terrain::material_at(20.0, 0.0) == "Snow" {
//!         terrain::paint(Vector3::new(20.0, ground, 0.0), 3.0, "Mud", 0.5);
//!     }
//! }
//! ```
//!
//! Points and sizes are `Vector3`s, borrowed so the script keeps its value.
//! A native Rune function takes at most five arguments, too few for the
//! seven numbers of a block.
//!
//! ## Edits are queued
//!
//! An edit returns `false`, queueing nothing, for an unknown material, a
//! number that is not finite, or a size, radius or strength that is not
//! positive. Otherwise it queues and returns `true`. The queue drains when the
//! frame's callbacks have returned, in call order, as one batch of scripted
//! edits: no undo entry, and Stop restores the terrain Play snapshotted
//! before the first. Reads in the same frame see the terrain as the frame
//! began.
//!
//! ## Where it works
//!
//! [`crate::soul::rune_play::drive_rune_frame`] installs the terrain for
//! `on_init`, `on_ready` and `on_update` ([`with_terrain_view`]). Anywhere
//! else (the command bar, `on_button_click`, `on_exit`), and in a Space
//! without terrain a script can edit (none, or procedural terrain, which has
//! no height raster), every edit returns `false` and every read its empty
//! value: a `NaN` height, a `""` material, `0` occupancy.

use std::cell::{Cell, RefCell};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rune::{ContextError, Module};

use eustress_common::terrain::api::{read_voxels, TerrainCommand, TerrainFill, TerrainSculptMode};
use eustress_common::terrain::volume::material_at as edit_material_at;
use eustress_common::terrain::{
    cache_cell_at_world, chunk_volume_y_range, height_at_world, lattice_cell_size,
    material_at_world, sample_field, sample_field_parts, surface_data, FieldSample, FieldTerm,
    TerrainBaked, TerrainConfig, TerrainData, TerrainMaterial, TerrainRoot, TerrainVolume,
    TerrainVoxelWater,
};

use super::rune_ecs_module::Vector3;
use crate::terrain_commands::{apply_terrain_commands, TerrainCommandOrigin};

// ============================================================================
// The terrain a frame's scripts see
// ============================================================================

/// The Space's terrain root as scripts read and edit it for one frame.
#[derive(Clone, Copy)]
pub struct TerrainView<'a> {
    pub config: &'a TerrainConfig,
    /// The surface data: the bake when the root has layers, else the base.
    pub data: &'a TerrainData,
    /// The root's volumetric edits, empty when it has none.
    pub volume: &'a TerrainVolume,
    /// The water the root carries, if any.
    pub water: Option<&'a TerrainVoxelWater>,
}

impl TerrainView<'_> {
    /// The terrain has a height raster, which is what scripts read and edit.
    /// Procedural terrain, drawn from noise, has none.
    fn has_raster(&self) -> bool {
        !self.data.height_cache.is_empty() && self.data.cache_width > 0 && self.data.cache_height > 0
    }
}

/// The terrain root [`crate::soul::rune_play::drive_rune_frame`] hands its
/// scripts, read only.
#[derive(SystemParam)]
pub struct ScriptTerrain<'w, 's> {
    roots: Query<
        'w,
        's,
        (
            &'static TerrainConfig,
            &'static TerrainData,
            Option<&'static TerrainBaked>,
            Option<&'static TerrainVolume>,
            Option<&'static TerrainVoxelWater>,
        ),
        With<TerrainRoot>,
    >,
}

impl ScriptTerrain<'_, '_> {
    /// The Space's terrain root as scripts see it this frame. `None` without
    /// one, and while two roots coexist mid-replacement, like
    /// `TerrainMaterialQuery`.
    pub fn view(&self) -> Option<TerrainView<'_>> {
        let (config, base, baked, volume, water) = self.roots.single().ok()?;
        Some(TerrainView {
            config,
            data: surface_data(base, baked),
            volume: volume.unwrap_or(TerrainVolume::empty()),
            water,
        })
    }
}

thread_local! {
    /// The view [`with_terrain_view`] installed on this thread, as an erased
    /// `*const TerrainView`, or null. Only `with_terrain_view` stores a
    /// non-null value (its own view, then on exit the value it found) and
    /// `clear_terrain_bridge` stores null, so the slot always holds null or
    /// the view of a `with_terrain_view` call still running on this thread.
    static TERRAIN_VIEW: Cell<*const ()> = const { Cell::new(std::ptr::null()) };
    /// Edits scripts queued on this thread, taken by [`drain_pending_terrain`].
    static PENDING_TERRAIN: RefCell<Vec<TerrainCommand>> = const { RefCell::new(Vec::new()) };
}

/// Run `f` with `view` as the terrain `eustress::terrain` reads and edits on
/// this thread, then put back whatever was installed before, on return and
/// on unwind alike. `None` runs `f` without terrain.
///
/// The view borrows the root's components, so it can only be installed for
/// the span of a call: [`crate::soul::rune_play::drive_rune_frame`] wraps
/// the lifecycle callbacks in this.
pub fn with_terrain_view<R>(view: Option<&TerrainView<'_>>, f: impl FnOnce() -> R) -> R {
    // Puts the previous view back when dropped.
    struct Restore(*const ());
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0;
            let _ = TERRAIN_VIEW.try_with(|slot| slot.set(previous));
        }
    }

    let installed = view.map_or(std::ptr::null(), |view| {
        (view as *const TerrainView<'_>).cast::<()>()
    });
    let previous = TERRAIN_VIEW.with(|slot| slot.replace(installed));
    let _restore = Restore(previous);
    f()
}

/// Call `read` with the terrain installed on this thread, or return
/// `fallback` when there is none.
fn with_view<R>(fallback: R, read: impl FnOnce(&TerrainView<'_>) -> R) -> R {
    let installed = TERRAIN_VIEW
        .try_with(|slot| slot.get())
        .unwrap_or(std::ptr::null());
    if installed.is_null() {
        return fallback;
    }
    // SAFETY: the slot holds null or the view of a `with_terrain_view` call
    // still running on this thread. A call installs its own view and, on
    // return and on unwind alike, puts back the value it found, which by the
    // same rule was null or the view of a call enclosing it, still running
    // since calls on one thread nest; `clear_terrain_bridge` only stores
    // null. The installing call borrows that `TerrainView`, and everything
    // the view borrows, shared and alive until it returns, and it encloses
    // this frame, so it cannot return first. `read`'s higher-ranked signature
    // keeps the reference out of `R`.
    let view = unsafe { &*installed.cast::<TerrainView<'_>>() };
    read(view)
}

/// Queue `command` when this frame takes terrain edits: `true` once queued.
fn queue(command: TerrainCommand) -> bool {
    if !with_view(false, |view| view.has_raster()) {
        return false;
    }
    PENDING_TERRAIN.with(|pending| pending.borrow_mut().push(command));
    true
}

/// Take the edits scripts queued on this thread since the last call, in call
/// order.
pub fn drain_pending_terrain() -> Vec<TerrainCommand> {
    PENDING_TERRAIN.with(|pending| std::mem::take(&mut *pending.borrow_mut()))
}

/// Uninstall the terrain view and drop any queued edits. The per-frame
/// driver drains before it tears down, so this only discards edits nothing
/// would apply.
pub fn clear_terrain_bridge() {
    let _ = TERRAIN_VIEW.try_with(|slot| slot.set(std::ptr::null()));
    let _ = PENDING_TERRAIN.try_with(|pending| pending.borrow_mut().clear());
}

/// Apply one frame's queued edits to the Space's terrain root as scripted
/// edits, and log the ones the terrain refused.
pub fn apply_script_edits(world: &mut World, edits: Vec<TerrainCommand>) {
    let total = edits.len();
    let mut refused = 0usize;
    let mut first: Option<String> = None;
    for result in apply_terrain_commands(world, edits, TerrainCommandOrigin::Script) {
        if let Err(reason) = result {
            refused += 1;
            if first.is_none() {
                first = Some(reason);
            }
        }
    }
    if let Some(reason) = first {
        warn!("[Rune Script] terrain: {refused} of {total} edit(s) refused, the first because: {reason}");
    }
}

// ============================================================================
// Arguments
// ============================================================================

/// `v` in single precision, when every component stays finite.
fn to_vec3(v: &Vector3) -> Option<Vec3> {
    let p = Vec3::new(v.x as f32, v.y as f32, v.z as f32);
    p.is_finite().then_some(p)
}

/// A length or strength in single precision, when finite and positive.
fn positive(value: f64) -> Option<f32> {
    let value = value as f32;
    (value.is_finite() && value > 0.0).then_some(value)
}

/// A size whose every component is finite and positive.
fn to_size(v: &Vector3) -> Option<Vec3> {
    to_vec3(v).filter(|size| size.min_element() > 0.0)
}

/// The world-axis-aligned box between opposite corners `a` and `b`, in
/// either order, as `(min, max)`, when it has volume.
fn to_region(a: &Vector3, b: &Vector3) -> Option<(Vec3, Vec3)> {
    let (a, b) = (to_vec3(a)?, to_vec3(b)?);
    let (min, max) = (a.min(b), a.max(b));
    ((max - min).min_element() > 0.0).then_some((min, max))
}

/// A Roblox `Orientation` in degrees as a unit rotation: the one
/// `CFrame.fromOrientation` builds, roll about Z, then pitch about X, then
/// yaw about Y.
fn to_rotation(degrees: &Vector3) -> Option<Quat> {
    let d = to_vec3(degrees)?;
    let rotation = Quat::from_rotation_y(d.y.to_radians())
        * Quat::from_rotation_x(d.x.to_radians())
        * Quat::from_rotation_z(d.z.to_radians());
    Some(rotation.normalize())
}

/// What a fill of material `name` puts inside its shape.
fn fill_named(name: &str) -> Option<TerrainFill> {
    TerrainFill::from_material_name(name)
}

/// The solid material `name`: neither Air nor Water, in any spelling
/// `TerrainFill::from_material_name` accepts (it reads some spellings of
/// Water as the Water material rather than the Water fill).
fn solid_named(name: &str) -> Option<TerrainMaterial> {
    match TerrainFill::from_material_name(name)? {
        TerrainFill::Material(material) if material != TerrainMaterial::Water => Some(material),
        _ => None,
    }
}

/// The sculpt mode `name` ("raise", "lower", "smooth" or "flatten", in any
/// case); "flatten" levels the ground to `height`.
fn sculpt_mode(name: &str, height: f32) -> Option<TerrainSculptMode> {
    match name.trim().to_ascii_lowercase().as_str() {
        "raise" => Some(TerrainSculptMode::Raise),
        "lower" => Some(TerrainSculptMode::Lower),
        "smooth" => Some(TerrainSculptMode::Smooth),
        "flatten" => Some(TerrainSculptMode::Flatten { height }),
        _ => None,
    }
}

// ============================================================================
// Edits
// ============================================================================

/// Fill the ball of `radius` around `center` with `material`. `true` once
/// queued; `false`, with nothing queued, for invalid arguments or where
/// scripts cannot edit terrain.
#[rune::function]
fn fill_ball(center: &Vector3, radius: f64, material: &str) -> bool {
    let (Some(center), Some(radius), Some(fill)) =
        (to_vec3(center), positive(radius), fill_named(material))
    else {
        return false;
    };
    queue(TerrainCommand::FillBall { center, radius, fill })
}

/// Fill the world-axis-aligned box of `size` centred on `center` with
/// `material`. `true` once queued.
#[rune::function]
fn fill_block(center: &Vector3, size: &Vector3, material: &str) -> bool {
    let (Some(center), Some(size), Some(fill)) =
        (to_vec3(center), to_size(size), fill_named(material))
    else {
        return false;
    };
    queue(TerrainCommand::FillBlock { center, rotation: Quat::IDENTITY, size, fill })
}

/// Fill the box of `size` centred on `center` and turned by `orientation`
/// with `material`. `orientation` is a Roblox `Orientation` in degrees (the
/// angles `dm::get_vector3(part, "Orientation")` reads): the rotation
/// `CFrame.fromOrientation` builds, roll about Z, then pitch about X, then
/// yaw about Y. `true` once queued.
#[rune::function]
fn fill_block_rotated(
    center: &Vector3,
    size: &Vector3,
    orientation: &Vector3,
    material: &str,
) -> bool {
    let (Some(center), Some(size), Some(rotation), Some(fill)) = (
        to_vec3(center),
        to_size(size),
        to_rotation(orientation),
        fill_named(material),
    ) else {
        return false;
    };
    queue(TerrainCommand::FillBlock { center, rotation, size, fill })
}

/// Fill the upright cylinder of `height` and `radius` centred on `center`
/// with `material`. `true` once queued.
#[rune::function]
fn fill_cylinder(center: &Vector3, height: f64, radius: f64, material: &str) -> bool {
    let (Some(center), Some(height), Some(radius), Some(fill)) = (
        to_vec3(center),
        positive(height),
        positive(radius),
        fill_named(material),
    ) else {
        return false;
    };
    queue(TerrainCommand::FillCylinder { center, rotation: Quat::IDENTITY, height, radius, fill })
}

/// Fill the world-axis-aligned box between opposite corners `min` and `max`
/// (either order) with `material`. `true` once queued.
#[rune::function]
fn fill_region(min: &Vector3, max: &Vector3, material: &str) -> bool {
    let (Some((min, max)), Some(fill)) = (to_region(min, max), fill_named(material)) else {
        return false;
    };
    queue(TerrainCommand::FillRegion { min, max, fill })
}

/// Turn material `from` into `to` inside the box between opposite corners
/// `min` and `max`. Both are solid materials: `"Air"` and `"Water"` are
/// refused. `true` once queued.
#[rune::function]
fn replace_material(min: &Vector3, max: &Vector3, from: &str, to: &str) -> bool {
    let (Some((min, max)), Some(from), Some(to)) =
        (to_region(min, max), solid_named(from), solid_named(to))
    else {
        return false;
    };
    queue(TerrainCommand::ReplaceMaterial { min, max, from, to })
}

/// Sculpt the ground under a brush of `radius` at `center`. `mode` is
/// "raise", "lower", "smooth" or "flatten" in any case, and "flatten" levels
/// the ground to `center`'s height. `strength` is positive; the Studio's
/// brushes run from 0 to 1. `true` once queued.
#[rune::function]
fn sculpt(mode: &str, center: &Vector3, radius: f64, strength: f64) -> bool {
    let (Some(center), Some(radius), Some(strength)) =
        (to_vec3(center), positive(radius), positive(strength))
    else {
        return false;
    };
    let Some(mode) = sculpt_mode(mode, center.y) else {
        return false;
    };
    queue(TerrainCommand::Sculpt { mode, center, radius, strength })
}

/// Paint solid `material` onto the ground under a brush of `radius` at
/// `center`. `strength` is positive; the Studio's paint brush runs from 0 to
/// 1. `true` once queued.
#[rune::function]
fn paint(center: &Vector3, radius: f64, material: &str, strength: f64) -> bool {
    let (Some(center), Some(radius), Some(material), Some(strength)) = (
        to_vec3(center),
        positive(radius),
        solid_named(material),
        positive(strength),
    ) else {
        return false;
    };
    queue(TerrainCommand::Paint { center, radius, material: material.to_u8(), strength })
}

/// Clear the whole terrain: every column becomes a hole, and the volumetric
/// edits and the water are emptied. Stop restores it, like any scripted
/// edit. `true` once queued.
#[rune::function]
fn clear() -> bool {
    queue(TerrainCommand::Clear)
}

// ============================================================================
// Reads
// ============================================================================

/// Downward samples, at most, before [`column_top`] bisects.
const MAX_COLUMN_STEPS: usize = 4096;

/// Where the solid terrain in a world column begins, from above.
#[derive(Clone, Copy, Debug)]
struct ColumnTop {
    /// World Y of that surface.
    height: f32,
    /// A volumetric edit shapes that surface: an add or a carve decides the
    /// field there, or the column is a hole, where only edits make solid.
    edited: bool,
}

/// Whether `x, z` lies inside the terrain's chunk grid.
fn in_footprint(config: &TerrainConfig, x: f32, z: f32) -> bool {
    let (min, max) = config.footprint_xz();
    x >= min.x && x <= max.x && z >= min.y && z <= max.y
}

/// Whether the raster cell under world `x, z` is a hole: a column without
/// ground (see `TerrainData::cell_is_hole`).
fn column_is_hole(config: &TerrainConfig, data: &TerrainData, x: f32, z: f32) -> bool {
    cache_cell_at_world(config, data, x, z).is_some_and(|cell| {
        data.cell_is_hole(cell.y as usize * data.cache_width as usize + cell.x as usize)
    })
}

/// The top of the solid terrain over world `x, z`, or `None` where the
/// column holds none: a hole with nothing filled in, a point outside the
/// terrain, or a terrain without a height raster.
///
/// A column whose chunk holds no volumetric edits is its heightfield. Else
/// the search walks down from above everything the chunk can hold, half a
/// lattice cell at a time, and bisects the first step that lands in solid.
/// A hole column has no ground, so there only the edits can make solid.
fn column_top(view: &TerrainView<'_>, x: f32, z: f32) -> Option<ColumnTop> {
    let TerrainView { config, data, volume, .. } = *view;
    if !view.has_raster() || !x.is_finite() || !z.is_finite() || !in_footprint(config, x, z) {
        return None;
    }
    let hole = column_is_hole(config, data, x, z);
    let ground = if hole {
        None
    } else {
        Some(height_at_world(config, data, x, z)).filter(|height| height.is_finite())
    };
    let chunk_size = config.chunk_size.max(1e-3);
    let chunk = IVec2::new((x / chunk_size).floor() as i32, (z / chunk_size).floor() as i32);
    let Some((lowest, highest)) = chunk_volume_y_range(chunk, config, volume) else {
        return ground.map(|height| ColumnTop { height, edited: false });
    };

    let cell = lattice_cell_size(config);
    let solid_at = |y: f32| {
        let p = Vec3::new(x, y, z);
        let value = if hole {
            let (add, carve) = volume.edit_distances(cell, p);
            FieldSample::compose(f32::INFINITY, add, carve).value
        } else {
            sample_field(config, data, volume, p)
        };
        value < 0.0
    };
    let top = ground.map_or(highest, |g| g.max(highest)) + 2.0 * cell;
    let bottom = ground.map_or(lowest, |g| g.min(lowest)) - 2.0 * cell;
    let steps = (((top - bottom) / (0.5 * cell)).ceil() as usize).clamp(1, MAX_COLUMN_STEPS);
    let step = (top - bottom) / steps as f32;
    let mut air = top;
    for i in 1..=steps {
        let y = top - step * i as f32;
        if solid_at(y) {
            let mut solid = y;
            for _ in 0..24 {
                let mid = 0.5 * (air + solid);
                if solid_at(mid) {
                    solid = mid;
                } else {
                    air = mid;
                }
            }
            // The term deciding the field just inside the surface, which is
            // how the meshes choose the colour they draw it in.
            let edited = hole
                || sample_field_parts(config, data, volume, Vec3::new(x, solid, z)).term()
                    != FieldTerm::Heightfield;
            return Some(ColumnTop { height: solid, edited });
        }
        air = y;
    }
    None
}

/// The built-in material of the ground's material-map cell under `x, z`.
fn ground_material(view: &TerrainView<'_>, x: f32, z: f32) -> Option<TerrainMaterial> {
    material_at_world(view.config, view.data, x, z)
        .and_then(|sample| TerrainMaterial::from_u8(sample.primary))
}

/// World height of the top of the solid terrain over `x, z`, volumetric
/// fills and carves included. `NaN` where the column holds no solid terrain
/// (a hole with nothing filled in), outside the terrain, and without
/// terrain.
#[rune::function]
fn height_at(x: f64, z: f64) -> f64 {
    with_view(f64::NAN, |view| {
        column_top(view, x as f32, z as f32).map_or(f64::NAN, |top| f64::from(top.height))
    })
}

/// Roblox name of the material at the surface `height_at` finds over
/// `x, z`, as the meshes colour it: a volumetric edit's material where one
/// shapes that surface (Rock where the edit carries none), else the material
/// painted on the ground there. `""` where `height_at` is `NaN`, and for a
/// Space's custom material.
#[rune::function]
fn material_at(x: f64, z: f64) -> String {
    with_view(String::new(), |view| {
        let (x, z) = (x as f32, z as f32);
        let Some(top) = column_top(view, x, z) else {
            return String::new();
        };
        let material = if top.edited {
            Some(
                edit_material_at(view.config, view.volume, Vec3::new(x, top.height, z))
                    .unwrap_or(TerrainMaterial::Rock),
            )
        } else {
            ground_material(view, x, z)
        };
        material.map_or_else(String::new, |material| material.name().to_string())
    })
}

/// How full of solid terrain the lattice cell centred on `point` is, from 0
/// (empty) to 1 (solid), counting the heightfield and the volumetric edits
/// together. `0` outside the terrain and without terrain.
#[rune::function]
fn occupancy_at(point: &Vector3) -> f64 {
    let Some(point) = to_vec3(point) else {
        return 0.0;
    };
    with_view(0.0, |view| {
        if !view.has_raster() || !in_footprint(view.config, point.x, point.z) {
            return 0.0;
        }
        let cell = lattice_cell_size(view.config);
        // Without the water: a voxel under the water surface reads the share
        // of it the water covers, and water is not solid terrain.
        let (_, occupancy) = read_voxels(
            view.config,
            view.data,
            view.volume,
            None,
            point - Vec3::splat(0.5 * cell),
            cell,
            UVec3::ONE,
        );
        occupancy.first().map_or(0.0, |full| f64::from(*full))
    })
}

/// Whether this frame has terrain a script can read and edit: a terrain root
/// with a height raster.
#[rune::function]
fn has_terrain() -> bool {
    with_view(false, |view| view.has_raster())
}

/// The `eustress::terrain` module.
pub fn create_terrain_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["terrain"])?;
    m.function_meta(fill_ball)?;
    m.function_meta(fill_block)?;
    m.function_meta(fill_block_rotated)?;
    m.function_meta(fill_cylinder)?;
    m.function_meta(fill_region)?;
    m.function_meta(replace_material)?;
    m.function_meta(sculpt)?;
    m.function_meta(paint)?;
    m.function_meta(clear)?;
    m.function_meta(height_at)?;
    m.function_meta(material_at)?;
    m.function_meta(occupancy_at)?;
    m.function_meta(has_terrain)?;
    Ok(m)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::prelude::*;
    use eustress_common::terrain::api::{TerrainCommand, TerrainFill, TerrainSculptMode};
    use eustress_common::terrain::{
        apply_sphere, cache_cell_at_world, material_cell, CsgOp, TerrainConfig, TerrainData,
        TerrainMaterial, TerrainVolume, MATERIAL_SLOT_NONE,
    };

    use super::{
        clear_terrain_bridge, create_terrain_module, drain_pending_terrain, with_terrain_view,
        TerrainView,
    };

    /// One script calling every function of the module.
    const PROGRAM: &str = r#"
use eustress::Vector3;
use eustress::terrain;

pub fn every_function() {
    let at = Vector3::new(8.0, 10.0, 8.0);
    let size = Vector3::new(4.0, 4.0, 4.0);
    let turn = Vector3::new(0.0, 45.0, 0.0);
    let edits = [
        terrain::fill_ball(at, 4.0, "Grass"),
        terrain::fill_block(at, size, "Rock"),
        terrain::fill_block_rotated(at, size, turn, "Rock"),
        terrain::fill_cylinder(at, 6.0, 2.0, "Sand"),
        terrain::fill_region(at, size, "Air"),
        terrain::replace_material(at, size, "Grass", "Snow"),
        terrain::sculpt("raise", at, 8.0, 0.5),
        terrain::paint(at, 8.0, "Mud", 1.0),
        terrain::clear(),
    ];
    let reads = (
        terrain::height_at(8.0, 8.0),
        terrain::material_at(8.0, 8.0),
        terrain::occupancy_at(at),
        terrain::has_terrain(),
    );
    (edits, reads)
}

pub fn refused(nan) {
    let at = Vector3::new(8.0, 10.0, 8.0);
    let size = Vector3::new(4.0, 4.0, 4.0);
    [
        terrain::fill_ball(at, 4.0, "NotAMaterial"),
        terrain::fill_ball(Vector3::new(nan, 10.0, 8.0), 4.0, "Grass"),
        terrain::fill_ball(at, 0.0, "Grass"),
        terrain::fill_ball(at, -1.0, "Grass"),
        terrain::fill_ball(at, nan, "Grass"),
        terrain::fill_block(at, Vector3::new(4.0, 0.0, 4.0), "Rock"),
        terrain::fill_block_rotated(at, size, Vector3::new(0.0, nan, 0.0), "Rock"),
        terrain::fill_cylinder(at, 0.0, 2.0, "Sand"),
        terrain::fill_region(at, Vector3::new(12.0, 10.0, 12.0), "Rock"),
        terrain::replace_material(at, size, "Water", "Rock"),
        terrain::replace_material(at, size, "Rock", "Air"),
        terrain::sculpt("bulldoze", at, 8.0, 0.5),
        terrain::sculpt("raise", at, 8.0, 0.0),
        terrain::paint(at, 8.0, "Air", 1.0),
        terrain::paint(at, 8.0, "Mud", nan),
        terrain::paint(at, 8.0, "wa_ter", 1.0),
    ]
}

pub fn fill_ball_once() {
    terrain::fill_ball(Vector3::new(1.5, 2.25, -3.0), 4.0, "grass")
}

pub fn every_edit() {
    let at = Vector3::new(1.0, 2.0, 3.0);
    let size = Vector3::new(4.0, 6.0, 8.0);
    [
        terrain::fill_block(at, size, "Rock"),
        terrain::fill_block_rotated(at, size, Vector3::new(0.0, 90.0, 0.0), "Air"),
        terrain::fill_cylinder(at, 6.0, 2.0, "Water"),
        terrain::fill_region(Vector3::new(4.0, 5.0, 6.0), at, "Sand"),
        terrain::replace_material(at, size, "Grass", "Snow"),
        terrain::sculpt("Flatten", at, 8.0, 0.5),
        terrain::sculpt("smooth", at, 8.0, 0.25),
        terrain::paint(at, 8.0, "Mud", 1.0),
        terrain::clear(),
    ]
}

pub fn column(x, z) {
    (terrain::height_at(x, z), terrain::material_at(x, z))
}

pub fn occupancy(x, y, z) {
    terrain::occupancy_at(Vector3::new(x, y, z))
}

pub fn has() {
    terrain::has_terrain()
}
"#;

    /// [`PROGRAM`] compiled against the engine's module set, the one Play,
    /// the command bar, the editor analyzer and the LSP share, so a function
    /// missing from `engine_rune_modules()` fails here.
    fn compile() -> rune::Vm {
        let mut context = rune::Context::with_default_modules().expect("default modules");
        for module in crate::soul::rune_api::engine_rune_modules() {
            context.install(module).expect("engine module installs");
        }
        let mut sources = rune::Sources::new();
        sources
            .insert(rune::Source::memory(PROGRAM).expect("source"))
            .expect("insert");
        let mut diagnostics = rune::Diagnostics::new();
        let built = rune::prepare(&mut sources)
            .with_context(&context)
            .with_diagnostics(&mut diagnostics)
            .build();
        let unit = match built {
            Ok(unit) => unit,
            Err(_) => {
                let mut report = rune::termcolor::Buffer::no_color();
                let _ = diagnostics.emit(&mut report, &sources);
                panic!(
                    "the terrain script did not compile:\n{}",
                    String::from_utf8_lossy(report.as_slice())
                );
            }
        };
        rune::Vm::new(
            Arc::new(context.runtime().expect("runtime")),
            Arc::new(unit),
        )
    }

    /// Call script function `name` and convert what it returns.
    fn call<T: rune::FromValue>(
        vm: &mut rune::Vm,
        name: &str,
        args: impl rune::runtime::GuardedArgs,
    ) -> T {
        let value = vm
            .call([name], args)
            .unwrap_or_else(|e| panic!("`{name}` failed: {e}"));
        rune::from_value(value).unwrap_or_else(|e| panic!("`{name}` returned something else: {e}"))
    }

    /// Flat ground at 10 m painted Rock: 3 x 3 chunks of 32 m at 32 cells,
    /// so one metre cells spanning world -32 to 64 on X and Z.
    fn flat_terrain() -> (TerrainConfig, TerrainData) {
        let config = TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 32,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            height_offset: 10.0,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        data.material_cache =
            vec![material_cell(TerrainMaterial::Rock.to_u8()); data.height_cache.len()];
        (config, data)
    }

    #[test]
    fn module_builds() {
        assert!(create_terrain_module().is_ok());
    }

    /// Every function resolves (`compile` panics with the diagnostics if one
    /// does not), and outside a terrain frame, or over procedural terrain,
    /// every edit is refused and every read is empty.
    #[test]
    fn edits_are_refused_and_reads_empty_without_editable_terrain() {
        clear_terrain_bridge();
        let mut vm = compile();
        type Results = (Vec<bool>, (f64, String, f64, bool));

        let (edits, (height, material, occupancy, has)): Results =
            call(&mut vm, "every_function", ());
        assert_eq!(edits, vec![false; 9], "an edit was accepted outside a terrain frame");
        assert!(height.is_nan());
        assert_eq!(material, "");
        assert_eq!(occupancy, 0.0);
        assert!(!has);

        let config = TerrainConfig::default();
        let data = TerrainData::procedural();
        let volume = TerrainVolume::new();
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let (edits, (height, material, occupancy, has)): Results =
            with_terrain_view(Some(&view), || call(&mut vm, "every_function", ()));
        assert_eq!(edits, vec![false; 9], "an edit was accepted over procedural terrain");
        assert!(height.is_nan());
        assert_eq!(material, "");
        assert_eq!(occupancy, 0.0);
        assert!(!has);

        assert!(drain_pending_terrain().is_empty());
    }

    #[test]
    fn invalid_arguments_are_refused_and_queue_nothing() {
        clear_terrain_bridge();
        let (config, data) = flat_terrain();
        let volume = TerrainVolume::new();
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let mut vm = compile();

        let results: Vec<bool> =
            with_terrain_view(Some(&view), || call(&mut vm, "refused", (f64::NAN,)));
        assert_eq!(results.len(), 16);
        for (index, accepted) in results.iter().enumerate() {
            assert!(!accepted, "call {index} in `refused` was accepted");
        }
        assert!(drain_pending_terrain().is_empty(), "a refused call queued an edit");
    }

    #[test]
    fn fill_ball_queues_one_command_with_the_converted_values() {
        clear_terrain_bridge();
        let (config, data) = flat_terrain();
        let volume = TerrainVolume::new();
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let mut vm = compile();

        let queued: bool = with_terrain_view(Some(&view), || call(&mut vm, "fill_ball_once", ()));
        assert!(queued);
        assert_eq!(
            drain_pending_terrain(),
            vec![TerrainCommand::FillBall {
                center: Vec3::new(1.5, 2.25, -3.0),
                radius: 4.0,
                fill: TerrainFill::Material(TerrainMaterial::Grass),
            }]
        );
        assert!(drain_pending_terrain().is_empty(), "a drain empties the queue");
    }

    #[test]
    fn every_edit_queues_its_command() {
        clear_terrain_bridge();
        let (config, data) = flat_terrain();
        let volume = TerrainVolume::new();
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let mut vm = compile();

        let accepted: Vec<bool> = with_terrain_view(Some(&view), || call(&mut vm, "every_edit", ()));
        assert_eq!(accepted, vec![true; 9]);

        let queued = drain_pending_terrain();
        assert_eq!(queued.len(), 9);
        let center = Vec3::new(1.0, 2.0, 3.0);
        let size = Vec3::new(4.0, 6.0, 8.0);
        assert_eq!(
            queued[0],
            TerrainCommand::FillBlock {
                center,
                rotation: Quat::IDENTITY,
                size,
                fill: TerrainFill::Material(TerrainMaterial::Rock),
            }
        );
        match &queued[1] {
            TerrainCommand::FillBlock { center: c, rotation, size: s, fill } => {
                assert_eq!((*c, *s, *fill), (center, size, TerrainFill::Air));
                let quarter_turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
                assert!(rotation.abs_diff_eq(quarter_turn, 1e-6), "rotation {rotation:?}");
            }
            other => panic!("expected a turned FillBlock, got {other:?}"),
        }
        assert_eq!(
            queued[2],
            TerrainCommand::FillCylinder {
                center,
                rotation: Quat::IDENTITY,
                height: 6.0,
                radius: 2.0,
                fill: TerrainFill::Water,
            }
        );
        assert_eq!(
            queued[3],
            TerrainCommand::FillRegion {
                min: center,
                max: Vec3::new(4.0, 5.0, 6.0),
                fill: TerrainFill::Material(TerrainMaterial::Sand),
            }
        );
        assert_eq!(
            queued[4],
            TerrainCommand::ReplaceMaterial {
                min: center,
                max: size,
                from: TerrainMaterial::Grass,
                to: TerrainMaterial::Snow,
            }
        );
        assert_eq!(
            queued[5],
            TerrainCommand::Sculpt {
                mode: TerrainSculptMode::Flatten { height: 2.0 },
                center,
                radius: 8.0,
                strength: 0.5,
            }
        );
        assert_eq!(
            queued[6],
            TerrainCommand::Sculpt {
                mode: TerrainSculptMode::Smooth,
                center,
                radius: 8.0,
                strength: 0.25,
            }
        );
        assert_eq!(
            queued[7],
            TerrainCommand::Paint {
                center,
                radius: 8.0,
                material: TerrainMaterial::Mud.to_u8(),
                strength: 1.0,
            }
        );
        assert_eq!(queued[8], TerrainCommand::Clear);
    }

    #[test]
    fn reads_answer_from_the_installed_terrain() {
        clear_terrain_bridge();
        let (config, data) = flat_terrain();
        let volume = TerrainVolume::new();
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let mut vm = compile();

        with_terrain_view(Some(&view), || {
            assert!(call::<bool>(&mut vm, "has", ()));
            let (height, material): (f64, String) = call(&mut vm, "column", (8.0f64, 8.0f64));
            assert!((height - 10.0).abs() < 1e-4, "height {height}");
            assert_eq!(material, "Rock");
            let (height, material): (f64, String) = call(&mut vm, "column", (500.0f64, 8.0f64));
            assert!(height.is_nan(), "outside the terrain, height {height}");
            assert_eq!(material, "");
            let (height, _): (f64, String) = call(&mut vm, "column", (f64::NAN, 8.0f64));
            assert!(height.is_nan());
        });

        let (height, material): (f64, String) = call(&mut vm, "column", (8.0f64, 8.0f64));
        assert!(height.is_nan(), "the view outlived its scope");
        assert_eq!(material, "");
    }

    #[test]
    fn volumetric_edits_decide_the_column_top() {
        clear_terrain_bridge();
        let (config, data) = flat_terrain();
        let mut volume = TerrainVolume::new();
        let added = apply_sphere(
            &config,
            &mut volume,
            Vec3::new(8.0, 20.0, 8.0),
            4.0,
            CsgOp::Add,
            Some(TerrainMaterial::Sand),
        );
        assert!(!added.is_empty(), "the ball was not added");
        let carved = apply_sphere(
            &config,
            &mut volume,
            Vec3::new(40.0, 10.0, 40.0),
            3.0,
            CsgOp::Carve,
            Some(TerrainMaterial::Basalt),
        );
        assert!(!carved.is_empty(), "the crater was not carved");
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let mut vm = compile();

        with_terrain_view(Some(&view), || {
            // On top of the ball floating over the ground.
            let (height, material): (f64, String) = call(&mut vm, "column", (8.0f64, 8.0f64));
            assert!((height - 24.0).abs() < 0.25, "ball top {height}");
            assert_eq!(material, "Sand");
            // At the bottom of the crater.
            let (height, material): (f64, String) = call(&mut vm, "column", (40.0f64, 40.0f64));
            assert!((height - 7.0).abs() < 0.25, "crater floor {height}");
            assert_eq!(material, "Basalt");
            // One cell outside the crater's wall the carve painted Basalt on
            // the lattice, but the ground draws the surface there.
            let (height, material): (f64, String) = call(&mut vm, "column", (44.0f64, 40.0f64));
            assert!((height - 10.0).abs() < 0.25, "ground beside the crater {height}");
            assert_eq!(material, "Rock");
            // A chunk without edits reads its heightfield.
            let (height, material): (f64, String) = call(&mut vm, "column", (56.0f64, -24.0f64));
            assert!((height - 10.0).abs() < 1e-4, "untouched ground {height}");
            assert_eq!(material, "Rock");
        });
    }

    #[test]
    fn a_hole_column_has_no_ground_but_keeps_its_fills() {
        clear_terrain_bridge();
        let (config, mut data) = flat_terrain();
        data.sparse_surface = true;
        let hole = cache_cell_at_world(&config, &data, 16.0, 16.0).expect("on the raster");
        let index = hole.y as usize * data.cache_width as usize + hole.x as usize;
        data.material_cache[index] = [MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0];
        let empty = TerrainVolume::new();
        let mut vm = compile();

        let view = TerrainView { config: &config, data: &data, volume: &empty, water: None };
        with_terrain_view(Some(&view), || {
            let (height, material): (f64, String) = call(&mut vm, "column", (16.0f64, 16.0f64));
            assert!(height.is_nan(), "a hole read height {height}");
            assert_eq!(material, "");
            let (height, material): (f64, String) = call(&mut vm, "column", (20.0f64, 20.0f64));
            assert!((height - 10.0).abs() < 1e-4, "ground beside the hole {height}");
            assert_eq!(material, "Rock");
        });

        // A ball below where the ground would be shows through the hole.
        let mut filled = TerrainVolume::new();
        let added = apply_sphere(
            &config,
            &mut filled,
            Vec3::new(16.0, 0.0, 16.0),
            3.0,
            CsgOp::Add,
            Some(TerrainMaterial::Sand),
        );
        assert!(!added.is_empty(), "the ball was not added");
        let view = TerrainView { config: &config, data: &data, volume: &filled, water: None };
        with_terrain_view(Some(&view), || {
            let (height, material): (f64, String) = call(&mut vm, "column", (16.0f64, 16.0f64));
            assert!((height - 3.0).abs() < 0.25, "ball top in the hole {height}");
            assert_eq!(material, "Sand");
        });
    }

    #[test]
    fn occupancy_reads_the_combined_field() {
        clear_terrain_bridge();
        let (config, data) = flat_terrain();
        let volume = TerrainVolume::new();
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let mut vm = compile();

        with_terrain_view(Some(&view), || {
            let below: f64 = call(&mut vm, "occupancy", (8.0f64, 0.0f64, 8.0f64));
            assert!(below > 0.99, "10 m underground, occupancy {below}");
            let above: f64 = call(&mut vm, "occupancy", (8.0f64, 20.0f64, 8.0f64));
            assert!(above < 0.01, "10 m up, occupancy {above}");
            let outside: f64 = call(&mut vm, "occupancy", (500.0f64, 0.0f64, 8.0f64));
            assert_eq!(outside, 0.0);
        });
        let unseen: f64 = call(&mut vm, "occupancy", (8.0f64, 0.0f64, 8.0f64));
        assert_eq!(unseen, 0.0);
    }

    #[test]
    fn a_view_lasts_exactly_its_scope() {
        clear_terrain_bridge();
        let (config, data) = flat_terrain();
        let volume = TerrainVolume::new();
        let view = TerrainView { config: &config, data: &data, volume: &volume, water: None };
        let mut vm = compile();

        assert!(!call::<bool>(&mut vm, "has", ()));
        with_terrain_view(Some(&view), || {
            assert!(call::<bool>(&mut vm, "has", ()));
            with_terrain_view(None, || assert!(!call::<bool>(&mut vm, "has", ())));
            assert!(call::<bool>(&mut vm, "has", ()), "the outer view is back after the inner scope");
        });
        assert!(!call::<bool>(&mut vm, "has", ()), "the view outlived its scope");
    }
}
