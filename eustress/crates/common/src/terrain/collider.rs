//! Static physics colliders for terrain chunks.
//!
//! Each chunk gets one `RigidBody::Static` heightfield built from
//! [`super::chunk_height_grid`] at LOD 0, the same samples the full-detail
//! render mesh uses. The render LOD never feeds the collider: a coarse far-chunk
//! mesh would leave a body floating above, or sunk into, the ground it
//! reaches once it gets there, and LOD swaps would churn the broadphase.
//!
//! A chunk whose columns hold volumetric edits (see `volume`) collides on a
//! trimesh of its LOD-0 marching-cubes surface instead (see `marching`),
//! whatever LOD it renders at, since a heightfield cannot hold a cave.
//! A chunk of a sparse surface with holes in it (see
//! `TerrainData::sparse_surface`) collides on a trimesh of exactly the quads
//! its LOD-0 mesh keeps ([`super::chunk_ground_triangles`]), over the same
//! heights and along the same diagonals, and a chunk that keeps none has no
//! collider: a heightfield covers every cell of its chunk.
//! [`attach_chunk_collider`] and [`refresh_chunk_collider`] choose among them
//! for every chunk.
//!
//! Every trimesh is welded before it is built ([`weld_collider_triangles`]):
//! vertices closer than [`collider_weld_tolerance`] merge, and triangles
//! thinner than it, repeated, or on a vertex that is not finite are dropped.
//! Marching cubes puts a crossing exactly on a lattice point wherever the
//! field there is zero, which a cave cell beside a hole column does often, so
//! a marched surface can carry many coincident vertices and sliver triangles
//! that would spoil the internal-edge fix. A surface left with no triangle
//! has no trimesh. Should parry still refuse the welded mesh with
//! `FIX_INTERNAL_EDGES`, the trimesh is built without it (logged once) rather
//! than dropped.
//!
//! The collider lives on a child entity offset by half a chunk. parry's
//! heightfield is centred on its local origin (x and z span
//! `[-scale / 2, scale / 2]`), while chunk meshes span `[0, chunk_size]`
//! from the chunk entity's corner. `Collider::compound` cannot carry that
//! offset instead: parry panics on a heightfield inside a compound (nested
//! composite shapes are rejected).
//!
//! No collision layers are assigned. The play character, its ground probe and
//! every editor pick query use the default filter, which a default-layer
//! static collider already satisfies; [`TerrainChunkCollider`] is the marker
//! for code that wants to include or skip terrain explicitly.
//!
//! ## Friction
//!
//! A collider child takes its `Friction` from the chunk's dominant material
//! slot ([`dominant_chunk_slot`]) when that slot names a realism material the
//! registry resolves (`TerrainMaterialSlots::friction`); otherwise it keeps
//! Avian's default. [`apply_terrain_chunk_friction`] rescans a chunk whenever
//! its collider is built or rebuilt. Every material-map writer marks the
//! chunks it touched for a collider rebuild (see `dirty`), so a new collider
//! is the signal that the chunk's material may have changed, and this needs
//! no tracking of its own.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use super::height_query::cache_cell_at_world;
use super::material::{material_cell_weights, MATERIAL_SLOT_COUNT, MATERIAL_SLOT_NONE};
use super::{chunk_world_position, TerrainConfig, TerrainData, TerrainVolume};

#[cfg(feature = "physics")]
use super::{material_slots::TerrainMaterialSlots, surface_data, TerrainBaked, TerrainRoot};
#[cfg(feature = "physics")]
use avian3d::prelude::*;

/// Marker on the child entity that carries a chunk's static collider.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainChunkCollider {
    /// Grid position of the chunk this collider belongs to.
    pub chunk: IVec2,
}

/// Translation of the collider child relative to its chunk entity: half a
/// chunk along X and Z, which moves parry's origin-centred heightfield onto
/// the chunk mesh's `[0, chunk_size]` footprint.
pub fn chunk_collider_offset(config: &TerrainConfig) -> Vec3 {
    let half = config.chunk_size * 0.5;
    Vec3::new(half, 0.0, half)
}

/// Build the LOD-0 heightfield collider for `chunk_pos`, in the collider
/// child's local frame (see [`chunk_collider_offset`]). A chunk with holes
/// gets a trimesh of the quads its LOD-0 mesh keeps instead (see the module
/// docs). `None` when the config has no usable chunk size, and for a chunk
/// that keeps no quad.
#[cfg(feature = "physics")]
pub fn build_chunk_collider(
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &TerrainData,
) -> Option<Collider> {
    use avian3d::parry::shape::{HeightFieldFlags, SharedShape};
    use avian3d::parry::utils::Array2;

    if !(config.chunk_size.is_finite() && config.chunk_size > 0.0) {
        return None;
    }
    if let Some((positions, triangles)) = super::chunk_ground_triangles(chunk_pos, config, data) {
        // Heights the heightfield below would replace, replaced alike.
        let positions = positions
            .into_iter()
            .map(|p| if p.y.is_finite() { p } else { Vec3::new(p.x, config.height_offset, p.z) })
            .collect();
        return trimesh_collider_from_triangles(positions, triangles, config);
    }
    let resolution = config.resolution_for_lod(0);
    let stride = resolution as usize + 1;
    let grid = super::chunk_height_grid(chunk_pos, resolution, config, data);
    if grid.len() != stride * stride {
        return None;
    }

    // parry indexes heights as (row i, column j) with rows advancing along Z
    // and columns along X, stored column-major. Building the array from that
    // index pair keeps the mapping explicit. avian's
    // `Collider::heightfield(Vec<Vec<_>>)` flattens its rows row-major into
    // that column-major store, so its outer index runs along X: the
    // transpose of parry's convention, and an easy way to swap the axes.
    let heights = Array2::from_fn(stride, stride, |i, j| {
        let h = grid[i * stride + j];
        if h.is_finite() { h } else { config.height_offset }
    });
    let scale = Vec3::new(config.chunk_size, 1.0, config.chunk_size);
    // FIX_INTERNAL_EDGES stops a sliding capsule from catching on the shared
    // edges between heightfield triangles on flat or gently sloped ground.
    let shape = SharedShape::heightfield_with_flags(heights, scale, HeightFieldFlags::FIX_INTERNAL_EDGES);
    Some(Collider::from(shape))
}

