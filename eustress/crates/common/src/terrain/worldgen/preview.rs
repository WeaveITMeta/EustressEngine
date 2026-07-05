//! Visual QA rendering — hillshaded composite PNGs of a generated world.
//!
//! This is how the generator earns "AAA": every tuning round renders the
//! world to an image a human (or reviewing agent) actually LOOKS at.
//! Feature-gated behind the `image` optional dep (active by default via the
//! `geotiff` default feature).
//!
//! Renders (all deterministic):
//! - **hillshade** — Lambertian shading of the heightfield from a fixed NW
//!   sun (azimuth 315°, altitude 45°), water tinted by depth.
//! - **materials** — each cell's [`TerrainMaterial::base_color`].
//! - **composite** — `material colour × (0.35 + 0.65·hillshade)` (+ subtle
//!   height-tinted atmosphere) — the money shot used by the judge loop.
//!
//! A multi-region world is stitched into ONE image (shared edges drawn
//! once), so any seam defect is directly visible as a line at region pitch.
//!
//! Determinism contract: pure per-pixel functions of the world data — no
//! RNG, no time; two renders of the same [`WorldOutput`] are byte-identical
//! PNGs.

#![cfg(feature = "image")]

use std::path::Path;

use image::{Rgb, RgbImage};

use super::pipeline::WorldOutput;
use crate::terrain::material::TerrainMaterial;

/// Which raster to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Hillshade,
    Materials,
    Composite,
}

/// Sun azimuth for the hillshade, degrees clockwise from north (315 = NW).
const SUN_AZIMUTH_DEG: f32 = 315.0;
/// Sun altitude above the horizon, degrees.
const SUN_ALTITUDE_DEG: f32 = 45.0;

/// Depth (metres) at which water reaches its deepest tint.
const WATER_FULL_DEPTH_M: f32 = 35.0;

/// Atmospheric tint colour blended toward high altitudes (subtle fog).
const FOG_RGB: [f32; 3] = [0.74, 0.79, 0.88];
/// Maximum fog blend fraction at the world's highest point.
const FOG_MAX: f32 = 0.22;

#[inline]
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[inline]
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Material albedo as linear-ish `[0,1]` RGB. Unknown discriminants fall
/// back to the default material rather than panicking.
#[inline]
fn material_rgb(id: u8) -> [f32; 3] {
    let c = TerrainMaterial::from_u8(id)
        .unwrap_or_default()
        .base_color()
        .to_srgba();
    [c.red, c.green, c.blue]
}

/// Water colour by depth — shallow teal grading to deep navy.
#[inline]
fn water_rgb(depth_m: f32) -> [f32; 3] {
    let t = (depth_m / WATER_FULL_DEPTH_M).clamp(0.0, 1.0);
    lerp3([0.26, 0.50, 0.64], [0.02, 0.09, 0.28], t)
}

/// Lambertian hillshade `[0,1]` at pixel `(x, z)` from central differences
/// (one-sided at borders), slope in metres via `cell_size_m`, lit by the
/// fixed NW sun. `light` is the precomputed unit to-sun vector.
#[inline]
fn hillshade_at(
    heights: &[f32],
    w: usize,
    h: usize,
    x: usize,
    z: usize,
    cell_size_m: f32,
    light: (f32, f32, f32),
) -> f32 {
    let x0 = x.saturating_sub(1);
    let x1 = (x + 1).min(w - 1);
    let z0 = z.saturating_sub(1);
    let z1 = (z + 1).min(h - 1);
    let span_x = (x1 - x0).max(1) as f32 * cell_size_m;
    let span_z = (z1 - z0).max(1) as f32 * cell_size_m;
    let dhdx = (heights[z * w + x1] - heights[z * w + x0]) / span_x;
    let dhdz = (heights[z1 * w + x] - heights[z0 * w + x]) / span_z;
    // Y-up surface normal of the heightfield.
    let inv_len = 1.0 / (dhdx * dhdx + 1.0 + dhdz * dhdz).sqrt();
    let (nx, ny, nz) = (-dhdx * inv_len, inv_len, -dhdz * inv_len);
    (nx * light.0 + ny * light.1 + nz * light.2).clamp(0.0, 1.0)
}

/// Unit vector pointing at the sun. GIS convention: azimuth clockwise from
/// north; the image renders +Z downward, so north is −Z.
fn sun_vector() -> (f32, f32, f32) {
    let az = SUN_AZIMUTH_DEG.to_radians();
    let alt = SUN_ALTITUDE_DEG.to_radians();
    (az.sin() * alt.cos(), alt.sin(), -az.cos() * alt.cos())
}

