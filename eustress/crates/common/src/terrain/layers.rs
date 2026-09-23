//! Non-destructive terrain layers: splines, stamps, flatten pads, noise and
//! material fills stacked on top of a terrain's editable base.
//!
//! ## Base and baked
//! A terrain root's [`TerrainData`] is its editable BASE. Every brush, Part
//! to Terrain, heightmap import and undo write it, and Save writes it; layers
//! never touch it. A root with layers also carries a
//! [`TerrainBaked`]: a full `TerrainData` holding the base with every enabled
//! layer applied. The meshers, colliders, surface material, picking raycasts
//! and gameplay material queries read that, always through [`surface_data`].
//! A root without layers has no `TerrainBaked` and every reader gets the base
//! itself, so a terrain without layers pays nothing for them.
//!
//! ## Keeping the bake current
//! Writers keep marking what they touched in `TerrainDirtyChunks`. Every
//! raster mark also queues its region for a re-bake ([`TerrainRebake`]), and
//! `apply_terrain_dirty_chunks` re-bakes it through [`TerrainBaked::rebake`]
//! before it remeshes a single chunk, so no writer needs to know layers
//! exist. Relative edits (a raise adds to the height under the brush) read
//! the base, so a brush never compounds a layer into the base.
//!
//! A layer change reaches the bake through [`TerrainBaked::set_layers`]. The
//! next re-bake diffs the old list against the new one ([`layer_changes`]),
//! re-bakes the old and new bounds of every layer added, removed, moved,
//! edited or reordered (a spline's corridor rather than its whole box), and
//! hands those regions back for remeshing. Re-baking the old ones is what
//! puts the base back where a layer used to be.
//!
//! ## Evaluation
//! Layers apply in `order`, ties broken by `id`. A cell starts from its base
//! height; every layer whose bounds hold the cell reshapes it in turn. Once
//! every height is final the layers paint materials in the same order, so a
//! material fill's slope and height rules see the finished ground. Heights
//! are worked in world metres and stored normalized in the base's band, as
//! the base stores them, but never clamped to it: the bake is never saved.
//!
//! A spline's elevation profile is the `road` module's: centripetal
//! Catmull-Rom through its control points, knots at the points' own Y plus
//! extra knots sampled from the ground on long stretches, and a natural cubic
//! through the knots. The extra knots sample the ground as the layers ordered
//! before the spline leave it. They read the base, so a base edit under one
//! of them moves the whole profile; every re-bake re-samples them and
//! re-bakes the spline's whole corridor when one moved.
//!
//! Everything but [`TerrainBaked`] being a component is engine free: plain
//! data in, cells out.

use std::collections::HashMap;

use bevy::prelude::*;

use super::height_query::{ensure_material_cache, height_at_world};
use super::material::{material_cell, paint_material_cell, MaterialCell, TerrainMaterial, MATERIAL_SLOT_NONE};
use super::road::{build_road_path_with, dense_path_xz, smoothstep};
use super::worldgen::noise::fbm;
use super::{HeightBand, TerrainConfig, TerrainData};

/// A ridge stamp is this fraction of its radius wide on either side of its
/// crest, and its radius long on either side of its centre.
const RIDGE_WIDTH_FRACTION: f32 = 0.3;
/// A crater's bowl reaches this fraction of the radius...
const CRATER_BOWL: f32 = 0.8;
/// ...and its rim peaks this far out, this high (a fraction of the stamp's
/// height), this wide (a fraction of the radius).
const CRATER_RIM_AT: f32 = 0.85;
const CRATER_RIM_HEIGHT: f32 = 0.35;
const CRATER_RIM_WIDTH: f32 = 0.1;
/// A canyon's wall takes this fraction of its shoulder; the rest is rim,
/// painted but not carved.
const CANYON_WALL_FRACTION: f32 = 0.25;
/// How far a path pulls its bed toward the profile at full smoothing.
const PATH_PULL: f32 = 0.5;
/// Extra elevation knots sit this far apart at zero smoothing...
const MIN_KNOT_SPACING: f32 = 5.0;
/// ...and this much farther at full smoothing: sparser knots, a smoother
/// profile.
const KNOT_SPACING_RANGE: f32 = 35.0;
/// Spline stations follow the raster's cell spacing within these limits.
const MIN_STATION_SPACING: f32 = 0.5;
const MAX_STATION_SPACING: f32 = 4.0;
/// Noise octaves past this add nothing a raster cell can show.
const MAX_NOISE_OCTAVES: u32 = 12;
/// A spline profile or knot that moved less than this (metres) has not moved.
const PROFILE_EPSILON: f32 = 1e-4;
/// Cells per side of the blocks a bake works in. A block no layer overlaps
/// is copied from the base row by row, so a whole-terrain bake costs little
/// more than a copy away from the layers.
const BAKE_BLOCK: usize = 64;
/// Smallest cell of a spline's segment grid, metres.
const MIN_GRID_CELL: f32 = 4.0;
/// Most cells a spline's segment grid has along either side.
const MAX_GRID_SIDE: usize = 256;
/// A layer that paints nothing at a cell.
const NO_PAINT: (u8, f32) = (MATERIAL_SLOT_NONE, 0.0);

// ============================================================================
// Layer descriptions
// ============================================================================

/// What a [`SplineLayer`] does to the ground along its corridor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SplineMode {
    /// Cuts and fills a flat bed to the smoothed elevation profile, with a
    /// shoulder blending back to the ground.
    #[default]
    Road,
    /// Paints the corridor and pulls it part of the way toward the profile,
    /// softening bumps without flattening them.
    Path,
    /// Carves `depth` below the profile with sloped banks. Never raises.
    River,
    /// Carves `depth` below the profile with steep walls. Never raises.
    Canyon,
    /// Raises a bed `depth` above the profile with sloped sides. Never lowers.
    Embankment,
}

impl SplineMode {
    pub const ALL: [SplineMode; 5] = [Self::Road, Self::Path, Self::River, Self::Canyon, Self::Embankment];

    /// The name a class property stores the mode under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Road => "Road",
            Self::Path => "Path",
            Self::River => "River",
            Self::Canyon => "Canyon",
            Self::Embankment => "Embankment",
        }
    }

    /// The mode [`Self::name`] names, ignoring case and surrounding spaces.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// The shape a [`StampLayer`] presses into the ground.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StampShape {
    /// A bowl `height` deep with a low raised rim.
    Crater,
    /// A dome `height` tall.
    #[default]
    Mound,
    /// A flat top `height` up, its edge fading over the falloff.
    Plateau,
    /// A crest `height` tall running along the stamp's local X, its radius
    /// long either side of the centre.
    Ridge,
}

impl StampShape {
    pub const ALL: [StampShape; 4] = [Self::Crater, Self::Mound, Self::Plateau, Self::Ridge];

    /// The name a class property stores the shape under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Crater => "Crater",
            Self::Mound => "Mound",
            Self::Plateau => "Plateau",
            Self::Ridge => "Ridge",
        }
    }

    /// The shape [`Self::name`] names, ignoring case and surrounding spaces.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shape| shape.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// How a [`StampLayer`] combines its shape with the ground under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StampBlend {
    /// Adds the shape to the ground wherever it stands.
    #[default]
    Add,
    /// Raises the ground to the shape stood on the stamp's Y, never lowers.
    Max,
    /// Lowers the ground to the shape stood on the stamp's Y, never raises.
    Min,
    /// Sets the ground to the shape stood on the stamp's Y.
    Replace,
}

impl StampBlend {
    pub const ALL: [StampBlend; 4] = [Self::Add, Self::Max, Self::Min, Self::Replace];

    /// The name a class property stores the blend under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Max => "Max",
            Self::Min => "Min",
            Self::Replace => "Replace",
        }
    }

    /// The blend [`Self::name`] names, ignoring case and surrounding spaces.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|blend| blend.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// A corridor along a Catmull-Rom spline through `points`: a road, path,
/// river, canyon or embankment (see [`SplineMode`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SplineLayer {
    pub mode: SplineMode,
    /// Control points in world space, in path order. Their Y is the profile
    /// at each point. Fewer than two make the layer inert.
    pub points: Vec<Vec3>,
    /// Full width of the bed, metres.
    pub width: f32,
    /// Width of the shoulder on either side of the bed, metres, over which
    /// the corridor blends back into the ground.
    pub shoulder_width: f32,
    /// How far a river or canyon carves below the profile, or an embankment
    /// rises above it, metres. The mode decides the direction, so the sign
    /// is ignored; a road or path ignores it.
    pub depth: f32,
    /// 0 to 1: how coarsely the profile follows the ground between control
    /// points, and how hard a path pulls toward it.
    pub smoothing: f32,
    /// Material slot painted over the bed, `None` to leave it.
    pub bed_material: Option<u8>,
    /// Material slot painted over the shoulders, fading out across them.
    pub shoulder_material: Option<u8>,
}

impl SplineLayer {
    fn half_width(&self) -> f32 {
        (self.width * 0.5).max(0.0)
    }

    fn shoulder(&self) -> f32 {
        self.shoulder_width.max(0.0)
    }

    /// Farthest from its centreline the corridor touches the ground.
    pub fn reach(&self) -> f32 {
        self.half_width() + self.shoulder()
    }
}

/// An analytic shape pressed into the ground at `center`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StampLayer {
    /// World position. Its Y is the level Max, Min and Replace stand the
    /// shape on; Add ignores it.
    pub center: Vec3,
    /// Rotation about +Y, radians, as `Quat::from_rotation_y(yaw)`. Only a
    /// ridge shows it.
    pub yaw: f32,
    pub shape: StampShape,
    /// Footprint radius, metres.
    pub radius: f32,
    /// Height (or crater depth) of the shape, metres.
    pub height: f32,
    /// Metres inside the radius over which the stamp fades into the ground.
    /// A ridge also fades across its band over this much, at most the
    /// band's half width.
    pub falloff: f32,
    pub blend: StampBlend,
    /// 0 to 1: how far the stamp takes the ground toward its shape.
    pub strength: f32,
}

/// A flat pad at `center.y`, `size` across, blending back to the ground over
/// `falloff` metres beyond its edge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlattenPadLayer {
    /// World position; its Y is the pad's height.
    pub center: Vec3,
    /// Rotation about +Y, radians, as `Quat::from_rotation_y(yaw)`.
    pub yaw: f32,
    /// Flat top's extent along its local X and Z, metres.
    pub size: Vec2,
    /// Metres beyond the flat top over which the pad blends back into the
    /// ground.
    pub falloff: f32,
    /// Material slot painted over the pad, `None` to leave it.
    pub material: Option<u8>,
}

/// Deterministic fractal noise added over a rectangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoiseLayer {
    /// World position of the footprint's centre. The pattern moves and turns
    /// with the layer.
    pub center: Vec3,
    /// Rotation about +Y, radians, as `Quat::from_rotation_y(yaw)`.
    pub yaw: f32,
    /// Footprint extent along its local X and Z, metres.
    pub size: Vec2,
    /// Largest height the noise adds or removes, metres.
    pub amplitude: f32,
    /// Cycles per metre of the lowest octave.
    pub frequency: f32,
    pub octaves: u32,
    pub seed: u32,
    /// Metres inside the footprint's edge over which the noise fades in.
    pub falloff: f32,
}

