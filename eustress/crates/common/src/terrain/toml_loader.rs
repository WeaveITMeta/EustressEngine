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
//!   splatmap/x{N}_z{N}.png   ← Per-chunk material blend weights
//!   materials/*.mat.toml      ← PBR material definitions
//! ```

use serde::Deserialize;
use std::path::{Path, PathBuf};

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
    /// Height multiplier (default: 50.0)
    #[serde(default = "default_height_scale")]
    pub height_scale: f32,
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
    /// Slot index (0-7, maps to splatmap channels)
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

/// Build the filesystem path for a chunk's splatmap file
pub fn chunk_splatmap_path(terrain_dir: &Path, chunk_x: i32, chunk_z: i32) -> PathBuf {
    terrain_dir.join("splatmap").join(format!("x{}_z{}.png", chunk_x, chunk_z))
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
    std::fs::create_dir_all(terrain_dir.join("splatmap"))
        .map_err(|error| format!("Failed to create splatmap dir: {}", error))?;
    std::fs::create_dir_all(terrain_dir.join("materials"))
        .map_err(|error| format!("Failed to create materials dir: {}", error))?;
    
    let toml_path = terrain_dir.join("_terrain.toml");
    let content = r#"# Eustress Engine — Terrain Configuration
# This file defines the terrain for the current Space.
# Per-chunk heightmaps are stored in chunks/ as .r16 files (16-bit, git-friendly).

[terrain]
chunk_size = 64.0               # World units per chunk side
chunk_resolution = 64           # Vertices per chunk side
height_scale = 50.0             # Height multiplier (max height = height_scale)
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
# Material palette — slot index maps to splatmap channels
# Splatmap RGBA = slots 0-3

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
name = "Sand"
file = "materials/sand.mat.toml"

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
/// height cache at the correct offsets. Returns a list of chunk coordinates found.
pub fn load_chunks_from_disk(
    terrain_dir: &Path,
    config: &super::TerrainConfig,
    data: &mut super::TerrainData,
) -> Vec<bevy::math::IVec2> {
    use bevy::math::IVec2;
    
    let chunks_dir = terrain_dir.join("chunks");
    if !chunks_dir.exists() {
        return Vec::new();
    }
    
    // Ensure height cache is sized
    if data.height_cache.is_empty() {
        data.resize_cache(config);
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
                loaded_chunks.push(IVec2::new(chunk_x, chunk_z));
            }
            Err(error) => {
                tracing::warn!("Skipping chunk x{}_z{}: {}", chunk_x, chunk_z, error);
            }
        }
    }
    
    loaded_chunks
}

/// Write a chunk's height values into the global `TerrainData.height_cache`
fn write_chunk_to_cache(
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

/// Save all dirty chunks from the height cache to individual R16 files
pub fn save_chunks_to_disk(
    terrain_dir: &Path,
    config: &super::TerrainConfig,
    data: &super::TerrainData,
    dirty_chunks: &[bevy::math::IVec2],
) -> Result<usize, String> {
    let mut saved = 0;
    let resolution = config.chunk_resolution as usize;
    let cache_width = data.cache_width as usize;
    let half_x = config.chunks_x as i32;
    let half_z = config.chunks_z as i32;
    
    for chunk_pos in dirty_chunks {
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
        
        let path = chunk_r16_path(terrain_dir, chunk_pos.x, chunk_pos.y);
        save_chunk_r16(&path, &heights, config.chunk_resolution)?;
        saved += 1;
    }
    
    Ok(saved)
}

// ============================================================================
// 5. PBR Material TOML Loader (.mat.toml)
// ============================================================================

/// PBR material definition loaded from `.mat.toml` files
///
/// File format:
/// ```toml
/// [material]
/// name = "Grass"
/// albedo = "textures/grass_albedo.png"
/// normal = "textures/grass_normal.png"
/// roughness = 0.85
/// metallic = 0.0
/// ao = "textures/grass_ao.png"
/// tiling = [8.0, 8.0]
/// ```
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
    /// Roughness value (0.0 = mirror, 1.0 = matte)
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    /// Metallic value (0.0 = dielectric, 1.0 = metal)
    #[serde(default)]
    pub metallic: f32,
    /// Path to ambient occlusion texture
    #[serde(default)]
    pub ao: String,
    /// UV tiling per chunk [u_repeat, v_repeat]
    #[serde(default = "default_tiling")]
    pub tiling: [f32; 2],
    /// Optional height/displacement map
    #[serde(default)]
    pub height: String,
    /// Optional emissive texture
    #[serde(default)]
    pub emissive: String,
    /// Emissive color multiplier
    #[serde(default)]
    pub emissive_strength: f32,
}

fn default_roughness() -> f32 { 0.8 }
fn default_tiling() -> [f32; 2] { [8.0, 8.0] }

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
# Terrain material definition for splatmap-based painting

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
