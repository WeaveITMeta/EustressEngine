//! # Terrain TOML Loader — File-System-First Config
//!
//! ## Table of Contents
//! 1. TOML Deserialization Structs — map `_terrain.toml` fields to Rust types
//! 2. Conversion — `TerrainTomlFile` → `TerrainConfig` + `TerrainData`
//! 3. Chunk R16 I/O — per-chunk 16-bit heightmap save/load for git-friendly storage
//! 4. Loader Entry Point — `load_terrain_toml()` reads config from filesystem
//!
//! ## Filesystem Layout
//! ```
//! Space1/Workspace/Terrain/
//!   _terrain.toml             ← Master config
//!   chunks/x{N}_z{N}.r16     ← Per-chunk heightmap (16-bit, little-endian)
//!   matmap/x{N}_z{N}.png     ← Per-chunk material cells, RGBA8 [id_a, id_b, blend_b, 0]
//!   materials/*.mat.toml      ← Material slot definitions (see `material_slots`)
//! ```
//!
//! ## Material maps
//! A matmap holds one material cell (see the `material` module docs) per
//! raster cell of its chunk, with the tile offsets and row order of the R16
//! beside it. Every writer of this directory emits one per chunk. The
//! loader reads a chunk's matmap when there is one; failing that it
//! converts a legacy `splatmap/x{N}_z{N}.png` (4-bucket weights) through
//! [`convert_legacy_splat_tile`]; failing that the chunk stays all Grass.
//! [`save_material_chunks_to_disk`] writes matmaps and removes the legacy
//! splatmaps of the chunks it wrote, so each chunk has one source of truth.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use super::material::{legacy_bucket3_is_snow, legacy_splat_to_material_cell, material_cell, MaterialCell, TerrainMaterial};

// ============================================================================
// 1. TOML Deserialization Structs
// ============================================================================

/// Root `_terrain.toml` file structure
#[derive(Deserialize, Debug, Clone)]
pub struct TerrainTomlFile {
    pub terrain: TerrainTomlConfig,
    #[serde(default)]
    pub streaming: TerrainTomlStreaming,
    #[serde(default)]
    pub lod: TerrainTomlLod,
    #[serde(default)]
    pub materials: TerrainTomlMaterials,
    #[serde(default)]
    pub water: TerrainTomlWater,
}

/// Core terrain configuration
#[derive(Deserialize, Debug, Clone)]
pub struct TerrainTomlConfig {
    /// World units per chunk side (default: 64.0)
    #[serde(default = "default_chunk_size")]
    pub chunk_size: f32,
    /// Vertices per chunk side (default: 64)
    #[serde(default = "default_chunk_resolution")]
    pub chunk_resolution: u32,
    /// World-space range R16 values span above `height_offset` (default: 50.0)
    #[serde(default = "default_height_scale")]
    pub height_scale: f32,
    /// World Y of R16 value 0 (default: 0.0). Negative values let the
    /// surface sit below world Y = 0. Files written before this key existed
    /// omit it, and 0.0 reproduces how they always loaded.
    #[serde(default)]
    pub height_offset: f32,
    /// Procedural generation seed (default: 42)
    #[serde(default = "default_seed")]
    pub seed: u32,
    /// Global sea level in world Y (default: 0.0)
    #[serde(default)]
    pub water_level: f32,
}

/// Streaming/loading configuration
#[derive(Deserialize, Debug, Clone)]
pub struct TerrainTomlStreaming {
    /// Chunk load radius in world units (default: 1000.0)
    #[serde(default = "default_view_distance")]
    pub view_distance: f32,
    /// Extra distance before despawn to prevent popping (default: 200.0)
    #[serde(default = "default_cull_margin")]
    pub cull_margin: f32,
    /// Max chunks generated per frame (default: 4)
    #[serde(default = "default_chunks_per_frame")]
    pub chunks_per_frame: usize,
}

impl Default for TerrainTomlStreaming {
    fn default() -> Self {
        Self {
            view_distance: default_view_distance(),
            cull_margin: default_cull_margin(),
            chunks_per_frame: default_chunks_per_frame(),
        }
    }
}

/// Level of detail configuration
#[derive(Deserialize, Debug, Clone)]
pub struct TerrainTomlLod {
    /// Number of LOD levels (default: 4)
    #[serde(default = "default_lod_levels")]
    pub levels: u32,
    /// Distance thresholds for each LOD level (default: [100, 200, 400, 800])
    #[serde(default = "default_lod_distances")]
    pub distances: Vec<f32>,
}

impl Default for TerrainTomlLod {
    fn default() -> Self {
        Self {
            levels: default_lod_levels(),
            distances: default_lod_distances(),
        }
    }
}

/// Material palette configuration
#[derive(Deserialize, Debug, Clone, Default)]
pub struct TerrainTomlMaterials {
    /// Ordered list of material slots
    #[serde(default)]
    pub palette: Vec<TerrainTomlMaterialSlot>,
}

/// Single material slot in the palette
#[derive(Deserialize, Debug, Clone)]
pub struct TerrainTomlMaterialSlot {
    /// Material slot id this entry describes, the id matmap cells store:
    /// 0-22 are the built-in materials, 23-254 a Space's custom ones
    pub slot: u8,
    /// Display name (example: "Grass")
    pub name: String,
    /// Path to `.mat.toml` file, relative to terrain directory
    pub file: String,
}

/// Water configuration
#[derive(Deserialize, Debug, Clone)]
pub struct TerrainTomlWater {
    /// Whether water rendering is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Sea level in world Y
    #[serde(default)]
    pub sea_level: f32,
    /// Water mode: "static" (voxel plane) or "dynamic" (realism crate hydro)
    #[serde(default = "default_water_mode")]
    pub mode: String,
    /// Water tint color [r, g, b, a] in 0.0-1.0 range
    #[serde(default = "default_water_color")]
    pub color: [f32; 4],
}

impl Default for TerrainTomlWater {
    fn default() -> Self {
        Self {
            enabled: false,
            sea_level: 0.0,
            mode: default_water_mode(),
            color: default_water_color(),
        }
    }
}

// ============================================================================
// Default value functions for serde
// ============================================================================

fn default_chunk_size() -> f32 { 64.0 }
fn default_chunk_resolution() -> u32 { 64 }
fn default_height_scale() -> f32 { 50.0 }
fn default_seed() -> u32 { 42 }
fn default_view_distance() -> f32 { 1000.0 }
fn default_cull_margin() -> f32 { 200.0 }
fn default_chunks_per_frame() -> usize { 4 }
fn default_lod_levels() -> u32 { 4 }
fn default_lod_distances() -> Vec<f32> { vec![100.0, 200.0, 400.0, 800.0] }
fn default_water_mode() -> String { "static".to_string() }
fn default_water_color() -> [f32; 4] { [0.1, 0.3, 0.6, 0.8] }

// ============================================================================
// 2. Conversion — TOML → TerrainConfig
// ============================================================================

impl TerrainTomlFile {
    /// Convert TOML config into engine `TerrainConfig`
    pub fn to_terrain_config(&self) -> super::TerrainConfig {
        super::TerrainConfig {
            chunk_size: self.terrain.chunk_size,
            chunk_resolution: self.terrain.chunk_resolution,
            // For infinite streaming, chunks_x/chunks_z represent the initial view radius in chunks
            // Actual chunk count is dynamic based on camera position
            chunks_x: (self.streaming.view_distance / self.terrain.chunk_size).ceil() as u32,
            chunks_z: (self.streaming.view_distance / self.terrain.chunk_size).ceil() as u32,
            lod_levels: self.lod.levels,
            lod_distances: self.lod.distances.clone(),
            view_distance: self.streaming.view_distance,
            height_scale: self.terrain.height_scale,
            height_offset: self.terrain.height_offset,
            seed: self.terrain.seed,
        }
    }
}

// ============================================================================
// 3. Chunk R16 I/O — per-chunk 16-bit heightmap persistence
// ============================================================================

