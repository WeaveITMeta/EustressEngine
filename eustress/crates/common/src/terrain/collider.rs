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
//! [`attach_chunk_collider`] and [`refresh_chunk_collider`] choose between
//! the two for every chunk.
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
/// child's local frame (see [`chunk_collider_offset`]). `None` when the
/// config has no usable chunk size.
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
    volume_collider_from_triangles(positions, triangles, config)
}

/// The trimesh collider of a marched surface given as chunk-local positions
/// and triangles, in the collider child's local frame.
#[cfg(feature = "physics")]
fn volume_collider_from_triangles(
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    config: &TerrainConfig,
) -> Option<Collider> {
    let offset = chunk_collider_offset(config);
    let vertices: Vec<Vec3> = positions.into_iter().map(|p| p - offset).collect();
    // FIX_INTERNAL_EDGES for the same reason as the heightfield's, and a
    // lattice value of exactly zero makes coincident vertices whose sliver
    // triangles are better merged away.
    let flags = TrimeshFlags::FIX_INTERNAL_EDGES | TrimeshFlags::DELETE_DEGENERATE_TRIANGLES;
    Collider::try_trimesh_with_config(vertices, triangles, flags).ok()
}

/// The collider `chunk_pos` should have now: a trimesh of its marching-cubes
/// surface when bricks reach its columns, else its LOD-0 heightfield. A
/// volumetric chunk that cannot be marched keeps the heightfield, as its
/// render mesh does.
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
            .and_then(|(positions, triangles)| volume_collider_from_triangles(positions, triangles, config));
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
