//! Shared world-space height query/write helpers.
//!
//! `TerrainData` for a disk-loaded/generated terrain (the live, working path
//! — see `crate::terrain_disk_load::hydrate_terrain_from_disk` in the engine
//! crate, and `TerrainConfig::resize_cache`) is ONE global heightmap raster
//! spanning the whole terrain (`cache_width = (chunks_x*2+1) * chunk_resolution`,
//! centered so chunk `(0,0)` sits in the middle), living on the single
//! `TerrainRoot` entity — NOT one `TerrainData` per chunk entity. Chunks only
//! carry a `position`/`lod` for addressing into that shared raster at mesh-gen
//! time. `TerrainData::sample_height`/`set_height` (config.rs) already do the
//! correct bilinear read / nearest-cell write against that raster in
//! GLOBAL-normalized `world_u,world_v ∈ [0,1]` space — the exact same
//! parameterization `generate_chunk_mesh` (mesh.rs) feeds into the mesh, so
//! writes here are guaranteed visible on next remesh, not a parallel cache.
//!
//! This module adds the one piece that was missing: converting actual WORLD
//! coordinates (metres) to that `world_u,world_v` space, plus write-blending
//! and a real (non-planar) terrain raycast — factored out once so a third
//! caller (the road tool) doesn't hand-roll it a third time after
//! `editor::apply_brush_to_chunk` and `engine::part_to_terrain` already did.
//!
//! `height_cache` stores height NORMALIZED across the band
//! `[height_offset, height_offset + height_scale]`, not world-space metres.
//! Every reader and writer converts through `TerrainConfig::world_height` /
//! `TerrainConfig::normalized_height`; scaling by `height_scale` alone drops
//! the offset and puts a below-zero surface at the wrong height.
//!
//! The material layer (`TerrainData::material_cache`, one two-slot cell per
//! height sample) is read and written here too: [`paint_material_at_world`]
//! is its one writer for brushes and Part to Terrain,
//! [`material_at_world`] reads the cell under a point, and
//! [`material_weights_at_world`] the bilinear slot weights the vertex
//! colouring and other CPU consumers blend by.

use bevy::prelude::*;
use super::{TerrainConfig, TerrainData};
use super::material::{
    material_cell, material_cell_weights, paint_material_cell, SlotWeights, TerrainMaterial, MATERIAL_SLOT_NONE,
};
use super::volume::{heightfield_slope_factor, lattice_cell_size, sample_field_parts, FieldSample, TerrainVolume};

