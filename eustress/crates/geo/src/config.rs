//! # Geospatial Project Configuration
//!
//! Parses `geo.toml` — the declarative config for geospatial layers in an
//! Eustress project. Lives at `assets/geo/geo.toml` in the project directory.
//!
//! ## Table of Contents
//! 1. GeoConfig — Top-level project config
//! 2. TerrainSourceConfig — Elevation data sources
//! 3. VectorLayerConfig — Vector feature layers
//! 4. RasterLayerConfig — Imagery/raster layers
//! 5. StyleConfig — Visual styling for layers
//! 6. Parsing

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ============================================================================
// 1. GeoConfig — Top-level project config
// ============================================================================

/// Top-level geospatial project configuration, parsed from `geo.toml`
#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct GeoConfig {
    /// Project metadata
    pub project: GeoProjectConfig,
    /// Terrain elevation sources
    #[serde(default)]
    pub terrain: TerrainConfig,
    /// Vector and raster layers
    #[serde(default)]
    pub layers: LayersConfig,
}

/// Project-level metadata and coordinate origin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoProjectConfig {
    /// Human-readable project name
    pub name: String,
    /// Origin latitude (WGS84 degrees) — all local coords relative to this
    pub origin_lat: f64,
    /// Origin longitude (WGS84 degrees)
    pub origin_lon: f64,
    /// Human-readable name for the origin point
    #[serde(default)]
    pub origin_name: String,
    /// Target CRS for projection (e.g., "EPSG:32644" for UTM 44N)
    #[serde(default = "default_crs")]
    pub target_crs: String,
}

fn default_crs() -> String {
    "EPSG:4326".to_string()
}

// ============================================================================
// 2. TerrainSourceConfig — Elevation data sources
// ============================================================================

/// Terrain configuration section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TerrainConfig {
    /// Elevation data sources (processed in order; later overrides earlier)
    #[serde(default)]
    pub sources: Vec<TerrainSourceConfig>,
    /// Height scale multiplier (1.0 = real-world meters)
    #[serde(default = "default_one")]
    pub height_scale: f32,
    /// Vertical exaggeration for visibility (1.0 = no exaggeration)
    #[serde(default = "default_one")]
    pub vertical_exaggeration: f32,
    /// Chunk size in world units (meters)
    #[serde(default = "default_chunk_size")]
    pub chunk_size: f32,
    /// Vertices per chunk side
    #[serde(default = "default_chunk_resolution")]
    pub chunk_resolution: u32,
}

/// A single elevation data source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainSourceConfig {
    /// Relative path from geo.toml to the elevation file
    pub path: String,
    /// Format hint (auto-detected from extension if omitted)
    #[serde(default)]
    pub format: Option<String>,
}

fn default_one() -> f32 { 1.0 }
fn default_chunk_size() -> f32 { 1000.0 }
fn default_chunk_resolution() -> u32 { 128 }

// ============================================================================
// 3. VectorLayerConfig — Vector feature layers
// ============================================================================

/// Container for all layer definitions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LayersConfig {
    /// Vector layers (points, lines, polygons)
    #[serde(default)]
    pub vector: Vec<VectorLayerConfig>,
    /// Raster layers (imagery, heatmaps)
    #[serde(default)]
    pub raster: Vec<RasterLayerConfig>,
}

/// A vector feature layer (GeoJSON, FlatGeobuf, GeoPackage)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorLayerConfig {
    /// Human-readable layer name
    pub name: String,
    /// Relative path from geo.toml to the vector file
    pub path: String,
    /// GeoPackage layer name (only for .gpkg files)
    #[serde(default)]
    pub layer: Option<String>,
    /// Visual style
    #[serde(default)]
    pub style: VectorStyleConfig,
}

// ============================================================================
// 4. RasterLayerConfig — Imagery/raster layers
// ============================================================================

/// A raster layer (satellite imagery, heatmaps)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterLayerConfig {
    /// Human-readable layer name
    pub name: String,
    /// Relative path from geo.toml to the raster file
    pub path: String,
    /// Whether to drape onto terrain surface
    #[serde(default)]
    pub drape: bool,
}

// ============================================================================
// 5. StyleConfig — Visual styling for layers
// ============================================================================

/// Visual style for vector features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorStyleConfig {
    /// RGBA color [r, g, b] or [r, g, b, a] in 0.0–1.0 range
    #[serde(default = "default_color")]
    pub color: Vec<f32>,
    /// Line width in world units (for LineString features)
    #[serde(default = "default_width")]
    pub width: f32,
    /// Whether to extrude geometry vertically
    #[serde(default)]
    pub extrude: bool,
    /// Extrusion height in world units
    #[serde(default)]
    pub height: f32,
    /// Marker shape for Point features ("sphere", "cylinder", "cube")
    #[serde(default)]
    pub marker: Option<String>,
    /// Marker radius in world units
    #[serde(default = "default_radius")]
    pub radius: f32,
}

impl Default for VectorStyleConfig {
    fn default() -> Self {
        Self {
            color: default_color(),
            width: default_width(),
            extrude: false,
            height: 0.0,
            marker: None,
            radius: default_radius(),
        }
    }
}

fn default_color() -> Vec<f32> { vec![0.3, 0.6, 1.0, 1.0] }
fn default_width() -> f32 { 10.0 }
fn default_radius() -> f32 { 50.0 }

// ============================================================================
// 6. Parsing
// ============================================================================

impl GeoConfig {
    /// Load a GeoConfig from a `geo.toml` file path
    pub fn load(path: &Path) -> Result<Self, GeoConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| GeoConfigError::Io(path.to_path_buf(), e))?;
        let config: GeoConfig = toml::from_str(&content)
            .map_err(|e| GeoConfigError::Parse(path.to_path_buf(), e))?;
        Ok(config)
    }

    /// Resolve a relative path from geo.toml to an absolute path
    pub fn resolve_path(&self, geo_toml_dir: &Path, relative: &str) -> PathBuf {
        geo_toml_dir.join(relative)
    }

    /// Extract RGBA Color from a style color vec
    pub fn color_from_vec(color: &[f32]) -> Color {
        match color.len() {
            3 => Color::srgba(color[0], color[1], color[2], 1.0),
            4 => Color::srgba(color[0], color[1], color[2], color[3]),
            _ => Color::srgba(0.5, 0.5, 0.5, 1.0),
        }
    }
}

/// Errors from loading geo.toml
#[derive(Debug)]
pub enum GeoConfigError {
    /// File I/O error
    Io(PathBuf, std::io::Error),
    /// TOML parse error
    Parse(PathBuf, toml::de::Error),
}

impl std::fmt::Display for GeoConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeoConfigError::Io(path, e) => write!(f, "Failed to read {}: {}", path.display(), e),
            GeoConfigError::Parse(path, e) => write!(f, "Failed to parse {}: {}", path.display(), e),
        }
    }
}

impl std::error::Error for GeoConfigError {}
