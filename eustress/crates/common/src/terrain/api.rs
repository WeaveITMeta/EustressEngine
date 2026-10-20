//! Terrain edits as commands: the edits the MCP terrain tools, the Luau
//! `workspace.Terrain` methods and the Rune `eustress::terrain` module make,
//! and the voxel read they share.
//!
//! A [`TerrainCommand`] is one edit in world metres. [`apply_terrain_command`]
//! applies it to a root's base data, never its layer bake (the bake re-bakes
//! over the edit), and reports what changed as a [`TerrainCommandEffect`] for
//! the host to mark in `TerrainDirtyChunks`. [`command_bounds`] is the box a
//! command can touch, and [`record_terrain_command`] records it in a
//! `TerrainEditRecorder` before the command runs, so a batch undoes as one
//! entry. The engine's `terrain_commands` module drives all of this against
//! the live world.
//!
//! ## Fills
//!
//! A fill of solid material is a volume Add of its shape in that material
//! (see `volume::apply_shape`); an Air fill is a Carve, whose walls take the
//! material nearest the shape's centre: an earlier edit's there, else the
//! ground's surface material under it, else Rock. FillBall is a sphere,
//! FillRegion an axis box, FillBlock an oriented box and FillCylinder an
//! oriented cylinder along its local Y, each rotation normalized first. Fills
//! leave the heightfield alone. Water fills raise the water instead.
//!
//! ## Water
//!
//! Water lives in the root's `TerrainVoxelWater`, one surface height per
//! raster cell. A Water fill raises the surface over the shape's footprint
//! (the cells whose centres the shape covers from above) to the shape's top
//! over each cell, and never lowers it. A root without the component needs
//! one before the fill (the engine inserts it); this module sizes it to the
//! raster, every cell dry, and levels built for another raster are dropped
//! the same way. An Air fill lowers the surface where it reaches it: a cell
//! whose surface the shape spans drops to the shape's bottom there, and dries
//! out when that bottom is at or below its ground. Water voxels set their
//! columns likewise: see Voxels. A column holds one surface, so there is no
//! air pocket under water.
//!
//! FillWater and DrainWater touch water alone, the way Roblox's
//! `ReplaceMaterial` between Air and Water does (and the Sea Level tool with
//! it). FillWater puts water in the air of its box: every column of the box's
//! rectangle whose ground stands below the box's top (or that has no ground)
//! rises to that top, and none falls; ground at or above the top stays dry.
//! Its water reaches down to the ground, since a column holds one surface.
//! DrainWater lowers the water inside its box the way an Air fill does,
//! without carving: a surface the box spans drops to the box's bottom, drying
//! out where that bottom is at or below the ground.
//!
//! ## Voxels
//!
//! [`read_voxels`] samples the combined field (heightfield, adds and carves,
//! see `volume`) at every voxel centre and maps its signed distance `f`,
//! negative inside, to an occupancy over one voxel:
//! `clamp(0.5 - f / resolution, 0, 1)`. A centre half a voxel deep reads full
//! and one half a voxel clear reads empty. A voxel with any occupancy takes
//! the material that makes it solid, as the meshes colour that surface: the
//! material map's where the heightfield term wins (Grass on a terrain without
//! one), the edits' (`volume::material_at`) where an add or carve does (Rock
//! where no edit carries one). An empty voxel its column's water covers reads
//! Water, its occupancy the share of it under the surface; any other empty
//! voxel reads Air. A column the terrain has no ground in, a hole of a sparse
//! surface or anywhere off the raster, has no heightfield term. A custom
//! material slot reads as Grass, since a fill names only built-in materials.
//!
//! WriteVoxels is its inverse. Every lattice point that carries trilinear
//! weight at some voxel centre is written. Its target occupancy is the voxel
//! grid interpolated there (trilinear between voxel centres, the edge voxels'
//! values beyond the outermost ones), counting Air and Water voxels as empty,
//! and its target distance `d = (0.5 - occupancy) * resolution`, which it
//! stores as `A = d` and `C = -d`. The field `max(min(fh, A), -C)` is then
//! exactly `d` at the point whatever the heightfield `fh` there, and between
//! written points, where both fields interpolate the same targets, the
//! trilinear interpolation of `d`: the region holds the voxels' surface and
//! nothing of the ground it replaced, and every surface in it takes the
//! edits' material. Each written point takes the material of the nearest
//! solid voxel among the eight around it (no material when all eight are
//! empty). A voxel centre on a lattice point (a resolution of whole lattice
//! cells, `min` half a voxel off the lattice) reads back its occupancy to the
//! distance quantization of 1/32 cell; elsewhere it reads the lattice's
//! reconstruction of the grid. Stored distances saturate at about four
//! cells, so voxels wider than eight cells no longer read back quite full or
//! empty. Points past the region keep their edits, and the field blends into
//! them over one lattice cell.
//!
//! The water voxels of a column set its water surface to the top of the
//! highest one (its occupancy of it, measured from its floor), over every
//! raster cell whose centre lies in the column; water reaching the region's
//! top that stands higher above the region keeps its surface. A column of the
//! region without water voxels whose surface lies inside the region drops to
//! the region's floor, drying out when that floor is at or below its ground.
//!
//! ## Sculpt and Paint
//!
//! Sculpt is one dab of the editor's heightfield brush
//! (`editor::apply_heightfield_brush`) with its default falloff, which moves
//! the ground continuously rather than in steps: Raise and Lower move it by
//! `strength` times a tenth of the height band at the centre, Smooth and
//! Flatten blend it toward its neighbours' mean or toward `height` by
//! `strength` there, and every mode fades to nothing at the rim. Paint lays material slot `material` into every raster cell whose
//! centre lies in the disc (`height_query::paint_material_at_world`), at
//! `strength` times the same falloff; it leaves the holes of a sparse surface
//! alone, since a material would give them ground. Both clamp `strength` to
//! `0..=1` and write the base raster in world metres.
//!
//! ## Clear
//!
//! Clear makes the surface sparse and every cell a hole (no material), and
//! empties the volume and the water: no ground is left. It cannot be undone,
//! so undoable hosts refuse it.
//!
//! ## Refusals
//!
//! A command is refused, changing nothing, on procedural terrain (no height
//! raster), for a number that is not finite, a radius, size, height or
//! resolution that is not positive, a rotation of zero length, a voxel region
//! of no voxels or of more than [`MAX_SCRIPT_VOXELS`], voxel arrays that do not
//! hold one entry per voxel, a Paint of the no-material slot, a solid or air
//! shape or voxel region covering more lattice points than one volume edit
//! may visit (`volume::MAX_EDIT_LATTICE_POINTS`), and water for a root
//! without a water component.

use bevy::prelude::*;

use super::config::{TerrainConfig, TerrainData};
use super::editor::{apply_heightfield_brush, BrushDab, BrushMode};
use super::height_query::{material_at_world, paint_material_at_world};
use super::history::TerrainEditRecorder;
use super::material::{canonical_material_cell, TerrainMaterial, MATERIAL_SLOT_NONE};
use super::volume::{
    apply_shape, brick_cell_index, brick_lattice_origin, heightfield_term, lattice_cell_size, lattice_point_world,
    lattice_to_brick, material_at, quantize_distance, shape_local, CsgOp, CsgShape, FieldSample, FieldTerm,
    TerrainVolume, VolumeCell, VolumeEdit, BRICK_EDGE, MATERIAL_NONE, MAX_EDIT_LATTICE_POINTS, Q_NONE,
};
use super::voxel_water::TerrainVoxelWater;
use super::water_bodies::CellGrid;

/// Most voxels one ReadVoxels or WriteVoxels call may touch: 2^22, a
/// 256 x 256 x 64 region. A voxel write is also held to the volume's lattice
/// cap (`volume::MAX_EDIT_LATTICE_POINTS`, 256^3 lattice points), the only
/// bound on a fill.
pub const MAX_SCRIPT_VOXELS: u32 = 4_194_304;

/// Lattice coordinates are held to +/- 2^28 before integer conversion, as
/// the volume holds them, so neighbour arithmetic cannot overflow `i32`.
const LATTICE_CLAMP: f32 = 268_435_456.0;

// ============================================================================
// Commands
// ============================================================================

/// What a fill puts inside its shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerrainFill {
    /// Solid ground of this material.
    Material(TerrainMaterial),
    /// Empty space: carves the ground and removes water there.
    Air,
    /// Water: raises the terrain's water surface over the shape's footprint
    /// (`TerrainVoxelWater`).
    Water,
}

impl TerrainFill {
    /// The fill a Roblox material name names ("Grass", "Air", "Water",
    /// "Enum.Material.Rock", "Material.Rock"), ignoring case, and spaces,
    /// underscores and hyphens in the material itself. `None` for a material
    /// terrain does not have ("Plastic").
    pub fn from_material_name(name: &str) -> Option<TerrainFill> {
        let lower = name.trim().to_ascii_lowercase();
        let bare = lower
            .strip_prefix("enum.material.")
            .or_else(|| lower.strip_prefix("material."))
            .unwrap_or(&lower);
        // Air and Water are matched the way `TerrainMaterial::from_name`
        // matches the rest, without spaces, underscores or hyphens, so no
        // spelling of Water turns into a solid fill painted like water.
        let key: String = bare.chars().filter(|c| !matches!(c, ' ' | '_' | '-')).collect();
        match key.as_str() {
            "air" => Some(Self::Air),
            "water" => Some(Self::Water),
            _ => TerrainMaterial::from_name(bare).map(Self::Material),
        }
    }

    /// The Roblox material name this fill reads as: "Air", "Water", or the
    /// material's name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Material(material) => material.name(),
            Self::Air => "Air",
            Self::Water => "Water",
        }
    }
}

/// How a sculpt changes the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerrainSculptMode {
    /// Lift the ground.
    Raise,
    /// Sink the ground.
    Lower,
    /// Level the ground toward world height `height` (metres).
    Flatten { height: f32 },
    /// Blend the ground toward its neighbours.
    Smooth,
}

/// One terrain edit in world metres. Rotations are the shape's orientation
/// (Roblox CFrame rotation); cylinders run along their local Y.
#[derive(Clone, Debug, PartialEq)]
pub enum TerrainCommand {
    /// Fill a ball of `radius` around `center`.
    FillBall { center: Vec3, radius: f32, fill: TerrainFill },
    /// Fill a box of full size `size` around `center`, turned by `rotation`.
    FillBlock { center: Vec3, rotation: Quat, size: Vec3, fill: TerrainFill },
    /// Fill a cylinder of full `height` and `radius` around `center`, its
    /// axis the local Y `rotation` turns.
    FillCylinder { center: Vec3, rotation: Quat, height: f32, radius: f32, fill: TerrainFill },
    /// Fill the world-axis box `min..max`.
    FillRegion { min: Vec3, max: Vec3, fill: TerrainFill },
    /// Swap material `from` for `to` inside the world-axis box `min..max`:
    /// the material cells of the XZ rectangle whose surface height lies in
    /// `min.y..=max.y`, and the volume's edit materials inside the box.
    ReplaceMaterial { min: Vec3, max: Vec3, from: TerrainMaterial, to: TerrainMaterial },
    /// Water in the air of the world-axis box `min..max`, up to `max.y`; the
    /// ground stays (see Water in the module docs).
    FillWater { min: Vec3, max: Vec3 },
    /// Drain the water inside the world-axis box `min..max`; the ground stays
    /// (see Water in the module docs).
    DrainWater { min: Vec3, max: Vec3 },
    /// Voxels of `resolution` metres from `min`; `materials` and `occupancies`
    /// hold size.x * size.y * size.z entries, index = x + size.x * (y + size.y * z).
    WriteVoxels { min: Vec3, resolution: f32, size: UVec3, materials: Vec<TerrainFill>, occupancies: Vec<f32> },
    /// One dab of the editor's heightfield brush over the disc of `radius`
    /// around `center`, `strength` 0 to 1.
    Sculpt { mode: TerrainSculptMode, center: Vec3, radius: f32, strength: f32 },
    /// `material` is a material slot id (TerrainMaterial::to_u8() for built-ins).
    Paint { center: Vec3, radius: f32, material: u8, strength: f32 },
    /// Runtime clear: every cell becomes a hole, volume and water emptied.
    Clear,
}