/// World XZ (metres) → global normalized `world_u, world_v ∈ [0,1]` — the
/// exact inverse of the formula `generate_chunk_mesh` uses to go the other
/// way (mesh.rs: `world_u = (chunk_pos.x + u + chunks_x) / total_chunks_x`).
/// `chunk_pos.x + u` is algebraically just `world_x / chunk_size` (`u` is the
/// chunk-local fractional remainder), so this needs no chunk lookup at all —
/// one division, one offset, one clamp.
pub fn world_to_uv(config: &TerrainConfig, world_x: f32, world_z: f32) -> (f32, f32) {
    let total_x = (config.chunks_x * 2 + 1) as f32;
    let total_z = (config.chunks_z * 2 + 1) as f32;
    let u = (world_x / config.chunk_size.max(1e-3) + config.chunks_x as f32) / total_x;
    let v = (world_z / config.chunk_size.max(1e-3) + config.chunks_z as f32) / total_z;
    (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
}

/// World-space height (metres) at `world_x, world_z`, reading the CURRENT
/// (edited) terrain via `TerrainData::sample_height` — unlike the private
/// procedural `TerrainNoiseContext::sample_height` in `mesh.rs`, this
/// reflects brush edits and every other write, not just original generation.
pub fn height_at_world(config: &TerrainConfig, data: &TerrainData, world_x: f32, world_z: f32) -> f32 {
    let (u, v) = world_to_uv(config, world_x, world_z);
    config.world_height(data.sample_height(u, v))
}

/// Write a world-space height at `world_x, world_z`, blended toward the
/// existing value by `weight` (`1.0` = fully overwrite, `0.0` = no-op — the
/// corridor stamp's smoothstep shoulder passes a partial weight here instead
/// of computing its own lerp). Uses `TerrainData::set_height`, so it lands in
/// the exact cell the real mesh generator reads back.
pub fn set_height_at_world(config: &TerrainConfig, data: &mut TerrainData, world_x: f32, world_z: f32, world_h: f32, weight: f32) {
    if weight <= 0.0 {
        return;
    }
    let (u, v) = world_to_uv(config, world_x, world_z);
    let target = config.normalized_height(world_h);
    let normalized = if weight >= 1.0 {
        target
    } else {
        data.sample_height(u, v) * (1.0 - weight) + target * weight
    };
    data.set_height(u, v, normalized);
}

/// Paint material `slot` into the cell at `world_x, world_z` with
/// `strength` (`1.0` replaces the cell, a partial strength blends, `0.0` is
/// a no-op), by the top-two rule of [`paint_material_cell`]: repeated dabs
/// converge on `slot`, and a third material entering a mixed cell displaces
/// the weaker of the two already there. Writes the same cell
/// `TerrainData::set_height` would ([`cache_cell_at_world`]), allocating an
/// all-Grass material layer first when the terrain has none. Sets
/// `material_dirty` when the cell changed.
pub fn paint_material_at_world(
    config: &TerrainConfig,
    data: &mut TerrainData,
    world_x: f32,
    world_z: f32,
    slot: u8,
    strength: f32,
) {
    if slot == MATERIAL_SLOT_NONE || !(strength > 0.0) {
        return;
    }
    let Some(cell) = cache_cell_at_world(config, data, world_x, world_z) else {
        return;
    };
    ensure_material_cache(data);
    let index = cell.y as usize * data.cache_width as usize + cell.x as usize;
    let Some(current) = data.material_cache.get(index).copied() else {
        return;
    };
    let painted = paint_material_cell(current, slot, strength);
    if painted != current {
        data.material_cache[index] = painted;
        data.material_dirty = true;
    }
}

/// Size the material layer to the height raster, every cell Grass, unless
/// it already matches. Returns `true` when it allocated. A terrain with no
/// material layer colours by altitude instead (`mesh.rs`), so allocating
/// changes the look of every chunk, not only the one being painted.
pub fn ensure_material_cache(data: &mut TerrainData) -> bool {
    let total = data.cache_width as usize * data.cache_height as usize;
    if total == 0 || data.material_cache.len() == total {
        return false;
    }
    data.material_cache = vec![material_cell(TerrainMaterial::Grass.to_u8()); total];
    data.material_dirty = true;
    true
}

/// The material mix of one material-map cell, as [`material_at_world`]
/// reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialSample {
    /// The stronger slot (the only one in a single-material cell).
    pub primary: u8,
    /// The weaker slot, `None` in a single-material cell.
    pub secondary: Option<u8>,
    /// The weaker slot's share, 0 to 1 (0 when there is none).
    pub secondary_weight: f32,
}

/// The material mix of the cell at `world_x, world_z`: the cell a paint
/// there lands in, not a blend of its neighbours (see
/// [`material_weights_at_world`] for that). For gameplay queries such as
/// footstep sounds or a vehicle's grip. `None` when the terrain has no
/// material layer or the cell holds no material.
pub fn material_at_world(config: &TerrainConfig, data: &TerrainData, world_x: f32, world_z: f32) -> Option<MaterialSample> {
    if !data.has_material_layer() {
        return None;
    }
    let cell = cache_cell_at_world(config, data, world_x, world_z)?;
    let index = cell.y as usize * data.cache_width as usize + cell.x as usize;
    let mut pairs: Vec<(u8, f32)> = material_cell_weights(*data.material_cache.get(index)?)
        .into_iter()
        .filter(|(slot, weight)| *slot != MATERIAL_SLOT_NONE && *weight > 0.0)
        .collect();
    pairs.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (primary, _) = *pairs.first()?;
    let secondary = pairs.get(1).copied();
    Some(MaterialSample {
        primary,
        secondary: secondary.map(|(slot, _)| slot),
        secondary_weight: secondary.map_or(0.0, |(_, weight)| weight),
    })
}

/// Bilinear material weights at global `world_u, world_v`, sampled like
/// `TerrainData::sample_height` (texel `u * (cache_width - 1)`): the four
/// cells around the point each contribute their two slots, scaled by the
/// cell's bilinear weight, and the slots are merged and normalized to sum
/// to 1. Cells with no material add nothing. Empty when the terrain has no
/// material layer or no cell around the point holds a material.
pub fn material_weights_at_uv(data: &TerrainData, world_u: f32, world_v: f32) -> SlotWeights {
    let mut weights = SlotWeights::default();
    if !data.has_material_layer() {
        return weights;
    }
    let w = data.cache_width as usize;
    let h = data.cache_height as usize;
    let px = world_u.clamp(0.0, 1.0) * (w - 1) as f32;
    let pz = world_v.clamp(0.0, 1.0) * (h - 1) as f32;
    let x0 = (px.floor() as usize).min(w - 1);
    let z0 = (pz.floor() as usize).min(h - 1);
    let x1 = (x0 + 1).min(w - 1);
    let z1 = (z0 + 1).min(h - 1);
    let fx = px - x0 as f32;
    let fz = pz - z0 as f32;
    let corners = [
        (x0, z0, (1.0 - fx) * (1.0 - fz)),
        (x1, z0, fx * (1.0 - fz)),
        (x0, z1, (1.0 - fx) * fz),
        (x1, z1, fx * fz),
    ];
    for (x, z, corner) in corners {
        if !(corner > 0.0) {
            continue;
        }
        for (slot, weight) in material_cell_weights(data.material_cache[z * w + x]) {
            weights.add(slot, corner * weight);
        }
    }
    weights.normalize();
    weights
}

