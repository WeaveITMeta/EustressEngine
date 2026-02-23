//! # ECS Components for Geospatial Features
//!
//! Bevy components that tag entities with geospatial metadata.
//! These are attached to spawned meshes so systems can query
//! geographic context at runtime.
//!
//! ## Table of Contents
//! 1. GeoLayer — Layer grouping component
//! 2. GeoFeature — Individual feature metadata
//! 3. GeoTerrainChunk — Terrain chunk with LOD

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. GeoLayer — Layer grouping component
// ============================================================================

/// Groups geospatial entities into named layers for visibility toggling
#[derive(Component, Reflect, Debug, Clone, Serialize, Deserialize)]
#[reflect(Component)]
pub struct GeoLayer {
    /// Layer name (matches VectorLayerConfig.name)
    pub name: String,
    /// Whether this layer is currently visible
    pub visible: bool,
    /// Layer opacity (0.0–1.0)
    pub opacity: f32,
}

impl Default for GeoLayer {
    fn default() -> Self {
        Self {
            name: "Unnamed Layer".to_string(),
            visible: true,
            opacity: 1.0,
        }
    }
}

// ============================================================================
// 2. GeoFeature — Individual feature metadata
// ============================================================================

/// Marks an entity as a geospatial feature with source coordinates
#[derive(Component, Reflect, Debug, Clone, Serialize, Deserialize)]
#[reflect(Component)]
pub struct GeoFeature {
    /// Original latitude (WGS84 degrees)
    pub lat: f64,
    /// Original longitude (WGS84 degrees)
    pub lon: f64,
    /// Original elevation (meters, 0.0 if unknown)
    pub elevation: f64,
    /// Source file path (relative to geo.toml)
    pub source: String,
    /// Feature ID within source file
    pub feature_id: Option<u64>,
    /// Feature properties as JSON string (for inspector display)
    #[reflect(ignore)]
    pub properties_json: Option<String>,
}

impl Default for GeoFeature {
    fn default() -> Self {
        Self {
            lat: 0.0,
            lon: 0.0,
            elevation: 0.0,
            source: String::new(),
            feature_id: None,
            properties_json: None,
        }
    }
}

// ============================================================================
// 3. GeoTerrainChunk — Terrain chunk with LOD
// ============================================================================

/// Marks an entity as a geospatial terrain chunk
#[derive(Component, Reflect, Debug, Clone, Serialize, Deserialize)]
#[reflect(Component)]
pub struct GeoTerrainChunk {
    /// Grid X coordinate
    pub grid_x: i32,
    /// Grid Z coordinate
    pub grid_z: i32,
    /// Current LOD level (0 = highest detail)
    pub lod: u32,
    /// Path to .glb mesh (file-system-first)
    pub mesh_path: String,
}