/// Paints `material` over a rectangle wherever the finished ground's slope
/// and height fall in range.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialFillLayer {
    /// World position of the footprint's centre.
    pub center: Vec3,
    /// Rotation about +Y, radians, as `Quat::from_rotation_y(yaw)`.
    pub yaw: f32,
    /// Footprint extent along its local X and Z, metres.
    pub size: Vec2,
    pub material: u8,
    /// Slope range painted, degrees from horizontal, both ends included.
    pub min_slope: f32,
    pub max_slope: f32,
    /// World height range painted, metres, both ends included.
    pub min_height: f32,
    pub max_height: f32,
}

impl Default for MaterialFillLayer {
    fn default() -> Self {
        Self {
            center: Vec3::ZERO,
            yaw: 0.0,
            size: Vec2::ZERO,
            material: TerrainMaterial::Grass.to_u8(),
            min_slope: 0.0,
            max_slope: 90.0,
            min_height: f32::MIN,
            max_height: f32::MAX,
        }
    }
}

/// The five layer kinds and their parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerKind {
    Spline(SplineLayer),
    Stamp(StampLayer),
    FlattenPad(FlattenPadLayer),
    Noise(NoiseLayer),
    MaterialFill(MaterialFillLayer),
}

/// One enabled layer, as plain data in world units: what the layer instances
/// of a terrain map onto. Disabled layers are left out of the list entirely.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerDesc {
    /// Stable identity (the instance's), which breaks ties in `order` and
    /// matches a layer across two lists in [`layer_changes`].
    pub id: u64,
    /// Lower orders apply first.
    pub order: i32,
    pub kind: LayerKind,
}

impl LayerDesc {
    /// World XZ rectangle `(min, max)` outside which the layer changes
    /// nothing, or `None` when it can change nothing anywhere (a spline with
    /// fewer than two points, a zero size, a non-finite parameter).
    pub fn bounds(&self) -> Option<(Vec2, Vec2)> {
        let (lo, hi) = match &self.kind {
            LayerKind::Spline(spline) => {
                if spline.points.len() < 2 || spline.points.iter().any(|p| !p.is_finite()) {
                    return None;
                }
                let reach = spline.reach();
                if !(reach > 0.0) {
                    return None;
                }
                let (lo, hi) = point_bounds(dense_path_xz(&spline.points).into_iter())?;
                (lo - Vec2::splat(reach), hi + Vec2::splat(reach))
            }
            LayerKind::Stamp(stamp) => {
                if !(stamp.radius > 0.0) {
                    return None;
                }
                match stamp.shape {
                    StampShape::Ridge => rotated_rect_bounds(
                        stamp.center,
                        stamp.yaw,
                        Vec2::new(stamp.radius, stamp.radius * RIDGE_WIDTH_FRACTION),
                    ),
                    _ => {
                        let centre = Vec2::new(stamp.center.x, stamp.center.z);
                        (centre - Vec2::splat(stamp.radius), centre + Vec2::splat(stamp.radius))
                    }
                }
            }
            LayerKind::FlattenPad(pad) => {
                if !(pad.size.x > 0.0 && pad.size.y > 0.0) {
                    return None;
                }
                rotated_rect_bounds(pad.center, pad.yaw, pad.size * 0.5 + Vec2::splat(pad.falloff.max(0.0)))
            }
            LayerKind::Noise(noise) => {
                if !(noise.size.x > 0.0 && noise.size.y > 0.0) {
                    return None;
                }
                rotated_rect_bounds(noise.center, noise.yaw, noise.size * 0.5)
            }
            LayerKind::MaterialFill(fill) => {
                if !(fill.size.x > 0.0 && fill.size.y > 0.0) {
                    return None;
                }
                rotated_rect_bounds(fill.center, fill.yaw, fill.size * 0.5)
            }
        };
        (lo.is_finite() && hi.is_finite() && lo.x <= hi.x && lo.y <= hi.y).then_some((lo, hi))
    }

    /// Whether the layer paints any material.
    pub fn paints_materials(&self) -> bool {
        match &self.kind {
            LayerKind::Spline(spline) => paints(spline.bed_material) || paints(spline.shoulder_material),
            LayerKind::FlattenPad(pad) => paints(pad.material),
            LayerKind::MaterialFill(fill) => fill.material != MATERIAL_SLOT_NONE,
            LayerKind::Stamp(_) | LayerKind::Noise(_) => false,
        }
    }
}

fn paints(slot: Option<u8>) -> bool {
    slot.is_some_and(|slot| slot != MATERIAL_SLOT_NONE)
}

/// Bounding box of `points`, `None` when there are none.
fn point_bounds(points: impl Iterator<Item = Vec2>) -> Option<(Vec2, Vec2)> {
    points.fold(None, |acc, p| match acc {
        None => Some((p, p)),
        Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
    })
}

/// World XZ bounding box of a rectangle of half extents `half` (local X, Z)
/// centred on `center` and turned by `yaw` about +Y.
pub(super) fn rotated_rect_bounds(center: Vec3, yaw: f32, half: Vec2) -> (Vec2, Vec2) {
    let (sin, cos) = yaw.sin_cos();
    let extent = Vec2::new(cos.abs() * half.x + sin.abs() * half.y, sin.abs() * half.x + cos.abs() * half.y);
    let centre = Vec2::new(center.x, center.z);
    (centre - extent, centre + extent)
}

pub(super) fn rects_overlap(a: (Vec2, Vec2), b: (Vec2, Vec2)) -> bool {
    a.0.x <= b.1.x && b.0.x <= a.1.x && a.0.y <= b.1.y && b.0.y <= a.1.y
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A result that went non-finite (a NaN parameter) leaves the ground alone.
#[inline]
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

/// World rectangles whose layer bake the difference between layer lists
/// `old` and `new` changes: the old and the new bounds of every layer whose
/// description differs, the old bounds of every layer `new` lacks, and the
/// new bounds of every layer `old` lacks, matched by id. A layer that only
/// moved in `order` changes nothing outside its own bounds, so its bounds
/// cover a reorder too.
pub fn layer_changes(old: &[LayerDesc], new: &[LayerDesc]) -> Vec<(Vec2, Vec2)> {
    let old_by_id: HashMap<u64, &LayerDesc> = old.iter().map(|layer| (layer.id, layer)).collect();
    let new_by_id: HashMap<u64, &LayerDesc> = new.iter().map(|layer| (layer.id, layer)).collect();
    let mut rects = Vec::new();
    for layer in old {
        match new_by_id.get(&layer.id) {
            Some(now) if *now == layer => {}
            _ => rects.extend(layer.bounds()),
        }
    }
    for layer in new {
        match old_by_id.get(&layer.id) {
            Some(before) if *before == layer => {}
            _ => rects.extend(layer.bounds()),
        }
    }
    rects
}

/// [`layer_changes`], with each changed layer reported by the rectangles its
/// preparation in `old_prepared` or `new_prepared` can change (see
/// `PreparedLayer::change_rects`), and by its bounds when it has none.
fn prepared_layer_changes(
    old: &[LayerDesc],
    old_prepared: Option<&PreparedLayers>,
    new: &[LayerDesc],
    new_prepared: &PreparedLayers,
) -> Vec<(Vec2, Vec2)> {
    let rects_of = |desc: &LayerDesc, prepared: Option<&PreparedLayers>| -> Vec<(Vec2, Vec2)> {
        match prepared.and_then(|p| p.layers.iter().find(|l| l.desc.id == desc.id && l.desc == *desc)) {
            Some(layer) => layer.change_rects(),
            None => desc.bounds().into_iter().collect(),
        }
    };
    let old_by_id: HashMap<u64, &LayerDesc> = old.iter().map(|layer| (layer.id, layer)).collect();
    let new_by_id: HashMap<u64, &LayerDesc> = new.iter().map(|layer| (layer.id, layer)).collect();
    let mut rects = Vec::new();
    for layer in old {
        match new_by_id.get(&layer.id) {
            Some(now) if *now == layer => {}
            _ => rects.extend(rects_of(layer, old_prepared)),
        }
    }
    for layer in new {
        match old_by_id.get(&layer.id) {
            Some(before) if *before == layer => {}
            _ => rects.extend(rects_of(layer, Some(new_prepared))),
        }
    }
    rects
}

/// `layers` in the order they apply: by `order`, then by `id`.
fn sorted_layers(mut layers: Vec<LayerDesc>) -> Vec<LayerDesc> {
    layers.sort_by_key(|layer| (layer.order, layer.id));
    layers
}

// ============================================================================
// Per-point evaluation
// ============================================================================

/// Height and paint `spline` leaves `lateral` metres from its centreline,
/// where its profile stands at `profile_y` over ground `h`.
fn spline_cross_section(spline: &SplineLayer, lateral: f32, profile_y: f32, h: f32) -> (f32, Option<(u8, f32)>) {
    let half = spline.half_width();
    let shoulder = spline.shoulder();
    // Also refuses a NaN lateral.
    if !(lateral <= half + shoulder) {
        return (h, None);
    }
    let in_bed = lateral <= half;
    // 0 across the bed, rising to 1 at the shoulder's outer edge.
    let t = if in_bed { 0.0 } else { ((lateral - half) / shoulder.max(1e-3)).min(1.0) };
    let blend = 1.0 - smoothstep(t);
    let depth = spline.depth.abs();
    let shaped = match spline.mode {
        SplineMode::Road => lerp(h, profile_y, blend),
        SplineMode::Path => lerp(h, profile_y, PATH_PULL * spline.smoothing.clamp(0.0, 1.0) * blend),
        SplineMode::River => h.min(lerp(h, profile_y - depth, blend)),
        SplineMode::Canyon => {
            // The wall drops within the inner part of the shoulder; the rest
            // of the shoulder is rim.
            let wall = 1.0 - smoothstep(t / CANYON_WALL_FRACTION);
            h.min(lerp(h, profile_y - depth, wall))
        }
        SplineMode::Embankment => h.max(lerp(h, profile_y + depth, blend)),
    };
    let paint = if in_bed {
        spline.bed_material.map(|slot| (slot, 1.0))
    } else {
        spline.shoulder_material.map(|slot| (slot, blend))
    };
    (finite_or(shaped, h), paint.filter(|(slot, strength)| *slot != MATERIAL_SLOT_NONE && *strength > 0.0))
}

/// A crater's height at `rn` (distance over radius) as a fraction of the
/// stamp's height: the bowl below zero, the rim above.
fn crater_profile(rn: f32) -> f32 {
    let bowl = (1.0 - (rn / CRATER_BOWL).powi(2)).max(0.0);
    let rim = CRATER_RIM_HEIGHT * (-((rn - CRATER_RIM_AT) / CRATER_RIM_WIDTH).powi(2)).exp();
    rim - bowl
}

/// Height `stamp` leaves at `local` (its own frame) over ground `h`.
fn stamp_sample(stamp: &StampLayer, local: Vec2, h: f32) -> f32 {
    let radius = stamp.radius;
    if !(radius > 0.0) {
        return h;
    }
    let falloff = stamp.falloff.max(0.0).min(radius);
    let core = radius - falloff;
    // 1 inside the core, fading to 0 at the radius.
    let edge = |r: f32| -> f32 {
        if !(r < radius) {
            0.0
        } else if r <= core {
            1.0
        } else {
            1.0 - smoothstep((r - core) / falloff)
        }
    };
    let (mask, profile) = match stamp.shape {
        StampShape::Ridge => {
            let half_w = radius * RIDGE_WIDTH_FRACTION;
            let across = local.y.abs() / half_w;
            // Without a lateral fade, Max/Min/Replace pull the whole band (and,
            // when yawed, the corners of its axis-aligned bounds) to center.y
            // at full weight wherever the profile has dropped to 0. That leaves
            // cliffs at the band edges. Fading the weight across the band keeps
            // every blend inside the crest.
            let fw = falloff.min(half_w);
            let y = local.y.abs();
            let lateral = if !(y < half_w) {
                0.0
            } else if y <= half_w - fw {
                1.0
            } else {
                1.0 - smoothstep((y - (half_w - fw)) / fw)
            };
            (edge(local.x.abs()) * lateral, (1.0 - across * across).max(0.0))
        }
        StampShape::Mound => {
            let r = local.length();
            let rn = r / radius;
            (edge(r), (1.0 - rn * rn).max(0.0))
        }
        StampShape::Plateau => (edge(local.length()), 1.0),
        StampShape::Crater => {
            let r = local.length();
            (edge(r), crater_profile(r / radius))
        }
    };
    let weight = stamp.strength.clamp(0.0, 1.0) * mask;
    if !(weight > 0.0) {
        return h;
    }
    let displacement = stamp.height * profile;
    let target = stamp.center.y + displacement;
    let shaped = match stamp.blend {
        StampBlend::Add => h + weight * displacement,
        StampBlend::Max => lerp(h, h.max(target), weight),
        StampBlend::Min => lerp(h, h.min(target), weight),
        StampBlend::Replace => lerp(h, target, weight),
    };
    finite_or(shaped, h)
}

/// Height and paint `pad` leaves at `local` (its own frame) over ground `h`.
fn pad_sample(pad: &FlattenPadLayer, local: Vec2, h: f32) -> (f32, Option<(u8, f32)>) {
    let half = (pad.size * 0.5).max(Vec2::ZERO);
    let outside = (local.abs() - half).max(Vec2::ZERO).length();
    let falloff = pad.falloff.max(0.0);
    let weight = if outside <= 0.0 {
        1.0
    } else if !(outside < falloff) {
        0.0
    } else {
        1.0 - smoothstep(outside / falloff)
    };
    if !(weight > 0.0) {
        return (h, None);
    }
    let paint = pad.material.filter(|slot| *slot != MATERIAL_SLOT_NONE).map(|slot| (slot, weight));
    (finite_or(lerp(h, pad.center.y, weight), h), paint)
}

/// Height `noise` leaves at `local` (its own frame) over ground `h`.
fn noise_sample(noise: &NoiseLayer, local: Vec2, h: f32) -> f32 {
    let inside = noise.size * 0.5 - local.abs();
    let edge = inside.x.min(inside.y);
    if !(edge >= 0.0) {
        return h;
    }
    // Zero on the footprint's edge, so the noise meets the ground there.
    let mask = if noise.falloff > 0.0 { smoothstep(edge / noise.falloff) } else { 1.0 };
    if !(mask > 0.0) {
        return h;
    }
    let value = fbm(
        u64::from(noise.seed),
        f64::from(local.x * noise.frequency),
        f64::from(local.y * noise.frequency),
        noise.octaves.clamp(1, MAX_NOISE_OCTAVES),
        2.0,
        0.5,
    ) as f32;
    finite_or(h + noise.amplitude * mask * value, h)
}

/// Whether `fill` paints a point at `local` (its own frame) where the
/// finished ground slopes `slope_degrees` at world height `height`.
fn fill_accepts(fill: &MaterialFillLayer, local: Vec2, slope_degrees: f32, height: f32) -> bool {
    let half = fill.size * 0.5;
    if !(local.x.abs() <= half.x && local.y.abs() <= half.y) {
        return false;
    }
    let (slope_lo, slope_hi) = ordered(fill.min_slope, fill.max_slope);
    let (height_lo, height_hi) = ordered(fill.min_height, fill.max_height);
    slope_degrees >= slope_lo && slope_degrees <= slope_hi && height >= height_lo && height <= height_hi
}

/// A range typed backwards still means the range between its ends.
fn ordered(a: f32, b: f32) -> (f32, f32) {
    if a <= b { (a, b) } else { (b, a) }
}

// ============================================================================
// Prepared layers
// ============================================================================

/// A layer's own frame: world XZ offsets from its centre turned by `-yaw`.
#[derive(Clone, Copy, Debug)]
struct Frame {
    center: Vec2,
    cos: f32,
    sin: f32,
}

impl Frame {
    const IDENTITY: Frame = Frame { center: Vec2::ZERO, cos: 1.0, sin: 0.0 };

    fn new(center: Vec3, yaw: f32) -> Self {
        let (sin, cos) = yaw.sin_cos();
        Self { center: Vec2::new(center.x, center.z), cos, sin }
    }

    /// `p` in the frame: `Quat::from_rotation_y(yaw)` takes local +X to
    /// world `(cos, -sin)`, so this turns the offset back the other way.
    fn local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        Vec2::new(d.x * self.cos - d.y * self.sin, d.x * self.sin + d.y * self.cos)
    }
}

