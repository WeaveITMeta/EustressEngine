//! # Part to Terrain (Phase 1)
//!
//! Convert selected parts' footprints into terrain at a chosen material.
//!
//! ## v1 scope
//!
//! - `PartToTerrainEvent { material, delete_sources, voxel_size }`
//! - Plugin + handler that:
//!   * Collects each selected entity's world AABB
//!   * Raises the terrain heightfield under each footprint to the AABB's
//!     top and paints the chosen material there, through the shared
//!     `terrain::height_query` writers
//!   * Marks the touched chunks in `TerrainDirtyChunks`, which remeshes
//!     them and rebuilds their colliders
//!   * Records the terrain change as one undo entry
//!   * Optionally despawns the source entities. That is a plain despawn:
//!     it is not part of the undo entry and leaves their files on disk
//!
//! The terrain is a heightfield, so a part's volume becomes a raised
//! column, not voxels: overhangs and caves are out of reach until the
//! volumetric mesher lands.

use bevy::prelude::*;
use eustress_common::terrain::height_query::{height_at_world, paint_material_at_world, set_height_at_world};
use eustress_common::terrain::{
    TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainEditRecorder, TerrainMaterial, TerrainRoot,
};
use crate::selection_box::Selected;
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// Event
// ============================================================================

#[derive(Event, Message, Debug, Clone)]
pub struct PartToTerrainEvent {
    /// Terrain material label: any of the 23 `TerrainMaterial` names
    /// ("Grass", "Rock", "Sand", "WoodPlanks", ...; case, spaces and
    /// underscores ignored), plus the aliases "green", "stone" and "brown".
    /// Unknown values fall back to "Grass".
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
    // The height raster and material map are ONE global cache on the
    // terrain root, not per chunk: chunk entities carry only a grid position.
    mut terrain: Query<(Entity, &TerrainConfig, &mut TerrainData), With<TerrainRoot>>,
    // Optional so this plugin keeps working without `EngineTerrainPlugin`.
    mut dirty: Option<ResMut<TerrainDirtyChunks>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    for event in events.read() {
        let Ok((root, config, mut data)) = terrain.single_mut() else {
            warn!("🏔 Part to Terrain: no terrain is active");
            continue;
        };
        // Procedural terrain has no raster; writing one would flatten every
        // chunk from noise to the empty cache's floor.
        if data.height_cache.is_empty() || data.cache_width == 0 || data.cache_height == 0 {
            warn!("🏔 Part to Terrain: this terrain is procedural (no height raster to raise)");
            continue;
        }

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

        let material_slot = material_label_to_slot(&event.material);
        let mut affected_footprints = 0usize;
        let mut affected_cells = 0usize;

        // Raise the ground under each AABB footprint to the AABB's top (never
        // lower it) and paint the chosen material there. Samples step at
        // half a raster cell so every cell under the footprint is written,
        // and the footprint is clipped to the terrain so a huge part cannot
        // stall the frame writing clamped edge cells over and over.
        let (terrain_min, terrain_max) = config.footprint_xz();
        let step = (config.chunk_size / config.chunk_resolution.max(1) as f32 * 0.5).max(1e-3);

        // The whole conversion is one undo entry. A local recorder keeps it
        // apart from a brush stroke the shared resource may still hold open.
        let cell = Vec2::splat(config.chunk_size / config.chunk_resolution.max(1) as f32);
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Part to Terrain", Some(root), config, &data);

        for (_, mn, mx) in &aabbs {
            let lo = Vec2::new(mn.x, mn.z).max(terrain_min);
            let hi = Vec2::new(mx.x, mx.z).min(terrain_max);
            if !(lo.is_finite() && hi.is_finite()) || lo.x > hi.x || lo.y > hi.y {
                continue;
            }
            // One cell of margin keeps float rounding on the footprint edge
            // from leaving a written cell's tile unrecorded.
            recorder.record_world_rect(config, &data, lo - cell, hi + cell);
            // Compared and stored at the height Save can keep, so the raised
            // column survives a save and reopen unchanged.
            let top = config.clamp_to_saved_band(mx.y);
            let mut x = lo.x;
            while x <= hi.x {
                let mut z = lo.y;
                while z <= hi.y {
                    if height_at_world(config, &data, x, z) < top {
                        set_height_at_world(config, &mut data, x, z, top, 1.0);
                    }
                    paint_material_at_world(config, &mut data, x, z, material_slot, 1.0);
                    affected_cells += 1;
                    z += step;
                }
                x += step;
            }
            affected_footprints += 1;
            if let Some(dirty) = dirty.as_deref_mut() {
                dirty.mark_world_rect(config, lo, hi);
            }
        }
        if let (Some(edit), Some(undo)) = (recorder.finish(Some(root), &data), undo.as_deref_mut()) {
            undo.push_labeled(
                edit.label.clone(),
                crate::undo::Action::TerrainEdit {
                    label: edit.label,
                    root: root.to_bits(),
                    tiles: edit.tiles,
                    bricks: edit.bricks,
                },
            );
        }

        info!(
            "🏔 Part to Terrain [{}]: {} entities · {:.2}m³ · {} footprints on the terrain · {} samples written",
            event.material, aabbs.len(), total_volume, affected_footprints, affected_cells
        );

        if event.delete_sources {
            for (entity, _, _) in aabbs {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Map a user-facing material name to its built-in material slot (the
/// `TerrainMaterial` discriminant): any variant name, plus a few colour
/// aliases. Unknown names paint Grass.
fn material_label_to_slot(name: &str) -> u8 {
    let material = TerrainMaterial::from_name(name).unwrap_or_else(|| {
        match name.trim().to_ascii_lowercase().as_str() {
            "green" => TerrainMaterial::Grass,
            "stone" => TerrainMaterial::Rock,
            "brown" => TerrainMaterial::Dirt,
            _ => TerrainMaterial::Grass,
        }
    });
    material.to_u8()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_labels_name_their_own_slots() {
        assert_eq!(material_label_to_slot("Sand"), TerrainMaterial::Sand.to_u8());
        assert_eq!(material_label_to_slot("water"), TerrainMaterial::Water.to_u8());
        assert_eq!(material_label_to_slot("ICE"), TerrainMaterial::Ice.to_u8());
        assert_eq!(material_label_to_slot("wood planks"), TerrainMaterial::WoodPlanks.to_u8());
        assert_eq!(material_label_to_slot("stone"), TerrainMaterial::Rock.to_u8());
        assert_eq!(material_label_to_slot("brown"), TerrainMaterial::Dirt.to_u8());
        assert_eq!(material_label_to_slot("unobtainium"), TerrainMaterial::Grass.to_u8());
    }
}
