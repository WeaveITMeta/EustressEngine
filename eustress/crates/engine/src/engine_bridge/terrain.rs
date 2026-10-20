//! Terrain over the bridge: what the terrain is, what the ground is at a
//! point or along a ray, the voxels in a box, and the edits the Terrain editor
//! offers (generate, flat plate, sculpt, paint, fill, carve, replace material,
//! layer instances, clear).
//!
//! # One path per kind of work
//!
//! Reads go through [`with_terrain_read`], which hands out the surface data
//! (the layer bake when the terrain has layers), so a query answers for the
//! ground the user sees. Edits of the ground go through
//! [`apply_terrain_commands`] with [`TerrainCommandOrigin::Tool`], the command
//! path the editor and scripts share: each call is one undo step, and the
//! chunks it touched are marked dirty, so their meshes and colliders rebuild
//! over the next frames. Layer instances are created the way Insert creates
//! them, and clearing does what the ribbon's Clear does.
//!
//! Generation is the exception. `terrain.generate` and `terrain.flat` write
//! the event the Terrain ribbon writes; the world generator runs on the async
//! compute pool and the flat plate is written and loaded by its own system,
//! both on later frames. Their responses say `queued`, never done, and name
//! the call that confirms the result.
//!
//! # Limits
//!
//! Handlers run on the main thread inside one frame, so every request has a
//! cap: samples per query, voxels per read, brush and ball radii, shape
//! extents, and the volume lattice points one fill may visit. A request over a
//! cap is refused with the cap in the message, never trimmed.

use std::collections::BTreeMap;
use std::path::Path;

use bevy::prelude::*;
use serde_json::{json, Value};

use eustress_common::classes::ClassName;
use eustress_common::realism::particle_sim::class::FieldTable;
use eustress_common::terrain::api::{
    command_bounds, read_voxels, TerrainCommand, TerrainCommandEffect, TerrainFill, TerrainSculptMode,
};
use eustress_common::terrain::layer_instances::{layers_dir, LAYERS_FOLDER};
use eustress_common::terrain::worldgen::export::FlatSpec;
use eustress_common::terrain::worldgen::pipeline::WorldSpec;
use eustress_common::terrain::{
    cache_cell_at_world, field_normal, height_at_world, lattice_cell_size, material_at, material_at_world,
    raycast_terrain_surface, sample_field_parts, Chunk, FieldTerm, TerrainBaked, TerrainConfig, TerrainData,
    TerrainFlattenPad, TerrainGenerationQueue, TerrainMaterial, TerrainMaterialFill, TerrainMaterialSlots,
    TerrainNoise, TerrainRoot, TerrainScatter, TerrainSpline, TerrainSplinePoint, TerrainStamp, TerrainSurface,
    TerrainVolume, TerrainVoxelWater, TerrainWaterBody, FIRST_CUSTOM_MATERIAL_SLOT, MATERIAL_SLOT_COUNT,
    MATERIAL_SLOT_NONE,
};

use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::space::instance_create::{create_instance, CreatedInstance, InstanceOverrides};
use crate::space::SpaceRoot;
use crate::terrain_commands::{
    apply_terrain_commands, tool_edit_record, with_terrain_read, TerrainCommandOrigin, ToolEditRecord,
};
use crate::ui::{GenerateFlatTerrainEvent, GenerateWorldEvent, WorldgenTask};

/// Most samples one `terrain.query` may take.
const MAX_QUERY_SAMPLES: usize = 10_000;
/// Most voxels one `terrain.read_voxels` may read.
const MAX_READ_VOXELS: u64 = 32_768;
/// Ray length `terrain.raycast` uses when none is given, metres.
const DEFAULT_RAY_DISTANCE: f32 = 1_000.0;
/// Longest ray `terrain.raycast` casts, metres.
const MAX_RAY_DISTANCE: f32 = 100_000.0;
/// Largest brush, ball or cylinder radius, metres.
const MAX_RADIUS: f32 = 256.0;
/// Largest extent of a block, a cylinder or a box along any axis, metres.
const MAX_EXTENT: f32 = 512.0;
/// Most volume lattice points one solid or air fill may visit: about 128
/// cells a side, which keeps a fill to a few tens of milliseconds.
const MAX_FILL_LATTICE_POINTS: u64 = 128 * 128 * 128;
/// Sculpt strength when the caller gives none.
const DEFAULT_SCULPT_STRENGTH: f32 = 0.5;
/// Paint strength when the caller gives none: the material replaces the
/// cell's outright.
const DEFAULT_PAINT_STRENGTH: f32 = 1.0;
/// Seed of the Terrain ribbon's quick Generate presets.
const DEFAULT_WORLD_SEED: u64 = 42;
/// The ribbon's flat plate: 64 m chunks of 64 samples, over a height band
/// from 32 m below the plate to 96 m above it.
const FLAT_CHUNK_SIZE: f32 = 64.0;
const FLAT_CHUNK_RESOLUTION: u32 = 64;
const FLAT_DIG_DEPTH: f32 = 32.0;
const FLAT_BAND: f32 = 128.0;
/// Farthest from world Y 0 a flat plate may sit, metres.
const MAX_FLAT_HEIGHT: f32 = 10_000.0;
/// Most control points one `terrain.layer_create` gives a spline. Each point
/// reads the spline's folder back to number itself, so the cost grows with
/// the square of the count.
const MAX_SPLINE_POINTS: usize = 64;
/// How far either side of its position a spline created without points puts
/// its first two, metres, as Insert does.
const NEW_SPLINE_HALF_LENGTH: f32 = 20.0;
/// Slots the `terrain.stats` material histogram lists.
const HISTOGRAM_SLOTS: usize = 8;
/// Raster cells `terrain.stats` visits at most; a larger raster is read at a
/// stride, which the response reports.
const STATS_MAX_CELLS: usize = 4_194_304;
/// The Terrain instance's own file, directly in `Workspace/Terrain`.
const TERRAIN_INSTANCE_FILE: &str = "_instance.toml";

const NO_SPACE: &str = "no Space is open";
const NO_TERRAIN: &str = "the open Space has no terrain; terrain_generate or terrain_flat makes one";
const PROCEDURAL: &str = "this terrain is procedural: it has no height raster for the terrain tools to read or \
                          edit. terrain_flat or terrain_generate replaces it with one they can";
const GENERATION_BUSY: &str = "a terrain generation is running or queued; wait until terrain_stats reports \
                               generation.busy false";

// ---------------------------------------------------------------------------
// Replies and parameters
// ---------------------------------------------------------------------------

fn reply(req: &BridgeRequest, result: Value) -> BridgeResponse {
    BridgeResponse::ok(req.id.clone(), result)
}

fn refuse(req: &BridgeRequest, message: impl Into<String>) -> BridgeResponse {
    BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(message))
}

fn fail(req: &BridgeRequest, message: impl Into<String>) -> BridgeResponse {
    BridgeResponse::error(req.id.clone(), BridgeError::internal(message))
}

/// Answer a read that ran through [`with_terrain_read`], where `None` means
/// the Space has no terrain.
fn answer_read(req: &BridgeRequest, result: Option<Result<Value, String>>) -> BridgeResponse {
    match result {
        Some(Ok(value)) => reply(req, value),
        Some(Err(e)) => refuse(req, e),
        None => refuse(req, NO_TERRAIN),
    }
}

/// `x` rounded to `decimals` places, as the f64 the JSON carries. An f32 put
/// in JSON directly prints its binary expansion (0.1 as 0.10000000149011612);
/// a value that is not finite becomes null.
fn num(x: impl Into<f64>, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (x.into() * scale).round() / scale
}

fn xz_json(v: Vec2) -> Value {
    json!([num(v.x, 3), num(v.y, 3)])
}

fn xyz_json(v: Vec3) -> Value {
    json!([num(v.x, 3), num(v.y, 3), num(v.z, 3)])
}

fn normal_json(n: Vec3) -> Value {
    json!([num(n.x, 4), num(n.y, 4), num(n.z, 4)])
}

/// A parameter that is present and not null.
fn param<'a>(params: &'a Value, key: &str) -> Option<&'a Value> {
    params.get(key).filter(|v| !v.is_null())
}

/// `[x, y, z]`, three finite numbers.
fn vec3_of(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let p = Vec3::new(a[0].as_f64()? as f32, a[1].as_f64()? as f32, a[2].as_f64()? as f32);
    p.is_finite().then_some(p)
}

/// A world XZ position: `[x, z]`, or `[x, y, z]` with the height ignored.
fn xz_of(v: &Value) -> Option<Vec2> {
    let a = v.as_array()?;
    let (x, z) = match a.len() {
        2 => (&a[0], &a[1]),
        3 => (&a[0], &a[2]),
        _ => return None,
    };
    let p = Vec2::new(x.as_f64()? as f32, z.as_f64()? as f32);
    p.is_finite().then_some(p)
}

fn opt_vec3(params: &Value, key: &str) -> Result<Option<Vec3>, String> {
    match param(params, key) {
        None => Ok(None),
        Some(v) => vec3_of(v)
            .map(Some)
            .ok_or_else(|| format!("`{key}` must be [x, y, z]: three finite numbers in world metres")),
    }
}

fn req_vec3(params: &Value, key: &str) -> Result<Vec3, String> {
    opt_vec3(params, key)?.ok_or_else(|| format!("`{key}` [x, y, z] is required (world metres)"))
}

fn opt_f32(params: &Value, key: &str) -> Result<Option<f32>, String> {
    match param(params, key) {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .map(|n| n as f32)
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| format!("`{key}` must be a finite number")),
    }
}

fn opt_str<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match param(params, key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim()).filter(|s| !s.is_empty())),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

fn opt_bool(params: &Value, key: &str) -> Result<Option<bool>, String> {
    match param(params, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

/// A positive length no longer than `cap` metres. `what` names it in the
/// refusal.
fn check_length(what: &str, value: f32, cap: f32) -> Result<f32, String> {
    if !(value > 0.0) {
        return Err(format!("{what} must be greater than 0"));
    }
    if value > cap {
        return Err(format!("{what} {value} m is over the {cap} m cap; split the edit into smaller ones"));
    }
    Ok(value)
}

/// A radius in metres: required, positive, at most [`MAX_RADIUS`].
fn radius_of(params: &Value, key: &str) -> Result<f32, String> {
    let radius = opt_f32(params, key)?.ok_or_else(|| format!("`{key}` is required (metres)"))?;
    check_length(&format!("`{key}`"), radius, MAX_RADIUS)
}

/// Extents positive on every axis and at most [`MAX_EXTENT`] on any.
fn check_extent(what: &str, extent: Vec3) -> Result<Vec3, String> {
    if !extent.cmpgt(Vec3::ZERO).all() {
        return Err(format!("{what} must be greater than 0 on every axis"));
    }
    let longest = extent.max_element();
    if longest > MAX_EXTENT {
        return Err(format!(
            "{what} spans {longest} m, over the {MAX_EXTENT} m cap per axis; split the edit into smaller ones"
        ));
    }
    Ok(extent)
}

/// A world box from `min` and `max`, corners in either order, within
/// [`MAX_EXTENT`] per axis.
fn box_of(params: &Value) -> Result<(Vec3, Vec3), String> {
    let (a, b) = (req_vec3(params, "min")?, req_vec3(params, "max")?);
    let (min, max) = (a.min(b), a.max(b));
    check_extent("the box from `min` to `max`", max - min)?;
    Ok((min, max))
}

fn strength_of(params: &Value, default: f32) -> Result<f32, String> {
    let strength = opt_f32(params, "strength")?.unwrap_or(default);
    if !(0.0..=1.0).contains(&strength) {
        return Err(format!("`strength` must be between 0 and 1, got {strength}"));
    }
    Ok(strength)
}

fn rotation_of(params: &Value) -> Result<Quat, String> {
    match param(params, "rotation") {
        None => Ok(Quat::IDENTITY),
        Some(v) => parse_rotation(v),
    }
}

/// A shape's rotation: a quaternion `[x, y, z, w]` (normalized here), or
/// Euler angles in degrees `[x, y, z]` composed like Roblox's `CFrame.Angles`:
/// about X, then about the turned Y, then about the turned Z.
fn parse_rotation(v: &Value) -> Result<Quat, String> {
    let bad = || "`rotation` must be a quaternion [x, y, z, w] or Euler angles in degrees [x, y, z]".to_string();
    let parts: Vec<f32> = v
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .map(|n| n.as_f64().map(|n| n as f32).filter(|n| n.is_finite()))
        .collect::<Option<Vec<f32>>>()
        .ok_or_else(bad)?;
    match parts.as_slice() {
        &[x, y, z] => Ok(Quat::from_euler(EulerRot::XYZ, x.to_radians(), y.to_radians(), z.to_radians())),
        &[x, y, z, w] => {
            let q = Quat::from_xyzw(x, y, z, w);
            if !(q.length() > 1e-6) {
                return Err("`rotation` quaternion has zero length".to_string());
            }
            Ok(q.normalize())
        }
        _ => Err(bad()),
    }
}

// ---------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------

/// `name` without a Roblox enum prefix, as `TerrainFill::from_material_name`
/// reads it: "Enum.Material.Rock" and "Material.Rock" read as "Rock".
fn strip_material_prefix(name: &str) -> &str {
    let name = name.trim();
    for prefix in ["enum.material.", "material."] {
        if name.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix)) {
            return &name[prefix.len()..];
        }
    }
    name
}