/// The segments of a spline's station polyline bucketed on a coarse grid, so
/// a cell's closest point on the curve looks at a handful of segments rather
/// than every station.
#[derive(Clone, Debug)]
struct SegmentGrid {
    origin: Vec2,
    cell: f32,
    columns: usize,
    rows: usize,
    /// Grid cell `i`'s segments are `segments[starts[i]..starts[i + 1]]`.
    starts: Vec<u32>,
    segments: Vec<u32>,
}

impl SegmentGrid {
    /// Bucket the segments between consecutive `stations`, each into every
    /// grid cell within `reach` of it, so the segment closest to any point
    /// within `reach` of the curve is in that point's cell.
    fn new(stations: &[Vec3], reach: f32) -> Self {
        let xz = |p: Vec3| Vec2::new(p.x, p.z);
        let (lo, hi) = point_bounds(stations.iter().map(|&p| xz(p))).unwrap_or((Vec2::ZERO, Vec2::ZERO));
        let origin = lo - Vec2::splat(reach);
        let span = (hi + Vec2::splat(reach) - origin).max(Vec2::splat(1e-3));
        // At most MAX_GRID_SIDE - 1 whole cells per side, plus the one the
        // far edge falls in, so every point of the span lands on the grid.
        let cell = reach
            .max(MIN_GRID_CELL)
            .max(span.x / (MAX_GRID_SIDE - 1) as f32)
            .max(span.y / (MAX_GRID_SIDE - 1) as f32);
        let columns = ((span.x / cell).floor() as usize + 1).clamp(1, MAX_GRID_SIDE);
        let rows = ((span.y / cell).floor() as usize + 1).clamp(1, MAX_GRID_SIDE);
        let mut grid = Self { origin, cell, columns, rows, starts: Vec::new(), segments: Vec::new() };

        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); columns * rows];
        for (i, pair) in stations.windows(2).enumerate() {
            let (a, b) = (xz(pair[0]), xz(pair[1]));
            let (c0, r0) = grid.cell_of(a.min(b) - Vec2::splat(reach));
            let (c1, r1) = grid.cell_of(a.max(b) + Vec2::splat(reach));
            for row in r0..=r1 {
                for column in c0..=c1 {
                    buckets[row * columns + column].push(i as u32);
                }
            }
        }
        grid.starts.reserve(buckets.len() + 1);
        grid.starts.push(0);
        for bucket in buckets {
            grid.segments.extend(bucket);
            grid.starts.push(grid.segments.len() as u32);
        }
        grid
    }

    /// The grid cell holding `p`, clamped onto the grid.
    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let f = (p - self.origin) / self.cell;
        let column = (f.x.floor().max(0.0) as usize).min(self.columns - 1);
        let row = (f.y.floor().max(0.0) as usize).min(self.rows - 1);
        (column, row)
    }

    /// Segments that may be the closest to `p`; none when `p` is off the
    /// grid, which is farther than `reach` from the curve.
    fn candidates(&self, p: Vec2) -> &[u32] {
        let f = (p - self.origin) / self.cell;
        if !(f.x >= 0.0 && f.y >= 0.0 && f.x < self.columns as f32 && f.y < self.rows as f32) {
            return &[];
        }
        let i = f.y as usize * self.columns + f.x as usize;
        &self.segments[self.starts[i] as usize..self.starts[i + 1] as usize]
    }

    /// World rectangles of the grid cells holding any segment, merged into
    /// runs along each row: every point within `reach` of the curve lies in
    /// one, so a change of the curve changes nothing outside them.
    fn occupied_rects(&self) -> Vec<(Vec2, Vec2)> {
        let mut rects = Vec::new();
        let filled = |row: usize, column: usize| {
            let i = row * self.columns + column;
            self.starts[i + 1] > self.starts[i]
        };
        for row in 0..self.rows {
            let mut column = 0;
            while column < self.columns {
                if !filled(row, column) {
                    column += 1;
                    continue;
                }
                let first = column;
                while column < self.columns && filled(row, column) {
                    column += 1;
                }
                let lo = self.origin + Vec2::new(first as f32, row as f32) * self.cell;
                let hi = self.origin + Vec2::new(column as f32, (row + 1) as f32) * self.cell;
                rects.push((lo, hi));
            }
        }
        rects
    }
}

/// A spline evaluated once: its stations, their segment grid, and the extra
/// elevation knots its profile sampled from the ground.
#[derive(Clone, Debug)]
struct PreparedSpline {
    /// World XZ of each station, with the smoothed profile as Y.
    stations: Vec<Vec3>,
    grid: SegmentGrid,
    bounds: (Vec2, Vec2),
    /// Where each extra knot sampled the ground, and what it read there.
    knots: Vec<(Vec2, f32)>,
}

impl PreparedSpline {
    fn new(config: &TerrainConfig, base: &TerrainData, spline: &SplineLayer, prefix: &[PreparedLayer]) -> Option<Self> {
        if spline.points.len() < 2 || spline.points.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let reach = spline.reach();
        if !(reach > 0.0) {
            return None;
        }
        // Stations about a raster cell apart: denser buys nothing a cell can
        // show, sparser cuts corners on tight bends.
        let cell = config.chunk_size / config.chunk_resolution.max(1) as f32;
        let station_spacing = if cell.is_finite() { cell.clamp(MIN_STATION_SPACING, MAX_STATION_SPACING) } else { MAX_STATION_SPACING };
        let knot_spacing = MIN_KNOT_SPACING + KNOT_SPACING_RANGE * spline.smoothing.clamp(0.0, 1.0);
        let mut knots = Vec::new();
        let path = build_road_path_with(&spline.points, station_spacing, knot_spacing, |x, z| {
            let p = Vec2::new(x, z);
            let h = ground_through(prefix, config, base, p);
            knots.push((p, h));
            h
        })?;
        let stations: Vec<Vec3> = path.stations.iter().map(|station| station.pos).collect();
        let (lo, hi) = point_bounds(stations.iter().map(|p| Vec2::new(p.x, p.z)))?;
        let bounds = (lo - Vec2::splat(reach), hi + Vec2::splat(reach));
        let grid = SegmentGrid::new(&stations, reach);
        Some(Self { stations, grid, bounds, knots })
    }

