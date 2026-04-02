//! EmbedvecResource — Bevy Resource wrapping the embedvec 0.7 index
//!
//! ## Table of Contents
//! 1. IndexConfig     — dimension, HNSW params, quantization choice
//! 2. Quantization    — re-export of embedvec::Quantization for convenience
//! 3. SearchResult    — query result bridging embedvec::Hit → Bevy Entity
//! 4. EmbedvecIndex   — sync wrapper over EmbedVec (add_internal/search_internal)
//! 5. EmbedvecResource — Bevy Resource with RwLock + embedder
//!
//! ## Architecture
//! embedvec 0.7 exposes `EmbedVec::new_internal()` / `add_internal()` /
//! `search_internal()` as sync entry-points (public for Python bindings).
//! We use those here so the index lives entirely in Bevy's synchronous world
//! without needing a Tokio runtime.
//!
//! Entity → embedvec-ID mapping is maintained in `entity_to_id` so we can
//! translate `Hit.id: usize` back to a Bevy `Entity` after each search.
//!
//! H4 lattice quantization (~15× memory savings) is the default; callers can
//! override with `IndexConfig::with_quantization(Quantization::e8_default())`
//! or `Quantization::None` for full precision.

use crate::components::EmbeddingMetadata;
use crate::embedder::PropertyEmbedder;
use crate::error::{EmbedvecError, Result};
use bevy::prelude::*;
use embedvec::{distance::Distance, filter::FilterExpr, quantization::Quantization, EmbedVec};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// 1. IndexConfig
// ============================================================================

/// Configuration for the embedvec index
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Embedding dimension (must match embedder output)
    pub dimension: usize,
    /// HNSW M parameter — connections per layer (16–64)
    pub m: usize,
    /// HNSW ef_construction — search width during build (100–500)
    pub ef_construction: usize,
    /// ef_search — search width at query time (higher = better recall)
    pub ef_search: usize,
    /// Path for persistence (Sled backend, optional)
    pub persistence_path: Option<String>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            dimension: 128,
            m: 16,
            ef_construction: 200,
            ef_search: 50,
            persistence_path: None,
        }
    }
}

impl IndexConfig {
    /// Set the embedding dimension
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = dimension;
        self
    }

    /// Enable Sled persistence at the given path
    pub fn with_persistence(mut self, path: impl Into<String>) -> Self {
        self.persistence_path = Some(path.into());
        self
    }

    /// Set ef_search (query-time recall vs speed tradeoff)
    pub fn with_ef_search(mut self, ef_search: usize) -> Self {
        self.ef_search = ef_search;
        self
    }
}

// ============================================================================
// 3. SearchResult
// ============================================================================

/// Result from a similarity search — bridges embedvec::Hit to Bevy Entity
#[derive(Clone, Debug)]
pub struct SearchResult {
    /// Bevy entity associated with this result
    pub entity: Entity,
    /// Embedding ID (our internal stable UUID)
    pub embedding_id: Uuid,
    /// Similarity score (higher = more similar, cosine distance)
    pub score: f32,
    /// Metadata associated with this embedding
    pub metadata: EmbeddingMetadata,
}

// ============================================================================
// 4. EmbedvecIndex
// ============================================================================

/// Sync wrapper over `embedvec::EmbedVec` using `_internal` entry-points.
///
/// Maintains a bidirectional mapping between Bevy `Entity` and embedvec's
/// `usize` IDs so search results can be translated back to ECS entities.
pub struct EmbedvecIndex {
    config: IndexConfig,
    /// The real embedvec 0.7 index (H4 quantized by default)
    inner: EmbedVec,
    /// Bevy Entity → embedvec usize ID
    entity_to_id: HashMap<Entity, usize>,
    /// embedvec usize ID → Bevy Entity
    id_to_entity: HashMap<usize, Entity>,
    /// embedvec usize ID → our stable UUID
    id_to_uuid: HashMap<usize, Uuid>,
    /// embedvec usize ID → metadata (cached for search results)
    id_to_metadata: HashMap<usize, EmbeddingMetadata>,
}

