//! Spatial Indexing - Fast spatial queries using KD-trees
//!
//! ## Table of Contents
//! 1. SpatialIndex - KD-tree based spatial index
//! 2. IndexedEntity - Entity reference in the index

use crate::context::SpatialEntity;
use std::collections::HashMap;

/// Indexed entity reference
#[derive(Clone, Debug)]
pub struct IndexedEntity {
    /// Entity ID
    pub id: String,
    /// Position
    pub position: [f64; 3],
    /// Entity class
    pub class: String,
}

/// Spatial index for fast nearest-neighbor queries
/// Uses a simple grid-based approach for now
/// TODO: Replace with kiddo KD-tree when feature is enabled
pub struct SpatialIndex {
    /// Grid cell size
    cell_size: f64,
    /// Grid cells: (cell_x, cell_y, cell_z) -> entity IDs
    grid: HashMap<(i64, i64, i64), Vec<IndexedEntity>>,
    /// All indexed entities
    entities: HashMap<String, IndexedEntity>,
}

impl Default for SpatialIndex {
    fn default() -> Self {
        Self::new(10.0)
    }
}

impl SpatialIndex {
    /// Create a new spatial index with the given cell size
    pub fn new(cell_size: f64) -> Self {
        Self {
            cell_size,
            grid: HashMap::new(),
            entities: HashMap::new(),
        }
    }

    /// Get the grid cell for a position
    fn cell_for(&self, x: f64, y: f64, z: f64) -> (i64, i64, i64) {
        (
            (x / self.cell_size).floor() as i64,
            (y / self.cell_size).floor() as i64,
            (z / self.cell_size).floor() as i64,
        )
    }

    /// Insert an entity into the index
    pub fn insert(&mut self, entity: &SpatialEntity) {
        let indexed = IndexedEntity {
            id: entity.id.clone(),
            position: entity.position,
            class: entity.class.clone(),
        };

        let cell = self.cell_for(entity.position[0], entity.position[1], entity.position[2]);

        self.grid.entry(cell).or_default().push(indexed.clone());
        self.entities.insert(entity.id.clone(), indexed);
    }

    /// Remove an entity from the index
    pub fn remove(&mut self, id: &str) {
        if let Some(entity) = self.entities.remove(id) {
            let cell = self.cell_for(entity.position[0], entity.position[1], entity.position[2]);
            if let Some(entities) = self.grid.get_mut(&cell) {
                entities.retain(|e| e.id != id);
            }
        }
    }

    /// Find entities within radius of a point
    pub fn find_within_radius(&self, x: f64, y: f64, z: f64, radius: f64) -> Vec<&IndexedEntity> {
        let mut results = Vec::new();
        let radius_sq = radius * radius;

        // Calculate cell range to search
        let cells_to_check = (radius / self.cell_size).ceil() as i64 + 1;
        let center_cell = self.cell_for(x, y, z);

        for dx in -cells_to_check..=cells_to_check {
            for dy in -cells_to_check..=cells_to_check {
                for dz in -cells_to_check..=cells_to_check {
                    let cell = (center_cell.0 + dx, center_cell.1 + dy, center_cell.2 + dz);

                    if let Some(entities) = self.grid.get(&cell) {
                        for entity in entities {
                            let dist_sq = (entity.position[0] - x).powi(2)
                                + (entity.position[1] - y).powi(2)
                                + (entity.position[2] - z).powi(2);

                            if dist_sq <= radius_sq {
                                results.push(entity);
                            }
                        }
                    }
                }
            }
        }

        results
    }

    /// Find k nearest neighbors to a point
    pub fn find_k_nearest(&self, x: f64, y: f64, z: f64, k: usize) -> Vec<(&IndexedEntity, f64)> {
        // Simple brute-force for now
        // TODO: Use proper KD-tree for efficiency
        let mut distances: Vec<_> = self
            .entities
            .values()
            .map(|e| {
                let dist = ((e.position[0] - x).powi(2)
                    + (e.position[1] - y).powi(2)
                    + (e.position[2] - z).powi(2))
                .sqrt();
                (e, dist)
            })
            .collect();

        distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        distances.truncate(k);
        distances
    }

    /// Get entity count
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Clear the index
    pub fn clear(&mut self) {
        self.grid.clear();
        self.entities.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_index() {
        let mut index = SpatialIndex::new(10.0);

        index.insert(&SpatialEntity::new("e1", "Tree").with_position(0.0, 0.0, 0.0));
        index.insert(&SpatialEntity::new("e2", "Rock").with_position(5.0, 0.0, 0.0));
        index.insert(&SpatialEntity::new("e3", "Bush").with_position(100.0, 0.0, 0.0));

        assert_eq!(index.len(), 3);

        let nearby = index.find_within_radius(0.0, 0.0, 0.0, 10.0);
        assert_eq!(nearby.len(), 2);

        let nearest = index.find_k_nearest(0.0, 0.0, 0.0, 2);
        assert_eq!(nearest.len(), 2);
        assert_eq!(nearest[0].0.id, "e1");
    }
}
