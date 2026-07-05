//! Export a generated world into a Space's on-disk terrain model —
//! `Workspace/Terrain/_terrain.toml` + `chunks/x{cx}_z{cz}.r16` +
//! `splatmap/x{cx}_z{cz}.png` — in EXACTLY the format
//! [`crate::terrain::toml_loader`] reads back, so the existing engine
//! renderer/streamer consumes generated worlds with zero new code.
//!
//! ## Format ground truth (read from `toml_loader.rs` / `config.rs` / `mesh.rs`)
//!
//! - **Chunk addressing is SIGNED and CENTERED.** The loader derives the
//!   chunk half-extent `N = ceil(view_distance / chunk_size)`
//!   (`TerrainTomlFile::to_terrain_config`) and places chunk `(cx, cz)`,
//!   `cx, cz` in `[-N, +N]`, at cache offset `(c + N) * chunk_resolution`
//!   (`write_chunk_to_cache`). `view_distance` is therefore *addressing*,
//!   not just streaming: this exporter always writes
//!   `view_distance = N * chunk_size` exactly, and keeps `chunk_size` a
//!   power of two so `ceil((N*S)/S) == N` holds in f32 with no rounding
//!   hazard. Files outside `[-N, +N]` are silently row-dropped at load —
//!   never emitted here. (The heightmap importer in engine
//!   `ui/spawn_events.rs` uses unsigned `0..2N` coords; that contradicts
//!   the loader and must not be copied.)
//! - **The cache grid is fence-post.** The global height cache is
//!   `W = (2N+1)*R` samples per axis over `T = (2N+1)*S` metres and the
//!   mesh reads it back fence-post style (`sample_height`:
//!   `px = world_u * (W-1)`), so cache pixel `p` sits at world
//!   `-N*S + p*T/(W-1)`. The exporter maps the generated world's min
//!   corner onto `(-N*S, -N*S)` and samples the world at those exact
//!   positions (source position `g = p*T/(W-1)`, clamped to the generated
//!   extent) with bilinear resampling. Exact slicing is impossible in
//!   general — the source grid has `regions*(res-1)+1` fence-post samples
//!   while the cache tiles `R` samples per chunk *without* shared edge
//!   lines — but because every pixel is a pure global function of `p`
//!   (regardless of which chunk writes it), resampling is deterministic
//!   and seam-free by construction.
//! - **R16**: exactly `R*R` little-endian `u16`, row-major z-then-x, no
//!   header (`load_chunk_r16` rejects any other size). Stored value is
//!   NORMALIZED height: `u16 = round(clamp(h / height_scale, 0, 1) * 65535)`
//!   — written through [`save_chunk_r16`] itself for bit-parity with the
//!   loader's inverse (`raw/65535`, then world-Y `= value * height_scale`
//!   at mesh time). The toml `height_scale` is the ceiling of the
//!   generated band: `spec.sea_level + spec.height_scale`.
//! - **Splatmap**: RGBA8 PNG per chunk, `R x R`, pixel `(x, z)` with `z` =
//!   image row (same row-major order as the R16). Channels are the 4
//!   splat buckets in `splat_cache` RGBA order `[grass, rock, dirt, snow]`
//!   (`config.rs`); all 23 materials project onto them via
//!   [`TerrainMaterial::splat_bucket`]. Channel bytes always sum to
//!   exactly 255; a fixed 3x3 kernel (1-2-1 / 2-4-2 / 1-2-1) over
//!   neighbouring cache pixels softens material transitions
//!   deterministically. Requires the `image` feature (default via
//!   `geotiff`); without it the export writes R16 + toml only and reports
//!   `splatmaps_written = 0`. The palette slots are bucket-named
//!   Grass/Rock/Dirt/Snow — never the `create_default_terrain_toml`
//!   example, which wrongly puts Sand at slot 2.
//! - The 23-material identity is preserved separately in
//!   [`super::GeneratedRegion::materials`]; the splatmap is its lossy
//!   4-bucket projection for today's renderer (per material.rs Wave 9.C
//!   docs).
//!
//! ## Load trigger — INTEGRATOR NOTE
//!
//! The engine currently has NO code path that reads this format on Space
//! open: `load_terrain_toml` / `load_chunks_from_disk` /
//! `chunk_splatmap_path` have zero callers, so an exported
//! `Workspace/Terrain/` directory is inert until one of these lands:
//! (a) in-session, spawn it the way `handle_import_terrain` (engine
//! `ui/spawn_events.rs`) does — `TerrainTomlFile::to_terrain_config()`,
//! then `TerrainData::procedural()` + `resize_cache(&config)` +
//! `load_chunks_from_disk(terrain_dir, &config, &mut data)`, then
//! `spawn_terrain(...)`; or (b) an engine-side once-per-Space latch
//! mirroring `terrain_voxel_load.rs`. CAUTION: `sync_terrain_class_to_system`
//! (engine `terrain_plugin.rs`) despawns `TerrainRoot` and respawns pure
//! procedural data on `Added<Terrain>` — a disk-load hook must run after
//! (or suppress) it, or the loaded R16 terrain is clobbered.
//!
//! Determinism: same [`WorldOutput`] in, byte-identical files out — fixed
//! templates (no timestamps), fixed chunk order (`cz` outer `-N..=N`, `cx`
//! inner), pure f32/f64 arithmetic, no HashMap iteration, no RNG, no time.
//! R16 + toml are fully byte-stable; PNG byte-stability holds per `image`
//! crate version.

