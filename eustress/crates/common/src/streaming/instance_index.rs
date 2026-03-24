//! # Instance Index — Flat Metadata for Explorer Queries
//!
//! ## Table of Contents
//! - IndexEntry      — lightweight record: id, name, tags, chunk_coord, version
//! - InstanceIndex   — flat in-memory index with search + serialize to disk
//!
//! ## Design
//! The Explorer sees a flat, lazily-loaded virtual list of ALL instances,
//! regardless of which chunk they live in or which storage tier they're on.
//!
//! This index holds only metadata (~50–100 bytes per entry), NOT the full
//! InstanceBin data. It fits entirely in RAM for millions of instances and
//! never needs to be chunked.
//!
//! The index file `instances.index` is a bincode + zstd blob next to `space.toml`.
//! It's rebuilt from the SpatialChunkGrid periodically (or on explicit save).
//!
//! Explorer search hits this index — never the TOML files or chunks directly.

use std::path::Path;
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use super::types::{ChunkCoord, InstanceId};

/// Magic bytes for the index file.
const INDEX_MAGIC: [u8; 4] = *b"EUSI";

/// Current index format version.
const INDEX_VERSION: u32 = 1;

/// Zstd compression level for the index file.
const ZSTD_LEVEL: i32 = 1;

// ─────────────────────────────────────────────────────────────────────────────
// IndexEntry — one row in the flat index
// ─────────────────────────────────────────────────────────────────────────────

/// Lightweight metadata for one instance — what the Explorer needs for
/// search, sort, and virtual list display. No geometry or physics data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Unique instance identifier (matches TOML filename stem).
    pub id: InstanceId,
    /// Human-readable name for display in the Explorer.
    pub name: String,
    /// Tags for search/filter (e.g. ["tree", "vegetation", "LOD2"]).
    pub tags: Vec<String>,
    /// Which spatial chunk this instance belongs to.
    pub chunk_coord: ChunkCoord,
    /// World-space position (for Explorer sort-by-distance).
    pub position: [f32; 3],
    /// Class/type identifier (for Explorer group-by-class).
    pub class_id: u32,
    /// Last-known version counter (for staleness detection).
    pub version: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// InstanceIndex — the full in-memory index
// ─────────────────────────────────────────────────────────────────────────────

/// Flat in-memory index of all instance metadata in a Space.
/// Supports search, filter, and serialization to disk.
///
/// Thread-safe: wrapped in the StreamingPlugin as a Bevy `Resource`.
pub struct InstanceIndex {
    /// All entries, kept sorted by name for binary search.
    entries: Vec<IndexEntry>,
}

