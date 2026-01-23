//! Persistence - Sled-based storage for embedvec indices
//!
//! ## Table of Contents
//! 1. PersistentIndex - Sled-backed vector index
//! 2. PersistenceConfig - Configuration for storage
//! 3. Serialization helpers

use crate::components::EmbeddingMetadata;
use crate::error::{EmbedvecError, Result};
use crate::resource::{IndexConfig, SearchResult};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Configuration for persistent storage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistenceConfig {
    /// Path to the Sled database directory
    pub path: String,
    /// Cache size in bytes (default: 1GB)
    pub cache_size: u64,
    /// Flush interval in milliseconds (0 = manual flush only)
    pub flush_interval_ms: u64,
    /// Enable compression
    pub compression: bool,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            path: "./embedvec_db".to_string(),
            cache_size: 1024 * 1024 * 1024, // 1GB
            flush_interval_ms: 500,
            compression: true,
        }
    }
}

impl PersistenceConfig {
    /// Create config with custom path
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Set cache size
    pub fn with_cache_size(mut self, size: u64) -> Self {
        self.cache_size = size;
        self
    }

    /// Set flush interval
    pub fn with_flush_interval(mut self, ms: u64) -> Self {
        self.flush_interval_ms = ms;
        self
    }
}

/// Serializable entry for storage
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredEntry {
    entity_bits: u64,
    embedding_id: Uuid,
    embedding: Vec<f32>,
    metadata: EmbeddingMetadata,
    /// Ontology class path (if applicable)
    class_path: Option<String>,
}

/// Persistent vector index backed by Sled
pub struct PersistentIndex {
    /// Sled database
    db: sled::Db,
    /// Embeddings tree
    embeddings: sled::Tree,
    /// Entity to embedding ID mapping
    entity_index: sled::Tree,
    /// Class path index (for ontology queries)
    class_index: sled::Tree,
    /// Metadata tree
    metadata: sled::Tree,
    /// Index configuration
    config: IndexConfig,
    /// In-memory cache for fast search
    cache: HashMap<Uuid, StoredEntry>,
    /// Whether cache is dirty
    cache_dirty: bool,
}

impl PersistentIndex {
    /// Open or create a persistent index at the given path
    pub fn open(index_config: IndexConfig, persistence_config: PersistenceConfig) -> Result<Self> {
        let db = sled::Config::new()
            .path(&persistence_config.path)
            .cache_capacity(persistence_config.cache_size)
            .flush_every_ms(if persistence_config.flush_interval_ms > 0 {
                Some(persistence_config.flush_interval_ms)
            } else {
                None
            })
            .use_compression(persistence_config.compression)
            .open()
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to open Sled DB: {}", e)))?;

        let embeddings = db
            .open_tree("embeddings")
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to open embeddings tree: {}", e)))?;

        let entity_index = db
            .open_tree("entity_index")
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to open entity_index tree: {}", e)))?;

        let class_index = db
            .open_tree("class_index")
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to open class_index tree: {}", e)))?;

        let metadata = db
            .open_tree("metadata")
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to open metadata tree: {}", e)))?;

        // Store config in metadata
        let config_bytes = serde_json::to_vec(&index_config)
            .map_err(|e| EmbedvecError::Serialization(e.to_string()))?;
        metadata
            .insert("config", config_bytes)
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to store config: {}", e)))?;

        let mut index = Self {
            db,
            embeddings,
            entity_index,
            class_index,
            metadata,
            config: index_config,
            cache: HashMap::new(),
            cache_dirty: false,
        };

        // Load existing entries into cache
        index.load_cache()?;

        Ok(index)
    }

    /// Load all entries into memory cache for fast search
    fn load_cache(&mut self) -> Result<()> {
        self.cache.clear();

        for result in self.embeddings.iter() {
            let (key, value) = result
                .map_err(|e| EmbedvecError::Persistence(format!("Failed to iterate: {}", e)))?;

            let embedding_id = Uuid::from_slice(&key)
                .map_err(|e| EmbedvecError::Persistence(format!("Invalid UUID: {}", e)))?;

            let entry: StoredEntry = serde_json::from_slice(&value)
                .map_err(|e| EmbedvecError::Serialization(e.to_string()))?;

            self.cache.insert(embedding_id, entry);
        }

        tracing::info!(count = self.cache.len(), "Loaded embeddings from persistent storage");
        Ok(())
    }