use std::fs;
use std::path::Path;

use super::pipeline::{WorldOutput, WorldSpec};
use crate::terrain::toml_loader::{chunk_r16_path, save_chunk_r16};

#[cfg(feature = "image")]
use crate::terrain::material::TerrainMaterial;
#[cfg(feature = "image")]
use crate::terrain::toml_loader::chunk_splatmap_path;

/// Samples per chunk side written to disk (`[terrain] chunk_resolution`).
/// 64 is the loader default and keeps every R16 at exactly 8 KiB
/// (64 * 64 * 2 bytes).
pub const EXPORT_CHUNK_RESOLUTION: u32 = 64;

/// Smallest chunk side (metres) the grid planner will emit.
const MIN_CHUNK_SIZE_M: f64 = 16.0;

/// Hard ceiling on the chunk half-extent `N` — at most `(2*32+1)^2 = 4225`
/// chunk files. The planner doubles `chunk_size` (coarsening the cache
/// pitch) until the world fits.
const MAX_HALF_EXTENT: u32 = 32;

/// What the export wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportSummary {
    pub chunks_written: usize,
    pub splatmaps_written: usize,
    /// Total bytes written across all files.
    pub bytes_written: u64,
}

/// The engine-grid geometry an export uses — a deterministic pure function
/// of the [`WorldSpec`] (exposed so callers and tests can predict the exact
/// file set before writing anything).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportGrid {
    /// Metres per chunk side. Always a power of two: the loader re-derives
    /// the half-extent as `ceil(view_distance / chunk_size)` in f32, and a
    /// power-of-two divisor makes that division exact (a fractional
    /// quotient rounding a hair above `N` would shift EVERY chunk's cache
    /// offset by one chunk).
    pub chunk_size: f32,
    /// Samples per chunk side (R16 file = `chunk_resolution^2` u16).
    pub chunk_resolution: u32,
    /// Chunk coordinates span `[-half_extent, +half_extent]` on both axes.
    pub half_extent: u32,
    /// The `[terrain] height_scale` written to toml — the ceiling of the
    /// generated height band, `spec.sea_level + spec.height_scale`.
    pub height_scale: f32,
}

impl ExportGrid {
    /// Chunks per axis (`2N + 1`).
    #[inline]
    pub fn chunks_per_axis(&self) -> u32 {
        self.half_extent * 2 + 1
    }

    /// Global cache samples per axis (`W = (2N+1) * R`).
    #[inline]
    pub fn cache_samples_per_axis(&self) -> u32 {
        self.chunks_per_axis() * self.chunk_resolution
    }

    /// Total covered extent `T` (metres) — `(2N+1) * chunk_size`.
    #[inline]
    pub fn total_extent_m(&self) -> f64 {
        self.chunks_per_axis() as f64 * self.chunk_size as f64
    }

    /// The `[streaming] view_distance` — MUST stay `half_extent * chunk_size`
    /// exactly; see [`ExportGrid::chunk_size`].
    #[inline]
    pub fn view_distance(&self) -> f32 {
        self.half_extent as f32 * self.chunk_size
    }
}

/// Plan the engine grid for a world: pick a power-of-two `chunk_size` whose
/// cache pitch approximately matches the generated source pitch (snapped
/// DOWN so we slightly oversample and preserve detail), then the smallest
/// half-extent `N >= 1` covering the world extent (`N = 0` would force
/// `view_distance = 0`, which the streamer reads as "load nothing").
/// Worlds too large for [`MAX_HALF_EXTENT`] get coarser chunks instead of
/// more of them.
pub fn plan_export_grid(spec: &WorldSpec) -> Result<ExportGrid, String> {
    if spec.regions_x == 0 || spec.regions_z == 0 {
        return Err(format!(
            "export: world has no regions ({} x {})",
            spec.regions_x, spec.regions_z
        ));
    }
    if spec.region_res < 2 {
        return Err(format!(
            "export: region_res {} < 2 (need a fence-post grid)",
            spec.region_res
        ));
    }
    if !(spec.region_size_m > 0.0) {
        return Err(format!(
            "export: region_size_m {} must be positive",
            spec.region_size_m
        ));
    }
    let ceiling = spec.sea_level + spec.height_scale;
    if !(ceiling > 0.0) {
        return Err(format!(
            "export: height band ceiling sea_level + height_scale = {ceiling} must be positive \
             (it becomes the toml height_scale that R16 values normalize against)"
        ));
    }

    // Metres between adjacent source samples (regions are fence-post grids).
    let cell = spec.region_size_m / (spec.region_res - 1) as f64;
    // The engine grid is square and origin-centered; cover the larger axis.
    let extent = spec.regions_x.max(spec.regions_z) as f64 * spec.region_size_m;

    // Chunk side that keeps cache density ~= source density (R samples per
    // chunk side => pitch ~= cell), snapped down to a power of two.
    let target = (EXPORT_CHUNK_RESOLUTION as f64 * cell)
        .min(extent)
        .max(MIN_CHUNK_SIZE_M);
    let mut size = MIN_CHUNK_SIZE_M;
    while size * 2.0 <= target {
        size *= 2.0;
    }
    let mut half = half_extent_for(extent, size);
    while half > MAX_HALF_EXTENT {
        size *= 2.0;
        half = half_extent_for(extent, size);
    }

    Ok(ExportGrid {
        chunk_size: size as f32,
        chunk_resolution: EXPORT_CHUNK_RESOLUTION,
        half_extent: half,
        height_scale: ceiling as f32,
    })
}