/// What one command changed.
#[derive(Clone, Debug, Default)]
pub struct TerrainCommandEffect {
    /// World XZ `(min, max)` of the changed heights.
    pub height_rect: Option<(Vec2, Vec2)>,
    /// World XZ `(min, max)` of the changed material cells. The whole
    /// footprint when the command allocated the material layer, which
    /// recolours every chunk.
    pub material_rect: Option<(Vec2, Vec2)>,
    /// The volume bricks the command created, changed or removed.
    pub volume_edit: Option<VolumeEdit>,
    /// The water levels changed.
    pub water_changed: bool,
    /// The terrain was cleared: every chunk changed.
    pub cleared: bool,
}

impl TerrainCommandEffect {
    /// The command changed nothing.
    pub fn is_empty(&self) -> bool {
        self.height_rect.is_none()
            && self.material_rect.is_none()
            && self.volume_edit.is_none()
            && !self.water_changed
            && !self.cleared
    }
}

// ============================================================================
// Bounds and recording
// ============================================================================

/// World-space box (min, max) a command can touch, for undo recording before
/// it runs; None for Clear. A fill's box is its shape's edit bounds, a voxel
/// write's the region with every lattice point it writes, a material swap's,
/// a FillWater's and a DrainWater's their box. Sculpt and Paint change whole
/// columns of their disc, so their box spans every height. `None` too for a
/// fill whose rotation has no direction.
pub fn command_bounds(config: &TerrainConfig, cmd: &TerrainCommand) -> Option<(Vec3, Vec3)> {
    match cmd {
        TerrainCommand::FillBall { .. }
        | TerrainCommand::FillBlock { .. }
        | TerrainCommand::FillCylinder { .. }
        | TerrainCommand::FillRegion { .. } => fill_shape(cmd).map(|shape| shape.edit_bounds(config)),
        TerrainCommand::ReplaceMaterial { min, max, .. }
        | TerrainCommand::FillWater { min, max }
        | TerrainCommand::DrainWater { min, max } => Some((min.min(*max), min.max(*max))),
        TerrainCommand::WriteVoxels { min, resolution, size, .. } => {
            Some(voxel_region_bounds(*min, *resolution, *size, lattice_cell_size(config)))
        }
        TerrainCommand::Sculpt { center, radius, .. } | TerrainCommand::Paint { center, radius, .. } => Some((
            Vec3::new(center.x - radius, f32::NEG_INFINITY, center.z - radius),
            Vec3::new(center.x + radius, f32::INFINITY, center.z + radius),
        )),
        TerrainCommand::Clear => None,
    }
}

/// Record in `recorder` everything `cmd` can write, over its
/// [`command_bounds`], ahead of running it on `data` and `volume`: the raster
/// tiles under a Sculpt, Paint or material swap (with one raster cell of
/// margin against rounding on the edge), and the volume bricks a solid or air
/// fill, a voxel write or a material swap can touch. Water fills, FillWater,
/// DrainWater and Clear record nothing here: the water levels any command
/// can change are the caller's to record, over the same bounds
/// (`TerrainEditRecorder::record_water_rect`), and Clear is not undoable. A
/// command that [`apply_terrain_command`] refuses for its numbers, sizes or
/// lattice cost records nothing either, since it writes nothing.
pub fn record_terrain_command(
    recorder: &mut TerrainEditRecorder,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    cmd: &TerrainCommand,
) {
    if check_command(cmd).is_err() {
        return;
    }
    let Some((lo, hi)) = command_bounds(config, cmd) else {
        return;
    };
    let margin = Vec2::splat(config.chunk_size / config.chunk_resolution.max(1) as f32);
    // A volume edit past the lattice cap is refused and writes no brick.
    let fits_lattice = || check_lattice_cap(lo, hi, lattice_cell_size(config)).is_ok();
    match cmd {
        TerrainCommand::FillBall { fill, .. }
        | TerrainCommand::FillBlock { fill, .. }
        | TerrainCommand::FillCylinder { fill, .. }
        | TerrainCommand::FillRegion { fill, .. } => {
            if *fill != TerrainFill::Water && fits_lattice() {
                recorder.record_volume_aabb(config, volume, lo, hi);
            }
        }
        TerrainCommand::ReplaceMaterial { .. } => {
            recorder.record_world_rect(config, data, lo.xz() - margin, hi.xz() + margin);
            // A swap changes only bricks that exist, so those are all it records.
            let coords: Vec<IVec3> = volume.bricks_in_aabb(config, lo, hi).map(|(coord, _)| coord).collect();
            recorder.record_bricks(volume, &coords);
        }
        TerrainCommand::WriteVoxels { .. } => {
            if fits_lattice() {
                recorder.record_volume_aabb(config, volume, lo, hi);
            }
        }
        TerrainCommand::Sculpt { .. } | TerrainCommand::Paint { .. } => {
            recorder.record_world_rect(config, data, lo.xz() - margin, hi.xz() + margin);
        }
        TerrainCommand::FillWater { .. } | TerrainCommand::DrainWater { .. } | TerrainCommand::Clear => {}
    }
}

/// Whether `cmd` writes water, so its root needs a `TerrainVoxelWater` before
/// it runs: a Water fill, a FillWater, or a voxel write holding a water voxel.
/// A DrainWater only lowers water that is there.
pub fn command_needs_water(cmd: &TerrainCommand) -> bool {
    match cmd {
        TerrainCommand::FillBall { fill, .. }
        | TerrainCommand::FillBlock { fill, .. }
        | TerrainCommand::FillCylinder { fill, .. }
        | TerrainCommand::FillRegion { fill, .. } => *fill == TerrainFill::Water,
        TerrainCommand::FillWater { .. } => true,
        TerrainCommand::WriteVoxels { materials, occupancies, .. } => materials
            .iter()
            .zip(occupancies)
            .any(|(fill, occupancy)| *fill == TerrainFill::Water && *occupancy > 0.0),
        TerrainCommand::ReplaceMaterial { .. }
        | TerrainCommand::DrainWater { .. }
        | TerrainCommand::Sculpt { .. }
        | TerrainCommand::Paint { .. }
        | TerrainCommand::Clear => false,
    }
}

// ============================================================================
// Applying commands
// ============================================================================

/// Apply one command to a root's base data. Refuses (Err) procedural terrain
/// (empty height_cache), non-finite numbers, non-positive sizes, and edits
/// larger than the volume's lattice cap, and the rest of the refusals the
/// module docs list, before changing anything.
pub fn apply_terrain_command(
    config: &TerrainConfig,
    data: &mut TerrainData,
    volume: &mut TerrainVolume,
    water: Option<&mut TerrainVoxelWater>,
    cmd: &TerrainCommand,
) -> Result<TerrainCommandEffect, String> {
    let grid = editable_grid(config, data)?;
    check_command(cmd)?;
    match cmd {
        TerrainCommand::FillBall { fill, .. }
        | TerrainCommand::FillBlock { fill, .. }
        | TerrainCommand::FillCylinder { fill, .. }
        | TerrainCommand::FillRegion { fill, .. } => {
            let shape = fill_shape(cmd).ok_or_else(|| "the fill's rotation has no direction".to_string())?;
            apply_fill(config, data, volume, water, &grid, shape, *fill)
        }
        TerrainCommand::ReplaceMaterial { min, max, from, to } => {
            Ok(replace_material(config, data, volume, &grid, *min, *max, *from, *to))
        }
        TerrainCommand::FillWater { min, max } => {
            let Some(water) = water else {
                return Err("the terrain has no water component to hold the water".to_string());
            };
            let effect = TerrainCommandEffect {
                water_changed: fill_water_over_air(config, data, water, &grid, min.min(*max), min.max(*max)),
                ..TerrainCommandEffect::default()
            };
            Ok(effect)
        }
        TerrainCommand::DrainWater { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            let shape = CsgShape::AxisBox { center: (lo + hi) * 0.5, half_extents: (hi - lo) * 0.5 };
            let water_changed = water.is_some_and(|water| lower_water(config, data, water, &grid, &shape));
            Ok(TerrainCommandEffect { water_changed, ..TerrainCommandEffect::default() })
        }
        TerrainCommand::WriteVoxels { min, resolution, size, materials, occupancies } => {
            write_voxels(config, data, volume, water, &grid, *min, *resolution, *size, materials, occupancies)
        }
        TerrainCommand::Sculpt { mode, center, radius, strength } => {
            Ok(sculpt(config, data, *mode, *center, *radius, *strength))
        }
        TerrainCommand::Paint { center, radius, material, strength } => {
            Ok(paint(config, data, &grid, *center, *radius, *material, *strength))
        }
        TerrainCommand::Clear => Ok(clear(data, volume, water)),
    }
}

/// The raster cells of a terrain commands can edit: a whole height raster of
/// at least two cells a side over a usable chunk grid.
fn editable_grid(config: &TerrainConfig, data: &TerrainData) -> Result<CellGrid, String> {
    if data.height_cache.is_empty() {
        return Err("the terrain is procedural: it has no height raster to edit".to_string());
    }
    if !(config.chunk_size.is_finite() && config.chunk_size > 0.0) || config.chunk_resolution == 0 {
        return Err("the terrain's chunk grid has no usable size".to_string());
    }
    CellGrid::of(config, data).ok_or_else(|| "the terrain's height raster does not cover its chunk grid".to_string())
}

fn check_point(point: Vec3, what: &str) -> Result<(), String> {
    if point.is_finite() {
        Ok(())
    } else {
        Err(format!("{what} {point} is not a finite point"))
    }
}

fn check_number(value: f32, what: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{what} {value} is not a finite number"))
    }
}

fn check_length(value: f32, what: &str) -> Result<(), String> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(format!("{what} {value} is not a positive length"))
    }
}

fn check_size(size: Vec3, what: &str) -> Result<(), String> {
    if size.is_finite() && size.cmpgt(Vec3::ZERO).all() {
        Ok(())
    } else {
        Err(format!("{what} {size} is not positive on every axis"))
    }
}

/// `rotation` scaled to unit length, or `None` when it has no direction (zero
/// length, or not finite).
fn unit_rotation(rotation: Quat) -> Option<Quat> {
    let length_squared = rotation.length_squared();
    (rotation.is_finite() && length_squared.is_finite() && length_squared > 1e-12).then(|| rotation.normalize())
}

fn check_rotation(rotation: Quat) -> Result<(), String> {
    unit_rotation(rotation)
        .map(|_| ())
        .ok_or_else(|| format!("rotation {rotation} is not a rotation"))
}

/// The voxel count of a region of `size`, refusing an empty region and one
/// over [`MAX_SCRIPT_VOXELS`].
fn voxel_count(size: UVec3) -> Result<usize, String> {
    let count = size.x as u128 * size.y as u128 * size.z as u128;
    if count == 0 {
        return Err(format!("voxel region {size} holds no voxels"));
    }
    if count > MAX_SCRIPT_VOXELS as u128 {
        return Err(format!("voxel region {size} holds {count} voxels, more than the {MAX_SCRIPT_VOXELS} one call may touch"));
    }
    Ok(count as usize)
}

/// Every refusal that needs no terrain: the numbers, sizes, arrays and slots.
fn check_command(cmd: &TerrainCommand) -> Result<(), String> {
    match cmd {
        TerrainCommand::FillBall { center, radius, .. } => {
            check_point(*center, "center")?;
            check_length(*radius, "radius")
        }
        TerrainCommand::FillBlock { center, rotation, size, .. } => {
            check_point(*center, "center")?;
            check_rotation(*rotation)?;
            check_size(*size, "size")
        }
        TerrainCommand::FillCylinder { center, rotation, height, radius, .. } => {
            check_point(*center, "center")?;
            check_rotation(*rotation)?;
            check_length(*height, "height")?;
            check_length(*radius, "radius")
        }
        TerrainCommand::FillRegion { min, max, .. }
        | TerrainCommand::ReplaceMaterial { min, max, .. }
        | TerrainCommand::FillWater { min, max }
        | TerrainCommand::DrainWater { min, max } => {
            check_point(*min, "min")?;
            check_point(*max, "max")?;
            check_size(*max - *min, "region size (max - min)")
        }
        TerrainCommand::WriteVoxels { min, resolution, size, materials, occupancies } => {
            check_point(*min, "min")?;
            check_length(*resolution, "resolution")?;
            let count = voxel_count(*size)?;
            if materials.len() != count || occupancies.len() != count {
                return Err(format!(
                    "voxel region {size} holds {count} voxels, but {} materials and {} occupancies were given",
                    materials.len(),
                    occupancies.len()
                ));
            }
            match occupancies.iter().position(|occupancy| !occupancy.is_finite()) {
                Some(index) => Err(format!("occupancy {} of voxel {index} is not a finite number", occupancies[index])),
                None => Ok(()),
            }
        }
        TerrainCommand::Sculpt { mode, center, radius, strength } => {
            check_point(*center, "center")?;
            check_length(*radius, "radius")?;
            check_number(*strength, "strength")?;
            match mode {
                TerrainSculptMode::Flatten { height } => check_number(*height, "flatten height"),
                TerrainSculptMode::Raise | TerrainSculptMode::Lower | TerrainSculptMode::Smooth => Ok(()),
            }
        }
        TerrainCommand::Paint { center, radius, material, strength } => {
            check_point(*center, "center")?;
            check_length(*radius, "radius")?;
            check_number(*strength, "strength")?;
            if *material == MATERIAL_SLOT_NONE {
                Err(format!("material slot {MATERIAL_SLOT_NONE} means no material, so it cannot be painted"))
            } else {
                Ok(())
            }
        }
        TerrainCommand::Clear => Ok(()),
    }
}

