//! # Vector Data Import
//!
//! Parses GeoJSON files into typed geometry collections projected to local coords.
//! Each feature becomes a `LocalFeature` with Bevy-space vertices ready for meshing.
//!
//! ## Table of Contents
//! 1. LocalFeature — Projected feature with Bevy-space geometry
//! 2. LocalGeometry — Geometry variants in local coordinates
//! 3. GeoJSON import
//! 4. Feature property extraction

use bevy::prelude::*;
use geojson::{Feature, GeoJson, Geometry, Value};
use std::path::Path;

use crate::coords::{geojson_coord_to_world, GeoOrigin};

// ============================================================================
// 1. LocalFeature — Projected feature with Bevy-space geometry
// ============================================================================

/// A geospatial feature projected into Bevy local coordinates
#[derive(Debug, Clone)]
pub struct LocalFeature {
    /// Feature index within the source file
    pub index: usize,
    /// Geometry in Bevy world space
    pub geometry: LocalGeometry,
    /// Feature name (from "name" property, if present)
    pub name: Option<String>,
    /// Raw properties as JSON string
    pub properties_json: Option<String>,
    /// Centroid latitude (original WGS84)
    pub centroid_lat: f64,
    /// Centroid longitude (original WGS84)
    pub centroid_lon: f64,
}

// ============================================================================
// 2. LocalGeometry — Geometry variants in local coordinates
// ============================================================================

/// Geometry projected into Bevy world space (Y-up, meters from origin)
#[derive(Debug, Clone)]
pub enum LocalGeometry {
    /// Single point
    Point(Vec3),
    /// Multiple points
    MultiPoint(Vec<Vec3>),
    /// Polyline (ordered vertices)
    LineString(Vec<Vec3>),
    /// Multiple polylines
    MultiLineString(Vec<Vec<Vec3>>),
    /// Polygon (outer ring + optional holes)
    Polygon {
        outer: Vec<Vec3>,
        holes: Vec<Vec<Vec3>>,
    },
    /// Multiple polygons
    MultiPolygon(Vec<PolygonRings>),
}

/// A polygon with outer ring and optional holes
#[derive(Debug, Clone)]
pub struct PolygonRings {
    pub outer: Vec<Vec3>,
    pub holes: Vec<Vec<Vec3>>,
}

// ============================================================================
// 3. GeoJSON import
// ============================================================================

/// Import a GeoJSON file and project all features to Bevy local coordinates
pub fn import_geojson(path: &Path, origin: &GeoOrigin) -> Result<Vec<LocalFeature>, GeoImportError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| GeoImportError::Io(path.to_path_buf(), e))?;

    let geojson: GeoJson = content.parse()
        .map_err(|e| GeoImportError::Parse(path.to_path_buf(), format!("{}", e)))?;

    let features = match geojson {
        GeoJson::FeatureCollection(fc) => fc.features,
        GeoJson::Feature(f) => vec![f],
        GeoJson::Geometry(g) => vec![Feature {
            bbox: None,
            geometry: Some(g),
            id: None,
            properties: None,
            foreign_members: None,
        }],
    };

    let mut local_features = Vec::with_capacity(features.len());

    for (index, feature) in features.into_iter().enumerate() {
        if let Some(geometry) = feature.geometry {
            let (local_geom, centroid_lat, centroid_lon) = project_geometry(&geometry, origin);
            let name = extract_name(&feature.properties);
            let properties_json = feature.properties
                .as_ref()
                .map(|p| serde_json::to_string(p).unwrap_or_default());

            local_features.push(LocalFeature {
                index,
                geometry: local_geom,
                name,
                properties_json,
                centroid_lat,
                centroid_lon,
            });
        }
    }

    tracing::info!(
        "Imported {} features from {}",
        local_features.len(),
        path.display()
    );

    Ok(local_features)
}

