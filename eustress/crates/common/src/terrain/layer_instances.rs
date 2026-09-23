//! Terrain layers as instances: the `TerrainSpline`, `TerrainSplinePoint`,
//! `TerrainStamp`, `TerrainFlattenPad`, `TerrainNoise` and
//! `TerrainMaterialFill` classes, how their components map onto the
//! [`LayerDesc`]s the bake takes, and the system that keeps every terrain
//! root's [`TerrainBaked`] in step with them.
//!
//! `TerrainScatter` and `TerrainWaterBody` are layer classes in every way but
//! one: they live in the same folder, keep their properties in a field table,
//! load, save, edit and undo the same way, but they bake nothing. A scatter
//! maps onto a `scatter::ScatterLayer`, which `scatter` places over the
//! finished ground, and a water body onto a `water_bodies::WaterBodyDesc`,
//! which `water_bodies` floods over it, so the bake sync below leaves both
//! out. A spline's WaterSurface and WaterFill likewise bake nothing: they
//! are read by `water_bodies` and stay out of its `SplineLayer`, so filling a
//! river never re-carves it.
//!
//! ## Classes
//! Each class is one component whose properties are a field table, the same
//! machinery the `ParticleSimulation` class uses
//! (`realism::particle_sim::class`): one table per class drives its TOML
//! section, its Properties rows, panel edits and undo. Position and yaw come
//! from the instance's `Transform` under its ancestors'; a stamp, pad, noise
//! or fill ignores its scale. A spline's control points are its
//! `TerrainSplinePoint` children, joined in ascending `Index` (then by name),
//! each placed by its own `Transform` under the spline's, so moving the
//! spline moves the whole corridor and the Move tool edits single points.
//!
//! ## Where they live
//! On disk a layer is a folder under `Workspace/Terrain/Layers` and a point
//! a folder inside its spline's. In the ECS a layer is NOT parented to the
//! `TerrainRoot`. That root is runtime state: regenerating, importing or the
//! Terrain class sync despawns it and spawns another, and a despawn takes
//! every child with it, which would drop the layers from the world while
//! their files stay on disk. At Space open the root also spawns after the
//! file loader, so there is nothing to parent to yet. Layers therefore sit
//! beside the Workspace's own children, belong to whatever terrain root
//! exists (a Space has one), and the Explorer lists them under it.
//!
//! ## Keeping the bake current
//! [`sync_terrain_layers`] watches the layer components, their transforms and
//! their ancestors', their hierarchy, their removal and new terrain roots.
//! When anything changed it gathers every enabled layer into a `LayerDesc`
//! list, sorted by `Order` then by a stable id (from the instance uuid), and
//! hands it to each root:
//! a root without a bake gets a new [`TerrainBaked`], a root with one gets
//! [`TerrainBaked::set_layers`], and when the list is empty the bake is
//! removed and the bounds of the layers it held (every chunk, when the bake
//! carried a material layer the base lacks) are marked dirty so those
//! chunks remesh from the base. The bake keeps the list it last baked, so a
//! layer that was removed, moved or disabled re-bakes its old bounds too,
//! which is what restores the base under it. A hand-over happens at most
//! once every [`PUSH_INTERVAL_SECS`], so a drag re-bakes about 20 times a
//! second rather than every frame, and a change held back by that limit is
//! always handed over once it passes, so the final state is always baked.
//! The baking itself, and marking what it changed for remeshing, happen in
//! `apply_terrain_dirty_chunks`, which this system runs before. After it,
//! `road_surface::sync_road_surfaces` rebuilds the drivable surface of every
//! Road spline whose baked stations, or the finished ground under them,
//! changed.

use std::path::{Path, PathBuf};

use bevy::ecs::component::Mutable;
use bevy::ecs::system::{EntityCommands, SystemParam};
use bevy::prelude::*;

use super::layers::{
    FlattenPadLayer, LayerDesc, LayerKind, MaterialFillLayer, NoiseLayer, SplineLayer, SplineMode, StampBlend,
    StampLayer, StampShape, TerrainBaked,
};
use super::material::{TerrainMaterial, MATERIAL_SLOT_NONE};
use super::road_surface::{sync_road_surfaces, RoadSurfaces};
use super::scatter::{
    ScatterFootprint, ScatterKind, ScatterLayer, TreeType, MAX_DENSITY, MAX_RADIUS, MAX_SCALE, MIN_SCALE,
};
use super::water_bodies::WaterBodyDesc;
use super::{apply_terrain_dirty_chunks, TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainRoot};
use crate::classes::{ClassName, Instance};
use crate::realism::particle_sim::class::{field as field_spec, from_section, FieldKind, FieldSpec, FieldTable, FieldValue};

/// A changed layer stack is handed to the bake at most this often, seconds.
pub const PUSH_INTERVAL_SECS: f64 = 0.05;

/// TOML section of a `TerrainSpline` instance.
pub const SPLINE_SECTION: &str = "terrain_spline";
/// TOML section of a `TerrainSplinePoint` instance.
pub const POINT_SECTION: &str = "terrain_spline_point";
/// TOML section of a `TerrainStamp` instance.
pub const STAMP_SECTION: &str = "terrain_stamp";
/// TOML section of a `TerrainFlattenPad` instance.
pub const FLATTEN_PAD_SECTION: &str = "terrain_flatten_pad";
/// TOML section of a `TerrainNoise` instance.
pub const NOISE_SECTION: &str = "terrain_noise";
/// TOML section of a `TerrainMaterialFill` instance.
pub const MATERIAL_FILL_SECTION: &str = "terrain_material_fill";
/// TOML section of a `TerrainScatter` instance.
pub const SCATTER_SECTION: &str = "terrain_scatter";
/// TOML section of a `TerrainWaterBody` instance.
pub const WATER_BODY_SECTION: &str = "terrain_water_body";
/// Every section the layer classes own.
pub const LAYER_SECTIONS: [&str; 8] = [
    SPLINE_SECTION,
    POINT_SECTION,
    STAMP_SECTION,
    FLATTEN_PAD_SECTION,
    NOISE_SECTION,
    MATERIAL_FILL_SECTION,
    SCATTER_SECTION,
    WATER_BODY_SECTION,
];

/// Folder under a Space's `Workspace/Terrain` that holds its layer instances.
pub const LAYERS_FOLDER: &str = "Layers";

/// A Space's terrain layer folder, `Workspace/Terrain/Layers`.
pub fn layers_dir(space_root: &Path) -> PathBuf {
    space_root.join("Workspace").join("Terrain").join(LAYERS_FOLDER)
}

/// Ancestors followed when composing a layer's world pose; a deeper chain is
/// a broken hierarchy, not a real one.
const MAX_POSE_DEPTH: usize = 64;

/// World pose of `entity`: its `Transform` under its ancestors', folded here
/// rather than read from `GlobalTransform`, which lags a frame. The layer bake
/// and the road tool both use this so a click inverted into spline-local space
/// lands back where the bake places it. An ancestor without a `Transform` (a
/// service root) counts as the identity.
pub fn compose_world_pose(
    entity: Entity,
    transform: impl Fn(Entity) -> Option<Transform>,
    parent: impl Fn(Entity) -> Option<Entity>,
) -> Transform {
    let mut pose = transform(entity).unwrap_or_default();
    let mut next = parent(entity);
    for _ in 0..MAX_POSE_DEPTH {
        let Some(ancestor) = next else { break };
        if let Some(parent_pose) = transform(ancestor) {
            pose = parent_pose.mul_transform(pose);
        }
        next = parent(ancestor);
    }
    pose
}

// ============================================================================
// Field tables
// ============================================================================

const SPLINE_MODES: &[&str] = &["Road", "Path", "River", "Canyon", "Embankment"];
const STAMP_SHAPES: &[&str] = &["Crater", "Mound", "Plateau", "Ridge"];
const STAMP_BLENDS: &[&str] = &["Add", "Max", "Min", "Replace"];
const SCATTER_KINDS: &[&str] = &["Grass", "Shrubs", "Rocks", "Trees", "Custom"];
const TREE_TYPES: &[&str] = &["Mixed", "Conifer", "Broadleaf"];
/// "None", then the built-in materials in slot order.
const MATERIALS: &[&str] = &[
    "None", "Grass", "Rock", "Dirt", "Snow", "Sand", "Mud", "Concrete", "Asphalt", "Slate", "Brick", "WoodPlanks",
    "Glacier", "Sandstone", "Basalt", "Ground", "CrackedLava", "Cobblestone", "Ice", "LeafyGrass", "Salt", "Limestone",
    "Pavement", "Water",
];

macro_rules! layer_field {
    ($name:literal, $key:literal, $kind:expr, $cat:literal, $unit:literal, $desc:literal) => {
        FieldSpec { name: $name, key: $key, kind: $kind, category: $cat, unit: $unit, description: $desc, restarts: false }
    };
}

use FieldKind::{Bool, Choice, Float, Int};

const ENABLED: FieldSpec =
    layer_field!("Enabled", "enabled", Bool, "Layer", "", "Apply this layer. Off leaves the ground as the layers below it shape it.");
const ORDER: FieldSpec = layer_field!("Order", "order", Int, "Layer", "",
    "Layers apply in ascending Order. Layers with the same Order apply in a fixed order of their own.");

/// Every property of `TerrainSpline`, in panel order.
pub const SPLINE_FIELDS: &[FieldSpec] = &[
    ENABLED,
    ORDER,
    layer_field!("Mode", "mode", Choice(SPLINE_MODES), "Spline", "",
        "Road cuts and fills a flat bed and lays a drivable surface on it, Path paints and smooths, River and Canyon carve below the profile, Embankment raises above it."),
    layer_field!("Width", "width", Float, "Spline", "m", "Full width of the bed."),
    layer_field!("ShoulderWidth", "shoulder_width", Float, "Spline", "m",
        "Width of the blend back into the ground on either side of the bed."),
    layer_field!("Depth", "depth", Float, "Spline", "m",
        "How far a River or Canyon carves below the profile, or an Embankment rises above it. Road and Path ignore it."),
    layer_field!("Smoothing", "smoothing", Float, "Spline", "",
        "0 to 1: how loosely the profile follows the ground between control points, and how hard a Path pulls toward it."),
    layer_field!("BedMaterial", "bed_material", Choice(MATERIALS), "Materials", "",
        "Material painted over the bed. None leaves the ground's own."),
    layer_field!("ShoulderMaterial", "shoulder_material", Choice(MATERIALS), "Materials", "",
        "Material painted over the shoulders, fading out across them. None leaves the ground's own."),
    layer_field!("WaterSurface", "water_surface", Bool, "Water", "",
        "River only: lay a water surface along the channel, flowing downhill. Nothing collides with it."),
    layer_field!("WaterFill", "water_fill", Float, "Water", "",
        "River only: 0 to 1, how full of water the channel is, as a fraction of its Depth."),
];

/// Every property of `TerrainSplinePoint`, in panel order.
pub const POINT_FIELDS: &[FieldSpec] = &[layer_field!("Index", "index", Float, "Point", "",
    "Place along the spline: points join in ascending Index, then by name. A fraction puts a point between two others.")];

/// Every property of `TerrainStamp`, in panel order.
pub const STAMP_FIELDS: &[FieldSpec] = &[
    ENABLED,
    ORDER,
    layer_field!("Shape", "shape", Choice(STAMP_SHAPES), "Stamp", "",
        "Crater: a bowl with a raised rim. Mound: a dome. Plateau: a flat top. Ridge: a crest along the stamp's X axis."),
    layer_field!("Radius", "radius", Float, "Stamp", "m", "Footprint radius, or a ridge's half length."),
    layer_field!("Height", "height", Float, "Stamp", "m", "Height of the shape, or a crater's depth."),
    layer_field!("Falloff", "falloff", Float, "Stamp", "m", "Distance inside the radius over which the stamp fades into the ground."),
    layer_field!("Blend", "blend", Choice(STAMP_BLENDS), "Stamp", "",
        "Add raises or lowers the ground by the shape. Max, Min and Replace stand the shape on the stamp's Position Y and raise, lower or set the ground to it."),
    layer_field!("Strength", "strength", Float, "Stamp", "", "0 to 1: how far the stamp takes the ground toward its shape."),
];

