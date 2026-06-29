//! # Fjall Persistence — Cold-tier vector index backed by Fjall (LSM-tree)
//!
//! Replaces the former RocksDB backend so embedvec persistence standardizes on
//! **Fjall** — the same LSM store `worlddb` / `eustress-fjall` use — removing the
//! foreign RocksDB dependency from the workspace.
//!
//! ## Design
//!
//! Four partitions mirror the Sled tree / former-RocksDB column-family layout so
//! the backends stay drop-in replaceable:
//!
//! | Partition       | Key                        | Value              |
//! |-----------------|----------------------------|--------------------|
//! | `embeddings`    | UUID (16 bytes)            | JSON `FjallEntry`  |
//! | `entity_index`  | entity bits (8 bytes, BE)  | UUID (16 bytes)    |
//! | `class_index`   | `"{class}:{uuid}"` (UTF-8) | UUID (16 bytes)    |
//! | `meta`          | arbitrary UTF-8 key        | arbitrary bytes    |
//!
//! An in-memory cache holds embeddings for cosine search; Fjall is the durable
//! backing store — all mutations are written through.

use crate::components::EmbeddingMetadata;
use crate::error::{EmbedvecError, Result};
use crate::resource::{IndexConfig, SearchResult};
use bevy::prelude::*;
use fjall::{Config, Keyspace, PartitionCreateOptions, PartitionHandle, PersistMode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Partition names
// ─────────────────────────────────────────────────────────────────────────────

const P_EMBEDDINGS: &str = "embeddings";
const P_ENTITY_INDEX: &str = "entity_index";
const P_CLASS_INDEX: &str = "class_index";
const P_META: &str = "meta";

// ─────────────────────────────────────────────────────────────────────────────
// FjallConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the Fjall cold-tier store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FjallConfig {
    /// Path to the Fjall keyspace directory.
    pub path: String,
}

impl Default for FjallConfig {
    fn default() -> Self {
        Self {
            path: "./embedvec_fjall".to_string(),
        }
    }
}

impl FjallConfig {
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FjallEntry
// ─────────────────────────────────────────────────────────────────────────────

/// Serialisable entry stored in the `embeddings` partition.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct FjallEntry {
    entity_bits: u64,
    embedding_id: Uuid,
    embedding: Vec<f32>,
    metadata: EmbeddingMetadata,
    class_path: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// FjallIndex
// ─────────────────────────────────────────────────────────────────────────────

/// Fjall-backed vector index.
///
/// Provides the same interface as `PersistentIndex` (Sled) so callers can swap
/// backends without changing call sites.
pub struct FjallIndex {
    keyspace: Keyspace,
    embeddings: PartitionHandle,
    entity_index: PartitionHandle,
    class_index: PartitionHandle,
    /// Reserved for arbitrary metadata KV (layout parity with the prior backend).
    #[allow(dead_code)]
    meta: PartitionHandle,
    config: IndexConfig,
    /// In-memory cache for cosine search (loaded at open, kept in sync).
    cache: HashMap<Uuid, FjallEntry>,
}

impl FjallIndex {
    /// Open or create a Fjall index at the configured path.
    pub fn open(index_config: IndexConfig, fjall_config: FjallConfig) -> Result<Self> {
        let keyspace = Config::new(&fjall_config.path)
            .open()
            .map_err(|e| EmbedvecError::Persistence(format!("Fjall open: {e}")))?;

        let mut open_p = |name: &str| -> Result<PartitionHandle> {
            keyspace
                .open_partition(name, PartitionCreateOptions::default())
                .map_err(|e| EmbedvecError::Persistence(format!("Fjall partition {name}: {e}")))
        };

        let embeddings = open_p(P_EMBEDDINGS)?;
        let entity_index = open_p(P_ENTITY_INDEX)?;
        let class_index = open_p(P_CLASS_INDEX)?;
        let meta = open_p(P_META)?;

        let mut index = Self {
            keyspace,
            embeddings,
            entity_index,
            class_index,
            meta,
            config: index_config,
            cache: HashMap::new(),
        };
        index.load_cache()?;
        Ok(index)
    }