/// A built-in terrain material by name, in any spelling
/// `TerrainMaterial::from_name` takes, with or without a Roblox enum prefix.
fn builtin_material(name: &str) -> Option<TerrainMaterial> {
    TerrainMaterial::from_name(strip_material_prefix(name))
}

fn builtin_material_names() -> String {
    TerrainMaterial::all().iter().map(|m| m.name()).collect::<Vec<_>>().join(", ")
}

/// The material slot `name` paints: a built-in material, or a custom
/// material the Space defines.
fn material_slot(slots: Option<&TerrainMaterialSlots>, name: &str) -> Result<u8, String> {
    if let Some(material) = builtin_material(name) {
        return Ok(material.to_u8());
    }
    let wanted = strip_material_prefix(name);
    let mut custom = Vec::new();
    for (slot, def) in slots.into_iter().flat_map(|s| s.iter()) {
        if slot < FIRST_CUSTOM_MATERIAL_SLOT {
            continue;
        }
        if def.name.eq_ignore_ascii_case(wanted) {
            return Ok(slot);
        }
        custom.push(def.name.as_str());
    }
    let own = if custom.is_empty() {
        String::new()
    } else {
        format!(", or one of this Space's own: {}", custom.join(", "))
    };
    Err(format!("unknown material '{name}'; use one of {}{own}", builtin_material_names()))
}

/// The name slot `slot` goes by: the Space's slot table first (it names the
/// custom slots, and any built-in a material file renames), else the
/// built-in material's.
fn slot_name(slots: Option<&TerrainMaterialSlots>, slot: u8) -> String {
    if let Some(def) = slots.and_then(|s| s.get(slot)) {
        return def.name.clone();
    }
    TerrainMaterial::from_u8(slot).map_or_else(|| format!("Slot{slot}"), |m| m.name().to_string())
}

// ---------------------------------------------------------------------------
// Raster cells
// ---------------------------------------------------------------------------

/// Index into the raster's per-cell arrays of the cell under world `x, z`.
fn cell_index(config: &TerrainConfig, data: &TerrainData, x: f32, z: f32) -> Option<usize> {
    let cell = cache_cell_at_world(config, data, x, z)?;
    Some(cell.y as usize * data.cache_width as usize + cell.x as usize)
}

/// A column without ground (see `TerrainData::cell_is_hole`): a cell of a
/// sparse raster, imported voxel terrain, that holds no material. Its height
/// only feeds normals and water.
fn is_hole(data: &TerrainData, cell: Option<usize>) -> bool {
    cell.is_some_and(|i| data.cell_is_hole(i))
}

/// The water surface over raster cell `cell`, when the water was built for
/// this raster and the column holds water.
fn water_level(water: Option<&TerrainVoxelWater>, data: &TerrainData, cell: Option<usize>) -> Option<f32> {
    let water = water.filter(|w| w.width == data.cache_width && w.height == data.cache_height)?;
    cell.and_then(|i| water.levels.get(i).copied()).filter(|level| level.is_finite())
}

fn inside_footprint(config: &TerrainConfig, x: f32, z: f32) -> bool {
    let (min, max) = config.footprint_xz();
    x >= min.x && x <= max.x && z >= min.y && z <= max.y
}

/// The material slot of the surface at `p`: a volume edit's material where
/// an edit forms the surface (a cave wall, an overhang), else the heightfield
/// cell's.
fn surface_slot(config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume, p: Vec3) -> Option<u8> {
    if !matches!(sample_field_parts(config, data, volume, p).term(), FieldTerm::Heightfield) {
        if let Some(material) = material_at(config, volume, p) {
            return Some(material.to_u8());
        }
    }
    material_at_world(config, data, p.x, p.z).map(|m| m.primary)
}

// ---------------------------------------------------------------------------
// Generation state
// ---------------------------------------------------------------------------

/// A generation is running, its chunks are still meshing, a request for one
/// waits in the event queue, or an imported terrain is still being built.
/// The ribbon's own single-flight gate, plus the waiting requests, so two
/// calls in one frame cannot both queue one, plus the voxel loader's
/// background build, whose root would replace whatever a call made meanwhile.
pub(crate) fn generation_busy(world: &World) -> bool {
    let generating = world.get_resource::<WorldgenTask>().is_some_and(WorldgenTask::is_busy);
    let meshing = world
        .get_resource::<TerrainGenerationQueue>()
        .is_some_and(TerrainGenerationQueue::is_generating);
    let waiting = world
        .get_resource::<Messages<GenerateWorldEvent>>()
        .is_some_and(|m| !m.is_empty())
        || world
            .get_resource::<Messages<GenerateFlatTerrainEvent>>()
            .is_some_and(|m| !m.is_empty());
    generating || meshing || waiting || voxel_build_running(world)
}

fn generation_json(world: &World) -> Value {
    let task = world.get_resource::<WorldgenTask>();
    let status = task.map(|t| t.status.clone()).filter(|s| !s.is_empty());
    let queued = world
        .get_resource::<TerrainGenerationQueue>()
        .map_or(0, |q| q.pending_chunks.len());
    json!({
        "busy": generation_busy(world),
        "generating": task.is_some_and(|t| t.task.is_some()),
        "meshing": task.is_some_and(|t| t.meshing),
        "importing": voxel_build_running(world),
        "status": status,
        "chunks_queued": queued,
    })
}

#[cfg(feature = "world-db")]
fn voxel_sourced(world: &World, root: Entity) -> bool {
    world.get::<crate::terrain_voxel_load::VoxelSourcedTerrain>(root).is_some()
}

#[cfg(not(feature = "world-db"))]
fn voxel_sourced(_world: &World, _root: Entity) -> bool {
    false
}

/// The voxel loader is building a converted Space's imported terrain in the
/// background (see `terrain_voxel_load`): it has not landed yet, or a moved
/// window is about to replace it.
#[cfg(feature = "world-db")]
fn voxel_build_running(world: &World) -> bool {
    world
        .get_resource::<crate::terrain_voxel_load::VoxelTerrainBuild>()
        .is_some_and(crate::terrain_voxel_load::VoxelTerrainBuild::is_running)
}

#[cfg(not(feature = "world-db"))]
fn voxel_build_running(_world: &World) -> bool {
    false
}

/// Where a root's terrain came from: `voxel` (a converted Space's world
/// database), `disk` (Workspace/Terrain: generated, flat, imported or saved)
/// or `runtime` (made in this session and never saved).
fn terrain_source(world: &World, root: Entity) -> &'static str {
    if voxel_sourced(world, root) {
        "voxel"
    } else if world.get::<crate::terrain_disk_load::DiskSourcedTerrain>(root).is_some() {
        "disk"
    } else {
        "runtime"
    }
}

// ---------------------------------------------------------------------------
// terrain.stats
// ---------------------------------------------------------------------------

/// `terrain.stats`: whether the open Space has a terrain, and what it is. All
/// lengths are world metres.
pub fn terrain_stats(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let roots: Vec<Entity> = world.query_filtered::<Entity, With<TerrainRoot>>().iter(world).collect();
    let chunks = world.query_filtered::<(), With<Chunk>>().iter(world).count();
    let layers = layer_counts(world);
    let world: &World = world;
    let generation = generation_json(world);
    let Some(&root) = roots.first() else {
        return reply(
            req,
            json!({
                "present": false,
                "roots": 0,
                "chunks_spawned": chunks,
                "layers": layers,
                "generation": generation,
                "note": "The open Space has no terrain. terrain_generate or terrain_flat makes one.",
            }),
        );
    };
    let slots = world.get_resource::<TerrainMaterialSlots>();
    let raster = with_terrain_read(world, |config, data, volume, water| {
        raster_json(config, data, volume, water, slots)
    });
    let mut out = json!({
        "present": true,
        "roots": roots.len(),
        "source": terrain_source(world, root),
        "layers_baked": world.get::<TerrainBaked>(root).is_some(),
        "textured_surface": world.get::<TerrainSurface>(root).is_some_and(TerrainSurface::is_drawable),
        "chunks_spawned": chunks,
        "layers": layers,
        "generation": generation,
    });
    if let (Value::Object(base), Some(Value::Object(extra))) = (&mut out, raster) {
        base.extend(extra);
    }
    reply(req, out)
}

/// Instances of every terrain layer class in the world.
fn layer_counts(world: &mut World) -> Value {
    fn count<T: Component>(world: &mut World) -> usize {
        world.query_filtered::<(), With<T>>().iter(world).count()
    }
    json!({
        "TerrainSpline": count::<TerrainSpline>(world),
        "TerrainSplinePoint": count::<TerrainSplinePoint>(world),
        "TerrainStamp": count::<TerrainStamp>(world),
        "TerrainFlattenPad": count::<TerrainFlattenPad>(world),
        "TerrainNoise": count::<TerrainNoise>(world),
        "TerrainMaterialFill": count::<TerrainMaterialFill>(world),
        "TerrainScatter": count::<TerrainScatter>(world),
        "TerrainWaterBody": count::<TerrainWaterBody>(world),
    })
}

/// The grid, raster, height range, material histogram, volume and water of
/// one terrain.
fn raster_json(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    water: Option<&TerrainVoxelWater>,
    slots: Option<&TerrainMaterialSlots>,
) -> Value {
    let cells = data.height_cache.len();
    let step = cells.div_ceil(STATS_MAX_CELLS).max(1);
    let scale = step as u64;
    let has_materials = data.has_material_layer();
    let mut counts = vec![0u64; MATERIAL_SLOT_COUNT];
    let (mut lowest, mut highest) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut holes = 0u64;
    for i in (0..cells).step_by(step) {
        if is_hole(data, Some(i)) {
            holes += 1;
            continue;
        }
        let h = config.world_height(data.height_cache[i]);
        if h.is_finite() {
            lowest = lowest.min(h);
            highest = highest.max(h);
        }
        let cell = if has_materials { data.material_cache.get(i).copied() } else { None };
        if let Some([slot, ..]) = cell {
            if slot != MATERIAL_SLOT_NONE {
                counts[usize::from(slot)] += 1;
            }
        }
    }

    let with_material: u64 = counts.iter().sum();
    let mut top: Vec<(usize, u64)> = counts.iter().copied().enumerate().filter(|&(_, n)| n > 0).collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(HISTOGRAM_SLOTS);
    let materials: Vec<Value> = top
        .iter()
        .map(|&(slot, n)| {
            let slot = slot as u8;
            json!({
                "slot": slot,
                "name": slot_name(slots, slot),
                "share": num(n as f64 / with_material as f64, 3),
                "cells": n * scale,
            })
        })
        .collect();

    let heights = (lowest <= highest).then(|| json!({ "min": num(lowest, 3), "max": num(highest, 3) }));
    let water = water.map(|w| {
        let (mut wet, mut low, mut high) = (0u64, f32::INFINITY, f32::NEG_INFINITY);
        for level in w.levels.iter().step_by(step).copied().filter(|l| l.is_finite()) {
            wet += 1;
            low = low.min(level);
            high = high.max(level);
        }
        json!({
            "wet_cells": wet * scale,
            "levels_m": (wet > 0).then(|| json!([num(low, 3), num(high, 3)])),
            "matches_raster": w.width == data.cache_width && w.height == data.cache_height,
        })
    });
    let (footprint_min, footprint_max) = config.footprint_xz();
    let (width, depth) = config.total_size();
    json!({
        "grid": {
            "chunk_size_m": num(config.chunk_size, 3),
            "chunk_resolution": config.chunk_resolution,
            "chunks_per_side": [u64::from(config.chunks_x) * 2 + 1, u64::from(config.chunks_z) * 2 + 1],
            "chunks_from_center": [config.chunks_x, config.chunks_z],
            "center_chunk": [config.center_chunk.x, config.center_chunk.y],
            "lattice_cell_m": num(lattice_cell_size(config), 4),
            "footprint": { "min": xz_json(footprint_min), "max": xz_json(footprint_max) },
            "size_m": [num(width, 3), num(depth, 3)],
            "height_band_m": [num(config.world_height(0.0), 3), num(config.world_height(1.0), 3)],
        },
        "raster": {
            "width": data.cache_width,
            "height": data.cache_height,
            "procedural": data.height_cache.is_empty(),
            "sparse": data.sparse_surface,
            "material_layer": has_materials,
            "holes": holes * scale,
            "scan_step": step,
        },
        "heights_m": heights,
        "materials": materials,
        "volume": { "bricks": volume.brick_count() },
        "water": water,
    })
}

