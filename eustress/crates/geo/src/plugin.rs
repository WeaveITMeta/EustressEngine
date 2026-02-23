//! # Bevy Plugin for Geospatial Data
//!
//! Registers ECS components, resources, and systems for loading and rendering
//! geospatial data from `geo.toml` and associated files.
//!
//! ## Table of Contents
//! 1. GeoPlugin — Main plugin
//! 2. Systems: load_geo_config, spawn_vector_layers, update_layer_visibility

use bevy::prelude::*;
use std::path::PathBuf;

use crate::config::{GeoConfig, VectorLayerConfig};
use crate::coords::GeoOrigin;
use crate::layers::{GeoFeature, GeoLayer, GeoTerrainChunk};
use crate::spatial_index::{GeoSpatialIndex, IndexedFeature};
use crate::terrain_import;
use crate::vector_import::{import_geojson, LocalFeature, LocalGeometry};
use crate::vector_render::{
    generate_flat_polygon_mesh, generate_marker_mesh, generate_ribbon_mesh, generate_tube_mesh,
    MarkerShape,
};

// ============================================================================
// 1. GeoPlugin — Main plugin
// ============================================================================

/// Main Bevy plugin for file-system-first geospatial data.
///
/// Add to your App to enable geospatial layer loading and rendering.
/// Requires a `GeoConfig` resource to be inserted (or loaded via `load_geo_config`).
pub struct GeoPlugin;

impl Plugin for GeoPlugin {
    fn build(&self, app: &mut App) {
        app
            // Register types for reflection/serialization
            .register_type::<GeoLayer>()
            .register_type::<GeoFeature>()
            .register_type::<GeoTerrainChunk>()
            // Init resources
            .init_resource::<GeoSpatialIndex>()
            .init_resource::<GeoLoadState>()
            // Systems
            .add_systems(Update, (
                spawn_vector_layers.run_if(resource_exists::<GeoConfig>.and(not(geo_layers_loaded))),
                spawn_terrain_from_config.run_if(resource_exists::<GeoConfig>.and(not(geo_terrain_loaded))),
                update_layer_visibility,
            ));
    }
}

/// Tracks whether geo layers have been spawned
#[derive(Resource, Default)]
pub struct GeoLoadState {
    /// Whether vector layers have been loaded and spawned
    pub layers_loaded: bool,
    /// Whether terrain tiles have been loaded and spawned
    pub terrain_loaded: bool,
    /// Path to the geo.toml directory (for resolving relative paths)
    pub geo_dir: Option<PathBuf>,
}

/// Run condition: layers not yet loaded
fn geo_layers_loaded(state: Res<GeoLoadState>) -> bool {
    state.layers_loaded
}

// ============================================================================
// 2. Systems
// ============================================================================

/// Load geo.toml from a path and insert GeoConfig + GeoOrigin resources.
///
/// Call this manually or use it as a startup system with a known path.
pub fn load_geo_config(path: PathBuf, commands: &mut Commands) -> Result<(), String> {
    let config = GeoConfig::load(&path)
        .map_err(|e| format!("{}", e))?;

    let origin = GeoOrigin::from(&config);
    let geo_dir = path.parent().map(|p| p.to_path_buf());

    commands.insert_resource(config);
    commands.insert_resource(origin);
    commands.insert_resource(GeoLoadState {
        layers_loaded: false,
        terrain_loaded: false,
        geo_dir,
    });

    tracing::info!("Loaded geo config from {}", path.display());
    Ok(())
}

