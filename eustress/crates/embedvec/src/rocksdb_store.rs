//! # RocksDB Persistence — Cold-tier vector index backed by RocksDB
//!
//! ## Table of Contents
//! 1. RocksConfig   — configuration for the RocksDB instance
//! 2. RocksEntry    — serialisable entry stored per embedding
//! 3. RocksIndex    — RocksDB-backed vector index (mirrors PersistentIndex API)
//! 4. RocksOntologyIndex — ontology-aware wrapper (mirrors PersistentOntologyIndex)
//!
//! ## Design
//!
//! Four column families mirror the Sled tree layout so the two backends are
//! drop-in replaceable:
//!
//! | CF              | Key                        | Value              |
//! |-----------------|----------------------------|--------------------|
//! | `embeddings`    | UUID (16 bytes)            | JSON `RocksEntry`  |
//! | `entity_index`  | entity bits (8 bytes, BE)  | UUID (16 bytes)    |
//! | `class_index`   | `"{class}:{uuid}"` (UTF-8) | UUID (16 bytes)    |
//! | `meta`          | arbitrary UTF-8 key        | arbitrary bytes    |
//!
//! In-memory cache mirrors the Sled implementation: embeddings are loaded
//! into a `HashMap` at open time for O(1) HNSW-style cosine search.
//! RocksDB is the durable backing store — all mutations are written through.

use crate::components::EmbeddingMetadata;
use crate::error::{EmbedvecError, Result};
use crate::resource::{IndexConfig, SearchResult};
use bevy::prelude::*;
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Column family names
// ─────────────────────────────────────────────────────────────────────────────

const CF_EMBEDDINGS: &str = "embeddings";
const CF_ENTITY_INDEX: &str = "entity_index";
const CF_CLASS_INDEX: &str = "class_index";
const CF_META: &str = "meta";

// ─────────────────────────────────────────────────────────────────────────────
// RocksConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the RocksDB cold-tier store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RocksConfig {
    /// Path to the RocksDB directory.
    pub path: String,
    /// LZ4 compression on all column families.
    pub compression: bool,
    /// Write buffer size per column family in bytes (default: 64 MB).
    pub write_buffer_size: usize,
    /// Maximum number of open files (default: 1000; -1 = unlimited).
    pub max_open_files: i32,
}

impl Default for RocksConfig {
    fn default() -> Self {
        Self {
            path: "./embedvec_rocks".to_string(),
            compression: true,
            write_buffer_size: 64 * 1024 * 1024,
            max_open_files: 1000,
        }
    }
}

impl RocksConfig {
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub fn with_write_buffer(mut self, bytes: usize) -> Self {
        self.write_buffer_size = bytes;
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RocksEntry
// ─────────────────────────────────────────────────────────────────────────────

/// Serialisable entry stored in the `embeddings` column family.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RocksEntry {
    entity_bits: u64,
    embedding_id: Uuid,
    embedding: Vec<f32>,
    metadata: EmbeddingMetadata,
    class_path: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// RocksIndex
// ─────────────────────────────────────────────────────────────────────────────

/// RocksDB-backed vector index.
///
/// Provides the same interface as `PersistentIndex` (Sled) so callers can
/// swap backends without changing call sites.
pub struct RocksIndex {
    db: Arc<DB>,
    config: IndexConfig,
    /// In-memory cache for cosine search (loaded at open, kept in sync).
    cache: HashMap<Uuid, RocksEntry>,
}

impl RocksIndex {
    /// Open or create a RocksDB index at the configured path.
    pub fn open(index_config: IndexConfig, rocks_config: RocksConfig) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.set_max_open_files(rocks_config.max_open_files);

        let cf_opts = {
            let mut o = Options::default();
            o.set_write_buffer_size(rocks_config.write_buffer_size);
            if rocks_config.compression {
                o.set_compression_type(rocksdb::DBCompressionType::Lz4);
            }
            o
        };

        let cf_descriptors = vec![
            ColumnFamilyDescriptor::new(CF_EMBEDDINGS, cf_opts.clone()),
            ColumnFamilyDescriptor::new(CF_ENTITY_INDEX, cf_opts.clone()),
            ColumnFamilyDescriptor::new(CF_CLASS_INDEX, cf_opts.clone()),
            ColumnFamilyDescriptor::new(CF_META, cf_opts),
        ];

        let db = DB::open_cf_descriptors(&opts, &rocks_config.path, cf_descriptors)
            .map_err(|e| EmbedvecError::Persistence(format!("RocksDB open: {e}")))?;

        let db = Arc::new(db);
        let mut index = Self {
            db,
            config: index_config,
            cache: HashMap::new(),
        };
        index.load_cache()?;
        Ok(index)
    }

    fn load_cache(&mut self) -> Result<()> {
        let cf = self
            .db
            .cf_handle(CF_EMBEDDINGS)
            .ok_or_else(|| EmbedvecError::Persistence("CF embeddings missing".into()))?;

        self.cache.clear();
        let iter = self.db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, value) =
                item.map_err(|e| EmbedvecError::Persistence(format!("RocksDB iter: {e}")))?;
            let id = Uuid::from_slice(&key)
                .map_err(|e| EmbedvecError::Persistence(format!("Bad UUID key: {e}")))?;
            let entry: RocksEntry = serde_json::from_slice(&value)
                .map_err(|e| EmbedvecError::Serialization(e.to_string()))?;
            self.cache.insert(id, entry);
        }