// ---------------------------------------------------------------------------
// terrain.query
// ---------------------------------------------------------------------------

/// Where `terrain.query` samples, in order, and the grid they form when a
/// grid was asked for.
#[derive(Debug, PartialEq)]
struct QuerySamples {
    positions: Vec<Vec2>,
    grid: Option<GridLayout>,
}

/// A regular grid of samples `step` apart from `min`, `nx` by `nz`, row-major
/// with x running fastest.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GridLayout {
    min: Vec2,
    step: f32,
    nx: usize,
    nz: usize,
}

impl GridLayout {
    fn positions(&self) -> Vec<Vec2> {
        let GridLayout { min, step, nx, nz } = *self;
        (0..nz)
            .flat_map(|iz| (0..nx).map(move |ix| min + Vec2::new(ix as f32, iz as f32) * step))
            .collect()
    }
}

fn query_samples(params: &Value) -> Result<QuerySamples, String> {
    match (param(params, "points"), param(params, "grid")) {
        (Some(_), Some(_)) => Err("pass `points` or `grid`, not both".to_string()),
        (Some(points), None) => Ok(QuerySamples { positions: parse_points(points)?, grid: None }),
        (None, Some(grid)) => {
            let layout = parse_grid(grid)?;
            Ok(QuerySamples { positions: layout.positions(), grid: Some(layout) })
        }
        (None, None) => Err("pass `points` [[x, z], ...] or `grid` {min: [x, z], max: [x, z], step}".to_string()),
    }
}

fn parse_points(v: &Value) -> Result<Vec<Vec2>, String> {
    let list = v.as_array().ok_or("`points` must be an array of [x, z] positions")?;
    if list.is_empty() {
        return Err("`points` is empty".to_string());
    }
    if list.len() > MAX_QUERY_SAMPLES {
        return Err(format!(
            "{} points is over the {MAX_QUERY_SAMPLES}-sample cap; split the query",
            list.len()
        ));
    }
    list.iter()
        .enumerate()
        .map(|(i, p)| xz_of(p).ok_or_else(|| format!("points[{i}] must be [x, z]: two finite numbers in world metres")))
        .collect()
}

fn parse_grid(v: &Value) -> Result<GridLayout, String> {
    let corner = |key: &str| {
        v.get(key)
            .and_then(xz_of)
            .ok_or_else(|| format!("`grid.{key}` must be [x, z] in world metres"))
    };
    let (a, b) = (corner("min")?, corner("max")?);
    let step = v
        .get("step")
        .and_then(Value::as_f64)
        .map(|s| s as f32)
        .filter(|s| s.is_finite() && *s > 0.0)
        .ok_or("`grid.step` must be a number greater than 0 (metres)")?;
    let (min, max) = (a.min(b), a.max(b));
    // Samples from min to max inclusive; the slack keeps a span that is a
    // whole number of steps from losing its last sample to rounding.
    let count = |span: f32| ((span / step + 1e-3).floor() as usize).saturating_add(1);
    let (nx, nz) = (count(max.x - min.x), count(max.y - min.y));
    let total = nx.saturating_mul(nz);
    if total > MAX_QUERY_SAMPLES {
        return Err(format!(
            "the grid holds {nx} x {nz} = {total} samples, over the {MAX_QUERY_SAMPLES}-sample cap; raise `step` or \
             shrink the grid"
        ));
    }
    Ok(GridLayout { min, step, nx, nz })
}

/// `terrain.query`: the surface at world XZ points, or over a grid. Height is
/// the heightfield's (world metres), the material is the surface cell's, and
/// the normal is the terrain field's, caves and overhangs included. Samples
/// outside the footprint, and holes in imported terrain, are null.
pub fn terrain_query(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let samples = match query_samples(&req.params) {
        Ok(samples) => samples,
        Err(e) => return refuse(req, e),
    };
    let world: &World = world;
    let slots = world.get_resource::<TerrainMaterialSlots>();
    let result = with_terrain_read(world, |config, data, volume, water| {
        if data.height_cache.is_empty() {
            return Err(PROCEDURAL.to_string());
        }
        Ok(sample_surface(config, data, volume, water, slots, &samples))
    });
    answer_read(req, result)
}

fn sample_surface(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    water: Option<&TerrainVoxelWater>,
    slots: Option<&TerrainMaterialSlots>,
    samples: &QuerySamples,
) -> Value {
    let n = samples.positions.len();
    let mut heights = Vec::with_capacity(n);
    let mut materials = Vec::with_capacity(n);
    let mut slot_ids = Vec::with_capacity(n);
    let mut normals = Vec::with_capacity(n);
    let mut water_levels = Vec::with_capacity(n);
    let mut holes = Vec::new();
    let mut outside = Vec::new();
    let mut names: BTreeMap<u8, String> = BTreeMap::new();
    let (mut lowest, mut highest) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut any_water = false;
    for (i, p) in samples.positions.iter().enumerate() {
        if !inside_footprint(config, p.x, p.y) {
            outside.push(i);
            for column in [&mut heights, &mut materials, &mut slot_ids, &mut normals, &mut water_levels] {
                column.push(Value::Null);
            }
            continue;
        }
        let cell = cell_index(config, data, p.x, p.y);
        let level = water_level(water, data, cell);
        any_water |= level.is_some();
        water_levels.push(level.map_or(Value::Null, |l| json!(num(l, 3))));
        if is_hole(data, cell) {
            holes.push(i);
            for column in [&mut heights, &mut materials, &mut slot_ids, &mut normals] {
                column.push(Value::Null);
            }
            continue;
        }
        let h = height_at_world(config, data, p.x, p.y);
        lowest = lowest.min(h);
        highest = highest.max(h);
        heights.push(json!(num(h, 3)));
        normals.push(normal_json(field_normal(config, data, volume, Vec3::new(p.x, h, p.y))));
        match material_at_world(config, data, p.x, p.y) {
            Some(m) => {
                let name = names.entry(m.primary).or_insert_with(|| slot_name(slots, m.primary));
                materials.push(Value::String(name.clone()));
                slot_ids.push(json!(m.primary));
            }
            None => {
                materials.push(Value::Null);
                slot_ids.push(Value::Null);
            }
        }
    }

    let range = (lowest <= highest).then(|| json!([num(lowest, 3), num(highest, 3)]));
    let mut out = json!({
        "count": n,
        "heights": heights,
        "materials": materials,
        "slots": slot_ids,
        "normals": normals,
        "holes": holes,
        "outside": outside,
        "height_range": range,
    });
    if any_water {
        out["water_levels"] = Value::Array(water_levels);
    }
    if let Some(grid) = samples.grid {
        out["grid"] = json!({
            "min": xz_json(grid.min),
            "step": num(grid.step, 4),
            "nx": grid.nx,
            "nz": grid.nz,
            "index": "ix + nx * iz (x fastest)",
        });
    }
    out
}

// ---------------------------------------------------------------------------
// terrain.raycast
// ---------------------------------------------------------------------------

/// `terrain.raycast`: the first point where a ray enters the terrain field
/// (the heightfield plus caves and overhangs), ignoring every other collider.
pub fn terrain_raycast(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let parsed = (|| -> Result<(Vec3, Dir3, f32), String> {
        let origin = req_vec3(&req.params, "origin")?;
        let direction = opt_vec3(&req.params, "direction")?.unwrap_or(Vec3::NEG_Y);
        let direction = Dir3::new(direction).map_err(|_| "`direction` must be a non-zero vector".to_string())?;
        let max = opt_f32(&req.params, "max_distance")?.unwrap_or(DEFAULT_RAY_DISTANCE);
        if !(max > 0.0) || max > MAX_RAY_DISTANCE {
            return Err(format!("`max_distance` must be greater than 0 and at most {MAX_RAY_DISTANCE} m"));
        }
        Ok((origin, direction, max))
    })();
    let (origin, direction, max) = match parsed {
        Ok(ray) => ray,
        Err(e) => return refuse(req, e),
    };
    let world: &World = world;
    let slots = world.get_resource::<TerrainMaterialSlots>();
    let result = with_terrain_read(world, |config, data, volume, water| {
        if data.height_cache.is_empty() {
            return Err(PROCEDURAL.to_string());
        }
        let mut out = json!({
            "origin": xyz_json(origin),
            "direction": xyz_json(*direction),
            "max_distance": num(max, 3),
            "hit": false,
        });
        let ray = Ray3d::new(origin, direction);
        let Some(point) = raycast_terrain_surface(config, data, Some(volume), ray, max, lattice_cell_size(config))
        else {
            return Ok(out);
        };
        // Past its footprint the raster reads as its edge heights extended
        // outward, which is no ground at all.
        if !inside_footprint(config, point.x, point.z) {
            out["note"] = json!("the ray met only the terrain's edge extended past its footprint");
            return Ok(out);
        }
        let cell = cell_index(config, data, point.x, point.z);
        if is_hole(data, cell) {
            out["note"] = json!("the ray reached a column with no ground (a hole in imported terrain)");
            return Ok(out);
        }
        let slot = surface_slot(config, data, volume, point);
        out["hit"] = json!(true);
        out["point"] = xyz_json(point);
        out["distance"] = json!(num(origin.distance(point), 3));
        out["normal"] = normal_json(field_normal(config, data, volume, point));
        out["material"] = json!(slot.map(|s| slot_name(slots, s)));
        out["slot"] = json!(slot);
        out["water_level"] = json!(water_level(water, data, cell).map(|l| num(l, 3)));
        Ok(out)
    });
    answer_read(req, result)
}

// ---------------------------------------------------------------------------
// terrain.read_voxels
// ---------------------------------------------------------------------------

/// Voxels of `resolution` metres covering the box from `min` to `max`,
/// rounded up per axis, within [`MAX_READ_VOXELS`].
fn voxel_grid(min: Vec3, max: Vec3, resolution: f32) -> Result<UVec3, String> {
    if !(resolution.is_finite() && resolution > 0.0) {
        return Err("`resolution` must be a number greater than 0 (metres)".to_string());
    }
    // The slack keeps a side that is a whole number of voxels from rounding
    // up to one more.
    let cells = ((max - min) / resolution - Vec3::splat(1e-3)).ceil().max(Vec3::ONE);
    if !cells.is_finite() || cells.max_element() > MAX_READ_VOXELS as f32 {
        return Err(format!(
            "the box is more than {MAX_READ_VOXELS} voxels along an axis at {resolution} m; raise `resolution` or \
             shrink the box"
        ));
    }
    let size = cells.as_uvec3();
    let count = u64::from(size.x) * u64::from(size.y) * u64::from(size.z);
    if count > MAX_READ_VOXELS {
        return Err(format!(
            "the box holds {} x {} x {} = {count} voxels at {resolution} m, over the {MAX_READ_VOXELS}-voxel cap; \
             raise `resolution` or shrink the box",
            size.x, size.y, size.z
        ));
    }
    Ok(size)
}