/// System: spawn 3D meshes for all vector layers defined in GeoConfig.
///
/// Runs once when GeoConfig is available and layers haven't been loaded yet.
fn spawn_vector_layers(
    mut commands: Commands,
    config: Res<GeoConfig>,
    origin: Res<GeoOrigin>,
    mut load_state: ResMut<GeoLoadState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spatial_index: ResMut<GeoSpatialIndex>,
) {
    let geo_dir = match &load_state.geo_dir {
        Some(dir) => dir.clone(),
        None => {
            tracing::warn!("GeoLoadState.geo_dir not set — cannot resolve layer paths");
            load_state.layers_loaded = true;
            return;
        }
    };

    let mut all_indexed = Vec::new();

    for layer_config in &config.layers.vector {
        let layer_path = geo_dir.join(&layer_config.path);
        let extension = layer_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let features = match extension.as_str() {
            "geojson" | "json" => {
                match import_geojson(&layer_path, &origin) {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::error!("Failed to import {}: {}", layer_path.display(), e);
                        continue;
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "Unsupported vector format '{}' for layer '{}' — only .geojson supported in Phase 1",
                    extension, layer_config.name
                );
                continue;
            }
        };

        // Spawn each feature as a Bevy entity with mesh
        let color = GeoConfig::color_from_vec(&layer_config.style.color);
        let material = materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            alpha_mode: if layer_config.style.color.len() > 3 && layer_config.style.color[3] < 1.0 {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            ..default()
        });

        for feature in &features {
            let entity = spawn_feature_mesh(
                &mut commands,
                &mut meshes,
                &material,
                feature,
                layer_config,
            );

            if let Some(entity) = entity {
                // Add to spatial index
                let indexed = match &feature.geometry {
                    LocalGeometry::Point(pos) => {
                        IndexedFeature::from_point(entity, &layer_config.name, feature.name.clone(), *pos)
                    }
                    LocalGeometry::LineString(verts) => {
                        IndexedFeature::from_vertices(entity, &layer_config.name, feature.name.clone(), verts)
                    }
                    LocalGeometry::Polygon { outer, .. } => {
                        IndexedFeature::from_vertices(entity, &layer_config.name, feature.name.clone(), outer)
                    }
                    _ => {
                        IndexedFeature::from_point(
                            entity,
                            &layer_config.name,
                            feature.name.clone(),
                            Vec3::ZERO,
                        )
                    }
                };
                all_indexed.push(indexed);
            }
        }

        tracing::info!(
            "Spawned {} features for layer '{}'",
            features.len(),
            layer_config.name
        );
    }

    // Bulk-load spatial index
    if !all_indexed.is_empty() {
        *spatial_index = GeoSpatialIndex::bulk_load(all_indexed);
        tracing::info!("Spatial index built with {} features", spatial_index.len());
    }

    load_state.layers_loaded = true;
}

/// Spawn a single feature as a Bevy entity with the appropriate mesh
fn spawn_feature_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    feature: &LocalFeature,
    layer_config: &VectorLayerConfig,
) -> Option<Entity> {
    let mesh_handle = match &feature.geometry {
        LocalGeometry::Point(pos) => {
            let shape = layer_config.style.marker.as_deref()
                .map(MarkerShape::from_str)
                .unwrap_or(MarkerShape::Sphere);
            let mesh = generate_marker_mesh(shape, layer_config.style.radius);
            let handle = meshes.add(mesh);

            let entity = commands.spawn((
                Mesh3d(handle),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(*pos),
                GeoLayer {
                    name: layer_config.name.clone(),
                    visible: true,
                    opacity: 1.0,
                },
                GeoFeature {
                    lat: feature.centroid_lat,
                    lon: feature.centroid_lon,
                    elevation: pos.y as f64,
                    source: layer_config.path.clone(),
                    feature_id: Some(feature.index as u64),
                    properties_json: feature.properties_json.clone(),
                },
            )).id();

            return Some(entity);
        }
        LocalGeometry::LineString(verts) => {
            if layer_config.style.extrude {
                let mesh = generate_tube_mesh(verts, layer_config.style.width * 0.5, 8);
                meshes.add(mesh)
            } else {
                let mesh = generate_ribbon_mesh(verts, layer_config.style.width);
                meshes.add(mesh)
            }
        }
        LocalGeometry::MultiLineString(lines) => {
            // Merge all lines into one ribbon (simplified)
            let all_verts: Vec<Vec3> = lines.iter().flat_map(|l| l.iter().copied()).collect();
            if layer_config.style.extrude {
                let mesh = generate_tube_mesh(&all_verts, layer_config.style.width * 0.5, 8);
                meshes.add(mesh)
            } else {
                let mesh = generate_ribbon_mesh(&all_verts, layer_config.style.width);
                meshes.add(mesh)
            }
        }
        LocalGeometry::Polygon { outer, .. } => {
            let mesh = generate_flat_polygon_mesh(outer);
            meshes.add(mesh)
        }
        LocalGeometry::MultiPolygon(polys) => {
            // Use first polygon (simplified)
            if let Some(first) = polys.first() {
                let mesh = generate_flat_polygon_mesh(&first.outer);
                meshes.add(mesh)
            } else {
                return None;
            }
        }
        LocalGeometry::MultiPoint(points) => {
            // Spawn each point as a separate marker
            let shape = layer_config.style.marker.as_deref()
                .map(MarkerShape::from_str)
                .unwrap_or(MarkerShape::Sphere);
            let mesh = generate_marker_mesh(shape, layer_config.style.radius);
            let handle = meshes.add(mesh);

            for (i, pos) in points.iter().enumerate() {
                commands.spawn((
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(*pos),
                    GeoLayer {
                        name: layer_config.name.clone(),
                        visible: true,
                        opacity: 1.0,
                    },
                    GeoFeature {
                        lat: feature.centroid_lat,
                        lon: feature.centroid_lon,
                        elevation: pos.y as f64,
                        source: layer_config.path.clone(),
                        feature_id: Some((feature.index * 1000 + i) as u64),
                        properties_json: feature.properties_json.clone(),
                    },
                ));
            }
            return None; // Already spawned individually
        }
    };

    let entity = commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(material.clone()),
        Transform::IDENTITY,
        GeoLayer {
            name: layer_config.name.clone(),
            visible: true,
            opacity: 1.0,
        },
        GeoFeature {
            lat: feature.centroid_lat,
            lon: feature.centroid_lon,
            elevation: 0.0,
            source: layer_config.path.clone(),
            feature_id: Some(feature.index as u64),
            properties_json: feature.properties_json.clone(),
        },
    )).id();

    Some(entity)
}