/// Every property of `TerrainFlattenPad`, in panel order.
pub const FLATTEN_PAD_FIELDS: &[FieldSpec] = &[
    ENABLED,
    ORDER,
    layer_field!("SizeX", "size_x", Float, "Pad", "m", "Flat top's extent along the pad's X axis. The top sits at the pad's Position Y."),
    layer_field!("SizeZ", "size_z", Float, "Pad", "m", "Flat top's extent along the pad's Z axis."),
    layer_field!("Falloff", "falloff", Float, "Pad", "m", "Distance beyond the flat top over which the pad blends back into the ground."),
    layer_field!("Material", "material", Choice(MATERIALS), "Pad", "",
        "Material painted over the pad, fading out across the falloff. None leaves the ground's own."),
];

/// Every property of `TerrainNoise`, in panel order.
pub const NOISE_FIELDS: &[FieldSpec] = &[
    ENABLED,
    ORDER,
    layer_field!("SizeX", "size_x", Float, "Noise", "m", "Footprint's extent along the layer's X axis."),
    layer_field!("SizeZ", "size_z", Float, "Noise", "m", "Footprint's extent along the layer's Z axis."),
    layer_field!("Amplitude", "amplitude", Float, "Noise", "m", "Largest height the noise adds or removes."),
    layer_field!("Frequency", "frequency", Float, "Noise", "1/m", "Cycles per metre of the coarsest octave."),
    layer_field!("Octaves", "octaves", Int, "Noise", "", "Layers of finer detail, 1 to 12."),
    layer_field!("Seed", "seed", Int, "Noise", "", "Pattern seed. The same seed and properties give the same ground."),
    layer_field!("Falloff", "falloff", Float, "Noise", "m", "Distance inside the footprint's edge over which the noise fades in."),
];

/// Every property of `TerrainMaterialFill`, in panel order.
pub const MATERIAL_FILL_FIELDS: &[FieldSpec] = &[
    ENABLED,
    ORDER,
    layer_field!("SizeX", "size_x", Float, "Fill", "m", "Footprint's extent along the layer's X axis."),
    layer_field!("SizeZ", "size_z", Float, "Fill", "m", "Footprint's extent along the layer's Z axis."),
    layer_field!("Material", "material", Choice(MATERIALS), "Fill", "", "Material painted wherever the rules hold. None paints nothing."),
    layer_field!("MinSlope", "min_slope", Float, "Rules", "deg", "Shallowest slope painted, in degrees from horizontal."),
    layer_field!("MaxSlope", "max_slope", Float, "Rules", "deg", "Steepest slope painted, in degrees from horizontal."),
    layer_field!("MinHeight", "min_height", Float, "Rules", "m", "Lowest ground painted."),
    layer_field!("MaxHeight", "max_height", Float, "Rules", "m", "Highest ground painted."),
];

/// Every property of `TerrainScatter`, in panel order. Enabled and Order
/// mean something else for a scatter than for a baked layer, so they are
/// its own rows rather than the shared `ENABLED` and `ORDER`.
pub const SCATTER_FIELDS: &[FieldSpec] = &[
    layer_field!("Enabled", "enabled", Bool, "Layer", "", "Place this scatter. Off removes everything it placed."),
    layer_field!("Order", "order", Int, "Layer", "",
        "Scatters in view place in ascending Order, each nearest the view first. The same Order places in a fixed order of its own."),
    layer_field!("Kind", "kind", Choice(SCATTER_KINDS), "Scatter", "",
        "What is placed: grass tufts, shrubs, rocks, trees, or the mesh MeshAsset names."),
    layer_field!("TreeType", "tree_type", Choice(TREE_TYPES), "Scatter", "",
        "Trees only: conifers, broadleaf trees, or a mix of both."),
    layer_field!("MeshAsset", "mesh_asset", FieldKind::Text, "Scatter", "",
        "Custom only: a .glb inside the Space folder, as a path relative to that folder, loaded through the Space's space:// asset source. Its first mesh and material are placed."),
    layer_field!("Density", "density", Float, "Scatter", "",
        "Instances per 100 square metres wherever every rule holds, up to 1000."),
    layer_field!("MinScale", "min_scale", Float, "Scatter", "", "Smallest size an instance is placed at, 1 being its natural size."),
    layer_field!("MaxScale", "max_scale", Float, "Scatter", "", "Largest size an instance is placed at."),
    layer_field!("AlignToNormal", "align_to_normal", Bool, "Scatter", "",
        "Tilt each instance with the slope under it. Off stands it upright."),
    layer_field!("Collide", "collide", Bool, "Scatter", "",
        "Give trees a trunk collider and large rocks a round one. Grass, shrubs, small rocks and custom meshes never collide."),
    layer_field!("Seed", "seed", Int, "Scatter", "", "Pattern seed. The same seed and properties place the same instances."),
    layer_field!("Material", "material", Choice(MATERIALS), "Rules", "",
        "Place only where the ground shows this material, thinning out where it blends into another. None places on any material."),
    layer_field!("MinSlope", "min_slope", Float, "Rules", "deg", "Shallowest slope placed on, in degrees from horizontal."),
    layer_field!("MaxSlope", "max_slope", Float, "Rules", "deg", "Steepest slope placed on, in degrees from horizontal."),
    layer_field!("MinHeight", "min_height", Float, "Rules", "m", "Lowest ground placed on."),
    layer_field!("MaxHeight", "max_height", Float, "Rules", "m", "Highest ground placed on."),
    layer_field!("AvoidRoads", "avoid_roads", Bool, "Rules", "", "Keep off the beds and shoulders of Road and Path splines."),
    layer_field!("SizeX", "size_x", Float, "Footprint", "m",
        "Footprint's extent along the layer's X axis. 0 covers the whole terrain along it."),
    layer_field!("SizeZ", "size_z", Float, "Footprint", "m",
        "Footprint's extent along the layer's Z axis. 0 covers the whole terrain along it."),
    layer_field!("Radius", "radius", Float, "Streaming", "m",
        "How far from the view instances are drawn. 0 uses the Kind's distance: grass 120 m, shrubs 250 m, rocks 450 m, trees and custom meshes 1500 m."),
];

/// Every property of `TerrainWaterBody`, in panel order. Enabled and Order
/// mean something else for a water body than for a baked layer, so they are
/// its own rows rather than the shared `ENABLED` and `ORDER`.
pub const WATER_BODY_FIELDS: &[FieldSpec] = &[
    layer_field!("Enabled", "enabled", Bool, "Layer", "", "Fill this water body. Off removes its water."),
    layer_field!("Order", "order", Int, "Layer", "",
        "Water bodies fill in ascending Order. The same Order fills in a fixed order of its own."),
    layer_field!("Level", "level", Float, "Water", "m",
        "Height of the water surface above the body's Position. The ground below it that joins the ground under the Position is filled; higher ground holds the water back. Nothing collides with the water."),
    layer_field!("SizeX", "size_x", Float, "Footprint", "m",
        "Footprint's extent along the body's X axis; the water stops at its edge. 0 lets it spread across the whole terrain along it."),
    layer_field!("SizeZ", "size_z", Float, "Footprint", "m",
        "Footprint's extent along the body's Z axis; the water stops at its edge. 0 lets it spread across the whole terrain along it."),
];

// ============================================================================
// Components
// ============================================================================

/// A road, path, river, canyon or embankment along a spline through its
/// `TerrainSplinePoint` children (class `TerrainSpline`).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TerrainSpline {
    pub enabled: bool,
    pub order: i64,
    pub mode: SplineMode,
    pub width: f64,
    pub shoulder_width: f64,
    pub depth: f64,
    pub smoothing: f64,
    pub bed_material: Option<TerrainMaterial>,
    pub shoulder_material: Option<TerrainMaterial>,
    /// River only: a water surface along the channel. Bakes nothing.
    pub water_surface: bool,
    /// River only: how full the channel is, 0 to 1 of `depth`. Bakes
    /// nothing.
    pub water_fill: f64,
}

impl Default for TerrainSpline {
    /// An 8 m asphalt road with 4 m dirt shoulders. Water is on and the
    /// channel 60 % full, so switching the mode to River gives a river that
    /// holds water.
    fn default() -> Self {
        Self {
            enabled: true,
            order: 0,
            mode: SplineMode::Road,
            width: 8.0,
            shoulder_width: 4.0,
            depth: 3.0,
            smoothing: 0.5,
            bed_material: Some(TerrainMaterial::Asphalt),
            shoulder_material: Some(TerrainMaterial::Dirt),
            water_surface: true,
            water_fill: 0.6,
        }
    }
}

/// One control point of a `TerrainSpline` (class `TerrainSplinePoint`). Its
/// `Transform` places it.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct TerrainSplinePoint {
    pub index: f64,
}

/// An analytic crater, mound, plateau or ridge pressed into the ground at its
/// `Transform` (class `TerrainStamp`).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TerrainStamp {
    pub enabled: bool,
    pub order: i64,
    pub shape: StampShape,
    pub radius: f64,
    pub height: f64,
    pub falloff: f64,
    pub blend: StampBlend,
    pub strength: f64,
}

impl Default for TerrainStamp {
    /// A 20 m mound 6 m tall, added to the ground.
    fn default() -> Self {
        Self {
            enabled: true,
            order: 0,
            shape: StampShape::Mound,
            radius: 20.0,
            height: 6.0,
            falloff: 8.0,
            blend: StampBlend::Add,
            strength: 1.0,
        }
    }
}

/// A flat pad at its `Transform`'s height, blending back into the ground
/// (class `TerrainFlattenPad`).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TerrainFlattenPad {
    pub enabled: bool,
    pub order: i64,
    pub size_x: f64,
    pub size_z: f64,
    pub falloff: f64,
    pub material: Option<TerrainMaterial>,
}

impl Default for TerrainFlattenPad {
    /// A 20 m square pad blending out over 10 m, painting nothing.
    fn default() -> Self {
        Self { enabled: true, order: 0, size_x: 20.0, size_z: 20.0, falloff: 10.0, material: None }
    }
}

/// Deterministic fractal noise over a rectangle (class `TerrainNoise`).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TerrainNoise {
    pub enabled: bool,
    pub order: i64,
    pub size_x: f64,
    pub size_z: f64,
    pub amplitude: f64,
    pub frequency: f64,
    pub octaves: i64,
    pub seed: i64,
    pub falloff: f64,
}

impl Default for TerrainNoise {
    /// 3 m of four-octave noise over 100 m, fading in over 20 m.
    fn default() -> Self {
        Self {
            enabled: true,
            order: 0,
            size_x: 100.0,
            size_z: 100.0,
            amplitude: 3.0,
            frequency: 0.02,
            octaves: 4,
            seed: 1,
            falloff: 20.0,
        }
    }
}

/// A material painted over a rectangle by slope and height rules (class
/// `TerrainMaterialFill`).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TerrainMaterialFill {
    pub enabled: bool,
    pub order: i64,
    pub size_x: f64,
    pub size_z: f64,
    pub material: Option<TerrainMaterial>,
    pub min_slope: f64,
    pub max_slope: f64,
    pub min_height: f64,
    pub max_height: f64,
}

impl Default for TerrainMaterialFill {
    /// Rock on every slope of 30 degrees or more over 100 m, at any height.
    fn default() -> Self {
        Self {
            enabled: true,
            order: 0,
            size_x: 100.0,
            size_z: 100.0,
            material: Some(TerrainMaterial::Rock),
            min_slope: 30.0,
            max_slope: 90.0,
            min_height: -10_000.0,
            max_height: 10_000.0,
        }
    }
}

/// Grass, shrubs, rocks, trees or a custom mesh scattered over the finished
/// ground by rules (class `TerrainScatter`). Bakes nothing into the terrain:
/// `scatter` places it near the view, and nothing it places is stored.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TerrainScatter {
    pub enabled: bool,
    pub order: i64,
    pub kind: ScatterKind,
    pub tree_type: TreeType,
    /// Custom only: the mesh asset, a path relative to the Space folder.
    pub mesh_asset: String,
    /// Instances per 100 square metres where every rule holds.
    pub density: f64,
    pub min_scale: f64,
    pub max_scale: f64,
    pub align_to_normal: bool,
    pub collide: bool,
    pub seed: i64,
    /// The material the ground must show, `None` for any.
    pub material: Option<TerrainMaterial>,
    pub min_slope: f64,
    pub max_slope: f64,
    pub min_height: f64,
    pub max_height: f64,
    pub avoid_roads: bool,
    /// Footprint extents; 0 covers the whole terrain along that axis.
    pub size_x: f64,
    pub size_z: f64,
    /// Streaming distance; 0 uses the kind's.
    pub radius: f64,
}