/// `terrain.read_voxels`: Roblox's `Terrain:ReadVoxels` over a world box, the
/// materials (a terrain material, Air or Water) and occupancies (0 to 1) of
/// its voxels, indexed `x + size_x * (y + size_y * z)`.
pub fn terrain_read_voxels(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let parsed = (|| -> Result<(Vec3, Vec3, Option<f32>), String> {
        let (a, b) = (req_vec3(&req.params, "min")?, req_vec3(&req.params, "max")?);
        let (min, max) = (a.min(b), a.max(b));
        if !(max - min).cmpgt(Vec3::ZERO).all() {
            return Err("the box from `min` to `max` must be greater than 0 on every axis".to_string());
        }
        Ok((min, max, opt_f32(&req.params, "resolution")?))
    })();
    let (min, max, resolution) = match parsed {
        Ok(parsed) => parsed,
        Err(e) => return refuse(req, e),
    };
    let result = with_terrain_read(world, |config, data, volume, water| -> Result<Value, String> {
        if data.height_cache.is_empty() {
            return Err(PROCEDURAL.to_string());
        }
        let resolution = resolution.unwrap_or_else(|| lattice_cell_size(config));
        let size = voxel_grid(min, max, resolution)?;
        let (fills, occupancies) = read_voxels(config, data, volume, water, min, resolution, size);
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        for fill in &fills {
            *tally.entry(fill.name()).or_insert(0) += 1;
        }
        let materials: Vec<&str> = fills.iter().map(TerrainFill::name).collect();
        let occupancies: Vec<f64> = occupancies.iter().map(|o| num(*o, 3)).collect();
        Ok(json!({
            "min": xyz_json(min),
            "max": xyz_json(min + size.as_vec3() * resolution),
            "resolution": num(resolution, 4),
            "size": [size.x, size.y, size.z],
            "count": materials.len(),
            "index": "x + size_x * (y + size_y * z)",
            "materials": materials,
            "occupancies": occupancies,
            "material_counts": tally,
        }))
    });
    answer_read(req, result)
}

// ---------------------------------------------------------------------------
// terrain.generate / terrain.flat
// ---------------------------------------------------------------------------

/// The Terrain ribbon's Generate presets: regions per side, and fine-grid
/// samples per region side.
fn world_preset(name: &str) -> Result<(u32, u32), String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "small" => Ok((2, 384)),
        "medium" => Ok((3, 320)),
        "large" => Ok((4, 288)),
        other => Err(format!("unknown preset '{other}' (expected small, medium or large)")),
    }
}

/// The Terrain ribbon's flat plate sizes: chunks from the centre chunk to each
/// edge.
fn flat_half_extent(size: &str) -> Result<u32, String> {
    match size.trim().to_ascii_lowercase().as_str() {
        "small" => Ok(2),
        "medium" => Ok(4),
        "large" => Ok(8),
        other => Err(format!("unknown size '{other}' (expected small, medium or large)")),
    }
}

/// The plate the ribbon's Generate > Flat writes, `half_extent` chunks either
/// side of the origin, at `height_m` in `material_slot`. Its height band
/// keeps the ribbon's room to work: 32 m to dig below the plate, 96 m to
/// build above it.
fn flat_spec(half_extent: u32, height_m: f32, material_slot: u8) -> Result<FlatSpec, String> {
    if !(height_m.abs() <= MAX_FLAT_HEIGHT) {
        return Err(format!("`height_m` must be within {MAX_FLAT_HEIGHT} m of 0"));
    }
    let spec = FlatSpec {
        half_extent,
        chunk_size: FLAT_CHUNK_SIZE,
        chunk_resolution: FLAT_CHUNK_RESOLUTION,
        height_m,
        height_offset: height_m - FLAT_DIG_DEPTH,
        height_scale: FLAT_BAND,
        material_slot,
        ..FlatSpec::default()
    };
    spec.grid()?;
    Ok(spec)
}

/// `terrain.generate`: queue a procedural world from one of the ribbon's
/// presets. The world generator runs in the background and replaces the
/// terrain when it finishes; the response only says it was queued.
pub fn terrain_generate(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let parsed = (|| -> Result<(String, (u32, u32), u64), String> {
        let preset = opt_str(&req.params, "preset")?.unwrap_or("medium").to_ascii_lowercase();
        let regions = world_preset(&preset)?;
        let seed = match param(&req.params, "seed") {
            None => DEFAULT_WORLD_SEED,
            Some(v) => v.as_u64().ok_or("`seed` must be a whole number, 0 or greater")?,
        };
        Ok((preset, regions, seed))
    })();
    let (preset, (regions, region_res), seed) = match parsed {
        Ok(parsed) => parsed,
        Err(e) => return refuse(req, e),
    };
    if world.get_resource::<SpaceRoot>().is_none() {
        return refuse(req, NO_SPACE);
    }
    if generation_busy(world) {
        return refuse(req, GENERATION_BUSY);
    }
    let spec = WorldSpec { seed, regions_x: regions, regions_z: regions, region_res, ..Default::default() };
    let side = f64::from(regions) * spec.region_size_m;
    if world.write_message(GenerateWorldEvent { spec }).is_none() {
        return fail(req, "the world generator is not running in this engine (it comes with the Studio UI)");
    }
    info!("terrain.generate: queued a {preset} world, seed {seed}, {regions}x{regions} regions");
    reply(
        req,
        json!({
            "queued": true,
            "preset": preset,
            "seed": seed,
            "regions": [regions, regions],
            "region_res": region_res,
            "size_m": [side, side],
            "note": "Generation runs in the background and replaces the current terrain when it finishes, then \
                     its chunks mesh over a few more seconds. Call terrain_stats until generation.busy is false \
                     to confirm.",
        }),
    )
}

/// `terrain.flat`: queue a flat plate, the ribbon's Generate > Flat. The plate
/// is written and loaded on the next frame and meshes over the frames after;
/// the response only says it was queued.
pub fn terrain_flat(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let parsed = {
        let slots = world.get_resource::<TerrainMaterialSlots>();
        (|| -> Result<(String, FlatSpec, String), String> {
            let size = opt_str(&req.params, "size")?.unwrap_or("medium").to_ascii_lowercase();
            let half_extent = flat_half_extent(&size)?;
            let height_m = opt_f32(&req.params, "height_m")?.unwrap_or(0.0);
            let slot = match opt_str(&req.params, "material")? {
                Some(name) => material_slot(slots, name)?,
                None => TerrainMaterial::Grass.to_u8(),
            };
            Ok((size, flat_spec(half_extent, height_m, slot)?, slot_name(slots, slot)))
        })()
    };
    let (size, spec, material) = match parsed {
        Ok(parsed) => parsed,
        Err(e) => return refuse(req, e),
    };
    if world.get_resource::<SpaceRoot>().is_none() {
        return refuse(req, NO_SPACE);
    }
    if generation_busy(world) {
        return refuse(req, GENERATION_BUSY);
    }
    let side = spec.half_extent * 2 + 1;
    let extent = spec.total_extent_m();
    let band = [num(spec.height_offset, 3), num(spec.height_offset + spec.height_scale, 3)];
    let height = num(spec.height_m, 3);
    if world.write_message(GenerateFlatTerrainEvent { spec }).is_none() {
        return fail(req, "the flat plate writer is not running in this engine (it comes with the Studio UI)");
    }
    info!("terrain.flat: queued a {size} plate, {extent} m square at Y={height}, {material}");
    reply(
        req,
        json!({
            "queued": true,
            "size": size,
            "chunks_per_side": side,
            "extent_m": num(extent, 3),
            "height_m": height,
            "height_band_m": band,
            "material": material,
            "note": "The plate is written to Workspace/Terrain and loaded on the next frame, then its chunks mesh \
                     over the frames after. Call terrain_stats to confirm.",
        }),
    )
}

// ---------------------------------------------------------------------------
// Edits: sculpt, paint, fill, carve, replace material
// ---------------------------------------------------------------------------

/// Whether `cmd` writes the volume lattice, the cost [`MAX_FILL_LATTICE_POINTS`]
/// caps. Water only raises the water surface over a footprint.
fn visits_volume(cmd: &TerrainCommand) -> bool {
    match cmd {
        TerrainCommand::FillBall { fill, .. }
        | TerrainCommand::FillBlock { fill, .. }
        | TerrainCommand::FillCylinder { fill, .. }
        | TerrainCommand::FillRegion { fill, .. } => !matches!(fill, TerrainFill::Water),
        _ => false,
    }
}

/// Volume lattice points in the world box from `min` to `max` at `cell`
/// metres: the work a volumetric edit over it costs.
fn lattice_points(min: Vec3, max: Vec3, cell: f32) -> u64 {
    let span = ((max - min).max(Vec3::ZERO) / cell.max(1e-4)).ceil() + Vec3::ONE;
    if !span.is_finite() {
        return u64::MAX;
    }
    (span.x as u64).saturating_mul(span.y as u64).saturating_mul(span.z as u64)
}

/// Refuse an edit before it runs: no terrain, a procedural one, or a fill
/// that would visit more of the volume lattice than one call may.
fn preflight(world: &World, cmd: &TerrainCommand) -> Result<(), String> {
    let checked = with_terrain_read(world, |config, data, _, _| {
        if data.height_cache.is_empty() {
            return Err(PROCEDURAL.to_string());
        }
        if !visits_volume(cmd) {
            return Ok(());
        }
        let Some((min, max)) = command_bounds(config, cmd) else {
            return Ok(());
        };
        let cell = lattice_cell_size(config);
        let points = lattice_points(min, max, cell);
        if points > MAX_FILL_LATTICE_POINTS {
            return Err(format!(
                "this fill would visit {points} volume lattice points at the terrain's {} m cell, over the \
                 {MAX_FILL_LATTICE_POINTS} one call may; split it into smaller fills",
                num(cell, 3)
            ));
        }
        Ok(())
    });
    checked.unwrap_or_else(|| Err(NO_TERRAIN.to_string()))
}

/// Apply `cmd` as a tool edit labelled `label`, and report what it changed
/// and whether it can be undone (see `terrain_commands`' module docs).
fn apply_one(world: &mut World, req: &BridgeRequest, cmd: TerrainCommand, label: &str) -> BridgeResponse {
    if let Err(e) = preflight(world, &cmd) {
        return refuse(req, e);
    }
    let record = tool_edit_record(world);
    let origin = TerrainCommandOrigin::Tool { label: label.to_string() };
    match apply_terrain_commands(world, vec![cmd], origin).pop() {
        Some(Ok(effect)) => reply(req, effect_json(label, &effect, record)),
        Some(Err(e)) => refuse(req, e),
        None => fail(req, "the terrain command returned no result"),
    }
}

fn effect_json(label: &str, effect: &TerrainCommandEffect, record: ToolEditRecord) -> Value {
    let rect = |r: Option<(Vec2, Vec2)>| r.map(|(min, max)| json!({ "min": xz_json(min), "max": xz_json(max) }));
    let volume = effect.volume_edit.as_ref().filter(|edit| !edit.is_empty());
    // Water levels are no part of a terrain undo entry, so an edit that only
    // moved water has nothing to undo.
    let ground_changed =
        effect.height_rect.is_some() || effect.material_rect.is_some() || volume.is_some() || effect.cleared;
    let changed = ground_changed || effect.water_changed;
    let undoable = ground_changed && record == ToolEditRecord::Undo;
    let note = match (changed, record) {
        (false, _) => "Nothing changed: the edit reached no terrain.",
        (true, ToolEditRecord::Undo) if !ground_changed => {
            "Applied to the water only, which terrain undo does not record, so it has no undo step and is not saved \
             with the terrain."
        }
        (true, ToolEditRecord::Undo) => "Applied as one undo step. Meshes and colliders rebuild over the next frames.",
        (true, ToolEditRecord::RolledBackAtStop) => {
            "Applied for this Play session only: it comes undone when Play stops, with no undo step. Meshes and \
             colliders rebuild over the next frames."
        }
        (true, ToolEditRecord::NotUndoable) => {
            "Applied without an undo step, because a terrain brush stroke was open. Meshes and colliders rebuild \
             over the next frames."
        }
    };
    let volume = volume.map(|edit| {
        json!({ "bricks_changed": edit.bricks.len(), "min": xyz_json(edit.min), "max": xyz_json(edit.max) })
    });
    json!({
        "applied": true,
        "changed": changed,
        "undoable": undoable,
        "undo_label": label,
        "height_rect": rect(effect.height_rect),
        "material_rect": rect(effect.material_rect),
        "volume": volume,
        "water_changed": effect.water_changed,
        "note": note,
    })
}