    /// Distance from `p` to the closest point of the station polyline, and
    /// the profile there, when that point is within the grid's reach. The
    /// true closest point (not the nearest station) keeps the inside of a
    /// hairpin from ridging.
    fn closest(&self, p: Vec2) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        for &segment in self.grid.candidates(p) {
            let (a, b) = (self.stations[segment as usize], self.stations[segment as usize + 1]);
            let (a2, b2) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
            let ab = b2 - a2;
            let length_sq = ab.length_squared();
            let t = if length_sq > 1e-12 { ((p - a2).dot(ab) / length_sq).clamp(0.0, 1.0) } else { 0.0 };
            let distance_sq = p.distance_squared(a2 + ab * t);
            if best.is_none_or(|(closest, _)| distance_sq < closest) {
                best = Some((distance_sq, lerp(a.y, b.y, t)));
            }
        }
        best.map(|(distance_sq, y)| (distance_sq.sqrt(), y))
    }

    /// The extra knots still read the ground they did, re-sampled through
    /// `prefix` over `base`.
    fn knots_hold(&self, config: &TerrainConfig, base: &TerrainData, prefix: &[PreparedLayer]) -> bool {
        self.knots
            .iter()
            .all(|&(p, h)| (ground_through(prefix, config, base, p) - h).abs() <= PROFILE_EPSILON)
    }

    /// Both lay the same stations along the same profile.
    fn same_profile(&self, other: &Self) -> bool {
        self.stations.len() == other.stations.len()
            && self
                .stations
                .iter()
                .zip(&other.stations)
                .all(|(a, b)| (*a - *b).abs().max_element() <= PROFILE_EPSILON)
    }
}

/// One layer ready to evaluate cell by cell.
#[derive(Clone, Debug)]
struct PreparedLayer {
    desc: LayerDesc,
    /// Where the layer can change anything; a spline's is the tighter box of
    /// its stations.
    bounds: (Vec2, Vec2),
    frame: Frame,
    spline: Option<PreparedSpline>,
}

impl PreparedLayer {
    /// `desc` prepared over the ground `prefix` (the layers before it) leaves
    /// on `base`. `None` when the layer can change nothing.
    fn new(config: &TerrainConfig, base: &TerrainData, desc: &LayerDesc, prefix: &[PreparedLayer]) -> Option<Self> {
        let bounds = desc.bounds()?;
        let (frame, spline) = match &desc.kind {
            LayerKind::Spline(spline) => (Frame::IDENTITY, Some(PreparedSpline::new(config, base, spline, prefix)?)),
            LayerKind::Stamp(stamp) => (Frame::new(stamp.center, stamp.yaw), None),
            LayerKind::FlattenPad(pad) => (Frame::new(pad.center, pad.yaw), None),
            LayerKind::Noise(noise) => (Frame::new(noise.center, noise.yaw), None),
            LayerKind::MaterialFill(fill) => (Frame::new(fill.center, fill.yaw), None),
        };
        let bounds = spline.as_ref().map_or(bounds, |spline| spline.bounds);
        Some(Self { desc: desc.clone(), bounds, frame, spline })
    }

    fn contains(&self, p: Vec2) -> bool {
        p.x >= self.bounds.0.x && p.x <= self.bounds.1.x && p.y >= self.bounds.0.y && p.y <= self.bounds.1.y
    }

    /// Where a change of this layer can change the bake: a spline's corridor
    /// as the coarse cells it occupies, clipped to its box, since the box of
    /// a long or diagonal corridor covers far more ground than the corridor;
    /// any other layer's bounds.
    fn change_rects(&self) -> Vec<(Vec2, Vec2)> {
        match &self.spline {
            Some(spline) => spline
                .grid
                .occupied_rects()
                .into_iter()
                .map(|(lo, hi)| (lo.max(self.bounds.0), hi.min(self.bounds.1)))
                .filter(|(lo, hi)| lo.x <= hi.x && lo.y <= hi.y)
                .collect(),
            None => vec![self.bounds],
        }
    }

    /// Height the layer leaves at world `p` over ground `h`, and the material
    /// it paints there by geometry. A material fill paints by the finished
    /// ground's slope instead, in the material pass, and leaves heights alone.
    fn sample(&self, p: Vec2, h: f32) -> (f32, Option<(u8, f32)>) {
        match &self.desc.kind {
            LayerKind::Spline(spline) => match self.spline.as_ref().and_then(|prepared| prepared.closest(p)) {
                Some((lateral, profile_y)) => spline_cross_section(spline, lateral, profile_y, h),
                None => (h, None),
            },
            LayerKind::Stamp(stamp) => (stamp_sample(stamp, self.frame.local(p), h), None),
            LayerKind::FlattenPad(pad) => pad_sample(pad, self.frame.local(p), h),
            LayerKind::Noise(noise) => (noise_sample(noise, self.frame.local(p), h), None),
            LayerKind::MaterialFill(_) => (h, None),
        }
    }
}

/// World height of the ground at `p` as `prefix` leaves it on `base`.
fn ground_through(prefix: &[PreparedLayer], config: &TerrainConfig, base: &TerrainData, p: Vec2) -> f32 {
    let mut h = height_at_world(config, base, p.x, p.y);
    for layer in prefix {
        if layer.contains(p) {
            h = layer.sample(p, h).0;
        }
    }
    h
}

/// Where the cells of a raster lie in the world: cell `(x, z)` at
/// `origin + (x, z) * step`, the inverse of `height_query::world_to_uv`
/// times `size - 1`, so cell centres are exactly the points
/// `TerrainData::sample_height` reads without blending.
#[derive(Clone, Copy, Debug)]
struct RasterGrid {
    origin: Vec2,
    step: Vec2,
    width: usize,
    height: usize,
}

/// An inclusive rectangle of raster cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellRect {
    x0: usize,
    z0: usize,
    x1: usize,
    z1: usize,
}

impl CellRect {
    fn contains(&self, x: usize, z: usize) -> bool {
        x >= self.x0 && x <= self.x1 && z >= self.z0 && z <= self.z1
    }
}

impl RasterGrid {
    /// The grid of `data` laid over `config`'s chunk grid; `None` without a
    /// whole height raster.
    fn of(config: &TerrainConfig, data: &TerrainData) -> Option<Self> {
        let (width, height) = (data.cache_width as usize, data.cache_height as usize);
        if width == 0 || height == 0 || data.height_cache.len() != width * height {
            return None;
        }
        let (min, max) = config.footprint_xz();
        let span = max - min;
        let step = Vec2::new(
            if width > 1 { span.x / (width - 1) as f32 } else { 0.0 },
            if height > 1 { span.y / (height - 1) as f32 } else { 0.0 },
        );
        Some(Self { origin: min, step, width, height })
    }

    fn world(&self, x: usize, z: usize) -> Vec2 {
        self.origin + Vec2::new(x as f32, z as f32) * self.step
    }

    fn all(&self) -> CellRect {
        CellRect { x0: 0, z0: 0, x1: self.width - 1, z1: self.height - 1 }
    }

    /// Every cell a write anywhere in world rectangle `lo..hi` could land
    /// in: writers round a position to its nearest cell, so the corners are
    /// rounded outward. `None` when the rectangle misses the raster or is
    /// not finite.
    fn cells_in(&self, lo: Vec2, hi: Vec2) -> Option<CellRect> {
        if !(lo.is_finite() && hi.is_finite()) {
            return None;
        }
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        let axis = |a: f32, b: f32, origin: f32, step: f32, count: usize| -> Option<(usize, usize)> {
            let last = (count - 1) as f32;
            let (fa, fb) = if step > 0.0 { ((a - origin) / step, (b - origin) / step) } else { (0.0, 0.0) };
            let (first, end) = (fa.floor().max(0.0), fb.ceil().min(last));
            (first <= end).then_some((first as usize, end as usize))
        };
        let (x0, x1) = axis(lo.x, hi.x, self.origin.x, self.step.x, self.width)?;
        let (z0, z1) = axis(lo.y, hi.y, self.origin.y, self.step.y, self.height)?;
        Some(CellRect { x0, z0, x1, z1 })
    }

    /// `rect` grown by `margin` cells, clipped to the raster.
    fn expand(&self, rect: CellRect, margin: usize) -> CellRect {
        CellRect {
            x0: rect.x0.saturating_sub(margin),
            z0: rect.z0.saturating_sub(margin),
            x1: (rect.x1 + margin).min(self.width - 1),
            z1: (rect.z1 + margin).min(self.height - 1),
        }
    }
}

/// Buffers one bake reuses from block to block.
#[derive(Default)]
struct BakeScratch {
    /// Indices of the layers overlapping the block, in apply order.
    active: Vec<usize>,
    /// World heights over the block and its margin.
    heights: Vec<f32>,
    /// Whether any layer changed each height.
    moved: Vec<bool>,
    /// Per block cell, per active layer: the material it paints by geometry.
    paints: Vec<(u8, f32)>,
}

/// A layer list prepared against one base: sorted, degenerate layers
/// dropped, spline paths and profiles computed. Bakes any region of that
/// base on demand.
#[derive(Clone, Debug, Default)]
pub struct PreparedLayers {
    layers: Vec<PreparedLayer>,
}

impl PreparedLayers {
    /// Prepare `layers` over `base`. Each spline samples its extra knots from
    /// the ground the layers before it leave.
    pub fn new(config: &TerrainConfig, base: &TerrainData, layers: &[LayerDesc]) -> Self {
        let mut sorted: Vec<&LayerDesc> = layers.iter().collect();
        sorted.sort_by_key(|layer| (layer.order, layer.id));
        let mut prepared: Vec<PreparedLayer> = Vec::with_capacity(sorted.len());
        for desc in sorted {
            if let Some(layer) = PreparedLayer::new(config, base, desc, &prepared) {
                prepared.push(layer);
            }
        }
        Self { layers: prepared }
    }

    /// No layer can change anything.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Bounds of every prepared layer, in apply order.
    pub fn bounds(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        self.layers.iter().map(|layer| layer.bounds)
    }

    /// Whether world `p` lies on the corridor of a spline in one of `modes`,
    /// its bed or shoulders: within the spline's reach of the stations it was
    /// laid along. Scatter keeps off roads and paths through this, so it
    /// follows the very line the bake carved rather than a second evaluation
    /// of the spline.
    pub fn in_corridor(&self, p: Vec2, modes: &[SplineMode]) -> bool {
        self.layers.iter().any(|layer| match (&layer.desc.kind, &layer.spline) {
            (LayerKind::Spline(spline), Some(prepared)) if modes.contains(&spline.mode) && layer.contains(p) => {
                prepared.closest(p).is_some_and(|(lateral, _)| lateral <= spline.reach())
            }
            _ => false,
        })
    }

    fn paints_materials(&self) -> bool {
        self.layers.iter().any(|layer| layer.desc.paints_materials())
    }