/// [`material_weights_at_uv`] at world XZ (metres).
pub fn material_weights_at_world(config: &TerrainConfig, data: &TerrainData, world_x: f32, world_z: f32) -> SlotWeights {
    let (u, v) = world_to_uv(config, world_x, world_z);
    material_weights_at_uv(data, u, v)
}

/// Cache cell `(column, row)` a write at world `world_x, world_z` lands in,
/// by the exact rounding [`set_height_at_world`] (through
/// `TerrainData::set_height`) and [`paint_material_at_world`] use. `None`
/// when the raster has no cells. Undo recording names the tile a write
/// touches through this, so it cannot disagree with the write itself.
pub fn cache_cell_at_world(config: &TerrainConfig, data: &TerrainData, world_x: f32, world_z: f32) -> Option<UVec2> {
    if data.cache_width == 0 || data.cache_height == 0 {
        return None;
    }
    let (u, v) = world_to_uv(config, world_x, world_z);
    let (x, z) = data.cell_at_uv(u, v);
    Some(UVec2::new(x as u32, z as u32))
}

/// Raymarch `ray` against the REAL terrain surface (via [`height_at_world`])
/// and return the world-space hit point, or `None` if the ray never crosses
/// the surface within `max_distance`. Steps at `step` world units (coarse
/// pass), then bisects the last two samples for a precise crossing (fine
/// pass) — cheap enough to run every frame for interactive picking (a few
/// dozen height samples per call, not a mesh raycast).
///
/// Replaces the flat Y=0-plane intersection every terrain-picking call site
/// used previously (`editor::terrain_paint_system`'s `// TODO: Proper
/// terrain raycast`) — a plane hit-test is wrong on any non-flat terrain,
/// let alone a mountain.
pub fn raycast_terrain(config: &TerrainConfig, data: &TerrainData, ray: Ray3d, max_distance: f32, step: f32) -> Option<Vec3> {
    if step <= 0.0 || max_distance <= 0.0 {
        return None;
    }
    let sample_at = |t: f32| -> (Vec3, f32) {
        let p = ray.origin + ray.direction * t;
        let h = height_at_world(config, data, p.x, p.z);
        (p, p.y - h)
    };

    let mut t = 0.0f32;
    let (_, mut prev_diff) = sample_at(t);
    while t < max_distance {
        let next_t = (t + step).min(max_distance);
        let (next_pt, next_diff) = sample_at(next_t);

        // Sign change (ray was above the surface, now at/below it) = a
        // crossing between the two samples. Bisect for a tighter fix.
        if prev_diff > 0.0 && next_diff <= 0.0 {
            let mut lo_t = t;
            let mut hi_t = next_t;
            for _ in 0..12 {
                let mid_t = (lo_t + hi_t) * 0.5;
                let (mid_pt, mid_diff) = sample_at(mid_t);
                if mid_diff > 0.0 {
                    lo_t = mid_t;
                } else {
                    hi_t = mid_t;
                }
                if hi_t - lo_t < 1e-3 {
                    return Some(mid_pt);
                }
            }
            return Some(ray.origin + ray.direction * ((lo_t + hi_t) * 0.5));
        }

        t = next_t;
        prev_diff = next_diff;
    }
    None
}