/// Build the filesystem path for a chunk's R16 heightmap file
pub fn chunk_r16_path(terrain_dir: &Path, chunk_x: i32, chunk_z: i32) -> PathBuf {
    terrain_dir.join("chunks").join(format!("x{}_z{}.r16", chunk_x, chunk_z))
}

/// Directory of a terrain's per-chunk material maps.
pub const MATMAP_DIR: &str = "matmap";

/// Directory of the legacy 4-bucket splatmaps, read only to convert them.
pub const LEGACY_SPLATMAP_DIR: &str = "splatmap";

/// Build the filesystem path for a chunk's material map (`matmap/*.png`)
pub fn chunk_matmap_path(terrain_dir: &Path, chunk_x: i32, chunk_z: i32) -> PathBuf {
    terrain_dir.join(MATMAP_DIR).join(format!("x{}_z{}.png", chunk_x, chunk_z))
}

/// Build the filesystem path for a chunk's LEGACY splatmap, which the
/// loader converts when the chunk has no matmap. Nothing writes these.
pub fn chunk_splatmap_path(terrain_dir: &Path, chunk_x: i32, chunk_z: i32) -> PathBuf {
    terrain_dir.join(LEGACY_SPLATMAP_DIR).join(format!("x{}_z{}.png", chunk_x, chunk_z))
}

/// Load a chunk's height data from an R16 file
///
/// Returns a Vec<f32> of normalized heights (0.0-1.0), sized `resolution × resolution`.
/// The raw R16 file stores 16-bit unsigned integers (little-endian), mapped to 0.0-1.0.
pub fn load_chunk_r16(path: &Path, resolution: u32) -> Result<Vec<f32>, String> {
    let expected_bytes = (resolution * resolution * 2) as usize;
    let data = std::fs::read(path)
        .map_err(|error| format!("Failed to read R16 file {:?}: {}", path, error))?;
    
    if data.len() != expected_bytes {
        return Err(format!(
            "R16 file {:?} size mismatch: expected {} bytes ({}×{} × 2), got {}",
            path, expected_bytes, resolution, resolution, data.len()
        ));
    }
    
    // Convert pairs of bytes (little-endian u16) to normalized f32
    let heights: Vec<f32> = data
        .chunks_exact(2)
        .map(|pair| {
            let raw = u16::from_le_bytes([pair[0], pair[1]]);
            raw as f32 / 65535.0
        })
        .collect();
    
    Ok(heights)
}

/// Save a chunk's height data to an R16 file
///
/// Heights should be normalized 0.0-1.0. Values are clamped and converted
/// to 16-bit unsigned integers (little-endian).
pub fn save_chunk_r16(path: &Path, heights: &[f32], resolution: u32) -> Result<(), String> {
    let expected_count = (resolution * resolution) as usize;
    if heights.len() != expected_count {
        return Err(format!(
            "Height data size mismatch: expected {} values ({}×{}), got {}",
            expected_count, resolution, resolution, heights.len()
        ));
    }
    
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create chunks directory {:?}: {}", parent, error))?;
    }
    
    // Convert normalized f32 to little-endian u16 bytes
    let mut bytes = Vec::with_capacity(heights.len() * 2);
    for &height in heights {
        let clamped = height.clamp(0.0, 1.0);
        let raw = (clamped * 65535.0).round() as u16;
        bytes.extend_from_slice(&raw.to_le_bytes());
    }
    
    std::fs::write(path, &bytes)
        .map_err(|error| format!("Failed to write R16 file {:?}: {}", path, error))?;
    
    Ok(())
}

/// `_terrain.toml` text with `height_offset` and `height_scale` of its
/// `[terrain]` table set to `band`, every other byte kept: comments, key
/// order, spacing, line endings and the other tables. A key the table lacks
/// is added after its last key. Also returns the band exactly as
/// [`load_terrain_toml`] will read it back, which is the band a caller should
/// store its raster in, so the raster and the file cannot disagree by an
/// ulp.
///
/// Refuses a file with no `[terrain]` table and a result that does not parse
/// or does not read back as `band`, so a file it cannot edit cleanly is
/// never written.
pub fn rewrite_height_band(text: &str, band: super::HeightBand) -> Result<(String, super::HeightBand), String> {
    if !(band.offset.is_finite() && band.scale.is_finite()) {
        return Err(format!("height band {band:?} is not finite"));
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    let mut in_terrain = false;
    let mut header: Option<usize> = None;
    let mut last_key: Option<usize> = None;
    let (mut wrote_offset, mut wrote_scale) = (false, false);
    for (index, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_terrain = toml_table_name(trimmed) == Some("terrain");
            if in_terrain && header.is_none() {
                header = Some(index);
            }
            continue;
        }
        if !in_terrain || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        last_key = Some(index);
        let value = match trimmed.split_once('=').map(|(key, _)| key.trim()) {
            Some("height_offset") => {
                wrote_offset = true;
                band.offset
            }
            Some("height_scale") => {
                wrote_scale = true;
                band.scale
            }
            _ => continue,
        };
        *line = replace_toml_value(line, value);
    }
    let Some(header) = header else {
        return Err("_terrain.toml has no [terrain] table".to_string());
    };
    let mut missing = String::new();
    if !wrote_scale {
        missing.push_str(&format!("height_scale = {:?}{newline}", band.scale));
    }
    if !wrote_offset {
        missing.push_str(&format!("height_offset = {:?}{newline}", band.offset));
    }
    if !missing.is_empty() {
        let at = last_key.unwrap_or(header);
        if !lines[at].ends_with('\n') {
            lines[at].push_str(newline);
        }
        lines.insert(at + 1, missing);
    }
    let out = lines.concat();

    let parsed: TerrainTomlFile = toml::from_str(&out)
        .map_err(|error| format!("_terrain.toml would not parse with the new height band: {error}"))?;
    let read_back = super::HeightBand { offset: parsed.terrain.height_offset, scale: parsed.terrain.height_scale };
    let close = |a: f32, b: f32| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0);
    if !(close(read_back.offset, band.offset) && close(read_back.scale, band.scale)) {
        return Err(format!("_terrain.toml reads back height band {read_back:?} instead of {band:?}"));
    }
    Ok((out, read_back))
}

/// Name of the table a `[name]` header line opens, `None` for an array of
/// tables (`[[name]]`), which is never the `[terrain]` table.
fn toml_table_name(trimmed: &str) -> Option<&str> {
    if trimmed.starts_with("[[") {
        return None;
    }
    let inner = trimmed.strip_prefix('[')?;
    Some(inner[..inner.find(']')?].trim())
}

/// `key = value  # comment` with the value replaced by `value`, keeping the
/// key, the spacing around `=`, the comment and the line ending.
fn replace_toml_value(line: &str, value: f32) -> String {
    let (body, ending) = match line.strip_suffix("\r\n") {
        Some(body) => (body, "\r\n"),
        None => match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        },
    };
    let Some(eq) = body.find('=') else {
        return line.to_string();
    };
    let after = &body[eq + 1..];
    let lead = after.len() - after.trim_start().len();
    let rest = &after[lead..];
    let value_end = rest.find('#').unwrap_or(rest.len());
    let value_len = rest[..value_end].trim_end().len();
    format!("{}{}{:?}{}{}", &body[..eq + 1], &after[..lead], value, &rest[value_len..], ending)
}

/// Set `height_offset` and `height_scale` in `terrain_dir/_terrain.toml` to
/// `band` with [`rewrite_height_band`], replacing the file through a staging
/// file so an interrupted write leaves the old one whole. Returns the band as
/// the loader will read it back, or `None` when the directory has no
/// `_terrain.toml`: nothing reads its heights back then, and creating one
/// here would invent every other setting.
pub fn write_height_band_to_toml(terrain_dir: &Path, band: super::HeightBand) -> Result<Option<super::HeightBand>, String> {
    let path = terrain_dir.join("_terrain.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Failed to read terrain TOML {:?}: {}", path, error)),
    };
    let (updated, read_back) = rewrite_height_band(&text, band)?;
    if updated != text {
        let staging = terrain_dir.join("_terrain.toml.tmp");
        std::fs::write(&staging, &updated)
            .map_err(|error| format!("Failed to write terrain TOML {:?}: {}", staging, error))?;
        if let Err(error) = std::fs::rename(&staging, &path) {
            let _ = std::fs::remove_file(&staging);
            return Err(format!("Failed to move terrain TOML into place at {:?}: {}", path, error));
        }
    }
    Ok(Some(read_back))
}

