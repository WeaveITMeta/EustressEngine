//! Ontology - Hierarchical instance and property schema for vector store
//!
//! ## Table of Contents
//! 1. OntologyNode - Node in the ontology tree
//! 2. OntologyTree - Full hierarchical schema
//! 3. PropertySchema - Property type definitions
//! 4. InstancePath - Hierarchical path to an instance
//! 5. OntologyIndex - Vector index with ontology awareness

use crate::components::EmbeddingMetadata;
use crate::error::{EmbedvecError, Result};
use crate::resource::{EmbedvecIndex, IndexConfig, SearchResult};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// Property Schema
// ============================================================================

/// Property data type
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PropertyType {
    /// Boolean value
    Bool,
    /// Integer value
    Int,
    /// Floating point value
    Float,
    /// String value
    String,
    /// Vector2 (x, y)
    Vec2,
    /// Vector3 (x, y, z)
    Vec3,
    /// Vector4 (x, y, z, w) or Quaternion
    Vec4,
    /// Color (r, g, b, a)
    Color,
    /// Reference to another entity
    EntityRef,
    /// Reference to an asset
    AssetRef,
    /// Nested object with properties
    Object(String), // Schema name
    /// Array of values
    Array(Box<PropertyType>),
    /// Enum with variants
    Enum(Vec<String>),
    /// Any JSON value
    Any,
}

/// Property definition in a schema
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropertySchema {
    /// Property name
    pub name: String,
    /// Property type
    pub property_type: PropertyType,
    /// Whether the property is required
    pub required: bool,
    /// Default value (JSON)
    pub default: Option<serde_json::Value>,
    /// Description
    pub description: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Whether to include in embeddings
    pub embed: bool,
    /// Embedding weight (0.0 - 1.0)
    pub embed_weight: f32,
}

impl PropertySchema {
    /// Create a new property schema
    pub fn new(name: impl Into<String>, property_type: PropertyType) -> Self {
        Self {
            name: name.into(),
            property_type,
            required: false,
            default: None,
            description: None,
            tags: Vec::new(),
            embed: true,
            embed_weight: 1.0,
        }
    }