/// Build the trimesh collider of a chunk holding volumetric edits from its
/// LOD-0 marching-cubes surface, in the collider child's local frame (see
/// [`chunk_collider_offset`]). `None` when the chunk's lattice cannot be
/// marched.
#[cfg(feature = "physics")]
pub fn build_volume_chunk_collider(
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) -> Option<Collider> {
    let (positions, triangles) = super::volume_chunk_triangles(chunk_pos, config, data, volume)?;
    trimesh_collider_from_triangles(positions, triangles, config)
}

/// The trimesh collider of a surface given as chunk-local positions and
/// triangles (a marched surface, or the ground a chunk with holes keeps), in
/// the collider child's local frame, welded first (see the module docs).
/// `None` when no triangle survives the weld.
#[cfg(feature = "physics")]
fn trimesh_collider_from_triangles(
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    config: &TerrainConfig,
) -> Option<Collider> {
    use std::sync::atomic::{AtomicBool, Ordering};

    // Set by the first trimesh built without FIX_INTERNAL_EDGES.
    static WITHOUT_EDGE_FIX_LOGGED: AtomicBool = AtomicBool::new(false);

    let tolerance = collider_weld_tolerance(config, &positions);
    let (positions, triangles) = weld_collider_triangles(&positions, &triangles, tolerance);
    if triangles.is_empty() {
        return None;
    }
    let offset = chunk_collider_offset(config);
    let vertices: Vec<Vec3> = positions.into_iter().map(|p| p - offset).collect();
    // FIX_INTERNAL_EDGES for the same reason as the heightfield's. The weld
    // left no coincident vertex or sliver for DELETE_DEGENERATE_TRIANGLES to
    // find; it stays as a guard.
    let flags = TrimeshFlags::FIX_INTERNAL_EDGES | TrimeshFlags::DELETE_DEGENERATE_TRIANGLES;
    match Collider::try_trimesh_with_config(vertices.clone(), triangles.clone(), flags) {
        Ok(collider) => Some(collider),
        Err(error) => {
            if !WITHOUT_EDGE_FIX_LOGGED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "Terrain collider: a chunk trimesh could not be built with FIX_INTERNAL_EDGES ({error}); \
                     it and any later one that fails alike are built without it"
                );
            }
            Collider::try_trimesh_with_config(vertices, triangles, TrimeshFlags::empty()).ok()
        }
    }
}

/// Fraction of a lattice cell (`lattice_cell_size`) within which
/// [`weld_collider_triangles`] merges vertices.
const WELD_CELL_FRACTION: f32 = 1e-3;

/// f32 steps at the largest coordinate that the weld tolerance always covers,
/// so vertices high above the origin, whose rounding is coarser, still weld.
const WELD_F32_STEPS: f32 = 8.0;

/// Largest weld-grid coordinate [`weld_cell`] rounds to an integer; past it a
/// vertex welds only with its exact copies.
const MAX_WELD_CELL: f32 = 1.0e15;

/// Distance within which the trimesh of a chunk welds its chunk-local
/// `positions` (see [`weld_collider_triangles`]): a thousandth of a lattice
/// cell, and at least [`WELD_F32_STEPS`] f32 steps at the largest finite
/// coordinate among them.
pub fn collider_weld_tolerance(config: &TerrainConfig, positions: &[Vec3]) -> f32 {
    let largest = positions
        .iter()
        .filter(|position| position.is_finite())
        .fold(0.0f32, |largest, position| largest.max(position.abs().max_element()));
    (super::lattice_cell_size(config) * WELD_CELL_FRACTION).max(largest * WELD_F32_STEPS * f32::EPSILON)
}

/// Weld a triangle list for a trimesh collider. Vertices landing in one cube
/// of a grid `tolerance` wide merge into the first of them; a triangle is
/// dropped when a vertex is past the buffer or not finite, when two of its
/// corners weld together, when it is no thicker than `tolerance` (its height
/// over its longest edge), and when it repeats the corners of a kept triangle
/// in any order. Returns the vertices the kept triangles use, in the order
/// they first use them, and the kept triangles, in their order and winding.
/// A tolerance that is not a positive number welds exact copies only.
pub fn weld_collider_triangles(
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    tolerance: f32,
) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let tolerance = if tolerance.is_finite() { tolerance.max(0.0) } else { 0.0 };
    let mut welded_of: Vec<Option<u32>> = vec![None; positions.len()];
    let mut cells: HashMap<(bool, [i64; 3]), u32> = HashMap::new();
    let mut welded: Vec<Vec3> = Vec::new();
    let mut seen: HashSet<[u32; 3]> = HashSet::new();
    let mut kept: Vec<[u32; 3]> = Vec::with_capacity(triangles.len());
    'triangles: for triangle in triangles {
        let mut ids = [0u32; 3];
        for (id, &index) in ids.iter_mut().zip(triangle) {
            let Some(welded_id) = weld_vertex(index, positions, tolerance, &mut welded_of, &mut cells, &mut welded)
            else {
                continue 'triangles;
            };
            *id = welded_id;
        }
        if ids[0] == ids[1] || ids[0] == ids[2] || ids[1] == ids[2] {
            continue;
        }
        let [a, b, c] = ids.map(|id| welded[id as usize]);
        let longest = (b - a).length().max((c - a).length()).max((c - b).length());
        // Twice the area over the longest edge is the height onto that edge;
        // written so a NaN drops the triangle too.
        if !((b - a).cross(c - a).length() > tolerance * longest) {
            continue;
        }
        let mut corners = ids;
        corners.sort_unstable();
        if seen.insert(corners) {
            kept.push(ids);
        }
    }

    let mut compact_of = vec![u32::MAX; welded.len()];
    let mut vertices: Vec<Vec3> = Vec::new();
    for triangle in &mut kept {
        for id in triangle.iter_mut() {
            let slot = &mut compact_of[*id as usize];
            if *slot == u32::MAX {
                *slot = vertices.len() as u32;
                vertices.push(welded[*id as usize]);
            }
            *id = *slot;
        }
    }
    (vertices, kept)
}