/// Refuse a volume edit over world box `lo..hi` that visits more lattice
/// points than one edit may, counted outward to whole cells the way the CSG
/// ops and the undo recorder count them.
fn check_lattice_cap(lo: Vec3, hi: Vec3, cell: f32) -> Result<(), String> {
    let first = (lo / cell).floor();
    let last = (hi / cell).ceil();
    let extent = (last - first + Vec3::ONE).max(Vec3::ZERO);
    let points = extent.x as f64 * extent.y as f64 * extent.z as f64;
    if points <= MAX_EDIT_LATTICE_POINTS as f64 {
        Ok(())
    } else {
        Err(format!(
            "the edit covers {points:.0} lattice points of {cell} m, more than the {MAX_EDIT_LATTICE_POINTS} one volume edit may visit"
        ))
    }
}

/// The CSG shape of a fill command, its rotation normalized; `None` for a
/// command that is not a fill, or a rotation with no direction.
fn fill_shape(cmd: &TerrainCommand) -> Option<CsgShape> {
    match *cmd {
        TerrainCommand::FillBall { center, radius, .. } => Some(CsgShape::Sphere { center, radius }),
        TerrainCommand::FillBlock { center, rotation, size, .. } => Some(CsgShape::OrientedBox {
            center,
            half_extents: size * 0.5,
            rotation: unit_rotation(rotation)?,
        }),
        TerrainCommand::FillCylinder { center, rotation, height, radius, .. } => Some(CsgShape::OrientedCylinder {
            center,
            radius,
            half_height: height * 0.5,
            rotation: unit_rotation(rotation)?,
        }),
        TerrainCommand::FillRegion { min, max, .. } => {
            Some(CsgShape::AxisBox { center: (min + max) * 0.5, half_extents: (max - min) * 0.5 })
        }
        TerrainCommand::ReplaceMaterial { .. }
        | TerrainCommand::FillWater { .. }
        | TerrainCommand::DrainWater { .. }
        | TerrainCommand::WriteVoxels { .. }
        | TerrainCommand::Sculpt { .. }
        | TerrainCommand::Paint { .. }
        | TerrainCommand::Clear => None,
    }
}

/// Fill `shape` with `fill` (see the module docs).
#[allow(clippy::too_many_arguments)]
fn apply_fill(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &mut TerrainVolume,
    water: Option<&mut TerrainVoxelWater>,
    grid: &CellGrid,
    shape: CsgShape,
    fill: TerrainFill,
) -> Result<TerrainCommandEffect, String> {
    let mut effect = TerrainCommandEffect::default();
    let cell = lattice_cell_size(config);
    match fill {
        TerrainFill::Material(material) => {
            let (lo, hi) = shape.edit_bounds(config);
            check_lattice_cap(lo, hi, cell)?;
            let edit = apply_shape(config, volume, shape, CsgOp::Add, Some(material));
            effect.volume_edit = (!edit.is_empty()).then_some(edit);
        }
        TerrainFill::Air => {
            let (lo, hi) = shape.edit_bounds(config);
            check_lattice_cap(lo, hi, cell)?;
            let (shape_lo, shape_hi) = shape.bounds();
            let walls = carve_material(config, data, volume, (shape_lo + shape_hi) * 0.5);
            let edit = apply_shape(config, volume, shape, CsgOp::Carve, Some(walls));
            effect.volume_edit = (!edit.is_empty()).then_some(edit);
            if let Some(water) = water {
                effect.water_changed = lower_water(config, data, water, grid, &shape);
            }
        }
        TerrainFill::Water => {
            let Some(water) = water else {
                return Err("the terrain has no water component to hold the water".to_string());
            };
            effect.water_changed = raise_water(data, water, grid, &shape);
        }
    }
    Ok(effect)
}

/// The material an air fill centred on `at` gives the walls it exposes: an
/// earlier edit's there, else the ground's surface material under it, else
/// Rock.
fn carve_material(config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume, at: Vec3) -> TerrainMaterial {
    material_at(config, volume, at)
        .or_else(|| material_at_world(config, data, at.x, at.z).and_then(|sample| TerrainMaterial::from_u8(sample.primary)))
        .unwrap_or(TerrainMaterial::Rock)
}

// ============================================================================
// Raster helpers
// ============================================================================

/// Raster cells `(x0, z0, x1, z1)`, inclusive, whose centres lie in world
/// rectangle `lo..hi`; `None` when none do.
fn cells_in(grid: &CellGrid, lo: Vec2, hi: Vec2) -> Option<(usize, usize, usize, usize)> {
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let last = Vec2::new((grid.width - 1) as f32, (grid.height - 1) as f32);
    let first = ((lo - grid.origin) / grid.step).ceil().max(Vec2::ZERO);
    let end = ((hi - grid.origin) / grid.step).floor().min(last);
    (first.x <= end.x && first.y <= end.y).then(|| (first.x as usize, first.y as usize, end.x as usize, end.y as usize))
}

/// Raster index of the cell nearest world `p`, `None` off the raster.
fn cell_at(grid: &CellGrid, p: Vec2) -> Option<usize> {
    let cell = ((p - grid.origin) / grid.step).round();
    let last = Vec2::new((grid.width - 1) as f32, (grid.height - 1) as f32);
    (cell.x >= 0.0 && cell.y >= 0.0 && cell.x <= last.x && cell.y <= last.y)
        .then(|| cell.y as usize * grid.width + cell.x as usize)
}

/// World height of raster cell `index`'s ground, `None` over a hole of a
/// sparse surface (`TerrainData::cell_is_hole`).
fn cell_ground(config: &TerrainConfig, data: &TerrainData, index: usize) -> Option<f32> {
    if data.cell_is_hole(index) {
        return None;
    }
    data.height_cache.get(index).map(|&normalized| config.world_height(normalized))
}

/// `rect` grown to hold `p`.
fn grow_rect(rect: Option<(Vec2, Vec2)>, p: Vec2) -> (Vec2, Vec2) {
    match rect {
        Some((lo, hi)) => (lo.min(p), hi.max(p)),
        None => (p, p),
    }
}

/// Whether world rectangle `lo..hi` overlaps the terrain's footprint.
fn overlaps_footprint(config: &TerrainConfig, lo: Vec2, hi: Vec2) -> bool {
    let (min, max) = config.footprint_xz();
    lo.x <= max.x && hi.x >= min.x && lo.y <= max.y && hi.y >= min.y
}

// ============================================================================
// Water
// ============================================================================

/// Whether `water` holds levels for `data`'s raster.
fn water_matches(water: &TerrainVoxelWater, data: &TerrainData) -> bool {
    water.width == data.cache_width
        && water.height == data.cache_height
        && water.levels.len() == data.cache_width as usize * data.cache_height as usize
}

/// Size `water` to `data`'s raster, every cell dry, unless it already
/// matches. Returns `true` when it resized. The brush's water dabs share it.
pub(crate) fn size_water(water: &mut TerrainVoxelWater, data: &TerrainData) -> bool {
    if water_matches(water, data) {
        return false;
    }
    water.levels = vec![f32::NAN; data.cache_width as usize * data.cache_height as usize];
    water.width = data.cache_width;
    water.height = data.cache_height;
    true
}

/// `level` and `wanted` are the same water surface (both dry, or equal).
fn same_level(level: f32, wanted: f32) -> bool {
    level == wanted || (level.is_nan() && wanted.is_nan())
}

/// Narrow the line parameter range `range` to where `|origin + t * dir|` is
/// at most `half`, `None` once nothing is left.
fn clip_slab(range: (f32, f32), origin: f32, dir: f32, half: f32) -> Option<(f32, f32)> {
    if dir.abs() <= 1e-9 {
        return (origin.abs() <= half).then_some(range);
    }
    let a = (-half - origin) / dir;
    let b = (half - origin) / dir;
    let clipped = (range.0.max(a.min(b)), range.1.min(a.max(b)));
    (clipped.0 <= clipped.1).then_some(clipped)
}

/// World Y interval `(bottom, top)` the vertical line through world `(x, z)`
/// spends inside `shape`, `None` when the line misses it.
pub(crate) fn column_span(shape: &CsgShape, x: f32, z: f32) -> Option<(f32, f32)> {
    match *shape {
        CsgShape::Sphere { center, radius } => {
            let across = Vec2::new(x - center.x, z - center.z).length_squared();
            let reach = radius * radius - across;
            (reach >= 0.0).then(|| {
                let half = reach.sqrt();
                (center.y - half, center.y + half)
            })
        }
        CsgShape::AxisBox { center, half_extents } => ((x - center.x).abs() <= half_extents.x
            && (z - center.z).abs() <= half_extents.z)
            .then(|| (center.y - half_extents.y, center.y + half_extents.y)),
        CsgShape::Cylinder { center, radius, half_height } => (Vec2::new(x - center.x, z - center.z).length_squared()
            <= radius * radius)
            .then(|| (center.y - half_height, center.y + half_height)),
        CsgShape::OrientedBox { center, half_extents, rotation } => {
            // The line is (x, t, z) for world height t; in the box's frame it
            // runs from `origin` along `dir`, and the box is three slabs.
            let origin = shape_local(rotation, Vec3::new(x, 0.0, z) - center);
            let dir = shape_local(rotation, Vec3::Y);
            let span = (f32::NEG_INFINITY, f32::INFINITY);
            let span = clip_slab(span, origin.x, dir.x, half_extents.x)?;
            let span = clip_slab(span, origin.y, dir.y, half_extents.y)?;
            clip_slab(span, origin.z, dir.z, half_extents.z)
        }
        CsgShape::OrientedCylinder { center, radius, half_height, rotation } => {
            let origin = shape_local(rotation, Vec3::new(x, 0.0, z) - center);
            let dir = shape_local(rotation, Vec3::Y);
            let span = clip_slab((f32::NEG_INFINITY, f32::INFINITY), origin.y, dir.y, half_height)?;
            // Within `radius` of the axis: a t^2 + b t + c <= 0.
            let a = dir.x * dir.x + dir.z * dir.z;
            let b = 2.0 * (origin.x * dir.x + origin.z * dir.z);
            let c = origin.x * origin.x + origin.z * origin.z - radius * radius;
            if a <= 1e-12 {
                // The line runs along the axis.
                return (c <= 0.0).then_some(span);
            }
            let discriminant = b * b - 4.0 * a * c;
            if discriminant < 0.0 {
                return None;
            }
            let root = discriminant.sqrt();
            let clipped = (span.0.max((-b - root) / (2.0 * a)), span.1.min((-b + root) / (2.0 * a)));
            (clipped.0 <= clipped.1).then_some(clipped)
        }
    }
}

/// Raise `water` over `shape`'s footprint to the shape's top over each cell,
/// sizing it to the raster first. Returns whether a level changed.
fn raise_water(data: &TerrainData, water: &mut TerrainVoxelWater, grid: &CellGrid, shape: &CsgShape) -> bool {
    let mut changed = size_water(water, data);
    let (lo, hi) = shape.bounds();
    let Some((x0, z0, x1, z1)) = cells_in(grid, lo.xz(), hi.xz()) else {
        return changed;
    };
    for z in z0..=z1 {
        for x in x0..=x1 {
            let p = grid.world(x, z);
            let Some((_, top)) = column_span(shape, p.x, p.y) else {
                continue;
            };
            let level = &mut water.levels[z * grid.width + x];
            // Dry (NaN) or lower: raise. Never lower.
            if !(*level >= top) {
                *level = top;
                changed = true;
            }
        }
    }
    changed
}