    /// Re-sample every spline's extra knots against `base` (through the
    /// layers before it, already refreshed) and rebuild each spline whose
    /// knots moved. Returns the old and new corridor of every rebuilt
    /// spline: the whole corridor, since a moved knot bends the whole
    /// profile.
    fn refresh(&mut self, config: &TerrainConfig, base: &TerrainData) -> Vec<(Vec2, Vec2)> {
        let mut moved = Vec::new();
        for i in 0..self.layers.len() {
            let (prefix, rest) = self.layers.split_at_mut(i);
            let layer = &mut rest[0];
            let holds = layer.spline.as_ref().is_none_or(|spline| spline.knots_hold(config, base, prefix));
            if holds {
                continue;
            }
            if let Some(rebuilt) = PreparedLayer::new(config, base, &layer.desc, prefix) {
                moved.extend(layer.change_rects());
                moved.extend(rebuilt.change_rects());
                *layer = rebuilt;
            }
        }
        moved
    }

    /// Corridors of the splines whose description is the same in `previous`
    /// but whose profile is not: a change to a layer before one of them
    /// moved the ground its knots read, bending its whole corridor.
    fn moved_splines(&self, previous: &PreparedLayers) -> Vec<(Vec2, Vec2)> {
        let mut moved = Vec::new();
        for layer in &self.layers {
            let Some(spline) = &layer.spline else { continue };
            let Some(before) = previous.layers.iter().find(|old| old.desc.id == layer.desc.id) else { continue };
            // A changed description is `layer_changes`' to report.
            if before.desc != layer.desc {
                continue;
            }
            if !before.spline.as_ref().is_some_and(|old| old.same_profile(spline)) {
                moved.extend(before.change_rects());
                moved.extend(layer.change_rects());
            }
        }
        moved
    }

    /// Bake the cells of world rectangle `rect` from `base` into `out`, which
    /// holds an earlier bake of `base` (anything else is replaced first, see
    /// below). Every cell whose bake a change of the base or of a layer
    /// inside `rect` can reach is recomputed from the base: the cells a write
    /// in `rect` could land in, and with a material fill present one ring of
    /// cells more, whose slope reads them. Cells beyond are left as they are.
    ///
    /// Returns `true` when `out` did not have the layout this bake needs (a
    /// raster of another size, or a material layer where none is wanted or
    /// the other way round) and was rebuilt from the base and baked whole:
    /// every chunk then reads different data.
    pub fn bake_rect(&self, config: &TerrainConfig, base: &TerrainData, rect: (Vec2, Vec2), out: &mut TerrainData) -> bool {
        let Some(grid) = RasterGrid::of(config, base) else {
            return self.take_rasterless_base(base, out);
        };
        if self.prepare_layout(base, out) {
            self.bake_cells(config, base, &grid, grid.all(), out);
            return true;
        }
        if let Some(cells) = grid.cells_in(rect.0, rect.1) {
            self.bake_cells(config, base, &grid, grid.expand(cells, self.slope_margin()), out);
        }
        false
    }

    /// Cells a bake reads around each cell it computes: one when a material
    /// fill needs the slope, which reads the finished height of the cells
    /// beside it, none otherwise.
    fn slope_margin(&self) -> usize {
        usize::from(self.layers.iter().any(|layer| matches!(layer.desc.kind, LayerKind::MaterialFill(_))))
    }

    /// Bake every cell of `base` into `out`. Returns `true` when `out`'s
    /// layout had to be rebuilt first, as [`Self::bake_rect`] does.
    pub fn bake_all(&self, config: &TerrainConfig, base: &TerrainData, out: &mut TerrainData) -> bool {
        let Some(grid) = RasterGrid::of(config, base) else {
            return self.take_rasterless_base(base, out);
        };
        let rebuilt = self.prepare_layout(base, out);
        self.bake_cells(config, base, &grid, grid.all(), out);
        rebuilt
    }

    /// A base with no height raster (procedural terrain) gives layers nothing
    /// to act on, so its bake is the base. Copied only when `out` differs in
    /// shape, so a bake of such a terrain does not dirty every chunk each time.
    fn take_rasterless_base(&self, base: &TerrainData, out: &mut TerrainData) -> bool {
        let same = out.cache_width == base.cache_width
            && out.cache_height == base.cache_height
            && out.height_cache.len() == base.height_cache.len()
            && out.material_cache.len() == base.material_cache.len();
        if same {
            return false;
        }
        *out = base.clone();
        true
    }

    /// Give `out` the layout a bake of `base` needs: `base`'s raster size,
    /// and a material layer when `base` has one or a layer paints (all Grass
    /// under the paint where `base` has none, as painting the base itself
    /// would allocate). Rebuilds `out` from `base` and returns `true` when it
    /// had another layout. Keeps its slot palette `base`'s either way.
    fn prepare_layout(&self, base: &TerrainData, out: &mut TerrainData) -> bool {
        let wants_material = base.has_material_layer() || self.paints_materials();
        let fits = out.cache_width == base.cache_width
            && out.cache_height == base.cache_height
            && out.height_cache.len() == base.height_cache.len()
            && out.has_material_layer() == wants_material;
        if fits {
            if out.slot_palette != base.slot_palette {
                out.slot_palette = base.slot_palette.clone();
            }
            return false;
        }
        let mut fresh = base.clone();
        if wants_material {
            ensure_material_cache(&mut fresh);
        }
        fresh.material_dirty = true;
        *out = fresh;
        true
    }

    /// Recompute `cells` of `out` from `base`, block by block. `out` has the
    /// layout [`Self::prepare_layout`] gives it.
    fn bake_cells(&self, config: &TerrainConfig, base: &TerrainData, grid: &RasterGrid, cells: CellRect, out: &mut TerrainData) {
        let margin = self.slope_margin();
        let mut scratch = BakeScratch::default();
        let mut material_changed = false;
        for z0 in (cells.z0..=cells.z1).step_by(BAKE_BLOCK) {
            for x0 in (cells.x0..=cells.x1).step_by(BAKE_BLOCK) {
                let block = CellRect {
                    x0,
                    z0,
                    x1: (x0 + BAKE_BLOCK - 1).min(cells.x1),
                    z1: (z0 + BAKE_BLOCK - 1).min(cells.z1),
                };
                material_changed |= self.bake_block(config, base, grid, block, margin, out, &mut scratch);
            }
        }
        if material_changed {
            out.material_dirty = true;
        }
    }

    /// Recompute one block of cells. Returns whether any material cell of
    /// `out` changed.
    #[allow(clippy::too_many_arguments)]
    fn bake_block(
        &self,
        config: &TerrainConfig,
        base: &TerrainData,
        grid: &RasterGrid,
        block: CellRect,
        margin: usize,
        out: &mut TerrainData,
        scratch: &mut BakeScratch,
    ) -> bool {
        let w = grid.width;
        let base_material = base.has_material_layer();
        let out_material = out.has_material_layer();
        let grass: MaterialCell = material_cell(TerrainMaterial::Grass.to_u8());
        let base_cell = |index: usize| if base_material { base.material_cache[index] } else { grass };
        let mut material_changed = false;

        let expanded = grid.expand(block, margin);
        let area = (grid.world(expanded.x0, expanded.z0), grid.world(expanded.x1, expanded.z1));
        scratch.active.clear();
        scratch.active.extend((0..self.layers.len()).filter(|&i| rects_overlap(self.layers[i].bounds, area)));

        if scratch.active.is_empty() {
            for z in block.z0..=block.z1 {
                let row = z * w;
                out.height_cache[row + block.x0..=row + block.x1]
                    .copy_from_slice(&base.height_cache[row + block.x0..=row + block.x1]);
                if out_material {
                    for index in row + block.x0..=row + block.x1 {
                        let cell = base_cell(index);
                        if out.material_cache[index] != cell {
                            out.material_cache[index] = cell;
                            material_changed = true;
                        }
                    }
                }
            }
            return material_changed;
        }

        // Heights, over the block and its margin.
        let (ew, eh) = (expanded.x1 - expanded.x0 + 1, expanded.z1 - expanded.z0 + 1);
        let bw = block.x1 - block.x0 + 1;
        let bh = block.z1 - block.z0 + 1;
        let count = scratch.active.len();
        scratch.heights.clear();
        scratch.heights.resize(ew * eh, 0.0);
        scratch.moved.clear();
        scratch.moved.resize(ew * eh, false);
        scratch.paints.clear();
        if out_material {
            scratch.paints.resize(bw * bh * count, NO_PAINT);
        }
        for z in expanded.z0..=expanded.z1 {
            for x in expanded.x0..=expanded.x1 {
                let p = grid.world(x, z);
                let mut h = config.world_height(base.height_cache[z * w + x]);
                let mut moved = false;
                let paint_slot = (out_material && block.contains(x, z)).then(|| ((z - block.z0) * bw + (x - block.x0)) * count);
                for (k, &li) in scratch.active.iter().enumerate() {
                    let layer = &self.layers[li];
                    if !layer.contains(p) {
                        continue;
                    }
                    let (next, paint) = layer.sample(p, h);
                    if next != h {
                        h = next;
                        moved = true;
                    }
                    if let (Some(slot), Some(paint)) = (paint_slot, paint) {
                        scratch.paints[slot + k] = paint;
                    }
                }
                let e = (z - expanded.z0) * ew + (x - expanded.x0);
                scratch.heights[e] = h;
                scratch.moved[e] = moved;
            }
        }
        for z in block.z0..=block.z1 {
            for x in block.x0..=block.x1 {
                let e = (z - expanded.z0) * ew + (x - expanded.x0);
                let index = z * w + x;
                // A cell no layer moved keeps the base's sample bit for bit,
                // rather than a round trip through world metres.
                out.height_cache[index] = if scratch.moved[e] {
                    config.normalized_height(scratch.heights[e])
                } else {
                    base.height_cache[index]
                };
            }
        }
        if !out_material {
            return false;
        }

        // Materials, in layer order over the finished heights.
        let fills = scratch
            .active
            .iter()
            .any(|&li| matches!(self.layers[li].desc.kind, LayerKind::MaterialFill(_)));
        for z in block.z0..=block.z1 {
            for x in block.x0..=block.x1 {
                let index = z * w + x;
                let p = grid.world(x, z);
                let e = (z - expanded.z0) * ew + (x - expanded.x0);
                let slope = if fills { slope_degrees(&scratch.heights, expanded, grid, x, z) } else { 0.0 };
                let height = scratch.heights[e];
                let slot = ((z - block.z0) * bw + (x - block.x0)) * count;
                let mut cell = base_cell(index);
                for (k, &li) in scratch.active.iter().enumerate() {
                    let layer = &self.layers[li];
                    match &layer.desc.kind {
                        LayerKind::MaterialFill(fill) => {
                            if fill.material != MATERIAL_SLOT_NONE
                                && layer.contains(p)
                                && fill_accepts(fill, layer.frame.local(p), slope, height)
                            {
                                cell = paint_material_cell(cell, fill.material, 1.0);
                            }
                        }
                        _ => {
                            let (painted, strength) = scratch.paints[slot + k];
                            if painted != MATERIAL_SLOT_NONE {
                                cell = paint_material_cell(cell, painted, strength);
                            }
                        }
                    }
                }
                if out.material_cache[index] != cell {
                    out.material_cache[index] = cell;
                    material_changed = true;
                }
            }
        }
        material_changed
    }
}