impl Default for TerrainScatter {
    /// Meadow grass over the whole terrain on slopes up to 35 degrees, off
    /// the roads, 40 tufts per 100 square metres.
    fn default() -> Self {
        Self {
            enabled: true,
            order: 0,
            kind: ScatterKind::Grass,
            tree_type: TreeType::Mixed,
            mesh_asset: String::new(),
            density: ScatterKind::Grass.default_density(),
            min_scale: 0.8,
            max_scale: 1.3,
            align_to_normal: true,
            collide: false,
            seed: 1,
            material: None,
            min_slope: 0.0,
            max_slope: 35.0,
            min_height: -10_000.0,
            max_height: 10_000.0,
            avoid_roads: true,
            size_x: 0.0,
            size_z: 0.0,
            radius: 0.0,
        }
    }
}

/// Water filling the ground below a level around its `Transform` (class
/// `TerrainWaterBody`): a lake or pond. Bakes nothing into the terrain:
/// `water_bodies` floods the finished ground from its position and draws
/// the water, and nothing it fills is stored.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TerrainWaterBody {
    pub enabled: bool,
    pub order: i64,
    /// Height of the water surface above the body's position, metres.
    /// Relative, so a body Insert drops on the ground under the view holds
    /// water at once, and moving it up or down raises or lowers the lake.
    pub level: f64,
    /// Footprint extents; 0 leaves that axis unlimited.
    pub size_x: f64,
    pub size_z: f64,
}

impl Default for TerrainWaterBody {
    /// Water 2 m above the body's position, within a 100 m square.
    fn default() -> Self {
        Self { enabled: true, order: 0, level: 2.0, size_x: 100.0, size_z: 100.0 }
    }
}

// ============================================================================
// Get / set
// ============================================================================

fn as_bool(v: &FieldValue) -> Result<bool, String> {
    match v {
        FieldValue::Bool(b) => Ok(*b),
        FieldValue::Int(i) => Ok(*i != 0),
        other => Err(format!("expected a boolean, got {other:?}")),
    }
}

fn as_f64(v: &FieldValue) -> Result<f64, String> {
    let f = match v {
        FieldValue::Float(f) => *f,
        FieldValue::Int(i) => *i as f64,
        other => return Err(format!("expected a number, got {other:?}")),
    };
    if f.is_finite() { Ok(f) } else { Err("value is not finite".into()) }
}

fn as_i64(v: &FieldValue) -> Result<i64, String> {
    match v {
        FieldValue::Int(i) => Ok(*i),
        FieldValue::Float(f) if f.is_finite() && f.fract() == 0.0 => Ok(*f as i64),
        other => Err(format!("expected an integer, got {other:?}")),
    }
}

fn as_choice(v: &FieldValue) -> Result<&str, String> {
    match v {
        FieldValue::Choice(s) => Ok(s),
        other => Err(format!("expected a name, got {other:?}")),
    }
}

fn as_text(v: &FieldValue) -> Result<&str, String> {
    match v {
        FieldValue::Text(s) | FieldValue::Choice(s) => Ok(s),
        other => Err(format!("expected text, got {other:?}")),
    }
}

/// An `Order` a layer can apply in: the bake orders by `i32`.
fn clamp_order(order: i64) -> i64 {
    order.clamp(i64::from(i32::MIN), i64::from(i32::MAX))
}

fn material_name(material: Option<TerrainMaterial>) -> &'static str {
    material.map_or("None", |m| m.name())
}

fn parse_material(v: &FieldValue) -> Result<Option<TerrainMaterial>, String> {
    let name = as_choice(v)?;
    if name.trim().eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    TerrainMaterial::from_name(name).map(Some).ok_or_else(|| format!("unknown terrain material {name}"))
}

fn no_property(class: &str, name: &str) -> String {
    format!("{class} has no property {name}")
}

impl TerrainSpline {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(SPLINE_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "order" => FieldValue::Int(self.order),
            "mode" => FieldValue::Choice(self.mode.name().into()),
            "width" => FieldValue::Float(self.width),
            "shoulder_width" => FieldValue::Float(self.shoulder_width),
            "depth" => FieldValue::Float(self.depth),
            "smoothing" => FieldValue::Float(self.smoothing),
            "bed_material" => FieldValue::Choice(material_name(self.bed_material).into()),
            "shoulder_material" => FieldValue::Choice(material_name(self.shoulder_material).into()),
            "water_surface" => FieldValue::Bool(self.water_surface),
            "water_fill" => FieldValue::Float(self.water_fill),
            _ => return None,
        })
    }

    /// Set one property, clamping it to a range the bake can use.
    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(SPLINE_FIELDS, name).ok_or_else(|| no_property("TerrainSpline", name))?;
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "order" => self.order = clamp_order(as_i64(&v)?),
            "mode" => {
                let s = as_choice(&v)?;
                self.mode = SplineMode::from_name(s).ok_or_else(|| format!("unknown spline mode {s}"))?;
            }
            "width" => self.width = as_f64(&v)?.max(0.0),
            "shoulder_width" => self.shoulder_width = as_f64(&v)?.max(0.0),
            // A magnitude: the mode decides which way it goes.
            "depth" => self.depth = as_f64(&v)?.abs(),
            "smoothing" => self.smoothing = as_f64(&v)?.clamp(0.0, 1.0),
            "bed_material" => self.bed_material = parse_material(&v)?,
            "shoulder_material" => self.shoulder_material = parse_material(&v)?,
            "water_surface" => self.water_surface = as_bool(&v)?,
            "water_fill" => self.water_fill = as_f64(&v)?.clamp(0.0, 1.0),
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// The spline layer through `points`, world positions in path order.
    /// WaterSurface and WaterFill are left out: they shape no ground.
    pub fn layer(&self, points: Vec<Vec3>) -> SplineLayer {
        SplineLayer {
            mode: self.mode,
            points,
            width: self.width as f32,
            shoulder_width: self.shoulder_width as f32,
            depth: self.depth as f32,
            smoothing: self.smoothing as f32,
            bed_material: self.bed_material.map(TerrainMaterial::to_u8),
            shoulder_material: self.shoulder_material.map(TerrainMaterial::to_u8),
        }
    }
}