/// Lower `water` where `shape` reaches its surface: a cell whose surface the
/// shape spans drops to the shape's bottom there, drying out when that bottom
/// is at or below the cell's ground. Returns whether a level changed.
fn lower_water(
    config: &TerrainConfig,
    data: &TerrainData,
    water: &mut TerrainVoxelWater,
    grid: &CellGrid,
    shape: &CsgShape,
) -> bool {
    if !water_matches(water, data) {
        return false;
    }
    let (lo, hi) = shape.bounds();
    let Some((x0, z0, x1, z1)) = cells_in(grid, lo.xz(), hi.xz()) else {
        return false;
    };
    let mut changed = false;
    for z in z0..=z1 {
        for x in x0..=x1 {
            let index = z * grid.width + x;
            let level = water.levels[index];
            if !level.is_finite() {
                continue;
            }
            let p = grid.world(x, z);
            let Some((bottom, top)) = column_span(shape, p.x, p.y) else {
                continue;
            };
            if bottom <= level && level <= top {
                let dries = cell_ground(config, data, index).is_some_and(|ground| bottom <= ground);
                water.levels[index] = if dries { f32::NAN } else { bottom };
                changed = true;
            }
        }
    }
    changed
}

/// Put water in the air of the world box `lo..hi` (a FillWater): every
/// column of its rectangle whose ground stands below `hi.y`, or that has no
/// ground, rises to `hi.y`; none falls. Sizes `water` to the raster first.
/// Returns whether a level changed.
fn fill_water_over_air(
    config: &TerrainConfig,
    data: &TerrainData,
    water: &mut TerrainVoxelWater,
    grid: &CellGrid,
    lo: Vec3,
    hi: Vec3,
) -> bool {
    let mut changed = size_water(water, data);
    let Some((x0, z0, x1, z1)) = cells_in(grid, lo.xz(), hi.xz()) else {
        return changed;
    };
    let top = hi.y;
    for z in z0..=z1 {
        for x in x0..=x1 {
            let index = z * grid.width + x;
            if cell_ground(config, data, index).is_some_and(|ground| ground >= top) {
                continue;
            }
            let level = &mut water.levels[index];
            // Dry (NaN) or lower: raise. Never lower.
            if !(*level >= top) {
                *level = top;
                changed = true;
            }
        }
    }
    changed
}

// ============================================================================
// Material swap
// ============================================================================

/// Swap `from` for `to` inside world box `min..max` (see
/// [`TerrainCommand::ReplaceMaterial`]).
#[allow(clippy::too_many_arguments)]
fn replace_material(
    config: &TerrainConfig,
    data: &mut TerrainData,
    volume: &mut TerrainVolume,
    grid: &CellGrid,
    min: Vec3,
    max: Vec3,
    from: TerrainMaterial,
    to: TerrainMaterial,
) -> TerrainCommandEffect {
    let mut effect = TerrainCommandEffect::default();
    if from == to {
        return effect;
    }
    let (lo, hi) = (min.min(max), min.max(max));
    let (from_id, to_id) = (from.to_u8(), to.to_u8());

    // A terrain without a material layer colours by altitude: it holds no
    // material cell to swap.
    if data.has_material_layer() {
        if let Some((x0, z0, x1, z1)) = cells_in(grid, lo.xz(), hi.xz()) {
            let mut swapped: Option<(Vec2, Vec2)> = None;
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let index = z * grid.width + x;
                    let height = config.world_height(data.height_cache[index]);
                    if !(height >= lo.y && height <= hi.y) {
                        continue;
                    }
                    let current = data.material_cache[index];
                    let [a, b, blend, _] = current;
                    if a != from_id && b != from_id {
                        continue;
                    }
                    let swap = |slot: u8| if slot == from_id { to_id } else { slot };
                    let cell = canonical_material_cell(swap(a), swap(b), blend);
                    if cell != current {
                        data.material_cache[index] = cell;
                        swapped = Some(grow_rect(swapped, grid.world(x, z)));
                    }
                }
            }
            if swapped.is_some() {
                data.material_dirty = true;
                effect.material_rect = swapped;
            }
        }
    }

    let edit = replace_brick_material(config, volume, lo, hi, from_id, to_id);
    effect.volume_edit = (!edit.is_empty()).then_some(edit);
    effect
}

/// Swap material `from_id` for `to_id` on the edited lattice points of
/// `volume` inside world box `lo..hi`.
fn replace_brick_material(
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    lo: Vec3,
    hi: Vec3,
    from_id: u8,
    to_id: u8,
) -> VolumeEdit {
    let cell = lattice_cell_size(config);
    let limit = Vec3::splat(LATTICE_CLAMP);
    let n0 = (lo / cell).ceil().clamp(-limit, limit).as_ivec3();
    let n1 = (hi / cell).floor().clamp(-limit, limit).as_ivec3();
    if n0.cmpgt(n1).any() {
        return VolumeEdit::default();
    }
    let coords: Vec<IVec3> = volume.bricks_in_aabb(config, lo, hi).map(|(coord, _)| coord).collect();
    let (mut changed_lo, mut changed_hi) = (IVec3::MAX, IVec3::MIN);
    let mut bricks = Vec::new();
    for coord in coords {
        let origin = brick_lattice_origin(coord);
        let local_lo = (n0 - origin).max(IVec3::ZERO);
        let local_hi = (n1 - origin).min(IVec3::splat(BRICK_EDGE - 1));
        if local_lo.cmpgt(local_hi).any() {
            continue;
        }
        let Some(mut brick) = volume.brick(coord).cloned() else {
            continue;
        };
        let mut changed = false;
        for k in local_lo.z..=local_hi.z {
            for j in local_lo.y..=local_hi.y {
                for i in local_lo.x..=local_hi.x {
                    let index = brick_cell_index(i, j, k);
                    if brick.material[index] == from_id && brick.cell(index).is_edited() {
                        brick.material[index] = to_id;
                        changed = true;
                        let n = origin + IVec3::new(i, j, k);
                        changed_lo = changed_lo.min(n);
                        changed_hi = changed_hi.max(n);
                    }
                }
            }
        }
        if changed {
            volume.set_brick(coord, Some(brick));
            bricks.push(coord);
        }
    }
    finish_volume_edit(bricks, changed_lo, changed_hi, cell)
}

/// The [`VolumeEdit`] of `bricks` whose changed lattice points span
/// `lo..=hi`: the box grows one cell past them, since a sample anywhere in
/// the cells around a point interpolates it.
fn finish_volume_edit(mut bricks: Vec<IVec3>, lo: IVec3, hi: IVec3, cell: f32) -> VolumeEdit {
    if bricks.is_empty() {
        return VolumeEdit::default();
    }
    bricks.sort_unstable_by_key(|coord| (coord.x, coord.y, coord.z));
    bricks.dedup();
    VolumeEdit {
        min: lattice_point_world(lo - IVec3::ONE, cell),
        max: lattice_point_world(hi + IVec3::ONE, cell),
        bricks,
    }
}

// ============================================================================
// Voxels
// ============================================================================

/// The world box a voxel write can touch: its region, and every lattice
/// point carrying weight at one of its voxel centres (from the lattice point
/// at or below the first centre to the one at or above the last, on each
/// axis), which reaches past the region when voxels are under two cells.
fn voxel_region_bounds(min: Vec3, resolution: f32, size: UVec3, cell: f32) -> (Vec3, Vec3) {
    let (n0, n1) = voxel_lattice_range(min, resolution, size, cell);
    let max = min + size.as_vec3() * resolution;
    (min.min(lattice_point_world(n0, cell)), max.max(lattice_point_world(n1, cell)))
}

/// The lattice points `(n0, n1)` a voxel write stores: those carrying
/// trilinear weight at one of its voxel centres.
fn voxel_lattice_range(min: Vec3, resolution: f32, size: UVec3, cell: f32) -> (IVec3, IVec3) {
    let first = min + Vec3::splat(0.5 * resolution);
    let last = min + (size.as_vec3() - Vec3::splat(0.5)) * resolution;
    let limit = Vec3::splat(LATTICE_CLAMP);
    ((first / cell).floor().clamp(-limit, limit).as_ivec3(), (last / cell).ceil().clamp(-limit, limit).as_ivec3())
}

/// The stored edit that makes the field exactly `distance` at a lattice
/// point whatever the heightfield there: `A = d` and `C = -d`, since
/// `max(min(fh, d), d) = d`. The distance stays a step inside the stored
/// range, where neither field reads as "no edit".
fn region_cell(distance: f32, cell: f32, material: u8) -> VolumeCell {
    let q = quantize_distance(distance, cell).clamp(-(Q_NONE - 1), Q_NONE - 1);
    VolumeCell { add: q, carve: -q, material }
}

/// A voxel region's layout: `min`, voxel edge `resolution`, and voxel counts
/// per axis.
#[derive(Clone, Copy, Debug)]
struct VoxelGrid {
    min: Vec3,
    resolution: f32,
    dims: [usize; 3],
}

impl VoxelGrid {
    fn index(&self, x: usize, y: usize, z: usize) -> usize {
        x + self.dims[0] * (y + self.dims[1] * z)
    }

    /// Target distance and material at world point `p`: the solid occupancy
    /// `ground` interpolated trilinearly between voxel centres (the edge
    /// voxels' values continuing past the outermost centres) as
    /// `(0.5 - occupancy) * resolution`, and the material of the nearest solid
    /// voxel among the eight around `p`, [`MATERIAL_NONE`] when all eight are
    /// empty.
    fn sample(&self, p: Vec3, ground: &[f32], fills: &[TerrainFill]) -> (f32, u8) {
        let last = Vec3::new((self.dims[0] - 1) as f32, (self.dims[1] - 1) as f32, (self.dims[2] - 1) as f32);
        let g = ((p - self.min) / self.resolution - Vec3::splat(0.5)).clamp(Vec3::ZERO, last);
        let base = g.floor();
        let t = g - base;
        let i0 = [base.x as usize, base.y as usize, base.z as usize];
        // Weight along one axis of the corner `step` (0 or 1) past the base.
        let along = |step: usize, t: f32| if step == 1 { t } else { 1.0 - t };
        let mut occupancy = 0.0f32;
        let mut nearest: Option<(f32, TerrainMaterial)> = None;
        for corner in 0..8usize {
            let offset = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
            let at = [
                (i0[0] + offset[0]).min(self.dims[0] - 1),
                (i0[1] + offset[1]).min(self.dims[1] - 1),
                (i0[2] + offset[2]).min(self.dims[2] - 1),
            ];
            let weight = along(offset[0], t.x) * along(offset[1], t.y) * along(offset[2], t.z);
            let index = self.index(at[0], at[1], at[2]);
            occupancy += weight * ground[index];
            if let TerrainFill::Material(material) = fills[index] {
                if ground[index] > 0.0 {
                    let distance = (g - Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32)).length_squared();
                    if nearest.map_or(true, |(best, _)| distance < best) {
                        nearest = Some((distance, material));
                    }
                }
            }
        }
        let material = nearest.map_or(MATERIAL_NONE, |(_, material)| material.to_u8());
        ((0.5 - occupancy) * self.resolution, material)
    }

    /// Per voxel column (`x + dims.x * z`), the world Y of the top of its
    /// highest water voxel, `None` for a column without water.
    fn water_tops(&self, fills: &[TerrainFill], occupancies: &[f32]) -> Vec<Option<f32>> {
        let mut tops = vec![None; self.dims[0] * self.dims[2]];
        for z in 0..self.dims[2] {
            for x in 0..self.dims[0] {
                for y in (0..self.dims[1]).rev() {
                    let index = self.index(x, y, z);
                    let filled = occupancies[index].clamp(0.0, 1.0);
                    if fills[index] == TerrainFill::Water && filled > 0.0 {
                        tops[x + self.dims[0] * z] = Some(self.min.y + (y as f32 + filled) * self.resolution);
                        break;
                    }
                }
            }
        }
        tops
    }
}