/// The welded vertex of `positions[index]` (see [`weld_collider_triangles`]),
/// made the first time its weld cell is met. `None` for an index past the
/// buffer or a position that is not finite.
fn weld_vertex(
    index: u32,
    positions: &[Vec3],
    tolerance: f32,
    welded_of: &mut [Option<u32>],
    cells: &mut HashMap<(bool, [i64; 3]), u32>,
    welded: &mut Vec<Vec3>,
) -> Option<u32> {
    let slot = welded_of.get_mut(index as usize)?;
    if slot.is_none() {
        let position = positions[index as usize];
        if !position.is_finite() {
            return None;
        }
        let id = *cells.entry(weld_cell(position, tolerance)).or_insert_with(|| {
            welded.push(position);
            (welded.len() - 1) as u32
        });
        *slot = Some(id);
    }
    *slot
}

/// The weld cell of a finite `position`: its coordinates over `tolerance`,
/// rounded, or (flagged) its exact bits where no grid applies, for a zero
/// tolerance or coordinates past [`MAX_WELD_CELL`].
fn weld_cell(position: Vec3, tolerance: f32) -> (bool, [i64; 3]) {
    let scaled = (position / tolerance).round();
    if tolerance > 0.0 && scaled.abs().max_element() <= MAX_WELD_CELL {
        (false, [scaled.x as i64, scaled.y as i64, scaled.z as i64])
    } else {
        (true, [position.x.to_bits().into(), position.y.to_bits().into(), position.z.to_bits().into()])
    }
}

/// The collider `chunk_pos` should have now: a trimesh of its marching-cubes
/// surface when bricks reach its columns, else its LOD-0 ground as
/// [`build_chunk_collider`] builds it (a heightfield, or with holes a trimesh
/// of the quads it keeps). A volumetric chunk that cannot be marched keeps
/// that ground, as its render mesh does.
#[cfg(feature = "physics")]
pub fn build_terrain_chunk_collider(
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) -> Option<Collider> {
    if !volume.is_empty() && super::chunk_has_volume(chunk_pos, config, volume) {
        if let Some(collider) = build_volume_chunk_collider(chunk_pos, config, data, volume) {
            return Some(collider);
        }
    }
    build_chunk_collider(chunk_pos, config, data)
}

/// Spawn the collider child for a freshly spawned chunk. A no-op without the
/// `physics` feature, so spawn sites need no cfg of their own.
///
/// `surface` is the chunk's marched surface when the spawn just built it for
/// the render mesh (see `generate_chunk_render_mesh_and_surface`), so a
/// volumetric chunk's trimesh reuses that march instead of sampling the
/// lattice a second time this frame. Without it, or when its trimesh cannot
/// be built, the collider is built as [`build_terrain_chunk_collider`] does.
pub fn attach_chunk_collider(
    commands: &mut Commands,
    chunk_entity: Entity,
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    surface: Option<(Vec<Vec3>, Vec<[u32; 3]>)>,
) {
    #[cfg(feature = "physics")]
    {
        let marched = surface
            .filter(|_| !volume.is_empty() && super::chunk_has_volume(chunk_pos, config, volume))
            .and_then(|(positions, triangles)| trimesh_collider_from_triangles(positions, triangles, config));
        if let Some(collider) = marched.or_else(|| build_terrain_chunk_collider(chunk_pos, config, data, volume)) {
            spawn_collider_child(commands, chunk_entity, chunk_pos, config, collider);
        }
    }
    #[cfg(not(feature = "physics"))]
    {
        let _ = (commands, chunk_entity, chunk_pos, config, data, volume, surface);
    }
}

/// Rebuild a chunk's collider from the current data, replacing the shape on
/// its existing collider child, or spawning the child if it has none. When
/// no collider can be built the stale child goes, so nothing keeps
/// colliding with ground that has changed.
#[cfg(feature = "physics")]
pub fn refresh_chunk_collider(
    commands: &mut Commands,
    chunk_entity: Entity,
    chunk_pos: IVec2,
    existing_child: Option<Entity>,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) {
    match (build_terrain_chunk_collider(chunk_pos, config, data, volume), existing_child) {
        (Some(collider), Some(child)) => {
            commands.entity(child).try_insert(collider);
        }
        (Some(collider), None) => spawn_collider_child(commands, chunk_entity, chunk_pos, config, collider),
        (None, Some(child)) => {
            commands.entity(child).try_despawn();
        }
        (None, None) => {}
    }
}

#[cfg(feature = "physics")]
fn spawn_collider_child(
    commands: &mut Commands,
    chunk_entity: Entity,
    chunk_pos: IVec2,
    config: &TerrainConfig,
    collider: Collider,
) {
    commands.spawn((
        TerrainChunkCollider { chunk: chunk_pos },
        Transform::from_translation(chunk_collider_offset(config)),
        RigidBody::Static,
        collider,
        Name::new(format!("ChunkCollider_{}_{}", chunk_pos.x, chunk_pos.y)),
        ChildOf(chunk_entity),
    ));
}