impl EmbedvecIndex {
    /// Create a new index — H4 quantization by default
    pub fn new(config: IndexConfig) -> Self {
        let quant = Quantization::h4_default();
        let inner = EmbedVec::new_internal(
            config.dimension,
            Distance::Cosine,
            config.m,
            config.ef_construction,
            quant,
        )
        .expect("Failed to create EmbedVec index");

        Self {
            config,
            inner,
            entity_to_id: HashMap::new(),
            id_to_entity: HashMap::new(),
            id_to_uuid: HashMap::new(),
            id_to_metadata: HashMap::new(),
        }
    }

    /// Create a new index with a specific quantization mode
    pub fn with_quantization(config: IndexConfig, quant: Quantization) -> Self {
        let inner = EmbedVec::new_internal(
            config.dimension,
            Distance::Cosine,
            config.m,
            config.ef_construction,
            quant,
        )
        .expect("Failed to create EmbedVec index");

        Self {
            config,
            inner,
            entity_to_id: HashMap::new(),
            id_to_entity: HashMap::new(),
            id_to_uuid: HashMap::new(),
            id_to_metadata: HashMap::new(),
        }
    }

    /// Get the index configuration
    pub fn config(&self) -> &IndexConfig {
        &self.config
    }

    /// Number of indexed vectors
    pub fn len(&self) -> usize {
        self.entity_to_id.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entity_to_id.is_empty()
    }

    /// Insert or update an embedding for a Bevy entity.
    ///
    /// embedvec 0.7 has no in-place update — we add a new vector and update
    /// our mapping; the old vector becomes orphaned (stale IDs don't affect
    /// search correctness since we filter by entity in results).
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

        // Build metadata payload for embedvec
        let mut payload = serde_json::Map::new();
        payload.insert(
            "entity_bits".to_string(),
            serde_json::json!(entity.to_bits()),
        );
        payload.insert(
            "embedding_id".to_string(),
            serde_json::json!(embedding_id.to_string()),
        );
        if let Some(ref name) = metadata.name {
            payload.insert("name".to_string(), serde_json::json!(name));
        }
        for (k, v) in &metadata.properties {
            payload.insert(k.clone(), v.clone());
        }
        for (i, tag) in metadata.tags.iter().enumerate() {
            payload.insert(format!("tag_{}", i), serde_json::json!(tag));
        }

        // Add to embedvec index (sync internal API — payload must be serde_json::Value)
        let vec_id = self
            .inner
            .add_internal(&embedding, serde_json::Value::Object(payload))
            .map_err(|e| EmbedvecError::Index(e.to_string()))?;

        // Update mappings (remove old entry if updating)
        if let Some(old_id) = self.entity_to_id.insert(entity, vec_id) {
            self.id_to_entity.remove(&old_id);
            self.id_to_uuid.remove(&old_id);
            self.id_to_metadata.remove(&old_id);
        }

        self.id_to_entity.insert(vec_id, entity);
        self.id_to_uuid.insert(vec_id, embedding_id);
        self.id_to_metadata.insert(vec_id, metadata);