// ============================================================================
// 4. Loader Entry Point
// ============================================================================

/// Load terrain configuration from a `_terrain.toml` file
pub fn load_terrain_toml(path: &Path) -> Result<TerrainTomlFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("Failed to read terrain TOML {:?}: {}", path, error))?;
    
    let parsed: TerrainTomlFile = toml::from_str(&content)
        .map_err(|error| format!("Failed to parse terrain TOML {:?}: {}", path, error))?;
    
    Ok(parsed)
}

/// Create a default `_terrain.toml` file at the given path
pub fn create_default_terrain_toml(terrain_dir: &Path) -> Result<PathBuf, String> {
    // Ensure directories exist
    std::fs::create_dir_all(terrain_dir.join("chunks"))
        .map_err(|error| format!("Failed to create chunks dir: {}", error))?;
    std::fs::create_dir_all(terrain_dir.join(MATMAP_DIR))
        .map_err(|error| format!("Failed to create matmap dir: {}", error))?;
    std::fs::create_dir_all(terrain_dir.join("materials"))
        .map_err(|error| format!("Failed to create materials dir: {}", error))?;
    
    let toml_path = terrain_dir.join("_terrain.toml");
    let content = r#"# Eustress Engine — Terrain Configuration
# This file defines the terrain for the current Space.
# Per-chunk heightmaps are stored in chunks/ as .r16 files (16-bit, git-friendly).

[terrain]
chunk_size = 64.0               # World units per chunk side
chunk_resolution = 64           # Vertices per chunk side
height_scale = 50.0             # Height range above height_offset
height_offset = 0.0             # World Y of the lowest height (negative = below Y 0)
seed = 42                       # Procedural generation seed
water_level = 0.0               # Global sea level (world Y)

[streaming]
view_distance = 1000.0          # Chunk load radius in world units
cull_margin = 200.0             # Extra distance before despawn (prevents popping)
chunks_per_frame = 4            # Max chunks generated per frame

[lod]
levels = 4
distances = [100.0, 200.0, 400.0, 800.0]

[materials]
# Material palette. `slot` is the material slot id matmap cells store:
# 0-22 are the built-in materials (0 Grass, 1 Rock, 2 Dirt, 3 Snow, ...),
# 23-254 this Space's custom ones.

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
sea_level = 0.0
mode = "static"                 # "static" = plane, "dynamic" = realism crate hydro
color = [0.1, 0.3, 0.6, 0.8]
"#;
    
    std::fs::write(&toml_path, content)
        .map_err(|error| format!("Failed to write _terrain.toml: {}", error))?;
    
    Ok(toml_path)
}

/// Load all available chunk heightmaps from the terrain directory into a `TerrainData`
///
/// Scans `terrain_dir/chunks/` for `x{N}_z{N}.r16` files and populates the
/// height cache at the correct offsets, and each found chunk's material
/// cells from its matmap, else its converted legacy splatmap, else Grass
/// (see the module docs). Returns a list of chunk coordinates found.
pub fn load_chunks_from_disk(
    terrain_dir: &Path,
    config: &super::TerrainConfig,
    data: &mut super::TerrainData,
) -> Vec<bevy::math::IVec2> {
    use bevy::math::IVec2;

    // Ensure height cache is sized
    if data.height_cache.is_empty() {
        data.resize_cache(config);
    }
    // Every cell starts as Grass, so ground whose chunk has neither a
    // matmap nor a legacy splatmap still renders as a material, never black
    // and never as the altitude colouring of a terrain without materials.
    // This runs before the chunks/ check: a terrain whose toml has no
    // chunks yet is all Grass too, and Save then writes its matmaps, so a
    // save and reopen keeps its look.
    if !data.has_material_layer() {
        let cells = data.cache_width as usize * data.cache_height as usize;
        data.material_cache = vec![material_cell(TerrainMaterial::Grass.to_u8()); cells];
    }
    data.material_dirty = true;

    let chunks_dir = terrain_dir.join("chunks");
    if !chunks_dir.exists() {
        return Vec::new();
    }

    let mut loaded_chunks = Vec::new();
    
    // Scan for R16 files matching the x{N}_z{N}.r16 pattern
    let entries = match std::fs::read_dir(&chunks_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        
        // Parse "x{N}_z{N}.r16" pattern
        if !name.ends_with(".r16") { continue; }
        let stem = &name[..name.len() - 4]; // strip .r16
        let parts: Vec<&str> = stem.split('_').collect();
        if parts.len() != 2 { continue; }
        
        let chunk_x: i32 = match parts[0].strip_prefix('x').and_then(|s| s.parse().ok()) {
            Some(value) => value,
            None => continue,
        };
        let chunk_z: i32 = match parts[1].strip_prefix('z').and_then(|s| s.parse().ok()) {
            Some(value) => value,
            None => continue,
        };
        
        // Load the R16 data
        let r16_path = entry.path();
        match load_chunk_r16(&r16_path, config.chunk_resolution) {
            Ok(heights) => {
                // Write heights into the global height cache at the chunk's offset
                write_chunk_to_cache(
                    data,
                    config,
                    IVec2::new(chunk_x, chunk_z),
                    &heights,
                );
                // The chunk's material cells, read after its heights: a
                // legacy splatmap needs them to tell snow from water.
                #[cfg(feature = "image")]
                load_chunk_materials(terrain_dir, config, data, IVec2::new(chunk_x, chunk_z), &heights);
                loaded_chunks.push(IVec2::new(chunk_x, chunk_z));
            }
            Err(error) => {
                tracing::warn!("Skipping chunk x{}_z{}: {}", chunk_x, chunk_z, error);
            }
        }
    }
    
    loaded_chunks
}

/// Write a chunk's height values into the global `TerrainData.height_cache`.
///
/// Public so the Wave 9.C voxel loader (engine-side) and the voxel
/// extractor (`super::voxel_extract`) reuse the EXACT offset math the
/// `.r16` disk path uses — a chunk at grid `(chunk_x, chunk_z)` lands at
/// cache offset `((chunk_pos + half) * resolution)`, the same centering
/// `generate_chunk_mesh` reads back via its `world_u`/`world_v`. Heights
/// are whatever `TerrainConfig::world_height` turns into world Y; the
/// voxel path stores raw studs with `height_scale = 1.0` and
/// `height_offset = 0.0`.
pub fn write_chunk_to_cache(
    data: &mut super::TerrainData,
    config: &super::TerrainConfig,
    chunk_pos: bevy::math::IVec2,
    heights: &[f32],
) {
    let resolution = config.chunk_resolution as usize;
    let cache_width = data.cache_width as usize;
    
    // Calculate the pixel offset of this chunk in the global cache
    // Chunk (0,0) is at center, chunks extend in both directions
    let half_x = config.chunks_x as i32;
    let half_z = config.chunks_z as i32;
    let offset_x = ((chunk_pos.x + half_x) as usize) * resolution;
    let offset_z = ((chunk_pos.y + half_z) as usize) * resolution;
    
    // Copy row by row
    for row in 0..resolution {
        let src_start = row * resolution;
        let dst_start = (offset_z + row) * cache_width + offset_x;
        
        if src_start + resolution <= heights.len() && dst_start + resolution <= data.height_cache.len() {
            data.height_cache[dst_start..dst_start + resolution]
                .copy_from_slice(&heights[src_start..src_start + resolution]);
        }
    }
}

