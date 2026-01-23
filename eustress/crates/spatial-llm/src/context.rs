//! Spatial Context - Scene graph and spatial entity representation
//!
//! ## Table of Contents
//! 1. SpatialContext - World state snapshot for LLM
//! 2. SceneGraph - Hierarchical entity relationships
//! 3. SpatialEntity - Entity with spatial properties

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Spatial entity representation for LLM context
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpatialEntity {
    /// Entity ID
    pub id: String,
    /// Entity name
    pub name: Option<String>,
    /// Entity class/type
    pub class: String,
    /// World position [x, y, z]
    pub position: [f64; 3],
    /// Rotation (euler angles) [x, y, z]
    pub rotation: [f64; 3],
    /// Scale [x, y, z]
    pub scale: [f64; 3],
    /// Entity properties
    pub properties: HashMap<String, serde_json::Value>,
    /// Tags
    pub tags: Vec<String>,
    /// Parent entity ID (if any)
    pub parent_id: Option<String>,
    /// Child entity IDs
    pub children: Vec<String>,
}

impl Default for SpatialEntity {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: None,
            class: "Entity".to_string(),
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            properties: HashMap::new(),
            tags: Vec::new(),
            parent_id: None,
            children: Vec::new(),
        }
    }
}

impl SpatialEntity {
    /// Create a new spatial entity
    pub fn new(id: impl Into<String>, class: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            class: class.into(),
            ..Default::default()
        }
    }

    /// Set position
    pub fn with_position(mut self, x: f64, y: f64, z: f64) -> Self {
        self.position = [x, y, z];
        self
    }

    /// Set name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Distance to another entity
    pub fn distance_to(&self, other: &SpatialEntity) -> f64 {
        let dx = self.position[0] - other.position[0];
        let dy = self.position[1] - other.position[1];
        let dz = self.position[2] - other.position[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Distance to a point
    pub fn distance_to_point(&self, x: f64, y: f64, z: f64) -> f64 {
        let dx = self.position[0] - x;
        let dy = self.position[1] - y;
        let dz = self.position[2] - z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

/// Scene graph representing hierarchical entity relationships
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SceneGraph {
    /// All entities by ID
    entities: HashMap<String, SpatialEntity>,
    /// Root entity IDs (no parent)
    roots: Vec<String>,
}

impl SceneGraph {
    /// Create a new empty scene graph
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entity to the scene graph
    pub fn add_entity(&mut self, entity: SpatialEntity) {
        let id = entity.id.clone();
        let parent_id = entity.parent_id.clone();

        self.entities.insert(id.clone(), entity);

        if parent_id.is_none() {
            self.roots.push(id);
        }
    }

    /// Get an entity by ID
    pub fn get(&self, id: &str) -> Option<&SpatialEntity> {
        self.entities.get(id)
    }

    /// Get all entities
    pub fn entities(&self) -> impl Iterator<Item = &SpatialEntity> {
        self.entities.values()
    }

    /// Get root entities
    pub fn roots(&self) -> impl Iterator<Item = &SpatialEntity> {
        self.roots.iter().filter_map(|id| self.entities.get(id))
    }

    /// Get children of an entity
    pub fn children(&self, parent_id: &str) -> impl Iterator<Item = &SpatialEntity> {
        self.entities
            .get(parent_id)
            .map(|e| e.children.clone())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| self.entities.get(&id))
    }

    /// Find entities within radius of a point
    pub fn find_within_radius(&self, x: f64, y: f64, z: f64, radius: f64) -> Vec<&SpatialEntity> {
        self.entities
            .values()
            .filter(|e| e.distance_to_point(x, y, z) <= radius)
            .collect()
    }

    /// Find entities by class
    pub fn find_by_class(&self, class: &str) -> Vec<&SpatialEntity> {
        self.entities
            .values()
            .filter(|e| e.class == class)
            .collect()
    }

    /// Find entities by tag
    pub fn find_by_tag(&self, tag: &str) -> Vec<&SpatialEntity> {
        self.entities
            .values()
            .filter(|e| e.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Get entity count
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// Spatial context for LLM - snapshot of world state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpatialContext {
    /// Scene graph with all entities
    pub scene_graph: SceneGraph,
    /// Focus point (camera/player position)
    pub focus_point: [f64; 3],
    /// Focus radius (how far to include entities)
    pub focus_radius: f64,
    /// Context metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Default for SpatialContext {
    fn default() -> Self {
        Self {
            scene_graph: SceneGraph::new(),
            focus_point: [0.0, 0.0, 0.0],
            focus_radius: 100.0,
            metadata: HashMap::new(),
        }
    }
}

impl SpatialContext {
    /// Create a new spatial context
    pub fn new() -> Self {
        Self::default()
    }

    /// Set focus point
    pub fn with_focus(mut self, x: f64, y: f64, z: f64, radius: f64) -> Self {
        self.focus_point = [x, y, z];
        self.focus_radius = radius;
        self
    }

    /// Get entities near the focus point
    pub fn focused_entities(&self) -> Vec<&SpatialEntity> {
        self.scene_graph.find_within_radius(
            self.focus_point[0],
            self.focus_point[1],
            self.focus_point[2],
            self.focus_radius,
        )
    }

    /// Convert to a text description for LLM prompt
    pub fn to_prompt_text(&self) -> String {
        let mut text = String::new();

        text.push_str(&format!(
            "Scene centered at ({:.1}, {:.1}, {:.1}) with radius {:.1}:\n",
            self.focus_point[0], self.focus_point[1], self.focus_point[2], self.focus_radius
        ));

        let entities = self.focused_entities();
        text.push_str(&format!("{} entities in view:\n", entities.len()));

        for entity in entities.iter().take(50) {
            let name = entity.name.as_deref().unwrap_or("unnamed");
            text.push_str(&format!(
                "- {} ({}) at ({:.1}, {:.1}, {:.1})",
                name, entity.class,
                entity.position[0], entity.position[1], entity.position[2]
            ));

            if !entity.tags.is_empty() {
                text.push_str(&format!(" [{}]", entity.tags.join(", ")));
            }

            text.push('\n');
        }

        if entities.len() > 50 {
            text.push_str(&format!("... and {} more entities\n", entities.len() - 50));
        }

        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_entity() {
        let entity = SpatialEntity::new("e1", "Tree")
            .with_position(10.0, 0.0, 5.0)
            .with_name("Oak Tree")
            .with_tag("vegetation");

        assert_eq!(entity.id, "e1");
        assert_eq!(entity.class, "Tree");
        assert_eq!(entity.position, [10.0, 0.0, 5.0]);
    }

    #[test]
    fn test_scene_graph() {
        let mut graph = SceneGraph::new();
        graph.add_entity(SpatialEntity::new("e1", "Tree").with_position(0.0, 0.0, 0.0));
        graph.add_entity(SpatialEntity::new("e2", "Rock").with_position(5.0, 0.0, 0.0));

        assert_eq!(graph.len(), 2);

        let nearby = graph.find_within_radius(0.0, 0.0, 0.0, 3.0);
        assert_eq!(nearby.len(), 1);
    }
}
