//! # Eustress Geo — File-System-First Geospatial for 3D Map Visualization
//!
//! Replaces PostGIS with local file-based geospatial data that loads directly
//! into Bevy's ECS. No database server, no Docker, no connection strings.
//!
//! ## Architecture
//! - `geo.toml` — Declarative project config (origin, CRS, layers)
//! - `assets/geo/` — Geospatial data files (GeoJSON, GeoTIFF, HGT, FlatGeobuf)
//! - `.eustress/cache/geo/` — Derived meshes and spatial indices
//!
//! ## Modules
//! - `config` — Parse `geo.toml` project configuration
//! - `coords` — Coordinate transforms (WGS84 → UTM → Bevy local)
//! - `layers` — ECS components for geospatial features and layers
//! - `vector_import` — GeoJSON/FlatGeobuf → typed geometry collections
//! - `vector_render` — Geometry → Bevy 3D meshes (tubes, markers, polygons)
//! - `spatial_index` — R-tree wrapper for runtime spatial queries
//! - `plugin` — Bevy plugin registration and systems
//!
//! ## Table of Contents
//! 1. Module declarations
//! 2. Re-exports
//! 3. Plugin registration

pub mod config;
pub mod coords;
pub mod layers;
pub mod vector_import;
pub mod vector_render;
pub mod spatial_index;
pub mod plugin;

pub use config::GeoConfig;
pub use coords::{geo_to_world, GeoOrigin};
pub use layers::{GeoFeature, GeoLayer, GeoTerrainChunk};
pub use plugin::GeoPlugin;
pub use spatial_index::GeoSpatialIndex;