    /// Mark as required
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Set default value
    pub fn with_default(mut self, default: serde_json::Value) -> Self {
        self.default = Some(default);
        self
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set embedding weight
    pub fn with_embed_weight(mut self, weight: f32) -> Self {
        self.embed_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Exclude from embeddings
    pub fn no_embed(mut self) -> Self {
        self.embed = false;
        self
    }
}

// ============================================================================
// Ontology Node
// ============================================================================

/// Node in the ontology tree representing a class/type
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OntologyNode {
    /// Unique node ID
    pub id: Uuid,
    /// Node name (class name)
    pub name: String,
    /// Parent node ID (None for root)
    pub parent_id: Option<Uuid>,
    /// Child node IDs
    pub children: Vec<Uuid>,
    /// Properties defined at this level
    pub properties: Vec<PropertySchema>,
    /// Description
    pub description: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Whether this is an abstract class (can't be instantiated)
    pub is_abstract: bool,
    /// Embedding for this class (semantic representation)
    pub class_embedding: Option<Vec<f32>>,
    /// Instance count
    pub instance_count: u64,
}

impl OntologyNode {
    /// Create a new ontology node
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            parent_id: None,
            children: Vec::new(),
            properties: Vec::new(),
            description: None,
            tags: Vec::new(),
            is_abstract: false,
            class_embedding: None,
            instance_count: 0,
        }
    }

    /// Set parent
    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Add property
    pub fn with_property(mut self, property: PropertySchema) -> Self {
        self.properties.push(property);
        self
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Mark as abstract
    pub fn abstract_class(mut self) -> Self {
        self.is_abstract = true;
        self
    }

    /// Add tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Get full path from root (requires tree context)
    pub fn path(&self) -> String {
        // This is a simplified version - full implementation needs tree context
        self.name.clone()
    }

    /// Get all property names including inherited
    pub fn all_property_names(&self) -> Vec<&str> {
        self.properties.iter().map(|p| p.name.as_str()).collect()
    }
}

// ============================================================================
// Ontology Tree
// ============================================================================

/// Hierarchical ontology tree
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OntologyTree {
    /// All nodes by ID
    nodes: HashMap<Uuid, OntologyNode>,
    /// Root node IDs
    roots: Vec<Uuid>,
    /// Name to ID mapping for fast lookup
    name_index: HashMap<String, Uuid>,
    /// Path to ID mapping (e.g., "Entity/Actor/Character/Player")
    path_index: HashMap<String, Uuid>,
}

impl OntologyTree {
    /// Create a new empty ontology tree
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with standard Eustress base ontology
    pub fn with_eustress_base() -> Self {
        let mut tree = Self::new();

        // Root: Entity
        let entity = OntologyNode::new("Entity")
            .abstract_class()
            .with_description("Base class for all world objects")
            .with_property(PropertySchema::new("name", PropertyType::String))
            .with_property(PropertySchema::new("tags", PropertyType::Array(Box::new(PropertyType::String))))
            .with_property(PropertySchema::new("enabled", PropertyType::Bool).with_default(serde_json::json!(true)));
        let entity_id = entity.id;
        tree.add_root(entity);

        // Entity/Spatial - Things with transforms
        let spatial = OntologyNode::new("Spatial")
            .with_parent(entity_id)
            .abstract_class()
            .with_description("Entity with spatial properties")
            .with_property(PropertySchema::new("position", PropertyType::Vec3).with_embed_weight(1.5))
            .with_property(PropertySchema::new("rotation", PropertyType::Vec4))
            .with_property(PropertySchema::new("scale", PropertyType::Vec3));
        let spatial_id = spatial.id;
        tree.add_child(entity_id, spatial);

        // Entity/Spatial/Actor - Things that can act
        let actor = OntologyNode::new("Actor")
            .with_parent(spatial_id)
            .abstract_class()
            .with_description("Entity that can perform actions")
            .with_property(PropertySchema::new("health", PropertyType::Float))
            .with_property(PropertySchema::new("velocity", PropertyType::Vec3));
        let actor_id = actor.id;
        tree.add_child(spatial_id, actor);

        // Entity/Spatial/Actor/Character
        let character = OntologyNode::new("Character")
            .with_parent(actor_id)
            .with_description("Humanoid character")
            .with_property(PropertySchema::new("display_name", PropertyType::String))
            .with_property(PropertySchema::new("level", PropertyType::Int));
        let character_id = character.id;
        tree.add_child(actor_id, character);

        // Entity/Spatial/Actor/Character/Player
        let player = OntologyNode::new("Player")
            .with_parent(character_id)
            .with_description("Player-controlled character")
            .with_property(PropertySchema::new("user_id", PropertyType::String))
            .with_property(PropertySchema::new("session_id", PropertyType::String).no_embed());
        tree.add_child(character_id, player);

        // Entity/Spatial/Actor/Character/NPC
        let npc = OntologyNode::new("NPC")
            .with_parent(character_id)
            .with_description("Non-player character")
            .with_property(PropertySchema::new("behavior", PropertyType::String))
            .with_property(PropertySchema::new("dialogue_tree", PropertyType::AssetRef).no_embed());
        tree.add_child(character_id, npc);

        // Entity/Spatial/Prop - Static objects
        let prop = OntologyNode::new("Prop")
            .with_parent(spatial_id)
            .with_description("Static world object")
            .with_property(PropertySchema::new("mesh", PropertyType::AssetRef).no_embed())
            .with_property(PropertySchema::new("material", PropertyType::AssetRef).no_embed())
            .with_property(PropertySchema::new("collision", PropertyType::Bool));
        let prop_id = prop.id;
        tree.add_child(spatial_id, prop);

        // Entity/Spatial/Prop/Vegetation
        let vegetation = OntologyNode::new("Vegetation")
            .with_parent(prop_id)
            .with_description("Plants and trees")
            .with_property(PropertySchema::new("species", PropertyType::String).with_embed_weight(1.2))
            .with_property(PropertySchema::new("growth_stage", PropertyType::Float));
        tree.add_child(prop_id, vegetation);

        // Entity/Spatial/Prop/Structure
        let structure = OntologyNode::new("Structure")
            .with_parent(prop_id)
            .with_description("Buildings and constructions")
            .with_property(PropertySchema::new("building_type", PropertyType::String).with_embed_weight(1.2))
            .with_property(PropertySchema::new("floors", PropertyType::Int));
        tree.add_child(prop_id, structure);

        // Entity/Spatial/Light
        let light = OntologyNode::new("Light")
            .with_parent(spatial_id)
            .with_description("Light source")
            .with_property(PropertySchema::new("color", PropertyType::Color))
            .with_property(PropertySchema::new("intensity", PropertyType::Float))
            .with_property(PropertySchema::new("range", PropertyType::Float));
        tree.add_child(spatial_id, light);

        // Entity/Spatial/Volume
        let volume = OntologyNode::new("Volume")
            .with_parent(spatial_id)
            .with_description("Spatial volume/region")
            .with_property(PropertySchema::new("shape", PropertyType::Enum(vec![
                "Box".to_string(), "Sphere".to_string(), "Capsule".to_string(), "Mesh".to_string()
            ])))
            .with_property(PropertySchema::new("extents", PropertyType::Vec3));
        tree.add_child(spatial_id, volume);

        // Entity/Data - Non-spatial data entities
        let data = OntologyNode::new("Data")
            .with_parent(entity_id)
            .abstract_class()
            .with_description("Non-spatial data entity");
        let data_id = data.id;
        tree.add_child(entity_id, data);

        // Entity/Data/Config
        let config = OntologyNode::new("Config")
            .with_parent(data_id)
            .with_description("Configuration data")
            .with_property(PropertySchema::new("settings", PropertyType::Any));
        tree.add_child(data_id, config);

        // Entity/Data/Event
        let event = OntologyNode::new("Event")
            .with_parent(data_id)
            .with_description("Event data")
            .with_property(PropertySchema::new("event_type", PropertyType::String))
            .with_property(PropertySchema::new("payload", PropertyType::Any))
            .with_property(PropertySchema::new("timestamp", PropertyType::Float));
        tree.add_child(data_id, event);

        tree.rebuild_indices();
        tree
    }

    /// Add a root node
    pub fn add_root(&mut self, node: OntologyNode) {
        let id = node.id;
        let name = node.name.clone();
        self.nodes.insert(id, node);
        self.roots.push(id);
        self.name_index.insert(name.clone(), id);
        self.path_index.insert(name, id);
    }

    /// Add a child node
    pub fn add_child(&mut self, parent_id: Uuid, mut node: OntologyNode) {
        node.parent_id = Some(parent_id);
        let id = node.id;
        let name = node.name.clone();

        // Build path
        let path = self.build_path(parent_id, &name);

        self.nodes.insert(id, node);
        self.name_index.insert(name, id);
        self.path_index.insert(path, id);

        // Update parent's children
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children.push(id);
        }
    }

    /// Build hierarchical path
    fn build_path(&self, parent_id: Uuid, name: &str) -> String {
        let mut path_parts = vec![name.to_string()];
        let mut current_id = Some(parent_id);

        while let Some(id) = current_id {
            if let Some(node) = self.nodes.get(&id) {
                path_parts.push(node.name.clone());
                current_id = node.parent_id;
            } else {
                break;
            }
        }

        path_parts.reverse();
        path_parts.join("/")
    }

    /// Rebuild all indices
    pub fn rebuild_indices(&mut self) {
        self.name_index.clear();
        self.path_index.clear();

        for (id, node) in &self.nodes {
            self.name_index.insert(node.name.clone(), *id);

            // Build path
            let path = if let Some(parent_id) = node.parent_id {
                self.build_path(parent_id, &node.name)
            } else {
                node.name.clone()
            };
            self.path_index.insert(path, *id);
        }
    }

    /// Get node by ID
    pub fn get(&self, id: Uuid) -> Option<&OntologyNode> {
        self.nodes.get(&id)
    }

    /// Get node by name
    pub fn get_by_name(&self, name: &str) -> Option<&OntologyNode> {
        self.name_index.get(name).and_then(|id| self.nodes.get(id))
    }

    /// Get node by path
    pub fn get_by_path(&self, path: &str) -> Option<&OntologyNode> {
        self.path_index.get(path).and_then(|id| self.nodes.get(id))
    }

    /// Get all ancestors of a node (from immediate parent to root)
    pub fn ancestors(&self, id: Uuid) -> Vec<&OntologyNode> {
        let mut result = Vec::new();
        let mut current_id = self.nodes.get(&id).and_then(|n| n.parent_id);

        while let Some(id) = current_id {
            if let Some(node) = self.nodes.get(&id) {
                result.push(node);
                current_id = node.parent_id;
            } else {
                break;
            }
        }

        result
    }

    /// Get all descendants of a node (recursive)
    pub fn descendants(&self, id: Uuid) -> Vec<&OntologyNode> {
        let mut result = Vec::new();
        let mut stack = vec![id];

        while let Some(current_id) = stack.pop() {
            if let Some(node) = self.nodes.get(&current_id) {
                if current_id != id {
                    result.push(node);
                }
                stack.extend(&node.children);
            }
        }

        result
    }

    /// Get all properties for a node including inherited
    pub fn all_properties(&self, id: Uuid) -> Vec<&PropertySchema> {
        let mut result = Vec::new();

        // Get ancestors in reverse order (root first)
        let mut ancestors = self.ancestors(id);
        ancestors.reverse();

        // Add properties from ancestors
        for ancestor in ancestors {
            result.extend(ancestor.properties.iter());
        }

        // Add own properties
        if let Some(node) = self.nodes.get(&id) {
            result.extend(node.properties.iter());
        }

        result
    }

    /// Get the path for a node
    pub fn path_for(&self, id: Uuid) -> Option<String> {
        self.path_index
            .iter()
            .find(|(_, &node_id)| node_id == id)
            .map(|(path, _)| path.clone())
    }

    /// Get all root nodes
    pub fn roots(&self) -> impl Iterator<Item = &OntologyNode> {
        self.roots.iter().filter_map(|id| self.nodes.get(id))
    }

    /// Get node count
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get all paths
    pub fn all_paths(&self) -> impl Iterator<Item = &String> {
        self.path_index.keys()
    }
}

// ============================================================================
// Instance Path
// ============================================================================

/// Hierarchical path to an instance
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct InstancePath {
    /// Class path (e.g., "Entity/Spatial/Actor/Character/Player")
    pub class_path: String,
    /// Instance ID
    pub instance_id: Uuid,
    /// Parent instance path (for nested instances)
    pub parent: Option<Box<InstancePath>>,
}

impl InstancePath {
    /// Create a new instance path
    pub fn new(class_path: impl Into<String>, instance_id: Uuid) -> Self {
        Self {
            class_path: class_path.into(),
            instance_id,
            parent: None,
        }
    }