    /// Get the index configuration
    pub fn config(&self) -> &IndexConfig {
        &self.config
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Insert or update an embedding
    pub fn upsert(
        &mut self,
        entity: Entity,
        embedding_id: Uuid,
        embedding: Vec<f32>,
        metadata: EmbeddingMetadata,
        class_path: Option<String>,
    ) -> Result<()> {
        if embedding.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: embedding.len(),
            });
        }

        let entry = StoredEntry {
            entity_bits: entity.to_bits(),
            embedding_id,
            embedding,
            metadata,
            class_path: class_path.clone(),
        };

        // Serialize entry
        let entry_bytes = serde_json::to_vec(&entry)
            .map_err(|e| EmbedvecError::Serialization(e.to_string()))?;

        // Store in embeddings tree
        self.embeddings
            .insert(embedding_id.as_bytes(), entry_bytes)
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to insert embedding: {}", e)))?;

        // Update entity index
        self.entity_index
            .insert(entity.to_bits().to_be_bytes(), embedding_id.as_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to update entity index: {}", e)))?;

        // Update class index if applicable
        if let Some(path) = &class_path {
            let class_key = format!("{}:{}", path, embedding_id);
            self.class_index
                .insert(class_key.as_bytes(), embedding_id.as_bytes())
                .map_err(|e| EmbedvecError::Persistence(format!("Failed to update class index: {}", e)))?;
        }

        // Update cache
        self.cache.insert(embedding_id, entry);
        self.cache_dirty = true;

        Ok(())
    }

    /// Remove an entity from the index
    pub fn remove(&mut self, entity: Entity) -> Result<()> {
        // Find embedding ID for entity
        let entity_key = entity.to_bits().to_be_bytes();
        let embedding_id_bytes = self
            .entity_index
            .remove(&entity_key)
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to remove from entity index: {}", e)))?
            .ok_or(EmbedvecError::EntityNotFound(entity))?;

        let embedding_id = Uuid::from_slice(&embedding_id_bytes)
            .map_err(|e| EmbedvecError::Persistence(format!("Invalid UUID: {}", e)))?;

        // Get entry for class path
        if let Some(entry) = self.cache.get(&embedding_id) {
            if let Some(path) = &entry.class_path {
                let class_key = format!("{}:{}", path, embedding_id);
                let _ = self.class_index.remove(class_key.as_bytes());
            }
        }

        // Remove from embeddings
        self.embeddings
            .remove(embedding_id.as_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to remove embedding: {}", e)))?;

        // Remove from cache
        self.cache.remove(&embedding_id);
        self.cache_dirty = true;

        Ok(())
    }

    /// Check if an entity is in the index
    pub fn contains(&self, entity: Entity) -> bool {
        self.entity_index
            .contains_key(entity.to_bits().to_be_bytes())
            .unwrap_or(false)
    }

    /// Get the embedding for an entity
    pub fn get_embedding(&self, entity: Entity) -> Option<Vec<f32>> {
        let entity_key = entity.to_bits().to_be_bytes();
        let embedding_id_bytes = self.entity_index.get(&entity_key).ok()??;
        let embedding_id = Uuid::from_slice(&embedding_id_bytes).ok()?;
        self.cache.get(&embedding_id).map(|e| e.embedding.clone())
    }

    /// Get metadata for an entity
    pub fn get_metadata(&self, entity: Entity) -> Option<EmbeddingMetadata> {
        let entity_key = entity.to_bits().to_be_bytes();
        let embedding_id_bytes = self.entity_index.get(&entity_key).ok()??;
        let embedding_id = Uuid::from_slice(&embedding_id_bytes).ok()?;
        self.cache.get(&embedding_id).map(|e| e.metadata.clone())
    }

    /// Search for similar embeddings
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchResult>> {
        if query.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: query.len(),
            });
        }

        let mut results: Vec<_> = self
            .cache
            .values()
            .map(|entry| {
                let score = cosine_similarity(query, &entry.embedding);
                SearchResult {
                    entity: Entity::from_bits(entry.entity_bits),
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

    /// Search with a metadata filter
    pub fn search_filtered<F>(&self, query: &[f32], k: usize, filter: F) -> Result<Vec<SearchResult>>
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
            .cache
            .values()
            .filter(|entry| filter(&entry.metadata))
            .map(|entry| {
                let score = cosine_similarity(query, &entry.embedding);
                SearchResult {
                    entity: Entity::from_bits(entry.entity_bits),
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

    /// Search within a specific ontology class path
    pub fn search_by_class(&self, query: &[f32], class_path: &str, k: usize) -> Result<Vec<SearchResult>> {
        if query.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: query.len(),
            });
        }

        let mut results: Vec<_> = self
            .cache
            .values()
            .filter(|entry| {
                entry.class_path.as_ref().map(|p| p.starts_with(class_path)).unwrap_or(false)
            })
            .map(|entry| {
                let score = cosine_similarity(query, &entry.embedding);
                SearchResult {
                    entity: Entity::from_bits(entry.entity_bits),
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

    /// Flush all pending writes to disk
    pub fn flush(&self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to flush: {}", e)))?;
        tracing::debug!("Flushed embedvec database to disk");
        Ok(())
    }

    /// Flush asynchronously
    pub async fn flush_async(&self) -> Result<()> {
        self.db
            .flush_async()
            .await
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to async flush: {}", e)))?;
        Ok(())
    }

    /// Get database size on disk
    pub fn size_on_disk(&self) -> Result<u64> {
        self.db
            .size_on_disk()
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to get size: {}", e)))
    }

    /// Clear all data
    pub fn clear(&mut self) -> Result<()> {
        self.embeddings
            .clear()
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to clear embeddings: {}", e)))?;
        self.entity_index
            .clear()
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to clear entity_index: {}", e)))?;
        self.class_index
            .clear()
            .map_err(|e| EmbedvecError::Persistence(format!("Failed to clear class_index: {}", e)))?;
        self.cache.clear();
        self.cache_dirty = false;
        Ok(())
    }

    /// Get statistics about the index
    pub fn stats(&self) -> IndexStats {
        let mut class_counts: HashMap<String, usize> = HashMap::new();

        for entry in self.cache.values() {
            if let Some(path) = &entry.class_path {
                *class_counts.entry(path.clone()).or_insert(0) += 1;
            }
        }

        IndexStats {
            total_entries: self.cache.len(),
            dimension: self.config.dimension,
            size_on_disk: self.size_on_disk().unwrap_or(0),
            class_counts,
        }
    }
}

/// Statistics about the index
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexStats {
    /// Total number of entries
    pub total_entries: usize,
    /// Embedding dimension
    pub dimension: usize,
    /// Size on disk in bytes
    pub size_on_disk: u64,
    /// Entry count per ontology class path
    pub class_counts: HashMap<String, usize>,
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

// ============================================================================
// Persistent Ontology Index
// ============================================================================

/// Persistent ontology-aware vector index backed by Sled
pub struct PersistentOntologyIndex {
    /// The underlying persistent index
    index: PersistentIndex,
    /// The ontology tree (in-memory, loaded from DB)
    ontology: crate::ontology::OntologyTree,
}

impl PersistentOntologyIndex {
    /// Open or create a persistent ontology index
    pub fn open(
        ontology: crate::ontology::OntologyTree,
        index_config: IndexConfig,
        persistence_config: PersistenceConfig,
    ) -> Result<Self> {
        let index = PersistentIndex::open(index_config, persistence_config)?;

        Ok(Self { index, ontology })
    }

    /// Open with Eustress base ontology
    pub fn with_eustress_base(
        index_config: IndexConfig,
        persistence_config: PersistenceConfig,
    ) -> Result<Self> {
        Self::open(
            crate::ontology::OntologyTree::with_eustress_base(),
            index_config,
            persistence_config,
        )
    }

    /// Get the ontology tree
    pub fn ontology(&self) -> &crate::ontology::OntologyTree {
        &self.ontology
    }

    /// Insert an instance with ontology class path
    pub fn insert(
        &mut self,
        class_path: &str,
        entity: Entity,
        instance_id: Uuid,
        embedding: Vec<f32>,
        metadata: EmbeddingMetadata,
    ) -> Result<()> {
        // Validate class path exists in ontology
        if self.ontology.get_by_path(class_path).is_none() {
            return Err(EmbedvecError::Index(format!("Unknown class path: {}", class_path)));
        }

        self.index.upsert(entity, instance_id, embedding, metadata, Some(class_path.to_string()))
    }

    /// Remove an entity
    pub fn remove(&mut self, entity: Entity) -> Result<()> {
        self.index.remove(entity)
    }

    /// Search within a specific class and its descendants
    pub fn search_class(
        &self,
        class_path: &str,
        query: &[f32],
        k: usize,
        include_descendants: bool,
    ) -> Result<Vec<SearchResult>> {
        if include_descendants {
            // Search by prefix match on class path
            self.index.search_by_class(query, class_path, k)
        } else {
            // Exact class match
            self.index.search_filtered(query, k, |meta| {
                meta.properties
                    .get("class_path")
                    .and_then(|v| v.as_str())
                    .map(|p| p == class_path)
                    .unwrap_or(false)
            })
        }
    }

    /// Search globally
    pub fn search_global(&self, query: &[f32], k: usize) -> Result<Vec<SearchResult>> {
        self.index.search(query, k)
    }

    /// Get instance count
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Flush to disk
    pub fn flush(&self) -> Result<()> {
        self.index.flush()
    }

    /// Flush asynchronously
    pub async fn flush_async(&self) -> Result<()> {
        self.index.flush_async().await
    }

    /// Get statistics
    pub fn stats(&self) -> IndexStats {
        self.index.stats()
    }

    /// Clear all data
    pub fn clear(&mut self) -> Result<()> {
        self.index.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_persistent_index() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();

        let index_config = IndexConfig::default().with_dimension(64);
        let persistence_config = PersistenceConfig::default().with_path(path);

        let mut index = PersistentIndex::open(index_config, persistence_config).unwrap();

        let entity = Entity::from_bits(1);
        let embedding = vec![0.1f32; 64];
        let metadata = EmbeddingMetadata::with_name("Test");

        index
            .upsert(entity, Uuid::new_v4(), embedding.clone(), metadata, Some("Entity/Spatial/Prop".to_string()))
            .unwrap();

        assert!(index.contains(entity));
        assert_eq!(index.len(), 1);

        let results = index.search(&embedding, 5).unwrap();
        assert_eq!(results.len(), 1);

        index.flush().unwrap();
    }

    #[test]
    fn test_persistence_reload() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();

        let entity = Entity::from_bits(42);
        let embedding_id = Uuid::new_v4();

        // Create and populate index
        {
            let index_config = IndexConfig::default().with_dimension(32);
            let persistence_config = PersistenceConfig::default().with_path(&path);

            let mut index = PersistentIndex::open(index_config, persistence_config).unwrap();

            let embedding = vec![0.5f32; 32];
            let metadata = EmbeddingMetadata::with_name("Persistent");

            index
                .upsert(entity, embedding_id, embedding, metadata, None)
                .unwrap();

            index.flush().unwrap();
        }

        // Reopen and verify data persisted
        {
            let index_config = IndexConfig::default().with_dimension(32);
            let persistence_config = PersistenceConfig::default().with_path(&path);

            let index = PersistentIndex::open(index_config, persistence_config).unwrap();

            assert_eq!(index.len(), 1);
            assert!(index.contains(entity));
        }
    }
}