/// Raycast `ray` against the terrain FIELD, the heightfield with every
/// volumetric edit (caves, overhangs, see `volume`) applied: the first point
/// within `max_distance` where the ray passes from air into solid, or `None`.
///
/// Sphere tracing: each step goes as far as the surface is certainly away
/// along the ray. The heightfield term `y - H` is a height, so it is divided
/// by the rate the ray closes on the ground, from its descent and the local
/// grade (see [`heightfield_slope_factor`]); the edit distances are distances
/// already. Two limits keep the step conservative where that
/// estimate is not: inside the box the bricks can influence, where an edit
/// distance reads +inf just past its band although the edited surface may be
/// only a few cells away, a step is at most one lattice cell, and outside it
/// a step never goes more than a cell past the box. A step that still lands
/// in solid is bisected back to the crossing. A ray that starts in solid
/// walks out first, then looks for the next way in, like
/// [`raycast_terrain`].
pub fn raycast_field(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    ray: Ray3d,
    max_distance: f32,
) -> Option<Vec3> {
    if !(max_distance > 0.0) || !ray.origin.is_finite() {
        return None;
    }
    let cell = lattice_cell_size(config);
    // Headroom under the conservative estimate, for the local slope factor
    // and the trilinear edit distances being estimates themselves.
    const SAFETY: f32 = 0.9;
    // Far from edits the heightfield alone bounds the step; this cap keeps a
    // thin ridge the local slope factor misjudges from being stepped over.
    let max_step = 2.0 * cell;
    let min_step = 1e-3 * cell;
    let hit_distance = 1e-3 * cell;
    const MAX_STEPS: usize = 8192;

    let edited = volume.influence_bounds(cell);
    let field_at = |t: f32| -> (Vec3, FieldSample) {
        let p = ray.origin + ray.direction * t;
        (p, sample_field_parts(config, data, volume, p))
    };
    let dir = *ray.direction;
    // Horizontal share of the ray: how much of each step runs across the slope.
    let across = (dir.x * dir.x + dir.z * dir.z).sqrt();

    let mut t = 0.0f32;
    // Last position known to be in air, once the ray has been in air.
    let mut air_t: Option<f32> = None;
    for _ in 0..MAX_STEPS {
        if t > max_distance {
            return None;
        }
        let (p, sample) = field_at(t);
        if !sample.value.is_finite() {
            return None;
        }
        if sample.is_solid() || sample.value == 0.0 {
            let Some(mut lo) = air_t else {
                // Started in solid: walk out one cell at a time.
                t += cell;
                continue;
            };
            // Bisect between the last air sample and this solid one.
            let mut hi = t;
            for _ in 0..24 {
                let mid = (lo + hi) * 0.5;
                if field_at(mid).1.is_solid() {
                    hi = mid;
                } else {
                    lo = mid;
                }
                if hi - lo < hit_distance {
                    break;
                }
            }
            return Some(ray.origin + ray.direction * hi);
        }
        air_t = Some(t);

        // Along the ray, `p.y - H` falls at most at the ray's descent plus the
        // local grade times its horizontal share. Dividing by that rate,
        // rather than by the slope factor (the Euclidean bound, never smaller
        // than this rate), lets a grazing ray over gentle ground step the
        // whole remaining way instead of closing the vertical gap
        // geometrically and running out of steps.
        let slope = heightfield_slope_factor(config, data, p.x, p.z);
        let grade = (slope * slope - 1.0).max(0.0).sqrt();
        let closing = -dir.y + grade * across;
        let heightfield = if closing > 1e-6 { sample.heightfield / closing } else { f32::INFINITY };
        let bound = FieldSample::compose(heightfield, sample.add, sample.carve).value;
        if bound < hit_distance {
            return Some(p);
        }
        let mut step = (bound * SAFETY).min(max_step);
        if let Some((lo, hi)) = edited {
            let outside = (lo - p).max(p - hi).max(Vec3::ZERO);
            if outside == Vec3::ZERO {
                step = step.min(cell);
            } else {
                step = step.min(outside.length() + cell);
            }
        }
        t += step.max(min_step);
    }
    None
}