fn sculpt_mode(name: &str, flatten_height: f32) -> Result<TerrainSculptMode, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "raise" => Ok(TerrainSculptMode::Raise),
        "lower" => Ok(TerrainSculptMode::Lower),
        "flatten" => Ok(TerrainSculptMode::Flatten { height: flatten_height }),
        "smooth" => Ok(TerrainSculptMode::Smooth),
        other => Err(format!("unknown mode '{other}' (expected raise, lower, flatten or smooth)")),
    }
}

/// `terrain.sculpt`: one dab of the editor's round brush, raising, lowering,
/// flattening or smoothing the ground. Flatten goes to `height`, else to the
/// centre's Y.
pub fn terrain_sculpt(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let p = &req.params;
    let parsed = (|| -> Result<TerrainCommand, String> {
        let mode = opt_str(p, "mode")?.ok_or("`mode` is required: raise, lower, flatten or smooth")?;
        let center = req_vec3(p, "center")?;
        let radius = radius_of(p, "radius")?;
        let strength = strength_of(p, DEFAULT_SCULPT_STRENGTH)?;
        let mode = sculpt_mode(mode, opt_f32(p, "height")?.unwrap_or(center.y))?;
        Ok(TerrainCommand::Sculpt { mode, center, radius, strength })
    })();
    match parsed {
        Ok(cmd) => apply_one(world, req, cmd, "Sculpt Terrain"),
        Err(e) => refuse(req, e),
    }
}

/// `terrain.paint`: paint a material slot onto the surface with a round brush.
pub fn terrain_paint(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let p = &req.params;
    let parsed = {
        let slots = world.get_resource::<TerrainMaterialSlots>();
        (|| -> Result<TerrainCommand, String> {
            let center = req_vec3(p, "center")?;
            let radius = radius_of(p, "radius")?;
            let name = opt_str(p, "material")?.ok_or("`material` is required: a terrain material name")?;
            let material = material_slot(slots, name)?;
            let strength = strength_of(p, DEFAULT_PAINT_STRENGTH)?;
            Ok(TerrainCommand::Paint { center, radius, material, strength })
        })()
    };
    match parsed {
        Ok(cmd) => apply_one(world, req, cmd, "Paint Terrain"),
        Err(e) => refuse(req, e),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FillShape {
    Ball,
    Block,
    Cylinder,
    Region,
}

impl FillShape {
    fn parse(name: &str) -> Result<Self, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "ball" | "sphere" => Ok(Self::Ball),
            "block" | "box" => Ok(Self::Block),
            "cylinder" => Ok(Self::Cylinder),
            "region" => Ok(Self::Region),
            other => Err(format!("unknown shape '{other}' (expected ball, block, cylinder or region)")),
        }
    }
}

/// The fill `params` describe, with `fill` in its shape.
fn fill_command(params: &Value, fill: TerrainFill) -> Result<TerrainCommand, String> {
    let shape = opt_str(params, "shape")?.ok_or("`shape` is required: ball, block, cylinder or region")?;
    Ok(match FillShape::parse(shape)? {
        FillShape::Ball => TerrainCommand::FillBall {
            center: req_vec3(params, "center")?,
            radius: radius_of(params, "radius")?,
            fill,
        },
        FillShape::Block => TerrainCommand::FillBlock {
            center: req_vec3(params, "center")?,
            rotation: rotation_of(params)?,
            size: check_extent("`size`", req_vec3(params, "size")?)?,
            fill,
        },
        FillShape::Cylinder => {
            let height = opt_f32(params, "height")?
                .ok_or("`height` is required for a cylinder (metres along its local Y)")?;
            TerrainCommand::FillCylinder {
                center: req_vec3(params, "center")?,
                rotation: rotation_of(params)?,
                height: check_length("`height`", height, MAX_EXTENT)?,
                radius: radius_of(params, "radius")?,
                fill,
            }
        }
        FillShape::Region => {
            let (min, max) = box_of(params)?;
            TerrainCommand::FillRegion { min, max, fill }
        }
    })
}

/// What a fill puts in its shape. Air only with `carve`: that flag is how the
/// Destructive `terrain_carve` tool asks for it, and the Write-class
/// `terrain_fill` never sends it, so no material name it forwards can remove
/// ground.
fn fill_of(material: Option<&str>, carve: bool) -> Result<TerrainFill, String> {
    if carve {
        return match material {
            None => Ok(TerrainFill::Air),
            Some(name) if matches!(TerrainFill::from_material_name(name), Some(TerrainFill::Air)) => {
                Ok(TerrainFill::Air)
            }
            Some(name) => Err(format!("a carve always leaves air; drop `material` ('{name}'), or fill instead")),
        };
    }
    let name = material.ok_or("`material` is required: a terrain material name, or Water")?;
    match TerrainFill::from_material_name(name) {
        Some(TerrainFill::Air) => Err("material Air removes ground, which is a carve: use terrain_carve".to_string()),
        Some(fill) => Ok(fill),
        None => Err(format!("unknown material '{name}'; use one of {}", builtin_material_names())),
    }
}

/// `terrain.fill`: fill a ball, block, cylinder or region with a material or
/// water, or with `carve` empty it to air.
pub fn terrain_fill(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let p = &req.params;
    let parsed = (|| -> Result<(TerrainCommand, &'static str), String> {
        let carve = opt_bool(p, "carve")?.unwrap_or(false);
        let fill = fill_of(opt_str(p, "material")?, carve)?;
        let label = if carve { "Carve Terrain" } else { "Fill Terrain" };
        Ok((fill_command(p, fill)?, label))
    })();
    match parsed {
        Ok((cmd, label)) => apply_one(world, req, cmd, label),
        Err(e) => refuse(req, e),
    }
}

/// `terrain.replace_material`: replace one material with another inside a
/// world box.
pub fn terrain_replace_material(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let p = &req.params;
    let parsed = (|| -> Result<TerrainCommand, String> {
        let (min, max) = box_of(p)?;
        let material = |key: &str| -> Result<TerrainMaterial, String> {
            let name = opt_str(p, key)?.ok_or_else(|| format!("`{key}` is required: a terrain material name"))?;
            builtin_material(name).ok_or_else(|| {
                format!("unknown material '{name}' in `{key}`; use one of {}", builtin_material_names())
            })
        };
        let (from, to) = (material("from")?, material("to")?);
        if from == to {
            return Err(format!("`from` and `to` are both {}", from.name()));
        }
        Ok(TerrainCommand::ReplaceMaterial { min, max, from, to })
    })();
    match parsed {
        Ok(cmd) => apply_one(world, req, cmd, "Replace Terrain Material"),
        Err(e) => refuse(req, e),
    }
}

// ---------------------------------------------------------------------------
// terrain.layer_create
// ---------------------------------------------------------------------------

/// The terrain layer classes a caller may create. A `TerrainSplinePoint` is
/// created with its spline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerClass {
    Spline,
    Stamp,
    FlattenPad,
    Noise,
    MaterialFill,
    Scatter,
    WaterBody,
}

impl LayerClass {
    const ALL: [LayerClass; 7] = [
        Self::Spline,
        Self::Stamp,
        Self::FlattenPad,
        Self::Noise,
        Self::MaterialFill,
        Self::Scatter,
        Self::WaterBody,
    ];

    fn class_name(self) -> ClassName {
        match self {
            Self::Spline => ClassName::TerrainSpline,
            Self::Stamp => ClassName::TerrainStamp,
            Self::FlattenPad => ClassName::TerrainFlattenPad,
            Self::Noise => ClassName::TerrainNoise,
            Self::MaterialFill => ClassName::TerrainMaterialFill,
            Self::Scatter => ClassName::TerrainScatter,
            Self::WaterBody => ClassName::TerrainWaterBody,
        }
    }

    fn name(self) -> &'static str {
        self.class_name().as_str()
    }

    fn parse(name: &str) -> Result<Self, String> {
        let wanted = name.trim();
        if wanted.eq_ignore_ascii_case(ClassName::TerrainSplinePoint.as_str()) {
            return Err("a TerrainSplinePoint belongs to a spline: create the TerrainSpline with `points`".to_string());
        }
        Self::ALL
            .into_iter()
            .find(|class| class.name().eq_ignore_ascii_case(wanted))
            .ok_or_else(|| {
                format!(
                    "unknown layer class '{wanted}'; expected one of {}",
                    Self::ALL.map(Self::name).join(", ")
                )
            })
    }

    /// Create the instance in `dir` with its properties set from `fields`, all
    /// or nothing.
    fn create(
        self,
        dir: &Path,
        name: Option<&str>,
        position: Vec3,
        fields: &[(String, String)],
    ) -> Result<CreatedInstance, String> {
        match self {
            Self::Spline => create_layer::<TerrainSpline>(dir, self, name, position, fields),
            Self::Stamp => create_layer::<TerrainStamp>(dir, self, name, position, fields),
            Self::FlattenPad => create_layer::<TerrainFlattenPad>(dir, self, name, position, fields),
            Self::Noise => create_layer::<TerrainNoise>(dir, self, name, position, fields),
            Self::MaterialFill => create_layer::<TerrainMaterialFill>(dir, self, name, position, fields),
            Self::Scatter => create_layer::<TerrainScatter>(dir, self, name, position, fields),
            Self::WaterBody => create_layer::<TerrainWaterBody>(dir, self, name, position, fields),
        }
    }
}

/// Create a layer instance of class `T` the way Insert does, through the
/// canonical `create_instance`, then write the properties `fields` sets.
///
/// Every field is parsed through the class's own field table
/// (`FieldTable::set_text`, what `terrain_layers::set_field_text` runs for a
/// Properties edit) before anything touches the disk, so a bad name or value
/// refuses the whole call and creates nothing. `set_field_text` itself needs
/// the live entity, which the file watcher spawns on a later frame.
fn create_layer<T: FieldTable + Default>(
    dir: &Path,
    class: LayerClass,
    name: Option<&str>,
    position: Vec3,
    fields: &[(String, String)],
) -> Result<CreatedInstance, String> {
    let mut component = T::default();
    let problems: Vec<String> = fields
        .iter()
        .filter_map(|(key, text)| component.set_text(key, text).err().map(|e| format!("{key}: {e}")))
        .collect();
    if !problems.is_empty() {
        let valid: Vec<&str> = T::FIELDS.iter().map(|f| f.name).collect();
        return Err(format!(
            "{} was not created: {}. Its properties are {}",
            class.name(),
            problems.join("; "),
            valid.join(", ")
        ));
    }
    let overrides = InstanceOverrides { position: Some(position), ..Default::default() };
    let created = create_instance(dir, class.name(), name, overrides)
        .map_err(|e| format!("{} was not created: {e}", class.name()))?;
    if !fields.is_empty() {
        if let Err(e) = crate::particles::bridge::save_class_section(&created.toml_path, &component) {
            // All or nothing: a layer without the properties it was asked for
            // is the wrong layer.
            let _ = std::fs::remove_dir_all(&created.folder_path);
            return Err(format!("{} was not created: its properties could not be written ({e})", class.name()));
        }
    }
    Ok(created)
}

/// `fields` as `(property, text)` pairs, each value in the text form the
/// Properties panel takes.
fn layer_fields(v: Option<&Value>) -> Result<Vec<(String, String)>, String> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(key, value)| field_text(key, value).map(|text| (key.clone(), text)))
            .collect(),
        Some(_) => Err("`fields` must be an object of property names to values, for example {\"Radius\": 30}".to_string()),
    }
}

fn field_text(key: &str, value: &Value) -> Result<String, String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(format!("`fields.{key}` must be a number, true or false, or a name")),
    }
}

/// A spline's control points, world positions in path order.
fn spline_points(v: &Value) -> Result<Vec<Vec3>, String> {
    let list = v.as_array().ok_or("`points` must be an array of [x, y, z] positions")?;
    if list.len() < 2 || list.len() > MAX_SPLINE_POINTS {
        return Err(format!("a spline takes 2 to {MAX_SPLINE_POINTS} points, got {}", list.len()));
    }
    list.iter()
        .enumerate()
        .map(|(i, p)| vec3_of(p).ok_or_else(|| format!("points[{i}] must be [x, y, z]: three finite numbers in world metres")))
        .collect()
}