/// Read `chunk_pos`'s material cells into `data`: its matmap when it has one
/// that decodes, else its legacy splatmap converted with the chunk's
/// `heights` (normalized, as the R16 held them), else nothing, leaving the
/// Grass the loader started every cell at. A matmap that fails to decode
/// is reported and the legacy splatmap tried in its place.
#[cfg(feature = "image")]
fn load_chunk_materials(
    terrain_dir: &Path,
    config: &super::TerrainConfig,
    data: &mut super::TerrainData,
    chunk_pos: bevy::math::IVec2,
    heights: &[f32],
) {
    let matmap = chunk_matmap_path(terrain_dir, chunk_pos.x, chunk_pos.y);
    if matmap.exists() {
        match load_chunk_png_rgba(&matmap, config.chunk_resolution) {
            Ok(cells) => {
                write_material_tile_to_cache(data, config, chunk_pos, &cells);
                return;
            }
            Err(error) => tracing::warn!("Material map x{}_z{}: {}", chunk_pos.x, chunk_pos.y, error),
        }
    }
    let splatmap = chunk_splatmap_path(terrain_dir, chunk_pos.x, chunk_pos.y);
    if splatmap.exists() {
        match load_chunk_png_rgba(&splatmap, config.chunk_resolution) {
            Ok(pixels) => {
                let cells = convert_legacy_splat_tile(config, &pixels, heights);
                write_material_tile_to_cache(data, config, chunk_pos, &cells);
            }
            Err(error) => tracing::warn!("Legacy splatmap x{}_z{}: {}", chunk_pos.x, chunk_pos.y, error),
        }
    }
}

/// Decode a chunk PNG (a matmap or a legacy splatmap) into its RGBA8 pixels,
/// row-major z-then-x. Expects a `resolution × resolution` image, the form
/// every writer of these files uses. Requires the `image` feature.
#[cfg(feature = "image")]
fn load_chunk_png_rgba(path: &Path, resolution: u32) -> Result<Vec<[u8; 4]>, String> {
    let img = image::open(path)
        .map_err(|e| format!("open PNG {:?}: {}", path, e))?
        .to_rgba8();
    if img.width() != resolution || img.height() != resolution {
        return Err(format!(
            "PNG {:?} is {}×{}, expected {}×{}",
            path, img.width(), img.height(), resolution, resolution
        ));
    }
    Ok(img.pixels().map(|p| p.0).collect())
}

/// Material cells for one chunk of a legacy splatmap: each pixel's two
/// heaviest buckets (see [`legacy_splat_to_material_cell`]), the fourth
/// bucket read as Snow or Water by the height of the same cell in
/// `heights`, the chunk's normalized R16 samples in the same row-major
/// order, exactly as the old vertex-colour mesher split it. A pixel with no
/// height beside it counts as the band floor.
pub fn convert_legacy_splat_tile(
    config: &super::TerrainConfig,
    pixels: &[[u8; 4]],
    heights: &[f32],
) -> Vec<MaterialCell> {
    pixels
        .iter()
        .enumerate()
        .map(|(i, &pixel)| {
            let world_height = config.world_height(heights.get(i).copied().unwrap_or(0.0));
            legacy_splat_to_material_cell(pixel, legacy_bucket3_is_snow(config, world_height))
        })
        .collect()
}

/// Write one chunk's material cells, `chunk_resolution²` row-major, into
/// `TerrainData.material_cache` with the centred offset math of
/// [`write_chunk_to_cache`] (chunk `(cx, cz)` at cache offset
/// `(chunk_pos + half) * resolution`), so the material map lines up 1:1
/// with the heightfield. Allocates an all-Grass layer first when the
/// terrain has none. A chunk outside the raster, or the part of one past
/// its edge, is skipped rather than wrapped.
pub fn write_material_tile_to_cache(
    data: &mut super::TerrainData,
    config: &super::TerrainConfig,
    chunk_pos: bevy::math::IVec2,
    cells: &[MaterialCell],
) {
    super::height_query::ensure_material_cache(data);
    let resolution = config.chunk_resolution as usize;
    let cache_width = data.cache_width as usize;
    let cache_height = data.cache_height as usize;
    let (Ok(grid_x), Ok(grid_z)) = (
        usize::try_from(chunk_pos.x + config.chunks_x as i32),
        usize::try_from(chunk_pos.y + config.chunks_z as i32),
    ) else {
        return;
    };
    let (offset_x, offset_z) = (grid_x * resolution, grid_z * resolution);
    let width = resolution.min(cache_width.saturating_sub(offset_x));
    for row in 0..resolution {
        let z = offset_z + row;
        let src = row * resolution;
        if width == 0 || z >= cache_height || src + width > cells.len() {
            break;
        }
        let dst = z * cache_width + offset_x;
        data.material_cache[dst..dst + width].copy_from_slice(&cells[src..src + width]);
    }
    data.material_dirty = true;
}

/// Save all dirty chunks from the height cache to individual R16 files:
/// [`stage_chunks_to_disk`] then [`commit_staged_chunks`].
pub fn save_chunks_to_disk(
    terrain_dir: &Path,
    config: &super::TerrainConfig,
    data: &super::TerrainData,
    dirty_chunks: &[bevy::math::IVec2],
) -> Result<usize, String> {
    commit_staged_chunks(&stage_chunks_to_disk(terrain_dir, config, data, dirty_chunks)?)
}

/// Write the R16 heightmap of each of `chunks` from the height cache beside
/// its file, as `x{cx}_z{cz}.r16.tmp`, leaving the files themselves alone
/// until [`commit_staged_chunks`] moves them into place. A caller moving the
/// height band stages every chunk in the new band, commits `_terrain.toml`,
/// and only then commits the chunks, so the R16 files never sit in another
/// band than the toml says. Staged files end in `.tmp`, so a leftover is
/// never read as a chunk. On an error every file staged so far is removed.
/// Returns the `(staged, destination)` pairs.
pub fn stage_chunks_to_disk(
    terrain_dir: &Path,
    config: &super::TerrainConfig,
    data: &super::TerrainData,
    chunks: &[bevy::math::IVec2],
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let resolution = config.chunk_resolution as usize;
    let cache_width = data.cache_width as usize;
    let half_x = config.chunks_x as i32;
    let half_z = config.chunks_z as i32;
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(chunks.len());

    for chunk_pos in chunks {
        let offset_x = ((chunk_pos.x + half_x) as usize) * resolution;
        let offset_z = ((chunk_pos.y + half_z) as usize) * resolution;

        // Extract chunk heights from global cache
        let mut heights = Vec::with_capacity(resolution * resolution);
        for row in 0..resolution {
            let start = (offset_z + row) * cache_width + offset_x;
            if start + resolution <= data.height_cache.len() {
                heights.extend_from_slice(&data.height_cache[start..start + resolution]);
            } else {
                // Pad with zeros if out of bounds
                heights.extend(std::iter::repeat(0.0f32).take(resolution));
            }
        }

        let destination = chunk_r16_path(terrain_dir, chunk_pos.x, chunk_pos.y);
        let temporary = destination.with_extension("r16.tmp");
        if let Err(error) = save_chunk_r16(&temporary, &heights, config.chunk_resolution) {
            let _ = std::fs::remove_file(&temporary);
            discard_staged_chunks(&staged);
            return Err(error);
        }
        staged.push((temporary, destination));
    }

    Ok(staged)
}

/// Move chunks staged by [`stage_chunks_to_disk`] into place. On a failed
/// move the chunks not moved yet are removed, and the error returned.
/// Returns the number of chunks moved.
pub fn commit_staged_chunks(staged: &[(PathBuf, PathBuf)]) -> Result<usize, String> {
    for (index, (temporary, destination)) in staged.iter().enumerate() {
        if let Err(error) = std::fs::rename(temporary, destination) {
            discard_staged_chunks(&staged[index..]);
            return Err(format!("Failed to move R16 file into place at {:?}: {}", destination, error));
        }
    }
    Ok(staged.len())
}

/// Remove chunks staged by [`stage_chunks_to_disk`] that will not be
/// committed.
pub fn discard_staged_chunks(staged: &[(PathBuf, PathBuf)]) {
    for (temporary, _) in staged {
        let _ = std::fs::remove_file(temporary);
    }
}