/// The material slot covering most of chunk `chunk_pos`'s ground: every
/// material-map cell in the chunk's footprint (border cells included, as the
/// mesh shares them) adds both of its slots' weights, and the heaviest slot
/// wins, ties going to the lower slot. `None` when the terrain has no
/// material layer or no cell there holds a material.
///
/// Reads the heightfield's material map only. Brick materials of a
/// volumetric chunk (`volume`) do not count; they cover caves and overhangs,
/// not the ground most bodies rest on.
pub fn dominant_chunk_slot(chunk_pos: IVec2, config: &TerrainConfig, data: &TerrainData) -> Option<u8> {
    if !data.has_material_layer() {
        return None;
    }
    let corner = chunk_world_position(chunk_pos, config);
    let size = config.chunk_size;
    let first = cache_cell_at_world(config, data, corner.x, corner.z)?;
    let last = cache_cell_at_world(config, data, corner.x + size, corner.z + size)?;
    let width = data.cache_width as usize;
    let mut totals = [0.0f32; MATERIAL_SLOT_COUNT];
    for z in first.y.min(last.y)..=first.y.max(last.y) {
        for x in first.x.min(last.x)..=first.x.max(last.x) {
            let Some(cell) = data.material_cache.get(z as usize * width + x as usize) else {
                continue;
            };
            for (slot, weight) in material_cell_weights(*cell) {
                if slot != MATERIAL_SLOT_NONE && weight > 0.0 {
                    totals[slot as usize] += weight;
                }
            }
        }
    }
    let mut best: Option<(u8, f32)> = None;
    for (slot, total) in totals.iter().enumerate() {
        // Strictly greater, so a tie keeps the lower slot.
        if *total > best.map_or(0.0, |(_, most)| most) {
            best = u8::try_from(slot).ok().map(|slot| (slot, *total));
        }
    }
    best.map(|(slot, _)| slot)
}

/// Which material slot a chunk collider takes its friction from, kept on the
/// collider child by [`apply_terrain_chunk_friction`].
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainChunkSurface {
    /// The chunk's [`dominant_chunk_slot`], `None` when the terrain has no
    /// material layer.
    pub dominant_slot: Option<u8>,
}

/// Give every terrain collider the friction of its chunk's dominant material
/// slot, or Avian's default where that slot has no physics material the
/// realism registry resolves (see the module docs).
///
/// A chunk is rescanned when its collider child is spawned or its shape is
/// rebuilt; a change to the slot table only remaps the stored dominant slots,
/// so editing a `.mat.toml` never rescans the whole material map.
#[cfg(feature = "physics")]
pub fn apply_terrain_chunk_friction(
    mut commands: Commands,
    slots: Res<TerrainMaterialSlots>,
    roots: Query<(&TerrainConfig, &TerrainData, Option<&TerrainBaked>), With<TerrainRoot>>,
    colliders: Query<(
        Entity,
        &TerrainChunkCollider,
        Ref<Collider>,
        Option<&TerrainChunkSurface>,
        Option<&Friction>,
    )>,
) {
    let remap_all = slots.is_changed();
    // The ground bodies stand on is the bake's when the root has layers.
    let root = roots.single().ok().map(|(config, data, baked)| (config, surface_data(data, baked)));
    for (entity, chunk, collider, surface, friction) in &colliders {
        let rescan = surface.is_none() || collider.is_changed();
        if !rescan && !remap_all {
            continue;
        }
        let dominant = if rescan {
            let Some((config, data)) = root else {
                // No terrain to read, or two roots mid-replacement: leave the
                // marker off so the chunk is scanned once one root remains.
                if surface.is_some() {
                    commands.entity(entity).try_remove::<TerrainChunkSurface>();
                }
                continue;
            };
            let marker = TerrainChunkSurface { dominant_slot: dominant_chunk_slot(chunk.chunk, config, data) };
            if surface != Some(&marker) {
                commands.entity(entity).try_insert(marker);
            }
            marker.dominant_slot
        } else {
            surface.and_then(|surface| surface.dominant_slot)
        };
        let wanted = dominant
            .and_then(|slot| slots.friction(slot))
            .map(|(static_coefficient, kinetic_coefficient)| {
                Friction::new(static_coefficient).with_dynamic_coefficient(kinetic_coefficient)
            });
        match (wanted, friction) {
            (Some(wanted), Some(current)) if *current == wanted => {}
            (Some(wanted), _) => {
                commands.entity(entity).try_insert(wanted);
            }
            (None, Some(_)) => {
                commands.entity(entity).try_remove::<Friction>();
            }
            (None, None) => {}
        }
    }
}

#[cfg(test)]
fn test_config() -> TerrainConfig {
    TerrainConfig {
        chunk_size: 64.0,
        chunk_resolution: 16,
        chunks_x: 2,
        chunks_z: 2,
        center_chunk: IVec2::ZERO,
        lod_levels: 3,
        lod_distances: vec![64.0, 128.0, 256.0],
        view_distance: 256.0,
        height_scale: 40.0,
        // A non-zero floor, so a collider that skipped `world_height` would
        // sit 10 m off the surface.
        height_offset: -10.0,
        seed: 7,
    }
}

