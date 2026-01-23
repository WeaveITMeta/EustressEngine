//! EmbedvecResource - Bevy Resource wrapping the embedvec index
//!
//! ## Table of Contents
//! 1. EmbedvecIndex - Core index wrapper
//! 2. EmbedvecResource - Bevy Resource for the index
//! 3. SearchResult - Query result type
//! 4. IndexConfig - Configuration for the index

use crate::components::EmbeddingMetadata;
use crate::embedder::PropertyEmbedder;
use crate::error::{EmbedvecError, Result};
use bevy::prelude::*;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Configuration for the embedvec index
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Embedding dimension
    pub dimension: usize,
    /// HNSW M parameter (number of connections per layer)
    pub m: usize,
    /// HNSW ef_construction parameter (search width during construction)
    pub ef_construction: usize,
    /// Path for persistence (if enabled)
    pub persistence_path: Option<String>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            dimension: 128,
            m: 16,
            ef_construction: 200,
            persistence_path: None,
        }
    }
}

impl IndexConfig {
    /// Create config with custom dimension
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = dimension;
        self
    }

    /// Enable persistence at the given path
    pub fn with_persistence(mut self, path: impl Into<String>) -> Self {
        self.persistence_path = Some(path.into());
        self
    }
}

/// Result from a similarity search
#[derive(Clone, Debug)]
pub struct SearchResult {
    /// Entity associated with this result
    pub entity: Entity,
    /// Embedding ID
    pub embedding_id: Uuid,
    /// Similarity score (higher = more similar)
    pub score: f32,
    /// Metadata associated with this embedding
    pub metadata: EmbeddingMetadata,
}

/// Internal entry in the index
#[derive(Clone, Debug, Serialize, Deserialize)]
struct IndexEntry {
    entity_bits: u64,
    embedding_id: Uuid,
    embedding: Vec<f32>,
    metadata: EmbeddingMetadata,
}

/// Core index wrapper around embedvec
/// Uses a simple in-memory HNSW-like structure for now
/// TODO: Replace with actual embedvec crate when API is confirmed
pub struct EmbedvecIndex {
    config: IndexConfig,
    /// Entity -> entry mapping
    entries: HashMap<Entity, IndexEntry>,
    /// Embedding ID -> Entity mapping for lookups
    id_to_entity: HashMap<Uuid, Entity>,
}

impl EmbedvecIndex {
    /// Create a new index with the given configuration
    pub fn new(config: IndexConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
            id_to_entity: HashMap::new(),
        }
    }

    /// Get the index configuration
    pub fn config(&self) -> &IndexConfig {
        &self.config
    }

    /// Get the number of entries in the index
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert or update an embedding for an entity
    pub fn upsert(
        &mut self,
        entity: Entity,
        embedding_id: Uuid,
        embedding: Vec<f32>,
        metadata: EmbeddingMetadata,
    ) -> Result<()> {
        if embedding.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: embedding.len(),
            });
        }

        let entry = IndexEntry {
            entity_bits: entity.to_bits(),
            embedding_id,
            embedding,
            metadata,
        };

        // Remove old ID mapping if updating
        if let Some(old_entry) = self.entries.get(&entity) {
            self.id_to_entity.remove(&old_entry.embedding_id);
        }

        self.id_to_entity.insert(embedding_id, entity);
        self.entries.insert(entity, entry);

        Ok(())
    }

    /// Remove an entity from the index
    pub fn remove(&mut self, entity: Entity) -> Result<()> {
        if let Some(entry) = self.entries.remove(&entity) {
            self.id_to_entity.remove(&entry.embedding_id);
            Ok(())
        } else {
            Err(EmbedvecError::EntityNotFound(entity))
        }
    }

    /// Check if an entity is in the index
    pub fn contains(&self, entity: Entity) -> bool {
        self.entries.contains_key(&entity)
    }

    /// Get the embedding for an entity
    pub fn get_embedding(&self, entity: Entity) -> Option<&[f32]> {
        self.entries.get(&entity).map(|e| e.embedding.as_slice())
    }

    /// Get metadata for an entity
    pub fn get_metadata(&self, entity: Entity) -> Option<&EmbeddingMetadata> {
        self.entries.get(&entity).map(|e| &e.metadata)
    }

    /// Search for similar embeddings
    /// Returns up to k results sorted by similarity (descending)
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchResult>> {
        if query.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: query.len(),
            });
        }

        let mut results: Vec<_> = self
            .entries
            .iter()
            .map(|(entity, entry)| {
                let score = cosine_similarity(query, &entry.embedding);
                SearchResult {
                    entity: *entity,
                    embedding_id: entry.embedding_id,
                    score,
                    metadata: entry.metadata.clone(),
                }
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Take top k
        results.truncate(k);

        Ok(results)
    }

    /// Search with a metadata filter
    pub fn search_filtered<F>(
        &self,
        query: &[f32],
        k: usize,
        filter: F,
    ) -> Result<Vec<SearchResult>>
    where
        F: Fn(&EmbeddingMetadata) -> bool,
    {
        if query.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: query.len(),
            });
        }

        let mut results: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| filter(&entry.metadata))
            .map(|(entity, entry)| {
                let score = cosine_similarity(query, &entry.embedding);
                SearchResult {
                    entity: *entity,
                    embedding_id: entry.embedding_id,
                    score,
                    metadata: entry.metadata.clone(),
                }
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);

        Ok(results)
    }

    /// Find entities similar to a given entity
    pub fn find_similar(&self, entity: Entity, k: usize) -> Result<Vec<SearchResult>> {
        let embedding = self
            .get_embedding(entity)
            .ok_or(EmbedvecError::EntityNotFound(entity))?
            .to_vec();

        let mut results = self.search(&embedding, k + 1)?;

        // Remove the query entity from results
        results.retain(|r| r.entity != entity);
        results.truncate(k);

        Ok(results)
    }

    /// Clear all entries from the index
    pub fn clear(&mut self) {
        self.entries.clear();
        self.id_to_entity.clear();
    }
}

