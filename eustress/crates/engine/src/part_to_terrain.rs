//! # Part to Terrain (Phase 1 scaffold)
//!
//! Convert selected parts' geometry to voxel terrain at a chosen
//! biome / material. The event interface + handler skeleton are
//! shipped; the actual voxel rasterization needs a deeper dive into
//! `eustress-common::terrain::{chunk, material}` APIs and lands as a
//! follow-up PR.
//!
//! ## v1 scope
//!
//! - `PartToTerrainEvent { material, delete_sources, voxel_size }`
//! - Plugin + handler that:
//!   * Collects each selected entity's world AABB
//!   * Logs what WOULD be rasterized (count, aggregate volume)
//!   * Optionally despawns the source entities (same path as the
//!     TrashEntities undo variant)
//!   * **TODO**: write voxels into the terrain grid
//!
//! Once the voxel-write lands, the public API stays identical — no
//! refactor at call-sites. This matches how the ribbon/MCP wire
//! ahead of the full implementation.

use bevy::prelude::*;
use crate::selection_box::Selected;
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// Event
// ============================================================================

#[derive(Event, Message, Debug, Clone)]
pub struct PartToTerrainEvent {
    /// Terrain material label — passed through to the voxel writer.
    /// Canonical values: "Grass", "Dirt", "Rock", "Sand", "Snow",
    /// "Water". Unknown values fall back to "Grass".
    pub material: String,
    /// If true, despawn source entities after rasterization.
    pub delete_sources: bool,
    /// Voxel size in studs (world units). Matches terrain grid
    /// resolution; default 0.5.
    pub voxel_size: f32,
}

impl Default for PartToTerrainEvent {
    fn default() -> Self {
        Self {
            material: "Grass".to_string(),
            delete_sources: false,
            voxel_size: 0.5,
        }
    }
}

/// Phase 2 — inverse direction. Given a world-space AABB, extract a
/// matching region of voxels from the terrain grid and materialize
/// them as a MeshPart with the region's mesh. Useful for "carve out
/// this hill into a standalone prop."
#[derive(Event, Message, Debug, Clone)]
pub struct TerrainToPartEvent {
    /// World-space AABB min corner of the region to extract.
    pub aabb_min: Vec3,
    /// World-space AABB max corner.
    pub aabb_max: Vec3,
    /// If true, flatten the source voxels to 0 after extraction
    /// (removes the terrain under where the part now sits).
    pub flatten_source: bool,
    /// Voxel sampling size — should match the terrain's native
    /// resolution for best fidelity. Default 0.5.
    pub voxel_size: f32,
}