/// Save the material layer of `chunks` to `matmap/x{cx}_z{cz}.png`, the
/// exact inverse of the loader's matmap read plus
/// [`write_material_tile_to_cache`]: same centred tile offsets, rows along
/// Z, columns along X, one RGBA8 pixel per cell holding the cell's four
/// bytes as they are, so a reload is bit-exact.
///
/// After every matmap is written, the legacy `splatmap/` PNG of each saved
/// chunk is removed (and the directory, once empty), so a chunk never has
/// two material files that could disagree. A legacy file that will not go
/// is only warned about: the matmap beside it takes precedence on load.
///
/// Returns the number of PNGs written, 0 when the terrain has no material
/// layer, which leaves any material files on disk alone. Every chunk is
/// checked against the cache before any file is touched, so a bad
/// coordinate refuses the whole save instead of leaving the directory half
/// rewritten.
#[cfg(feature = "image")]
pub fn save_material_chunks_to_disk(
    terrain_dir: &Path,
    config: &super::TerrainConfig,
    data: &super::TerrainData,
    chunks: &[bevy::math::IVec2],
) -> Result<usize, String> {
    if data.material_cache.is_empty() {
        return Ok(0);
    }
    let resolution = config.chunk_resolution as usize;
    let cache_width = data.cache_width as usize;
    let cache_height = data.cache_height as usize;
    if resolution == 0 || !data.has_material_layer() {
        return Err(format!(
            "material map holds {} cells, expected {} ({}x{})",
            data.material_cache.len(),
            cache_width * cache_height,
            cache_width,
            cache_height
        ));
    }

    let half_x = config.chunks_x as i32;
    let half_z = config.chunks_z as i32;
    let mut tiles = Vec::with_capacity(chunks.len());
    for chunk_pos in chunks {
        let grid_x = chunk_pos.x + half_x;
        let grid_z = chunk_pos.y + half_z;
        let inside = grid_x >= 0
            && grid_z >= 0
            && (grid_x as usize + 1) * resolution <= cache_width
            && (grid_z as usize + 1) * resolution <= cache_height;
        if !inside {
            return Err(format!(
                "material chunk x{}_z{} lies outside the {}x{} cell material map",
                chunk_pos.x, chunk_pos.y, cache_width, cache_height
            ));
        }
        tiles.push((*chunk_pos, grid_x as usize * resolution, grid_z as usize * resolution));
    }

    let matmap_dir = terrain_dir.join(MATMAP_DIR);
    std::fs::create_dir_all(&matmap_dir)
        .map_err(|error| format!("Failed to create matmap directory {:?}: {}", matmap_dir, error))?;

    let mut cells: Vec<MaterialCell> = Vec::with_capacity(resolution * resolution);
    let mut saved = 0;
    for (chunk_pos, offset_x, offset_z) in &tiles {
        cells.clear();
        for row in 0..resolution {
            let start = (offset_z + row) * cache_width + offset_x;
            cells.extend_from_slice(&data.material_cache[start..start + resolution]);
        }
        let png = encode_material_tile_png(&cells, config.chunk_resolution)
            .map_err(|error| format!("matmap x{}_z{}: {}", chunk_pos.x, chunk_pos.y, error))?;
        let path = chunk_matmap_path(terrain_dir, chunk_pos.x, chunk_pos.y);
        std::fs::write(&path, &png)
            .map_err(|error| format!("Failed to write matmap {:?}: {}", path, error))?;
        saved += 1;
    }

    remove_legacy_splatmaps(terrain_dir, tiles.iter().map(|(chunk_pos, _, _)| *chunk_pos));
    Ok(saved)
}

/// Without the `image` feature there is no PNG encoder, so a terrain that
/// has a material layer reports that it could not be saved instead of
/// silently dropping the paint.
#[cfg(not(feature = "image"))]
pub fn save_material_chunks_to_disk(
    _terrain_dir: &Path,
    _config: &super::TerrainConfig,
    data: &super::TerrainData,
    _chunks: &[bevy::math::IVec2],
) -> Result<usize, String> {
    if data.material_cache.is_empty() {
        return Ok(0);
    }
    Err("eustress-common was built without the `image` feature, so matmap PNGs cannot be written"
        .to_string())
}

/// Encode one chunk's material cells (`resolution²`, row-major z-then-x) as
/// the matmap PNG every writer emits: RGBA8, each pixel a cell's four bytes
/// unchanged. Shared by Save and the worldgen and flat exporters, so the
/// three cannot drift apart. Requires the `image` feature.
#[cfg(feature = "image")]
pub fn encode_material_tile_png(cells: &[MaterialCell], resolution: u32) -> Result<Vec<u8>, String> {
    let expected = resolution as usize * resolution as usize;
    if cells.len() != expected {
        return Err(format!("{} material cells, expected {} ({resolution}x{resolution})", cells.len(), expected));
    }
    let raw: Vec<u8> = cells.iter().flatten().copied().collect();
    let mut png = Vec::new();
    {
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&raw, resolution, resolution, image::ExtendedColorType::Rgba8)
            .map_err(|error| format!("failed to encode matmap PNG: {error}"))?;
    }
    Ok(png)
}

/// Remove the legacy `splatmap/` PNGs of `chunks`, then the directory if
/// that emptied it. Best effort: a file that will not go is warned about,
/// since the matmap written beside it wins on load anyway.
pub fn remove_legacy_splatmaps(terrain_dir: &Path, chunks: impl IntoIterator<Item = bevy::math::IVec2>) {
    let legacy_dir = terrain_dir.join(LEGACY_SPLATMAP_DIR);
    if !legacy_dir.is_dir() {
        return;
    }
    for chunk_pos in chunks {
        let path = chunk_splatmap_path(terrain_dir, chunk_pos.x, chunk_pos.y);
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("Could not remove legacy splatmap {:?}: {}", path, error);
            }
        }
    }
    // Fails, harmlessly, while files for chunks outside this save remain.
    let _ = std::fs::remove_dir(&legacy_dir);
}

// ============================================================================
// 5. PBR Material TOML Loader (.mat.toml)
// ============================================================================

/// PBR material definition loaded from `.mat.toml` files
///
/// File format:
/// ```toml
/// [material]
/// name = "Red Rock"
/// slot = 23                   # material slot a custom material claims
/// base = "Rock"               # built-in material it starts from
/// tint = [1.0, 0.7, 0.6]      # linear RGB multiplier on the albedo
/// tiling = 6.0                # metres of ground per texture repeat
/// roughness = 0.85
/// metallic = 0.0
/// physics_material = "Granite"
/// texture_set = "granite"     # a bundled set, or files:
/// albedo = "textures/red_rock_albedo.png"
/// normal = "textures/red_rock_normal.png"
/// orm = "textures/red_rock_orm.png"
/// ```
///
/// Every key but `name` is optional, so the files earlier exporters wrote
/// (name, empty texture paths, roughness, `tiling = [8.0, 8.0]`) still
/// parse. How the terrain turns these into a material slot, and which keys
/// override a built-in slot, is `material_slots`' business.
#[derive(Deserialize, Debug, Clone)]
pub struct MaterialTomlFile {
    pub material: MaterialTomlDef,
}