impl TerrainSplinePoint {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(POINT_FIELDS, name)?;
        Some(match f.key {
            "index" => FieldValue::Float(self.index),
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(POINT_FIELDS, name).ok_or_else(|| no_property("TerrainSplinePoint", name))?;
        match f.key {
            "index" => self.index = as_f64(&v)?,
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }
}

impl TerrainStamp {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(STAMP_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "order" => FieldValue::Int(self.order),
            "shape" => FieldValue::Choice(self.shape.name().into()),
            "radius" => FieldValue::Float(self.radius),
            "height" => FieldValue::Float(self.height),
            "falloff" => FieldValue::Float(self.falloff),
            "blend" => FieldValue::Choice(self.blend.name().into()),
            "strength" => FieldValue::Float(self.strength),
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(STAMP_FIELDS, name).ok_or_else(|| no_property("TerrainStamp", name))?;
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "order" => self.order = clamp_order(as_i64(&v)?),
            "shape" => {
                let s = as_choice(&v)?;
                self.shape = StampShape::from_name(s).ok_or_else(|| format!("unknown stamp shape {s}"))?;
            }
            "radius" => self.radius = as_f64(&v)?.max(0.0),
            "height" => self.height = as_f64(&v)?,
            "falloff" => self.falloff = as_f64(&v)?.max(0.0),
            "blend" => {
                let s = as_choice(&v)?;
                self.blend = StampBlend::from_name(s).ok_or_else(|| format!("unknown stamp blend {s}"))?;
            }
            "strength" => self.strength = as_f64(&v)?.clamp(0.0, 1.0),
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// The stamp layer at world pose `pose`.
    pub fn layer(&self, pose: &Transform) -> StampLayer {
        StampLayer {
            center: pose.translation,
            yaw: yaw_of(pose.rotation),
            shape: self.shape,
            radius: self.radius as f32,
            height: self.height as f32,
            falloff: self.falloff as f32,
            blend: self.blend,
            strength: self.strength as f32,
        }
    }
}

impl TerrainFlattenPad {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(FLATTEN_PAD_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "order" => FieldValue::Int(self.order),
            "size_x" => FieldValue::Float(self.size_x),
            "size_z" => FieldValue::Float(self.size_z),
            "falloff" => FieldValue::Float(self.falloff),
            "material" => FieldValue::Choice(material_name(self.material).into()),
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(FLATTEN_PAD_FIELDS, name).ok_or_else(|| no_property("TerrainFlattenPad", name))?;
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "order" => self.order = clamp_order(as_i64(&v)?),
            "size_x" => self.size_x = as_f64(&v)?.max(0.0),
            "size_z" => self.size_z = as_f64(&v)?.max(0.0),
            "falloff" => self.falloff = as_f64(&v)?.max(0.0),
            "material" => self.material = parse_material(&v)?,
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// The pad layer at world pose `pose`: its top at the pose's height.
    pub fn layer(&self, pose: &Transform) -> FlattenPadLayer {
        FlattenPadLayer {
            center: pose.translation,
            yaw: yaw_of(pose.rotation),
            size: Vec2::new(self.size_x as f32, self.size_z as f32),
            falloff: self.falloff as f32,
            material: self.material.map(TerrainMaterial::to_u8),
        }
    }
}

impl TerrainNoise {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(NOISE_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "order" => FieldValue::Int(self.order),
            "size_x" => FieldValue::Float(self.size_x),
            "size_z" => FieldValue::Float(self.size_z),
            "amplitude" => FieldValue::Float(self.amplitude),
            "frequency" => FieldValue::Float(self.frequency),
            "octaves" => FieldValue::Int(self.octaves),
            "seed" => FieldValue::Int(self.seed),
            "falloff" => FieldValue::Float(self.falloff),
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(NOISE_FIELDS, name).ok_or_else(|| no_property("TerrainNoise", name))?;
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "order" => self.order = clamp_order(as_i64(&v)?),
            "size_x" => self.size_x = as_f64(&v)?.max(0.0),
            "size_z" => self.size_z = as_f64(&v)?.max(0.0),
            "amplitude" => self.amplitude = as_f64(&v)?,
            "frequency" => self.frequency = as_f64(&v)?.max(0.0),
            "octaves" => self.octaves = as_i64(&v)?.clamp(1, 12),
            // The noise hashes a 32-bit seed.
            "seed" => self.seed = as_i64(&v)?.clamp(0, i64::from(u32::MAX)),
            "falloff" => self.falloff = as_f64(&v)?.max(0.0),
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// The noise layer at world pose `pose`; the pattern moves and turns with it.
    pub fn layer(&self, pose: &Transform) -> NoiseLayer {
        NoiseLayer {
            center: pose.translation,
            yaw: yaw_of(pose.rotation),
            size: Vec2::new(self.size_x as f32, self.size_z as f32),
            amplitude: self.amplitude as f32,
            frequency: self.frequency as f32,
            octaves: self.octaves.clamp(1, 12) as u32,
            seed: self.seed.clamp(0, i64::from(u32::MAX)) as u32,
            falloff: self.falloff as f32,
        }
    }
}

impl TerrainMaterialFill {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(MATERIAL_FILL_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "order" => FieldValue::Int(self.order),
            "size_x" => FieldValue::Float(self.size_x),
            "size_z" => FieldValue::Float(self.size_z),
            "material" => FieldValue::Choice(material_name(self.material).into()),
            "min_slope" => FieldValue::Float(self.min_slope),
            "max_slope" => FieldValue::Float(self.max_slope),
            "min_height" => FieldValue::Float(self.min_height),
            "max_height" => FieldValue::Float(self.max_height),
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(MATERIAL_FILL_FIELDS, name).ok_or_else(|| no_property("TerrainMaterialFill", name))?;
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "order" => self.order = clamp_order(as_i64(&v)?),
            "size_x" => self.size_x = as_f64(&v)?.max(0.0),
            "size_z" => self.size_z = as_f64(&v)?.max(0.0),
            "material" => self.material = parse_material(&v)?,
            "min_slope" => self.min_slope = as_f64(&v)?.clamp(0.0, 90.0),
            "max_slope" => self.max_slope = as_f64(&v)?.clamp(0.0, 90.0),
            "min_height" => self.min_height = as_f64(&v)?,
            "max_height" => self.max_height = as_f64(&v)?,
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// The fill layer at world pose `pose`.
    pub fn layer(&self, pose: &Transform) -> MaterialFillLayer {
        MaterialFillLayer {
            center: pose.translation,
            yaw: yaw_of(pose.rotation),
            size: Vec2::new(self.size_x as f32, self.size_z as f32),
            material: self.material.map_or(MATERIAL_SLOT_NONE, TerrainMaterial::to_u8),
            min_slope: self.min_slope as f32,
            max_slope: self.max_slope as f32,
            min_height: self.min_height as f32,
            max_height: self.max_height as f32,
        }
    }
}

impl TerrainScatter {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(SCATTER_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "order" => FieldValue::Int(self.order),
            "kind" => FieldValue::Choice(self.kind.name().into()),
            "tree_type" => FieldValue::Choice(self.tree_type.name().into()),
            "mesh_asset" => FieldValue::Text(self.mesh_asset.clone()),
            "density" => FieldValue::Float(self.density),
            "min_scale" => FieldValue::Float(self.min_scale),
            "max_scale" => FieldValue::Float(self.max_scale),
            "align_to_normal" => FieldValue::Bool(self.align_to_normal),
            "collide" => FieldValue::Bool(self.collide),
            "seed" => FieldValue::Int(self.seed),
            "material" => FieldValue::Choice(material_name(self.material).into()),
            "min_slope" => FieldValue::Float(self.min_slope),
            "max_slope" => FieldValue::Float(self.max_slope),
            "min_height" => FieldValue::Float(self.min_height),
            "max_height" => FieldValue::Float(self.max_height),
            "avoid_roads" => FieldValue::Bool(self.avoid_roads),
            "size_x" => FieldValue::Float(self.size_x),
            "size_z" => FieldValue::Float(self.size_z),
            "radius" => FieldValue::Float(self.radius),
            _ => return None,
        })
    }

    /// Set one property, clamping it to a range placement can use.
    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(SCATTER_FIELDS, name).ok_or_else(|| no_property("TerrainScatter", name))?;
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "order" => self.order = clamp_order(as_i64(&v)?),
            "kind" => {
                let s = as_choice(&v)?;
                let kind = ScatterKind::from_name(s).ok_or_else(|| format!("unknown scatter kind {s}"))?;
                // A density still at the old kind's default follows the new
                // kind, so a new Grass layer switched to Trees gives a forest,
                // not a tree every 2.5 square metres. Kind comes before
                // Density in SCATTER_FIELDS, so a saved density loads over
                // this.
                if self.density == self.kind.default_density() {
                    self.density = kind.default_density();
                }
                self.kind = kind;
            }
            "tree_type" => {
                let s = as_choice(&v)?;
                self.tree_type = TreeType::from_name(s).ok_or_else(|| format!("unknown tree type {s}"))?;
            }
            "mesh_asset" => self.mesh_asset = as_text(&v)?.trim().to_string(),
            "density" => self.density = as_f64(&v)?.clamp(0.0, MAX_DENSITY),
            "min_scale" => self.min_scale = as_f64(&v)?.clamp(MIN_SCALE, MAX_SCALE),
            "max_scale" => self.max_scale = as_f64(&v)?.clamp(MIN_SCALE, MAX_SCALE),
            "align_to_normal" => self.align_to_normal = as_bool(&v)?,
            "collide" => self.collide = as_bool(&v)?,
            // Hashed as 32 bits, like a noise layer's seed.
            "seed" => self.seed = as_i64(&v)?.clamp(0, i64::from(u32::MAX)),
            "material" => self.material = parse_material(&v)?,
            "min_slope" => self.min_slope = as_f64(&v)?.clamp(0.0, 90.0),
            "max_slope" => self.max_slope = as_f64(&v)?.clamp(0.0, 90.0),
            "min_height" => self.min_height = as_f64(&v)?,
            "max_height" => self.max_height = as_f64(&v)?,
            "avoid_roads" => self.avoid_roads = as_bool(&v)?,
            "size_x" => self.size_x = as_f64(&v)?.max(0.0),
            "size_z" => self.size_z = as_f64(&v)?.max(0.0),
            "radius" => self.radius = as_f64(&v)?.clamp(0.0, MAX_RADIUS),
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// The scatter layer `id` at world pose `pose`. Only a footprint follows
    /// the pose, so moving a layer that covers the whole terrain changes
    /// nothing it places.
    pub fn layer(&self, id: u64, pose: &Transform) -> ScatterLayer {
        let footprint = footprint_of(self.size_x, self.size_z, pose);
        ScatterLayer {
            id,
            order: order_of(self.order),
            kind: self.kind,
            tree_type: self.tree_type,
            mesh_asset: self.mesh_asset.clone(),
            density: self.density.clamp(0.0, MAX_DENSITY) as f32,
            min_scale: self.min_scale.clamp(MIN_SCALE, MAX_SCALE) as f32,
            max_scale: self.max_scale.clamp(MIN_SCALE, MAX_SCALE) as f32,
            material: self.material.map(TerrainMaterial::to_u8),
            min_slope: self.min_slope as f32,
            max_slope: self.max_slope as f32,
            min_height: self.min_height as f32,
            max_height: self.max_height as f32,
            align_to_normal: self.align_to_normal,
            avoid_roads: self.avoid_roads,
            collide: self.collide,
            seed: self.seed.clamp(0, i64::from(u32::MAX)) as u64,
            footprint,
            radius: if self.radius > 0.0 { self.radius.min(MAX_RADIUS) as f32 } else { self.kind.default_radius() },
        }
    }
}

impl TerrainWaterBody {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field_spec(WATER_BODY_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "order" => FieldValue::Int(self.order),
            "level" => FieldValue::Float(self.level),
            "size_x" => FieldValue::Float(self.size_x),
            "size_z" => FieldValue::Float(self.size_z),
            _ => return None,
        })
    }

    /// Set one property, clamping it to a range the flood can use.
    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field_spec(WATER_BODY_FIELDS, name).ok_or_else(|| no_property("TerrainWaterBody", name))?;
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "order" => self.order = clamp_order(as_i64(&v)?),
            // Signed: water may stand below the position too.
            "level" => self.level = as_f64(&v)?,
            "size_x" => self.size_x = as_f64(&v)?.max(0.0),
            "size_z" => self.size_z = as_f64(&v)?.max(0.0),
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// The water body `id` at world pose `pose`: it floods from the pose's
    /// XZ up to `Level` above its Y, inside a footprint that moves and turns
    /// with it.
    pub fn body(&self, id: u64, pose: &Transform) -> WaterBodyDesc {
        let center = Vec2::new(pose.translation.x, pose.translation.z);
        let footprint = footprint_of(self.size_x, self.size_z, pose);
        WaterBodyDesc {
            id,
            order: order_of(self.order),
            seed: center,
            level: pose.translation.y + self.level as f32,
            footprint,
        }
    }
}

macro_rules! impl_field_table {
    ($ty:ty, $fields:expr, $section:expr) => {
        impl FieldTable for $ty {
            const FIELDS: &'static [FieldSpec] = $fields;
            const SECTION: &'static str = $section;
            fn get_field(&self, name: &str) -> Option<FieldValue> {
                self.get(name)
            }
            fn set_field(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
                self.set(name, v)
            }
        }
    };
}

impl_field_table!(TerrainSpline, SPLINE_FIELDS, SPLINE_SECTION);
impl_field_table!(TerrainSplinePoint, POINT_FIELDS, POINT_SECTION);
impl_field_table!(TerrainStamp, STAMP_FIELDS, STAMP_SECTION);
impl_field_table!(TerrainFlattenPad, FLATTEN_PAD_FIELDS, FLATTEN_PAD_SECTION);
impl_field_table!(TerrainNoise, NOISE_FIELDS, NOISE_SECTION);
impl_field_table!(TerrainMaterialFill, MATERIAL_FILL_FIELDS, MATERIAL_FILL_SECTION);
impl_field_table!(TerrainScatter, SCATTER_FIELDS, SCATTER_SECTION);
impl_field_table!(TerrainWaterBody, WATER_BODY_FIELDS, WATER_BODY_SECTION);

// ============================================================================
// Class <-> component
// ============================================================================

/// The TOML section a layer class keeps its properties in.
pub fn section_name(class_name: ClassName) -> Option<&'static str> {
    Some(match class_name {
        ClassName::TerrainSpline => SPLINE_SECTION,
        ClassName::TerrainSplinePoint => POINT_SECTION,
        ClassName::TerrainStamp => STAMP_SECTION,
        ClassName::TerrainFlattenPad => FLATTEN_PAD_SECTION,
        ClassName::TerrainNoise => NOISE_SECTION,
        ClassName::TerrainMaterialFill => MATERIAL_FILL_SECTION,
        ClassName::TerrainScatter => SCATTER_SECTION,
        ClassName::TerrainWaterBody => WATER_BODY_SECTION,
        _ => return None,
    })
}

/// The component of one terrain layer class.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerComponent {
    Spline(TerrainSpline),
    Point(TerrainSplinePoint),
    Stamp(TerrainStamp),
    FlattenPad(TerrainFlattenPad),
    Noise(TerrainNoise),
    MaterialFill(TerrainMaterialFill),
    Scatter(TerrainScatter),
    WaterBody(TerrainWaterBody),
}

impl LayerComponent {
    /// The component of layer class `class_name`, read from its instance's
    /// section (`section(name)` is the instance's table called `name`), and
    /// the keys that could not be read and kept their defaults. `None` for
    /// any other class.
    pub fn from_sections<'a>(
        class_name: ClassName,
        section: impl Fn(&str) -> Option<&'a toml::value::Table>,
    ) -> Option<(Self, Vec<String>)> {
        Some(match class_name {
            ClassName::TerrainSpline => {
                let (c, problems) = from_section::<TerrainSpline>(section(SPLINE_SECTION));
                (Self::Spline(c), problems)
            }
            ClassName::TerrainSplinePoint => {
                let (c, problems) = from_section::<TerrainSplinePoint>(section(POINT_SECTION));
                (Self::Point(c), problems)
            }
            ClassName::TerrainStamp => {
                let (c, problems) = from_section::<TerrainStamp>(section(STAMP_SECTION));
                (Self::Stamp(c), problems)
            }
            ClassName::TerrainFlattenPad => {
                let (c, problems) = from_section::<TerrainFlattenPad>(section(FLATTEN_PAD_SECTION));
                (Self::FlattenPad(c), problems)
            }
            ClassName::TerrainNoise => {
                let (c, problems) = from_section::<TerrainNoise>(section(NOISE_SECTION));
                (Self::Noise(c), problems)
            }
            ClassName::TerrainMaterialFill => {
                let (c, problems) = from_section::<TerrainMaterialFill>(section(MATERIAL_FILL_SECTION));
                (Self::MaterialFill(c), problems)
            }
            ClassName::TerrainScatter => {
                let (c, problems) = from_section::<TerrainScatter>(section(SCATTER_SECTION));
                (Self::Scatter(c), problems)
            }
            ClassName::TerrainWaterBody => {
                let (c, problems) = from_section::<TerrainWaterBody>(section(WATER_BODY_SECTION));
                (Self::WaterBody(c), problems)
            }
            _ => return None,
        })
    }

    /// Insert the component on the entity `entity` builds.
    pub fn insert_into(self, entity: &mut EntityCommands) {
        match self {
            Self::Spline(c) => {
                entity.insert(c);
            }
            Self::Point(c) => {
                entity.insert(c);
            }
            Self::Stamp(c) => {
                entity.insert(c);
            }
            Self::FlattenPad(c) => {
                entity.insert(c);
            }
            Self::Noise(c) => {
                entity.insert(c);
            }
            Self::MaterialFill(c) => {
                entity.insert(c);
            }
            Self::Scatter(c) => {
                entity.insert(c);
            }
            Self::WaterBody(c) => {
                entity.insert(c);
            }
        }
    }

    /// Replace the entity's component of the same class with this one, only
    /// when they differ: a file re-read after the engine wrote it itself must
    /// not look like an edit and re-bake.
    pub fn replace_in(self, entity: &mut EntityWorldMut) {
        fn replace<T: Component<Mutability = Mutable> + PartialEq>(entity: &mut EntityWorldMut, fresh: T) {
            if let Some(mut current) = entity.get_mut::<T>() {
                if *current != fresh {
                    *current = fresh;
                }
            }
        }
        match self {
            Self::Spline(c) => replace(entity, c),
            Self::Point(c) => replace(entity, c),
            Self::Stamp(c) => replace(entity, c),
            Self::FlattenPad(c) => replace(entity, c),
            Self::Noise(c) => replace(entity, c),
            Self::MaterialFill(c) => replace(entity, c),
            Self::Scatter(c) => replace(entity, c),
            Self::WaterBody(c) => replace(entity, c),
        }
    }
}

// ============================================================================
// Mapping onto the bake
// ============================================================================