    /// Create with parent
    pub fn with_parent(mut self, parent: InstancePath) -> Self {
        self.parent = Some(Box::new(parent));
        self
    }

    /// Get full path string
    pub fn full_path(&self) -> String {
        if let Some(parent) = &self.parent {
            format!("{}/{}", parent.full_path(), self.class_path)
        } else {
            self.class_path.clone()
        }
    }

    /// Get depth in hierarchy
    pub fn depth(&self) -> usize {
        if let Some(parent) = &self.parent {
            1 + parent.depth()
        } else {
            0
        }
    }
}

// ============================================================================
// Ontology Index
// ============================================================================

/// Vector index with ontology awareness
pub struct OntologyIndex {
    /// The ontology tree
    ontology: OntologyTree,
    /// Per-class indices
    class_indices: HashMap<Uuid, EmbedvecIndex>,
    /// Global index (all instances)
    global_index: EmbedvecIndex,
    /// Instance to class mapping
    instance_classes: HashMap<Uuid, Uuid>,
    /// Embedding dimension
    dimension: usize,
}

impl OntologyIndex {
    /// Create a new ontology index
    pub fn new(ontology: OntologyTree, dimension: usize) -> Self {
        Self {
            ontology,
            class_indices: HashMap::new(),
            global_index: EmbedvecIndex::new(IndexConfig::default().with_dimension(dimension)),
            instance_classes: HashMap::new(),
            dimension,
        }
    }