/// Smallest `N` with `(2N+1) * size >= extent`, floored at 1.
fn half_extent_for(extent: f64, size: f64) -> u32 {
    let q = extent / size;
    ((q - 1.0) / 2.0).ceil().max(1.0) as u32
}

/// Bucket-named material palette written alongside the terrain: slot index
/// == splat channel (`[grass, rock, dirt, snow]`). Roughness values are
/// fixed constants so the files are byte-deterministic.
const MATERIAL_PALETTE: [(&str, &str, f32); 4] = [
    ("Grass", "grass", 0.85),
    ("Rock", "rock", 0.7),
    ("Dirt", "dirt", 0.8),
    ("Snow", "snow", 0.55),
];

/// Write `world` into `<space_root>/Workspace/Terrain/` per the module-doc
/// format contract. Deterministic: same world => byte-identical files.
pub fn export_to_space(world: &WorldOutput, space_root: &Path) -> Result<ExportSummary, String> {
    let spec = &world.spec;
    let grid = plan_export_grid(spec)?;
    let sampler = WorldSampler::new(world)?;

    let terrain_dir = space_root.join("Workspace").join("Terrain");
    let chunks_dir = terrain_dir.join("chunks");
    let materials_dir = terrain_dir.join("materials");
    fs::create_dir_all(&chunks_dir)
        .map_err(|e| format!("export: failed to create {:?}: {}", chunks_dir, e))?;
    fs::create_dir_all(&materials_dir)
        .map_err(|e| format!("export: failed to create {:?}: {}", materials_dir, e))?;
    #[cfg(feature = "image")]
    let splat_dir = {
        let dir = terrain_dir.join("splatmap");
        fs::create_dir_all(&dir)
            .map_err(|e| format!("export: failed to create {:?}: {}", dir, e))?;
        dir
    };

    // Hygiene: drop stale chunk files a previous, larger export left behind
    // so the directory afterwards contains EXACTLY this export. (Stale
    // out-of-range chunks are bounds-dropped by the loader anyway, so a
    // failed removal is non-fatal.)
    clear_stale_files(&chunks_dir, "r16");
    #[cfg(feature = "image")]
    clear_stale_files(&splat_dir, "png");

    let mut summary = ExportSummary::default();

    // ── Master config + palette (fixed templates, stable field order) ──
    let toml_text = render_terrain_toml(&grid, spec);
    let toml_path = terrain_dir.join("_terrain.toml");
    fs::write(&toml_path, toml_text.as_bytes())
        .map_err(|e| format!("export: failed to write {:?}: {}", toml_path, e))?;
    summary.bytes_written += toml_text.len() as u64;

    for (name, file_stem, roughness) in MATERIAL_PALETTE {
        let text = render_material_toml(name, roughness);
        let path = materials_dir.join(format!("{file_stem}.mat.toml"));
        fs::write(&path, text.as_bytes())
            .map_err(|e| format!("export: failed to write {:?}: {}", path, e))?;
        summary.bytes_written += text.len() as u64;
    }

    // ── Per-chunk heights (+ splatmaps) ──
    // Every cache pixel samples the generated world at its exact fence-post
    // position — a pure global function of the pixel index, so the value is
    // identical no matter which chunk writes it: seam-free by construction.
    let coords = cache_pixel_coords(&grid);
    let res = grid.chunk_resolution as usize;
    let half = grid.half_extent as i64;
    let ceiling = grid.height_scale;

    let mut heights = vec![0.0f32; res * res];
    for cz in -half..=half {
        for cx in -half..=half {
            for z in 0..res {
                let gz = coords[((cz + half) as usize) * res + z];
                for x in 0..res {
                    let gx = coords[((cx + half) as usize) * res + x];
                    // save_chunk_r16 clamps to [0,1] and quantises exactly
                    // like the loader's inverse expects.
                    heights[z * res + x] = sampler.height_at(gx, gz) / ceiling;
                }
            }
            let r16_path = chunk_r16_path(&terrain_dir, cx as i32, cz as i32);
            save_chunk_r16(&r16_path, &heights, grid.chunk_resolution)?;
            summary.chunks_written += 1;
            summary.bytes_written += (res * res * 2) as u64;

            #[cfg(feature = "image")]
            {
                let png = encode_chunk_splat_png(&sampler, &coords, cx, cz, half, res)?;
                let png_path = chunk_splatmap_path(&terrain_dir, cx as i32, cz as i32);
                fs::write(&png_path, &png)
                    .map_err(|e| format!("export: failed to write {:?}: {}", png_path, e))?;
                summary.splatmaps_written += 1;
                summary.bytes_written += png.len() as u64;
            }
        }
    }

    Ok(summary)
}