/// Heading of `rotation` about +Y, as `Quat::from_rotation_y(yaw)` names it:
/// where it turns local +X, flattened onto the ground. A tilted layer still
/// turns by its heading; one whose X axis points straight up or down falls
/// back to the Euler yaw.
pub fn yaw_of(rotation: Quat) -> f32 {
    let x = rotation * Vec3::X;
    if x.x.abs() + x.z.abs() > 1e-6 {
        (-x.z).atan2(x.x)
    } else {
        rotation.to_euler(EulerRot::YXZ).0
    }
}

/// The footprint a layer sized `size_x` by `size_z` covers at world pose
/// `pose`, turning with its heading. A zero extent leaves that axis
/// unlimited, and no size at all covers the whole terrain (`None`).
fn footprint_of(size_x: f64, size_z: f64, pose: &Transform) -> Option<ScatterFootprint> {
    let half = |size: f64| if size > 0.0 { (size * 0.5) as f32 } else { f32::INFINITY };
    (size_x > 0.0 || size_z > 0.0).then(|| ScatterFootprint {
        center: Vec2::new(pose.translation.x, pose.translation.z),
        yaw: yaw_of(pose.rotation),
        half: Vec2::new(half(size_x), half(size_z)),
    })
}

/// A layer's stable id: the first 16 hex digits of its instance uuid, so the
/// apply order of layers with the same `Order` survives a reload, or the
/// entity when it has no uuid.
pub fn layer_id(uuid: Option<&str>, entity: Entity) -> u64 {
    uuid.and_then(|u| u.get(..16))
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .unwrap_or_else(|| entity.to_bits())
}

fn order_of(order: i64) -> i32 {
    clamp_order(order) as i32
}

/// A spline point as the ordering needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct PointKey {
    pub index: f64,
    pub name: String,
    pub id: u64,
    /// World position.
    pub position: Vec3,
}

/// Control point positions in path order: ascending `Index`, then name, then
/// id, so two points with the same index still join the same way each time.
pub fn ordered_points(mut points: Vec<PointKey>) -> Vec<Vec3> {
    points.sort_by(|a, b| a.index.total_cmp(&b.index).then_with(|| a.name.cmp(&b.name)).then(a.id.cmp(&b.id)));
    points.into_iter().map(|p| p.position).collect()
}

/// Read access to every layer instance, for [`LayerQueries::gather`] and
/// [`LayerQueries::spline`].
#[derive(SystemParam)]
pub struct LayerQueries<'w, 's> {
    splines: Query<'w, 's, (Entity, Option<&'static Instance>, &'static TerrainSpline, Option<&'static Children>)>,
    points: Query<'w, 's, (Option<&'static Instance>, &'static TerrainSplinePoint)>,
    stamps: Query<'w, 's, (Entity, Option<&'static Instance>, &'static TerrainStamp)>,
    pads: Query<'w, 's, (Entity, Option<&'static Instance>, &'static TerrainFlattenPad)>,
    noises: Query<'w, 's, (Entity, Option<&'static Instance>, &'static TerrainNoise)>,
    fills: Query<'w, 's, (Entity, Option<&'static Instance>, &'static TerrainMaterialFill)>,
    transforms: Query<'w, 's, &'static Transform>,
    parents: Query<'w, 's, &'static ChildOf>,
}

impl LayerQueries<'_, '_> {
    /// World pose of `entity`: its `Transform` under its ancestors'. Composed
    /// here rather than read from `GlobalTransform`, which only catches up in
    /// `PostUpdate`: a pose read a frame late would bake the layer where it
    /// was. An ancestor without a `Transform` (a service root) counts as the
    /// identity.
    pub fn world_pose(&self, entity: Entity) -> Transform {
        compose_world_pose(
            entity,
            |e| self.transforms.get(e).ok().copied(),
            |e| self.parents.get(e).ok().map(ChildOf::parent),
        )
    }

    /// World positions of a spline's `TerrainSplinePoint` children, in path
    /// order.
    fn spline_points(&self, children: Option<&Children>) -> Vec<Vec3> {
        let points: Vec<PointKey> = children
            .map(|children| {
                children
                    .iter()
                    .filter_map(|child| {
                        let (point_instance, point) = self.points.get(child).ok()?;
                        Some(PointKey {
                            index: point.index,
                            name: point_instance.map(|i| i.name.clone()).unwrap_or_default(),
                            id: layer_id(point_instance.map(|i| i.uuid.as_str()), child),
                            position: self.world_pose(child).translation,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        ordered_points(points)
    }

    /// Spline `entity` as the bake takes it, enabled or not: its layer id,
    /// its `Enabled` flag, and its layer through its control points' world
    /// positions in path order. `None` when `entity` is not a spline. The
    /// Studio draws a selected spline from this, so its gizmo joins the
    /// points exactly as the bake does.
    pub fn spline(&self, entity: Entity) -> Option<(u64, bool, SplineLayer)> {
        let (entity, instance, spline, children) = self.splines.get(entity).ok()?;
        let id = layer_id(instance.map(|i| i.uuid.as_str()), entity);
        Some((id, spline.enabled, spline.layer(self.spline_points(children))))
    }

    /// Every enabled layer as a [`LayerDesc`], in apply order.
    pub fn gather(&self) -> Vec<LayerDesc> {
        let id = |instance: Option<&Instance>, entity: Entity| layer_id(instance.map(|i| i.uuid.as_str()), entity);
        let mut layers = Vec::new();
        for (entity, instance, spline, children) in &self.splines {
            if !spline.enabled {
                continue;
            }
            layers.push(LayerDesc {
                id: id(instance, entity),
                order: order_of(spline.order),
                kind: LayerKind::Spline(spline.layer(self.spline_points(children))),
            });
        }
        for (entity, instance, stamp) in &self.stamps {
            if stamp.enabled {
                let kind = LayerKind::Stamp(stamp.layer(&self.world_pose(entity)));
                layers.push(LayerDesc { id: id(instance, entity), order: order_of(stamp.order), kind });
            }
        }
        for (entity, instance, pad) in &self.pads {
            if pad.enabled {
                let kind = LayerKind::FlattenPad(pad.layer(&self.world_pose(entity)));
                layers.push(LayerDesc { id: id(instance, entity), order: order_of(pad.order), kind });
            }
        }
        for (entity, instance, noise) in &self.noises {
            if noise.enabled {
                let kind = LayerKind::Noise(noise.layer(&self.world_pose(entity)));
                layers.push(LayerDesc { id: id(instance, entity), order: order_of(noise.order), kind });
            }
        }
        for (entity, instance, fill) in &self.fills {
            if fill.enabled {
                let kind = LayerKind::MaterialFill(fill.layer(&self.world_pose(entity)));
                layers.push(LayerDesc { id: id(instance, entity), order: order_of(fill.order), kind });
            }
        }
        layers.sort_by_key(|layer| (layer.order, layer.id));
        layers
    }
}

// ============================================================================
// Keeping the bake current
// ============================================================================

type AnyLayer = Or<(
    With<TerrainSpline>,
    With<TerrainSplinePoint>,
    With<TerrainStamp>,
    With<TerrainFlattenPad>,
    With<TerrainNoise>,
    With<TerrainMaterialFill>,
)>;

type LayerEdited = Or<(
    Changed<TerrainSpline>,
    Changed<TerrainSplinePoint>,
    Changed<TerrainStamp>,
    Changed<TerrainFlattenPad>,
    Changed<TerrainNoise>,
    Changed<TerrainMaterialFill>,
    Changed<Transform>,
    Changed<ChildOf>,
    Changed<Children>,
    Changed<Instance>,
)>;

/// Everything that can change the layer stack since the system last ran: a
/// layer added or edited (`Changed` covers `Added`), moved, reparented,
/// renamed (points join by name on equal indices), a point added to or taken
/// from a spline, a layer removed, and a new terrain root to bake onto.
///
/// Two of those leave nothing on the layer's own components: an ancestor
/// that is not a layer moving (a Model holding a stamp) changes the layer's
/// world pose, and a parent taken away altogether (a reparent to the scene
/// root, or its undo) removes `ChildOf` rather than changing it. Both are
/// looked for here too.
#[derive(SystemParam)]
pub struct LayerChanges<'w, 's> {
    edited: Query<'w, 's, (), (AnyLayer, LayerEdited)>,
    new_roots: Query<'w, 's, (), Added<TerrainRoot>>,
    removed_splines: RemovedComponents<'w, 's, TerrainSpline>,
    removed_points: RemovedComponents<'w, 's, TerrainSplinePoint>,
    removed_stamps: RemovedComponents<'w, 's, TerrainStamp>,
    removed_pads: RemovedComponents<'w, 's, TerrainFlattenPad>,
    removed_noises: RemovedComponents<'w, 's, TerrainNoise>,
    removed_fills: RemovedComponents<'w, 's, TerrainMaterialFill>,
    /// The parent of every layer that has one, and the chain above it.
    layer_parents: Query<'w, 's, &'static ChildOf, AnyLayer>,
    parents: Query<'w, 's, &'static ChildOf>,
    moved: Query<'w, 's, (), Changed<Transform>>,
    /// Entities that lost `ChildOf`, of which the layers still alive count.
    unparented: RemovedComponents<'w, 's, ChildOf>,
    layers: Query<'w, 's, (), AnyLayer>,
}

impl LayerChanges<'_, '_> {
    /// Whether anything changed, reading every removal buffer to the end so
    /// the same removal is not seen twice.
    fn take(&mut self) -> bool {
        fn drain<T: Component>(removed: &mut RemovedComponents<'_, '_, T>) -> bool {
            let any = !removed.is_empty();
            removed.clear();
            any
        }
        let mut any = !self.edited.is_empty() || !self.new_roots.is_empty();
        any |= drain(&mut self.removed_splines);
        any |= drain(&mut self.removed_points);
        any |= drain(&mut self.removed_stamps);
        any |= drain(&mut self.removed_pads);
        any |= drain(&mut self.removed_noises);
        any |= drain(&mut self.removed_fills);
        // Every removal is read, not just up to the first layer, so none is
        // seen again next frame. A despawned layer is gone from `layers`;
        // its own component removal above already counted it.
        for entity in self.unparented.read() {
            any |= self.layers.contains(entity);
        }
        any || self.layer_parents.iter().any(|child_of| {
            std::iter::once(child_of.parent())
                .chain(self.parents.iter_ancestors(child_of.parent()))
                .take(MAX_POSE_DEPTH)
                .any(|ancestor| self.moved.contains(ancestor))
        })
    }
}

/// Bookkeeping of [`sync_terrain_layers`].
#[derive(Resource, Debug, Default)]
pub struct TerrainLayerSync {
    /// A change was seen and not yet handed to the bake.
    pending: bool,
    /// `Time<Real>` seconds of the last hand-over, `None` before the first.
    last_push: Option<f64>,
}

/// Hand the layer stack to every terrain root's bake when it changed (see the
/// module docs). Real time paces it, so a paused Play session still bakes
/// edits.
pub fn sync_terrain_layers(
    mut commands: Commands,
    time: Option<Res<Time<Real>>>,
    mut sync: ResMut<TerrainLayerSync>,
    mut dirty: ResMut<TerrainDirtyChunks>,
    mut changes: LayerChanges,
    layers: LayerQueries,
    mut roots: Query<(Entity, &TerrainConfig, &TerrainData, Option<&mut TerrainBaked>), With<TerrainRoot>>,
) {
    if changes.take() {
        sync.pending = true;
    }
    if !sync.pending {
        return;
    }
    let now = time.map(|t| t.elapsed_secs_f64());
    if let (Some(now), Some(last)) = (now, sync.last_push) {
        if now - last < PUSH_INTERVAL_SECS {
            return;
        }
    }
    sync.pending = false;
    sync.last_push = now;

    let stack = layers.gather();
    for (root, config, data, baked) in &mut roots {
        match baked {
            Some(baked) if stack.is_empty() => {
                // Without layers a root carries no bake; the chunks the last
                // layers covered remesh from the base. When the bake had a
                // material layer the base lacks (a layer painted onto a base
                // without one), every chunk was meshed and collided from the
                // bake's all-Grass map, so every chunk goes back to the base.
                // `mark_all` rather than meshes alone: a chunk's friction
                // marker is rescanned only when its collider is rebuilt. The
                // re-bake it queues goes unused once the bake is gone.
                if baked.data.has_material_layer() != data.has_material_layer() {
                    dirty.mark_all(config);
                } else {
                    for (lo, hi) in baked.layers().iter().filter_map(LayerDesc::bounds) {
                        dirty.mark_world_rect(config, lo, hi);
                    }
                }
                commands.entity(root).remove::<TerrainBaked>();
            }
            // Compared first through `Deref`, so an unchanged stack (a
            // rename, a write-back re-read) leaves the bake untouched.
            Some(mut baked) => {
                if baked.layers() != stack.as_slice() {
                    baked.set_layers(stack.clone());
                }
            }
            None if !stack.is_empty() => {
                commands.entity(root).insert(TerrainBaked::new(data, stack.clone()));
            }
            None => {}
        }
    }
}

/// Keeps every terrain root's bake in step with the layer instances, and
/// every Road spline's drivable surface in step with its baked corridor.
/// Added by the shared `TerrainPlugin` (the Client) and by the engine's
/// terrain plugin.
pub struct TerrainLayersPlugin;

impl Plugin for TerrainLayersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainLayerSync>()
            .init_resource::<TerrainDirtyChunks>()
            .init_resource::<RoadSurfaces>()
            // Before the dirty-chunk pass, so a new or replaced stack bakes
            // and remeshes in the frame it was handed over.
            .add_systems(Update, sync_terrain_layers.before(apply_terrain_dirty_chunks))
            // After it, so a road's surface is rebuilt from the stations of
            // the bake that pass just ran, in the same frame as its corridor.
            .add_systems(Update, sync_road_surfaces.after(apply_terrain_dirty_chunks));
    }
}

// ============================================================================
// Reading layer instances without the Studio loader
// ============================================================================

/// A terrain layer instance as its `_instance.toml` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInstanceFile {
    pub name: String,
    pub uuid: String,
    pub class_name: ClassName,
    /// Its `Transform`, under its parent's (a point's under its spline's).
    pub transform: Transform,
    pub component: LayerComponent,
    /// A spline's control points.
    pub points: Vec<LayerInstanceFile>,
}

fn transform_from_toml(table: &toml::Value) -> Transform {
    let floats = |key: &str, n: usize| -> Option<Vec<f32>> {
        let values = table.get(key)?.as_array()?;
        (values.len() == n).then(|| {
            values
                .iter()
                .map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).unwrap_or(0.0) as f32)
                .collect()
        })
    };
    let mut transform = Transform::default();
    if let Some(p) = floats("position", 3) {
        transform.translation = Vec3::new(p[0], p[1], p[2]);
    }
    if let Some(r) = floats("rotation", 4) {
        let rotation = Quat::from_xyzw(r[0], r[1], r[2], r[3]);
        if rotation.is_finite() && rotation.length_squared() > 1e-12 {
            transform.rotation = rotation.normalize();
        }
    }
    if let Some(s) = floats("scale", 3) {
        transform.scale = Vec3::new(s[0], s[1], s[2]);
    }
    transform
}

/// Read one `_instance.toml`. `Ok(None)` when it is not a terrain layer class
/// (a spline point counts); otherwise the instance and the keys that could
/// not be read and kept their defaults. `folder_name` names an instance whose
/// metadata has no name.
pub fn parse_layer_instance(text: &str, folder_name: &str) -> Result<Option<(LayerInstanceFile, Vec<String>)>, String> {
    let doc: toml::Value = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let metadata = doc.get("metadata");
    let meta_str = |key: &str| metadata.and_then(|m| m.get(key)).and_then(|v| v.as_str());
    let Some(class_name) = meta_str("class_name").and_then(|s| ClassName::from_str(s).ok()) else {
        return Ok(None);
    };
    let section = |name: &str| doc.get(name).and_then(|v| v.as_table());
    let Some((component, problems)) = LayerComponent::from_sections(class_name, section) else {
        return Ok(None);
    };
    let layer = LayerInstanceFile {
        name: meta_str("name").unwrap_or(folder_name).to_string(),
        uuid: meta_str("uuid").unwrap_or_default().to_string(),
        class_name,
        transform: doc.get("transform").map(transform_from_toml).unwrap_or_default(),
        component,
        points: Vec::new(),
    };
    Ok(Some((layer, problems)))
}

/// Instance folders directly under `dir`, in name order.
fn instance_folders(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut folders: Vec<PathBuf> =
        entries.flatten().map(|entry| entry.path()).filter(|path| path.join("_instance.toml").is_file()).collect();
    folders.sort();
    folders
}

fn read_layer_folder(folder: &Path, problems: &mut Vec<String>) -> Option<LayerInstanceFile> {
    let toml_path = folder.join("_instance.toml");
    let text = match std::fs::read_to_string(&toml_path) {
        Ok(text) => text,
        Err(e) => {
            problems.push(format!("{}: {e}", toml_path.display()));
            return None;
        }
    };
    let name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("Layer");
    match parse_layer_instance(&text, name) {
        Ok(Some((layer, issues))) => {
            problems.extend(issues.into_iter().map(|issue| format!("{}: {issue}", toml_path.display())));
            Some(layer)
        }
        Ok(None) => None,
        Err(e) => {
            problems.push(format!("{}: {e}", toml_path.display()));
            None
        }
    }
}

/// Every terrain layer instance under a Space's `Workspace/Terrain/Layers`,
/// splines with their points, in folder-name order, and a line for every
/// file that could not be read or value that kept its default. For hosts
/// without the Studio loader (the Client). Only that folder is read, the one
/// Insert creates layers in; the Studio also loads a layer kept anywhere else.
pub fn read_layer_instances(space_root: &Path) -> (Vec<LayerInstanceFile>, Vec<String>) {
    let mut layers = Vec::new();
    let mut problems = Vec::new();
    for folder in instance_folders(&layers_dir(space_root)) {
        let Some(mut layer) = read_layer_folder(&folder, &mut problems) else { continue };
        match layer.class_name {
            // A point outside a spline has nothing to join.
            ClassName::TerrainSplinePoint => continue,
            ClassName::TerrainSpline => {
                for point_folder in instance_folders(&folder) {
                    if let Some(point) = read_layer_folder(&point_folder, &mut problems) {
                        if point.class_name == ClassName::TerrainSplinePoint {
                            layer.points.push(point);
                        }
                    }
                }
            }
            _ => {}
        }
        layers.push(layer);
    }
    (layers, problems)
}

/// Spawn what [`read_layer_instances`] read, each point parented to its
/// spline. Returns how many entities were spawned.
pub fn spawn_layer_instances(commands: &mut Commands, layers: &[LayerInstanceFile]) -> usize {
    layers.iter().map(|layer| spawn_layer_instance(commands, layer, None)).sum()
}

fn spawn_layer_instance(commands: &mut Commands, layer: &LayerInstanceFile, parent: Option<Entity>) -> usize {
    let entity = {
        let mut spawned = commands.spawn((
            Instance {
                name: layer.name.clone(),
                class_name: layer.class_name,
                archivable: true,
                id: 0,
                uuid: layer.uuid.clone(),
                ai: false,
            },
            Name::new(layer.name.clone()),
            layer.transform,
            Visibility::default(),
        ));
        layer.component.clone().insert_into(&mut spawned);
        if let Some(parent) = parent {
            spawned.insert(ChildOf(parent));
        }
        spawned.id()
    };
    1 + layer.points.iter().map(|point| spawn_layer_instance(commands, point, Some(entity))).sum::<usize>()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::realism::particle_sim::class::{format_value, parse_value, value_from_toml, value_to_toml};

    fn round_trip<T: FieldTable + Default>() {
        let defaults = T::default();
        for f in T::FIELDS {
            let v = defaults.get_field(f.name).unwrap_or_else(|| panic!("get {}", f.name));
            let mut other = T::default();
            other.set_field(f.name, v.clone()).unwrap_or_else(|e| panic!("set {}: {e}", f.name));
            assert_eq!(other.get_field(f.name).as_ref(), Some(&v), "{} set then get", f.name);
            assert_eq!(value_from_toml(f.kind, &value_to_toml(&v)).as_ref(), Some(&v), "{} TOML round trip", f.name);
            let parsed = parse_value(f.kind, &format_value(&v)).unwrap_or_else(|e| panic!("parse {}: {e}", f.name));
            assert_eq!(parsed, v, "{} text round trip", f.name);
        }
        let mut names: Vec<_> = T::FIELDS.iter().map(|f| f.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), T::FIELDS.len(), "{} has a repeated property name", T::SECTION);
    }

