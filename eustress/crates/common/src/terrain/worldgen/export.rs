//! Export a generated world into a Space's on-disk terrain model
//! (`Workspace/Terrain/_terrain.toml` + `chunks/x{cx}_z{cz}.r16` +
//! `matmap/x{cx}_z{cz}.png`) in EXACTLY the format
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
//!   height NORMALIZED across the band `[height_offset, height_offset +
//!   height_scale]`:
//!   `u16 = round(clamp((h - height_offset) / height_scale, 0, 1) * 65535)`,
//!   written through [`save_chunk_r16`] itself for bit-parity with the
//!   loader's inverse (`raw/65535`, then world-Y
//!   `= height_offset + value * height_scale` at mesh time, via
//!   `TerrainConfig::world_height`). A generated world writes
//!   `height_offset = min(0, lowest generated sample)` and
//!   `height_scale = spec.sea_level + spec.height_scale - height_offset`,
//!   so seabeds below Y = 0 keep their depth; a flat plate takes both from
//!   its [`FlatSpec`].
//! - **Matmap**: RGBA8 PNG per chunk, `R x R`, pixel `(x, z)` with `z` =
//!   image row (same row-major order as the R16). Each pixel is one
//!   material cell `[id_a, id_b, blend_b, 0]` (see the `material` module
//!   docs): the two heaviest of the region materials under a fixed 3x3
//!   kernel (1-2-1 / 2-4-2 / 1-2-1) over neighbouring cache pixels, ties to
//!   the lower id, so material transitions soften deterministically while
//!   every cell keeps the true ids of
//!   [`super::GeneratedRegion::materials`] (built-in slots 0..=22). Encoded
//!   by [`crate::terrain::toml_loader::encode_material_tile_png`], the
//!   encoder Save uses. Requires the `image` feature (default via
//!   `geotiff`); without it the export writes R16 + toml only, reports
//!   `matmaps_written = 0`, and the loader reads every cell as Grass.
//! - Stale `matmap/` PNGs and any legacy `splatmap/` PNGs a previous
//!   export or build left are removed first, so the loader cannot convert
//!   an old splatmap over the new ground.
//! - **Default layers**: with `WorldSpec::default_layers` on (the default)
//!   the export also writes the world's default `TerrainScatter` and
//!   `TerrainWaterBody` instances into `Layers/`, in the format Insert
//!   writes (see [`super::default_layers`]). Every export, the flat plate's
//!   included, first removes the layers an earlier export wrote; the other
//!   layers in the folder stay.
//!
//! ## Load trigger — INTEGRATOR NOTE
//!
//! The engine currently has NO code path that reads this format on Space
//! open: `load_terrain_toml` / `load_chunks_from_disk` /
//! `chunk_matmap_path` have zero callers, so an exported
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
use crate::terrain::material::{TerrainMaterial, MATERIAL_SLOT_NONE};
use crate::terrain::toml_loader::{chunk_r16_path, save_chunk_r16, LEGACY_SPLATMAP_DIR, MATMAP_DIR};

#[cfg(feature = "image")]
use crate::terrain::material::{canonical_material_cell, material_cell, MaterialCell};
#[cfg(feature = "image")]
use crate::terrain::toml_loader::{chunk_matmap_path, encode_material_tile_png};

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
    pub matmaps_written: usize,
    /// Default layer instances written into `Workspace/Terrain/Layers` (see
    /// [`super::default_layers`]); 0 when `WorldSpec::default_layers` is off
    /// and for a flat plate.
    pub layers_written: usize,
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
    /// The `[terrain] height_scale` written to toml: the range R16 values
    /// span above the `height_offset` written beside it. [`plan_export_grid`]
    /// returns the band ceiling measured from Y = 0,
    /// `spec.sea_level + spec.height_scale`, and [`export_to_space`] widens
    /// it downward by the world's floor.
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

/// World Y of the generated band's floor: the lowest region sample, capped
/// at 0 so a world with no ground below Y = 0 keeps offset 0 and exports
/// byte-identically. Bilinear resampling never undershoots its lowest
/// input, so no cache pixel can fall below this.
fn world_height_floor(world: &WorldOutput) -> f32 {
    world
        .regions
        .iter()
        .flat_map(|r| r.heights.iter().copied())
        .filter(|h| h.is_finite())
        .fold(0.0f32, f32::min)
}

/// Smallest `N` with `(2N+1) * size >= extent`, floored at 1.
fn half_extent_for(extent: f64, size: f64) -> u32 {
    let q = extent / size;
    ((q - 1.0) / 2.0).ceil().max(1.0) as u32
}