/// The first two points Insert gives a spline when there is no view to aim
/// by: [`NEW_SPLINE_HALF_LENGTH`] either side of `position` along X, each on
/// the ground under it (at `position`'s height where there is no terrain).
fn default_spline_points(world: &World, position: Vec3) -> Vec<Vec3> {
    [-1.0f32, 1.0]
        .into_iter()
        .map(|side| {
            let at = position + Vec3::X * (side * NEW_SPLINE_HALF_LENGTH);
            let ground = with_terrain_read(world, |config, data, _, _| {
                (!data.height_cache.is_empty()).then(|| height_at_world(config, data, at.x, at.z))
            })
            .flatten()
            .unwrap_or(position.y);
            Vec3::new(at.x, ground, at.z)
        })
        .collect()
}

/// A parsed `terrain.layer_create` request.
struct LayerRequest {
    class: LayerClass,
    name: Option<String>,
    position: Vec3,
    fields: Vec<(String, String)>,
    points: Option<Vec<Vec3>>,
}

fn layer_request(p: &Value) -> Result<LayerRequest, String> {
    let class = opt_str(p, "class")?.ok_or_else(|| {
        format!("`class` is required: one of {}", LayerClass::ALL.map(LayerClass::name).join(", "))
    })?;
    let class = LayerClass::parse(class)?;
    let points = param(p, "points").map(spline_points).transpose()?;
    if points.is_some() && class != LayerClass::Spline {
        return Err(format!("`points` only applies to TerrainSpline, not {}", class.name()));
    }
    Ok(LayerRequest {
        class,
        name: opt_str(p, "name")?.map(str::to_owned),
        position: req_vec3(p, "position")?,
        fields: layer_fields(param(p, "fields"))?,
        points,
    })
}

/// `terrain.layer_create`: create a terrain layer instance under
/// `Workspace/Terrain/Layers` as Insert does, one undo step that removes its
/// folder. A spline's points go under it, numbered in path order.
pub fn terrain_layer_create(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let request = match layer_request(&req.params) {
        Ok(request) => request,
        Err(e) => return refuse(req, e),
    };
    let Some(space_root) = world.get_resource::<SpaceRoot>().map(|root| root.0.clone()) else {
        return refuse(req, NO_SPACE);
    };
    let LayerRequest { class, name, position, fields, points } = request;
    let points = match (class, points) {
        (LayerClass::Spline, None) => default_spline_points(world, position),
        (_, points) => points.unwrap_or_default(),
    };
    let created = match class.create(&layers_dir(&space_root), name.as_deref(), position, &fields) {
        Ok(created) => created,
        Err(e) => return refuse(req, e),
    };
    // Points sit under the spline, which Insert places unturned at `position`.
    let mut point_errors = Vec::new();
    for point in &points {
        if let Err(e) = crate::terrain_layers::append_spline_point(&created.folder_path, *point - position) {
            point_errors.push(e);
        }
    }
    let label = format!("Insert {}", class.name());
    if let Some(mut undo) = world.get_resource_mut::<crate::undo::UndoStack>() {
        undo.push_labeled(
            label.clone(),
            crate::undo::Action::spawn_folders(&space_root, &[created.folder_path.clone()]),
        );
    }
    let fields_set: Vec<&str> = fields.iter().map(|(key, _)| key.as_str()).collect();
    reply(
        req,
        json!({
            "created": true,
            "class": class.name(),
            "name": created.folder_name,
            "folder": created.folder_path.to_string_lossy(),
            "position": xyz_json(position),
            "fields_set": fields_set,
            "points": points.len() - point_errors.len(),
            "point_errors": point_errors,
            "undo_label": label,
            "note": "The file watcher spawns the instance on a following frame and the layer bakes over the next \
                     frames. One undo step removes it.",
        }),
    )
}

// ---------------------------------------------------------------------------
// terrain.clear
// ---------------------------------------------------------------------------

/// `terrain.clear`: delete the open Space's terrain ([`clear_terrain_world`]).
/// Irreversible, but for the layers.
///
/// Everything under `Workspace/Terrain` goes (raster, material maps, volume
/// bricks, custom material files, `_terrain.toml`, imported voxel chunks)
/// but two things. The Layers folder stays unless `include_layers`, which
/// sends it to the trash with an undo entry, as the Terrain ribbon's Clear
/// always does: it holds the layer instances someone authored. The Terrain
/// instance's own
/// `_instance.toml` stays too, rewritten with `[terrain] source = "none"`: a
/// converted Space keeps its imported voxels in the world database, which
/// cannot delete them, and that mark is what tells the voxel loader the
/// Terrain holds nothing, so the terrain does not come back on the next open.
/// Then every terrain root and chunk in the live world is despawned.
pub fn terrain_clear(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let include_layers = match opt_bool(&req.params, "include_layers") {
        Ok(flag) => flag.unwrap_or(false),
        Err(e) => return refuse(req, e),
    };
    let Some(space_root) = world.get_resource::<SpaceRoot>().map(|root| root.0.clone()) else {
        return refuse(req, "no Space is open, so nothing was cleared");
    };
    if generation_busy(world) {
        return refuse(
            req,
            "a terrain generation is running or queued, and clearing now would race it; wait until terrain_stats \
             reports generation.busy false",
        );
    }
    let TerrainClear { files, instance, roots, chunks, from_voxels, layers_trashed } =
        clear_terrain_world(world, &space_root, include_layers);
    info!(
        "terrain.clear: removed {} entries under Workspace/Terrain ({} failed), despawned {} roots and {} chunks",
        files.removed.len(),
        files.failed.len(),
        roots,
        chunks
    );

    let mut notes = vec![if layers_trashed {
        "Irreversible for the terrain itself; the layers went to the trash, and undo brings them back.".to_string()
    } else {
        "Irreversible: there is no undo.".to_string()
    }];
    if include_layers && !layers_trashed {
        notes.push("The Layers folder could not be moved to the trash, so the layers are still there.".to_string());
    }
    if from_voxels && !instance.marked {
        notes.push(
            "This terrain came from the world database's imported voxels and its Terrain instance could not be \
             marked cleared, so it loads again the next time the Space opens."
                .to_string(),
        );
    }
    if !files.failed.is_empty() {
        notes.push("Some entries could not be deleted; see `failed`.".to_string());
    }
    reply(
        req,
        json!({
            "cleared": true,
            "complete": files.failed.is_empty() && instance.error.is_none(),
            "removed": files.removed,
            "failed": files.failed,
            "include_layers": include_layers,
            "layers_removed": layers_trashed,
            "kept_instance_file": instance.kept,
            "marked_source_none": instance.marked,
            "instance_file_error": instance.error,
            "roots_despawned": roots,
            "chunks_despawned": chunks,
            "note": notes.join(" "),
        }),
    )
}

/// `Err` while Play runs: the Terrain panel's commands edit the Space
/// itself, and an edit made in Play is put back at Stop.
fn refuse_in_play(world: &World) -> Result<(), String> {
    let playing = world
        .get_resource::<State<crate::play_mode::PlayModeState>>()
        .is_some_and(|state| *state.get() != crate::play_mode::PlayModeState::Editing);
    if playing {
        return Err("Stop Play first: the Terrain panel edits the Space itself".to_string());
    }
    Ok(())
}

/// "Convert water paint" (the Terrain panel): turn the open terrain's ground
/// painted Water into seabed under real water, one undo entry
/// ([`eustress_common::terrain::voxel_water::convert_water_paint`] says how
/// the levels are guessed). Refused while Play runs or a generation is
/// under way.
pub(crate) fn convert_water_paint_world(
    world: &mut World,
) -> Result<eustress_common::terrain::voxel_water::WaterPaintConversion, String> {
    use eustress_common::terrain::voxel_water::convert_water_paint;
    use eustress_common::terrain::{TerrainDirtyChunks, TerrainEditRecorder};

    refuse_in_play(world)?;
    if generation_busy(world) {
        return Err("a terrain generation is running; convert once it finishes".to_string());
    }
    let root = world
        .query_filtered::<Entity, With<TerrainRoot>>()
        .iter(world)
        .next()
        .ok_or_else(|| NO_TERRAIN.to_string())?;
    // A root without water gets it only once something was converted.
    let mut new_water: Option<TerrainVoxelWater> = None;
    let (config, done, recorded) = {
        let mut query = world
            .query_filtered::<(&TerrainConfig, &mut TerrainData, Option<&mut TerrainVoxelWater>), With<TerrainRoot>>();
        let (config, mut data, mut water) = query.get_mut(world, root).map_err(|_| NO_TERRAIN.to_string())?;
        let config = config.clone();
        let mut recorder = TerrainEditRecorder::default();
        // Only a raster the recorder can tile is converted, so the
        // conversion always has its undo entry.
        if !recorder.begin("Convert Water Paint", Some(root), &config, &data) {
            return Err("this terrain's raster cannot be recorded for undo, so nothing was converted".to_string());
        }
        let done = match water.as_mut() {
            Some(water) => convert_water_paint(&config, data.bypass_change_detection(), water.bypass_change_detection(), Some(&mut recorder)),
            None => {
                let fresh = new_water.get_or_insert_with(TerrainVoxelWater::default);
                convert_water_paint(&config, data.bypass_change_detection(), fresh, Some(&mut recorder))
            }
        };
        if done.cells > 0 {
            data.set_changed();
            if let Some(water) = water.as_mut() {
                water.set_changed();
            }
        }
        let after = water.as_deref().or(new_water.as_ref());
        let recorded = recorder.finish_with_water(Some(root), &data, TerrainVolume::empty(), after);
        (config, done, recorded)
    };
    if let Some(fresh) = new_water.filter(|_| done.cells > 0) {
        world.entity_mut(root).insert(fresh);
    }
    if let (Some((lo, hi)), Some(mut dirty)) = (done.bounds, world.get_resource_mut::<TerrainDirtyChunks>()) {
        dirty.mark_world_rect_materials(&config, lo, hi);
    }
    if let (Some(edit), Some(mut undo)) = (recorded, world.get_resource_mut::<crate::undo::UndoStack>()) {
        undo.push_labeled(
            edit.label.clone(),
            crate::undo::Action::TerrainEdit {
                label: edit.label,
                root: root.to_bits(),
                tiles: edit.tiles,
                bricks: edit.bricks,
                water: edit.water,
            },
        );
    }
    Ok(done)
}

/// "Regenerate layers from seed" (the Terrain panel): write the seed's
/// scatter and lake layers for the open terrain as it stands, in place of
/// the ones a generation or an earlier regeneration wrote
/// ([`eustress_common::terrain::worldgen::default_layers::regenerate_default_layers`]);
/// layers made any other way stay. The file watcher loads what it writes. The
/// seed is the terrain's, and the sea level the ocean's.
pub(crate) fn regenerate_layers_world(
    world: &mut World,
) -> Result<eustress_common::terrain::worldgen::default_layers::DefaultLayersSummary, String> {
    use eustress_common::terrain::worldgen::default_layers::regenerate_default_layers;

    refuse_in_play(world)?;
    if generation_busy(world) {
        return Err("a terrain generation is running; it writes its own layers".to_string());
    }
    let space_root = world
        .get_resource::<SpaceRoot>()
        .map(|root| root.0.clone())
        .ok_or_else(|| "no Space is open".to_string())?;
    let sea_level = world.get_resource::<eustress_common::terrain::WaterConfig>().map_or(0.0, |ocean| ocean.sea_level);
    let mut query = world.query_filtered::<(&TerrainConfig, &TerrainData), With<TerrainRoot>>();
    let (config, data) = query.iter(world).next().ok_or_else(|| NO_TERRAIN.to_string())?;
    if data.height_cache.is_empty() {
        return Err("this terrain has no height raster to read layers off".to_string());
    }
    regenerate_default_layers(&space_root, config, data, u64::from(config.seed), f64::from(sea_level))
}