impl Default for TerrainToPartEvent {
    fn default() -> Self {
        Self {
            aabb_min: Vec3::splat(-5.0),
            aabb_max: Vec3::splat( 5.0),
            flatten_source: false,
            voxel_size: 0.5,
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct PartToTerrainPlugin;

impl Plugin for PartToTerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PartToTerrainEvent>()
            .add_message::<TerrainToPartEvent>()
            .add_systems(Update, (handle_part_to_terrain, handle_terrain_to_part));
    }
}

// ============================================================================
// Handler
// ============================================================================

fn handle_part_to_terrain(
    mut events: MessageReader<PartToTerrainEvent>,
    mut commands: Commands,
    selected: Query<(
        Entity,
        &GlobalTransform,
        Option<&crate::classes::BasePart>,
    ), With<Selected>>,
    mut terrain_chunks: Query<(
        &mut eustress_common::terrain::Chunk,
        &mut eustress_common::terrain::TerrainData,
    )>,
    config: Option<Res<eustress_common::terrain::TerrainConfig>>,
) {
    for event in events.read() {
        let Some(config) = config.as_deref() else {
            warn!("🏔 Part to Terrain: no TerrainConfig resource — terrain not active");
            continue;
        };

        // Collect AABBs of the selection.
        let mut aabbs: Vec<(Entity, Vec3, Vec3)> = Vec::new();
        let mut total_volume = 0.0_f32;
        for (entity, gt, bp) in selected.iter() {
            let t = gt.compute_transform();
            let size = bp.map(|b| b.size).unwrap_or(t.scale);
            let (mn, mx) = calculate_rotated_aabb(t.translation, size * 0.5, t.rotation);
            let span = mx - mn;
            total_volume += span.x * span.y * span.z;
            aabbs.push((entity, mn, mx));
        }
        if aabbs.is_empty() {
            info!("🏔 Part to Terrain: no selection");
            continue;
        }

        let mat_layer = material_label_to_layer(&event.material);
        let mut affected_chunks = 0usize;
        let mut affected_cells = 0usize;

        // For every chunk that overlaps any AABB, raise the heightmap
        // beneath the AABB footprint to the AABB's max-Y, and paint
        // the splat-cache cell toward the chosen material.
        for (chunk, mut data) in terrain_chunks.iter_mut() {
            let chunk_world_x = chunk.position.x as f32 * config.chunk_size;
            let chunk_world_z = chunk.position.y as f32 * config.chunk_size;
            let chunk_min = Vec3::new(chunk_world_x, -1e6, chunk_world_z);
            let chunk_max = Vec3::new(
                chunk_world_x + config.chunk_size, 1e6,
                chunk_world_z + config.chunk_size,
            );

            let overlapping: Vec<&(Entity, Vec3, Vec3)> = aabbs.iter().filter(|(_, mn, mx)| {
                aabb_overlap_xz(*mn, *mx, chunk_min, chunk_max)
            }).collect();
            if overlapping.is_empty() { continue; }

            // Initialize caches if empty.
            let total_pixels = ((config.chunk_resolution + 1) * (config.chunk_resolution + 1)) as usize;
            if data.height_cache.is_empty() {
                data.height_cache = vec![0.0; total_pixels];
                data.cache_width = config.chunk_resolution + 1;
                data.cache_height = config.chunk_resolution + 1;
            }
            if data.splat_cache.len() != total_pixels * 4 {
                data.splat_cache = vec![0.0; total_pixels * 4];
                for i in 0..total_pixels {
                    data.splat_cache[i * 4] = 1.0; // default: all grass
                }
            }

            let res = config.chunk_resolution;
            for z in 0..=res {
                for x in 0..=res {
                    let u = x as f32 / res as f32;
                    let v = z as f32 / res as f32;
                    let world_x = chunk_world_x + u * config.chunk_size;
                    let world_z = chunk_world_z + v * config.chunk_size;

                    // Max-Y over every overlapping AABB that contains
                    // this XZ cell.
                    let mut new_height: Option<f32> = None;
                    for (_, mn, mx) in &overlapping {
                        if world_x >= mn.x && world_x <= mx.x
                            && world_z >= mn.z && world_z <= mx.z
                        {
                            let h = mx.y / config.height_scale.max(1e-3);
                            new_height = Some(new_height.map_or(h, |cur| cur.max(h)));
                        }
                    }
                    let Some(h) = new_height else { continue };

                    let idx = (z * (res + 1) + x) as usize;
                    data.height_cache[idx] = h;

                    // Paint splat cache — bump chosen material channel
                    // to 1.0, zero others.
                    let splat_idx = idx * 4;
                    if splat_idx + 3 < data.splat_cache.len() {
                        for c in 0..4 {
                            data.splat_cache[splat_idx + c] = if c == mat_layer { 1.0 } else { 0.0 };
                        }
                    }
                    affected_cells += 1;
                }
            }
            affected_chunks += 1;
        }

        // Mark touched chunks dirty for mesh + splatmap regen.
        // `Chunk.dirty = true` re-triggers mesh generation;
        // `TerrainData.splat_dirty = true` re-uploads the splatmap
        // texture to the GPU.
        for (mut chunk, mut data) in terrain_chunks.iter_mut() {
            chunk.dirty = true;
            data.splat_dirty = true;
        }

        info!(
            "🏔 Part to Terrain [{}]: {} entities · {:.2}m³ · {} chunks touched · {} vertices raised",
            event.material, aabbs.len(), total_volume, affected_chunks, affected_cells
        );

        if event.delete_sources {
            for (entity, _, _) in aabbs {
                commands.entity(entity).despawn();
            }
        }
    }
}

fn aabb_overlap_xz(a_mn: Vec3, a_mx: Vec3, b_mn: Vec3, b_mx: Vec3) -> bool {
    a_mx.x >= b_mn.x && a_mn.x <= b_mx.x
        && a_mx.z >= b_mn.z && a_mn.z <= b_mx.z
}

/// Map user-facing material name → splatmap channel (0..3 — the
/// terrain shader supports 4 blend layers per chunk today).
fn material_label_to_layer(name: &str) -> usize {
    match name.to_ascii_lowercase().as_str() {
        "grass" | "green"   => 0,
        "dirt"  | "brown"   => 1,
        "rock"  | "stone"   => 2,
        "sand"  | "snow"    => 3,
        _ => 0,
    }
}

/// Inverse handler — carves a voxel region into a standalone MeshPart.
/// v1 logs + scaffolds; actual voxel-to-mesh extraction lands alongside
/// the Part-to-Terrain writer since they share the terrain-chunk
/// integration surface.
fn handle_terrain_to_part(
    mut events: MessageReader<TerrainToPartEvent>,
) {
    for event in events.read() {
        let span = event.aabb_max - event.aabb_min;
        let cell_count_x = (span.x / event.voxel_size).ceil().max(1.0) as u64;
        let cell_count_y = (span.y / event.voxel_size).ceil().max(1.0) as u64;
        let cell_count_z = (span.z / event.voxel_size).ceil().max(1.0) as u64;
        let total_cells = cell_count_x * cell_count_y * cell_count_z;

        // TODO (Phase-2 follow-up): sample terrain chunks overlapping
        // the AABB, build a marching-cubes mesh from the voxel field,
        // write it as a `parts/generated/<timestamp>.glb` + spawn a
        // MeshPart. Flatten the source voxels if requested. See
        // `eustress/crates/common/src/terrain/mesh.rs` for the
        // existing heightmap→mesh path we'd mirror.
        info!(
            "🏔 Terrain to Part: dry-run, AABB {:?}..{:?} ({} cells @ {:.2}m, flatten={})",
            event.aabb_min, event.aabb_max, total_cells, event.voxel_size, event.flatten_source
        );
    }
}