/// Write the voxels of a region (see the module docs).
#[allow(clippy::too_many_arguments)]
fn write_voxels(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &mut TerrainVolume,
    water: Option<&mut TerrainVoxelWater>,
    grid: &CellGrid,
    min: Vec3,
    resolution: f32,
    size: UVec3,
    fills: &[TerrainFill],
    occupancies: &[f32],
) -> Result<TerrainCommandEffect, String> {
    let cell = lattice_cell_size(config);
    let (lo, hi) = voxel_region_bounds(min, resolution, size, cell);
    check_lattice_cap(lo, hi, cell)?;
    let voxels = VoxelGrid { min, resolution, dims: [size.x as usize, size.y as usize, size.z as usize] };
    let tops = voxels.water_tops(fills, occupancies);
    let holds_water = tops.iter().any(Option::is_some);
    if holds_water && water.is_none() {
        return Err("the terrain has no water component to hold the water voxels".to_string());
    }

    // Air and Water voxels hold no ground.
    let ground: Vec<f32> = fills
        .iter()
        .zip(occupancies)
        .map(|(fill, occupancy)| match fill {
            TerrainFill::Material(_) => occupancy.clamp(0.0, 1.0),
            TerrainFill::Air | TerrainFill::Water => 0.0,
        })
        .collect();

    let (n0, n1) = voxel_lattice_range(min, resolution, size, cell);
    let (b0, b1) = (lattice_to_brick(n0).0, lattice_to_brick(n1).0);
    let (mut changed_lo, mut changed_hi) = (IVec3::MAX, IVec3::MIN);
    let mut bricks = Vec::new();
    for bz in b0.z..=b1.z {
        for by in b0.y..=b1.y {
            for bx in b0.x..=b1.x {
                let coord = IVec3::new(bx, by, bz);
                let origin = brick_lattice_origin(coord);
                let local_lo = (n0 - origin).max(IVec3::ZERO);
                let local_hi = (n1 - origin).min(IVec3::splat(BRICK_EDGE - 1));
                let mut brick = volume.brick(coord).cloned().unwrap_or_default();
                let mut changed = false;
                for k in local_lo.z..=local_hi.z {
                    for j in local_lo.y..=local_hi.y {
                        for i in local_lo.x..=local_hi.x {
                            let n = origin + IVec3::new(i, j, k);
                            let (distance, material) = voxels.sample(lattice_point_world(n, cell), &ground, fills);
                            let wanted = region_cell(distance, cell, material);
                            let index = brick_cell_index(i, j, k);
                            if brick.cell(index) != wanted {
                                brick.add[index] = wanted.add;
                                brick.carve[index] = wanted.carve;
                                brick.material[index] = wanted.material;
                                changed = true;
                                changed_lo = changed_lo.min(n);
                                changed_hi = changed_hi.max(n);
                            }
                        }
                    }
                }
                if changed {
                    volume.set_brick(coord, Some(brick));
                    bricks.push(coord);
                }
            }
        }
    }

    let mut effect = TerrainCommandEffect::default();
    let edit = finish_volume_edit(bricks, changed_lo, changed_hi, cell);
    effect.volume_edit = (!edit.is_empty()).then_some(edit);
    if let Some(water) = water {
        effect.water_changed = write_voxel_water(config, data, water, grid, &voxels, &tops, holds_water);
    }
    Ok(effect)
}

/// Set the water of a voxel region's columns from its water voxels' `tops`
/// (see the module docs). Returns whether a level changed.
fn write_voxel_water(
    config: &TerrainConfig,
    data: &TerrainData,
    water: &mut TerrainVoxelWater,
    grid: &CellGrid,
    voxels: &VoxelGrid,
    tops: &[Option<f32>],
    holds_water: bool,
) -> bool {
    let mut changed = false;
    if holds_water {
        changed = size_water(water, data);
    } else if !water_matches(water, data) {
        return false;
    }
    let (floor, roof) = (voxels.min.y, voxels.min.y + voxels.dims[1] as f32 * voxels.resolution);
    let lo = voxels.min.xz();
    let hi = lo + Vec2::new(voxels.dims[0] as f32, voxels.dims[2] as f32) * voxels.resolution;
    let Some((x0, z0, x1, z1)) = cells_in(grid, lo, hi) else {
        return changed;
    };
    for z in z0..=z1 {
        for x in x0..=x1 {
            let p = grid.world(x, z);
            let column = ((p - lo) / voxels.resolution).floor();
            let vx = (column.x.max(0.0) as usize).min(voxels.dims[0] - 1);
            let vz = (column.y.max(0.0) as usize).min(voxels.dims[2] - 1);
            let index = z * grid.width + x;
            let level = water.levels[index];
            let wanted = match tops[vx + voxels.dims[0] * vz] {
                // Water reaching the region's top joins water standing higher
                // above it rather than cutting that water off.
                Some(top) if level.is_finite() && level > roof && top >= roof - 1e-3 * voxels.resolution => level,
                Some(top) => top,
                None if level.is_finite() && level > floor && level <= roof => {
                    let dries = cell_ground(config, data, index).is_some_and(|ground| floor <= ground);
                    if dries {
                        f32::NAN
                    } else {
                        floor
                    }
                }
                None => level,
            };
            if !same_level(level, wanted) {
                water.levels[index] = wanted;
                changed = true;
            }
        }
    }
    changed
}

/// Roblox ReadVoxels: size.x*size.y*size.z voxels of `resolution` metres from `min`,
/// same index order as WriteVoxels. Occupancy 0..=1 from the combined field; material of the
/// nearest solid surface, Air where empty, Water where the column's water surface covers the voxel.
///
/// See the module docs for the mapping. Empty vectors when the region holds
/// no voxels or more than [`MAX_SCRIPT_VOXELS`]; every voxel Air when `min`
/// or `resolution` is not usable. Callers check those first.
pub fn read_voxels(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    water: Option<&TerrainVoxelWater>,
    min: Vec3,
    resolution: f32,
    size: UVec3,
) -> (Vec<TerrainFill>, Vec<f32>) {
    let count = size.x as u128 * size.y as u128 * size.z as u128;
    if count == 0 || count > MAX_SCRIPT_VOXELS as u128 {
        return (Vec::new(), Vec::new());
    }
    let count = count as usize;
    if !(min.is_finite() && resolution.is_finite() && resolution > 0.0) {
        return (vec![TerrainFill::Air; count], vec![0.0; count]);
    }
    let grid = CellGrid::of(config, data);
    let levels = water.filter(|water| water_matches(water, data)).map(|water| water.levels.as_slice());
    let cell = lattice_cell_size(config);
    let mut fills = Vec::with_capacity(count);
    let mut occupancies = Vec::with_capacity(count);
    for z in 0..size.z {
        for y in 0..size.y {
            for x in 0..size.x {
                let centre = min + (UVec3::new(x, y, z).as_vec3() + Vec3::splat(0.5)) * resolution;
                let column = grid.as_ref().and_then(|grid| cell_at(grid, centre.xz()));
                let has_ground = column.is_some_and(|index| !data.cell_is_hole(index));
                let heightfield = if has_ground { heightfield_term(config, data, centre) } else { f32::INFINITY };
                let (add, carve) = volume.edit_distances(cell, centre);
                let sample = FieldSample::compose(heightfield, add, carve);
                let occupancy = (0.5 - sample.value / resolution).clamp(0.0, 1.0);
                if occupancy > 0.0 {
                    fills.push(TerrainFill::Material(solid_material(config, data, volume, centre, sample)));
                    occupancies.push(occupancy);
                    continue;
                }
                let covered = column
                    .and_then(|index| levels.map(|levels| levels[index]))
                    .filter(|level| level.is_finite())
                    .map_or(0.0, |level| ((level - (centre.y - 0.5 * resolution)) / resolution).clamp(0.0, 1.0));
                if covered > 0.0 {
                    fills.push(TerrainFill::Water);
                    occupancies.push(covered);
                } else {
                    fills.push(TerrainFill::Air);
                    occupancies.push(0.0);
                }
            }
        }
    }
    (fills, occupancies)
}

/// The material that makes a solid voxel at `p` solid, as the meshes colour
/// that surface: the material map's under the heightfield term (Grass
/// without a material layer), the edits' under an add or carve (Rock where
/// no edit carries one).
fn solid_material(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    p: Vec3,
    sample: FieldSample,
) -> TerrainMaterial {
    match sample.term() {
        FieldTerm::Heightfield => material_at_world(config, data, p.x, p.z)
            .map_or(TerrainMaterial::Grass, |cell| TerrainMaterial::from_u8_or_default(cell.primary)),
        FieldTerm::Add | FieldTerm::Carve => material_at(config, volume, p).unwrap_or(TerrainMaterial::Rock),
    }
}

// ============================================================================
// Sculpt, paint, clear
// ============================================================================

/// The editor dab a command applies: the Studio's default falloff on a
/// disc, moving the ground continuously. A full-strength Raise or Lower
/// moves the centre a tenth of the height band, and Smooth and Flatten blend
/// fully by `strength` there (see the module docs), so one command does what
/// a script asks in one call rather than a brush stroke's small dab.
fn command_brush(config: &TerrainConfig, mode: BrushMode, radius: f32, strength: f32) -> BrushDab {
    BrushDab { rate: 0.1 * config.height_scale, blend: 1.0, ..BrushDab::new(mode, radius, strength) }
}

/// One dab of the heightfield brush (see the module docs).
fn sculpt(
    config: &TerrainConfig,
    data: &mut TerrainData,
    mode: TerrainSculptMode,
    center: Vec3,
    radius: f32,
    strength: f32,
) -> TerrainCommandEffect {
    let strength = strength.clamp(0.0, 1.0);
    let reach = Vec2::splat(radius);
    if !(strength > 0.0) || !overlaps_footprint(config, center.xz() - reach, center.xz() + reach) {
        return TerrainCommandEffect::default();
    }
    let (brush_mode, target) = match mode {
        TerrainSculptMode::Raise => (BrushMode::Raise, None),
        TerrainSculptMode::Lower => (BrushMode::Lower, None),
        TerrainSculptMode::Smooth => (BrushMode::Smooth, None),
        TerrainSculptMode::Flatten { height } => (BrushMode::Flatten, Some(height)),
    };
    let brush = BrushDab { flatten_target: target, ..command_brush(config, brush_mode, radius, strength) };
    // No bake: `center` and `height` are base heights, which layers add to.
    let rect = apply_heightfield_brush(&brush, center, config, data, None, None);
    TerrainCommandEffect { height_rect: Some(rect), ..TerrainCommandEffect::default() }
}

/// Paint `slot` into the raster cells of the disc (see the module docs).
#[allow(clippy::too_many_arguments)]
fn paint(
    config: &TerrainConfig,
    data: &mut TerrainData,
    grid: &CellGrid,
    center: Vec3,
    radius: f32,
    slot: u8,
    strength: f32,
) -> TerrainCommandEffect {
    let mut effect = TerrainCommandEffect::default();
    let strength = strength.clamp(0.0, 1.0);
    let at = center.xz();
    if !(strength > 0.0) {
        return effect;
    }
    let Some((x0, z0, x1, z1)) = cells_in(grid, at - Vec2::splat(radius), at + Vec2::splat(radius)) else {
        return effect;
    };
    let brush = command_brush(config, BrushMode::PaintTexture, radius, strength);
    let had_layer = data.has_material_layer();
    let mut painted: Option<(Vec2, Vec2)> = None;
    for z in z0..=z1 {
        for x in x0..=x1 {
            let p = grid.world(x, z);
            let distance = p.distance(at);
            let index = z * grid.width + x;
            if distance > radius || data.cell_is_hole(index) {
                continue;
            }
            let before = data.material_cache.get(index).copied();
            paint_material_at_world(config, data, p.x, p.y, slot, brush.strength * brush.weight_at(distance));
            if data.material_cache.get(index).copied() != before {
                painted = Some(grow_rect(painted, p));
            }
        }
    }
    effect.material_rect = if !had_layer && data.has_material_layer() {
        Some(config.footprint_xz())
    } else {
        painted
    };
    effect
}