/// Generated-world coordinate (metres) sampled by each global cache pixel:
/// pixel `p` of `W = (2N+1)*R` sits `p * T/(W-1)` metres from the generated
/// min corner (which the export maps onto the engine grid's `(-N*S, -N*S)`
/// corner). One array serves both axes; positions beyond the generated
/// extent are edge-clamped at sample time (no zero-cliffs).
fn cache_pixel_coords(grid: &ExportGrid) -> Vec<f64> {
    let w = grid.cache_samples_per_axis() as usize;
    let pitch = grid.total_extent_m() / (w - 1) as f64;
    (0..w).map(|p| p as f64 * pitch).collect()
}

/// Remove `*.{ext}` files from `dir` (best-effort; see call site).
fn clear_stale_files(dir: &Path, ext: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Render `_terrain.toml` — exact schema `TerrainTomlFile` parses. Fixed
/// template: stable field order, no timestamps, `{:?}` float formatting
/// (shortest round-trip, e.g. `256.0`).
fn render_terrain_toml(grid: &ExportGrid, spec: &WorldSpec) -> String {
    format!(
        r#"# Eustress Engine — Terrain Configuration
# Generated by the worldgen exporter (deterministic: same world => identical bytes).
# Heightmaps: chunks/x<cx>_z<cz>.r16 — chunk_resolution^2 little-endian u16,
#   row-major z-then-x, normalized height (world Y = value/65535 * height_scale).
# Splat weights: splatmap/x<cx>_z<cz>.png — RGBA channels = [grass, rock, dirt, snow].
# Chunk coords are SIGNED and CENTERED: cx, cz in [-N, +N] with
#   N = ceil(view_distance / chunk_size). The loader re-derives N that way,
#   so view_distance below is load-bearing — never edit it independently.

[terrain]
chunk_size = {chunk_size:?}
chunk_resolution = {chunk_resolution}
height_scale = {height_scale:?}
seed = {seed}
water_level = {water_level:?}

[streaming]
view_distance = {view_distance:?}
cull_margin = 200.0
chunks_per_frame = 4

[lod]
levels = 4
distances = [100.0, 200.0, 400.0, 800.0]

[materials]
# Slot index == splatmap channel: 0=R grass, 1=G rock, 2=B dirt, 3=A snow.

[[materials.palette]]
slot = 0
name = "Grass"
file = "materials/grass.mat.toml"

[[materials.palette]]
slot = 1
name = "Rock"
file = "materials/rock.mat.toml"

[[materials.palette]]
slot = 2
name = "Dirt"
file = "materials/dirt.mat.toml"

[[materials.palette]]
slot = 3
name = "Snow"
file = "materials/snow.mat.toml"

[water]
enabled = false
sea_level = {sea_level:?}
mode = "static"
color = [0.1, 0.3, 0.6, 0.8]
"#,
        chunk_size = grid.chunk_size,
        chunk_resolution = grid.chunk_resolution,
        height_scale = grid.height_scale,
        seed = spec.seed as u32,
        water_level = spec.sea_level as f32,
        view_distance = grid.view_distance(),
        sea_level = spec.sea_level as f32,
    )
}

/// Render one `materials/{name}.mat.toml` (schema `MaterialTomlFile`; only
/// `name` is required by the loader).
fn render_material_toml(name: &str, roughness: f32) -> String {
    format!(
        r#"# PBR Material: {name}
# Generated by the worldgen exporter.

[material]
name = "{name}"
albedo = ""
normal = ""
roughness = {roughness:?}
metallic = 0.0
ao = ""
tiling = [8.0, 8.0]
"#
    )
}

// ============================================================================
// Global world sampler
// ============================================================================

/// Global bilinear/nearest view over a [`WorldOutput`]'s stitched region
/// grid. Regions share their edge lines (fence-post) and are bit-exact on
/// shared edges after `reconcile_seams`, so a global sample index on a
/// border may take EITHER side — we deterministically take the lower-index
/// region.
struct WorldSampler<'a> {
    world: &'a WorldOutput,
    /// Sample intervals per region axis (`region_res - 1`).
    span: usize,
    /// Stitched source-grid samples per axis (`regions * span + 1`).
    src_w_x: usize,
    src_w_z: usize,
    /// Metres between adjacent source samples.
    cell: f64,
}