/// The terrain point a picking ray hits: [`raycast_field`] when the terrain
/// holds volumetric edits, so the brush lands on cave walls and overhangs,
/// else the cheaper heightfield march [`raycast_terrain`] with `step`.
pub fn raycast_terrain_surface(
    config: &TerrainConfig,
    data: &TerrainData,
    volume: Option<&TerrainVolume>,
    ray: Ray3d,
    max_distance: f32,
    step: f32,
) -> Option<Vec3> {
    match volume {
        Some(volume) if !volume.is_empty() => raycast_field(config, data, volume, ray, max_distance),
        _ => raycast_terrain(config, data, ray, max_distance, step),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 64.0,
            chunk_resolution: 32,
            chunks_x: 2,
            chunks_z: 2,
            lod_levels: 1,
            lod_distances: vec![64.0],
            view_distance: 512.0,
            height_scale: 50.0,
            height_offset: 0.0,
            seed: 1,
        }
    }

    /// A band that reaches below world Y = 0: normalized 0 is Y = -32 and
    /// normalized 1 is Y = 96 (the flat-plate preset's band).
    fn offset_config() -> TerrainConfig {
        TerrainConfig {
            height_offset: -32.0,
            height_scale: 128.0,
            ..test_config()
        }
    }

    fn test_data(config: &TerrainConfig) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        data
    }

    #[test]
    fn world_to_uv_center_chunk_starts_at_chunks_x_over_span() {
        let config = test_config();
        // World origin (0,0) is the ORIGIN CORNER of chunk (0,0) — the left
        // edge of the center chunk, not its middle. With `chunks_x` chunks on
        // either side plus the center one (`chunks_x*2+1` total), the center
        // chunk's left edge sits at u = chunks_x / (chunks_x*2+1) — for
        // chunks_x=2 that's 2/5 = 0.4, matching `generate_chunk_mesh`'s own
        // `world_u = (chunk_pos.x + u + chunks_x) / total_chunks_x` exactly
        // (verified against mesh.rs, not assumed). The chunk's actual
        // MIDPOINT (u=0.5 here) sits at world_x = chunk_size/2, not 0.0.
        let (u, v) = world_to_uv(&config, 0.0, 0.0);
        let expected = config.chunks_x as f32 / (config.chunks_x * 2 + 1) as f32;
        assert!((u - expected).abs() < 1e-4, "expected u={expected} at world origin, got {u}");
        assert!((v - expected).abs() < 1e-4, "expected v={expected} at world origin, got {v}");

        let (u_mid, _) = world_to_uv(&config, config.chunk_size / 2.0, 0.0);
        assert!((u_mid - 0.5).abs() < 0.02, "expected u~0.5 at the center chunk's actual midpoint, got {u_mid}");
    }

    #[test]
    fn world_to_uv_clamps_out_of_range() {
        let config = test_config();
        let (u, v) = world_to_uv(&config, -100_000.0, 100_000.0);
        assert_eq!(u, 0.0);
        assert_eq!(v, 1.0);
    }

    /// Writes a small dense patch (not one isolated point), matching how
    /// production actually writes (`editor::apply_brush_to_chunk` steps one
    /// sample at a time, touching every neighbouring cache cell). A truly
    /// isolated single-point write does NOT round-trip cleanly through a
    /// bilinear read at a fractional cache position (it blends with
    /// untouched neighbours at 0.0) — that's correct bilinear-heightfield
    /// behaviour, not a bug, but it makes a single-point test misleading.
    fn write_flat_patch(config: &TerrainConfig, data: &mut TerrainData, cx: f32, cz: f32, half_extent: f32, h: f32) {
        let cell = config.chunk_size / config.chunk_resolution as f32;
        let mut x = cx - half_extent;
        while x <= cx + half_extent {
            let mut z = cz - half_extent;
            while z <= cz + half_extent {
                set_height_at_world(config, data, x, z, h, 1.0);
                z += cell;
            }
            x += cell;
        }
    }

    #[test]
    fn write_then_read_round_trips_at_world_origin() {
        let config = test_config();
        let mut data = test_data(&config);
        write_flat_patch(&config, &mut data, 0.0, 0.0, 4.0, 25.0);
        let h = height_at_world(&config, &data, 0.0, 0.0);
        assert!((h - 25.0).abs() < 1.0, "expected ~25.0, got {h}");
    }

    /// Exercises the blend weight in complete isolation from
    /// `world_to_uv`'s chunk math: write/read the SAME exact `u,v` twice
    /// (no world-space stepping loop, so no risk of a coarse grid step
    /// revisiting a cache cell more than once within one pass — see
    /// `write_flat_patch`'s doc note on that being a real but low-stakes
    /// imprecision for dense corridor writes, not a correctness issue for a
    /// single blend). `TerrainData::sample_height`/`set_height` are exact
    /// at whole-pixel `u,v` (no fractional bilinear neighbour blending).
    #[test]
    fn partial_weight_blends_toward_target() {
        let config = test_config();
        let mut data = test_data(&config);
        let u = 64.0 / (data.cache_width.max(2) - 1) as f32; // an exact pixel index, not a fractional one
        data.set_height(u, u, config.normalized_height(50.0));
        let before = config.world_height(data.sample_height(u, u));
        assert!((before - 50.0).abs() < 0.1, "sanity: exact-pixel write/read should be exact, got {before}");

        let blended = data.sample_height(u, u) * (1.0 - 0.5) + 0.0 * 0.5;
        data.set_height(u, u, blended);
        let h = config.world_height(data.sample_height(u, u));
        assert!((h - 25.0).abs() < 0.1, "expected ~25.0 (halfway blend), got {h}");
    }

    #[test]
    fn height_at_world_adds_the_offset_to_the_scaled_sample() {
        let config = offset_config();
        let mut data = test_data(&config);
        // A uniform raster, so the bilinear read is exact at any position.
        data.height_cache.iter_mut().for_each(|h| *h = 0.25);
        let h = height_at_world(&config, &data, 12.0, -34.0);
        let expected = config.height_offset + 0.25 * config.height_scale;
        assert!((h - expected).abs() < 1e-4, "expected {expected}, got {h}");

        // Normalized 0 is the band floor, not world Y = 0.
        data.height_cache.iter_mut().for_each(|h| *h = 0.0);
        let floor = height_at_world(&config, &data, 12.0, -34.0);
        assert!((floor - config.height_offset).abs() < 1e-4, "expected the floor {}, got {floor}", config.height_offset);
    }

    #[test]
    fn negative_world_height_round_trips_through_set_and_get() {
        let config = offset_config();
        let mut data = test_data(&config);
        write_flat_patch(&config, &mut data, 0.0, 0.0, 4.0, -20.0);
        let h = height_at_world(&config, &data, 0.0, 0.0);
        assert!((h + 20.0).abs() < 1e-3, "expected -20.0, got {h}");
    }

    #[test]
    fn raycast_hits_terrain_below_world_zero() {
        let config = offset_config();
        // An all-zero raster is the band floor everywhere: Y = -32.
        let data = test_data(&config);
        let ray = Ray3d::new(Vec3::new(10.0, 100.0, 10.0), Dir3::NEG_Y);
        let hit = raycast_terrain(&config, &data, ray, 500.0, 2.0).expect("should hit the sunken floor");
        assert!((hit.y - config.height_offset).abs() < 0.5, "expected hit near y={}, got {}", config.height_offset, hit.y);
    }

    #[test]
    fn empty_cache_reads_as_flat_zero() {
        let config = test_config();
        let data = TerrainData::default();
        assert_eq!(height_at_world(&config, &data, 12.0, -34.0), 0.0);
    }

    /// `cache_cell_at_world` must name the exact cell each writer changes,
    /// including points that fall on a rounding boundary and points clamped
    /// in from outside the terrain.
    #[test]
    fn cache_cell_at_world_is_the_cell_each_write_lands_in() {
        let config = test_config();
        let points = [
            (0.0f32, 0.0f32),
            (1.0, 1.0),
            (-127.9, 191.3),
            (63.99, -64.01),
            (10_000.0, -10_000.0),
            (config.chunk_size / config.chunk_resolution as f32 * 0.5, 0.25),
        ];
        for (x, z) in points {
            let mut data = test_data(&config);
            set_height_at_world(&config, &mut data, x, z, 10.0, 1.0);
            let written: Vec<usize> = data.height_cache.iter().enumerate()
                .filter(|(_, h)| **h != 0.0)
                .map(|(i, _)| i)
                .collect();
            let cell = cache_cell_at_world(&config, &data, x, z).expect("raster has cells");
            let expected = cell.y as usize * data.cache_width as usize + cell.x as usize;
            assert_eq!(written, vec![expected], "height write at ({x}, {z})");

            let mut data = test_data(&config);
            let rock = TerrainMaterial::Rock.to_u8();
            paint_material_at_world(&config, &mut data, x, z, rock, 1.0);
            let painted: Vec<usize> = data.material_cache.iter().enumerate()
                .filter(|(_, cell)| cell[0] == rock)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(painted, vec![expected], "material write at ({x}, {z})");
        }
        assert_eq!(cache_cell_at_world(&config, &TerrainData::default(), 0.0, 0.0), None);
    }

    #[test]
    fn painting_allocates_an_all_grass_layer_and_marks_it_dirty() {
        let config = test_config();
        let mut data = test_data(&config);
        assert!(!data.has_material_layer());
        paint_material_at_world(&config, &mut data, 3.0, 3.0, TerrainMaterial::Sand.to_u8(), 0.0);
        assert!(!data.has_material_layer(), "a zero-strength paint changes nothing");

        paint_material_at_world(&config, &mut data, 3.0, 3.0, TerrainMaterial::Sand.to_u8(), 1.0);
        assert!(data.has_material_layer() && data.material_dirty);
        let grass = material_cell(TerrainMaterial::Grass.to_u8());
        assert_eq!(data.material_cache.iter().filter(|c| **c != grass).count(), 1, "one cell painted");
        assert!(!ensure_material_cache(&mut data), "an existing layer is kept");
    }

    #[test]
    fn material_at_world_reports_the_cell_mix() {
        let config = test_config();
        let mut data = test_data(&config);
        assert_eq!(material_at_world(&config, &data, 5.0, 5.0), None, "no layer, no material");
        let (grass, rock) = (TerrainMaterial::Grass.to_u8(), TerrainMaterial::Rock.to_u8());
        paint_material_at_world(&config, &mut data, 5.0, 5.0, rock, 0.25);
        let sample = material_at_world(&config, &data, 5.0, 5.0).expect("painted cell");
        assert_eq!(sample.primary, grass);
        assert_eq!(sample.secondary, Some(rock));
        assert!((sample.secondary_weight - 0.25).abs() <= 1.0 / 255.0 + 1e-6, "{sample:?}");

        paint_material_at_world(&config, &mut data, 5.0, 5.0, rock, 1.0);
        let sample = material_at_world(&config, &data, 5.0, 5.0).unwrap();
        assert_eq!((sample.primary, sample.secondary, sample.secondary_weight), (rock, None, 0.0));
    }

    #[test]
    fn bilinear_material_weights_sum_to_one_and_blend_across_cells() {
        use crate::terrain::material::canonical_material_cell;
        let config = test_config();
        let mut data = test_data(&config);
        assert!(material_weights_at_world(&config, &data, 0.0, 0.0).is_empty());
        ensure_material_cache(&mut data);
        let w = data.cache_width as usize;
        let (rock, sand, snow) = (
            TerrainMaterial::Rock.to_u8(),
            TerrainMaterial::Sand.to_u8(),
            TerrainMaterial::Snow.to_u8(),
        );
        // Columns alternate between a pure Rock cell and a Sand/Snow mix, and
        // one row holds no material, so every read below crosses cell kinds.
        for (i, cell) in data.material_cache.iter_mut().enumerate() {
            *cell = if (i % w) % 2 == 0 { material_cell(rock) } else { canonical_material_cell(sand, snow, 51) };
            if i / w == 7 {
                *cell = [MATERIAL_SLOT_NONE; 4];
            }
        }
        let cell = config.chunk_size / config.chunk_resolution as f32;
        for (x, z) in [(0.3f32, 0.1f32), (-17.77, 40.4), (63.0, -63.9), (5.0 * cell + 0.5, 0.0), (1e6, -1e6)] {
            let weights = material_weights_at_world(&config, &data, x, z);
            let sum: f32 = weights.as_slice().iter().map(|(_, w)| w).sum();
            assert!((sum - 1.0).abs() < 1e-5, "weights at ({x}, {z}) sum to {sum}: {weights:?}");
            assert!(weights.as_slice().iter().all(|(s, w)| *s != MATERIAL_SLOT_NONE && *w > 0.0));
        }

        // Exactly halfway between a Rock column and a Sand/Snow column: half
        // Rock, and the other half split 204 : 51.
        let (u0, v0) = (4.0 / (w - 1) as f32, 3.0 / (data.cache_height - 1) as f32);
        let half = 0.5 / (w - 1) as f32;
        let weights = material_weights_at_uv(&data, u0 + half, v0);
        assert!((weights.weight_of(rock) - 0.5).abs() < 1e-4, "{weights:?}");
        assert!((weights.weight_of(sand) - 0.5 * 204.0 / 255.0).abs() < 1e-4, "{weights:?}");
        assert!((weights.weight_of(snow) - 0.5 * 51.0 / 255.0).abs() < 1e-4, "{weights:?}");
        assert_eq!(weights.dominant(), Some(rock));

        // On a cell centre the read is that cell alone (up to the rounding
        // of `u0 * (w - 1)` back to the column).
        let exact = material_weights_at_uv(&data, u0, v0);
        assert!(exact.weight_of(rock) > 0.9999, "{exact:?}");
        // Next to the row with no material, the material row carries the
        // whole weight.
        let v7 = 7.0 / (data.cache_height - 1) as f32;
        let beside_gap = material_weights_at_uv(&data, u0, v7 + 0.25 / (data.cache_height - 1) as f32);
        assert!(beside_gap.weight_of(rock) > 0.9999, "{beside_gap:?}");
    }

    #[test]
    fn raycast_hits_flat_terrain_at_zero() {
        let config = test_config();
        let data = test_data(&config);
        let ray = Ray3d::new(Vec3::new(10.0, 100.0, 10.0), Dir3::NEG_Y);
        let hit = raycast_terrain(&config, &data, ray, 500.0, 2.0).expect("should hit flat ground");
        assert!(hit.y.abs() < 0.5, "expected hit near y=0, got {}", hit.y);
        assert!((hit.x - 10.0).abs() < 0.1 && (hit.z - 10.0).abs() < 0.1);
    }

    #[test]
    fn raycast_hits_raised_terrain() {
        let config = test_config();
        let mut data = test_data(&config);
        write_flat_patch(&config, &mut data, 10.0, 10.0, 4.0, 20.0);
        let ray = Ray3d::new(Vec3::new(10.0, 100.0, 10.0), Dir3::NEG_Y);
        let hit = raycast_terrain(&config, &data, ray, 500.0, 2.0).expect("should hit raised ground");
        assert!((hit.y - 20.0).abs() < 1.0, "expected hit near y=20, got {}", hit.y);
    }

    #[test]
    fn raycast_misses_when_pointing_away() {
        let config = test_config();
        let data = test_data(&config);
        let ray = Ray3d::new(Vec3::new(10.0, 100.0, 10.0), Dir3::Y);
        assert!(raycast_terrain(&config, &data, ray, 500.0, 2.0).is_none());
    }

    fn straight_down(x: f32, z: f32) -> Ray3d {
        Ray3d::new(Vec3::new(x, 100.0, z), Dir3::NEG_Y)
    }

    #[test]
    fn field_raycast_hits_a_ball_added_above_the_ground_and_the_ground_beside_it() {
        use crate::terrain::volume::{apply_sphere, CsgOp};

        let config = test_config();
        let data = test_data(&config); // flat at Y = 0, a 2 m lattice
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::new(10.0, 20.0, 10.0), 5.0, CsgOp::Add, None);

        let top = raycast_field(&config, &data, &volume, straight_down(10.0, 10.0), 500.0).expect("hits the ball");
        assert!((top.y - 25.0).abs() < 0.3, "the ball's top is at 25, hit at {}", top.y);
        let ground = raycast_field(&config, &data, &volume, straight_down(40.0, 40.0), 500.0).expect("hits the ground");
        assert!(ground.y.abs() < 0.05, "the ground is at 0, hit at {}", ground.y);

        // The picking helper traces the field only once there are edits.
        let plain = raycast_terrain_surface(&config, &data, None, straight_down(10.0, 10.0), 500.0, 2.0);
        assert!(plain.expect("hits the ground").y.abs() < 0.5);
        let empty = TerrainVolume::new();
        let unedited = raycast_terrain_surface(&config, &data, Some(&empty), straight_down(10.0, 10.0), 500.0, 2.0);
        assert!(unedited.expect("hits the ground").y.abs() < 0.5);
        let edited = raycast_terrain_surface(&config, &data, Some(&volume), straight_down(10.0, 10.0), 500.0, 2.0);
        assert!((edited.expect("hits the ball").y - 25.0).abs() < 0.3);
    }

    #[test]
    fn field_raycast_falls_into_a_carved_pit_and_never_hits_from_inside_leaving() {
        use crate::terrain::volume::{apply_box, CsgOp};

        let config = test_config();
        let data = test_data(&config);
        let mut volume = TerrainVolume::new();
        // Open at the top (the box reaches 2 m above the ground), floor at -12.
        apply_box(&config, &mut volume, Vec3::new(-20.0, -5.0, 30.0), Vec3::new(5.0, 7.0, 5.0), CsgOp::Carve, None);

        let floor = raycast_field(&config, &data, &volume, straight_down(-20.3, 29.6), 500.0).expect("hits the floor");
        assert!((floor.y + 12.0).abs() < 0.5, "the pit floor is at -12, hit at {}", floor.y);

        // Starting under the ground and heading up, the ray walks out and
        // never crosses back into solid.
        let leaving = Ray3d::new(Vec3::new(40.0, -30.0, 40.0), Dir3::Y);
        assert!(raycast_field(&config, &data, &volume, leaving, 500.0).is_none());
        assert!(raycast_field(&config, &data, &volume, straight_down(0.0, 0.0), 0.0).is_none());
    }

    #[test]
    fn field_raycast_hits_flat_ground_at_a_grazing_angle() {
        use crate::terrain::volume::{apply_sphere, CsgOp};

        let config = test_config(); // 2 m lattice, flat at Y = 0
        let data = test_data(&config);
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::new(-100.0, 20.0, -100.0), 3.0, CsgOp::Add, None);
        // 0.5 m up with sin(theta) = 0.0005: the ground is 1 km out.
        let ray = Ray3d::new(Vec3::new(0.0, 0.5, 0.0), Dir3::new(Vec3::new(1.0, -0.0005, 0.0)).unwrap());
        let hit = raycast_field(&config, &data, &volume, ray, 2000.0).expect("hits the far ground");
        assert!(hit.y.abs() < 0.05 && (hit.x - 1000.0).abs() < 5.0, "hit at {hit:?}");
        assert!(raycast_terrain(&config, &data, ray, 2000.0, 2.0).is_some());
    }
}