    fn load_cache(&mut self) -> Result<()> {
        self.cache.clear();
        for kv in self.embeddings.iter() {
            let (key, value) =
                kv.map_err(|e| EmbedvecError::Persistence(format!("Fjall iter: {e}")))?;
            let id = Uuid::from_slice(&key)
                .map_err(|e| EmbedvecError::Persistence(format!("Bad UUID key: {e}")))?;
            let entry: FjallEntry = serde_json::from_slice(&value)
                .map_err(|e| EmbedvecError::Serialization(e.to_string()))?;
            self.cache.insert(id, entry);
        }

        tracing::info!(count = self.cache.len(), "FjallIndex: loaded embeddings from disk");
        Ok(())
    }

    pub fn config(&self) -> &IndexConfig {
        &self.config
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Insert or update an embedding.
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

        let entry = FjallEntry {
            entity_bits: entity.to_bits(),
            embedding_id,
            embedding,
            metadata,
            class_path: class_path.clone(),
        };

        let entry_bytes =
            serde_json::to_vec(&entry).map_err(|e| EmbedvecError::Serialization(e.to_string()))?;

        self.embeddings
            .insert(embedding_id.as_bytes(), &entry_bytes)
            .map_err(|e| EmbedvecError::Persistence(format!("insert embeddings: {e}")))?;

        self.entity_index
            .insert(entity.to_bits().to_be_bytes(), embedding_id.as_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("insert entity_index: {e}")))?;

        if let Some(ref path) = class_path {
            let class_key = format!("{path}:{embedding_id}");
            self.class_index
                .insert(class_key.as_bytes(), embedding_id.as_bytes())
                .map_err(|e| EmbedvecError::Persistence(format!("insert class_index: {e}")))?;
        }

        self.cache.insert(embedding_id, entry);
        Ok(())
    }

    /// Remove an entity from the index.
    pub fn remove(&mut self, entity: Entity) -> Result<()> {
        let id_bytes = self
            .entity_index
            .get(entity.to_bits().to_be_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("get entity_index: {e}")))?
            .ok_or(EmbedvecError::EntityNotFound(entity))?;

        let embedding_id = Uuid::from_slice(&id_bytes)
            .map_err(|e| EmbedvecError::Persistence(format!("Bad UUID: {e}")))?;

        // Remove class index entry if present (best-effort).
        if let Some(entry) = self.cache.get(&embedding_id) {
            if let Some(ref path) = entry.class_path {
                let class_key = format!("{path}:{embedding_id}");
                let _ = self.class_index.remove(class_key.as_bytes());
            }
        }