/// Material palette written alongside the terrain: the first four built-in
/// material slots (Grass 0, Rock 1, Dirt 2, Snow 3, their `TerrainMaterial`
/// discriminants). Roughness values are fixed constants so the files are
/// byte-deterministic.
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
    let mut grid = plan_export_grid(spec)?;
    let sampler = WorldSampler::new(world)?;
    let floor = world_height_floor(world);
    // Band = [floor, sea_level + height_scale]; plan_export_grid validated
    // the ceiling > 0 >= floor.
    grid.height_scale -= floor;

    let terrain_dir = space_root.join("Workspace").join("Terrain");
    let chunks_dir = terrain_dir.join("chunks");
    let materials_dir = terrain_dir.join("materials");
    fs::create_dir_all(&chunks_dir)
        .map_err(|e| format!("export: failed to create {:?}: {}", chunks_dir, e))?;
    fs::create_dir_all(&materials_dir)
        .map_err(|e| format!("export: failed to create {:?}: {}", materials_dir, e))?;
    #[cfg(feature = "image")]
    {
        let dir = terrain_dir.join(MATMAP_DIR);
        fs::create_dir_all(&dir)
            .map_err(|e| format!("export: failed to create {:?}: {}", dir, e))?;
    }

    // Hygiene: drop stale chunk files a previous, larger export left behind
    // so the directory afterwards contains EXACTLY this export. (Stale
    // out-of-range chunks are bounds-dropped by the loader anyway, so a
    // failed removal is non-fatal.) The previous terrain's volume bricks go
    // too, or its caves would be carved into the new ground on load, and so
    // do its material maps, legacy splatmaps included.
    clear_stale_files(&chunks_dir, "r16");
    clear_stale_material_maps(&terrain_dir);
    clear_stale_files(&crate::terrain::volume::volume_dir(&terrain_dir), "vbk");

    let mut summary = ExportSummary::default();

    // ── Master config + palette (fixed templates, stable field order) ──
    // The band floor is the lowest generated sample, so seabeds and shelves
    // below Y = 0 survive instead of clamping flat at 0. The per-chunk
    // heights below are measured up from the same floor.
    let toml_text = render_terrain_toml(&grid, spec.seed as u32, spec.sea_level as f32, floor);
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

    // ── Per-chunk heights (+ material maps) ──
    // Every cache pixel samples the generated world at its exact fence-post
    // position — a pure global function of the pixel index, so the value is
    // identical no matter which chunk writes it: seam-free by construction.
    let coords = cache_pixel_coords(&grid);
    let res = grid.chunk_resolution as usize;
    let half = grid.half_extent as i64;

    let mut heights = vec![0.0f32; res * res];
    for cz in -half..=half {
        for cx in -half..=half {
            for z in 0..res {
                let gz = coords[((cz + half) as usize) * res + z];
                for x in 0..res {
                    let gx = coords[((cx + half) as usize) * res + x];
                    // Measured up from the band floor. save_chunk_r16 clamps
                    // to [0,1] and quantises exactly like the loader's
                    // inverse expects.
                    heights[z * res + x] = (sampler.height_at(gx, gz) - floor) / grid.height_scale;
                }
            }
            let r16_path = chunk_r16_path(&terrain_dir, cx as i32, cz as i32);
            save_chunk_r16(&r16_path, &heights, grid.chunk_resolution)?;
            summary.chunks_written += 1;
            summary.bytes_written += (res * res * 2) as u64;

            #[cfg(feature = "image")]
            {
                let cells = chunk_material_cells(&sampler, &coords, cx, cz, half, res);
                let png = encode_material_tile_png(&cells, grid.chunk_resolution)
                    .map_err(|e| format!("export: chunk x{cx}_z{cz}: {e}"))?;
                let png_path = chunk_matmap_path(&terrain_dir, cx as i32, cz as i32);
                fs::write(&png_path, &png)
                    .map_err(|e| format!("export: failed to write {:?}: {}", png_path, e))?;
                summary.matmaps_written += 1;
                summary.bytes_written += png.len() as u64;
            }
        }
    }

    // ── Default layers ──
    // The layers a previous export wrote belong to the ground it wrote, so
    // they are replaced even when this world asks for none.
    let layers = super::default_layers::write_default_layers(world, &grid, space_root)?;
    summary.layers_written = layers.written;
    summary.bytes_written += layers.bytes_written;

    Ok(summary)
}

// ============================================================================
// Flat plate ("baseplate") export
// ============================================================================

/// A dead-flat terrain plate — the ground a builder starts on.
///
/// Written in EXACTLY the format [`export_to_space`] writes and the engine's
/// `hydrate_terrain_from_disk` reads, so the plate persists across a Space
/// reload and every brush / LOD / streaming path treats it like any other
/// terrain. Unlike the worldgen pipeline (hydrology + erosion + climate +
/// materials), nothing is simulated here: the heights are a constant, so the
/// write returns in well under a second at the sizes the ribbon offers.
///
/// ## Height band
/// All heights here are world-space metres. The R16 format stores heights
/// NORMALIZED to `[0, 1]` across the band `[height_offset, height_offset +
/// height_scale]`, so the band is the room later sculpting has: Lower can
/// dig down to `height_offset` and Raise can build up to the top of the
/// band, and anything outside it is clamped when the chunks are saved. A
/// plate at `height_m = 0.0` with `height_offset = -32.0` and
/// `height_scale = 128.0` sits at world Y = 0 with 32 m to dig and 96 m to
/// build.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatSpec {
    /// Chunk coordinates span `[-half_extent, +half_extent]` on both axes,
    /// so the plate is `(2N + 1)` chunks per side. Must be `>= 1`.
    pub half_extent: u32,
    /// Metres per chunk side. Must be a whole power of two — the loader
    /// re-derives `N` as `ceil(view_distance / chunk_size)` and a
    /// non-power-of-two divisor can shift every chunk by one
    /// (see [`ExportGrid::chunk_size`]).
    pub chunk_size: f32,
    /// Samples per chunk side (R16 file = `chunk_resolution^2` u16).
    pub chunk_resolution: u32,
    /// World Y the flat surface sits at. Must be inside the band:
    /// `height_offset <= height_m <= height_offset + height_scale`.
    pub height_m: f32,
    /// World Y of R16 value 0, the deepest later sculpting can dig.
    /// Written to `[terrain] height_offset`.
    pub height_offset: f32,
    /// Range (metres) the R16 values span above `height_offset`, so the
    /// top of the band is `height_offset + height_scale`.
    pub height_scale: f32,
    /// Material slot painted across the whole plate, written as a uniform
    /// matmap: a built-in [`TerrainMaterial`] discriminant (0 = Grass, the
    /// default, 1 = Rock, 13 = Basalt, ...) or a custom slot 23..=254. Any
    /// id but `MATERIAL_SLOT_NONE`.
    pub material_slot: u8,
    /// Recorded in `[terrain] seed`. A flat plate's geometry does not use
    /// it; it seeds the mesher's macro colour variation.
    pub seed: u32,
}