/// Compute cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a < 1e-10 || norm_b < 1e-10 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Bevy Resource wrapping the embedvec index with thread-safe access
#[derive(Resource)]
pub struct EmbedvecResource {
    /// The underlying index (thread-safe)
    index: Arc<RwLock<EmbedvecIndex>>,
    /// The embedder used for this index
    embedder: Arc<dyn PropertyEmbedder>,
}

impl EmbedvecResource {
    /// Create a new resource with the given config and embedder
    pub fn new<E: PropertyEmbedder>(config: IndexConfig, embedder: E) -> Self {
        Self {
            index: Arc::new(RwLock::new(EmbedvecIndex::new(config))),
            embedder: Arc::new(embedder),
        }
    }

    /// Get read access to the index
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, EmbedvecIndex> {
        self.index.read()
    }

    /// Get write access to the index
    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, EmbedvecIndex> {
        self.index.write()
    }

    /// Get the embedder
    pub fn embedder(&self) -> &dyn PropertyEmbedder {
        self.embedder.as_ref()
    }

    /// Embed and insert properties for an entity
    pub fn embed_and_insert(
        &self,
        entity: Entity,
        embedding_id: Uuid,
        properties: &std::collections::HashMap<String, serde_json::Value>,
        metadata: EmbeddingMetadata,
    ) -> Result<()> {
        let embedding = self.embedder.embed_properties(properties)?;
        self.write().upsert(entity, embedding_id, embedding, metadata)
    }

    /// Embed a query and search
    pub fn query(&self, query: &str, k: usize) -> Result<Vec<SearchResult>> {
        let embedding = self.embedder.embed_query(query)?;
        self.read().search(&embedding, k)
    }

    /// Embed a query and search with filter
    pub fn query_filtered<F>(&self, query: &str, k: usize, filter: F) -> Result<Vec<SearchResult>>
    where
        F: Fn(&EmbeddingMetadata) -> bool,
    {
        let embedding = self.embedder.embed_query(query)?;
        self.read().search_filtered(&embedding, k, filter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::SimpleHashEmbedder;

    #[test]
    fn test_index_operations() {
        let config = IndexConfig::default().with_dimension(64);
        let mut index = EmbedvecIndex::new(config);

        let entity = Entity::from_bits(1);
        let embedding = vec![0.1f32; 64];
        let metadata = EmbeddingMetadata::with_name("Test");

        index
            .upsert(entity, Uuid::new_v4(), embedding.clone(), metadata)
            .unwrap();

        assert!(index.contains(entity));
        assert_eq!(index.len(), 1);

        let results = index.search(&embedding, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].score > 0.99); // Should be ~1.0 for same vector
    }

    #[test]
    fn test_resource_query() {
        let config = IndexConfig::default().with_dimension(64);
        let embedder = SimpleHashEmbedder::new(64);
        let resource = EmbedvecResource::new(config, embedder);

        let entity = Entity::from_bits(1);
        let mut props = std::collections::HashMap::new();
        props.insert("health".to_string(), serde_json::json!(100));
        props.insert("class".to_string(), serde_json::json!("warrior"));

        resource
            .embed_and_insert(
                entity,
                Uuid::new_v4(),
                &props,
                EmbeddingMetadata::with_name("Player"),
            )
            .unwrap();

        let results = resource.query("warrior health", 5).unwrap();
        assert!(!results.is_empty());
    }
}