/// A raster with no symmetry between X and Z, for checks at grid vertices.
#[cfg(test)]
fn bumpy_data(config: &TerrainConfig) -> TerrainData {
    let mut data = TerrainData::procedural();
    data.resize_cache(config);
    let w = data.cache_width as usize;
    let h = data.cache_height as usize;
    for z in 0..h {
        for x in 0..w {
            data.height_cache[z * w + x] =
                0.5 + 0.3 * (x as f32 * 0.37).sin() * (z as f32 * 0.21 + 0.4).cos() + 0.002 * x as f32;
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{chunk_height_grid, generate_chunk_mesh};

    #[test]
    fn lod0_mesh_vertices_are_the_collider_grid() {
        let config = test_config();
        let data = bumpy_data(&config);
        let chunk = IVec2::new(1, -1);
        let resolution = config.resolution_for_lod(0);
        let grid = chunk_height_grid(chunk, resolution, &config, &data);

        let mut meshes = Assets::<Mesh>::default();
        let handle = generate_chunk_mesh(chunk, 0, &config, &data, &mut meshes);
        let mesh = meshes.get(&handle).expect("mesh was just added");
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|values| values.as_float3())
            .expect("terrain mesh positions are Float32x3");

        let stride = resolution as usize + 1;
        let step = config.chunk_size / resolution as f32;
        for z in 0..stride {
            for x in 0..stride {
                let p = positions[z * stride + x];
                assert!((p[0] - x as f32 * step).abs() < 1e-4, "vertex ({x}, {z}) x = {}", p[0]);
                assert!((p[2] - z as f32 * step).abs() < 1e-4, "vertex ({x}, {z}) z = {}", p[2]);
                assert_eq!(p[1], grid[z * stride + x], "vertex ({x}, {z}) height");
            }
        }
    }

    #[test]
    fn the_dominant_slot_is_the_material_covering_most_of_the_chunk() {
        use crate::terrain::height_query::{ensure_material_cache, paint_material_at_world};
        use crate::terrain::TerrainMaterial;

        let config = test_config();
        let mut data = bumpy_data(&config);
        assert_eq!(dominant_chunk_slot(IVec2::new(0, 0), &config, &data), None, "no material layer yet");

        ensure_material_cache(&mut data);
        let grass = TerrainMaterial::Grass.to_u8();
        let ice = TerrainMaterial::Ice.to_u8();
        assert_eq!(dominant_chunk_slot(IVec2::new(1, 0), &config, &data), Some(grass));

        // The raster's 80 cells span 320 m, one every ~4.05 m. Chunk (1, 0)
        // (x 64..128, z 0..64) covers columns 47..=63; ice goes on 47..=58.
        let step = config.chunk_size / config.chunk_resolution as f32;
        let mut z = 0.0;
        while z <= 64.0 {
            let mut x = 64.0;
            while x <= 108.0 {
                paint_material_at_world(&config, &mut data, x, z, ice, 1.0);
                x += step;
            }
            z += step;
        }
        assert_eq!(dominant_chunk_slot(IVec2::new(1, 0), &config, &data), Some(ice));
        // Chunk (0, 0) shares only its border column with the ice; chunk
        // (2, 0) starts at column 63 and has none.
        assert_eq!(dominant_chunk_slot(IVec2::new(0, 0), &config, &data), Some(grass));
        assert_eq!(dominant_chunk_slot(IVec2::new(2, 0), &config, &data), Some(grass));
    }

    #[test]
    fn welding_merges_near_vertices_and_drops_degenerate_repeated_and_broken_triangles() {
        let positions = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1e-4, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(0.5, 1e-5, 0.5),
        ];
        let triangles: [[u32; 3]; 10] = [
            [0, 2, 1],  // kept
            [3, 2, 1],  // the first, once vertex 3 welds onto vertex 0
            [1, 2, 0],  // the first again, in another order
            [0, 1, 4],  // collinear
            [0, 0, 2],  // a repeated vertex
            [0, 3, 2],  // a repeated vertex once welded
            [5, 1, 2],  // a vertex that is not a number
            [1, 2, 99], // a vertex past the buffer
            [0, 6, 7],  // a sliver 10 micrometres thick
            [1, 2, 6],  // kept
        ];
        let (vertices, kept) = weld_collider_triangles(&positions, &triangles, 1e-3);
        assert_eq!(vertices, [Vec3::ZERO, Vec3::Z, Vec3::X, Vec3::new(1.0, 0.0, 1.0)]);
        assert_eq!(kept, [[0u32, 1, 2], [2, 1, 3]]);

        // Nothing usable leaves nothing, rather than an empty mesh with
        // vertices.
        let (vertices, kept) = weld_collider_triangles(&positions, &[[0, 0, 2], [0, 1, 4], [5, 1, 2]], 1e-3);
        assert!(vertices.is_empty() && kept.is_empty());

        // A tolerance that is not a number welds exact copies only, so
        // vertex 3 stays apart and its triangle is kept.
        let (_, kept) = weld_collider_triangles(&positions, &triangles[..2], f32::NAN);
        assert_eq!(kept.len(), 2);

        // A thousandth of a 4 m cell near the origin; a few f32 steps of the
        // largest finite coordinate far from it.
        let config = test_config();
        let near = collider_weld_tolerance(&config, &[Vec3::new(3.0, -2.0, 1.0), Vec3::NAN]);
        assert!((near - 4.0e-3).abs() < 1e-9, "got {near}");
        let far = collider_weld_tolerance(&config, &[Vec3::new(1.0, -1.0e5, 2.0)]);
        assert!((far - 1.0e5 * WELD_F32_STEPS * f32::EPSILON).abs() < 1e-6, "got {far}");
    }

    #[test]
    fn procedural_grid_ignores_the_empty_raster() {
        // No raster means noise terrain; the grid must follow the noise, not
        // read a flat band floor out of the empty cache.
        let config = test_config();
        let data = TerrainData::procedural();
        let grid = chunk_height_grid(IVec2::new(3, -2), 8, &config, &data);
        assert_eq!(grid.len(), 81);
        assert!(grid.iter().all(|h| h.is_finite()));
        assert!(grid.iter().any(|h| *h != config.height_offset));
    }
}

// Run with: cargo test -p eustress-common --features physics
// (a plain package-scoped run compiles these out, since `physics` is not a
// default feature, and would pass even with a transposed heightfield).
#[cfg(all(test, feature = "physics"))]
mod physics_tests {
    use super::*;
    use bevy::mesh::Indices;
    use crate::terrain::{chunk_world_position, generate_chunk_mesh};
    use crate::terrain::height_query::height_at_world;