impl<'a> WorldSampler<'a> {
    fn new(world: &'a WorldOutput) -> Result<Self, String> {
        let spec = &world.spec;
        let expected = (spec.regions_x as usize) * (spec.regions_z as usize);
        if world.regions.len() != expected {
            return Err(format!(
                "export: world has {} regions, spec says {} ({} x {})",
                world.regions.len(),
                expected,
                spec.regions_x,
                spec.regions_z
            ));
        }
        let res = spec.region_res;
        for (i, region) in world.regions.iter().enumerate() {
            let n = (res as usize) * (res as usize);
            if region.res_x != res
                || region.res_z != res
                || region.heights.len() != n
                || region.materials.len() != n
            {
                return Err(format!(
                    "export: region {i} is {}x{} ({} heights, {} materials), spec says {res}x{res}",
                    region.res_x,
                    region.res_z,
                    region.heights.len(),
                    region.materials.len()
                ));
            }
        }
        let span = (res - 1) as usize;
        Ok(Self {
            world,
            span,
            src_w_x: spec.regions_x as usize * span + 1,
            src_w_z: spec.regions_z as usize * span + 1,
            cell: spec.region_size_m / span as f64,
        })
    }

    /// Map a stitched global sample index to (region, local sample) on one
    /// axis. Border samples resolve to the lower-index region.
    #[inline]
    fn region_and_local(&self, gi: usize, regions: u32) -> (u32, u32) {
        let r = (gi / self.span).min(regions as usize - 1);
        (r as u32, (gi - r * self.span) as u32)
    }

    /// Height at stitched global sample `(gi, gj)`.
    #[inline]
    fn grid_height(&self, gi: usize, gj: usize) -> f32 {
        let (rx, ix) = self.region_and_local(gi, self.world.spec.regions_x);
        let (rz, iz) = self.region_and_local(gj, self.world.spec.regions_z);
        self.world.region(rx, rz).height(ix, iz)
    }

    /// Material id at stitched global sample `(gi, gj)`.
    #[cfg(feature = "image")]
    #[inline]
    fn grid_material(&self, gi: usize, gj: usize) -> u8 {
        let (rx, ix) = self.region_and_local(gi, self.world.spec.regions_x);
        let (rz, iz) = self.region_and_local(gj, self.world.spec.regions_z);
        let region = self.world.region(rx, rz);
        region.materials[region.idx(ix, iz)]
    }

    /// Bilinear height at generated-world metres, edge-clamped to the
    /// generated extent per axis. Pure arithmetic — deterministic.
    fn height_at(&self, gx: f64, gz: f64) -> f32 {
        let fx = (gx / self.cell).clamp(0.0, (self.src_w_x - 1) as f64);
        let fz = (gz / self.cell).clamp(0.0, (self.src_w_z - 1) as f64);
        let i0 = fx as usize;
        let j0 = fz as usize;
        let i1 = (i0 + 1).min(self.src_w_x - 1);
        let j1 = (j0 + 1).min(self.src_w_z - 1);
        let tx = fx - i0 as f64;
        let tz = fz - j0 as f64;
        let h00 = self.grid_height(i0, j0) as f64;
        let h10 = self.grid_height(i1, j0) as f64;
        let h01 = self.grid_height(i0, j1) as f64;
        let h11 = self.grid_height(i1, j1) as f64;
        (h00 * (1.0 - tx) * (1.0 - tz)
            + h10 * tx * (1.0 - tz)
            + h01 * (1.0 - tx) * tz
            + h11 * tx * tz) as f32
    }

    /// Nearest-neighbour material id at generated-world metres (clamped).
    /// Deterministic `floor(f + 0.5)` rounding of the source index.
    #[cfg(feature = "image")]
    fn material_at(&self, gx: f64, gz: f64) -> u8 {
        let gi = (gx / self.cell + 0.5)
            .floor()
            .clamp(0.0, (self.src_w_x - 1) as f64) as usize;
        let gj = (gz / self.cell + 0.5)
            .floor()
            .clamp(0.0, (self.src_w_z - 1) as f64) as usize;
        self.grid_material(gi, gj)
    }
}

// ============================================================================
// Splatmap encoding (feature "image")
// ============================================================================