/// Slope in degrees at cell `(x, z)` of `heights` (world metres over the
/// cells of `rect`), by central differences, one-sided at the raster's edge.
fn slope_degrees(heights: &[f32], rect: CellRect, grid: &RasterGrid, x: usize, z: usize) -> f32 {
    let width = rect.x1 - rect.x0 + 1;
    let at = |x: usize, z: usize| heights[(z - rect.z0) * width + (x - rect.x0)];
    let (left, right) = (x.saturating_sub(1).max(rect.x0), (x + 1).min(rect.x1));
    let (near, far) = (z.saturating_sub(1).max(rect.z0), (z + 1).min(rect.z1));
    let gx = if right > left && grid.step.x > 0.0 {
        (at(right, z) - at(left, z)) / ((right - left) as f32 * grid.step.x)
    } else {
        0.0
    };
    let gz = if far > near && grid.step.y > 0.0 {
        (at(x, far) - at(x, near)) / ((far - near) as f32 * grid.step.y)
    } else {
        0.0
    };
    (gx * gx + gz * gz).sqrt().atan().to_degrees()
}

/// Bake world rectangle `rect` of `base` with `layers` applied into `out`,
/// which holds an earlier bake of `base` (see [`PreparedLayers::bake_rect`],
/// including what the return value means). Prepares the layers for this one
/// call; [`TerrainBaked`] keeps them prepared between bakes instead.
pub fn bake_region(
    config: &TerrainConfig,
    base: &TerrainData,
    layers: &[LayerDesc],
    rect: (Vec2, Vec2),
    out: &mut TerrainData,
) -> bool {
    PreparedLayers::new(config, base, layers).bake_rect(config, base, rect, out)
}

// ============================================================================
// The baked component
// ============================================================================

/// Regions of the base to bake again: what the raster marks of
/// `TerrainDirtyChunks` queued since the last bake.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainRebake {
    /// Every cell.
    pub whole: bool,
    /// World XZ rectangles `(min, max)`.
    pub rects: Vec<(Vec2, Vec2)>,
}

impl TerrainRebake {
    /// A request to bake every cell.
    pub fn whole() -> Self {
        Self { whole: true, rects: Vec::new() }
    }

    /// Nothing to bake.
    pub fn is_empty(&self) -> bool {
        !self.whole && self.rects.is_empty()
    }
}

/// What [`TerrainBaked::rebake`] changed beyond the regions it was asked to
/// bake, which the caller marks for remeshing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RebakeOutcome {
    /// The bake was rebuilt from the base (see [`PreparedLayers::bake_rect`]):
    /// every chunk reads different data.
    pub remesh_all: bool,
    /// World XZ rectangles whose bake changed without a writer marking them:
    /// the bounds of layers added, removed or changed (a spline's corridor),
    /// and the corridors of splines whose profile moved.
    pub remesh: Vec<(Vec2, Vec2)>,
}

/// The base of a terrain root with every enabled layer applied (see the
/// module docs). Present only while the root has layers. Read it through
/// [`surface_data`], never directly, so a root without one reads its base.
#[derive(Component, Clone, Debug)]
pub struct TerrainBaked {
    /// The baked raster and material map, laid out like the base.
    pub data: TerrainData,
    /// The layers baked in, in apply order.
    layers: Vec<LayerDesc>,
    /// `layers` prepared against the base; `None` until the next bake after
    /// the list was replaced.
    prepared: Option<PreparedLayers>,
    /// The list, and its preparation, that `data` was last baked with, kept
    /// from the first replacement since then until the next bake diffs it.
    previous: Option<(Vec<LayerDesc>, Option<PreparedLayers>)>,
    /// `data` is still the base copy [`Self::new`] made.
    unbaked: bool,
}

impl TerrainBaked {
    /// A bake of `base` under `layers`, holding a plain copy of the base
    /// until the next [`Self::rebake`] bakes it whole and reports the layers'
    /// bounds for remeshing. Making one costs a copy of the base and nothing
    /// more; `apply_terrain_dirty_chunks` runs that first bake.
    pub fn new(base: &TerrainData, layers: Vec<LayerDesc>) -> Self {
        Self { data: base.clone(), layers: sorted_layers(layers), prepared: None, previous: None, unbaked: true }
    }

    /// The layers baked in, in apply order.
    pub fn layers(&self) -> &[LayerDesc] {
        &self.layers
    }

    /// Spline layer `id` as the last bake laid it: its parameters and its
    /// stations (world XZ along the curve, the smoothed profile as Y), the
    /// very line its corridor was carved to. A road's drivable surface is
    /// laid along these rather than a second evaluation of the spline, whose
    /// extra knots could read other ground; its heights are then read from
    /// the finished bake, so a later layer over the corridor is followed
    /// too. `None` when the bake holds
    /// no such spline (disabled, fewer than two points, nothing to change)
    /// or has not run since its layer list was replaced.
    pub fn baked_spline(&self, id: u64) -> Option<(&SplineLayer, &[Vec3])> {
        let layer = self.prepared.as_ref()?.layers.iter().find(|layer| layer.desc.id == id)?;
        match (&layer.desc.kind, &layer.spline) {
            (LayerKind::Spline(spline), Some(prepared)) => Some((spline, prepared.stations.as_slice())),
            _ => None,
        }
    }

    /// The layer list as the last bake prepared it; `None` before the first
    /// bake, and from a replacement of the list until the next bake. Scatter
    /// reads the spline corridors from it ([`PreparedLayers::in_corridor`]).
    pub fn prepared(&self) -> Option<&PreparedLayers> {
        self.prepared.as_ref()
    }

    /// Replace the layer list. Nothing is baked here: the next
    /// [`Self::rebake`] diffs the list `data` was baked with against this
    /// one, bakes the difference and reports it for remeshing, however many
    /// replacements came between. A list equal to the current one changes
    /// nothing.
    pub fn set_layers(&mut self, layers: Vec<LayerDesc>) {
        let layers = sorted_layers(layers);
        if layers == self.layers {
            return;
        }
        let old = std::mem::replace(&mut self.layers, layers);
        let old_prepared = self.prepared.take();
        // An unbaked copy bakes whole anyway, and a second replacement keeps
        // the list the data still reflects.
        if !self.unbaked && self.previous.is_none() {
            self.previous = Some((old, old_prepared));
        }
    }

    /// Whether the next [`Self::rebake`] has work of its own, even with
    /// nothing marked: the first bake, or a replaced layer list.
    pub fn wants_bake(&self) -> bool {
        self.unbaked || self.prepared.is_none()
    }

    /// Bring the bake up to date with `base` and the layer list: bake what
    /// `request` names, whatever a replaced layer list changed, the whole
    /// raster on the first bake, and the corridor of every spline whose
    /// profile a base edit moved. Returns what changed beyond `request`.
    pub fn rebake(&mut self, config: &TerrainConfig, base: &TerrainData, request: &TerrainRebake) -> RebakeOutcome {
        let mut outcome = RebakeOutcome::default();
        let prepared = match self.prepared.take() {
            Some(mut prepared) => {
                outcome.remesh.extend(prepared.refresh(config, base));
                prepared
            }
            None => {
                let fresh = PreparedLayers::new(config, base, &self.layers);
                if let Some((old_layers, old_prepared)) = self.previous.take() {
                    outcome.remesh.extend(prepared_layer_changes(&old_layers, old_prepared.as_ref(), &self.layers, &fresh));
                    if let Some(old_prepared) = &old_prepared {
                        outcome.remesh.extend(fresh.moved_splines(old_prepared));
                    }
                }
                fresh
            }
        };
        let whole = request.whole || self.unbaked;
        if self.unbaked {
            // The chunks meshed from the base copy differ from the bake only
            // under the layers.
            outcome.remesh.extend(prepared.bounds());
            self.unbaked = false;
        }
        let rebuilt = if whole {
            prepared.bake_all(config, base, &mut self.data)
        } else {
            // A bake that had to rebuild the layout baked every cell, which
            // `any` stops at.
            request
                .rects
                .iter()
                .chain(&outcome.remesh)
                .any(|rect| prepared.bake_rect(config, base, *rect, &mut self.data))
        };
        outcome.remesh_all = rebuilt;
        self.prepared = Some(prepared);
        outcome
    }

    /// Re-express the baked heights, normalized in band `from`, in band `to`,
    /// after Save moved the base's band so it could hold every height. The
    /// layers and their preparation are in world units and stay as they are.
    pub fn rebase_heights(&mut self, from: HeightBand, to: HeightBand) {
        from.rebase_into(to, &mut self.data.height_cache);
    }

    /// `data` is laid out like `base`, the only layout readers can index
    /// with the root's config.
    fn matches_layout(&self, base: &TerrainData) -> bool {
        self.data.cache_width == base.cache_width
            && self.data.cache_height == base.cache_height
            && self.data.height_cache.len() == base.height_cache.len()
    }
}