    /// Create with Eustress base ontology
    pub fn with_eustress_base(dimension: usize) -> Self {
        Self::new(OntologyTree::with_eustress_base(), dimension)
    }

    /// Get the ontology tree
    pub fn ontology(&self) -> &OntologyTree {
        &self.ontology
    }

    /// Insert an instance
    pub fn insert(
        &mut self,
        class_path: &str,
        entity: Entity,
        instance_id: Uuid,
        embedding: Vec<f32>,
        metadata: EmbeddingMetadata,
    ) -> Result<()> {
        // Get class node
        let class_node = self.ontology.get_by_path(class_path)
            .ok_or_else(|| EmbedvecError::Index(format!("Unknown class: {}", class_path)))?;

        let class_id = class_node.id;

        // Ensure class index exists
        if !self.class_indices.contains_key(&class_id) {
            self.class_indices.insert(
                class_id,
                EmbedvecIndex::new(IndexConfig::default().with_dimension(self.dimension)),
            );
        }

        // Insert into class index
        if let Some(class_index) = self.class_indices.get_mut(&class_id) {
            class_index.upsert(entity, instance_id, embedding.clone(), metadata.clone())?;
        }

        // Insert into global index
        let mut global_metadata = metadata;
        global_metadata.properties.insert(
            "class_path".to_string(),
            serde_json::json!(class_path),
        );
        self.global_index.upsert(entity, instance_id, embedding, global_metadata)?;

        // Track instance class
        self.instance_classes.insert(instance_id, class_id);

        Ok(())
    }