impl Default for FlatSpec {
    fn default() -> Self {
        Self {
            half_extent: 4,
            chunk_size: 64.0,
            chunk_resolution: EXPORT_CHUNK_RESOLUTION,
            height_m: 0.0,
            height_offset: 0.0,
            height_scale: 100.0,
            material_slot: TerrainMaterial::Grass as u8,
            seed: 0,
        }
    }
}

impl FlatSpec {
    /// Total covered extent (metres) — `(2N + 1) * chunk_size`.
    #[inline]
    pub fn total_extent_m(&self) -> f32 {
        (self.half_extent * 2 + 1) as f32 * self.chunk_size
    }

    /// Validate and lower to the shared [`ExportGrid`].
    pub fn grid(&self) -> Result<ExportGrid, String> {
        if self.half_extent < 1 {
            return Err(
                "flat export: half_extent must be >= 1 (0 would set view_distance = 0, which \
                 the streamer reads as load-nothing)"
                    .to_string(),
            );
        }
        if self.half_extent > MAX_HALF_EXTENT {
            return Err(format!(
                "flat export: half_extent {} exceeds the {} ceiling ({} chunk files)",
                self.half_extent,
                MAX_HALF_EXTENT,
                (MAX_HALF_EXTENT * 2 + 1).pow(2)
            ));
        }
        if !(self.chunk_size > 0.0)
            || self.chunk_size.fract() != 0.0
            || !(self.chunk_size as u32).is_power_of_two()
        {
            return Err(format!(
                "flat export: chunk_size {} must be a whole power of two",
                self.chunk_size
            ));
        }
        if self.chunk_resolution < 2 {
            return Err(format!(
                "flat export: chunk_resolution {} < 2 (need a fence-post grid)",
                self.chunk_resolution
            ));
        }
        if !(self.height_scale > 0.0) || !self.height_scale.is_finite() {
            return Err(format!(
                "flat export: height_scale {} must be positive and finite (R16 values normalize \
                 against it)",
                self.height_scale
            ));
        }
        if !self.height_offset.is_finite() {
            return Err(format!(
                "flat export: height_offset {} must be finite",
                self.height_offset
            ));
        }
        let ceiling = self.height_offset + self.height_scale;
        if !(self.height_m >= self.height_offset) || !(self.height_m <= ceiling) {
            return Err(format!(
                "flat export: height_m {} outside the band [height_offset = {}, height_offset + \
                 height_scale = {}]; the R16 format cannot represent a surface below the floor or \
                 above the ceiling",
                self.height_m, self.height_offset, ceiling
            ));
        }
        if self.material_slot == MATERIAL_SLOT_NONE {
            return Err(format!(
                "flat export: material_slot {} is the \"no material\" id; pick a material slot 0..=254",
                self.material_slot
            ));
        }
        Ok(ExportGrid {
            chunk_size: self.chunk_size,
            chunk_resolution: self.chunk_resolution,
            half_extent: self.half_extent,
            height_scale: self.height_scale,
        })
    }
}