    #[test]
    fn every_field_round_trips_through_get_set_toml_and_text() {
        round_trip::<TerrainSpline>();
        round_trip::<TerrainSplinePoint>();
        round_trip::<TerrainStamp>();
        round_trip::<TerrainFlattenPad>();
        round_trip::<TerrainNoise>();
        round_trip::<TerrainMaterialFill>();
        round_trip::<TerrainScatter>();
        round_trip::<TerrainWaterBody>();
    }

    #[test]
    fn a_water_body_floods_from_its_pose_up_to_its_level() {
        let pose = Transform::from_xyz(12.0, 30.0, -4.0).with_rotation(Quat::from_rotation_y(0.7));
        let mut body = TerrainWaterBody::default();
        let desc = body.body(5, &pose);
        assert_eq!(desc.id, 5);
        assert_eq!(desc.seed, Vec2::new(12.0, -4.0), "the flood starts under the body");
        assert_eq!(desc.level, 32.0, "Level is above the body's position");
        let footprint = desc.footprint.expect("the default footprint is 100 m square");
        assert_eq!(footprint.center, desc.seed);
        assert!((footprint.yaw - 0.7).abs() < 1e-5);
        assert_eq!(footprint.half, Vec2::splat(50.0));

        body.set_text("Level", "-1.5").unwrap();
        body.set_text("SizeX", "0").unwrap();
        body.set_text("SizeZ", "-3").unwrap();
        body.set_text("Order", "3").unwrap();
        let desc = body.body(5, &pose);
        assert_eq!(desc.level, 28.5, "water may stand below the position");
        assert_eq!(body.size_z, 0.0, "sizes are not negative");
        assert_eq!(desc.footprint, None, "no size lets the water spread over the whole terrain");
        assert_eq!(desc.order, 3);
    }

    #[test]
    fn a_splines_water_bakes_nothing() {
        let mut spline = TerrainSpline { mode: SplineMode::River, ..TerrainSpline::default() };
        assert!(spline.water_surface, "a river holds water by default");
        let before = spline.layer(vec![Vec3::ZERO, Vec3::X * 10.0]);
        spline.set_text("WaterFill", "1.5").unwrap();
        assert_eq!(spline.water_fill, 1.0, "the fill is a fraction of the depth");
        spline.set_text("WaterSurface", "false").unwrap();
        assert!(!spline.water_surface);
        assert_eq!(spline.layer(vec![Vec3::ZERO, Vec3::X * 10.0]), before, "the bake sees no difference");
    }

    #[test]
    fn scatter_choices_name_every_kind_and_tree_type() {
        let kinds: Vec<&str> = ScatterKind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(SCATTER_KINDS, kinds.as_slice());
        let trees: Vec<&str> = TreeType::ALL.iter().map(|t| t.name()).collect();
        assert_eq!(TREE_TYPES, trees.as_slice());
    }

    #[test]
    fn a_scatter_maps_its_footprint_radius_and_rules() {
        let pose = Transform::from_xyz(12.0, 30.0, -4.0).with_rotation(Quat::from_rotation_y(0.7));
        let mut scatter = TerrainScatter::default();
        let layer = scatter.layer(9, &pose);
        assert_eq!(layer.id, 9);
        assert_eq!(layer.footprint, None, "no size covers the whole terrain, wherever the layer stands");
        assert_eq!(layer.radius, ScatterKind::Grass.default_radius());
        assert_eq!(layer.material, None);

        scatter.set_text("Kind", "trees").unwrap();
        scatter.set_text("SizeX", "40").unwrap();
        scatter.set_text("Material", "LeafyGrass").unwrap();
        scatter.set_text("Density", "5000").unwrap();
        scatter.set_text("MeshAsset", "  Assets/pine.glb ").unwrap();
        let layer = scatter.layer(9, &pose);
        assert_eq!(layer.radius, ScatterKind::Trees.default_radius(), "Radius 0 follows the Kind");
        assert_eq!(layer.density as f64, MAX_DENSITY, "density is clamped");
        assert_eq!(layer.material, Some(TerrainMaterial::LeafyGrass.to_u8()));
        assert_eq!(layer.mesh_asset, "Assets/pine.glb", "surrounding spaces are trimmed");
        let footprint = layer.footprint.expect("a size makes a footprint");
        assert_eq!(footprint.center, Vec2::new(12.0, -4.0));
        assert!((footprint.yaw - 0.7).abs() < 1e-5);
        assert_eq!(footprint.half.x, 20.0);
        assert!(footprint.half.y.is_infinite(), "a zero SizeZ leaves Z unlimited");

        scatter.set_text("Radius", "300").unwrap();
        assert_eq!(scatter.layer(9, &pose).radius, 300.0);
        scatter.set_text("Kind", "moss").expect_err("kinds are fixed names");
    }