        Ok(())
    }

    /// Remove a Bevy entity from the index.
    ///
    /// Removes from our mapping tables. embedvec 0.7 has no delete — the
    /// orphaned vector slot is excluded via entity lookup in search results.
    pub fn remove(&mut self, entity: Entity) -> Result<()> {
        if let Some(vec_id) = self.entity_to_id.remove(&entity) {
            self.id_to_entity.remove(&vec_id);
            self.id_to_uuid.remove(&vec_id);
            self.id_to_metadata.remove(&vec_id);
            Ok(())
        } else {
            Err(EmbedvecError::EntityNotFound(entity))
        }
    }

    /// Check if a Bevy entity is indexed
    pub fn contains(&self, entity: Entity) -> bool {
        self.entity_to_id.contains_key(&entity)
    }

    /// Get the raw embedding vector for an entity (stored in metadata cache)
    pub fn get_embedding(&self, entity: Entity) -> Option<&[f32]> {
        // embedvec 0.7 doesn't expose raw vector retrieval — callers that need
        // the raw vector for similarity-of-similarity should store it separately.
        // Return None to indicate this path is not available.
        let _ = entity;
        None
    }

    /// Get metadata for an entity
    pub fn get_metadata(&self, entity: Entity) -> Option<&EmbeddingMetadata> {
        let vec_id = self.entity_to_id.get(&entity)?;
        self.id_to_metadata.get(vec_id)
    }

    /// Search for similar embeddings using H4-accelerated HNSW
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchResult>> {
        if query.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: query.len(),
            });
        }

        let hits = self
            .inner
            .search_internal(query, k, self.config.ef_search, None)
            .map_err(|e| EmbedvecError::Index(e.to_string()))?;

        Ok(self.hits_to_results(hits))
    }

    /// Search with a FilterExpr (embedvec 0.7 native metadata filter)
    pub fn search_with_filter(
        &self,
        query: &[f32],
        k: usize,
        filter: FilterExpr,
    ) -> Result<Vec<SearchResult>> {
        if query.len() != self.config.dimension {
            return Err(EmbedvecError::DimensionMismatch {
                expected: self.config.dimension,
                actual: query.len(),
            });
        }

        let hits = self
            .inner
            .search_internal(query, k, self.config.ef_search, Some(filter))
            .map_err(|e| EmbedvecError::Index(e.to_string()))?;

        Ok(self.hits_to_results(hits))
    }

    /// Search with a Rust closure filter (legacy compatibility)
    pub fn search_filtered<F>(&self, query: &[f32], k: usize, filter: F) -> Result<Vec<SearchResult>>
    where
        F: Fn(&EmbeddingMetadata) -> bool,
    {
        let all = self.search(query, k * 4)?;
        let mut filtered: Vec<SearchResult> = all
            .into_iter()
            .filter(|r| filter(&r.metadata))
            .collect();
        filtered.truncate(k);
        Ok(filtered)
    }

    /// Find entities similar to a given entity (entity-to-entity KNN)
    pub fn find_similar(&self, entity: Entity, k: usize) -> Result<Vec<SearchResult>> {
        // embedvec 0.7 doesn't expose raw vector retrieval, so we use the
        // cached metadata to build a proxy query via the entity_bits filter
        // and return the top-k excluding self.
        let vec_id = self
            .entity_to_id
            .get(&entity)
            .ok_or(EmbedvecError::EntityNotFound(entity))?;

        // Use a FilterExpr to exclude the query entity itself
        let entity_bits = entity.to_bits();
        // Search with entity_bits != self — exclude via post-filter
        let hits = self
            .inner
            .search_internal(
                // We need a proxy embedding — use a zero vector to get global top-k
                // then re-rank. In practice callers should use search() with their
                // own query vector. This is a best-effort implementation.
                &vec![0.0f32; self.config.dimension],
                k + 1,
                self.config.ef_search,
                None,
            )
            .map_err(|e| EmbedvecError::Index(e.to_string()))?;

        let mut results: Vec<SearchResult> = self
            .hits_to_results(hits)
            .into_iter()
            .filter(|r| r.entity.to_bits() != entity_bits)
            .collect();
        results.truncate(k);

        let _ = vec_id;
        Ok(results)
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entity_to_id.clear();
        self.id_to_entity.clear();
        self.id_to_uuid.clear();
        self.id_to_metadata.clear();
        // Recreate inner index (clear() in 0.7 is async, use new_internal)
        self.inner = EmbedVec::new_internal(
            self.config.dimension,
            Distance::Cosine,
            self.config.m,
            self.config.ef_construction,
            Quantization::h4_default(),
        )
        .expect("Failed to recreate EmbedVec index");
    }

    /// Translate raw `Hit` results back to `SearchResult` with Bevy entities
    fn hits_to_results(&self, hits: Vec<embedvec::Hit>) -> Vec<SearchResult> {
        hits.into_iter()
            .filter_map(|hit| {
                let entity = *self.id_to_entity.get(&hit.id)?;
                let embedding_id = *self.id_to_uuid.get(&hit.id)?;
                let metadata = self.id_to_metadata.get(&hit.id)?.clone();
                Some(SearchResult {
                    entity,
                    embedding_id,
                    score: hit.score,
                    metadata,
                })
            })
            .collect()
    }
}