/// Write a flat plate into `<space_root>/Workspace/Terrain/`.
///
/// Deterministic: same spec in, byte-identical files out. Clears stale
/// `.r16`/`.png` a previous, larger export left behind (legacy splatmaps
/// included), and the previous terrain's `.vbk` volume bricks, first, so
/// the directory afterwards contains EXACTLY this plate.
pub fn export_flat_to_space(spec: &FlatSpec, space_root: &Path) -> Result<ExportSummary, String> {
    let grid = spec.grid()?;

    let terrain_dir = space_root.join("Workspace").join("Terrain");
    let chunks_dir = terrain_dir.join("chunks");
    let materials_dir = terrain_dir.join("materials");
    fs::create_dir_all(&chunks_dir)
        .map_err(|e| format!("flat export: failed to create {:?}: {}", chunks_dir, e))?;
    fs::create_dir_all(&materials_dir)
        .map_err(|e| format!("flat export: failed to create {:?}: {}", materials_dir, e))?;
    #[cfg(feature = "image")]
    {
        let dir = terrain_dir.join(MATMAP_DIR);
        fs::create_dir_all(&dir)
            .map_err(|e| format!("flat export: failed to create {:?}: {}", dir, e))?;
    }

    clear_stale_files(&chunks_dir, "r16");
    clear_stale_material_maps(&terrain_dir);
    clear_stale_files(&crate::terrain::volume::volume_dir(&terrain_dir), "vbk");
    // A generated world's default layers go with its ground: its lakes would
    // otherwise flood their whole footprints on the flat plate. Layers the
    // user made stay.
    super::default_layers::clear_generated_layers(space_root);

    let mut summary = ExportSummary::default();

    // Sea level 0: the template writes `[water] enabled = false`, so a flat
    // plate never comes up with a water plane sitting over it.
    let toml_text = render_terrain_toml(&grid, spec.seed, 0.0, spec.height_offset);
    let toml_path = terrain_dir.join("_terrain.toml");
    fs::write(&toml_path, toml_text.as_bytes())
        .map_err(|e| format!("flat export: failed to write {:?}: {}", toml_path, e))?;
    summary.bytes_written += toml_text.len() as u64;

    for (name, file_stem, roughness) in MATERIAL_PALETTE {
        let text = render_material_toml(name, roughness);
        let path = materials_dir.join(format!("{file_stem}.mat.toml"));
        fs::write(&path, text.as_bytes())
            .map_err(|e| format!("flat export: failed to write {:?}: {}", path, e))?;
        summary.bytes_written += text.len() as u64;
    }

    // Every sample is the same normalized height — `save_chunk_r16` quantises
    // it exactly the way the loader's `raw / 65535.0` inverse expects. It is
    // measured up from the band floor the toml records as `height_offset`.
    let res = grid.chunk_resolution as usize;
    let half = grid.half_extent as i64;
    let normalized = (spec.height_m - spec.height_offset) / grid.height_scale;
    let heights = vec![normalized; res * res];

    // One uniform matmap serves every chunk: the whole plate is a single
    // material slot.
    #[cfg(feature = "image")]
    let png = encode_material_tile_png(&vec![material_cell(spec.material_slot); res * res], grid.chunk_resolution)
        .map_err(|e| format!("flat export: {e}"))?;

    for cz in -half..=half {
        for cx in -half..=half {
            let r16_path = chunk_r16_path(&terrain_dir, cx as i32, cz as i32);
            save_chunk_r16(&r16_path, &heights, grid.chunk_resolution)?;
            summary.chunks_written += 1;
            summary.bytes_written += (res * res * 2) as u64;

            #[cfg(feature = "image")]
            {
                let png_path = chunk_matmap_path(&terrain_dir, cx as i32, cz as i32);
                fs::write(&png_path, &png)
                    .map_err(|e| format!("flat export: failed to write {:?}: {}", png_path, e))?;
                summary.matmaps_written += 1;
                summary.bytes_written += png.len() as u64;
            }
        }
    }

    Ok(summary)
}

