//! Components for entity embeddings
//!
//! ## Table of Contents
//! 1. EmbeddedComponent - Marker + embedding data for indexed entities
//! 2. EmbeddingMetadata - Metadata stored alongside embeddings

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Component marking an entity as indexed in embedvec with its embedding vector
#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct EmbeddedComponent {
    /// Unique ID for this embedding (stable across sessions)
    #[reflect(ignore)]
    pub embedding_id: Uuid,
    /// The embedding vector (cached for quick access)
    pub embedding: Vec<f32>,
    /// Whether this embedding needs re-indexing
    pub dirty: bool,
    /// Last update timestamp (milliseconds since epoch)
    pub last_updated: u64,
}

impl Default for EmbeddedComponent {
    fn default() -> Self {
        Self {
            embedding_id: Uuid::new_v4(),
            embedding: Vec::new(),
            dirty: true,
            last_updated: 0,
        }
    }
}

impl EmbeddedComponent {
    /// Create a new embedded component with the given embedding
    pub fn new(embedding: Vec<f32>) -> Self {
        Self {
            embedding_id: Uuid::new_v4(),
            embedding,
            dirty: true,
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    /// Mark the embedding as needing re-indexing
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Mark the embedding as up-to-date
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    /// Update the embedding vector
    pub fn set_embedding(&mut self, embedding: Vec<f32>) {
        self.embedding = embedding;
        self.mark_dirty();
    }
}

/// Metadata stored alongside embeddings in the index
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EmbeddingMetadata {
    /// Entity name (if available)
    pub name: Option<String>,
    /// Component type names that contributed to this embedding
    pub component_types: Vec<String>,
    /// Arbitrary key-value properties for filtering
    pub properties: HashMap<String, serde_json::Value>,
    /// Tags for categorical filtering
    pub tags: Vec<String>,
}

impl EmbeddingMetadata {
    /// Create new metadata with a name
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Default::default()
        }
    }

    /// Add a component type
    pub fn with_component(mut self, component_type: impl Into<String>) -> Self {
        self.component_types.push(component_type.into());
        self
    }

    /// Add a property
    pub fn with_property(
        mut self,
        key: impl Into<String>,
        value: impl Serialize,
    ) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.properties.insert(key.into(), v);
        }
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Check if metadata matches a filter predicate
    pub fn matches<F>(&self, filter: F) -> bool
    where
        F: Fn(&EmbeddingMetadata) -> bool,
    {
        filter(self)
    }

    /// Check if metadata has a specific tag
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Get a property value
    pub fn get_property<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.properties
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}