/// Make every cell a hole and empty the volume and the water.
fn clear(data: &mut TerrainData, volume: &mut TerrainVolume, water: Option<&mut TerrainVoxelWater>) -> TerrainCommandEffect {
    let cells = data.cache_width as usize * data.cache_height as usize;
    data.sparse_surface = true;
    data.material_cache = vec![[MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0]; cells];
    data.material_dirty = true;
    volume.clear();
    let mut water_changed = false;
    if let Some(water) = water {
        water_changed = water.levels.iter().any(|level| !level.is_nan());
        water.levels.iter_mut().for_each(|level| *level = f32::NAN);
    }
    TerrainCommandEffect { water_changed, cleared: true, ..TerrainCommandEffect::default() }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::height_query::{ensure_material_cache, height_at_world};
    use crate::terrain::history::{apply_terrain_bricks, apply_terrain_tiles, TerrainTileSide};
    use crate::terrain::material::material_cell;
    use crate::terrain::volume::VolumeBrick;
    use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_4};

    /// 5 x 5 chunks of 32 m at 16 cells: a 2 m lattice under an 80 x 80
    /// raster spanning world -64..96 on both axes, heights 0..64 m.
    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 2,
            chunks_z: 2,
            center_chunk: IVec2::ZERO,
            lod_levels: 1,
            lod_distances: vec![64.0],
            view_distance: 512.0,
            height_scale: 64.0,
            height_offset: 0.0,
            seed: 1,
        }
    }

    fn flat(config: &TerrainConfig, world_y: f32) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let normalized = config.normalized_height(world_y);
        data.height_cache.iter_mut().for_each(|h| *h = normalized);
        data
    }

    /// Every brick of `volume`, in a fixed order, for comparing volumes.
    fn bricks_of(volume: &TerrainVolume) -> Vec<(IVec3, VolumeBrick)> {
        let mut bricks: Vec<(IVec3, VolumeBrick)> = volume.bricks().map(|(coord, brick)| (coord, brick.clone())).collect();
        bricks.sort_by_key(|(coord, _)| (coord.x, coord.y, coord.z));
        bricks
    }

    fn rock() -> TerrainFill {
        TerrainFill::Material(TerrainMaterial::Rock)
    }

    #[test]
    fn material_names_read_like_roblox() {
        assert_eq!(TerrainFill::from_material_name("Grass"), Some(TerrainFill::Material(TerrainMaterial::Grass)));
        assert_eq!(TerrainFill::from_material_name("enum.material.rock"), Some(TerrainFill::Material(TerrainMaterial::Rock)));
        assert_eq!(TerrainFill::from_material_name("Air"), Some(TerrainFill::Air));
        assert_eq!(TerrainFill::from_material_name("Water"), Some(TerrainFill::Water));
        assert_eq!(TerrainFill::from_material_name("Plastic"), None);
        assert_eq!(TerrainFill::from_material_name("Material.Sand"), Some(TerrainFill::Material(TerrainMaterial::Sand)));
        assert_eq!(
            TerrainFill::from_material_name("Enum.Material.WoodPlanks"),
            Some(TerrainFill::Material(TerrainMaterial::WoodPlanks))
        );
        assert_eq!(TerrainFill::from_material_name("Enum.Material.Water"), Some(TerrainFill::Water));
        assert_eq!(TerrainFill::from_material_name("Enum.Material."), None);
        for fill in [TerrainFill::Air, TerrainFill::Water, TerrainFill::Material(TerrainMaterial::CrackedLava)] {
            assert_eq!(TerrainFill::from_material_name(fill.name()), Some(fill), "{fill:?} reads back from its name");
        }
    }

    #[test]
    fn a_solid_ball_fills_its_centre_and_an_air_ball_empties_it() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let centre = Vec3::new(0.0, 12.0, 0.0);
        // One 2 m voxel around the ball's centre.
        let read = |data: &TerrainData, volume: &TerrainVolume| {
            read_voxels(&config, data, volume, None, centre - Vec3::ONE, 2.0, UVec3::ONE)
        };
        assert_eq!(read(&data, &volume), (vec![TerrainFill::Air], vec![0.0]), "open air before the fill");

        let solid = TerrainCommand::FillBall {
            center: centre,
            radius: 4.0,
            fill: TerrainFill::Material(TerrainMaterial::Sand),
        };
        let effect = apply_terrain_command(&config, &mut data, &mut volume, None, &solid).expect("fills");
        assert!(effect.volume_edit.is_some());
        assert!(effect.height_rect.is_none() && effect.material_rect.is_none() && !effect.water_changed);
        let (fills, occupancies) = read(&data, &volume);
        assert!(occupancies[0] > 0.5, "the centre is solid: {}", occupancies[0]);
        assert_eq!(fills[0], TerrainFill::Material(TerrainMaterial::Sand));

        let air = TerrainCommand::FillBall { center: centre, radius: 4.0, fill: TerrainFill::Air };
        let effect = apply_terrain_command(&config, &mut data, &mut volume, None, &air).expect("carves");
        assert!(effect.volume_edit.is_some());
        let (fills, occupancies) = read(&data, &volume);
        assert!(occupancies[0] < 0.5, "the centre is empty again: {}", occupancies[0]);
        assert_eq!(fills[0], TerrainFill::Air);
    }

    #[test]
    fn unturned_oriented_shapes_are_exactly_the_axis_aligned_ones() {
        let config = config();
        // Whole and half metres, so a region's centre and half size are exact.
        let center = Vec3::new(4.0, 8.0, -6.0);
        let half_extents = Vec3::new(5.0, 2.5, 3.5);
        let basalt = Some(TerrainMaterial::Basalt);

        let mut axis = TerrainVolume::new();
        let mut oriented = TerrainVolume::new();
        let a = apply_shape(&config, &mut axis, CsgShape::AxisBox { center, half_extents }, CsgOp::Add, basalt);
        let turned = CsgShape::OrientedBox { center, half_extents, rotation: Quat::IDENTITY };
        let b = apply_shape(&config, &mut oriented, turned, CsgOp::Add, basalt);
        assert!(!a.is_empty());
        assert_eq!(a, b, "the same edit");
        assert_eq!(bricks_of(&axis), bricks_of(&oriented), "the same bricks, cell for cell");
        assert_eq!(turned.edit_bounds(&config), CsgShape::AxisBox { center, half_extents }.edit_bounds(&config));

        let mut upright = TerrainVolume::new();
        let mut oriented = TerrainVolume::new();
        let cylinder = CsgShape::Cylinder { center, radius: 3.0, half_height: 4.5 };
        let turned = CsgShape::OrientedCylinder { center, radius: 3.0, half_height: 4.5, rotation: Quat::IDENTITY };
        let a = apply_shape(&config, &mut upright, cylinder, CsgOp::Carve, basalt);
        let b = apply_shape(&config, &mut oriented, turned, CsgOp::Carve, basalt);
        assert!(!a.is_empty());
        assert_eq!(a, b);
        assert_eq!(bricks_of(&upright), bricks_of(&oriented));
        assert_eq!(turned.bounds(), cylinder.bounds());

        // Through the commands: an unturned block is the region of its box.
        let mut data = flat(&config, 0.0);
        let mut block = TerrainVolume::new();
        let mut region = TerrainVolume::new();
        let fill = TerrainFill::Material(TerrainMaterial::Basalt);
        let by_block = TerrainCommand::FillBlock { center, rotation: Quat::IDENTITY, size: half_extents * 2.0, fill };
        let by_region = TerrainCommand::FillRegion { min: center - half_extents, max: center + half_extents, fill };
        let a = apply_terrain_command(&config, &mut data, &mut block, None, &by_block).unwrap();
        let b = apply_terrain_command(&config, &mut data, &mut region, None, &by_region).unwrap();
        assert_eq!(a.volume_edit, b.volume_edit);
        assert_eq!(bricks_of(&block), bricks_of(&region));
    }

    #[test]
    fn a_turned_box_bounds_hold_every_corner() {
        let config = config();
        let center = Vec3::new(1.0, 5.0, -2.0);
        let half_extents = Vec3::new(6.0, 1.0, 2.0);
        for rotation in [Quat::from_rotation_y(FRAC_PI_4), Quat::from_rotation_z(FRAC_PI_4) * Quat::from_rotation_x(0.3)] {
            let shape = CsgShape::OrientedBox { center, half_extents, rotation };
            let (lo, hi) = shape.edit_bounds(&config);
            let (tight_lo, tight_hi) = shape.bounds();
            let mut reach = Vec3::ZERO;
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let p = center + rotation * (half_extents * sign);
                assert!(p.cmpge(lo).all() && p.cmple(hi).all(), "corner {p} outside the edit bounds {lo}..{hi}");
                let slack = Vec3::splat(1e-4);
                assert!(p.cmpge(tight_lo - slack).all() && p.cmple(tight_hi + slack).all(), "corner {p} outside the bounds");
                assert!(shape.distance(p).abs() < 1e-4, "corner {p} is on the surface");
                reach = reach.max((p - center).abs());
            }
            assert!(((tight_hi - center) - reach).abs().max_element() < 1e-4, "the bounds touch the corners");
            // A point halfway along the long axis is a metre inside.
            assert!((shape.distance(center + rotation * Vec3::new(5.0, 0.0, 0.0)) + 1.0).abs() < 1e-4);
        }
        // Turned 45 degrees about Y, the long side and the short side reach
        // along X together.
        let (_, hi) = CsgShape::OrientedBox { center, half_extents, rotation: Quat::from_rotation_y(FRAC_PI_4) }.bounds();
        assert!((hi.x - center.x - (6.0 + 2.0) * FRAC_1_SQRT_2).abs() < 1e-4);
        assert!((hi.y - center.y - 1.0).abs() < 1e-5);

        // A cylinder lying along X reaches its half height along X. The
        // turn's rounding leaves the axis a hair off X, which the square
        // root in the disc's reach magnifies to under a millimetre.
        let lying = CsgShape::OrientedCylinder {
            center,
            radius: 2.0,
            half_height: 5.0,
            rotation: Quat::from_rotation_z(FRAC_PI_2),
        };
        let (lo, hi) = lying.bounds();
        assert!(((hi - lo) - Vec3::new(10.0, 4.0, 4.0)).abs().max_element() < 1e-2, "{lo}..{hi}");
        assert!(lying.distance(center + Vec3::new(4.0, 0.0, 0.0)) < 0.0, "inside along its axis");
        assert!(lying.distance(center + Vec3::new(0.0, 3.0, 0.0)) > 0.0, "outside across it");
    }

    #[test]
    fn a_water_block_floods_exactly_its_footprint_and_air_drains_it() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let mut water = TerrainVoxelWater::default();
        let grid = CellGrid::of(&config, &data).expect("a whole raster");
        let under_block = |p: Vec2| p.x.abs() <= 4.0 && p.y.abs() <= 4.0;

        let block = TerrainCommand::FillBlock {
            center: Vec3::new(0.0, 5.0, 0.0),
            rotation: Quat::IDENTITY,
            size: Vec3::new(8.0, 10.0, 8.0),
            fill: TerrainFill::Water,
        };
        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &block).unwrap();
        assert!(effect.water_changed && effect.volume_edit.is_none(), "water leaves the ground alone");
        assert!(volume.is_empty());
        assert_eq!((water.width, water.height), (data.cache_width, data.cache_height), "sized to the raster");
        let mut wet = 0;
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                let level = water.levels[z * grid.width + x];
                if under_block(p) {
                    assert_eq!(level, 10.0, "cell {p} under the block");
                    wet += 1;
                } else {
                    assert!(level.is_nan(), "cell {p} is outside the block but holds water at {level}");
                }
            }
        }
        assert!(wet >= 9, "{wet} cells flooded");

        // An air ball through the surface lowers it to the ball's bottom.
        let ball = TerrainCommand::FillBall { center: Vec3::new(0.0, 10.0, 0.0), radius: 3.0, fill: TerrainFill::Air };
        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &ball).unwrap();
        assert!(effect.water_changed && effect.volume_edit.is_some());
        let mut lowered = 0;
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                let level = water.levels[z * grid.width + x];
                if !under_block(p) {
                    assert!(level.is_nan());
                } else if p.length() <= 3.0 {
                    let expected = 10.0 - (9.0 - p.length_squared()).sqrt();
                    assert!((level - expected).abs() < 1e-4, "cell {p}: {level} against {expected}");
                    lowered += 1;
                } else {
                    assert_eq!(level, 10.0, "cell {p} beyond the ball");
                }
            }
        }
        assert!(lowered > 0);

        // An air shaft down to the ground dries its columns.
        let shaft = TerrainCommand::FillRegion {
            min: Vec3::new(-1.5, -1.0, -1.5),
            max: Vec3::new(1.5, 12.0, 1.5),
            fill: TerrainFill::Air,
        };
        apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &shaft).unwrap();
        let mut dried = 0;
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                if p.x.abs() <= 1.5 && p.y.abs() <= 1.5 {
                    assert!(water.levels[z * grid.width + x].is_nan(), "cell {p} under the shaft");
                    dried += 1;
                }
            }
        }
        assert!(dried > 0);
    }

    #[test]
    fn water_fills_only_the_air_and_drains_without_carving() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let grid = CellGrid::of(&config, &data).expect("a whole raster");
        // A ridge 20 m high east of x = 8 stands above the 10 m level.
        let ridge = config.normalized_height(20.0);
        for z in 0..grid.height {
            for x in 0..grid.width {
                if grid.world(x, z).x > 8.0 {
                    data.height_cache[z * grid.width + x] = ridge;
                }
            }
        }
        let heights = data.height_cache.clone();
        let mut volume = TerrainVolume::new();
        let fill = TerrainCommand::FillWater { min: Vec3::new(-16.0, -1.0, -16.0), max: Vec3::new(16.0, 10.0, 16.0) };
        assert!(command_needs_water(&fill));
        assert_eq!(command_bounds(&config, &fill), Some((Vec3::new(-16.0, -1.0, -16.0), Vec3::new(16.0, 10.0, 16.0))));
        assert!(apply_terrain_command(&config, &mut data, &mut volume, None, &fill).is_err(), "no water component");

        let mut water = TerrainVoxelWater::default();
        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &fill).unwrap();
        assert!(effect.water_changed && effect.height_rect.is_none() && effect.volume_edit.is_none());
        let in_rect = |p: Vec2, half: f32| p.x.abs() <= half && p.y.abs() <= half;
        let (mut wet, mut ridge_dry) = (0, 0);
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                let level = water.levels[z * grid.width + x];
                if in_rect(p, 16.0) && p.x <= 8.0 {
                    assert_eq!(level, 10.0, "cell {p} is low ground inside the rectangle");
                    wet += 1;
                } else {
                    assert!(level.is_nan(), "cell {p} holds water at {level}");
                    ridge_dry += usize::from(in_rect(p, 16.0));
                }
            }
        }
        assert!(wet > 50 && ridge_dry > 10, "{wet} cells wet, {ridge_dry} ridge cells dry");

        // A lower fill changes nothing: water never falls to a fill.
        let lower = TerrainCommand::FillWater { min: Vec3::new(-16.0, -1.0, -16.0), max: Vec3::new(16.0, 5.0, 16.0) };
        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &lower).unwrap();
        assert!(!effect.water_changed);

        // A drain down past the ground dries its columns; one whose bottom
        // stands above the ground lowers them to it. Neither carves.
        let drain = TerrainCommand::DrainWater { min: Vec3::new(-4.0, -1.0, -4.0), max: Vec3::new(4.0, 12.0, 4.0) };
        assert!(!command_needs_water(&drain));
        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &drain).unwrap();
        assert!(effect.water_changed && effect.volume_edit.is_none());
        let shallow = TerrainCommand::DrainWater { min: Vec3::new(-16.0, 6.0, -16.0), max: Vec3::new(-8.0, 12.0, 16.0) };
        apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &shallow).unwrap();
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                let level = water.levels[z * grid.width + x];
                if in_rect(p, 4.0) {
                    assert!(level.is_nan(), "cell {p} under the drain still holds water at {level}");
                } else if in_rect(p, 16.0) && p.x <= -8.0 {
                    assert_eq!(level, 6.0, "cell {p} drained to the box's bottom");
                } else if in_rect(p, 16.0) && p.x <= 8.0 {
                    assert_eq!(level, 10.0, "cell {p} is outside both drains");
                }
            }
        }
        assert!(volume.is_empty(), "no brick was written");
        assert_eq!(data.height_cache, heights, "the ground never moved");

        // Draining a terrain without water is no change, not an error.
        let effect = apply_terrain_command(&config, &mut data, &mut volume, None, &drain).unwrap();
        assert!(effect.is_empty());
    }

    #[test]
    fn a_material_swap_changes_only_what_is_inside_its_box() {
        let config = config();
        let mut data = flat(&config, 0.0);
        ensure_material_cache(&mut data);
        let grid = CellGrid::of(&config, &data).unwrap();
        let (grass, rock, sand) =
            (TerrainMaterial::Grass.to_u8(), TerrainMaterial::Rock.to_u8(), TerrainMaterial::Sand.to_u8());
        let cell_near = |x: f32, z: f32| cell_at(&grid, Vec2::new(x, z)).unwrap();
        // A raised cell and a mixed cell inside the box's rectangle.
        let raised = cell_near(4.0, 4.0);
        data.height_cache[raised] = config.normalized_height(20.0);
        let mixed = cell_near(-4.0, 4.0);
        data.material_cache[mixed] = canonical_material_cell(rock, grass, 60);

        let mut volume = TerrainVolume::new();
        let blob = TerrainFill::Material(TerrainMaterial::Grass);
        for center in [Vec3::new(0.0, 5.0, 0.0), Vec3::new(40.0, 5.0, 40.0)] {
            let ball = TerrainCommand::FillBall { center, radius: 2.0, fill: blob };
            apply_terrain_command(&config, &mut data, &mut volume, None, &ball).unwrap();
        }

        let swap = TerrainCommand::ReplaceMaterial {
            min: Vec3::new(-10.0, -1.0, -10.0),
            max: Vec3::new(10.0, 8.0, 10.0),
            from: TerrainMaterial::Grass,
            to: TerrainMaterial::Sand,
        };
        let effect = apply_terrain_command(&config, &mut data, &mut volume, None, &swap).unwrap();
        assert!(effect.material_rect.is_some() && effect.volume_edit.is_some() && effect.height_rect.is_none());
        assert!(data.material_dirty);
        for z in 0..grid.height {
            for x in 0..grid.width {
                let index = z * grid.width + x;
                let p = grid.world(x, z);
                let inside = p.x.abs() <= 10.0 && p.y.abs() <= 10.0;
                let cell = data.material_cache[index];
                if index == raised {
                    assert_eq!(cell, material_cell(grass), "the raised cell stands above the box");
                } else if index == mixed {
                    assert_eq!(cell, canonical_material_cell(rock, sand, 60), "the mix keeps its weights");
                } else if inside {
                    assert_eq!(cell, material_cell(sand), "cell {p} inside the box");
                } else {
                    assert_eq!(cell, material_cell(grass), "cell {p} outside the box");
                }
            }
        }
        assert_eq!(material_at(&config, &volume, Vec3::new(0.0, 7.0, 0.0)), Some(TerrainMaterial::Sand));
        assert_eq!(material_at(&config, &volume, Vec3::new(40.0, 7.0, 40.0)), Some(TerrainMaterial::Grass));
    }

    #[test]
    fn written_voxels_read_back_at_their_centres() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let size = UVec3::new(4, 4, 4);
        let count = 64;
        let solid = |i: usize| (i % 4 + i / 4 % 4 + i / 16) % 2 == 0;
        let materials: Vec<TerrainFill> = (0..count)
            .map(|i| if solid(i) { TerrainFill::Material(TerrainMaterial::Slate) } else { TerrainFill::Air })
            .collect();
        let occupancies: Vec<f32> = (0..count).map(|i| if solid(i) { 1.0 } else { 0.0 }).collect();
        // 2 m voxels from half a voxel off the 2 m lattice, so every centre is
        // a lattice point: one region in open air, one under the ground.
        for min in [Vec3::new(-1.0, 19.0, -1.0), Vec3::new(9.0, -11.0, 9.0)] {
            let write = TerrainCommand::WriteVoxels {
                min,
                resolution: 2.0,
                size,
                materials: materials.clone(),
                occupancies: occupancies.clone(),
            };
            let effect = apply_terrain_command(&config, &mut data, &mut volume, None, &write).unwrap();
            assert!(effect.volume_edit.is_some() && !effect.water_changed);
            let (fills, read) = read_voxels(&config, &data, &volume, None, min, 2.0, size);
            assert_eq!((fills.len(), read.len()), (count, count));
            for i in 0..count {
                assert!((read[i] - occupancies[i]).abs() <= 0.1, "voxel {i} from {min}: wrote {}, read {}", occupancies[i], read[i]);
                assert_eq!(fills[i], materials[i], "voxel {i} from {min}");
            }
        }
        // Writing the same voxels again changes nothing.
        let again = TerrainCommand::WriteVoxels {
            min: Vec3::new(-1.0, 19.0, -1.0),
            resolution: 2.0,
            size,
            materials: materials.clone(),
            occupancies: occupancies.clone(),
        };
        assert!(apply_terrain_command(&config, &mut data, &mut volume, None, &again).unwrap().is_empty());
    }

    #[test]
    fn water_voxels_set_their_columns_and_read_back_as_water() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let mut water = TerrainVoxelWater::default();
        let grid = CellGrid::of(&config, &data).unwrap();
        // One column of four 2 m voxels from the ground up: full water, half
        // water, then air.
        let min = Vec3::new(-1.0, 0.0, -1.0);
        let fills = vec![TerrainFill::Water, TerrainFill::Water, TerrainFill::Air, TerrainFill::Air];
        let occupancies = vec![1.0, 0.5, 0.0, 0.0];
        let write = TerrainCommand::WriteVoxels { min, resolution: 2.0, size: UVec3::new(1, 4, 1), materials: fills, occupancies };
        assert!(command_needs_water(&write));
        assert!(apply_terrain_command(&config, &mut data, &mut volume, None, &write).is_err(), "no water component");
        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &write).unwrap();
        assert!(effect.water_changed);
        let (x0, z0, x1, z1) = cells_in(&grid, Vec2::splat(-1.0), Vec2::splat(1.0)).expect("the column covers cells");
        for z in 0..grid.height {
            for x in 0..grid.width {
                let level = water.levels[z * grid.width + x];
                if (x0..=x1).contains(&x) && (z0..=z1).contains(&z) {
                    assert_eq!(level, 3.0, "the half-full voxel's top");
                } else {
                    assert!(level.is_nan());
                }
            }
        }
        let (read_fills, read) = read_voxels(&config, &data, &volume, Some(&water), min, 2.0, UVec3::new(1, 4, 1));
        assert_eq!(read_fills, vec![TerrainFill::Water, TerrainFill::Water, TerrainFill::Air, TerrainFill::Air]);
        assert!((read[0] - 1.0).abs() < 1e-5 && (read[1] - 0.5).abs() < 1e-5 && read[2] == 0.0);

        // The same column written without water drains to the region's floor,
        // which is the ground: dry.
        let dry = TerrainCommand::WriteVoxels {
            min,
            resolution: 2.0,
            size: UVec3::new(1, 4, 1),
            materials: vec![TerrainFill::Air; 4],
            occupancies: vec![0.0; 4],
        };
        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &dry).unwrap();
        assert!(effect.water_changed);
        assert!(water.levels.iter().all(|level| level.is_nan()));
    }

    #[test]
    fn clear_leaves_nothing_but_holes() {
        let config = config();
        let mut data = flat(&config, 5.0);
        let mut volume = TerrainVolume::new();
        let mut water = TerrainVoxelWater::default();
        let ball = TerrainCommand::FillBall { center: Vec3::new(0.0, 12.0, 0.0), radius: 3.0, fill: rock() };
        apply_terrain_command(&config, &mut data, &mut volume, None, &ball).unwrap();
        let lake = TerrainCommand::FillRegion { min: Vec3::splat(-8.0), max: Vec3::splat(8.0), fill: TerrainFill::Water };
        apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &lake).unwrap();
        assert!(!volume.is_empty() && water.levels.iter().any(|level| level.is_finite()));

        let effect = apply_terrain_command(&config, &mut data, &mut volume, Some(&mut water), &TerrainCommand::Clear).unwrap();
        assert!(effect.cleared && effect.water_changed);
        assert!(data.sparse_surface && data.material_dirty);
        assert!(data.has_material_layer());
        assert!(data.material_cache.iter().all(|cell| *cell == [MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0]));
        assert!((0..data.material_cache.len()).all(|index| data.cell_is_hole(index)), "every cell is a hole");
        assert!(volume.is_empty());
        assert!(water.levels.iter().all(|level| level.is_nan()));
        // Nothing reads solid, not even deep under the old ground.
        let (fills, occupancies) =
            read_voxels(&config, &data, &volume, Some(&water), Vec3::new(-8.0, -20.0, -8.0), 4.0, UVec3::new(4, 8, 4));
        assert!(fills.iter().all(|fill| *fill == TerrainFill::Air));
        assert!(occupancies.iter().all(|occupancy| *occupancy == 0.0));
        // Paint gives no hole ground.
        let paint = TerrainCommand::Paint { center: Vec3::ZERO, radius: 6.0, material: TerrainMaterial::Rock.to_u8(), strength: 1.0 };
        assert!(apply_terrain_command(&config, &mut data, &mut volume, None, &paint).unwrap().is_empty());
    }

    #[test]
    fn unusable_commands_are_refused_and_change_nothing() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let heights = data.height_cache.clone();
        let mut volume = TerrainVolume::new();
        let at = Vec3::new(3.0, 2.0, 1.0);
        let voxels = |size: UVec3, materials: usize, occupancies: usize| TerrainCommand::WriteVoxels {
            min: Vec3::ZERO,
            resolution: 2.0,
            size,
            materials: vec![rock(); materials],
            occupancies: vec![1.0; occupancies],
        };
        let refused = [
            ("a NaN centre", TerrainCommand::FillBall { center: Vec3::NAN, radius: 2.0, fill: rock() }),
            ("a zero radius", TerrainCommand::FillBall { center: at, radius: 0.0, fill: rock() }),
            (
                "a negative size",
                TerrainCommand::FillBlock { center: at, rotation: Quat::IDENTITY, size: Vec3::new(2.0, -1.0, 2.0), fill: rock() },
            ),
            (
                "a zero rotation",
                TerrainCommand::FillBlock { center: at, rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0), size: Vec3::ONE, fill: rock() },
            ),
            (
                "an infinite height",
                TerrainCommand::FillCylinder { center: at, rotation: Quat::IDENTITY, height: f32::INFINITY, radius: 1.0, fill: rock() },
            ),
            (
                "a flat region",
                TerrainCommand::FillRegion { min: Vec3::ONE, max: Vec3::new(3.0, 1.0, 3.0), fill: rock() },
            ),
            (
                "an inverted region",
                TerrainCommand::ReplaceMaterial {
                    min: Vec3::splat(4.0),
                    max: Vec3::ZERO,
                    from: TerrainMaterial::Grass,
                    to: TerrainMaterial::Sand,
                },
            ),
            (
                "a zero resolution",
                TerrainCommand::WriteVoxels {
                    min: Vec3::ZERO,
                    resolution: 0.0,
                    size: UVec3::ONE,
                    materials: vec![rock()],
                    occupancies: vec![1.0],
                },
            ),
            ("no voxels", voxels(UVec3::new(0, 1, 1), 0, 0)),
            ("too many voxels", voxels(UVec3::new(256, 256, 65), 0, 0)),
            ("too few materials", voxels(UVec3::new(2, 2, 2), 7, 8)),
            ("too few occupancies", voxels(UVec3::new(2, 2, 2), 8, 7)),
            (
                "a NaN occupancy",
                TerrainCommand::WriteVoxels {
                    min: Vec3::ZERO,
                    resolution: 2.0,
                    size: UVec3::ONE,
                    materials: vec![rock()],
                    occupancies: vec![f32::NAN],
                },
            ),
            (
                // 8 m voxels over a 2 m lattice: 257 lattice points a side.
                "voxels past the lattice cap",
                TerrainCommand::WriteVoxels {
                    min: Vec3::ZERO,
                    resolution: 8.0,
                    size: UVec3::splat(64),
                    materials: vec![rock(); 64 * 64 * 64],
                    occupancies: vec![1.0; 64 * 64 * 64],
                },
            ),
            (
                "painting no material",
                TerrainCommand::Paint { center: at, radius: 3.0, material: MATERIAL_SLOT_NONE, strength: 1.0 },
            ),
            (
                "a NaN strength",
                TerrainCommand::Sculpt { mode: TerrainSculptMode::Raise, center: at, radius: 3.0, strength: f32::NAN },
            ),
            (
                "a NaN flatten height",
                TerrainCommand::Sculpt {
                    mode: TerrainSculptMode::Flatten { height: f32::NAN },
                    center: at,
                    radius: 3.0,
                    strength: 1.0,
                },
            ),
            ("water without a water component", TerrainCommand::FillBall { center: at, radius: 2.0, fill: TerrainFill::Water }),
            ("a ball past the lattice cap", TerrainCommand::FillBall { center: at, radius: 400.0, fill: rock() }),
            ("an air ball past the lattice cap", TerrainCommand::FillBall { center: at, radius: 400.0, fill: TerrainFill::Air }),
        ];
        for (what, command) in &refused {
            assert!(apply_terrain_command(&config, &mut data, &mut volume, None, command).is_err(), "{what} was accepted");
        }
        assert!(volume.is_empty(), "a refused command changes no brick");
        assert_eq!(data.height_cache, heights, "or height");
        assert!(data.material_cache.is_empty(), "or material");

        // Procedural terrain has no raster to edit, not even to clear.
        let mut procedural = TerrainData::procedural();
        let ball = TerrainCommand::FillBall { center: at, radius: 2.0, fill: rock() };
        assert!(apply_terrain_command(&config, &mut procedural, &mut volume, None, &ball).is_err());
        assert!(apply_terrain_command(&config, &mut procedural, &mut volume, None, &TerrainCommand::Clear).is_err());
        assert!(volume.is_empty());

        // Reads of an unusable region stay in bounds.
        assert_eq!(read_voxels(&config, &data, &volume, None, Vec3::ZERO, 2.0, UVec3::new(0, 4, 4)).0.len(), 0);
        let (fills, occupancies) = read_voxels(&config, &data, &volume, None, Vec3::NAN, 2.0, UVec3::new(2, 2, 2));
        assert_eq!((fills.len(), occupancies.len()), (8, 8));
    }

    #[test]
    fn sculpt_and_paint_go_through_the_editor_brush() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let at = Vec3::new(8.0, 0.0, 8.0);
        let mut apply = |data: &mut TerrainData, command: TerrainCommand| {
            apply_terrain_command(&config, data, &mut volume, None, &command).expect("applies")
        };

        let raise = TerrainCommand::Sculpt { mode: TerrainSculptMode::Raise, center: at, radius: 6.0, strength: 1.0 };
        let effect = apply(&mut data, raise);
        assert_eq!(effect.height_rect, Some((Vec2::new(2.0, 2.0), Vec2::new(14.0, 14.0))));
        // Full strength raises the centre by about a tenth of the 64 m band,
        // somewhat more where the brush reads ground its earlier samples of
        // the same dab already raised.
        let raised = height_at_world(&config, &data, at.x, at.z);
        assert!(raised > 3.0 && raised < 13.0, "{raised}");
        assert_eq!(height_at_world(&config, &data, at.x + 20.0, at.z), 0.0, "ground outside the disc stays");

        let flatten = TerrainCommand::Sculpt {
            mode: TerrainSculptMode::Flatten { height: 3.0 },
            center: at,
            radius: 6.0,
            strength: 1.0,
        };
        apply(&mut data, flatten);
        let flattened = height_at_world(&config, &data, at.x, at.z);
        assert!((flattened - 3.0).abs() < 1.5, "{flattened}");

        let idle = TerrainCommand::Sculpt { mode: TerrainSculptMode::Lower, center: at, radius: 6.0, strength: 0.0 };
        assert!(apply(&mut data, idle).is_empty(), "no strength, no change");

        // The first paint allocates the material layer, which recolours every chunk.
        let sand = TerrainMaterial::Sand.to_u8();
        let paint = TerrainCommand::Paint { center: at, radius: 4.0, material: sand, strength: 1.0 };
        let effect = apply(&mut data, paint);
        assert_eq!(effect.material_rect, Some(config.footprint_xz()));
        assert!(effect.height_rect.is_none());
        assert_eq!(material_at_world(&config, &data, at.x, at.z).map(|cell| cell.primary), Some(sand));
        let outside = material_at_world(&config, &data, at.x + 10.0, at.z).map(|cell| cell.primary);
        assert_eq!(outside, Some(TerrainMaterial::Grass.to_u8()));
        // A later paint marks its own disc.
        let paint = TerrainCommand::Paint { center: at + Vec3::X * 20.0, radius: 3.0, material: sand, strength: 1.0 };
        let (lo, hi) = apply(&mut data, paint).material_rect.expect("painted");
        assert!(lo.x >= at.x + 17.0 - 1e-3 && hi.x <= at.x + 23.0 + 1e-3, "{lo}..{hi}");
    }

    #[test]
    fn recorded_commands_undo_exactly() {
        let config = config();
        let mut data = flat(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let heights = data.height_cache.clone();
        let commands = [
            TerrainCommand::FillBlock {
                center: Vec3::new(0.0, 6.0, 0.0),
                rotation: Quat::from_rotation_y(0.5),
                size: Vec3::new(10.0, 4.0, 6.0),
                fill: TerrainFill::Material(TerrainMaterial::Brick),
            },
            TerrainCommand::FillCylinder {
                center: Vec3::new(20.0, 0.0, 0.0),
                rotation: Quat::from_rotation_x(FRAC_PI_2),
                height: 12.0,
                radius: 3.0,
                fill: TerrainFill::Air,
            },
            TerrainCommand::Sculpt {
                mode: TerrainSculptMode::Raise,
                center: Vec3::new(-20.0, 0.0, -20.0),
                radius: 5.0,
                strength: 1.0,
            },
            TerrainCommand::WriteVoxels {
                min: Vec3::new(30.0, -3.0, 30.0),
                resolution: 4.0,
                size: UVec3::new(2, 2, 2),
                materials: vec![rock(); 8],
                occupancies: vec![0.0, 1.0, 0.5, 1.0, 0.0, 0.0, 1.0, 0.25],
            },
        ];
        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin("Terrain Commands", None, &config, &data));
        for command in &commands {
            record_terrain_command(&mut recorder, &config, &data, &volume, command);
            apply_terrain_command(&config, &mut data, &mut volume, None, command).expect("applies");
        }
        let edit = recorder.finish_with_volume(None, &data, &volume).expect("the batch changed the terrain");
        assert!(!edit.tiles.is_empty() && !edit.bricks.is_empty());

        let mut undone = data.clone();
        let mut undone_volume = volume.clone();
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::Before).unwrap();
        apply_terrain_bricks(&config, &mut undone_volume, &edit.bricks, TerrainTileSide::Before).unwrap();
        assert_eq!(undone.height_cache, heights, "undo puts back every height");
        assert!(undone_volume.is_empty(), "and removes every brick");
        apply_terrain_bricks(&config, &mut undone_volume, &edit.bricks, TerrainTileSide::After).unwrap();
        assert_eq!(bricks_of(&undone_volume), bricks_of(&volume), "redo brings them back");
    }

    #[test]
    fn bounds_cover_what_each_command_touches() {
        let config = config();
        assert_eq!(command_bounds(&config, &TerrainCommand::Clear), None);
        let ball = TerrainCommand::FillBall { center: Vec3::ONE, radius: 2.0, fill: rock() };
        assert_eq!(
            command_bounds(&config, &ball),
            Some(CsgShape::Sphere { center: Vec3::ONE, radius: 2.0 }.edit_bounds(&config))
        );
        let sculpt = TerrainCommand::Sculpt { mode: TerrainSculptMode::Smooth, center: Vec3::new(1.0, 2.0, 3.0), radius: 4.0, strength: 0.5 };
        let (lo, hi) = command_bounds(&config, &sculpt).unwrap();
        assert_eq!((lo.xz(), hi.xz()), (Vec2::new(-3.0, -1.0), Vec2::new(5.0, 7.0)));
        assert!(lo.y == f32::NEG_INFINITY && hi.y == f32::INFINITY, "a sculpt changes whole columns");
        // A voxel region reaches every lattice point it writes: 1 m voxels on
        // a 2 m lattice write the points around their centres.
        let write = TerrainCommand::WriteVoxels {
            min: Vec3::new(0.5, 0.5, 0.5),
            resolution: 1.0,
            size: UVec3::ONE,
            materials: vec![rock()],
            occupancies: vec![1.0],
        };
        assert_eq!(command_bounds(&config, &write), Some((Vec3::ZERO, Vec3::splat(2.0))));
    }
}