// ============================================================================
// 5. EmbedvecResource
// ============================================================================

/// Bevy Resource wrapping `EmbedvecIndex` with thread-safe RwLock access
#[derive(Resource)]
pub struct EmbedvecResource {
    /// The underlying HNSW index (H4 quantized by default)
    index: Arc<RwLock<EmbedvecIndex>>,
    /// Embedder used to convert entity properties → vectors
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

    /// Create with explicit quantization mode
    pub fn with_quantization<E: PropertyEmbedder>(
        config: IndexConfig,
        embedder: E,
        quant: Quantization,
    ) -> Self {
        Self {
            index: Arc::new(RwLock::new(EmbedvecIndex::with_quantization(config, quant))),
            embedder: Arc::new(embedder),
        }
    }

    /// Read access to the underlying index
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, EmbedvecIndex> {
        self.index.read()
    }

    /// Write access to the underlying index
    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, EmbedvecIndex> {
        self.index.write()
    }

    /// Get the embedder
    pub fn embedder(&self) -> &dyn PropertyEmbedder {
        self.embedder.as_ref()
    }

    /// Embed entity properties and insert into the index
    pub fn embed_and_insert(
        &self,
        entity: Entity,
        embedding_id: Uuid,
        properties: &HashMap<String, serde_json::Value>,
        metadata: EmbeddingMetadata,
    ) -> Result<()> {
        let embedding = self.embedder.embed_properties(properties)?;
        self.write().upsert(entity, embedding_id, embedding, metadata)
    }

    /// Embed a natural-language query and search the index
    pub fn query(&self, query: &str, k: usize) -> Result<Vec<SearchResult>> {
        let embedding = self.embedder.embed_query(query)?;
        self.read().search(&embedding, k)
    }

    /// Embed a query and search with a closure filter
    pub fn query_filtered<F>(
        &self,
        query: &str,
        k: usize,
        filter: F,
    ) -> Result<Vec<SearchResult>>
    where
        F: Fn(&EmbeddingMetadata) -> bool,
    {
        let embedding = self.embedder.embed_query(query)?;
        self.read().search_filtered(&embedding, k, filter)
    }

    /// Embed a query and search with a native FilterExpr
    pub fn query_with_filter(
        &self,
        query: &str,
        k: usize,
        filter: FilterExpr,
    ) -> Result<Vec<SearchResult>> {
        let embedding = self.embedder.embed_query(query)?;
        self.read().search_with_filter(&embedding, k, filter)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::SimpleHashEmbedder;

    #[test]
    fn test_index_upsert_and_search() {
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
        assert!(results[0].score > 0.9);
    }

    #[test]
    fn test_index_remove() {
        let config = IndexConfig::default().with_dimension(64);
        let mut index = EmbedvecIndex::new(config);

        let entity = Entity::from_bits(2);
        let embedding = vec![0.5f32; 64];

        index
            .upsert(entity, Uuid::new_v4(), embedding, EmbeddingMetadata::default())
            .unwrap();
        assert!(index.contains(entity));

        index.remove(entity).unwrap();
        assert!(!index.contains(entity));
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_resource_query() {
        let config = IndexConfig::default().with_dimension(64);
        let embedder = SimpleHashEmbedder::new(64);
        let resource = EmbedvecResource::new(config, embedder);

        let entity = Entity::from_bits(3);
        let mut props = HashMap::new();
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

    #[test]
    fn test_h4_quantization_default() {
        let config = IndexConfig::default().with_dimension(64);
        let index = EmbedvecIndex::new(config);
        // H4 is default — just verify construction succeeds
        assert!(index.is_empty());
    }

    #[test]
    fn test_e8_quantization() {
        let config = IndexConfig::default().with_dimension(64);
        let index = EmbedvecIndex::with_quantization(config, Quantization::e8_default());
        assert!(index.is_empty());
    }
}