        self.embeddings
            .remove(embedding_id.as_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("remove embeddings: {e}")))?;

        self.entity_index
            .remove(entity.to_bits().to_be_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("remove entity_index: {e}")))?;

        self.cache.remove(&embedding_id);
        Ok(())
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.entity_index
            .get(entity.to_bits().to_be_bytes())
            .map(|opt| opt.is_some())
            .unwrap_or(false)
    }

    pub fn get_embedding(&self, entity: Entity) -> Option<Vec<f32>> {
        let id_bytes = self
            .entity_index
            .get(entity.to_bits().to_be_bytes())
            .ok()??;
        let id = Uuid::from_slice(&id_bytes).ok()?;
        self.cache.get(&id).map(|e| e.embedding.clone())
    }

    pub fn get_metadata(&self, entity: Entity) -> Option<EmbeddingMetadata> {
        let id_bytes = self
            .entity_index
            .get(entity.to_bits().to_be_bytes())
            .ok()??;
        let id = Uuid::from_slice(&id_bytes).ok()?;
        self.cache.get(&id).map(|e| e.metadata.clone())
    }

    /// Cosine similarity search over the in-memory cache.
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

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(k);
        Ok(results)
    }

    /// Search filtered by a metadata predicate.
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

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(k);
        Ok(results)
    }

    /// Search within a specific ontology class path (prefix match).
    pub fn search_by_class(
        &self,
        query: &[f32],
        class_path: &str,
        k: usize,
    ) -> Result<Vec<SearchResult>> {
        self.search_filtered(query, k, |meta| {
            meta.properties
                .get("class_path")
                .and_then(|v| v.as_str())
                .map(|p| p.starts_with(class_path))
                .unwrap_or(false)
        })
    }

    /// Persist all pending writes to disk.
    pub fn flush(&self) -> Result<()> {
        self.keyspace
            .persist(PersistMode::SyncAll)
            .map_err(|e| EmbedvecError::Persistence(format!("Fjall persist: {e}")))?;
        tracing::debug!("FjallIndex: persisted to disk");
        Ok(())
    }

    /// Approximate size on disk (sum of live segment sizes across the keyspace).
    pub fn size_on_disk(&self) -> u64 {
        self.keyspace.disk_space()
    }

    pub fn clear(&mut self) -> Result<()> {
        self.cache.clear();
        tracing::warn!(
            "FjallIndex::clear() cleared in-memory cache only; delete the keyspace dir to wipe disk"
        );
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FjallOntologyIndex
// ─────────────────────────────────────────────────────────────────────────────

/// Ontology-aware vector index backed by Fjall.
///
/// Mirrors `PersistentOntologyIndex` (Sled) so the two are drop-in replaceable.
pub struct FjallOntologyIndex {
    index: FjallIndex,
    ontology: crate::ontology::OntologyTree,
}

impl FjallOntologyIndex {
    pub fn open(
        ontology: crate::ontology::OntologyTree,
        index_config: IndexConfig,
        fjall_config: FjallConfig,
    ) -> Result<Self> {
        let index = FjallIndex::open(index_config, fjall_config)?;
        Ok(Self { index, ontology })
    }

    pub fn with_eustress_base(
        index_config: IndexConfig,
        fjall_config: FjallConfig,
    ) -> Result<Self> {
        Self::open(
            crate::ontology::OntologyTree::with_eustress_base(),
            index_config,
            fjall_config,
        )
    }

    pub fn ontology(&self) -> &crate::ontology::OntologyTree {
        &self.ontology
    }

    pub fn insert(
        &mut self,
        class_path: &str,
        entity: Entity,
        instance_id: Uuid,
        embedding: Vec<f32>,
        metadata: EmbeddingMetadata,
    ) -> Result<()> {
        if self.ontology.get_by_path(class_path).is_none() {
            return Err(EmbedvecError::Index(format!(
                "Unknown ontology class path: {class_path}"
            )));
        }
        self.index.upsert(
            entity,
            instance_id,
            embedding,
            metadata,
            Some(class_path.to_string()),
        )
    }

    pub fn remove(&mut self, entity: Entity) -> Result<()> {
        self.index.remove(entity)
    }

    pub fn search_class(
        &self,
        class_path: &str,
        query: &[f32],
        k: usize,
        include_descendants: bool,
    ) -> Result<Vec<SearchResult>> {
        if include_descendants {
            self.index.search_by_class(query, class_path, k)
        } else {
            self.index.search_filtered(query, k, |meta| {
                meta.properties
                    .get("class_path")
                    .and_then(|v| v.as_str())
                    .map(|p| p == class_path)
                    .unwrap_or(false)
            })
        }
    }

    pub fn search_global(&self, query: &[f32], k: usize) -> Result<Vec<SearchResult>> {
        self.index.search(query, k)
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn flush(&self) -> Result<()> {
        self.index.flush()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Cosine similarity helper
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::EmbeddingMetadata;
    use tempfile::tempdir;

    #[test]
    fn open_insert_search_flush() {
        let dir = tempdir().unwrap();
        let fjall_config = FjallConfig::default().with_path(dir.path().to_str().unwrap());
        let index_config = IndexConfig::default().with_dimension(16);

        let mut index = FjallIndex::open(index_config, fjall_config).unwrap();

        let entity = Entity::from_bits(1);
        let id = Uuid::new_v4();
        let embedding = vec![0.5f32; 16];
        let metadata = EmbeddingMetadata::with_name("TestEntry");

        index
            .upsert(entity, id, embedding.clone(), metadata, Some("Entity/Spatial".into()))
            .unwrap();

        assert!(index.contains(entity));
        assert_eq!(index.len(), 1);

        let results = index.search(&embedding, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].score > 0.99);

        index.flush().unwrap();
    }

    #[test]
    fn persistence_reload() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();

        let entity = Entity::from_bits(42);
        let emb_id = Uuid::new_v4();

        // Write
        {
            let mut idx = FjallIndex::open(
                IndexConfig::default().with_dimension(8),
                FjallConfig::default().with_path(&path),
            )
            .unwrap();
            idx.upsert(
                entity,
                emb_id,
                vec![0.1f32; 8],
                EmbeddingMetadata::with_name("Persist"),
                None,
            )
            .unwrap();
            idx.flush().unwrap();
        }

        // Reload
        {
            let idx = FjallIndex::open(
                IndexConfig::default().with_dimension(8),
                FjallConfig::default().with_path(&path),
            )
            .unwrap();
            assert_eq!(idx.len(), 1);
            assert!(idx.contains(entity));
        }
    }
}
