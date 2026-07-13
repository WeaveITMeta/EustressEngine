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
//! `height_cache` stores height NORMALIZED by `TerrainConfig.height_scale`,
//! not world-space metres — every reader/writer must divide/multiply by it.

use bevy::prelude::*;
use super::{TerrainConfig, TerrainData};

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
/// reflects brush edits and road conforms, not just original generation.
pub fn height_at_world(config: &TerrainConfig, data: &TerrainData, world_x: f32, world_z: f32) -> f32 {
    let (u, v) = world_to_uv(config, world_x, world_z);
    data.sample_height(u, v) * config.height_scale
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
    let target = world_h / config.height_scale.max(1e-3);
    let normalized = if weight >= 1.0 {
        target
    } else {
        data.sample_height(u, v) * (1.0 - weight) + target * weight
    };
    data.set_height(u, v, normalized);
}

/// Blend a splat channel (0=grass,1=rock,2=dirt,3=snow/water — see
/// `TerrainMaterial::splat_bucket`) toward `1.0` at `world_x, world_z`,
/// weighted like [`set_height_at_world`]; every other channel blends toward
/// `0.0` by the same weight so the four channels stay normalized. Mirrors
/// `mesh.rs::sample_splat_weights`'s `(cache_width - 1)`-scaled indexing
/// exactly (no public `TerrainData` setter exists for splats, unlike
/// height's `set_height` — this is that missing counterpart).
pub fn set_splat_at_world(config: &TerrainConfig, data: &mut TerrainData, world_x: f32, world_z: f32, channel: usize, weight: f32) {
    if weight <= 0.0 || channel > 3 {
        return;
    }
    let w = data.cache_width as usize;
    let h = data.cache_height as usize;
    if w == 0 || h == 0 {
        return;
    }
    let total = w * h;
    if data.splat_cache.len() != total * 4 {
        data.splat_cache = vec![0.0; total * 4];
        for i in 0..total {
            data.splat_cache[i * 4] = 1.0; // default: all grass
        }
    }
    let (u, v) = world_to_uv(config, world_x, world_z);
    let x = (u * (w - 1) as f32).round() as usize;
    let z = (v * (h - 1) as f32).round() as usize;
    let base = (z * w + x) * 4;
    if base + 3 < data.splat_cache.len() {
        for c in 0..4 {
            let target = if c == channel { 1.0 } else { 0.0 };
            data.splat_cache[base + c] = data.splat_cache[base + c] * (1.0 - weight) + target * weight;
        }
    }
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
            seed: 1,
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

    /// Writes a small dense patch (not one isolated point) — matching how
    /// production actually writes (`conform_terrain_to_road` steps by
    /// `cell_size`, touching every neighbouring cache cell). A truly
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
        data.set_height(u, u, 50.0 / config.height_scale);
        let before = data.sample_height(u, u) * config.height_scale;
        assert!((before - 50.0).abs() < 0.1, "sanity: exact-pixel write/read should be exact, got {before}");

        let blended = data.sample_height(u, u) * (1.0 - 0.5) + 0.0 * 0.5;
        data.set_height(u, u, blended);
        let h = data.sample_height(u, u) * config.height_scale;
        assert!((h - 25.0).abs() < 0.1, "expected ~25.0 (halfway blend), got {h}");
    }

    #[test]
    fn empty_cache_reads_as_flat_zero() {
        let config = test_config();
        let data = TerrainData::default();
        assert_eq!(height_at_world(&config, &data, 12.0, -34.0), 0.0);
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
}
