//! # Spatial Hashing
//!
//! Spatial data structure for efficient neighbor queries.
//!
//! ## Table of Contents
//!
//! 1. **SpatialHash** - Grid-based spatial hash for O(1) cell lookup
//! 2. **Query Methods** - Radius queries, k-nearest neighbors

use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// Spatial Hash
// ============================================================================

/// Grid-based spatial hash for efficient neighbor queries
#[derive(Resource, Debug, Clone)]
pub struct SpatialHash {
    /// Cell size in world units
    pub cell_size: f32,
    /// Map from cell coordinates to entities in that cell
    cells: HashMap<IVec3, Vec<(Entity, Vec3)>>,
    /// Total entity count
    entity_count: usize,
}

impl Default for SpatialHash {
    fn default() -> Self {
        Self {
            cell_size: 1.0,
            cells: HashMap::new(),
            entity_count: 0,
        }
    }
}

impl SpatialHash {
    /// Create a new spatial hash with given cell size
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size: cell_size.max(0.01),
            cells: HashMap::new(),
            entity_count: 0,
        }
    }
    
    /// Clear all entries
    pub fn clear(&mut self) {
        self.cells.clear();
        self.entity_count = 0;
    }
    
    /// Convert world position to cell coordinates
    #[inline]
    fn position_to_cell(&self, position: Vec3) -> IVec3 {
        IVec3::new(
            (position.x / self.cell_size).floor() as i32,
            (position.y / self.cell_size).floor() as i32,
            (position.z / self.cell_size).floor() as i32,
        )
    }
    
    /// Insert an entity at a position
    pub fn insert(&mut self, entity: Entity, position: Vec3) {
        let cell = self.position_to_cell(position);
        self.cells
            .entry(cell)
            .or_insert_with(Vec::new)
            .push((entity, position));
        self.entity_count += 1;
    }
    
    /// Remove an entity from a position
    pub fn remove(&mut self, entity: Entity, position: Vec3) {
        let cell = self.position_to_cell(position);
        if let Some(entities) = self.cells.get_mut(&cell) {
            entities.retain(|(e, _)| *e != entity);
            if entities.is_empty() {
                self.cells.remove(&cell);
            }
            self.entity_count = self.entity_count.saturating_sub(1);
        }
    }
    
    /// Update an entity's position
    pub fn update(&mut self, entity: Entity, old_position: Vec3, new_position: Vec3) {
        let old_cell = self.position_to_cell(old_position);
        let new_cell = self.position_to_cell(new_position);
        
        if old_cell != new_cell {
            self.remove(entity, old_position);
            self.insert(entity, new_position);
        } else {
            // Update position in same cell
            if let Some(entities) = self.cells.get_mut(&old_cell) {
                for (e, pos) in entities.iter_mut() {
                    if *e == entity {
                        *pos = new_position;
                        break;
                    }
                }
            }
        }
    }
    
    /// Query all entities within a radius of a position
    pub fn query_radius(&self, center: Vec3, radius: f32) -> Vec<Entity> {
        let radius_squared = radius * radius;
        let cells_to_check = (radius / self.cell_size).ceil() as i32 + 1;
        let center_cell = self.position_to_cell(center);
        
        let mut results = Vec::new();
        
        for dx in -cells_to_check..=cells_to_check {
            for dy in -cells_to_check..=cells_to_check {
                for dz in -cells_to_check..=cells_to_check {
                    let cell = center_cell + IVec3::new(dx, dy, dz);
                    if let Some(entities) = self.cells.get(&cell) {
                        for (entity, position) in entities {
                            let distance_squared = (*position - center).length_squared();
                            if distance_squared <= radius_squared {
                                results.push(*entity);
                            }
                        }
                    }
                }
            }
        }
        
        results
    }
    
    /// Query all entities within a radius, returning entity and position
    pub fn query_radius_with_positions(&self, center: Vec3, radius: f32) -> Vec<(Entity, Vec3)> {
        let radius_squared = radius * radius;
        let cells_to_check = (radius / self.cell_size).ceil() as i32 + 1;
        let center_cell = self.position_to_cell(center);
        
        let mut results = Vec::new();
        
        for dx in -cells_to_check..=cells_to_check {
            for dy in -cells_to_check..=cells_to_check {
                for dz in -cells_to_check..=cells_to_check {
                    let cell = center_cell + IVec3::new(dx, dy, dz);
                    if let Some(entities) = self.cells.get(&cell) {
                        for (entity, position) in entities {
                            let distance_squared = (*position - center).length_squared();
                            if distance_squared <= radius_squared {
                                results.push((*entity, *position));
                            }
                        }
                    }
                }
            }
        }
        
        results
    }
    
    /// Query all entities within a radius, excluding a specific entity
    pub fn query_radius_excluding(&self, center: Vec3, radius: f32, exclude: Entity) -> Vec<Entity> {
        self.query_radius(center, radius)
            .into_iter()
            .filter(|e| *e != exclude)
            .collect()
    }
    
    /// Find k-nearest neighbors
    pub fn query_k_nearest(&self, center: Vec3, k: usize) -> Vec<(Entity, f32)> {
        // Start with a reasonable search radius and expand if needed
        let mut search_radius = self.cell_size * 2.0;
        let mut results: Vec<(Entity, f32)> = Vec::new();
        
        for _ in 0..10 {
            results = self.query_radius_with_positions(center, search_radius)
                .into_iter()
                .map(|(e, p)| (e, (p - center).length()))
                .collect();
            
            if results.len() >= k {
                break;
            }
            search_radius *= 2.0;
        }
        
        // Sort by distance and take k nearest
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);
        results
    }
    
    /// Get all entities in a specific cell
    pub fn get_cell(&self, cell: IVec3) -> Option<&Vec<(Entity, Vec3)>> {
        self.cells.get(&cell)
    }
    
    /// Get the cell containing a position
    pub fn get_cell_at(&self, position: Vec3) -> Option<&Vec<(Entity, Vec3)>> {
        let cell = self.position_to_cell(position);
        self.cells.get(&cell)
    }
    
    /// Get total entity count
    pub fn len(&self) -> usize {
        self.entity_count
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entity_count == 0
    }
    
    /// Get number of occupied cells
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
    
    /// Iterate over all entities
    pub fn iter(&self) -> impl Iterator<Item = (Entity, Vec3)> + '_ {
        self.cells.values().flatten().copied()
    }
}

