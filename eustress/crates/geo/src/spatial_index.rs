//! # Spatial Index
//!
//! R-tree wrapper for runtime spatial queries on geospatial features.
//! Replaces PostGIS `ST_Contains`, `ST_Intersects`, `ST_DWithin` with
//! in-memory R-tree lookups via the `rstar` crate.
//!
//! ## Table of Contents
//! 1. GeoSpatialIndex — R-tree resource
//! 2. IndexedFeature — R-tree entry
//! 3. Query methods

use bevy::prelude::*;
use rstar::{RTree, RTreeObject, AABB};

// ============================================================================
// 1. GeoSpatialIndex — R-tree resource
// ============================================================================

/// Bevy resource holding an R-tree spatial index of all loaded geospatial features.
/// Enables fast spatial queries (nearest neighbor, bounding box intersection).
#[derive(Resource)]
pub struct GeoSpatialIndex {
    /// R-tree of indexed features
    tree: RTree<IndexedFeature>,
}

impl Default for GeoSpatialIndex {
    fn default() -> Self {
        Self {
            tree: RTree::new(),
        }
    }
}

impl GeoSpatialIndex {
    /// Create a new empty spatial index
    pub fn new() -> Self {
        Self::default()
    }

    /// Bulk-load features into the R-tree (much faster than individual inserts)
    pub fn bulk_load(features: Vec<IndexedFeature>) -> Self {
        Self {
            tree: RTree::bulk_load(features),
        }
    }

    /// Insert a single feature
    pub fn insert(&mut self, feature: IndexedFeature) {
        self.tree.insert(feature);
    }

    /// Find all features whose bounding box intersects the given AABB.
    /// Coordinates are in Bevy world space (XZ plane).
    pub fn query_rect(&self, min_x: f32, min_z: f32, max_x: f32, max_z: f32) -> Vec<&IndexedFeature> {
        let envelope = AABB::from_corners([min_x, min_z], [max_x, max_z]);
        self.tree.locate_in_envelope(&envelope).collect()
    }

    /// Find the nearest feature to a point in XZ plane
    pub fn nearest(&self, x: f32, z: f32) -> Option<&IndexedFeature> {
        self.tree.nearest_neighbor(&[x, z])
    }

    /// Find all features within `radius` of a point in XZ plane
    pub fn within_radius(&self, x: f32, z: f32, radius: f32) -> Vec<&IndexedFeature> {
        let envelope = AABB::from_corners(
            [x - radius, z - radius],
            [x + radius, z + radius],
        );
        // First pass: bounding box filter, then distance check
        self.tree
            .locate_in_envelope(&envelope)
            .filter(|f| {
                let dx = f.center_x - x;
                let dz = f.center_z - z;
                (dx * dx + dz * dz) <= radius * radius
            })
            .collect()
    }

    /// Number of indexed features
    pub fn len(&self) -> usize {
        self.tree.size()
    }

    /// Whether the index is empty
    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }
}

// ============================================================================
// 2. IndexedFeature — R-tree entry
// ============================================================================

/// A feature entry in the spatial index.
/// Stores the bounding box in Bevy XZ plane coordinates and a reference
/// back to the ECS entity.
#[derive(Debug, Clone)]
pub struct IndexedFeature {
    /// Bevy entity this feature belongs to
    pub entity: Entity,
    /// Layer name
    pub layer: String,
    /// Feature name (if available)
    pub name: Option<String>,
    /// Bounding box min X (Bevy world space)
    pub min_x: f32,
    /// Bounding box min Z (Bevy world space)
    pub min_z: f32,
    /// Bounding box max X (Bevy world space)
    pub max_x: f32,
    /// Bounding box max Z (Bevy world space)
    pub max_z: f32,
    /// Center X (for distance queries)
    pub center_x: f32,
    /// Center Z (for distance queries)
    pub center_z: f32,
}

impl RTreeObject for IndexedFeature {
    type Envelope = AABB<[f32; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.min_x, self.min_z], [self.max_x, self.max_z])
    }
}

impl rstar::PointDistance for IndexedFeature {
    fn distance_2(&self, point: &[f32; 2]) -> f32 {
        let dx = (self.center_x - point[0]).max(0.0);
        let dz = (self.center_z - point[1]).max(0.0);
        dx * dx + dz * dz
    }
}

impl IndexedFeature {
    /// Create from a single point
    pub fn from_point(entity: Entity, layer: &str, name: Option<String>, pos: Vec3) -> Self {
        Self {
            entity,
            layer: layer.to_string(),
            name,
            min_x: pos.x,
            min_z: pos.z,
            max_x: pos.x,
            max_z: pos.z,
            center_x: pos.x,
            center_z: pos.z,
        }
    }

    /// Create from a list of vertices (computes bounding box)
    pub fn from_vertices(entity: Entity, layer: &str, name: Option<String>, verts: &[Vec3]) -> Self {
        if verts.is_empty() {
            return Self::from_point(entity, layer, name, Vec3::ZERO);
        }
        let mut min_x = f32::MAX;
        let mut min_z = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_z = f32::MIN;
        for v in verts {
            min_x = min_x.min(v.x);
            min_z = min_z.min(v.z);
            max_x = max_x.max(v.x);
            max_z = max_z.max(v.z);
        }
        Self {
            entity,
            layer: layer.to_string(),
            name,
            min_x,
            min_z,
            max_x,
            max_z,
            center_x: (min_x + max_x) * 0.5,
            center_z: (min_z + max_z) * 0.5,
        }
    }
}