    #[test]
    fn a_default_density_follows_the_kind_and_a_chosen_one_stays() {
        let mut scatter = TerrainScatter::default();
        assert_eq!(scatter.density, ScatterKind::Grass.default_density());
        scatter.set_text("Kind", "Trees").unwrap();
        assert_eq!(scatter.density, 1.0, "a new grass layer switched to trees gives a forest");
        scatter.set_text("Kind", "Rocks").unwrap();
        assert_eq!(scatter.density, ScatterKind::Rocks.default_density());

        let mut chosen = TerrainScatter::default();
        chosen.set_text("Density", "12").unwrap();
        chosen.set_text("Kind", "Trees").unwrap();
        assert_eq!(chosen.density, 12.0, "a density set by hand survives the switch");

        // A saved Trees layer at the grass default loads as saved: Kind is
        // applied before Density.
        let mut table = toml::value::Table::new();
        table.insert("kind".into(), toml::Value::String("Trees".into()));
        table.insert("density".into(), toml::Value::Float(40.0));
        let mut loaded = TerrainScatter::default();
        assert!(loaded.apply_toml_table(&table).is_empty());
        assert_eq!((loaded.kind, loaded.density), (ScatterKind::Trees, 40.0));
    }

    #[test]
    fn material_choices_are_none_then_every_built_in() {
        let built_in: Vec<&str> = TerrainMaterial::all().iter().map(|m| m.name()).collect();
        assert_eq!(MATERIALS[0], "None");
        assert_eq!(&MATERIALS[1..], built_in.as_slice());
        let mut fill = TerrainMaterialFill::default();
        fill.set_text("Material", "none").expect("None is a choice");
        assert_eq!(fill.material, None);
        assert_eq!(fill.layer(&Transform::IDENTITY).material, MATERIAL_SLOT_NONE);
        fill.set_text("Material", "cracked lava").expect_err("choices are exact names");
        fill.set_text("Material", "crackedlava").expect("choices ignore case");
        assert_eq!(fill.material, Some(TerrainMaterial::CrackedLava));
    }

    /// Every class template loads cleanly into its component's defaults, so
    /// Insert and a component built in code agree.
    #[test]
    fn class_templates_match_the_defaults() {
        fn check<T: FieldTable + Default + PartialEq + std::fmt::Debug>(template: &str, class: &str) {
            let doc: toml::Value = template.parse().unwrap_or_else(|e| panic!("{class} template: {e}"));
            let section = doc.get(T::SECTION).and_then(|v| v.as_table());
            assert!(section.is_some(), "{class} template has no [{}] section", T::SECTION);
            let (loaded, problems): (T, _) = from_section(section);
            assert!(problems.is_empty(), "{class}: {problems:?}");
            assert_eq!(loaded, T::default(), "{class} template differs from the defaults");
            let declared = doc.get("metadata").and_then(|m| m.get("class_name")).and_then(|v| v.as_str());
            assert_eq!(declared, Some(class));
            let class_name = ClassName::from_str(class).expect("a ClassName");
            assert!(class_name.is_terrain_layer());
            assert_eq!(section_name(class_name), Some(T::SECTION));
        }
        check::<TerrainSpline>(include_str!("../../assets/class_schema/TerrainSpline/_instance.toml"), "TerrainSpline");
        check::<TerrainSplinePoint>(
            include_str!("../../assets/class_schema/TerrainSplinePoint/_instance.toml"),
            "TerrainSplinePoint",
        );
        check::<TerrainStamp>(include_str!("../../assets/class_schema/TerrainStamp/_instance.toml"), "TerrainStamp");
        check::<TerrainFlattenPad>(
            include_str!("../../assets/class_schema/TerrainFlattenPad/_instance.toml"),
            "TerrainFlattenPad",
        );
        check::<TerrainNoise>(include_str!("../../assets/class_schema/TerrainNoise/_instance.toml"), "TerrainNoise");
        check::<TerrainMaterialFill>(
            include_str!("../../assets/class_schema/TerrainMaterialFill/_instance.toml"),
            "TerrainMaterialFill",
        );
        check::<TerrainScatter>(include_str!("../../assets/class_schema/TerrainScatter/_instance.toml"), "TerrainScatter");
        check::<TerrainWaterBody>(
            include_str!("../../assets/class_schema/TerrainWaterBody/_instance.toml"),
            "TerrainWaterBody",
        );
    }

    #[test]
    fn components_map_their_pose_and_parameters() {
        let pose = Transform::from_xyz(12.0, 30.0, -4.0).with_rotation(Quat::from_rotation_y(0.7));

        let mut stamp = TerrainStamp::default();
        stamp.set_text("Shape", "ridge").unwrap();
        stamp.set_text("Blend", "Max").unwrap();
        stamp.set_text("Strength", "3").unwrap();
        let layer = stamp.layer(&pose);
        assert_eq!(layer.center, Vec3::new(12.0, 30.0, -4.0));
        assert!((layer.yaw - 0.7).abs() < 1e-5, "yaw {}", layer.yaw);
        assert_eq!(layer.shape, StampShape::Ridge);
        assert_eq!(layer.blend, StampBlend::Max);
        assert_eq!(layer.strength, 1.0, "strength is clamped to 1");
        assert_eq!((layer.radius, layer.height, layer.falloff), (20.0, 6.0, 8.0));

        let mut pad = TerrainFlattenPad::default();
        pad.set_text("SizeX", "30").unwrap();
        pad.set_text("Material", "Concrete").unwrap();
        let layer = pad.layer(&pose);
        assert_eq!(layer.size, Vec2::new(30.0, 20.0));
        assert_eq!(layer.center.y, 30.0, "the pad's top is its height");
        assert_eq!(layer.material, Some(TerrainMaterial::Concrete.to_u8()));

        let mut noise = TerrainNoise::default();
        noise.set_text("Octaves", "40").unwrap();
        noise.set_text("Seed", "-5").unwrap();
        let layer = noise.layer(&pose);
        assert_eq!(layer.octaves, 12);
        assert_eq!(layer.seed, 0);

        let mut spline = TerrainSpline::default();
        spline.set_text("Mode", "canyon").unwrap();
        spline.set_text("Depth", "-7").unwrap();
        spline.set_text("ShoulderMaterial", "None").unwrap();
        let layer = spline.layer(vec![Vec3::ZERO, Vec3::X]);
        assert_eq!(layer.mode, SplineMode::Canyon);
        assert_eq!(layer.depth, 7.0, "depth is a magnitude");
        assert_eq!(layer.bed_material, Some(TerrainMaterial::Asphalt.to_u8()));
        assert_eq!(layer.shoulder_material, None);
        assert_eq!(layer.points, vec![Vec3::ZERO, Vec3::X]);
    }

    #[test]
    fn yaw_is_the_heading_even_when_tilted() {
        for yaw in [-2.5f32, -0.3, 0.0, 0.9, 3.0] {
            let turned = Quat::from_rotation_y(yaw);
            assert!((yaw_of(turned) - yaw).abs() < 1e-5, "{yaw}");
            let tilted = turned * Quat::from_rotation_x(0.4);
            assert!((yaw_of(tilted) - yaw).abs() < 1e-5, "{yaw} tilted");
        }
        // Local +X straight up: the heading falls back to the Euler yaw.
        let upright = Quat::from_rotation_y(0.5) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        assert!(yaw_of(upright).is_finite());
    }

    #[test]
    fn ids_come_from_the_uuid_and_points_join_by_index_then_name() {
        let entity = Entity::from_bits(42);
        assert_eq!(layer_id(Some("00000000000000ff0123456789abcdef"), entity), 0xff);
        assert_eq!(layer_id(Some(""), entity), entity.to_bits());
        assert_eq!(layer_id(None, entity), entity.to_bits());
        assert_eq!(layer_id(Some("not hex at all, not hex at all"), entity), entity.to_bits());

        let point = |index: f64, name: &str, id: u64, x: f32| PointKey { index, name: name.into(), id, position: Vec3::X * x };
        let ordered = ordered_points(vec![
            point(2.0, "A", 1, 3.0),
            point(0.0, "Z", 2, 0.0),
            point(1.0, "B", 3, 2.0),
            point(1.0, "A", 4, 1.0),
            point(0.5, "Q", 5, 0.5),
        ]);
        assert_eq!(ordered, vec![Vec3::ZERO, Vec3::X * 0.5, Vec3::X, Vec3::X * 2.0, Vec3::X * 3.0]);
    }

    #[test]
    fn a_layer_file_parses_with_its_transform_and_section() {
        let text = r#"
            [transform]
            position = [1.0, 2, 3.5]
            rotation = [0.0, 0.0, 0.0, 2.0]
            scale = [1.0, 1.0, 1.0]

            [terrain_flatten_pad]
            size_x = 12
            falloff = "wide"

            [metadata]
            class_name = "TerrainFlattenPad"
            name = "Helipad"
            uuid = "0000000000000001aaaaaaaaaaaaaaaa"
        "#;
        let (layer, problems) = parse_layer_instance(text, "Folder").expect("parses").expect("a layer class");
        assert_eq!(layer.name, "Helipad");
        assert_eq!(layer.class_name, ClassName::TerrainFlattenPad);
        assert_eq!(layer.transform.translation, Vec3::new(1.0, 2.0, 3.5));
        assert_eq!(layer.transform.rotation, Quat::IDENTITY, "rotation is normalized");
        let LayerComponent::FlattenPad(pad) = &layer.component else { panic!("{:?}", layer.component) };
        assert_eq!(pad.size_x, 12.0);
        assert_eq!(pad.falloff, TerrainFlattenPad::default().falloff, "a bad value keeps its default");
        assert_eq!(problems.len(), 1, "{problems:?}");

        let part = "[metadata]\nclass_name = \"Part\"\n";
        assert!(parse_layer_instance(part, "Part").expect("parses").is_none());
    }