// ============================================================================
// Neighbor Iterator
// ============================================================================

/// Iterator over neighbors within a radius
pub struct NeighborIterator<'a> {
    spatial_hash: &'a SpatialHash,
    center: Vec3,
    radius_squared: f32,
    current_cell_offset: i32,
    cells_to_check: i32,
    center_cell: IVec3,
    current_dx: i32,
    current_dy: i32,
    current_dz: i32,
    current_cell_iter: Option<std::slice::Iter<'a, (Entity, Vec3)>>,
}

impl<'a> NeighborIterator<'a> {
    pub fn new(spatial_hash: &'a SpatialHash, center: Vec3, radius: f32) -> Self {
        let cells_to_check = (radius / spatial_hash.cell_size).ceil() as i32 + 1;
        Self {
            spatial_hash,
            center,
            radius_squared: radius * radius,
            current_cell_offset: 0,
            cells_to_check,
            center_cell: spatial_hash.position_to_cell(center),
            current_dx: -cells_to_check,
            current_dy: -cells_to_check,
            current_dz: -cells_to_check - 1, // Will be incremented on first next()
            current_cell_iter: None,
        }
    }
    
    fn advance_cell(&mut self) -> bool {
        self.current_dz += 1;
        if self.current_dz > self.cells_to_check {
            self.current_dz = -self.cells_to_check;
            self.current_dy += 1;
            if self.current_dy > self.cells_to_check {
                self.current_dy = -self.cells_to_check;
                self.current_dx += 1;
                if self.current_dx > self.cells_to_check {
                    return false;
                }
            }
        }
        
        let cell = self.center_cell + IVec3::new(self.current_dx, self.current_dy, self.current_dz);
        self.current_cell_iter = self.spatial_hash.cells.get(&cell).map(|v| v.iter());
        true
    }
}

impl<'a> Iterator for NeighborIterator<'a> {
    type Item = (Entity, Vec3, f32);
    
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut iter) = self.current_cell_iter {
                while let Some((entity, position)) = iter.next() {
                    let distance_squared = (*position - self.center).length_squared();
                    if distance_squared <= self.radius_squared {
                        return Some((*entity, *position, distance_squared.sqrt()));
                    }
                }
            }
            
            if !self.advance_cell() {
                return None;
            }
        }
    }
}

impl SpatialHash {
    /// Get an iterator over neighbors within a radius
    pub fn neighbors(&self, center: Vec3, radius: f32) -> NeighborIterator<'_> {
        NeighborIterator::new(self, center, radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_insert_and_query() {
        let mut hash = SpatialHash::new(1.0);
        
        let e1 = Entity::from_raw(1);
        let e2 = Entity::from_raw(2);
        let e3 = Entity::from_raw(3);
        
        hash.insert(e1, Vec3::new(0.0, 0.0, 0.0));
        hash.insert(e2, Vec3::new(0.5, 0.0, 0.0));
        hash.insert(e3, Vec3::new(5.0, 0.0, 0.0));
        
        let results = hash.query_radius(Vec3::ZERO, 1.0);
        assert_eq!(results.len(), 2);
        assert!(results.contains(&e1));
        assert!(results.contains(&e2));
        assert!(!results.contains(&e3));
    }
    
    #[test]
    fn test_k_nearest() {
        let mut hash = SpatialHash::new(1.0);
        
        for i in 0..10 {
            let e = Entity::from_raw(i);
            hash.insert(e, Vec3::new(i as f32, 0.0, 0.0));
        }
        
        let results = hash.query_k_nearest(Vec3::ZERO, 3);
        assert_eq!(results.len(), 3);
        assert!(results[0].1 < results[1].1);
        assert!(results[1].1 < results[2].1);
    }
    
    #[test]
    fn test_update() {
        let mut hash = SpatialHash::new(1.0);
        let e = Entity::from_raw(1);
        
        hash.insert(e, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(hash.query_radius(Vec3::ZERO, 0.5).len(), 1);
        
        hash.update(e, Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(hash.query_radius(Vec3::ZERO, 0.5).len(), 0);
        assert_eq!(hash.query_radius(Vec3::new(10.0, 0.0, 0.0), 0.5).len(), 1);
    }
}