    /// Normalized raster rising linearly along one cache axis only. Linear
    /// ground is reproduced exactly by the triangulated heightfield, so any
    /// mismatch with `height_at_world` is a placement error, not sampling.
    fn sloped_data(config: &TerrainConfig, along_x: bool) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let w = data.cache_width as usize;
        let h = data.cache_height as usize;
        for z in 0..h {
            for x in 0..w {
                data.height_cache[z * w + x] = if along_x {
                    x as f32 / (w - 1) as f32
                } else {
                    z as f32 / (h - 1) as f32
                };
            }
        }
        data
    }

    /// World Y where a straight-down ray at world `(x, z)` meets the chunk's
    /// collider, placed exactly as `spawn_collider_child` places it.
    fn collider_hit_y(collider: &Collider, chunk: IVec2, config: &TerrainConfig, x: f32, z: f32) -> f32 {
        let translation = chunk_world_position(chunk, config) + chunk_collider_offset(config);
        // Close above the ground (heights span -10..30 m) so the f32 hit
        // distance keeps well under the 1e-3 tolerance.
        let origin = Vec3::new(x, 200.0, z);
        let (distance, _normal) = collider
            .cast_ray(translation, Quat::IDENTITY, origin, Vec3::NEG_Y, 1_000.0, true)
            .unwrap_or_else(|| panic!("ray at ({x}, {z}) missed the collider of chunk {chunk}"));
        origin.y - distance
    }

    fn assert_matches_surface(config: &TerrainConfig, data: &TerrainData, chunk: IVec2, points: &[(f32, f32)]) {
        let collider = build_chunk_collider(chunk, config, data).expect("valid config builds a collider");
        for &(x, z) in points {
            let expected = height_at_world(config, data, x, z);
            let hit = collider_hit_y(&collider, chunk, config, x, z);
            assert!(
                (hit - expected).abs() < 1e-3,
                "chunk {chunk} at ({x}, {z}): collider y {hit}, surface y {expected}"
            );
        }
    }

    #[test]
    fn collider_follows_a_slope_along_x() {
        let config = test_config();
        let data = sloped_data(&config, true);
        // Chunk (1, -1) spans x 64..128, z -64..0. The points sit at unequal
        // local x and z, so a transposed heightfield reads the wrong height,
        // and a half-chunk shift is off by about 4 m (or misses entirely).
        assert_matches_surface(
            &config,
            &data,
            IVec2::new(1, -1),
            &[(71.3, -52.1), (103.7, -9.4), (66.2, -30.8), (125.9, -61.5), (90.0, -3.3)],
        );
    }

    #[test]
    fn collider_follows_a_slope_along_z() {
        let config = test_config();
        let data = sloped_data(&config, false);
        // Chunk (-1, 1) spans x -64..0, z 64..128.
        assert_matches_surface(
            &config,
            &data,
            IVec2::new(-1, 1),
            &[(-57.4, 70.2), (-6.1, 119.8), (-40.3, 101.1), (-22.2, 66.6), (-60.9, 127.0)],
        );
    }

    #[test]
    fn a_volumetric_chunk_collides_on_its_carved_surface() {
        use crate::terrain::lattice_surface_height;
        use crate::terrain::volume::{apply_box, CsgOp};

        let config = test_config();
        let data = bumpy_data(&config);
        let mut volume = TerrainVolume::new();
        // A shaft through chunk (0, 0): x and z 22..42 m, floor at y = -26.
        apply_box(&config, &mut volume, Vec3::new(32.0, 7.0, 32.0), Vec3::new(10.0, 33.0, 10.0), CsgOp::Carve, None);
        let chunk = IVec2::new(0, 0);
        let collider =
            build_terrain_chunk_collider(chunk, &config, &data, &volume).expect("a volumetric chunk builds a collider");
        assert!(collider.shape().as_trimesh().is_some(), "a volumetric chunk collides on a trimesh");

        // Down the shaft the first thing hit is its floor.
        let floor = collider_hit_y(&collider, chunk, &config, 31.3, 32.7);
        assert!((floor + 26.0).abs() < 1.0, "shaft floor at {floor}");

        // Away from the shaft the trimesh passes through the lattice heights.
        let cell = config.chunk_size / config.resolution_for_lod(0) as f32;
        let (i, k) = (2, 14);
        let ground = collider_hit_y(&collider, chunk, &config, i as f32 * cell + 0.005, k as f32 * cell + 0.005);
        let expected = lattice_surface_height(&config, &data, i, k);
        assert!((ground - expected).abs() < 0.05, "ground at {ground}, lattice height {expected}");

        // Without bricks the chunk keeps its heightfield.
        let plain = build_terrain_chunk_collider(chunk, &config, &data, &TerrainVolume::new()).expect("builds");
        assert!(plain.shape().as_heightfield().is_some());
    }

    #[test]
    fn a_chunk_with_holes_collides_on_a_trimesh_of_the_quads_it_keeps() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::realism::materials::properties::MaterialProperties;
        use crate::terrain::height_query::ensure_material_cache;
        use crate::terrain::material::MATERIAL_SLOT_NONE;
        use crate::terrain::{chunk_ground_quads, chunk_height_grid, TerrainMaterialSlots, TerrainRoot};

        let config = test_config();
        let mut data = bumpy_data(&config);
        ensure_material_cache(&mut data);
        data.sparse_surface = true;
        let chunk = IVec2::new(0, 0);
        let resolution = config.resolution_for_lod(0);
        let stride = resolution + 1;
        // A hole under LOD-0 vertex (5, 7), whose cell no other vertex of the
        // chunk stands on: it takes the four quads around that vertex.
        let r = resolution as f32;
        let cell_of = |x: u32, z: u32| {
            let uv = config.chunk_point_uv(chunk, x as f32 / r, z as f32 / r);
            let (cx, cz) = data.cell_at_uv(uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0));
            cz * data.cache_width as usize + cx
        };
        let hole = cell_of(5, 7);
        assert_eq!((0..stride * stride).filter(|&i| cell_of(i % stride, i / stride) == hole).count(), 1);
        data.material_cache[hole] = [MATERIAL_SLOT_NONE; 4];

        let kept = chunk_ground_quads(chunk, resolution, &config, &data).expect("the chunk has a hole");
        let kept = kept.iter().filter(|kept| **kept).count();
        assert_eq!(kept, (resolution * resolution) as usize - 4);
        let collider = build_chunk_collider(chunk, &config, &data).expect("the chunk keeps ground");
        let trimesh = collider.shape().as_trimesh().expect("a chunk with holes collides on a trimesh");
        assert_eq!(trimesh.num_triangles(), 2 * kept, "two triangles per quad the mesh keeps");

        // Inside a kept quad the collider meets the mesh's triangle, one probe
        // per triangle; over the hole's quads a ray falls through.
        let grid = chunk_height_grid(chunk, resolution, &config, &data);
        let step = config.chunk_size / r;
        let corner = chunk_world_position(chunk, &config);
        let (cx, cz) = (11usize, 12usize);
        let h = |x: usize, z: usize| grid[z * stride as usize + x];
        let (h00, h10, h01, h11) = (h(cx, cz), h(cx + 1, cz), h(cx, cz + 1), h(cx + 1, cz + 1));
        for (fu, fv) in [(0.3f32, 0.2f32), (0.8, 0.6)] {
            // The mesh splits each quad along its (x, z + 1)-(x + 1, z) diagonal.
            let expected = if fu + fv <= 1.0 {
                h00 + fu * (h10 - h00) + fv * (h01 - h00)
            } else {
                h11 + (1.0 - fu) * (h01 - h11) + (1.0 - fv) * (h10 - h11)
            };
            let (x, z) = (corner.x + (cx as f32 + fu) * step, corner.z + (cz as f32 + fv) * step);
            let hit = collider_hit_y(&collider, chunk, &config, x, z);
            assert!((hit - expected).abs() < 1e-3, "quad ({cx}, {cz}) at ({fu}, {fv}): collider y {hit}, mesh y {expected}");
        }
        let translation = corner + chunk_collider_offset(&config);
        let over_hole = Vec3::new(corner.x + 5.5 * step, 200.0, corner.z + 7.5 * step);
        assert!(collider.cast_ray(translation, Quat::IDENTITY, over_hole, Vec3::NEG_Y, 1_000.0, true).is_none());

        // A chunk clear of the hole keeps its heightfield, and a chunk of
        // holes has no collider.
        let clear = build_chunk_collider(IVec2::new(2, 2), &config, &data).expect("builds");
        assert!(clear.shape().as_heightfield().is_some());
        let mut holes = data.clone();
        holes.material_cache.fill([MATERIAL_SLOT_NONE; 4]);
        assert!(build_chunk_collider(chunk, &config, &holes).is_none());

        // The trimesh takes the friction of the ground it keeps.
        let mut world = World::new();
        world.insert_resource(TerrainMaterialSlots::builtins());
        world.spawn((TerrainRoot, config.clone(), data));
        let entity = world.spawn((TerrainChunkCollider { chunk }, collider)).id();
        world.run_system_once(apply_terrain_chunk_friction).expect("the system runs");
        let grass = MaterialProperties::from_name("Grass").expect("the registry knows grass");
        assert_eq!(world.get::<Friction>(entity).map(|friction| friction.static_coefficient), Some(grass.friction_static));
    }

    #[test]
    fn a_trimesh_keeps_its_good_triangles_among_degenerate_ones() {
        let config = test_config();
        // A flat square at y = 5 over x and z 10..20 of chunk (0, 0), and the
        // junk a marched cell beside a hole column leaves: a vertex a
        // millimetre from a corner (the weld tolerance is 4 mm here), a vertex
        // that is not a number, and one on the square's diagonal.
        let positions = vec![
            Vec3::new(10.0, 5.0, 10.0),
            Vec3::new(20.0, 5.0, 10.0),
            Vec3::new(10.0, 5.0, 20.0),
            Vec3::new(20.0, 5.0, 20.0),
            Vec3::new(10.001, 5.0, 10.0),
            Vec3::new(f32::NAN, 5.0, 15.0),
            Vec3::new(15.0, 5.0, 15.0),
        ];
        let square: [[u32; 3]; 2] = [[0, 2, 1], [1, 2, 3]];
        // A copy of the first triangle once welded, a collinear one, repeated
        // vertices before and after the weld, a vertex that is not a number,
        // one past the buffer, and the second triangle wound the other way.
        let junk: [[u32; 3]; 7] = [[4, 2, 1], [0, 6, 3], [0, 0, 1], [5, 1, 2], [1, 2, 42], [4, 0, 2], [2, 1, 3]];
        let triangles: Vec<[u32; 3]> = square.iter().chain(junk.iter()).copied().collect();
        let collider =
            trimesh_collider_from_triangles(positions.clone(), triangles, &config).expect("the square survives the weld");
        let trimesh = collider.shape().as_trimesh().expect("a trimesh collider");
        assert_eq!(trimesh.num_triangles(), 2, "only the square's two triangles");
        let hit = collider_hit_y(&collider, IVec2::ZERO, &config, 15.3, 12.4);
        assert!((hit - 5.0).abs() < 1e-3, "the square is hit at y {hit}");

        // Nothing but degenerate or broken triangles: no collider, rather
        // than an empty trimesh.
        let broken: Vec<[u32; 3]> = vec![[0, 6, 3], [0, 0, 1], [5, 1, 2], [1, 2, 42], [4, 0, 2]];
        assert!(trimesh_collider_from_triangles(positions, broken, &config).is_none());
    }

    #[test]
    fn a_collider_takes_the_friction_of_its_chunks_dominant_material() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::realism::materials::properties::MaterialProperties;
        use crate::terrain::height_query::{ensure_material_cache, paint_material_at_world};
        use crate::terrain::{builtin_slot, TerrainMaterial, TerrainMaterialSlots, TerrainRoot};

        let config = test_config();
        let mut data = bumpy_data(&config);
        ensure_material_cache(&mut data);
        // Ice over the whole of chunk (1, 0): x 64..128, z 0..64.
        let ice_slot = TerrainMaterial::Ice.to_u8();
        let step = config.chunk_size / config.chunk_resolution as f32;
        let mut z = 0.0;
        while z <= 64.0 {
            let mut x = 64.0;
            while x <= 128.0 {
                paint_material_at_world(&config, &mut data, x, z, ice_slot, 1.0);
                x += step;
            }
            z += step;
        }
        let collider = build_chunk_collider(IVec2::new(1, 0), &config, &data).expect("valid config builds a collider");

        let mut world = World::new();
        world.insert_resource(TerrainMaterialSlots::builtins());
        world.spawn((TerrainRoot, config.clone(), data));
        let icy = world.spawn((TerrainChunkCollider { chunk: IVec2::new(1, 0) }, collider.clone())).id();
        let grassy = world.spawn((TerrainChunkCollider { chunk: IVec2::new(-2, -2) }, collider)).id();
        world.run_system_once(apply_terrain_chunk_friction).expect("the system runs");

        let ice = MaterialProperties::from_name("Ice").expect("the registry knows ice");
        assert_eq!(
            world.get::<Friction>(icy),
            Some(&Friction::new(ice.friction_static).with_dynamic_coefficient(ice.friction_kinetic))
        );
        assert_eq!(world.get::<TerrainChunkSurface>(icy), Some(&TerrainChunkSurface { dominant_slot: Some(ice_slot) }));
        let grass = MaterialProperties::from_name("Grass").expect("the registry knows grass");
        assert_eq!(world.get::<Friction>(grassy).map(|friction| friction.static_coefficient), Some(grass.friction_static));

        // Ice without a physics material: that collider falls back to Avian's
        // default, the grassy one keeps its friction.
        let mut slots = TerrainMaterialSlots::builtins();
        let mut plain_ice = builtin_slot(TerrainMaterial::Ice);
        plain_ice.physics_material = None;
        slots.set_slot(ice_slot, Some(plain_ice));
        world.insert_resource(slots);
        world.run_system_once(apply_terrain_chunk_friction).expect("the system runs");
        assert!(world.get::<Friction>(icy).is_none());
        assert!(world.get::<Friction>(grassy).is_some());
    }

    #[test]
    fn collider_vertices_match_uneven_ground() {
        let config = test_config();
        let data = bumpy_data(&config);
        let chunk = IVec2::new(0, 1);
        let resolution = config.resolution_for_lod(0);
        let step = config.chunk_size / resolution as f32;
        let corner = chunk_world_position(chunk, &config);
        let points: Vec<(f32, f32)> = [(3u32, 11u32), (13, 2), (9, 14), (5, 5)]
            .iter()
            .map(|&(x, z)| (corner.x + x as f32 * step, corner.z + z as f32 * step))
            .collect();
        assert_matches_surface(&config, &data, chunk, &points);
    }

    /// Pins the quad split: mesh.rs cuts every cell along the shared
    /// (x, z+1)-(x+1, z) diagonal, and parry's heightfield without
    /// ZIGZAG_SUBDIVISION cuts along the same one. Between grid vertices on
    /// uneven ground the two diagonals give different heights, so probes
    /// inside each cell (one per triangle) catch a change to either side
    /// that the slope and vertex checks above cannot.
    #[test]
    fn collider_triangulation_matches_lod0_mesh() {
        let config = test_config();
        let data = bumpy_data(&config);
        let chunk = IVec2::new(1, -1);
        let collider = build_chunk_collider(chunk, &config, &data).expect("valid config builds a collider");
        let resolution = config.resolution_for_lod(0);
        let step = config.chunk_size / resolution as f32;
        let stride = resolution as usize + 1;
        let corner = chunk_world_position(chunk, &config);

        let mut meshes = Assets::<Mesh>::default();
        let handle = generate_chunk_mesh(chunk, 0, &config, &data, &mut meshes);
        let mesh = meshes.get(&handle).expect("mesh was just added");
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|values| values.as_float3())
            .expect("terrain mesh positions are Float32x3");
        let indices: &[u32] = match mesh.indices() {
            Some(Indices::U32(values)) => values.as_slice(),
            _ => panic!("terrain mesh indices are U32"),
        };
        // The surface triangles come first; the skirts follow them.
        let surface = &indices[..resolution as usize * resolution as usize * 6];

        // Height of the render surface at local (lx, lz), from the one mesh
        // triangle whose XZ projection holds the point.
        let mesh_height = |lx: f32, lz: f32| -> f32 {
            let mut found: Option<f32> = None;
            let mut claims = 0;
            for tri in surface.chunks_exact(3) {
                let [a, b, c] = [positions[tri[0] as usize], positions[tri[1] as usize], positions[tri[2] as usize]];
                let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
                if det.abs() < 1e-6 {
                    continue;
                }
                let wa = ((b[2] - c[2]) * (lx - c[0]) + (c[0] - b[0]) * (lz - c[2])) / det;
                let wb = ((c[2] - a[2]) * (lx - c[0]) + (a[0] - c[0]) * (lz - c[2])) / det;
                let wc = 1.0 - wa - wb;
                if wa >= -1e-5 && wb >= -1e-5 && wc >= -1e-5 {
                    claims += 1;
                    found = Some(wa * a[1] + wb * b[1] + wc * c[1]);
                }
            }
            assert_eq!(claims, 1, "local ({lx}, {lz}) should lie in exactly one mesh triangle");
            found.expect("one triangle claimed the point")
        };

        let mut splits_differ = false;
        for (cx, cz) in [(2usize, 5usize), (11, 3), (7, 13), (14, 9)] {
            for (fu, fv) in [(0.3f32, 0.15f32), (0.8, 0.6)] {
                let lx = (cx as f32 + fu) * step;
                let lz = (cz as f32 + fv) * step;
                let expected = mesh_height(lx, lz);
                let hit = collider_hit_y(&collider, chunk, &config, corner.x + lx, corner.z + lz);
                assert!(
                    (hit - expected).abs() < 1e-3,
                    "cell ({cx}, {cz}) offset ({fu}, {fv}): collider y {hit}, mesh y {expected}"
                );

                // The same point under the other diagonal, (x, z)-(x+1, z+1).
                let h = |x: usize, z: usize| positions[z * stride + x][1];
                let (h00, h10, h01, h11) = (h(cx, cz), h(cx + 1, cz), h(cx, cz + 1), h(cx + 1, cz + 1));
                let other = if fu >= fv {
                    h00 + fu * (h10 - h00) + fv * (h11 - h10)
                } else {
                    h00 + fv * (h01 - h00) + fu * (h11 - h01)
                };
                if (other - expected).abs() > 1e-2 {
                    splits_differ = true;
                }
            }
        }
        assert!(
            splits_differ,
            "the raster is too planar for the two diagonals to disagree, so this test could not fail"
        );
    }
}