/// What [`clear_terrain_world`] did.
#[derive(Debug, Default)]
pub(crate) struct TerrainClear {
    pub(crate) files: ClearedFiles,
    pub(crate) instance: InstanceMark,
    /// Terrain roots and chunks despawned.
    pub(crate) roots: usize,
    pub(crate) chunks: usize,
    /// A cleared root came from the world database's imported voxels.
    pub(crate) from_voxels: bool,
    /// The Layers folder went to the trash, with an undo entry to bring it
    /// back.
    pub(crate) layers_trashed: bool,
}

/// Clear the open Space's terrain: `Workspace/Terrain`'s files
/// ([`clear_terrain_files`]), the Terrain instance marked cleared
/// ([`mark_terrain_instance_cleared`]), every terrain root and chunk
/// despawned, and the ocean turned off, since the `[water]` it was read from
/// went with `_terrain.toml`. With `trash_layers`, the Layers folder (roads,
/// stamps, pads, fills, scatter, lakes) goes to the trash as a Delete sends
/// it, its instances despawned, and one undo entry brings it back. The
/// Terrain ribbon's Clear and the bridge's `terrain.clear` share this.
pub(crate) fn clear_terrain_world(world: &mut World, space_root: &Path, trash_layers: bool) -> TerrainClear {
    let roots: Vec<Entity> = world.query_filtered::<Entity, With<TerrainRoot>>().iter(world).collect();
    let chunks: Vec<Entity> = world.query_filtered::<Entity, With<Chunk>>().iter(world).collect();
    let from_voxels = roots.iter().any(|&root| voxel_sourced(world, root));

    let files = clear_terrain_files(space_root);
    let instance = mark_terrain_instance_cleared(space_root);

    let mut layers_trashed = false;
    let layers = layers_dir(space_root);
    if trash_layers && layers.exists() {
        let trash = crate::undo::Action::reserve_trash_path(space_root, &layers);
        if crate::undo::trash_created_folder(world, &layers, &trash, &[]) {
            layers_trashed = true;
            if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
                stack.push_labeled(
                    "Clear Terrain layers",
                    crate::undo::Action::TrashEntities { paths: vec![(layers, trash)] },
                );
            }
        } else {
            warn!("Clear Terrain: the Layers folder at {} would not move to the trash", layers.display());
        }
    }

    // A chunk goes with its root; the rest are strays.
    for &entity in roots.iter().chain(&chunks) {
        let _ = world.try_despawn(entity);
    }
    if let Some(mut water) = world.get_resource_mut::<eustress_common::terrain::WaterConfig>() {
        if water.enabled {
            water.enabled = false;
        }
    }
    if let Some(mut explorer) = world.get_resource_mut::<crate::ui::slint_ui::UnifiedExplorerState>() {
        explorer.dirty = true;
    }
    TerrainClear { files, instance, roots: roots.len(), chunks: chunks.len(), from_voxels, layers_trashed }
}

/// What clearing `Workspace/Terrain` removed and what it could not.
#[derive(Debug, Default)]
pub(crate) struct ClearedFiles {
    pub(crate) removed: Vec<String>,
    pub(crate) failed: Vec<String>,
}

/// Remove everything under the Space's `Workspace/Terrain` except the Terrain
/// instance's `_instance.toml` and the Layers folder, whose instances
/// [`clear_terrain_world`] sends to the trash instead.
///
/// The raster folders can hold thousands of files, too many to delete inside
/// a frame, so each top-level entry is renamed into a staging folder under
/// `.eustress/trash` (one rename apiece, and the file watcher ignores
/// `.eustress`) that a worker thread then deletes. Staging empties the
/// terrain folder at once, so a generation started right after cannot race
/// the deletion. An entry that will not rename is deleted in place.
pub(crate) fn clear_terrain_files(space_root: &Path) -> ClearedFiles {
    let mut out = ClearedFiles::default();
    let terrain_dir = space_root.join("Workspace").join("Terrain");
    let entries = match std::fs::read_dir(&terrain_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            out.failed.push(format!("{}: {e}", terrain_dir.display()));
            return out;
        }
    };
    let staging = space_root
        .join(".eustress")
        .join("trash")
        .join(format!("terrain_clear_{}", chrono::Utc::now().format("%Y%m%d_%H%M%S_%f")));
    let can_stage = std::fs::create_dir_all(&staging).is_ok();
    let mut staged = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == std::ffi::OsStr::new(TERRAIN_INSTANCE_FILE) {
            continue;
        }
        if name == std::ffi::OsStr::new(LAYERS_FOLDER) {
            continue;
        }
        let path = entry.path();
        let label = name.to_string_lossy().into_owned();
        if can_stage && std::fs::rename(&path, staging.join(&name)).is_ok() {
            staged += 1;
            out.removed.push(label);
            continue;
        }
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let removed = if is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        match removed {
            Ok(()) => out.removed.push(label),
            Err(e) => out.failed.push(format!("{}: {e}", path.display())),
        }
    }
    if staged > 0 {
        if crate::space::active_db::is_active() {
            forget_staged_in_db(&staging, &terrain_dir);
        }
        std::thread::spawn(move || {
            if let Err(e) = std::fs::remove_dir_all(&staging) {
                warn!("terrain.clear: the staged terrain files at {} could not be deleted: {e}", staging.display());
            }
        });
    } else if can_stage {
        let _ = std::fs::remove_dir(&staging);
    }
    out
}

/// Drop the world database's copy of every file under `staged`, named by
/// where it lived under `original`: a converted Space mirrors loose files
/// (material maps among them) in its file tree. A file deleted in place gets
/// this from the file watcher; a folder renamed away reports no per-file
/// removals, so the clear does it before it answers.
fn forget_staged_in_db(staged: &Path, original: &Path) {
    let Ok(entries) = std::fs::read_dir(staged) else {
        return;
    };
    for entry in entries.flatten() {
        let (from, to) = (entry.path(), original.join(entry.file_name()));
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            forget_staged_in_db(&from, &to);
        } else {
            crate::space::active_db::delete_tree_file(&to);
        }
    }
}

/// What a clear did to the Terrain instance's `_instance.toml`.
#[derive(Debug, Default)]
pub(crate) struct InstanceMark {
    /// The file exists (on disk, or only in the world database) and was kept.
    pub(crate) kept: bool,
    /// Its `[terrain] source` now reads "none".
    pub(crate) marked: bool,
    /// Why it could not be marked.
    pub(crate) error: Option<String>,
}

/// Rewrite the Terrain instance's `Workspace/Terrain/_instance.toml` with
/// `[terrain] source = "none"`, the importer's value for a Terrain without
/// terrain, which the voxel loader skips. It is read where the loader reads
/// it: the disk file, else the world database's copy for a Space whose loose
/// files live only there. A file that does not parse is left untouched.
pub(crate) fn mark_terrain_instance_cleared(space_root: &Path) -> InstanceMark {
    let path = space_root.join("Workspace").join("Terrain").join(TERRAIN_INSTANCE_FILE);
    let (text, on_disk) = match std::fs::read_to_string(&path) {
        Ok(text) => (text, true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match crate::space::active_db::get_instance_text(&path) {
                Some(text) => (text, false),
                None => return InstanceMark::default(),
            }
        }
        Err(e) => {
            return InstanceMark { kept: true, marked: false, error: Some(format!("{}: {e}", path.display())) };
        }
    };
    let mut mark = InstanceMark { kept: true, ..InstanceMark::default() };
    match mark_source_none(&text) {
        Ok(marked) => {
            // The database copy is kept in step either way, as a class
            // section save does; the loader reads the disk file first.
            let in_db = crate::space::active_db::put_instance_text(&path, &marked);
            if on_disk {
                match crate::space::gui_loader::write_atomic(&path, marked.as_bytes()) {
                    Ok(()) => mark.marked = true,
                    Err(e) => mark.error = Some(format!("{}: {e}", path.display())),
                }
            } else if in_db {
                mark.marked = true;
            } else {
                mark.error = Some(format!("{}: the world database refused the rewrite", path.display()));
            }
        }
        Err(e) => mark.error = Some(format!("{}: {e}; left untouched", path.display())),
    }
    mark
}