/// The terrain every reader sees: the bake when the root has one, else the
/// base. Meshers, colliders, the surface material, picking raycasts and
/// gameplay material queries all read through this; writers write the base.
/// A bake left laid out for another raster (the base was replaced and not
/// yet re-baked) is passed over for the base.
#[inline]
pub fn surface_data<'a>(base: &'a TerrainData, baked: Option<&'a TerrainBaked>) -> &'a TerrainData {
    match baked {
        Some(baked) if baked.matches_layout(base) => &baked.data,
        _ => base,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::height_query::{material_at_world, set_height_at_world};

    /// 3 x 3 chunks of 32 m at 32 cells: a 96 x 96 raster, about 1 m cells,
    /// spanning world -32..64 on both axes, heights 0..100 m.
    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 32,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            height_offset: 0.0,
            ..TerrainConfig::default()
        }
    }

    /// A base whose world height is `height(x, z)` at every cell, with an
    /// all-Grass material layer.
    fn base_with(config: &TerrainConfig, height: impl Fn(f32, f32) -> f32) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        ensure_material_cache(&mut data);
        data.material_dirty = false;
        let grid = RasterGrid::of(config, &data).expect("a whole raster");
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                data.height_cache[z * grid.width + x] = config.normalized_height(height(p.x, p.y));
            }
        }
        data
    }

    fn bumpy(config: &TerrainConfig) -> TerrainData {
        base_with(config, |x, z| 20.0 + 3.0 * (x * 0.21).sin() + 2.0 * (z * 0.17).cos())
    }

    fn footprint(config: &TerrainConfig) -> (Vec2, Vec2) {
        config.footprint_xz()
    }

    fn bake_whole(config: &TerrainConfig, base: &TerrainData, layers: &[LayerDesc]) -> TerrainData {
        let mut out = TerrainData::default();
        bake_region(config, base, layers, footprint(config), &mut out);
        out
    }

    fn layer(id: u64, order: i32, kind: LayerKind) -> LayerDesc {
        LayerDesc { id, order, kind }
    }

    fn stamp(center: Vec3, shape: StampShape, blend: StampBlend) -> StampLayer {
        StampLayer { center, yaw: 0.0, shape, radius: 8.0, height: 6.0, falloff: 3.0, blend, strength: 1.0 }
    }

    fn road(points: Vec<Vec3>) -> SplineLayer {
        SplineLayer {
            mode: SplineMode::Road,
            points,
            width: 8.0,
            shoulder_width: 4.0,
            depth: 0.0,
            smoothing: 0.5,
            bed_material: Some(TerrainMaterial::Asphalt.to_u8()),
            shoulder_material: Some(TerrainMaterial::Rock.to_u8()),
        }
    }

    /// World height of the baked cell nearest `(x, z)`.
    fn cell_height(config: &TerrainConfig, data: &TerrainData, x: f32, z: f32) -> f32 {
        let grid = RasterGrid::of(config, data).unwrap();
        let fx = ((x - grid.origin.x) / grid.step.x).round() as usize;
        let fz = ((z - grid.origin.y) / grid.step.y).round() as usize;
        config.world_height(data.height_cache[fz * grid.width + fx])
    }

    /// Every cell of `out` outside `bounds` equals `base`, heights and
    /// materials bit for bit; returns how many cells inside differ.
    fn changes_only_inside(config: &TerrainConfig, base: &TerrainData, out: &TerrainData, bounds: (Vec2, Vec2)) -> usize {
        let grid = RasterGrid::of(config, base).unwrap();
        let mut inside_changes = 0;
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                let i = z * grid.width + x;
                let changed = out.height_cache[i] != base.height_cache[i] || out.material_cache[i] != base.material_cache[i];
                let inside = p.x >= bounds.0.x && p.x <= bounds.1.x && p.y >= bounds.0.y && p.y <= bounds.1.y;
                if inside {
                    inside_changes += usize::from(changed);
                } else {
                    assert!(!changed, "cell ({x}, {z}) at {p} outside {bounds:?} changed");
                }
            }
        }
        inside_changes
    }

    #[test]
    fn no_layers_bake_to_the_base() {
        let config = config();
        let mut base = bumpy(&config);
        base.material_cache[17] = material_cell(TerrainMaterial::Sand.to_u8());
        let out = bake_whole(&config, &base, &[]);
        assert_eq!(out.height_cache, base.height_cache);
        assert_eq!(out.material_cache, base.material_cache);

        let mut baked = TerrainBaked::new(&base, Vec::new());
        assert!(baked.wants_bake());
        let outcome = baked.rebake(&config, &base, &TerrainRebake::default());
        assert!(!baked.wants_bake());
        assert!(!outcome.remesh_all && outcome.remesh.is_empty(), "{outcome:?}");
        assert_eq!(baked.data.height_cache, base.height_cache);
        assert_eq!(baked.data.material_cache, base.material_cache);
    }

    #[test]
    fn each_layer_kind_changes_only_cells_inside_its_bounds() {
        let config = config();
        let base = bumpy(&config);
        let rock = TerrainMaterial::Rock.to_u8();
        let kinds = [
            LayerKind::Spline(road(vec![Vec3::new(-20.0, 22.0, -10.0), Vec3::new(0.0, 22.0, 5.0), Vec3::new(20.0, 22.0, 0.0)])),
            LayerKind::Spline(SplineLayer {
                mode: SplineMode::Canyon,
                depth: 8.0,
                ..road(vec![Vec3::new(10.0, 20.0, 30.0), Vec3::new(40.0, 20.0, 40.0)])
            }),
            LayerKind::Stamp(stamp(Vec3::new(10.0, 20.0, 10.0), StampShape::Crater, StampBlend::Add)),
            LayerKind::Stamp(StampLayer { yaw: 0.7, ..stamp(Vec3::new(-5.0, 20.0, 20.0), StampShape::Ridge, StampBlend::Max) }),
            LayerKind::FlattenPad(FlattenPadLayer {
                center: Vec3::new(30.0, 25.0, -10.0),
                yaw: 0.4,
                size: Vec2::new(10.0, 6.0),
                falloff: 4.0,
                material: Some(TerrainMaterial::Concrete.to_u8()),
            }),
            LayerKind::Noise(NoiseLayer {
                center: Vec3::new(-10.0, 0.0, -15.0),
                yaw: 0.2,
                size: Vec2::new(20.0, 14.0),
                amplitude: 4.0,
                frequency: 0.15,
                octaves: 4,
                seed: 7,
                falloff: 3.0,
            }),
            LayerKind::MaterialFill(MaterialFillLayer {
                center: Vec3::new(0.0, 0.0, 0.0),
                size: Vec2::new(16.0, 16.0),
                material: rock,
                ..MaterialFillLayer::default()
            }),
        ];
        for kind in kinds {
            let desc = layer(1, 0, kind);
            let bounds = desc.bounds().expect("a live layer has bounds");
            let out = bake_whole(&config, &base, std::slice::from_ref(&desc));
            let changed = changes_only_inside(&config, &base, &out, bounds);
            assert!(changed > 0, "{:?} changed nothing inside its bounds", desc.kind);
        }
    }

    #[test]
    fn a_yawed_ridge_moves_nothing_beyond_its_crest_under_any_blend() {
        let config = config();
        let base = bumpy(&config);
        let grid = RasterGrid::of(&config, &base).unwrap();
        let step = grid.step.max_element();
        let (radius, yaw) = (20.0f32, 0.7f32);
        let half_w = radius * RIDGE_WIDTH_FRACTION;
        let (sin, cos) = yaw.sin_cos();
        // The ground stands at 15 to 25 m, so 40 m is above it and 0 m below:
        // each blend moves every cell it weighs at all. The ridge's box is
        // axis-aligned and so reaches well past the yawed band at its corners.
        for (blend, level) in [(StampBlend::Replace, 40.0), (StampBlend::Max, 40.0), (StampBlend::Min, 0.0)] {
            let center = Vec3::new(16.0, level, 16.0);
            let ridge = StampLayer { yaw, radius, ..stamp(center, StampShape::Ridge, blend) };
            let out = bake_whole(&config, &base, &[layer(1, 0, LayerKind::Stamp(ridge))]);
            let mut changed = 0;
            for z in 0..grid.height {
                for x in 0..grid.width {
                    let i = z * grid.width + x;
                    if out.height_cache[i] == base.height_cache[i] {
                        continue;
                    }
                    changed += 1;
                    // Into the stamp's frame, turned as `Frame::local` turns it.
                    let d = grid.world(x, z) - Vec2::new(center.x, center.z);
                    let along = d.x * cos - d.y * sin;
                    let across = d.x * sin + d.y * cos;
                    assert!(across.abs() < half_w + step, "{blend:?} moved a cell {across} m across the crest, past its {half_w} m band");
                    assert!(along.abs() < radius + step, "{blend:?} moved a cell {along} m along the crest, past its {radius} m radius");
                }
            }
            assert!(changed > 0, "{blend:?} moved nothing");
        }
    }

    #[test]
    fn dragging_a_spline_point_rebakes_its_corridors_not_their_box() {
        let config = config();
        let base = bumpy(&config);
        let before = layer(1, 0, LayerKind::Spline(road(vec![Vec3::new(-28.0, 20.0, -28.0), Vec3::new(60.0, 20.0, 60.0)])));
        let after = layer(1, 0, LayerKind::Spline(road(vec![Vec3::new(-28.0, 20.0, -28.0), Vec3::new(60.0, 20.0, 52.0)])));
        let mut baked = TerrainBaked::new(&base, vec![before.clone()]);
        baked.rebake(&config, &base, &TerrainRebake::default());
        baked.set_layers(vec![after.clone()]);
        let outcome = baked.rebake(&config, &base, &TerrainRebake::default());
        assert!(!outcome.remesh_all && !outcome.remesh.is_empty(), "{outcome:?}");

        // Far from both curves, yet inside both boxes.
        let far = Vec2::new(50.0, -20.0);
        let holds_far = |(lo, hi): (Vec2, Vec2)| far.x >= lo.x && far.x <= hi.x && far.y >= lo.y && far.y <= hi.y;
        assert!(holds_far(before.bounds().unwrap()) && holds_far(after.bounds().unwrap()));
        assert!(!outcome.remesh.iter().any(|rect| holds_far(*rect)), "{far} is rebaked though no corridor comes near it");

        // Re-baking only the corridors leaves exactly what a whole bake gives.
        let fresh = bake_whole(&config, &base, &[after]);
        assert_eq!(baked.data.height_cache, fresh.height_cache);
        assert_eq!(baked.data.material_cache, fresh.material_cache);
    }

    #[test]
    fn order_decides_which_replace_wins() {
        let config = config();
        let base = bumpy(&config);
        let low = LayerKind::Stamp(stamp(Vec3::new(0.0, 10.0, 0.0), StampShape::Plateau, StampBlend::Replace));
        let high = LayerKind::Stamp(stamp(Vec3::new(0.0, 30.0, 0.0), StampShape::Plateau, StampBlend::Replace));

        let high_last = bake_whole(&config, &base, &[layer(1, 0, low.clone()), layer(2, 1, high.clone())]);
        assert!((cell_height(&config, &high_last, 0.0, 0.0) - 36.0).abs() < 1e-3);

        let low_last = bake_whole(&config, &base, &[layer(1, 1, low.clone()), layer(2, 0, high.clone())]);
        assert!((cell_height(&config, &low_last, 0.0, 0.0) - 16.0).abs() < 1e-3);

        // Equal orders fall back to the id; the list's own order is irrelevant.
        let by_id = bake_whole(&config, &base, &[layer(9, 0, low), layer(3, 0, high)]);
        assert!((cell_height(&config, &by_id, 0.0, 0.0) - 16.0).abs() < 1e-3);
    }

    #[test]
    fn a_road_flattens_its_corridor_to_the_profile_and_paints_it() {
        let config = config();
        let base = bumpy(&config);
        // Nodes closer than the knot spacing, all at 10 m: the profile is 10 m
        // everywhere, so the bed must be too, over ground at 15 to 25 m.
        let points: Vec<Vec3> = (-3..=3).map(|i| Vec3::new(i as f32 * 8.0, 10.0, 4.0)).collect();
        let desc = layer(1, 0, LayerKind::Spline(road(points)));
        let out = bake_whole(&config, &base, std::slice::from_ref(&desc));

        let asphalt = TerrainMaterial::Asphalt.to_u8();
        for x in (-20..=20).map(|x| x as f32) {
            for z in [1.5f32, 4.0, 6.5] {
                let h = cell_height(&config, &out, x, z);
                assert!((h - 10.0).abs() < 1e-3, "bed at ({x}, {z}) is {h}, not the 10 m profile");
                assert_eq!(material_at_world(&config, &out, x, z).map(|m| m.primary), Some(asphalt));
            }
        }
        // Across the shoulder the height climbs back to the ground.
        let (edge, mid, beyond) = (cell_height(&config, &out, 0.0, 9.0), cell_height(&config, &out, 0.0, 10.0), cell_height(&config, &out, 0.0, 12.5));
        let ground = cell_height(&config, &base, 0.0, 12.5);
        assert!(edge < mid && mid < ground, "shoulder {edge} -> {mid} does not climb toward {ground}");
        assert_eq!(beyond, ground, "past the shoulder the ground is untouched");
    }

    #[test]
    fn a_road_follows_a_sloped_profile() {
        let config = config();
        let base = bumpy(&config);
        let points: Vec<Vec3> = (0..=4).map(|i| Vec3::new(-20.0 + i as f32 * 10.0, 10.0 + i as f32 * 2.5, -8.0)).collect();
        let out = bake_whole(&config, &base, &[layer(1, 0, LayerKind::Spline(road(points)))]);
        for x in [-15.0f32, -5.0, 5.0, 15.0] {
            let expected = 10.0 + (x + 20.0) * 0.25;
            let h = cell_height(&config, &out, x, -8.0);
            assert!((h - expected).abs() < 0.2, "centreline at x {x} is {h}, the profile is {expected}");
        }
    }

    #[test]
    fn a_baked_spline_hands_out_the_stations_its_corridor_was_carved_to() {
        let config = config();
        let base = bumpy(&config);
        let points: Vec<Vec3> = (0..=4).map(|i| Vec3::new(-20.0 + i as f32 * 10.0, 10.0 + i as f32 * 2.5, -8.0)).collect();
        let desc = layer(7, 0, LayerKind::Spline(road(points)));
        let mut baked = TerrainBaked::new(&base, vec![desc.clone()]);
        assert!(baked.baked_spline(7).is_none(), "nothing is laid before the first bake");
        baked.rebake(&config, &base, &TerrainRebake::default());

        let (spline, stations) = baked.baked_spline(7).expect("the road was baked");
        assert_eq!(spline.width, 8.0);
        assert!(stations.len() > 4, "{} stations", stations.len());
        // The profile climbs 0.25 m per metre and the nearest cell is at most
        // half a cell along the road from a station.
        for station in &stations[2..stations.len() - 2] {
            let bed = cell_height(&config, &baked.data, station.x, station.z);
            assert!((bed - station.y).abs() < 0.2, "the bed under {station:?} is at {bed}");
        }
        assert!(baked.baked_spline(8).is_none(), "no layer has that id");

        baked.set_layers(vec![layer(7, 1, desc.kind.clone())]);
        assert!(baked.baked_spline(7).is_none(), "a replaced list is laid only by the next bake");
    }

    #[test]
    fn a_river_carves_its_bed_and_never_raises() {
        let config = config();
        let base = base_with(&config, |_, _| 20.0);
        let water = TerrainMaterial::Water.to_u8();
        let river = SplineLayer {
            mode: SplineMode::River,
            depth: 5.0,
            width: 6.0,
            bed_material: Some(water),
            shoulder_material: None,
            ..road((-3..=3).map(|i| Vec3::new(i as f32 * 8.0, 20.0, -5.0)).collect())
        };
        let out = bake_whole(&config, &base, &[layer(1, 0, LayerKind::Spline(river))]);
        let bed = cell_height(&config, &out, 0.0, -5.0);
        assert!((bed - 15.0).abs() < 1e-3, "bed at {bed}, not 5 m under the 20 m profile");
        assert_eq!(material_at_world(&config, &out, 0.0, -5.0).map(|m| m.primary), Some(water));
        let bank = cell_height(&config, &out, 0.0, -5.0 + 4.5);
        assert!(bank > 15.0 && bank < 20.0, "bank at {bank}");
        for (i, n) in out.height_cache.iter().enumerate() {
            assert!(config.world_height(*n) <= 20.0 + 1e-4, "cell {i} was raised to {}", config.world_height(*n));
        }
        assert!((cell_height(&config, &out, 0.0, 10.0) - 20.0).abs() < 1e-4, "past the banks the ground is untouched");
    }

    #[test]
    fn a_material_fill_respects_its_slope_and_height_ranges() {
        let config = config();
        // Flat at 5 m west of x = 0, a 45 degree ramp east of it.
        let base = base_with(&config, |x, _| 5.0 + x.max(0.0));
        let (rock, snow, grass) = (TerrainMaterial::Rock.to_u8(), TerrainMaterial::Snow.to_u8(), TerrainMaterial::Grass.to_u8());
        let steep = layer(
            1,
            0,
            LayerKind::MaterialFill(MaterialFillLayer {
                center: Vec3::new(16.0, 0.0, 16.0),
                size: Vec2::new(96.0, 96.0),
                material: rock,
                min_slope: 30.0,
                max_slope: 90.0,
                ..MaterialFillLayer::default()
            }),
        );
        let high = layer(
            2,
            1,
            LayerKind::MaterialFill(MaterialFillLayer {
                center: Vec3::new(16.0, 0.0, 16.0),
                size: Vec2::new(96.0, 96.0),
                material: snow,
                min_height: 40.0,
                max_height: 1000.0,
                ..MaterialFillLayer::default()
            }),
        );
        let out = bake_whole(&config, &base, &[steep, high]);
        let primary = |x: f32, z: f32| material_at_world(&config, &out, x, z).map(|m| m.primary);
        assert_eq!(primary(-20.0, 0.0), Some(grass), "flat ground is not steep enough");
        assert_eq!(primary(-3.0, 10.0), Some(grass));
        assert_eq!(primary(10.0, 0.0), Some(rock), "the ramp is 45 degrees at 15 m");
        assert_eq!(primary(30.0, -20.0), Some(rock), "the ramp is 45 degrees at 35 m");
        assert_eq!(primary(40.0, 5.0), Some(snow), "45 m is above 40 m, and the later fill wins");
        assert_eq!(primary(60.0, 30.0), Some(snow));
        // Nothing moved: a fill paints, it never shapes.
        assert_eq!(out.height_cache, base.height_cache);
    }

    #[test]
    fn rebaking_old_and_new_bounds_after_a_stamp_moves_restores_the_base() {
        let config = config();
        let base = bumpy(&config);
        let at_a = layer(5, 0, LayerKind::Stamp(stamp(Vec3::new(-15.0, 20.0, -15.0), StampShape::Mound, StampBlend::Add)));
        let at_b = layer(5, 0, LayerKind::Stamp(stamp(Vec3::new(25.0, 20.0, 30.0), StampShape::Mound, StampBlend::Add)));
        let (a_bounds, b_bounds) = (at_a.bounds().unwrap(), at_b.bounds().unwrap());

        let mut out = bake_whole(&config, &base, std::slice::from_ref(&at_a));
        assert!(changes_only_inside(&config, &base, &out, a_bounds) > 0);
        let rects = layer_changes(std::slice::from_ref(&at_a), std::slice::from_ref(&at_b));
        assert_eq!(rects, vec![a_bounds, b_bounds]);
        for rect in rects {
            assert!(!bake_region(&config, &base, std::slice::from_ref(&at_b), rect, &mut out));
        }
        assert!(changes_only_inside(&config, &base, &out, b_bounds) > 0, "the stamp is at B now");
        assert_eq!(out.height_cache, bake_whole(&config, &base, std::slice::from_ref(&at_b)).height_cache);

        // The same move through the component, which finds the bounds itself.
        let mut baked = TerrainBaked::new(&base, vec![at_a.clone()]);
        let first = baked.rebake(&config, &base, &TerrainRebake::default());
        assert_eq!(first.remesh, vec![a_bounds], "the first bake reports the layers' bounds");
        baked.set_layers(vec![at_b.clone()]);
        assert!(baked.wants_bake());
        let moved = baked.rebake(&config, &base, &TerrainRebake::default());
        assert_eq!(moved.remesh, vec![a_bounds, b_bounds]);
        assert!(!moved.remesh_all);
        assert!(changes_only_inside(&config, &base, &baked.data, b_bounds) > 0);

        // Removing the layer puts the base back everywhere.
        baked.set_layers(Vec::new());
        let removed = baked.rebake(&config, &base, &TerrainRebake::default());
        assert_eq!(removed.remesh, vec![b_bounds]);
        assert_eq!(baked.data.height_cache, base.height_cache);
    }

    #[test]
    fn a_base_edit_under_a_profile_knot_rebakes_the_whole_corridor() {
        let config = config();
        let mut base = bumpy(&config);
        // Two nodes 60 m apart at zero smoothing: extra knots every 5 m or so
        // sample the ground between them.
        let spline = SplineLayer { smoothing: 0.0, ..road(vec![Vec3::new(-28.0, 20.0, 0.0), Vec3::new(32.0, 20.0, 0.0)]) };
        let desc = layer(1, 0, LayerKind::Spline(spline));
        let mut baked = TerrainBaked::new(&base, vec![desc.clone()]);
        baked.rebake(&config, &base, &TerrainRebake::default());

        // Raise a patch of the base around one knot, and ask for just that patch.
        let (lo, hi) = (Vec2::new(-6.0, -3.0), Vec2::new(6.0, 3.0));
        let grid = RasterGrid::of(&config, &base).unwrap();
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                if p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y {
                    set_height_at_world(&config, &mut base, p.x, p.y, 40.0, 1.0);
                }
            }
        }
        let outcome = baked.rebake(&config, &base, &TerrainRebake { whole: false, rects: vec![(lo, hi)] });
        assert!(!outcome.remesh.is_empty(), "the profile moved, so the corridor must be remeshed");
        let fresh = bake_whole(&config, &base, &[desc]);
        assert_eq!(baked.data.height_cache, fresh.height_cache, "the rebake matches a bake from scratch");
    }

    #[test]
    fn a_layer_that_paints_gives_a_bare_base_a_material_layer() {
        let config = config();
        let mut base = bumpy(&config);
        base.material_cache.clear();
        let pad = layer(
            1,
            0,
            LayerKind::FlattenPad(FlattenPadLayer {
                center: Vec3::new(0.0, 20.0, 0.0),
                yaw: 0.0,
                size: Vec2::new(6.0, 6.0),
                falloff: 2.0,
                material: Some(TerrainMaterial::Concrete.to_u8()),
            }),
        );
        let mut out = base.clone();
        assert!(bake_region(&config, &base, &[pad.clone()], (Vec2::ZERO, Vec2::ONE), &mut out), "the layout changed");
        assert!(out.has_material_layer() && out.material_dirty);
        assert_eq!(material_at_world(&config, &out, 0.0, 0.0).map(|m| m.primary), Some(TerrainMaterial::Concrete.to_u8()));
        assert_eq!(material_at_world(&config, &out, 30.0, 30.0).map(|m| m.primary), Some(TerrainMaterial::Grass.to_u8()));
        // Baking again in the same layout is not a layout change.
        assert!(!bake_region(&config, &base, &[pad], (Vec2::ZERO, Vec2::ONE), &mut out));
    }

    #[test]
    fn surface_data_reads_the_bake_only_when_it_fits_the_base() {
        let config = config();
        let base = bumpy(&config);
        assert!(std::ptr::eq(surface_data(&base, None), &base));
        let baked = TerrainBaked::new(&base, Vec::new());
        assert!(std::ptr::eq(surface_data(&base, Some(&baked)), &baked.data));
        let mut other = base.clone();
        other.resize_cache(&TerrainConfig { chunks_x: 2, ..config.clone() });
        assert!(std::ptr::eq(surface_data(&other, Some(&baked)), &other), "a stale layout falls back to the base");
    }

    #[test]
    fn names_round_trip() {
        for mode in SplineMode::ALL {
            assert_eq!(SplineMode::from_name(mode.name()), Some(mode));
        }
        for shape in StampShape::ALL {
            assert_eq!(StampShape::from_name(&shape.name().to_lowercase()), Some(shape));
        }
        for blend in StampBlend::ALL {
            assert_eq!(StampBlend::from_name(&format!(" {} ", blend.name())), Some(blend));
        }
        assert_eq!(SplineMode::from_name("Highway"), None);
    }
}