/// Individual material definition
#[derive(Deserialize, Debug, Clone)]
pub struct MaterialTomlDef {
    /// Display name
    pub name: String,
    /// Path to albedo/diffuse texture (relative to material file)
    #[serde(default)]
    pub albedo: String,
    /// Path to normal map texture
    #[serde(default)]
    pub normal: String,
    /// Path to a packed occlusion (R), roughness (G), metallic (B) texture,
    /// the layout of the bundled `*_metallic_roughness.png` maps
    #[serde(default)]
    pub orm: String,
    /// Roughness value (0.0 = mirror, 1.0 = matte). Optional so a file that
    /// leaves it out keeps the value of the material it overrides.
    #[serde(default)]
    pub roughness: Option<f32>,
    /// Metallic value (0.0 = dielectric, 1.0 = metal)
    #[serde(default)]
    pub metallic: Option<f32>,
    /// Path to ambient occlusion texture
    #[serde(default)]
    pub ao: String,
    /// Texture repeat size: metres per repeat, or the legacy per-chunk
    /// repeat pair (see [`MaterialTiling`])
    #[serde(default)]
    pub tiling: Option<MaterialTiling>,
    /// Optional height/displacement map
    #[serde(default)]
    pub height: String,
    /// Optional emissive texture
    #[serde(default)]
    pub emissive: String,
    /// Emissive color multiplier
    #[serde(default)]
    pub emissive_strength: f32,
    /// Material slot this file defines, for a file the `_terrain.toml`
    /// palette does not list (the palette entry's slot wins when it does)
    #[serde(default)]
    pub slot: Option<u8>,
    /// Name of the built-in material a custom slot starts from
    #[serde(default)]
    pub base: Option<String>,
    /// Linear RGB multiplier on the albedo texture
    #[serde(default)]
    pub tint: Option<[f32; 3]>,
    /// Realism material registry name friction comes from ("" for none)
    #[serde(default)]
    pub physics_material: Option<String>,
    /// Name of a bundled texture set ("granite"), or "none" for a flat
    /// colour, instead of albedo/normal/orm files
    #[serde(default)]
    pub texture_set: Option<String>,
}

/// A `.mat.toml` `tiling` value.
///
/// The terrain reads only [`MaterialTiling::Metres`]. The array form is what
/// every file written before material slots existed carries, always the
/// template's `[8.0, 8.0]` repeats per chunk, which no renderer ever read;
/// honouring it would flatten every overridden built-in onto one tiling, so
/// it parses and is otherwise ignored.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(untagged)]
pub enum MaterialTiling {
    /// Metres of ground one texture repeat covers.
    Metres(f32),
    /// Legacy `[u_repeat, v_repeat]` per chunk.
    PerChunk([f32; 2]),
}

/// Load a `.mat.toml` material definition from disk
pub fn load_material_toml(path: &Path) -> Result<MaterialTomlDef, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read material {:?}: {}", path, e))?;
    let file: MaterialTomlFile = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse material {:?}: {}", path, e))?;
    Ok(file.material)
}

/// Load all material palette entries referenced in `_terrain.toml`
///
/// Returns a vec of (slot_index, material_def) pairs.
pub fn load_material_palette(
    terrain_dir: &Path,
    palette: &[TerrainTomlMaterialSlot],
) -> Vec<(usize, MaterialTomlDef)> {
    let mut loaded = Vec::new();
    for entry in palette {
        let mat_path = terrain_dir.join(&entry.file);
        match load_material_toml(&mat_path) {
            Ok(mat) => {
                loaded.push((entry.slot as usize, mat));
            }
            Err(e) => {
                tracing::warn!("Failed to load terrain material slot {}: {}", entry.slot, e);
            }
        }
    }
    loaded
}

