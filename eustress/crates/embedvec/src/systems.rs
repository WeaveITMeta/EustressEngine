//! Systems for automatic indexing and synchronization
//!
//! ## Table of Contents
//! 1. index_dirty_embeddings - Sync dirty EmbeddedComponents to index
//! 2. remove_despawned_entities - Clean up index when entities are removed
//! 3. auto_embed_reflected - Auto-embed entities with Reflect components

use crate::components::{EmbeddedComponent, EmbeddingMetadata};
use crate::resource::EmbedvecResource;
use bevy::prelude::*;

/// System that indexes entities with dirty EmbeddedComponent markers
pub fn index_dirty_embeddings(
    mut query: Query<(Entity, &mut EmbeddedComponent, Option<&Name>), Changed<EmbeddedComponent>>,
    embedvec: Option<Res<EmbedvecResource>>,
) {
    let Some(embedvec) = embedvec else {
        return;
    };

    for (entity, mut embedded, name) in query.iter_mut() {
        if !embedded.dirty {
            continue;
        }

        if embedded.embedding.is_empty() {
            continue;
        }

        let metadata = EmbeddingMetadata {
            name: name.map(|n| n.to_string()),
            component_types: vec!["EmbeddedComponent".to_string()],
            ..Default::default()
        };

        if let Err(e) = embedvec.write().upsert(
            entity,
            embedded.embedding_id,
            embedded.embedding.clone(),
            metadata,
        ) {
            tracing::warn!("Failed to index entity {:?}: {}", entity, e);
        } else {
            embedded.mark_clean();
            tracing::trace!("Indexed entity {:?} with embedding", entity);
        }
    }
}

/// System that removes despawned entities from the index
pub fn remove_despawned_entities(
    mut removed: RemovedComponents<EmbeddedComponent>,
    embedvec: Option<Res<EmbedvecResource>>,
) {
    let Some(embedvec) = embedvec else {
        return;
    };

    for entity in removed.read() {
        if let Err(e) = embedvec.write().remove(entity) {
            tracing::trace!("Entity {:?} not in index (already removed?): {}", entity, e);
        } else {
            tracing::trace!("Removed entity {:?} from index", entity);
        }
    }
}

/// Marker component for entities that should be auto-embedded via Reflect
#[derive(Component, Default, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct AutoEmbed {
    /// Component type names to embed (empty = all reflected components)
    pub include_components: Vec<String>,
    /// Component type names to exclude
    pub exclude_components: Vec<String>,
    /// Whether to re-embed on any component change
    pub auto_update: bool,
}

impl AutoEmbed {
    /// Create with auto-update enabled
    pub fn with_auto_update() -> Self {
        Self {
            auto_update: true,
            ..Default::default()
        }
    }

    /// Include specific component types
    pub fn include(mut self, component: impl Into<String>) -> Self {
        self.include_components.push(component.into());
        self
    }

    /// Exclude specific component types
    pub fn exclude(mut self, component: impl Into<String>) -> Self {
        self.exclude_components.push(component.into());
        self
    }
}

/// System that auto-embeds entities marked with AutoEmbed using Reflect
/// This is a simplified version - full implementation would use the type registry
pub fn auto_embed_reflected(
    mut commands: Commands,
    query: Query<(Entity, &AutoEmbed, Option<&Name>), Without<EmbeddedComponent>>,
    embedvec: Option<Res<EmbedvecResource>>,
) {
    let Some(embedvec) = embedvec else {
        return;
    };

    for (entity, _auto_embed, name) in query.iter() {
        // Create a simple embedding based on entity ID and name
        // Full implementation would iterate over reflected components
        let mut properties = std::collections::HashMap::new();

        properties.insert(
            "entity_id".to_string(),
            serde_json::json!(entity.to_bits()),
        );

        if let Some(name) = name {
            properties.insert("name".to_string(), serde_json::json!(name.to_string()));
        }

        match embedvec.embedder().embed_properties(&properties) {
            Ok(embedding) => {
                commands.entity(entity).insert(EmbeddedComponent::new(embedding));
                tracing::trace!("Auto-embedded entity {:?}", entity);
            }
            Err(e) => {
                tracing::warn!("Failed to auto-embed entity {:?}: {}", entity, e);
            }
        }
    }
}

/// System set for embedvec systems
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmbedvecSet {
    /// Auto-embedding of new entities
    AutoEmbed,
    /// Indexing of dirty embeddings
    Index,
    /// Cleanup of removed entities
    Cleanup,
}

/// Query helper for semantic entity search
pub struct SemanticQuery<'w> {
    embedvec: &'w EmbedvecResource,
}

impl<'w> SemanticQuery<'w> {
    /// Create a new semantic query helper
    pub fn new(embedvec: &'w EmbedvecResource) -> Self {
        Self { embedvec }
    }

    /// Find entities matching a natural language query
    pub fn find(&self, query: &str, k: usize) -> Vec<Entity> {
        self.embedvec
            .query(query, k)
            .map(|results| results.into_iter().map(|r| r.entity).collect())
            .unwrap_or_default()
    }

    /// Find entities matching a query with a tag filter
    pub fn find_with_tag(&self, query: &str, tag: &str, k: usize) -> Vec<Entity> {
        let tag = tag.to_string();
        self.embedvec
            .query_filtered(query, k, |meta| meta.has_tag(&tag))
            .map(|results| results.into_iter().map(|r| r.entity).collect())
            .unwrap_or_default()
    }

    /// Find entities similar to a given entity
    pub fn find_similar(&self, entity: Entity, k: usize) -> Vec<Entity> {
        self.embedvec
            .read()
            .find_similar(entity, k)
            .map(|results| results.into_iter().map(|r| r.entity).collect())
            .unwrap_or_default()
    }
}