    // ── The sync system ──────────────────────────────────────────────────

    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            ..TerrainConfig::default()
        }
    }

    struct Rig {
        world: World,
        system: bevy::ecs::system::SystemId,
        root: Entity,
    }

    impl Rig {
        fn new() -> Self {
            let mut world = World::new();
            world.init_resource::<Time<Real>>();
            world.init_resource::<TerrainDirtyChunks>();
            world.init_resource::<TerrainLayerSync>();
            let config = config();
            let mut base = TerrainData::procedural();
            base.resize_cache(&config);
            let root = world.spawn((TerrainRoot, config, base)).id();
            // Registered once, so change detection and the removal readers
            // carry over from run to run as they do in a schedule.
            let system = world.register_system(sync_terrain_layers);
            Self { world, system, root }
        }

        fn run(&mut self) {
            assert!(self.world.run_system(self.system).is_ok(), "the sync system runs");
        }

        fn wait(&mut self, millis: u64) {
            self.world.resource_mut::<Time<Real>>().advance_by(Duration::from_millis(millis));
        }

        fn layers(&self) -> Option<Vec<LayerDesc>> {
            self.world.get::<TerrainBaked>(self.root).map(|baked| baked.layers().to_vec())
        }

        fn instance(class_name: ClassName, name: &str, uuid: &str) -> Instance {
            Instance { name: name.into(), class_name, archivable: true, id: 0, uuid: uuid.into(), ai: false }
        }
    }

    fn uuid(n: u8) -> String {
        format!("{n:016x}{}", "0".repeat(16))
    }

    #[test]
    fn enabled_layers_bake_in_order_with_their_points() {
        let mut rig = Rig::new();
        let stamp_pose = Transform::from_xyz(5.0, 0.0, 5.0);
        rig.world.spawn((
            Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(3)),
            TerrainStamp { order: 5, ..TerrainStamp::default() },
            stamp_pose,
        ));
        rig.world.spawn((
            Rig::instance(ClassName::TerrainFlattenPad, "Pad", &uuid(2)),
            TerrainFlattenPad { order: -1, ..TerrainFlattenPad::default() },
            Transform::from_xyz(-5.0, 12.0, 0.0),
        ));
        rig.world.spawn((
            Rig::instance(ClassName::TerrainNoise, "Off", &uuid(4)),
            TerrainNoise { enabled: false, ..TerrainNoise::default() },
            Transform::default(),
        ));
        // A spline moved 10 m along X; its points sit under it, joined by
        // Index whatever their spawn order.
        let spline = rig
            .world
            .spawn((
                Rig::instance(ClassName::TerrainSpline, "Road", &uuid(1)),
                TerrainSpline::default(),
                Transform::from_xyz(10.0, 0.0, 0.0),
            ))
            .id();
        rig.world.spawn((
            Rig::instance(ClassName::TerrainSplinePoint, "B", &uuid(11)),
            TerrainSplinePoint { index: 1.0 },
            Transform::from_xyz(0.0, 3.0, 8.0),
            ChildOf(spline),
        ));
        rig.world.spawn((
            Rig::instance(ClassName::TerrainSplinePoint, "A", &uuid(10)),
            TerrainSplinePoint { index: 0.0 },
            Transform::from_xyz(0.0, 3.0, -8.0),
            ChildOf(spline),
        ));

        rig.run();
        let layers = rig.layers().expect("a root with layers gets a bake");
        let ids: Vec<u64> = layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![2, 1, 3], "pad (order -1), spline (0), stamp (5); the disabled noise is left out");
        let LayerKind::Spline(road) = &layers[1].kind else { panic!("{:?}", layers[1].kind) };
        assert_eq!(road.points, vec![Vec3::new(10.0, 3.0, -8.0), Vec3::new(10.0, 3.0, 8.0)]);
        let LayerKind::Stamp(hill) = &layers[2].kind else { panic!("{:?}", layers[2].kind) };
        assert_eq!(hill.center, stamp_pose.translation);
    }

    #[test]
    fn edits_wait_for_the_interval_and_the_final_state_is_baked() {
        let mut rig = Rig::new();
        let stamp = rig
            .world
            .spawn((Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(1)), TerrainStamp::default(), Transform::default()))
            .id();
        rig.run();
        let radius = |rig: &Rig| match &rig.layers().expect("baked")[0].kind {
            LayerKind::Stamp(s) => s.radius,
            other => panic!("{other:?}"),
        };
        assert_eq!(radius(&rig), 20.0);

        // Two edits inside one interval: neither is handed over yet.
        rig.world.get_mut::<TerrainStamp>(stamp).unwrap().radius = 25.0;
        rig.run();
        rig.wait(20);
        rig.world.get_mut::<TerrainStamp>(stamp).unwrap().radius = 30.0;
        rig.run();
        assert_eq!(radius(&rig), 20.0, "held back by the interval");

        // No further edit, but the interval has passed: the last state goes.
        rig.wait(40);
        rig.run();
        assert_eq!(radius(&rig), 30.0, "the final state is baked");
    }

    #[test]
    fn the_last_layer_going_removes_the_bake_and_remeshes_its_bounds() {
        let mut rig = Rig::new();
        let stamp = rig
            .world
            .spawn((Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(1)), TerrainStamp::default(), Transform::default()))
            .id();
        rig.run();
        assert!(rig.layers().is_some());
        rig.world.resource_mut::<TerrainDirtyChunks>().remesh.clear();

        rig.world.despawn(stamp);
        rig.wait(60);
        rig.run();
        assert!(rig.layers().is_none(), "no layers, no bake");
        assert!(!rig.world.resource::<TerrainDirtyChunks>().remesh.is_empty(), "the stamp's chunks remesh from the base");

        // Disabling works the same way as removing.
        let stamp = rig
            .world
            .spawn((Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(1)), TerrainStamp::default(), Transform::default()))
            .id();
        rig.wait(60);
        rig.run();
        assert!(rig.layers().is_some());
        rig.world.get_mut::<TerrainStamp>(stamp).unwrap().enabled = false;
        rig.wait(60);
        rig.run();
        assert!(rig.layers().is_none());
    }

    #[test]
    fn a_new_terrain_root_gets_the_layers() {
        let mut rig = Rig::new();
        rig.world.spawn((Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(1)), TerrainStamp::default(), Transform::default()));
        rig.run();
        rig.world.despawn(rig.root);
        let config = config();
        let mut base = TerrainData::procedural();
        base.resize_cache(&config);
        rig.root = rig.world.spawn((TerrainRoot, config, base)).id();
        rig.wait(60);
        rig.run();
        assert_eq!(rig.layers().map(|layers| layers.len()), Some(1), "the regenerated terrain is baked too");
    }

    #[test]
    fn point_moves_index_edits_and_removal_reach_the_bake() {
        let mut rig = Rig::new();
        let spline = rig
            .world
            .spawn((Rig::instance(ClassName::TerrainSpline, "Road", &uuid(1)), TerrainSpline::default(), Transform::default()))
            .id();
        let point = |rig: &mut Rig, name: &str, n: u8, index: f64, z: f32| {
            rig.world
                .spawn((
                    Rig::instance(ClassName::TerrainSplinePoint, name, &uuid(n)),
                    TerrainSplinePoint { index },
                    Transform::from_xyz(0.0, 0.0, z),
                    ChildOf(spline),
                ))
                .id()
        };
        let a = point(&mut rig, "A", 10, 0.0, -10.0);
        let b = point(&mut rig, "B", 11, 1.0, 10.0);
        let points = |rig: &Rig| match &rig.layers().expect("baked")[0].kind {
            LayerKind::Spline(s) => s.points.clone(),
            other => panic!("{other:?}"),
        };
        rig.run();
        assert_eq!(points(&rig), vec![Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0)]);

        // The Move tool (or its undo) writes the point's Transform.
        rig.world.get_mut::<Transform>(b).unwrap().translation.x = 6.0;
        rig.wait(60);
        rig.run();
        assert_eq!(points(&rig)[1], Vec3::new(6.0, 0.0, 10.0), "a moved point moves the corridor");

        // Properties (or undo) sets an Index that puts A after B.
        rig.world.get_mut::<TerrainSplinePoint>(a).unwrap().index = 2.0;
        rig.wait(60);
        rig.run();
        assert_eq!(points(&rig), vec![Vec3::new(6.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -10.0)], "points rejoin by Index");

        // Delete (or undo of an Insert) despawns a point.
        rig.world.despawn(a);
        rig.wait(60);
        rig.run();
        assert_eq!(points(&rig), vec![Vec3::new(6.0, 0.0, 10.0)], "a removed point leaves the corridor");
    }

    #[test]
    fn a_layer_moved_through_its_parent_or_unparented_reaches_the_bake() {
        let mut rig = Rig::new();
        // A holder that is not a layer, like a Model the user put a stamp in.
        let holder = rig.world.spawn(Transform::from_xyz(10.0, 0.0, 0.0)).id();
        let stamp = rig
            .world
            .spawn((
                Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(1)),
                TerrainStamp::default(),
                Transform::from_xyz(5.0, 0.0, 5.0),
                ChildOf(holder),
            ))
            .id();
        let centre = |rig: &Rig| match &rig.layers().expect("baked")[0].kind {
            LayerKind::Stamp(s) => s.center,
            other => panic!("{other:?}"),
        };
        rig.run();
        assert_eq!(centre(&rig), Vec3::new(15.0, 0.0, 5.0));

        // Only the holder's Transform changes.
        rig.world.get_mut::<Transform>(holder).unwrap().translation.x = 20.0;
        rig.wait(60);
        rig.run();
        assert_eq!(centre(&rig), Vec3::new(25.0, 0.0, 5.0), "a parent's move moves the layer");

        // Taken out to the scene root: `ChildOf` is removed, not changed.
        rig.world.entity_mut(stamp).remove::<ChildOf>();
        rig.wait(60);
        rig.run();
        assert_eq!(centre(&rig), Vec3::new(5.0, 0.0, 5.0), "an unparented layer bakes where it now stands");
    }

    #[test]
    fn a_spline_reads_as_the_bake_takes_it_even_while_disabled() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        let spline = world
            .spawn((
                Rig::instance(ClassName::TerrainSpline, "River", &uuid(1)),
                TerrainSpline { enabled: false, mode: SplineMode::River, ..TerrainSpline::default() },
                Transform::from_xyz(0.0, 2.0, 0.0),
            ))
            .id();
        for (n, index, z) in [(2u8, 1.0, 5.0f32), (3, 0.0, -5.0)] {
            world.spawn((
                Rig::instance(ClassName::TerrainSplinePoint, "Point", &uuid(n)),
                TerrainSplinePoint { index },
                Transform::from_xyz(1.0, 0.0, z),
                ChildOf(spline),
            ));
        }
        let stamp = world
            .spawn((Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(4)), TerrainStamp::default(), Transform::default()))
            .id();
        let (read, of_stamp) = world
            .run_system_once(move |layers: LayerQueries| (layers.spline(spline), layers.spline(stamp)))
            .expect("runs");
        let (id, enabled, layer) = read.expect("a spline");
        assert_eq!(id, 1, "the id the bake knows it by");
        assert!(!enabled);
        assert_eq!(layer.mode, SplineMode::River);
        assert_eq!(layer.points, vec![Vec3::new(1.0, 2.0, -5.0), Vec3::new(1.0, 2.0, 5.0)], "under the spline, by Index");
        assert!(of_stamp.is_none(), "a stamp is not a spline");
    }

    /// The whole path a layer edit and a base undo take in the Studio: the
    /// sync system and the dirty-chunk pass run together as the schedule runs
    /// them. A moved layer puts the base back where it was, and a base write
    /// under a layer, marked the way undo marks it, bakes with the layer on
    /// top.
    #[test]
    fn a_move_restores_the_base_and_a_base_undo_under_a_layer_rebakes() {
        use bevy::ecs::schedule::Schedule;
        use crate::terrain::height_query::{cache_cell_at_world, height_at_world, set_height_at_world};

        let mut world = World::new();
        world.init_resource::<Time>();
        world.init_resource::<Time<Real>>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<TerrainDirtyChunks>();
        world.init_resource::<TerrainLayerSync>();
        let config = config();
        let mut base = TerrainData::procedural();
        base.resize_cache(&config);
        let root = world.spawn((TerrainRoot, config.clone(), base)).id();
        let stamp = world
            .spawn((
                Rig::instance(ClassName::TerrainStamp, "Hill", &uuid(1)),
                TerrainStamp::default(),
                Transform::from_xyz(4.0, 0.0, 4.0),
            ))
            .id();

        let mut schedule = Schedule::default();
        schedule.add_systems((sync_terrain_layers, apply_terrain_dirty_chunks).chain());
        let mut step = |world: &mut World| {
            world.resource_mut::<Time<Real>>().advance_by(Duration::from_millis(60));
            schedule.run(world);
        };
        let baked_at = |world: &World, x: f32, z: f32| {
            height_at_world(&config, &world.get::<TerrainBaked>(root).expect("baked").data, x, z)
        };

        step(&mut world);
        assert!((baked_at(&world, 4.0, 4.0) - 6.0).abs() < 0.1, "the 6 m mound is baked in");

        world.get_mut::<Transform>(stamp).unwrap().translation = Vec3::new(40.0, 0.0, 40.0);
        step(&mut world);
        assert!(baked_at(&world, 4.0, 4.0).abs() < 1e-3, "the ground the mound left is the base again");
        assert!((baked_at(&world, 40.0, 40.0) - 6.0).abs() < 0.1, "the mound stands where it was moved");

        // What the mound adds to the cell under it.
        let cell = {
            let data = world.get::<TerrainData>(root).expect("the base");
            let cell = cache_cell_at_world(&config, data, 40.0, 40.0).expect("a raster");
            cell.y as usize * data.cache_width as usize + cell.x as usize
        };
        let cell_height = |world: &World, baked: bool| {
            let data = if baked {
                &world.get::<TerrainBaked>(root).expect("baked").data
            } else {
                world.get::<TerrainData>(root).expect("the base")
            };
            config.world_height(data.height_cache[cell])
        };
        let added = cell_height(&world, true) - cell_height(&world, false);
        assert!(added > 5.0, "the mound adds {added} m");

        // Undo writes the recorded base tiles, then marks their chunks.
        {
            let mut data = world.get_mut::<TerrainData>(root).expect("the base");
            set_height_at_world(&config, &mut data, 40.0, 40.0, 10.0, 1.0);
        }
        world.resource_mut::<TerrainDirtyChunks>().mark(IVec2::new(1, 1));
        step(&mut world);
        assert!((cell_height(&world, false) - 10.0).abs() < 1e-3, "the base took the write");
        assert!(
            (cell_height(&world, true) - (10.0 + added)).abs() < 1e-3,
            "the bake is the undone base with the mound still on top"
        );

        // Moving the mound off again shows the undone base itself.
        world.get_mut::<Transform>(stamp).unwrap().translation = Vec3::new(4.0, 0.0, 4.0);
        step(&mut world);
        assert!((cell_height(&world, true) - 10.0).abs() < 1e-3, "the old bounds re-bake from the current base");
    }
}