/// `text`, a Terrain instance's TOML, with `[terrain] source = "none"`.
fn mark_source_none(text: &str) -> Result<String, String> {
    let mut doc: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("does not parse ({e})"))?;
    let terrain = doc
        .entry("terrain")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(table) = terrain.as_table_mut() else {
        return Err("its `terrain` key is not a table".to_string());
    };
    table.insert("source".to_string(), toml::Value::String("none".to_string()));
    toml::to_string_pretty(&doc).map_err(|e| format!("cannot be written back ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::terrain::{builtin_slot, MaterialSlot};

    #[test]
    fn fill_shapes_parse_by_name() {
        assert_eq!(FillShape::parse("Ball"), Ok(FillShape::Ball));
        assert_eq!(FillShape::parse("sphere"), Ok(FillShape::Ball));
        assert_eq!(FillShape::parse(" block "), Ok(FillShape::Block));
        assert_eq!(FillShape::parse("box"), Ok(FillShape::Block));
        assert_eq!(FillShape::parse("CYLINDER"), Ok(FillShape::Cylinder));
        assert_eq!(FillShape::parse("region"), Ok(FillShape::Region));
        assert!(FillShape::parse("wedge").is_err());
    }

    #[test]
    fn fills_build_the_command_their_shape_names() {
        let rock = TerrainFill::Material(TerrainMaterial::Rock);
        let ball = fill_command(&json!({"shape": "ball", "center": [1, 2, 3], "radius": 4}), rock).unwrap();
        assert_eq!(ball, TerrainCommand::FillBall { center: Vec3::new(1.0, 2.0, 3.0), radius: 4.0, fill: rock });
        // A region takes its corners in either order.
        let region = fill_command(
            &json!({"shape": "region", "min": [10, 0, 10], "max": [0, 5, 0]}),
            TerrainFill::Water,
        )
        .unwrap();
        assert_eq!(
            region,
            TerrainCommand::FillRegion { min: Vec3::ZERO, max: Vec3::new(10.0, 5.0, 10.0), fill: TerrainFill::Water }
        );
        // Missing and oversized parameters are refused, never trimmed.
        let air = TerrainFill::Air;
        assert!(fill_command(&json!({"shape": "ball", "center": [0, 0, 0]}), air).is_err());
        assert!(fill_command(&json!({"shape": "ball", "center": [0, 0, 0], "radius": 300}), air).is_err());
        assert!(fill_command(&json!({"shape": "block", "center": [0, 0, 0], "size": [600, 1, 1]}), air).is_err());
        assert!(fill_command(&json!({"shape": "block", "center": [0, 0, 0], "size": [1, 0, 1]}), air).is_err());
        assert!(fill_command(&json!({"shape": "cylinder", "center": [0, 0, 0], "radius": 2}), air).is_err());
        assert!(fill_command(&json!({"shape": "region", "min": [0, 0, 0], "max": [0, 5, 5]}), air).is_err());
        assert!(fill_command(&json!({"center": [0, 0, 0], "radius": 2}), air).is_err());
    }

    #[test]
    fn a_fill_only_carves_when_asked_to() {
        assert!(fill_of(Some("Air"), false).is_err());
        assert!(fill_of(Some("Enum.Material.Air"), false).is_err());
        assert_eq!(fill_of(Some("air"), true), Ok(TerrainFill::Air));
        assert_eq!(fill_of(None, true), Ok(TerrainFill::Air));
        assert!(fill_of(Some("Grass"), true).is_err());
        assert_eq!(fill_of(Some("Grass"), false), Ok(TerrainFill::Material(TerrainMaterial::Grass)));
        assert_eq!(fill_of(Some("Water"), false), Ok(TerrainFill::Water));
        assert!(fill_of(None, false).is_err());
        assert!(fill_of(Some("Unobtainium"), false).is_err());
    }

    #[test]
    fn material_names_take_roblox_enum_prefixes() {
        assert_eq!(builtin_material("Enum.Material.Rock"), Some(TerrainMaterial::Rock));
        assert_eq!(builtin_material("material.wood planks"), Some(TerrainMaterial::WoodPlanks));
        assert_eq!(builtin_material(" LeafyGrass "), Some(TerrainMaterial::LeafyGrass));
        assert_eq!(builtin_material("Air"), None);
        assert_eq!(material_slot(None, "Sand"), Ok(TerrainMaterial::Sand.to_u8()));
        assert!(material_slot(None, "Unobtainium").is_err());
        // A Space's own material resolves to its custom slot.
        let mut slots = TerrainMaterialSlots::builtins();
        let moss = MaterialSlot { name: "Moss".to_string(), ..builtin_slot(TerrainMaterial::Grass) };
        slots.set_slot(FIRST_CUSTOM_MATERIAL_SLOT, Some(moss));
        assert_eq!(material_slot(Some(&slots), "moss"), Ok(FIRST_CUSTOM_MATERIAL_SLOT));
        assert_eq!(slot_name(Some(&slots), FIRST_CUSTOM_MATERIAL_SLOT), "Moss");
        assert_eq!(slot_name(None, TerrainMaterial::Rock.to_u8()), "Rock");
    }

    #[test]
    fn presets_match_the_terrain_ribbon() {
        assert_eq!(world_preset("small"), Ok((2, 384)));
        assert_eq!(world_preset("Medium"), Ok((3, 320)));
        assert_eq!(world_preset("large"), Ok((4, 288)));
        assert!(world_preset("huge").is_err());

        assert_eq!(flat_half_extent("small"), Ok(2));
        assert_eq!(flat_half_extent("medium"), Ok(4));
        assert_eq!(flat_half_extent("large"), Ok(8));
        assert!(flat_half_extent("flat").is_err());

        // At the default height the plate is the ribbon's `flat_preset`.
        let plate = flat_spec(4, 0.0, TerrainMaterial::Grass.to_u8()).unwrap();
        assert_eq!((plate.half_extent, plate.chunk_size, plate.chunk_resolution), (4, 64.0, 64));
        assert_eq!((plate.height_m, plate.height_offset, plate.height_scale), (0.0, -32.0, 128.0));
        assert_eq!((plate.material_slot, plate.seed), (0, 0));
        assert_eq!(plate.total_extent_m(), 576.0);
        // Elsewhere the band moves with the plate: 32 m to dig, 96 m to build.
        let raised = flat_spec(2, 50.0, TerrainMaterial::Rock.to_u8()).unwrap();
        assert_eq!((raised.height_offset, raised.height_offset + raised.height_scale), (18.0, 146.0));
        assert!(flat_spec(2, 20_000.0, 0).is_err());
        assert!(flat_spec(2, 0.0, MATERIAL_SLOT_NONE).is_err());
    }

    #[test]
    fn queries_are_capped_at_ten_thousand_samples() {
        let grid = query_samples(&json!({"grid": {"min": [0, 0], "max": [99, 99], "step": 1}})).unwrap();
        assert_eq!(grid.positions.len(), MAX_QUERY_SAMPLES);
        assert!(query_samples(&json!({"grid": {"min": [0, 0], "max": [100, 100], "step": 1}})).is_err());
        let points: Vec<Value> = (0..=MAX_QUERY_SAMPLES).map(|i| json!([i, 0])).collect();
        assert!(query_samples(&json!({ "points": points })).is_err());
        assert!(query_samples(&json!({})).is_err());
        assert!(query_samples(&json!({"points": [[0, 0]], "grid": {"min": [0, 0], "max": [1, 1], "step": 1}})).is_err());
        assert!(query_samples(&json!({"grid": {"min": [0, 0], "max": [1, 1], "step": 0}})).is_err());
        assert!(query_samples(&json!({"points": [[0, "north"]]})).is_err());
    }

    #[test]
    fn a_grid_runs_x_fastest_from_its_low_corner() {
        let samples = query_samples(&json!({"grid": {"min": [10, 5], "max": [0, 0], "step": 5}})).unwrap();
        let grid = samples.grid.expect("a grid query reports its layout");
        assert_eq!((grid.nx, grid.nz), (3, 2));
        let expected = [(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]
            .map(|(x, z)| Vec2::new(x, z))
            .to_vec();
        assert_eq!(samples.positions, expected);
        // A point may carry a height, which is ignored.
        let points = query_samples(&json!({"points": [[1, 2], [3, 99, 4]]})).unwrap();
        assert_eq!(points.positions, vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)]);
    }

    #[test]
    fn rotations_take_quaternions_or_roblox_euler_degrees() {
        let turned = parse_rotation(&json!([0, 90, 0])).unwrap();
        assert!((turned * Vec3::X - Vec3::NEG_Z).length() < 1e-5, "90 degrees about Y takes +X to -Z");
        let euler = parse_rotation(&json!([30, 45, 60])).unwrap();
        let expected = Quat::from_rotation_x(30f32.to_radians())
            * Quat::from_rotation_y(45f32.to_radians())
            * Quat::from_rotation_z(60f32.to_radians());
        assert!(euler.angle_between(expected) < 1e-4, "X, then the turned Y, then the turned Z");
        let quat = parse_rotation(&json!([0, 0, 0, 2])).unwrap();
        assert!(quat.abs_diff_eq(Quat::IDENTITY, 1e-6), "a quaternion is normalized");
        assert!(parse_rotation(&json!([0, 0, 0, 0])).is_err());
        assert!(parse_rotation(&json!([1, 2])).is_err());
        assert!(parse_rotation(&json!("up")).is_err());
    }

    #[test]
    fn voxel_reads_are_capped_at_32768_voxels() {
        assert_eq!(voxel_grid(Vec3::ZERO, Vec3::splat(32.0), 1.0), Ok(UVec3::splat(32)));
        // A side that is not a whole number of voxels rounds up.
        assert_eq!(voxel_grid(Vec3::ZERO, Vec3::new(2.5, 1.0, 1.0), 1.0), Ok(UVec3::new(3, 1, 1)));
        assert!(voxel_grid(Vec3::ZERO, Vec3::new(33.0, 32.0, 32.0), 1.0).is_err());
        assert!(voxel_grid(Vec3::ZERO, Vec3::ONE, 0.0).is_err());
        assert!(voxel_grid(Vec3::ZERO, Vec3::splat(1e9), 1e-6).is_err());
    }

    #[test]
    fn a_fill_counts_the_lattice_points_its_box_covers() {
        assert_eq!(lattice_points(Vec3::ZERO, Vec3::splat(10.0), 1.0), 11 * 11 * 11);
        assert_eq!(lattice_points(Vec3::ZERO, Vec3::splat(10.0), 2.0), 6 * 6 * 6);
        assert_eq!(lattice_points(Vec3::ONE, Vec3::ZERO, 1.0), 1);
        assert_eq!(lattice_points(Vec3::ZERO, Vec3::splat(f32::INFINITY), 1.0), u64::MAX);
        let ball = TerrainCommand::FillBall { center: Vec3::ZERO, radius: 1.0, fill: TerrainFill::Air };
        let pond = TerrainCommand::FillBall { center: Vec3::ZERO, radius: 1.0, fill: TerrainFill::Water };
        assert!(visits_volume(&ball));
        assert!(!visits_volume(&pond), "water only raises the water surface");
    }

    #[test]
    fn brush_sizes_and_strengths_are_bounded() {
        assert_eq!(radius_of(&json!({"radius": 256}), "radius"), Ok(256.0));
        assert!(radius_of(&json!({"radius": 256.5}), "radius").is_err());
        assert!(radius_of(&json!({"radius": 0}), "radius").is_err());
        assert!(radius_of(&json!({}), "radius").is_err());
        assert_eq!(strength_of(&json!({}), 0.5), Ok(0.5));
        assert!(strength_of(&json!({"strength": 1.5}), 0.5).is_err());
        assert_eq!(sculpt_mode("Flatten", 12.0), Ok(TerrainSculptMode::Flatten { height: 12.0 }));
        assert_eq!(sculpt_mode("raise", 0.0), Ok(TerrainSculptMode::Raise));
        assert!(sculpt_mode("erode", 0.0).is_err());
    }

    #[test]
    fn layer_classes_parse_by_name_and_refuse_points() {
        assert_eq!(LayerClass::parse("terrainstamp"), Ok(LayerClass::Stamp));
        assert_eq!(LayerClass::parse("TerrainWaterBody"), Ok(LayerClass::WaterBody));
        let point = LayerClass::parse("TerrainSplinePoint").unwrap_err();
        assert!(point.contains("points"), "{point}");
        let unknown = LayerClass::parse("TerrainCloud").unwrap_err();
        assert!(unknown.contains("TerrainScatter"), "the refusal lists the classes: {unknown}");
        for class in LayerClass::ALL {
            assert_eq!(LayerClass::parse(class.name()), Ok(class));
        }
        assert!(layer_request(&json!({"class": "TerrainStamp", "position": [0, 0, 0], "points": [[0, 0, 0], [1, 0, 0]]})).is_err());
        assert!(layer_request(&json!({"class": "TerrainSpline", "position": [0, 0, 0], "points": [[0, 0, 0]]})).is_err());
        assert!(layer_request(&json!({"class": "TerrainStamp"})).is_err(), "a position is required");
    }

    #[test]
    fn layer_fields_become_panel_text_the_field_table_takes() {
        let fields =
            layer_fields(Some(&json!({"Radius": 30, "Shape": "Crater", "Enabled": false, "Strength": 0.5}))).unwrap();
        let text: BTreeMap<&str, &str> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(text["Radius"], "30");
        assert_eq!(text["Shape"], "Crater");
        assert_eq!(text["Enabled"], "false");
        assert_eq!(text["Strength"], "0.5");
        assert!(layer_fields(Some(&json!({"Radius": null}))).is_err());
        assert!(layer_fields(Some(&json!([1, 2]))).is_err());
        assert!(layer_fields(None).unwrap().is_empty());
        let mut stamp = TerrainStamp::default();
        for (key, value) in &fields {
            stamp.set_text(key, value).unwrap();
        }
        assert_eq!((stamp.radius, stamp.enabled, stamp.strength), (30.0, false, 0.5));
    }

    #[test]
    fn a_layer_with_a_bad_field_is_not_created() {
        let dir = std::env::temp_dir().join("eustress_terrain_bridge_bad_field");
        let fields = vec![("Radius".to_string(), "30".to_string()), ("Colour".to_string(), "red".to_string())];
        let refusal = LayerClass::Stamp.create(&dir, None, Vec3::ZERO, &fields).unwrap_err();
        assert!(refusal.contains("Colour") && refusal.contains("Radius, "), "{refusal}");
        assert!(!dir.exists(), "nothing was written");
    }

    #[test]
    fn a_hole_is_a_sparse_cell_without_material() {
        let mut data = TerrainData::default();
        data.material_cache = vec![[0, MATERIAL_SLOT_NONE, 0, 0], [MATERIAL_SLOT_NONE; 4]];
        assert!(!is_hole(&data, Some(1)), "only a sparse raster has holes");
        data.sparse_surface = true;
        assert!(!is_hole(&data, Some(0)));
        assert!(is_hole(&data, Some(1)));
        assert!(!is_hole(&data, Some(7)), "a cell past the material layer is not a hole");
        assert!(!is_hole(&data, None));
    }

    #[test]
    fn a_cleared_terrain_instance_reads_source_none() {
        let text = "[metadata]\nclass_name = \"Terrain\"\nunit = \"ft\"\n\n[terrain]\nsource = \"imported\"\n\
                    water_transparency = 0.3\n";
        let doc: toml::Table = mark_source_none(text).unwrap().parse().unwrap();
        assert_eq!(doc["terrain"]["source"].as_str(), Some("none"));
        assert_eq!(doc["terrain"]["water_transparency"].as_float(), Some(0.3));
        assert_eq!(doc["metadata"]["unit"].as_str(), Some("ft"), "the unit stamp survives");
        // A Terrain without a [terrain] table gains one.
        let bare: toml::Table = mark_source_none("[metadata]\nclass_name = \"Terrain\"\n").unwrap().parse().unwrap();
        assert_eq!(bare["terrain"]["source"].as_str(), Some("none"));
        // Anything else is refused, which leaves the file untouched.
        assert!(mark_source_none("[terrain\nsource = ").is_err());
        assert!(mark_source_none("terrain = 3\n").is_err());
    }

    #[test]
    fn numbers_reach_json_without_their_f32_expansion() {
        assert_eq!(num(0.1f32, 3), 0.1);
        assert_eq!(num(12.3456f32, 2), 12.35);
        assert_eq!(json!(num(f32::NAN, 3)), Value::Null);
    }
}