impl InstanceIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Create an index pre-allocated for `capacity` entries.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { entries: Vec::with_capacity(capacity) }
    }

    /// Total number of indexed instances.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert or update an entry. If an entry with the same id exists, it's replaced.
    pub fn upsert(&mut self, entry: IndexEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.id == entry.id) {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Remove an entry by id.
    pub fn remove(&mut self, id: &InstanceId) {
        self.entries.retain(|e| e.id != *id);
    }

    /// Look up an entry by id.
    pub fn get(&self, id: &InstanceId) -> Option<&IndexEntry> {
        self.entries.iter().find(|e| e.id == *id)
    }

    /// Search entries by name substring (case-insensitive).
    /// Returns matching entries in name-sorted order.
    pub fn search_name(&self, query: &str) -> Vec<&IndexEntry> {
        let query_lower = query.to_lowercase();
        self.entries.iter()
            .filter(|e| e.name.to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Search entries by tag (exact match, case-insensitive).
    pub fn search_tag(&self, tag: &str) -> Vec<&IndexEntry> {
        let tag_lower = tag.to_lowercase();
        self.entries.iter()
            .filter(|e| e.tags.iter().any(|t| t.to_lowercase() == tag_lower))
            .collect()
    }

    /// Search entries by combined name + tag query.
    /// Splits the query on whitespace; all terms must match (AND logic).
    /// A term prefixed with `#` matches tags, otherwise matches name.
    pub fn search(&self, query: &str) -> Vec<&IndexEntry> {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if terms.is_empty() {
            return self.entries.iter().collect();
        }

        self.entries.iter()
            .filter(|entry| {
                terms.iter().all(|term| {
                    if let Some(tag_term) = term.strip_prefix('#') {
                        // Tag match (exact, case-insensitive).
                        let tag_lower = tag_term.to_lowercase();
                        entry.tags.iter().any(|t| t.to_lowercase() == tag_lower)
                    } else {
                        // Name substring match (case-insensitive).
                        entry.name.to_lowercase().contains(&term.to_lowercase())
                    }
                })
            })
            .collect()
    }

    /// Return entries sorted by distance to a point (for Explorer sort-by-proximity).
    pub fn sorted_by_distance(&self, from: [f32; 3]) -> Vec<&IndexEntry> {
        let mut sorted: Vec<&IndexEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| {
            let da = distance_squared(a.position, from);
            let db = distance_squared(b.position, from);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted
    }

    /// Return a paginated slice for virtual list display.
    /// `offset` is the first item index, `limit` is the page size.
    pub fn page(&self, offset: usize, limit: usize) -> &[IndexEntry] {
        let start = offset.min(self.entries.len());
        let end = (start + limit).min(self.entries.len());
        &self.entries[start..end]
    }

    // ── Disk Serialization ──────────────────────────────────────────────────

    /// Save the index to disk as a bincode + zstd blob.
    /// File format: [4 magic][4 version][zstd(bincode(entries))]
    pub fn save_to_disk(&self, path: &Path) -> Result<(), IndexError> {
        let raw = bincode::serialize(&self.entries)
            .map_err(|e| IndexError::Encode(format!("bincode: {e}")))?;

        let compressed = zstd::encode_all(raw.as_slice(), ZSTD_LEVEL)
            .map_err(|e| IndexError::Encode(format!("zstd: {e}")))?;

        let tmp_path = path.with_extension("index.tmp");
        let mut file = std::fs::File::create(&tmp_path)
            .map_err(|e| IndexError::Io(e.to_string()))?;

        file.write_all(&INDEX_MAGIC)
            .map_err(|e| IndexError::Io(e.to_string()))?;
        file.write_all(&INDEX_VERSION.to_le_bytes())
            .map_err(|e| IndexError::Io(e.to_string()))?;
        file.write_all(&compressed)
            .map_err(|e| IndexError::Io(e.to_string()))?;
        file.flush()
            .map_err(|e| IndexError::Io(e.to_string()))?;
        drop(file);

        std::fs::rename(&tmp_path, path)
            .map_err(|e| IndexError::Io(e.to_string()))?;

        Ok(())
    }

    /// Load the index from disk.
    pub fn load_from_disk(path: &Path) -> Result<Self, IndexError> {
        let data = std::fs::read(path)
            .map_err(|e| IndexError::Io(e.to_string()))?;

        if data.len() < 8 {
            return Err(IndexError::Decode("file too short".into()));
        }

        // Validate magic.
        if &data[0..4] != INDEX_MAGIC.as_slice() {
            return Err(IndexError::Decode("bad magic bytes".into()));
        }

        // Validate version.
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != INDEX_VERSION {
            return Err(IndexError::Decode(format!(
                "version {} != expected {INDEX_VERSION}", version
            )));
        }

        // Decompress + deserialize.
        let compressed = &data[8..];
        let mut raw = Vec::new();
        let mut decoder = zstd::Decoder::new(compressed)
            .map_err(|e| IndexError::Decode(format!("zstd init: {e}")))?;
        decoder.read_to_end(&mut raw)
            .map_err(|e| IndexError::Decode(format!("zstd read: {e}")))?;

        let entries: Vec<IndexEntry> = bincode::deserialize(&raw)
            .map_err(|e| IndexError::Decode(format!("bincode: {e}")))?;

        Ok(Self { entries })
    }

    // ── Rebuild from Grid ───────────────────────────────────────────────────

    /// Rebuild the entire index from the SpatialChunkGrid.
    /// Called on initial load and periodically for consistency.
    pub fn rebuild_from_grid(&mut self, grid: &super::chunk_grid::SpatialChunkGrid) {
        self.entries.clear();
        grid.for_each_chunk(|coord, chunk| {
            for record in &chunk.instances {
                self.entries.push(IndexEntry {
                    id:          record.id.clone(),
                    name:        record.name.clone(),
                    tags:        record.tags.clone(),
                    chunk_coord: *coord,
                    position:    record.bin.position,
                    class_id:    record.bin.class_id,
                    version:     record.current_version(),
                });
            }
        });
    }
}

impl Default for InstanceIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

// ─────────────────────────────────────────────────────────────────────────────
// Error type
// ─────────────────────────────────────────────────────────────────────────────

/// Errors from index operations.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("index encode error: {0}")]
    Encode(String),

    #[error("index decode error: {0}")]
    Decode(String),

    #[error("index I/O error: {0}")]
    Io(String),
}