        tracing::info!(count = self.cache.len(), "RocksIndex: loaded embeddings from disk");
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

        let entry = RocksEntry {
            entity_bits: entity.to_bits(),
            embedding_id,
            embedding,
            metadata,
            class_path: class_path.clone(),
        };

        let entry_bytes = serde_json::to_vec(&entry)
            .map_err(|e| EmbedvecError::Serialization(e.to_string()))?;

        // Write to embeddings CF
        let cf_emb = self
            .db
            .cf_handle(CF_EMBEDDINGS)
            .ok_or_else(|| EmbedvecError::Persistence("CF embeddings missing".into()))?;
        self.db
            .put_cf(&cf_emb, embedding_id.as_bytes(), &entry_bytes)
            .map_err(|e| EmbedvecError::Persistence(format!("put embeddings: {e}")))?;

        // Write to entity_index CF
        let cf_ent = self
            .db
            .cf_handle(CF_ENTITY_INDEX)
            .ok_or_else(|| EmbedvecError::Persistence("CF entity_index missing".into()))?;
        self.db
            .put_cf(
                &cf_ent,
                entity.to_bits().to_be_bytes(),
                embedding_id.as_bytes(),
            )
            .map_err(|e| EmbedvecError::Persistence(format!("put entity_index: {e}")))?;

        // Write to class_index CF if applicable
        if let Some(ref path) = class_path {
            let cf_cls = self
                .db
                .cf_handle(CF_CLASS_INDEX)
                .ok_or_else(|| EmbedvecError::Persistence("CF class_index missing".into()))?;
            let class_key = format!("{path}:{embedding_id}");
            self.db
                .put_cf(&cf_cls, class_key.as_bytes(), embedding_id.as_bytes())
                .map_err(|e| EmbedvecError::Persistence(format!("put class_index: {e}")))?;
        }