/// Run condition: terrain not yet loaded
fn geo_terrain_loaded(state: Res<GeoLoadState>) -> bool {
    state.terrain_loaded
}

/// System: load HGT terrain tiles from geo.toml terrain sources and spawn mesh entities.
/// Each tile is positioned on the WGS84 orbital grid relative to GeoOrigin.
fn spawn_terrain_from_config(
    mut commands: Commands,
    config: Res<GeoConfig>,
    origin: Res<GeoOrigin>,
    mut load_state: ResMut<GeoLoadState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let geo_dir = match &load_state.geo_dir {
        Some(dir) => dir.clone(),
        None => {
            load_state.terrain_loaded = true;
            return;
        }
    };

    // Collect all HGT tiles from configured terrain sources
    let mut all_tiles = Vec::new();

    for source in &config.terrain.sources {
        let source_path = geo_dir.join(&source.path);
        let extension = source_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "hgt" => {
                match terrain_import::HgtTile::load(&source_path) {
                    Ok(tile) => all_tiles.push(tile),
                    Err(e) => tracing::error!("Failed to load HGT tile: {}", e),
                }
            }
            _ => {
                // Check if it's a directory containing .hgt files
                if source_path.is_dir() {
                    all_tiles.extend(terrain_import::load_hgt_directory(&source_path));
                } else {
                    tracing::warn!(
                        "Unsupported terrain format '{}' — only .hgt supported in Phase 2",
                        extension
                    );
                }
            }
        }
    }

    if !all_tiles.is_empty() {
        // Position tiles on the WGS84 orbital grid
        let positioned = terrain_import::position_tiles(all_tiles, &origin);

        // Spawn terrain mesh entities
        let entities = terrain_import::spawn_terrain_tiles(
            &mut commands,
            &mut meshes,
            &mut materials,
            &positioned,
            config.terrain.vertical_exaggeration,
            config.terrain.chunk_resolution,
        );

        tracing::info!(
            "Spawned {} terrain tiles with {}x vertical exaggeration",
            entities.len(),
            config.terrain.vertical_exaggeration,
        );
    }

    load_state.terrain_loaded = true;
}

/// System: sync GeoLayer.visible to Bevy Visibility component
fn update_layer_visibility(
    mut query: Query<(&GeoLayer, &mut Visibility), Changed<GeoLayer>>,
) {
    for (layer, mut visibility) in query.iter_mut() {
        *visibility = if layer.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