/// Encode one chunk's splatmap PNG: RGBA8, `res x res`, channels = splat
/// buckets `[grass, rock, dirt, snow]`, bytes summing to exactly 255 per
/// pixel. A fixed 3x3 kernel over neighbouring *cache* pixels (positions
/// are global, so adjacent chunks blend identically at their border)
/// softens one-hot material transitions.
#[cfg(feature = "image")]
fn encode_chunk_splat_png(
    sampler: &WorldSampler<'_>,
    coords: &[f64],
    cx: i64,
    cz: i64,
    half: i64,
    res: usize,
) -> Result<Vec<u8>, String> {
    /// 3x3 smoothing kernel; weights sum to 16.
    const KERNEL: [[u32; 3]; 3] = [[1, 2, 1], [2, 4, 2], [1, 2, 1]];
    const KERNEL_SUM: u32 = 16;

    let w = coords.len();
    let mut raw = vec![0u8; res * res * 4];
    for z in 0..res {
        let gp_z = ((cz + half) as usize) * res + z;
        for x in 0..res {
            let gp_x = ((cx + half) as usize) * res + x;

            let mut weights = [0u32; 4];
            for (dz, row) in KERNEL.iter().enumerate() {
                let qz = (gp_z as i64 + dz as i64 - 1).clamp(0, (w - 1) as i64) as usize;
                for (dx, &k) in row.iter().enumerate() {
                    let qx = (gp_x as i64 + dx as i64 - 1).clamp(0, (w - 1) as i64) as usize;
                    let id = sampler.material_at(coords[qx], coords[qz]);
                    weights[TerrainMaterial::from_u8_or_default(id).splat_bucket()] += k;
                }
            }

            // Scale 16 -> 255 with floors, then hand the rounding residue
            // to the dominant bucket (ties: lowest index) so the channels
            // sum to exactly 255. All-integer: deterministic.
            let mut bytes = [0u8; 4];
            let mut acc: u32 = 0;
            for c in 0..4 {
                let v = weights[c] * 255 / KERNEL_SUM;
                bytes[c] = v as u8;
                acc += v;
            }
            let mut win = 0usize;
            for c in 1..4 {
                if weights[c] > weights[win] {
                    win = c;
                }
            }
            bytes[win] += (255 - acc) as u8;

            let o = (z * res + x) * 4;
            raw[o..o + 4].copy_from_slice(&bytes);
        }
    }

    let mut png = Vec::new();
    {
        use image::ImageEncoder;
        let encoder = image::codecs::png::PngEncoder::new(&mut png);
        encoder
            .write_image(&raw, res as u32, res as u32, image::ExtendedColorType::Rgba8)
            .map_err(|e| format!("export: failed to encode splat PNG for chunk x{cx}_z{cz}: {e}"))?;
    }
    Ok(png)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::toml_loader;
    use crate::terrain::worldgen::pipeline::{params_for, route_archetype};
    use crate::terrain::worldgen::{generate_base, GenParams};
    use std::path::PathBuf;

    /// Deterministic hand-assembled world. Builds `GenParams` directly (not
    /// through the pipeline's in-flight expert presets) so the base field is
    /// guaranteed seamless and these tests stay decoupled from the other
    /// worldgen agents' work.
    fn test_world(
        regions_x: u32,
        regions_z: u32,
        region_size_m: f64,
        region_res: u32,
        material: u8,
    ) -> WorldOutput {
        let spec = WorldSpec {
            seed: 42,
            regions_x,
            regions_z,
            region_size_m,
            region_res,
            sea_level: 0.0,
            height_scale: 120.0,
            wind_dx: 1.0,
            wind_dz: 0.25,
        };
        let mut regions = Vec::new();
        let mut recipes = Vec::new();
        for rz in 0..regions_z {
            for rx in 0..regions_x {
                let (ox, oz) = spec.region_origin(rx, rz);
                let gen = GenParams {
                    seed: spec.seed,
                    origin_x: ox,
                    origin_z: oz,
                    size_x: spec.region_size_m,
                    size_z: spec.region_size_m,
                    res_x: spec.region_res,
                    res_z: spec.region_res,
                    height_scale: spec.height_scale,
                    sea_level: spec.sea_level,
                    ..Default::default()
                };
                let mut region = generate_base(&gen);
                region.materials.fill(material);
                regions.push(region);
                recipes.push(params_for(&spec, route_archetype(&spec, rx, rz), rx, rz));
            }
        }
        WorldOutput {
            spec,
            regions,
            recipes,
        }
    }

    /// Fresh per-test scratch directory under the OS temp dir.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eustress_worldgen_export_{}_{tag}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("stale export test dir should be removable");
        }
        dir
    }

    fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    fn dir_file_bytes(dir: &Path) -> u64 {
        let mut files = Vec::new();
        collect_files(dir, &mut files);
        files
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().len())
            .sum()
    }

    #[test]
    fn export_writes_the_exact_loader_file_set() {
        let world = test_world(3, 1, 256.0, 65, 4); // uniform Sand
        let grid = plan_export_grid(&world.spec).unwrap();
        // cell = 4 m => target = 64 * 4 = 256 (pow2 already); extent 768 =>
        // (2*1+1) * 256 covers it.
        assert_eq!(grid.chunk_resolution, 64);
        assert_eq!(grid.chunk_size, 256.0);
        assert_eq!(grid.half_extent, 1);

        let root = temp_dir("file_set");
        let summary = export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");

        assert_eq!(summary.chunks_written, 9, "3x3 chunk files for N = 1");
        assert!(terrain.join("_terrain.toml").is_file());
        for cz in -1..=1 {
            for cx in -1..=1 {
                let r16 = toml_loader::chunk_r16_path(&terrain, cx, cz);
                let meta =
                    std::fs::metadata(&r16).unwrap_or_else(|_| panic!("missing chunk {r16:?}"));
                assert_eq!(
                    meta.len(),
                    64 * 64 * 2,
                    "R16 must be exactly resolution^2 * 2 bytes or load_chunk_r16 rejects it"
                );
            }
        }
        for stem in ["grass", "rock", "dirt", "snow"] {
            assert!(
                terrain
                    .join("materials")
                    .join(format!("{stem}.mat.toml"))
                    .is_file(),
                "missing palette material {stem}"
            );
        }
        #[cfg(feature = "image")]
        {
            assert_eq!(summary.splatmaps_written, 9);
            for cz in -1..=1 {
                for cx in -1..=1 {
                    assert!(toml_loader::chunk_splatmap_path(&terrain, cx, cz).is_file());
                }
            }
        }
        #[cfg(not(feature = "image"))]
        assert_eq!(summary.splatmaps_written, 0);

        assert_eq!(
            summary.bytes_written,
            dir_file_bytes(&root),
            "bytes_written must equal the on-disk total"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn terrain_toml_round_trips_through_the_loader() {
        let world = test_world(3, 1, 256.0, 65, 0);
        let root = temp_dir("toml");
        export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");

        let parsed = toml_loader::load_terrain_toml(&terrain.join("_terrain.toml")).unwrap();
        let grid = plan_export_grid(&world.spec).unwrap();

        assert_eq!(parsed.terrain.chunk_resolution, grid.chunk_resolution);
        assert_eq!(parsed.terrain.chunk_size, grid.chunk_size);
        assert_eq!(parsed.terrain.height_scale, grid.height_scale);
        assert_eq!(parsed.terrain.seed, 42);
        assert_eq!(parsed.terrain.water_level, 0.0);
        assert_eq!(parsed.water.sea_level, 0.0);
        assert!(!parsed.water.enabled);

        // THE load-bearing invariant: the loader re-derives the chunk
        // half-extent as ceil(view_distance / chunk_size) and every chunk's
        // cache offset depends on it landing on exactly our N.
        assert_eq!(parsed.streaming.view_distance, grid.view_distance());
        let derived = (parsed.streaming.view_distance / parsed.terrain.chunk_size).ceil() as u32;
        assert_eq!(derived, grid.half_extent);

        // Palette: slot index == splat channel, bucket-named (NOT the
        // create_default_terrain_toml example, which puts Sand at slot 2).
        let slots: Vec<(u8, &str)> = parsed
            .materials
            .palette
            .iter()
            .map(|s| (s.slot, s.name.as_str()))
            .collect();
        assert_eq!(
            slots,
            vec![(0, "Grass"), (1, "Rock"), (2, "Dirt"), (3, "Snow")]
        );
        for slot in &parsed.materials.palette {
            let def = toml_loader::load_material_toml(&terrain.join(&slot.file)).unwrap();
            assert_eq!(def.name, slot.name, "palette file must parse to its slot name");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn r16_heights_round_trip_within_one_quantum() {
        let world = test_world(3, 1, 256.0, 65, 0);
        let root = temp_dir("roundtrip");
        export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");

        let grid = plan_export_grid(&world.spec).unwrap();
        let sampler = WorldSampler::new(&world).unwrap();
        let coords = cache_pixel_coords(&grid);
        let res = grid.chunk_resolution as usize;
        let half = grid.half_extent as i64;
        let quantum = grid.height_scale / 65535.0;
        // Half a quantum of quantisation error + a little f32 slack.
        let tolerance = quantum * 0.5 + grid.height_scale * 1e-6;

        for (cx, cz) in [(-1i64, -1i64), (0, 0), (1, 0), (0, 1)] {
            let path = toml_loader::chunk_r16_path(&terrain, cx as i32, cz as i32);
            let loaded = toml_loader::load_chunk_r16(&path, grid.chunk_resolution).unwrap();
            assert_eq!(loaded.len(), res * res);
            for z in 0..res {
                let gz = coords[((cz + half) as usize) * res + z];
                for x in 0..res {
                    let gx = coords[((cx + half) as usize) * res + x];
                    let expected = sampler.height_at(gx, gz).clamp(0.0, grid.height_scale);
                    let reconstructed = loaded[z * res + x] * grid.height_scale;
                    assert!(
                        (reconstructed - expected).abs() <= tolerance,
                        "chunk x{cx}_z{cz} pixel ({x},{z}): loaded {reconstructed} vs sampled {expected}"
                    );
                }
            }
        }

        // Anchor: the min-corner chunk's first pixel is the generated
        // world's origin sample — this pins the signed/centered chunk
        // naming (x-1_z-1 <=> world (0,0), NOT unsigned 0..2N coords).
        let loaded =
            toml_loader::load_chunk_r16(&toml_loader::chunk_r16_path(&terrain, -1, -1), 64)
                .unwrap();
        let origin_height = world.region(0, 0).heights[0];
        assert!(
            (loaded[0] * grid.height_scale - origin_height).abs() <= tolerance,
            "min-corner pixel must be the world origin sample: {} vs {origin_height}",
            loaded[0] * grid.height_scale
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn export_is_byte_identical_across_runs() {
        let world = test_world(2, 2, 256.0, 65, 7);
        let root_a = temp_dir("det_a");
        let root_b = temp_dir("det_b");
        let summary_a = export_to_space(&world, &root_a).unwrap();
        let summary_b = export_to_space(&world, &root_b).unwrap();
        assert_eq!(summary_a, summary_b);

        let mut files_a = Vec::new();
        let mut files_b = Vec::new();
        collect_files(&root_a, &mut files_a);
        collect_files(&root_b, &mut files_b);
        let mut rel_a: Vec<PathBuf> = files_a
            .iter()
            .map(|p| p.strip_prefix(&root_a).unwrap().to_path_buf())
            .collect();
        let mut rel_b: Vec<PathBuf> = files_b
            .iter()
            .map(|p| p.strip_prefix(&root_b).unwrap().to_path_buf())
            .collect();
        rel_a.sort();
        rel_b.sort();
        assert_eq!(rel_a, rel_b, "both exports must produce the same file set");
        for rel in &rel_a {
            let bytes_a = std::fs::read(root_a.join(rel)).unwrap();
            let bytes_b = std::fs::read(root_b.join(rel)).unwrap();
            assert_eq!(bytes_a, bytes_b, "file {rel:?} must be byte-identical");
        }
        std::fs::remove_dir_all(&root_a).ok();
        std::fs::remove_dir_all(&root_b).ok();
    }

    #[cfg(feature = "image")]
    #[test]
    fn splatmap_channels_are_bucket_weights_summing_to_255() {
        // Uniform Sand(4) world: bucket 2 (dirt) one-hot everywhere, even
        // after smoothing (all neighbours agree).
        let world = test_world(3, 1, 256.0, 65, 4);
        let root = temp_dir("splat_uniform");
        export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");
        let img = image::open(toml_loader::chunk_splatmap_path(&terrain, 0, 0))
            .unwrap()
            .to_rgba8();
        assert_eq!(img.dimensions(), (64, 64));
        for pixel in img.pixels() {
            assert_eq!(
                pixel.0,
                [0, 0, 255, 0],
                "uniform Sand must be one-hot in the dirt bucket (channel B)"
            );
        }
        std::fs::remove_dir_all(&root).ok();

        // Mixed materials (cycling all 23 ids): channels still sum to
        // exactly 255 on every pixel thanks to residue redistribution.
        let mut world = test_world(2, 1, 256.0, 65, 0);
        for region in &mut world.regions {
            for (i, m) in region.materials.iter_mut().enumerate() {
                *m = (i % 23) as u8;
            }
        }
        let root = temp_dir("splat_mixed");
        export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");
        let img = image::open(toml_loader::chunk_splatmap_path(&terrain, 0, 0))
            .unwrap()
            .to_rgba8();
        for pixel in img.pixels() {
            let sum: u32 = pixel.0.iter().map(|&b| b as u32).sum();
            assert_eq!(sum, 255, "splat weights must sum to exactly 255");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn grid_plan_covers_extent_with_power_of_two_chunks() {
        let specs = [
            WorldSpec::default(), // 3x3 regions of 1024 m
            WorldSpec {
                regions_x: 1,
                regions_z: 1,
                region_size_m: 64.0,
                region_res: 17,
                ..WorldSpec::default()
            },
            WorldSpec {
                regions_x: 5,
                regions_z: 2,
                region_size_m: 512.0,
                region_res: 129,
                ..WorldSpec::default()
            },
            // Huge world: must clamp via chunk coarsening, not file count.
            WorldSpec {
                regions_x: 32,
                regions_z: 32,
                region_size_m: 2048.0,
                region_res: 513,
                ..WorldSpec::default()
            },
        ];
        for spec in specs {
            let grid = plan_export_grid(&spec).unwrap();
            let extent = spec.regions_x.max(spec.regions_z) as f64 * spec.region_size_m;
            assert!(
                grid.total_extent_m() >= extent,
                "grid {grid:?} must cover extent {extent}"
            );
            assert!(grid.half_extent >= 1 && grid.half_extent <= MAX_HALF_EXTENT);

            // chunk_size is an exactly-representable power of two…
            let size_int = grid.chunk_size as u64;
            assert_eq!(size_int as f32, grid.chunk_size);
            assert!(size_int.is_power_of_two(), "chunk_size {}", grid.chunk_size);
            // …so the loader's f32 inversion lands exactly on our N.
            let derived = (grid.view_distance() / grid.chunk_size).ceil() as u32;
            assert_eq!(derived, grid.half_extent);
        }

        // Degenerate specs are rejected, not exported as garbage.
        assert!(plan_export_grid(&WorldSpec {
            height_scale: 0.0,
            sea_level: 0.0,
            ..WorldSpec::default()
        })
        .is_err());
        assert!(plan_export_grid(&WorldSpec {
            region_res: 1,
            ..WorldSpec::default()
        })
        .is_err());
        assert!(plan_export_grid(&WorldSpec {
            regions_x: 0,
            ..WorldSpec::default()
        })
        .is_err());
    }
}