/// Render the stitched world to a PNG at `path`. Image width is
/// `regions_x·(region_res−1)+1` px (shared edges collapsed), same for
/// height. Returns the (width, height) written.
pub fn render_world_png(
    world: &WorldOutput,
    kind: PreviewKind,
    path: &Path,
) -> Result<(u32, u32), String> {
    let spec = &world.spec;
    let res = spec.region_res as usize;
    let rxs = spec.regions_x as usize;
    let rzs = spec.regions_z as usize;
    if res < 2 || rxs == 0 || rzs == 0 {
        return Err(format!(
            "cannot render world: regions {rxs}x{rzs}, region_res {res}"
        ));
    }
    if world.regions.len() != rxs * rzs {
        return Err(format!(
            "world has {} regions, spec says {}",
            world.regions.len(),
            rxs * rzs
        ));
    }

    // --- Stitch regions into one raster (shared edges drawn once) ----------
    // Adjacent regions share their edge line; post-reconcile both sides carry
    // bit-identical values, so overlapping writes are idempotent and the
    // stitched raster is well-defined.
    let w = rxs * (res - 1) + 1;
    let h = rzs * (res - 1) + 1;
    let mut heights = vec![0.0f32; w * h];
    let mut mats = vec![0u8; w * h];
    for rz in 0..rzs {
        for rx in 0..rxs {
            let region = &world.regions[rz * rxs + rx];
            debug_assert_eq!(region.res_x as usize, res);
            debug_assert_eq!(region.res_z as usize, res);
            for iz in 0..res {
                let gz = rz * (res - 1) + iz;
                for ix in 0..res {
                    let gx = rx * (res - 1) + ix;
                    heights[gz * w + gx] = region.heights[iz * res + ix];
                    mats[gz * w + gx] = region.materials[iz * res + ix];
                }
            }
        }
    }

    // --- Per-pixel shading ---------------------------------------------------
    let cell = spec.cell_size_m().max(1e-6);
    let sea = spec.sea_level as f32;
    let light = sun_vector();
    let water_id = TerrainMaterial::Water.to_u8();
    let mut h_max = sea;
    for &v in &heights {
        if v > h_max {
            h_max = v;
        }
    }
    let fog_denom = (h_max - sea).max(1.0);

    let mut img = RgbImage::new(w as u32, h as u32);
    for z in 0..h {
        for x in 0..w {
            let hv = heights[z * w + x];
            let m = mats[z * w + x];
            let is_water = m == water_id || hv < sea;
            let depth = (sea - hv).max(0.0);
            let shade = hillshade_at(&heights, w, h, x, z, cell, light);

            let rgb: [f32; 3] = match kind {
                PreviewKind::Hillshade => {
                    if is_water {
                        water_rgb(depth)
                    } else {
                        [shade, shade, shade]
                    }
                }
                PreviewKind::Materials => material_rgb(m),
                PreviewKind::Composite => {
                    if is_water {
                        // Depth tint with a whisper of sun on the surface.
                        let c = water_rgb(depth);
                        let lum = 0.6 + 0.4 * shade;
                        [c[0] * lum, c[1] * lum, c[2] * lum]
                    } else {
                        let c = material_rgb(m);
                        let lum = 0.35 + 0.65 * shade;
                        let lit = [c[0] * lum, c[1] * lum, c[2] * lum];
                        // Subtle elevation fog toward high altitudes
                        // (quadratic so lowlands stay clean).
                        let t = ((hv - sea) / fog_denom).clamp(0.0, 1.0);
                        lerp3(lit, FOG_RGB, FOG_MAX * t * t)
                    }
                }
            };
            img.put_pixel(
                x as u32,
                z as u32,
                Rgb([to_u8(rgb[0]), to_u8(rgb[1]), to_u8(rgb[2])]),
            );
        }
    }

    img.save(path)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok((w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::worldgen::pipeline::{generate_world, WorldSpec};

    #[test]
    fn tiny_world_renders_all_kinds_byte_deterministically() {
        let spec = WorldSpec {
            seed: 7,
            regions_x: 2,
            regions_z: 2,
            region_size_m: 256.0,
            region_res: 33,
            height_scale: 120.0,
            ..Default::default()
        };
        let world = generate_world(&spec);
        let dir = std::env::temp_dir().join("eustress_worldgen_preview_selftest");
        std::fs::create_dir_all(&dir).expect("create temp preview dir");
        let expected = ((2 * 32 + 1) as u32, (2 * 32 + 1) as u32);
        for (kind, name) in [
            (PreviewKind::Hillshade, "tiny_hillshade.png"),
            (PreviewKind::Materials, "tiny_materials.png"),
            (PreviewKind::Composite, "tiny_composite.png"),
        ] {
            let path = dir.join(name);
            let dims = render_world_png(&world, kind, &path).expect("first render");
            assert_eq!(dims, expected, "{name}: stitched dims wrong");
            let first = std::fs::read(&path).expect("read first render");
            render_world_png(&world, kind, &path).expect("second render");
            let second = std::fs::read(&path).expect("read second render");
            assert_eq!(first, second, "{name}: render not byte-deterministic");
            assert!(!first.is_empty(), "{name}: empty PNG written");
        }
    }

    /// Env-gated visual QA artifact writer — the judge-loop entry point.
    ///
    /// Set `EUSTRESS_WORLDGEN_PREVIEW` to an output directory to write the
    /// seed-42 3×3 world's composite/hillshade/materials PNGs there;
    /// without the variable the test is a silent no-op.
    ///
    /// ```text
    /// EUSTRESS_WORLDGEN_PREVIEW=preview_out cargo test -p eustress-common \
    ///     write_preview_pngs -- --nocapture
    /// ```
    #[test]
    fn write_preview_pngs() {
        let Some(dir) = std::env::var_os("EUSTRESS_WORLDGEN_PREVIEW") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("create preview output dir");

        let spec = WorldSpec {
            seed: 42,
            regions_x: 3,
            regions_z: 3,
            region_res: 129,
            region_size_m: 1024.0,
            height_scale: 180.0,
            ..Default::default()
        };
        let world = generate_world(&spec);
        let expected = ((3 * 128 + 1) as u32, (3 * 128 + 1) as u32);
        for (kind, name) in [
            (PreviewKind::Composite, "worldgen_composite.png"),
            (PreviewKind::Hillshade, "worldgen_hillshade.png"),
            (PreviewKind::Materials, "worldgen_materials.png"),
        ] {
            let path = dir.join(name);
            let dims = render_world_png(&world, kind, &path).expect("render preview");
            assert_eq!(dims, expected);
            eprintln!("wrote {} ({}x{})", path.display(), dims.0, dims.1);
        }
    }
}