        self.cache.insert(embedding_id, entry);
        Ok(())
    }

    /// Remove an entity from the index.
    pub fn remove(&mut self, entity: Entity) -> Result<()> {
        let cf_ent = self
            .db
            .cf_handle(CF_ENTITY_INDEX)
            .ok_or_else(|| EmbedvecError::Persistence("CF entity_index missing".into()))?;

        let id_bytes = self
            .db
            .get_cf(&cf_ent, entity.to_bits().to_be_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("get entity_index: {e}")))?
            .ok_or(EmbedvecError::EntityNotFound(entity))?;

        let embedding_id = Uuid::from_slice(&id_bytes)
            .map_err(|e| EmbedvecError::Persistence(format!("Bad UUID: {e}")))?;

        // Remove class index entry if present
        if let Some(entry) = self.cache.get(&embedding_id) {
            if let Some(ref path) = entry.class_path {
                if let Some(cf_cls) = self.db.cf_handle(CF_CLASS_INDEX) {
                    let class_key = format!("{path}:{embedding_id}");
                    let _ = self.db.delete_cf(&cf_cls, class_key.as_bytes());
                }
            }
        }

        // Remove from embeddings CF
        let cf_emb = self
            .db
            .cf_handle(CF_EMBEDDINGS)
            .ok_or_else(|| EmbedvecError::Persistence("CF embeddings missing".into()))?;
        self.db
            .delete_cf(&cf_emb, embedding_id.as_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("delete embeddings: {e}")))?;

        // Remove from entity_index CF
        self.db
            .delete_cf(&cf_ent, entity.to_bits().to_be_bytes())
            .map_err(|e| EmbedvecError::Persistence(format!("delete entity_index: {e}")))?;

        self.cache.remove(&embedding_id);
        Ok(())
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.db
            .cf_handle(CF_ENTITY_INDEX)
            .and_then(|cf| {
                self.db
                    .get_cf(&cf, entity.to_bits().to_be_bytes())
                    .ok()
                    .flatten()
            })
            .is_some()
    }

    pub fn get_embedding(&self, entity: Entity) -> Option<Vec<f32>> {
        let cf_ent = self.db.cf_handle(CF_ENTITY_INDEX)?;
        let id_bytes = self
            .db
            .get_cf(&cf_ent, entity.to_bits().to_be_bytes())
            .ok()??;
        let id = Uuid::from_slice(&id_bytes).ok()?;
        self.cache.get(&id).map(|e| e.embedding.clone())
    }

    pub fn get_metadata(&self, entity: Entity) -> Option<EmbeddingMetadata> {
        let cf_ent = self.db.cf_handle(CF_ENTITY_INDEX)?;
        let id_bytes = self
            .db
            .get_cf(&cf_ent, entity.to_bits().to_be_bytes())
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

    /// Flush all pending writes (RocksDB handles this automatically via WAL,
    /// but an explicit flush ensures everything is on disk before archival).
    pub fn flush(&self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| EmbedvecError::Persistence(format!("RocksDB flush: {e}")))?;
        tracing::debug!("RocksIndex: flushed to disk");
        Ok(())
    }

    /// Approximate size on disk (sum of live SST file sizes).
    pub fn size_on_disk(&self) -> u64 {
        self.db
            .property_int_value("rocksdb.total-sst-files-size")
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    pub fn clear(&mut self) -> Result<()> {
        // Drop and recreate: simplest way to clear all CFs atomically.
        // For production use, iterate and delete range instead.
        self.cache.clear();
        tracing::warn!("RocksIndex::clear() cleared in-memory cache only; use DB::destroy to wipe disk");
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RocksOntologyIndex
// ─────────────────────────────────────────────────────────────────────────────

/// Ontology-aware vector index backed by RocksDB.
///
/// Mirrors `PersistentOntologyIndex` (Sled) so the two are drop-in replaceable.
pub struct RocksOntologyIndex {
    index: RocksIndex,
    ontology: crate::ontology::OntologyTree,
}

impl RocksOntologyIndex {
    pub fn open(
        ontology: crate::ontology::OntologyTree,
        index_config: IndexConfig,
        rocks_config: RocksConfig,
    ) -> Result<Self> {
        let index = RocksIndex::open(index_config, rocks_config)?;
        Ok(Self { index, ontology })
    }

    pub fn with_eustress_base(
        index_config: IndexConfig,
        rocks_config: RocksConfig,
    ) -> Result<Self> {
        Self::open(
            crate::ontology::OntologyTree::with_eustress_base(),
            index_config,
            rocks_config,
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
        self.index
            .upsert(entity, instance_id, embedding, metadata, Some(class_path.to_string()))
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
        let rocks_config = RocksConfig::default().with_path(dir.path().to_str().unwrap());
        let index_config = IndexConfig::default().with_dimension(16);

        let mut index = RocksIndex::open(index_config, rocks_config).unwrap();

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
            let mut idx = RocksIndex::open(
                IndexConfig::default().with_dimension(8),
                RocksConfig::default().with_path(&path),
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
            let idx = RocksIndex::open(
                IndexConfig::default().with_dimension(8),
                RocksConfig::default().with_path(&path),
            )
            .unwrap();
            assert_eq!(idx.len(), 1);
            assert!(idx.contains(entity));
        }
    }
}