/// Project a GeoJSON Geometry into Bevy local coordinates.
/// Returns (LocalGeometry, centroid_lat, centroid_lon).
fn project_geometry(geometry: &Geometry, origin: &GeoOrigin) -> (LocalGeometry, f64, f64) {
    match &geometry.value {
        Value::Point(coord) => {
            let pos = geojson_coord_to_world(coord, origin);
            (LocalGeometry::Point(pos), coord[1], coord[0])
        }
        Value::MultiPoint(coords) => {
            let points: Vec<Vec3> = coords.iter()
                .map(|c| geojson_coord_to_world(c, origin))
                .collect();
            let (clat, clon) = centroid_of_coords(coords);
            (LocalGeometry::MultiPoint(points), clat, clon)
        }
        Value::LineString(coords) => {
            let verts: Vec<Vec3> = coords.iter()
                .map(|c| geojson_coord_to_world(c, origin))
                .collect();
            let (clat, clon) = centroid_of_coords(coords);
            (LocalGeometry::LineString(verts), clat, clon)
        }
        Value::MultiLineString(lines) => {
            let all_coords: Vec<&Vec<f64>> = lines.iter().flat_map(|l| l.iter()).collect();
            let (clat, clon) = centroid_of_coord_refs(&all_coords);
            let multi: Vec<Vec<Vec3>> = lines.iter()
                .map(|line| line.iter().map(|c| geojson_coord_to_world(c, origin)).collect())
                .collect();
            (LocalGeometry::MultiLineString(multi), clat, clon)
        }
        Value::Polygon(rings) => {
            let (clat, clon) = if !rings.is_empty() {
                centroid_of_coords(&rings[0])
            } else {
                (0.0, 0.0)
            };
            let outer: Vec<Vec3> = rings.first()
                .map(|r| r.iter().map(|c| geojson_coord_to_world(c, origin)).collect())
                .unwrap_or_default();
            let holes: Vec<Vec<Vec3>> = rings.iter().skip(1)
                .map(|r| r.iter().map(|c| geojson_coord_to_world(c, origin)).collect())
                .collect();
            (LocalGeometry::Polygon { outer, holes }, clat, clon)
        }
        Value::MultiPolygon(polys) => {
            let mut all_outer_coords: Vec<Vec<f64>> = Vec::new();
            let multi: Vec<PolygonRings> = polys.iter().map(|rings| {
                let outer: Vec<Vec3> = rings.first()
                    .map(|r| {
                        all_outer_coords.extend(r.iter().cloned());
                        r.iter().map(|c| geojson_coord_to_world(c, origin)).collect()
                    })
                    .unwrap_or_default();
                let holes: Vec<Vec<Vec3>> = rings.iter().skip(1)
                    .map(|r| r.iter().map(|c| geojson_coord_to_world(c, origin)).collect())
                    .collect();
                PolygonRings { outer, holes }
            }).collect();
            let (clat, clon) = centroid_of_coords(&all_outer_coords);
            (LocalGeometry::MultiPolygon(multi), clat, clon)
        }
        Value::GeometryCollection(geoms) => {
            // Flatten: use first geometry
            if let Some(first) = geoms.first() {
                project_geometry(first, origin)
            } else {
                (LocalGeometry::Point(Vec3::ZERO), 0.0, 0.0)
            }
        }
    }
}

// ============================================================================
// 4. Feature property extraction
// ============================================================================

/// Extract a "name" property from GeoJSON feature properties
fn extract_name(properties: &Option<serde_json::Map<String, serde_json::Value>>) -> Option<String> {
    properties.as_ref().and_then(|props| {
        props.get("name")
            .or_else(|| props.get("Name"))
            .or_else(|| props.get("NAME"))
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    })
}

/// Compute centroid of a list of [lon, lat, ...] coordinates
fn centroid_of_coords(coords: &[Vec<f64>]) -> (f64, f64) {
    if coords.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum_lat = 0.0;
    let mut sum_lon = 0.0;
    for c in coords {
        sum_lon += c[0];
        sum_lat += c[1];
    }
    let n = coords.len() as f64;
    (sum_lat / n, sum_lon / n)
}

/// Compute centroid from references to coordinate vecs
fn centroid_of_coord_refs(coords: &[&Vec<f64>]) -> (f64, f64) {
    if coords.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum_lat = 0.0;
    let mut sum_lon = 0.0;
    for c in coords {
        sum_lon += c[0];
        sum_lat += c[1];
    }
    let n = coords.len() as f64;
    (sum_lat / n, sum_lon / n)
}

/// Errors from geospatial data import
#[derive(Debug)]
pub enum GeoImportError {
    /// File I/O error
    Io(std::path::PathBuf, std::io::Error),
    /// Parse error
    Parse(std::path::PathBuf, String),
}

impl std::fmt::Display for GeoImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeoImportError::Io(path, e) => write!(f, "Failed to read {}: {}", path.display(), e),
            GeoImportError::Parse(path, e) => write!(f, "Failed to parse {}: {}", path.display(), e),
        }
    }
}

impl std::error::Error for GeoImportError {}