    /// Search within a specific class and its descendants
    pub fn search_class(
        &self,
        class_path: &str,
        query: &[f32],
        k: usize,
        include_descendants: bool,
    ) -> Result<Vec<SearchResult>> {
        let class_node = self.ontology.get_by_path(class_path)
            .ok_or_else(|| EmbedvecError::Index(format!("Unknown class: {}", class_path)))?;

        let mut results = Vec::new();

        // Search in class index
        if let Some(class_index) = self.class_indices.get(&class_node.id) {
            results.extend(class_index.search(query, k)?);
        }

        // Search in descendant indices
        if include_descendants {
            for descendant in self.ontology.descendants(class_node.id) {
                if let Some(desc_index) = self.class_indices.get(&descendant.id) {
                    results.extend(desc_index.search(query, k)?);
                }
            }
        }

        // Sort by score and take top k
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);

        Ok(results)
    }

    /// Search globally
    pub fn search_global(&self, query: &[f32], k: usize) -> Result<Vec<SearchResult>> {
        self.global_index.search(query, k)
    }

    /// Search with class filter
    pub fn search_filtered<F>(
        &self,
        query: &[f32],
        k: usize,
        class_filter: F,
    ) -> Result<Vec<SearchResult>>
    where
        F: Fn(&str) -> bool,
    {
        self.global_index.search_filtered(query, k, |meta| {
            meta.properties
                .get("class_path")
                .and_then(|v| v.as_str())
                .map(|path| class_filter(path))
                .unwrap_or(false)
        })
    }

    /// Get instance count per class
    pub fn class_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();

        for (class_id, index) in &self.class_indices {
            if let Some(path) = self.ontology.path_for(*class_id) {
                stats.insert(path, index.len());
            }
        }

        stats
    }

    /// Get total instance count
    pub fn len(&self) -> usize {
        self.global_index.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.global_index.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ontology_tree() {
        let tree = OntologyTree::with_eustress_base();

        assert!(tree.get_by_name("Entity").is_some());
        assert!(tree.get_by_path("Entity/Spatial/Actor/Character/Player").is_some());

        let player = tree.get_by_name("Player").unwrap();
        let ancestors = tree.ancestors(player.id);
        assert_eq!(ancestors.len(), 4); // Character, Actor, Spatial, Entity

        let all_props = tree.all_properties(player.id);
        assert!(all_props.iter().any(|p| p.name == "name")); // Inherited from Entity
        assert!(all_props.iter().any(|p| p.name == "position")); // Inherited from Spatial
        assert!(all_props.iter().any(|p| p.name == "user_id")); // Own property
    }

    #[test]
    fn test_property_schema() {
        let prop = PropertySchema::new("health", PropertyType::Float)
            .required()
            .with_default(serde_json::json!(100.0))
            .with_description("Entity health points")
            .with_embed_weight(1.5);

        assert!(prop.required);
        assert_eq!(prop.embed_weight, 1.0); // Clamped to max
    }

    #[test]
    fn test_instance_path() {
        let parent = InstancePath::new("Entity/Spatial", Uuid::new_v4());
        let child = InstancePath::new("Actor/Character", Uuid::new_v4())
            .with_parent(parent);

        assert_eq!(child.depth(), 1);
        assert!(child.full_path().contains("Entity/Spatial"));
    }
}