/// Write a default `.mat.toml` material file to disk
pub fn write_default_material_toml(path: &Path, name: &str, base_color_hint: [f32; 3]) -> Result<(), String> {
    let content = format!(
        r#"# PBR Material: {name}
# Terrain material definition for material-map painting

[material]
name = "{name}"
albedo = ""
normal = ""
roughness = {roughness:.2}
metallic = 0.0
ao = ""
tiling = [8.0, 8.0]
"#,
        name = name,
        roughness = if base_color_hint[1] > 0.5 { 0.85 } else { 0.7 }, // Greener = rougher (grass)
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create dir {:?}: {}", parent, e))?;
    }
    std::fs::write(path, content)
        .map_err(|e| format!("Failed to write material {:?}: {}", path, e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toml_without_height_offset_loads_at_zero() {
        let parsed: TerrainTomlFile = toml::from_str("[terrain]\nchunk_size = 64.0\n").unwrap();
        assert_eq!(parsed.terrain.height_offset, 0.0);
        assert_eq!(parsed.to_terrain_config().height_offset, 0.0);
    }

    #[test]
    fn a_negative_height_offset_reaches_the_config() {
        let text = "[terrain]\n\
                    chunk_size = 64.0\n\
                    chunk_resolution = 16\n\
                    height_scale = 128.0\n\
                    height_offset = -32.0\n";
        let parsed: TerrainTomlFile = toml::from_str(text).unwrap();
        let config = parsed.to_terrain_config();
        assert_eq!(config.height_offset, -32.0);
        assert_eq!(config.height_scale, 128.0);
        assert_eq!(config.world_height(0.0), -32.0);
        assert_eq!(config.world_height(1.0), 96.0);
    }

    #[test]
    fn the_default_template_parses_with_a_zero_offset() {
        let dir = std::env::temp_dir().join(format!(
            "eustress_terrain_toml_default_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = create_default_terrain_toml(&dir).unwrap();
        let parsed = load_terrain_toml(&path).unwrap();
        assert_eq!(parsed.terrain.height_offset, 0.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rewriting_the_band_keeps_every_other_byte() {
        use crate::terrain::HeightBand;
        let text = "# Terrain\r\n[terrain]\r\nchunk_size = 64.0   # side\r\n\
                    height_scale = 50.0             # Height range above height_offset\r\n\
                    height_offset = 0.0\r\nseed = 42\r\n\r\n[[materials.palette]]\r\nslot = 0\r\n\
                    name = \"Grass\"\r\nfile = \"materials/grass.mat.toml\"\r\n";
        let band = HeightBand { offset: -45.5, scale: 110.25 };
        let (out, read_back) = rewrite_height_band(text, band).unwrap();
        assert_eq!(read_back, band);
        let expected = text
            .replace("height_scale = 50.0 ", "height_scale = 110.25 ")
            .replace("height_offset = 0.0", "height_offset = -45.5");
        assert_eq!(out, expected);
        let config = toml::from_str::<TerrainTomlFile>(&out).unwrap().to_terrain_config();
        assert_eq!((config.height_offset, config.height_scale), (-45.5, 110.25));
    }

    #[test]
    fn a_missing_height_offset_is_added_to_the_terrain_table() {
        use crate::terrain::HeightBand;
        let text = "[terrain]\nchunk_size = 64.0\nheight_scale = 50.0\n\n[streaming]\nview_distance = 256.0\n";
        let band = HeightBand { offset: -12.0, scale: 80.0 };
        let (out, read_back) = rewrite_height_band(text, band).unwrap();
        assert_eq!(read_back, band);
        assert_eq!(
            out,
            "[terrain]\nchunk_size = 64.0\nheight_scale = 80.0\nheight_offset = -12.0\n\n[streaming]\nview_distance = 256.0\n"
        );
    }

    #[test]
    fn a_file_without_a_terrain_table_is_not_rewritten() {
        use crate::terrain::HeightBand;
        let band = HeightBand { offset: 0.0, scale: 10.0 };
        assert!(rewrite_height_band("[streaming]\nview_distance = 256.0\n", band).is_err());
        assert!(rewrite_height_band("[terrain]\nchunk_size = 64.0\n", HeightBand { offset: f32::NAN, scale: 1.0 }).is_err());
    }

    #[test]
    fn the_band_written_to_disk_is_the_band_the_loader_reads() {
        use crate::terrain::HeightBand;
        let dir = std::env::temp_dir().join(format!("eustress_terrain_toml_band_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let band = HeightBand { offset: -271.2, scale: 422.4 };
        assert_eq!(write_height_band_to_toml(&dir, band).unwrap(), None, "no toml, nothing written");
        assert!(!dir.join("_terrain.toml").exists());

        let path = create_default_terrain_toml(&dir).unwrap();
        let read_back = write_height_band_to_toml(&dir, band).unwrap().expect("the toml exists now");
        let config = load_terrain_toml(&path).unwrap().to_terrain_config();
        assert_eq!(HeightBand::of(&config), read_back);
        assert!(!dir.join("_terrain.toml.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_legacy_material_file_parses_with_every_new_key_unset() {
        // What the worldgen exporter and `write_default_material_toml` write.
        let text = "[material]\nname = \"Grass\"\nalbedo = \"\"\nnormal = \"\"\n\
                    roughness = 0.85\nmetallic = 0.0\nao = \"\"\ntiling = [8.0, 8.0]\n";
        let def = toml::from_str::<MaterialTomlFile>(text).unwrap().material;
        assert_eq!(def.name, "Grass");
        assert_eq!(def.roughness, Some(0.85));
        assert_eq!(def.metallic, Some(0.0));
        assert_eq!(def.tiling, Some(MaterialTiling::PerChunk([8.0, 8.0])));
        assert!(def.slot.is_none() && def.base.is_none() && def.tint.is_none());
        assert!(def.physics_material.is_none() && def.texture_set.is_none() && def.orm.is_empty());

        // Only the name is required.
        let bare = toml::from_str::<MaterialTomlFile>("[material]\nname = \"Bare\"\n").unwrap().material;
        assert!(bare.roughness.is_none() && bare.metallic.is_none() && bare.tiling.is_none());
    }

    #[test]
    fn a_custom_material_file_parses_its_slot_keys() {
        let text = "[material]\nname = \"Red Rock\"\nslot = 40\nbase = \"Rock\"\n\
                    tint = [1.0, 0.7, 0.6]\ntiling = 6\nphysics_material = \"Granite\"\n\
                    texture_set = \"granite\"\norm = \"textures/orm.png\"\n";
        let def = toml::from_str::<MaterialTomlFile>(text).unwrap().material;
        assert_eq!(def.slot, Some(40));
        assert_eq!(def.base.as_deref(), Some("Rock"));
        assert_eq!(def.tint, Some([1.0, 0.7, 0.6]));
        // An integer tiling reads as metres.
        assert_eq!(def.tiling, Some(MaterialTiling::Metres(6.0)));
        assert_eq!(def.physics_material.as_deref(), Some("Granite"));
        assert_eq!(def.texture_set.as_deref(), Some("granite"));
        assert_eq!(def.orm, "textures/orm.png");
        // A slot past the u8 range is a parse error, not a wrapped id.
        assert!(toml::from_str::<MaterialTomlFile>("[material]\nname = \"X\"\nslot = 300\n").is_err());
    }

    #[cfg(feature = "image")]
    mod material_map_round_trip {
        use super::super::*;
        use crate::terrain::material::{canonical_material_cell, MATERIAL_SLOT_NONE};
        use crate::terrain::{TerrainConfig, TerrainData};
        use bevy::math::IVec2;

        fn map_config() -> TerrainConfig {
            TerrainConfig {
                chunk_resolution: 8,
                chunks_x: 1,
                chunks_z: 1,
                ..TerrainConfig::default()
            }
        }

        fn every_chunk(config: &TerrainConfig) -> Vec<IVec2> {
            let (hx, hz) = (config.chunks_x as i32, config.chunks_z as i32);
            (-hx..=hx)
                .flat_map(|x| (-hz..=hz).map(move |z| IVec2::new(x, z)))
                .collect()
        }

        /// A known cell per cache cell: pure built-ins, mixes, custom slots,
        /// the "no material" slot and blends that vary per cell, so a
        /// row/column or tile-offset mix-up cannot pass.
        fn painted_cell(x: usize, z: usize) -> MaterialCell {
            match (x + 2 * z) % 5 {
                0 => material_cell(((x + z) % 23) as u8),
                1 => canonical_material_cell((x % 23) as u8, (z % 23) as u8, ((x * 7 + z) % 128) as u8),
                2 => material_cell(23 + ((x * z) % 200) as u8),
                3 => canonical_material_cell(200, (x % 23) as u8, 1 + (z % 100) as u8),
                _ => [MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0],
            }
        }

        fn painted_terrain(config: &TerrainConfig) -> TerrainData {
            let mut data = TerrainData::default();
            data.resize_cache(config);
            let width = data.cache_width as usize;
            let height = data.cache_height as usize;
            for (i, h) in data.height_cache.iter_mut().enumerate() {
                *h = (i % 97) as f32 / 97.0;
            }
            data.material_cache = (0..height)
                .flat_map(|z| (0..width).map(move |x| painted_cell(x, z)))
                .collect();
            data
        }

        fn temp_terrain_dir(tag: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir()
                .join(format!("eustress_terrain_matmap_{}_{}", tag, std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        }

        /// Write a legacy 4-bucket splatmap PNG the way older builds did.
        fn write_legacy_splat(dir: &std::path::Path, chunk: IVec2, pixels: &[[u8; 4]], resolution: u32) {
            use image::ImageEncoder;
            let raw: Vec<u8> = pixels.iter().flatten().copied().collect();
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(&raw, resolution, resolution, image::ExtendedColorType::Rgba8)
                .unwrap();
            let path = chunk_splatmap_path(dir, chunk.x, chunk.y);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, png).unwrap();
        }

        /// The cells of `chunk`'s tile of `data`, row-major.
        fn tile_cells(config: &TerrainConfig, data: &TerrainData, chunk: IVec2) -> Vec<MaterialCell> {
            let res = config.chunk_resolution as usize;
            let w = data.cache_width as usize;
            let x0 = (chunk.x + config.chunks_x as i32) as usize * res;
            let z0 = (chunk.y + config.chunks_z as i32) as usize * res;
            (0..res)
                .flat_map(|row| {
                    let start = (z0 + row) * w + x0;
                    data.material_cache[start..start + res].to_vec()
                })
                .collect()
        }

        #[test]
        fn a_saved_material_map_reloads_bit_exact() {
            let config = map_config();
            let chunks = every_chunk(&config);
            let data = painted_terrain(&config);
            let dir = temp_terrain_dir("round_trip");

            save_chunks_to_disk(&dir, &config, &data, &chunks).unwrap();
            let written = save_material_chunks_to_disk(&dir, &config, &data, &chunks).unwrap();
            assert_eq!(written, chunks.len());
            for chunk in &chunks {
                let img = image::open(chunk_matmap_path(&dir, chunk.x, chunk.y)).unwrap().to_rgba8();
                assert_eq!((img.width(), img.height()), (8, 8));
                let pixels: Vec<MaterialCell> = img.pixels().map(|p| p.0).collect();
                assert_eq!(pixels, tile_cells(&config, &data, *chunk), "chunk {chunk} holds its tile, rows along Z");
            }

            let mut loaded = TerrainData::default();
            let found = load_chunks_from_disk(&dir, &config, &mut loaded);
            assert_eq!(found.len(), chunks.len());
            assert!(loaded.material_dirty);
            assert_eq!(loaded.material_cache, data.material_cache);

            // And it saves back to the same bytes.
            let first: Vec<Vec<u8>> = chunks
                .iter()
                .map(|c| std::fs::read(chunk_matmap_path(&dir, c.x, c.y)).unwrap())
                .collect();
            save_material_chunks_to_disk(&dir, &config, &loaded, &chunks).unwrap();
            for (chunk, before) in chunks.iter().zip(&first) {
                let after = std::fs::read(chunk_matmap_path(&dir, chunk.x, chunk.y)).unwrap();
                assert_eq!(&after, before, "chunk x{}_z{} drifted on re-save", chunk.x, chunk.y);
            }
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn a_terrain_without_a_material_layer_writes_no_pngs() {
            let config = map_config();
            let mut data = TerrainData::default();
            data.resize_cache(&config);
            let dir = temp_terrain_dir("empty");
            let written = save_material_chunks_to_disk(&dir, &config, &data, &every_chunk(&config)).unwrap();
            assert_eq!(written, 0);
            assert!(!dir.join(MATMAP_DIR).exists());
        }

        #[test]
        fn a_chunk_outside_the_grid_refuses_the_whole_save() {
            let config = map_config();
            let data = painted_terrain(&config);
            let dir = temp_terrain_dir("outside");
            let chunks = [IVec2::new(0, 0), IVec2::new(2, 0)];
            assert!(save_material_chunks_to_disk(&dir, &config, &data, &chunks).is_err());
            assert!(!chunk_matmap_path(&dir, 0, 0).exists(), "nothing is written before the check");
        }

        #[test]
        fn a_chunk_with_no_material_file_loads_as_grass() {
            let config = map_config();
            let chunks = every_chunk(&config);
            let data = painted_terrain(&config);
            let dir = temp_terrain_dir("grass");
            save_chunks_to_disk(&dir, &config, &data, &chunks).unwrap();

            let mut loaded = TerrainData::default();
            assert_eq!(load_chunks_from_disk(&dir, &config, &mut loaded).len(), chunks.len());
            assert!(loaded.has_material_layer(), "a loaded terrain always has a material layer");
            let grass = material_cell(TerrainMaterial::Grass.to_u8());
            assert!(loaded.material_cache.iter().all(|cell| *cell == grass));
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn a_terrain_with_no_chunks_yet_loads_as_grass() {
            let config = map_config();
            // Never created, so it has no chunks/ directory.
            let dir = temp_terrain_dir("no_chunks");
            let mut loaded = TerrainData::default();
            assert!(load_chunks_from_disk(&dir, &config, &mut loaded).is_empty());
            assert!(loaded.has_material_layer(), "Save writes matmaps only for a terrain with a material layer");
            let grass = material_cell(TerrainMaterial::Grass.to_u8());
            assert!(loaded.material_cache.iter().all(|cell| *cell == grass));
        }

        #[test]
        fn a_legacy_splatmap_converts_on_load_with_the_snow_line() {
            // Default band: 0 to 50 m, so the snow line sits at normalized 0.72.
            let config = map_config();
            let res = config.chunk_resolution as usize;
            let mut data = TerrainData::default();
            data.resize_cache(&config);
            let w = data.cache_width as usize;
            // Even columns are peaks above the snow line, odd ones lowland.
            for (i, h) in data.height_cache.iter_mut().enumerate() {
                *h = if (i % w) % 2 == 0 { 0.9 } else { 0.1 };
            }
            let chunks = every_chunk(&config);
            let dir = temp_terrain_dir("legacy");
            save_chunks_to_disk(&dir, &config, &data, &chunks).unwrap();

            // Chunk (0, 0) gets a legacy splatmap; the others get none.
            let legacy_pixel = |x: usize, z: usize| -> [u8; 4] {
                match (x + z) % 4 {
                    0 => [0, 0, 0, 255],     // the fourth bucket alone
                    1 => [0, 153, 102, 0],   // rock over dirt
                    2 => [0, 0, 0, 0],       // never painted
                    _ => [60, 10, 5, 180],   // four buckets: snow-or-water over grass
                }
            };
            let pixels: Vec<[u8; 4]> = (0..res).flat_map(|z| (0..res).map(move |x| legacy_pixel(x, z))).collect();
            write_legacy_splat(&dir, IVec2::ZERO, &pixels, config.chunk_resolution);

            let mut loaded = TerrainData::default();
            load_chunks_from_disk(&dir, &config, &mut loaded);
            let cells = tile_cells(&config, &loaded, IVec2::ZERO);
            let (grass, rock, dirt, snow, water) = (
                TerrainMaterial::Grass.to_u8(),
                TerrainMaterial::Rock.to_u8(),
                TerrainMaterial::Dirt.to_u8(),
                TerrainMaterial::Snow.to_u8(),
                TerrainMaterial::Water.to_u8(),
            );
            // Chunk (0, 0) starts at cache column 8, an even one.
            for z in 0..res {
                for x in 0..res {
                    let high = x % 2 == 0;
                    let fourth = if high { snow } else { water };
                    let expected = match (x + z) % 4 {
                        0 => material_cell(fourth),
                        1 => [rock, dirt, 102, 0],
                        2 => material_cell(grass),
                        _ => [fourth, grass, 63, 0],
                    };
                    assert_eq!(cells[z * res + x], expected, "cell ({x}, {z}), high = {high}");
                }
            }
            // The chunks without any material file stay Grass.
            assert!(tile_cells(&config, &loaded, IVec2::new(-1, 1)).iter().all(|c| *c == material_cell(grass)));

            // A matmap beside the legacy file wins.
            let painted = painted_terrain(&config);
            save_material_chunks_to_disk(&dir, &config, &painted, &[IVec2::new(1, -1)]).unwrap();
            write_legacy_splat(&dir, IVec2::new(1, -1), &pixels, config.chunk_resolution);
            let mut preferred = TerrainData::default();
            load_chunks_from_disk(&dir, &config, &mut preferred);
            assert_eq!(
                tile_cells(&config, &preferred, IVec2::new(1, -1)),
                tile_cells(&config, &painted, IVec2::new(1, -1))
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn saving_replaces_the_legacy_splatmaps_it_covers() {
            let config = map_config();
            let res = config.chunk_resolution as usize;
            let chunks = every_chunk(&config);
            let data = painted_terrain(&config);
            let dir = temp_terrain_dir("replace_legacy");
            save_chunks_to_disk(&dir, &config, &data, &chunks).unwrap();
            let pixels = vec![[0u8, 255, 0, 0]; res * res];
            for chunk in &chunks {
                write_legacy_splat(&dir, *chunk, &pixels, config.chunk_resolution);
            }

            // A save of part of the grid removes only that part's legacy files.
            save_material_chunks_to_disk(&dir, &config, &data, &chunks[..4]).unwrap();
            for (i, chunk) in chunks.iter().enumerate() {
                assert_eq!(chunk_splatmap_path(&dir, chunk.x, chunk.y).exists(), i >= 4, "chunk {chunk}");
            }
            assert!(dir.join(LEGACY_SPLATMAP_DIR).is_dir());

            // The full save leaves one source of truth: no splatmap directory.
            save_material_chunks_to_disk(&dir, &config, &data, &chunks).unwrap();
            assert!(!dir.join(LEGACY_SPLATMAP_DIR).exists());
            let mut loaded = TerrainData::default();
            load_chunks_from_disk(&dir, &config, &mut loaded);
            assert_eq!(loaded.material_cache, data.material_cache);
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn legacy_splat_tiles_split_the_fourth_bucket_by_cell_height() {
        use crate::terrain::TerrainConfig;
        let config = TerrainConfig { height_offset: -20.0, height_scale: 100.0, ..TerrainConfig::default() };
        let pixels = [[0u8, 0, 0, 255]; 4];
        // Normalized 0.72 is the snow line, Y = 52: two cells just either
        // side of it, one well above and one on the floor.
        let heights = [0.9, 0.725, 0.715, 0.0];
        let cells = convert_legacy_splat_tile(&config, &pixels, &heights);
        let (snow, water) = (TerrainMaterial::Snow.to_u8(), TerrainMaterial::Water.to_u8());
        assert_eq!(cells, vec![material_cell(snow), material_cell(snow), material_cell(water), material_cell(water)]);
        // A pixel with no height beside it is on the band floor.
        assert_eq!(convert_legacy_splat_tile(&config, &pixels, &[]), vec![material_cell(water); 4]);
    }

    #[test]
    fn material_tiles_land_at_their_chunk_and_clip_at_the_edge() {
        use crate::terrain::{TerrainConfig, TerrainData};
        use bevy::math::IVec2;
        let config = TerrainConfig { chunk_resolution: 4, chunks_x: 1, chunks_z: 1, ..TerrainConfig::default() };
        let mut data = TerrainData::default();
        data.resize_cache(&config);
        let rock = material_cell(TerrainMaterial::Rock.to_u8());
        write_material_tile_to_cache(&mut data, &config, IVec2::new(1, -1), &vec![rock; 16]);
        assert!(data.has_material_layer() && data.material_dirty);
        let w = data.cache_width as usize;
        for (i, cell) in data.material_cache.iter().enumerate() {
            let (x, z) = (i % w, i / w);
            let inside = (8..12).contains(&x) && (0..4).contains(&z);
            assert_eq!(*cell == rock, inside, "cell ({x}, {z})");
        }
        // A chunk left of the raster, or past its edge, writes nothing.
        let before = data.material_cache.clone();
        write_material_tile_to_cache(&mut data, &config, IVec2::new(-2, 0), &vec![rock; 16]);
        write_material_tile_to_cache(&mut data, &config, IVec2::new(2, 0), &vec![rock; 16]);
        assert_eq!(data.material_cache, before);
    }
}