/// Remove the material maps a previous terrain left: every `matmap/*.png`,
/// and every legacy `splatmap/*.png` with the directory itself, which no
/// exporter writes and the loader would otherwise convert for any chunk
/// whose matmap is missing. Best effort, like [`clear_stale_files`].
fn clear_stale_material_maps(terrain_dir: &Path) {
    clear_stale_files(&terrain_dir.join(MATMAP_DIR), "png");
    let legacy = terrain_dir.join(LEGACY_SPLATMAP_DIR);
    clear_stale_files(&legacy, "png");
    let _ = fs::remove_dir(&legacy);
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
///
/// Takes the scalars it actually writes rather than a whole [`WorldSpec`],
/// so the flat-plate exporter ([`export_flat_to_space`]) shares this one
/// template instead of keeping a second copy that could drift out of sync
/// with what the loader parses. `height_offset` is the world Y of R16
/// value 0 and must match the band the caller normalized its heights to.
fn render_terrain_toml(grid: &ExportGrid, seed: u32, sea_level: f32, height_offset: f32) -> String {
    format!(
        r#"# Eustress Engine — Terrain Configuration
# Generated by the worldgen exporter (deterministic: same world => identical bytes).
# Heightmaps: chunks/x<cx>_z<cz>.r16 — chunk_resolution^2 little-endian u16,
#   row-major z-then-x, normalized height
#   (world Y = height_offset + value/65535 * height_scale).
# Materials: matmap/x<cx>_z<cz>.png, RGBA8 per cell = [slot a, slot b, weight of b / 255, 0].
# Chunk coords are SIGNED and CENTERED: cx, cz in [-N, +N] with
#   N = ceil(view_distance / chunk_size). The loader re-derives N that way,
#   so view_distance below is load-bearing — never edit it independently.

[terrain]
chunk_size = {chunk_size:?}
chunk_resolution = {chunk_resolution}
height_scale = {height_scale:?}
height_offset = {height_offset:?}
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
# Slot = the material slot id matmap cells store: 0-22 built-in, 23-254 custom.

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
        height_offset = height_offset,
        seed = seed,
        water_level = sea_level,
        view_distance = grid.view_distance(),
        sea_level = sea_level,
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

/// A generated world stitched into one fence-post grid of `width x depth`
/// samples `cell` metres apart, row-major (`j * width + i`): sample `(i, j)`
/// sits at generated-world metres `(i * cell, j * cell)`, which the export
/// places at engine `(i * cell - N * S, j * cell - N * S)`. Shared edge lines
/// take the lower-index region's sample, like every read [`WorldSampler`]
/// makes.
pub(super) struct StitchedWorld {
    pub(super) width: usize,
    pub(super) depth: usize,
    pub(super) cell: f64,
    /// Height in metres per sample.
    pub(super) heights: Vec<f32>,
    /// `TerrainMaterial` discriminant per sample.
    pub(super) materials: Vec<u8>,
}

/// Stitch `world`'s regions into one grid (see [`StitchedWorld`]).
pub(super) fn stitch_world(world: &WorldOutput) -> Result<StitchedWorld, String> {
    let sampler = WorldSampler::new(world)?;
    let (width, depth) = (sampler.src_w_x, sampler.src_w_z);
    let mut heights = Vec::with_capacity(width * depth);
    let mut materials = Vec::with_capacity(width * depth);
    for gj in 0..depth {
        for gi in 0..width {
            heights.push(sampler.grid_height(gi, gj));
            materials.push(sampler.grid_material(gi, gj));
        }
    }
    Ok(StitchedWorld { width, depth, cell: sampler.cell, heights, materials })
}

// ============================================================================
// Material-map cells (feature "image")
// ============================================================================

/// 3x3 smoothing kernel over neighbouring cache pixels; weights sum to 16.
#[cfg(feature = "image")]
const MATERIAL_KERNEL: [[u32; 3]; 3] = [[1, 2, 1], [2, 4, 2], [1, 2, 1]];

/// One chunk's material cells, `res x res` row-major: each cache pixel's
/// two heaviest region materials under [`MATERIAL_KERNEL`] (positions are
/// global, so adjacent chunks blend identically at their border), via
/// [`kernel_material_cell`]. The ids are the region's own
/// `TerrainMaterial` discriminants, so no material collapses onto another.
#[cfg(feature = "image")]
fn chunk_material_cells(
    sampler: &WorldSampler<'_>,
    coords: &[f64],
    cx: i64,
    cz: i64,
    half: i64,
    res: usize,
) -> Vec<MaterialCell> {
    let w = coords.len();
    let mut cells = Vec::with_capacity(res * res);
    for z in 0..res {
        let gp_z = ((cz + half) as usize) * res + z;
        for x in 0..res {
            let gp_x = ((cx + half) as usize) * res + x;

            // At most nine distinct materials sit under the kernel.
            let mut weights = [(MATERIAL_SLOT_NONE, 0u32); 9];
            let mut len = 0;
            for (dz, row) in MATERIAL_KERNEL.iter().enumerate() {
                let qz = (gp_z as i64 + dz as i64 - 1).clamp(0, (w - 1) as i64) as usize;
                for (dx, &k) in row.iter().enumerate() {
                    let qx = (gp_x as i64 + dx as i64 - 1).clamp(0, (w - 1) as i64) as usize;
                    let slot = TerrainMaterial::from_u8_or_default(sampler.material_at(coords[qx], coords[qz])).to_u8();
                    match weights[..len].iter_mut().find(|(s, _)| *s == slot) {
                        Some(entry) => entry.1 += k,
                        None => {
                            weights[len] = (slot, k);
                            len += 1;
                        }
                    }
                }
            }
            cells.push(kernel_material_cell(&mut weights[..len]));
        }
    }
    cells
}

/// The material cell for kernel weights `(slot, weight)`: the heaviest two
/// (ties to the lower slot), `id_b` weighted by its share of the pair,
/// floored so the blend stays at or under 127 and the heavier stays first.
/// All-integer, so the export is deterministic. Reorders `weights`.
#[cfg(feature = "image")]
fn kernel_material_cell(weights: &mut [(u8, u32)]) -> MaterialCell {
    weights.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    match &weights[..] {
        [] => material_cell(TerrainMaterial::Grass.to_u8()),
        [(a, _)] => material_cell(*a),
        [(a, weight_a), (b, weight_b), ..] => {
            let blend = (*weight_b * 255 / (*weight_a + *weight_b)) as u8;
            canonical_material_cell(*a, *b, blend)
        }
    }
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
            default_layers: true,
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
            assert_eq!(summary.matmaps_written, 9);
            for cz in -1..=1 {
                for cx in -1..=1 {
                    assert!(toml_loader::chunk_matmap_path(&terrain, cx, cz).is_file());
                }
            }
        }
        #[cfg(not(feature = "image"))]
        assert_eq!(summary.matmaps_written, 0);

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
        let mut grid = plan_export_grid(&world.spec).unwrap();
        let floor = world_height_floor(&world);
        grid.height_scale -= floor;

        assert_eq!(parsed.terrain.chunk_resolution, grid.chunk_resolution);
        assert_eq!(parsed.terrain.chunk_size, grid.chunk_size);
        assert_eq!(parsed.terrain.height_scale, grid.height_scale);
        assert_eq!(
            parsed.terrain.height_offset, floor,
            "generated worlds normalize against a band whose floor is their lowest sample, capped at 0"
        );
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

        // Palette: the first four built-in material slots, named as their
        // `TerrainMaterial` discriminants are.
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

        let mut grid = plan_export_grid(&world.spec).unwrap();
        let floor = world_height_floor(&world);
        grid.height_scale -= floor;
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
                    let expected = sampler.height_at(gx, gz).clamp(floor, floor + grid.height_scale);
                    let reconstructed = floor + loaded[z * res + x] * grid.height_scale;
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
            (floor + loaded[0] * grid.height_scale - origin_height).abs() <= tolerance,
            "min-corner pixel must be the world origin sample: {} vs {origin_height}",
            floor + loaded[0] * grid.height_scale
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn seabed_below_zero_survives_the_export() {
        let world = test_world(3, 1, 256.0, 65, 0);
        let floor = world_height_floor(&world);
        // A world with no ground below Y = 0 would leave nothing to check;
        // then the floor is 0 and the export matches the unshifted band.
        if floor >= 0.0 {
            assert_eq!(floor, 0.0);
            return;
        }
        let root = temp_dir("seabed");
        export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");

        let mut grid = plan_export_grid(&world.spec).unwrap();
        grid.height_scale -= floor;
        let sampler = WorldSampler::new(&world).unwrap();
        let coords = cache_pixel_coords(&grid);
        let res = grid.chunk_resolution as usize;
        let half = grid.half_extent as i64;
        let tolerance = grid.height_scale / 65535.0 * 0.5 + grid.height_scale * 1e-6;

        // The deepest written pixel must reconstruct below Y = 0, at its
        // sampled depth, instead of clamping flat at 0.
        let mut deepest: Option<(f32, f32)> = None;
        for cz in -half..=half {
            for cx in -half..=half {
                let path = toml_loader::chunk_r16_path(&terrain, cx as i32, cz as i32);
                let loaded = toml_loader::load_chunk_r16(&path, grid.chunk_resolution).unwrap();
                for z in 0..res {
                    let gz = coords[((cz + half) as usize) * res + z];
                    for x in 0..res {
                        let gx = coords[((cx + half) as usize) * res + x];
                        let sampled = sampler.height_at(gx, gz);
                        if deepest.map_or(true, |(s, _)| sampled < s) {
                            deepest = Some((sampled, floor + loaded[z * res + x] * grid.height_scale));
                        }
                    }
                }
            }
        }
        let (sampled, reconstructed) = deepest.expect("the export wrote pixels");
        if sampled < 0.0 {
            assert!(reconstructed < 0.0, "seabed pixel sampled at {sampled} reloaded at {reconstructed}");
            assert!(
                (reconstructed - sampled).abs() <= tolerance,
                "seabed pixel sampled at {sampled} reloaded at {reconstructed}"
            );
        }
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
    fn matmap_cells_carry_the_region_material_ids() {
        use std::collections::BTreeMap;

        // Uniform Sand (4) world: every cell is Sand alone, even after the
        // kernel (all neighbours agree), not a bucket it used to share.
        let world = test_world(3, 1, 256.0, 65, TerrainMaterial::Sand.to_u8());
        let root = temp_dir("matmap_uniform");
        export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");
        let img = image::open(toml_loader::chunk_matmap_path(&terrain, 0, 0))
            .unwrap()
            .to_rgba8();
        assert_eq!(img.dimensions(), (64, 64));
        for pixel in img.pixels() {
            assert_eq!(pixel.0, material_cell(TerrainMaterial::Sand.to_u8()), "uniform Sand stays Sand");
        }
        std::fs::remove_dir_all(&root).ok();

        // Every one of the 23 ids, cycling per source sample: each cell must
        // be the two heaviest region materials under the kernel at that
        // cache pixel, counted here independently of the exporter.
        let mut world = test_world(2, 1, 256.0, 65, 0);
        for region in &mut world.regions {
            for (i, m) in region.materials.iter_mut().enumerate() {
                *m = (i % 23) as u8;
            }
        }
        let root = temp_dir("matmap_mixed");
        let summary = export_to_space(&world, &root).unwrap();
        let terrain = root.join("Workspace").join("Terrain");
        let grid = plan_export_grid(&world.spec).unwrap();
        let sampler = WorldSampler::new(&world).unwrap();
        let coords = cache_pixel_coords(&grid);
        let res = grid.chunk_resolution as usize;
        let half = grid.half_extent as i64;
        let w = coords.len();
        let mut seen: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
        for (cx, cz) in [(0i64, 0i64), (-1, 1)] {
            let img = image::open(toml_loader::chunk_matmap_path(&terrain, cx as i32, cz as i32))
                .unwrap()
                .to_rgba8();
            for z in 0..res {
                for x in 0..res {
                    let gz = ((cz + half) as usize) * res + z;
                    let gx = ((cx + half) as usize) * res + x;
                    let mut counts: BTreeMap<u8, u32> = BTreeMap::new();
                    for (dz, row) in [[1u32, 2, 1], [2, 4, 2], [1, 2, 1]].iter().enumerate() {
                        for (dx, k) in row.iter().enumerate() {
                            let qz = (gz as i64 + dz as i64 - 1).clamp(0, w as i64 - 1) as usize;
                            let qx = (gx as i64 + dx as i64 - 1).clamp(0, w as i64 - 1) as usize;
                            *counts.entry(sampler.material_at(coords[qx], coords[qz])).or_default() += k;
                        }
                    }
                    let mut ranked: Vec<(u8, u32)> = counts.into_iter().collect();
                    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                    let expected = match ranked.as_slice() {
                        [(a, _)] => material_cell(*a),
                        [(a, wa), (b, wb), ..] => canonical_material_cell(*a, *b, (wb * 255 / (wa + wb)) as u8),
                        [] => unreachable!("the kernel always sees nine samples"),
                    };
                    let cell = img.get_pixel(x as u32, z as u32).0;
                    assert_eq!(cell, expected, "chunk x{cx}_z{cz} pixel ({x}, {z})");
                    assert!(cell[0] < 23 && (cell[1] < 23 || cell[1] == MATERIAL_SLOT_NONE));
                    seen.insert(cell[0]);
                }
            }
        }
        assert!(seen.len() > 4, "the matmap keeps more than the four old buckets: {seen:?}");

        // The loader reads the cells back exactly.
        let parsed = toml_loader::load_terrain_toml(&terrain.join("_terrain.toml")).unwrap();
        let config = parsed.to_terrain_config();
        let mut data = crate::terrain::TerrainData::procedural();
        data.resize_cache(&config);
        toml_loader::load_chunks_from_disk(&terrain, &config, &mut data);
        let img = image::open(toml_loader::chunk_matmap_path(&terrain, 0, 0)).unwrap().to_rgba8();
        let cache_w = data.cache_width as usize;
        let (x0, z0) = (half as usize * res, half as usize * res);
        for z in 0..res {
            for x in 0..res {
                assert_eq!(data.material_cache[(z0 + z) * cache_w + x0 + x], img.get_pixel(x as u32, z as u32).0);
            }
        }
        assert_eq!(summary.matmaps_written, (grid.chunks_per_axis() * grid.chunks_per_axis()) as usize);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn exports_clear_a_previous_terrains_material_maps() {
        let root = temp_dir("stale_materials");
        let terrain = root.join("Workspace").join("Terrain");
        // A legacy splatmap and an out-of-range matmap from an older terrain.
        for dir in [LEGACY_SPLATMAP_DIR, MATMAP_DIR] {
            std::fs::create_dir_all(terrain.join(dir)).unwrap();
            std::fs::write(terrain.join(dir).join("x9_z9.png"), b"stale").unwrap();
        }
        export_flat_to_space(&FlatSpec { half_extent: 1, chunk_resolution: 16, ..Default::default() }, &root).unwrap();
        assert!(!terrain.join(LEGACY_SPLATMAP_DIR).exists(), "no legacy splatmap outlives an export");
        assert!(!terrain.join(MATMAP_DIR).join("x9_z9.png").exists());
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
    // ── Flat plate ("baseplate") export ──────────────────────────────────

    #[test]
    fn flat_export_round_trips_to_a_dead_flat_surface() {
        let root = temp_dir("flat_round_trip");
        // A surface BELOW world Y = 0, inside a band that reaches further
        // down: the toml must carry the negative offset, or the plate
        // reloads 32 m too high.
        let spec = FlatSpec {
            half_extent: 2,
            chunk_size: 64.0,
            chunk_resolution: 32,
            height_m: -12.5,
            height_offset: -32.0,
            height_scale: 128.0,
            material_slot: 0,
            seed: 7,
        };
        let summary = export_flat_to_space(&spec, &root).expect("flat export");
        assert_eq!(summary.chunks_written, 25); // (2*2+1)^2

        // Load back through the SAME path the engine hydrates with.
        let terrain = root.join("Workspace").join("Terrain");
        let toml = toml_loader::load_terrain_toml(&terrain.join("_terrain.toml")).unwrap();
        assert_eq!(toml.terrain.height_offset, -32.0);
        let config = toml.to_terrain_config();

        // view_distance is load-bearing: the loader must re-derive N = 2, or
        // every chunk lands at the wrong cache offset.
        assert_eq!(config.chunks_x, 2);
        assert_eq!(config.chunks_z, 2);
        assert_eq!(config.chunk_resolution, 32);
        assert_eq!(config.height_scale, 128.0);
        assert_eq!(config.height_offset, -32.0);

        let mut data = crate::terrain::TerrainData::procedural();
        data.resize_cache(&config);
        let loaded = toml_loader::load_chunks_from_disk(&terrain, &config, &mut data);
        assert_eq!(loaded.len(), 25);

        // Every cache sample is the SAME height, and it is the authored one.
        // (u16 quantisation: 19.5/128 * 65535 = 9983.85 -> 9984 -> 0.1523461.)
        let expected = (spec.height_m - spec.height_offset) / spec.height_scale;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for &h in &data.height_cache {
            lo = lo.min(h);
            hi = hi.max(h);
        }
        assert!(
            (hi - lo).abs() < 1e-6,
            "plate is not flat: spread {} (lo {lo}, hi {hi})",
            hi - lo
        );
        assert!(
            (lo - expected).abs() < 1.0 / 65535.0,
            "plate sample {lo} != authored {expected}"
        );

        // And in world space: the loaded surface is where it was authored,
        // within half a u16 quantum of the band.
        let tolerance = 0.5 * spec.height_scale / 65535.0 + 1e-4;
        let world = config.world_height(lo);
        assert!(
            (world - spec.height_m).abs() <= tolerance,
            "plate reloaded at Y={world}, authored Y={}",
            spec.height_m
        );
        let queried = crate::terrain::height_query::height_at_world(&config, &data, 10.0, -20.0);
        assert!(
            (queried - spec.height_m).abs() <= tolerance,
            "height_at_world reads Y={queried}, authored Y={}",
            spec.height_m
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(feature = "image")]
    #[test]
    fn flat_export_writes_a_uniform_matmap_of_its_slot() {
        let root = temp_dir("flat_matmap");
        let basalt = TerrainMaterial::Basalt.to_u8();
        let spec = FlatSpec {
            half_extent: 1,
            chunk_resolution: 16,
            material_slot: basalt,
            ..Default::default()
        };
        let summary = export_flat_to_space(&spec, &root).expect("flat export");
        assert_eq!(summary.matmaps_written, 9);

        let terrain = root.join("Workspace").join("Terrain");
        for (cx, cz) in [(0, 0), (-1, 1), (1, -1)] {
            let img = image::open(toml_loader::chunk_matmap_path(&terrain, cx, cz))
                .expect("matmap png")
                .to_rgba8();
            assert_eq!(img.dimensions(), (16, 16));
            assert!(img.pixels().all(|px| px.0 == material_cell(basalt)), "chunk x{cx}_z{cz}");
        }

        // Loaded back, every cell of the plate is Basalt.
        let config = toml_loader::load_terrain_toml(&terrain.join("_terrain.toml")).unwrap().to_terrain_config();
        let mut data = crate::terrain::TerrainData::procedural();
        data.resize_cache(&config);
        assert_eq!(toml_loader::load_chunks_from_disk(&terrain, &config, &mut data).len(), 9);
        assert!(data.has_material_layer());
        assert!(data.material_cache.iter().all(|cell| *cell == material_cell(basalt)));

        // A custom slot is written as it is.
        export_flat_to_space(&FlatSpec { material_slot: 200, ..spec }, &root).unwrap();
        let img = image::open(toml_loader::chunk_matmap_path(&terrain, 0, 0)).unwrap().to_rgba8();
        assert!(img.pixels().all(|px| px.0 == material_cell(200)));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn flat_export_is_byte_deterministic() {
        let spec = FlatSpec::default();
        let a = temp_dir("flat_det_a");
        let b = temp_dir("flat_det_b");
        export_flat_to_space(&spec, &a).unwrap();
        export_flat_to_space(&spec, &b).unwrap();
        for rel in [
            "_terrain.toml",
            "chunks/x0_z0.r16",
            "chunks/x-4_z3.r16",
            "materials/grass.mat.toml",
        ] {
            let pa = a.join("Workspace").join("Terrain").join(rel);
            let pb = b.join("Workspace").join("Terrain").join(rel);
            assert_eq!(
                std::fs::read(&pa).unwrap(),
                std::fs::read(&pb).unwrap(),
                "{rel} differs between two exports of the same spec"
            );
        }
    }

    #[test]
    fn flat_export_clears_a_larger_previous_export() {
        let root = temp_dir("flat_shrink");
        export_flat_to_space(
            &FlatSpec {
                half_extent: 3,
                chunk_resolution: 16,
                ..Default::default()
            },
            &root,
        )
        .unwrap();
        let terrain = root.join("Workspace").join("Terrain");
        assert!(toml_loader::chunk_r16_path(&terrain, 3, 3).is_file());

        export_flat_to_space(
            &FlatSpec {
                half_extent: 1,
                chunk_resolution: 16,
                ..Default::default()
            },
            &root,
        )
        .unwrap();
        assert!(
            !toml_loader::chunk_r16_path(&terrain, 3, 3).exists(),
            "stale out-of-range chunk survived a smaller re-export"
        );
        assert!(toml_loader::chunk_r16_path(&terrain, 1, 1).is_file());
    }

    #[test]
    fn flat_spec_rejects_specs_the_loader_cannot_round_trip() {
        // chunk_size must be a whole power of two, or ceil(view_distance /
        // chunk_size) can land off by one and shift every chunk.
        assert!(FlatSpec {
            chunk_size: 100.0,
            ..Default::default()
        }
        .grid()
        .is_err());
        // R16 cannot represent a surface above the ceiling it normalizes to.
        assert!(FlatSpec {
            height_m: 150.0,
            height_scale: 100.0,
            ..Default::default()
        }
        .grid()
        .is_err());
        // ...nor below the band floor (`height_offset`, 0 by default).
        assert!(FlatSpec {
            height_m: -1.0,
            ..Default::default()
        }
        .grid()
        .is_err());
        // A non-finite offset has no band at all.
        assert!(FlatSpec {
            height_offset: f32::NAN,
            ..Default::default()
        }
        .grid()
        .is_err());
        // half_extent 0 => view_distance 0 => the streamer loads nothing.
        assert!(FlatSpec {
            half_extent: 0,
            ..Default::default()
        }
        .grid()
        .is_err());
        // 255 is "no material", not a slot to paint a plate with; every
        // other id, built-in or custom, is.
        assert!(FlatSpec {
            material_slot: MATERIAL_SLOT_NONE,
            ..Default::default()
        }
        .grid()
        .is_err());
        for slot in [4u8, 22, 23, 254] {
            assert!(FlatSpec { material_slot: slot, ..Default::default() }.grid().is_ok(), "slot {slot}");
        }
        assert!(FlatSpec::default().grid().is_ok());
        assert_eq!(FlatSpec::default().material_slot, TerrainMaterial::Grass.to_u8());
    }

    #[test]
    fn flat_spec_accepts_any_surface_inside_a_band_below_zero() {
        // The Studio preset's band: floor -32, ceiling -32 + 128 = 96.
        let band = |height_m: f32| FlatSpec {
            height_m,
            height_offset: -32.0,
            height_scale: 128.0,
            ..Default::default()
        };
        for inside in [-32.0f32, -10.0, 0.0, 50.0, 96.0] {
            assert!(band(inside).grid().is_ok(), "height_m {inside} is inside [-32, 96]");
        }
        for outside in [-32.5f32, -100.0, 96.5, 200.0] {
            assert!(band(outside).grid().is_err(), "height_m {outside} is outside [-32, 96]");
        }
    }
}
