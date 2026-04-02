//! Memory Store — Scored, Decaying Memory Entries
//!
//! ## Table of Contents
//! 1. MemoryType      — Semantic / Episodic / Procedural / Working
//! 2. Memory          — scored entry with confidence, importance, temporal decay
//! 3. MemoryQuery     — builder for querying the store
//! 4. MemoryStore     — in-memory store with type index and relevance scoring
//!
//! ## Design
//! Inspired by Vortex `cognition/memory.rs`. Each memory has:
//! - `confidence` (0.0–1.0) — how certain we are it is correct
//! - `importance` (0.0–1.0) — how significant it is
//! - Temporal decay with configurable half-life (default 7 days)
//! - `access_count` — access frequency boosts relevance
//!
//! The relevance score formula:
//! ```text
//! score = similarity × confidence × importance × decay(age) × sacred_boost
//! ```
//! where `sacred_boost = 1.2` if the memory is marked sacred, else 1.0.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// ============================================================================
// 1. MemoryType
// ============================================================================

/// Classification of memory by cognitive type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryType {
    /// Facts and concepts (long-term declarative)
    Semantic,
    /// Specific events and experiences (episodic log)
    Episodic,
    /// Skills and how-to knowledge
    Procedural,
    /// Short-lived working context
    Working,
}

// ============================================================================
// 2. Memory
// ============================================================================

/// A single memory entry with confidence, importance, and temporal decay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Unique stable ID
    pub id: Uuid,
    /// Text content of this memory
    pub content: String,
    /// Cognitive type
    pub memory_type: MemoryType,
    /// Confidence this memory is accurate (0.0–1.0)
    pub confidence: f32,
    /// Importance weight (0.0–1.0)
    pub importance: f32,
    /// Optional embedding vector for similarity queries
    pub embedding: Option<Vec<f32>>,
    /// Arbitrary key-value metadata
    pub properties: HashMap<String, serde_json::Value>,
    /// Tags for categorical filtering
    pub tags: Vec<String>,
    /// Unix timestamp ms when created
    pub created_at_ms: u64,
    /// Unix timestamp ms when last accessed
    pub last_accessed_ms: u64,
    /// Number of times this memory has been retrieved
    pub access_count: u64,
    /// Half-life for temporal decay in seconds (default: 7 days)
    pub half_life_secs: f32,
    /// Whether this is a "sacred" high-priority memory (gets 1.2× boost)
    pub is_sacred: bool,
}

impl Memory {
    /// Create a new memory with default scoring
    pub fn new(content: impl Into<String>, memory_type: MemoryType) -> Self {
        let now_ms = now_ms();
        Self {
            id: Uuid::new_v4(),
            content: content.into(),
            memory_type,
            confidence: 0.8,
            importance: 0.5,
            embedding: None,
            properties: HashMap::new(),
            tags: Vec::new(),
            created_at_ms: now_ms,
            last_accessed_ms: now_ms,
            access_count: 0,
            half_life_secs: 7.0 * 24.0 * 3600.0, // 7 days
            is_sacred: false,
        }
    }

    /// Set confidence
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Set importance
    pub fn with_importance(mut self, importance: f32) -> Self {
        self.importance = importance.clamp(0.0, 1.0);
        self
    }

    /// Attach an embedding vector
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Add a key-value property
    pub fn with_property(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.properties.insert(key.into(), v);
        }
        self
    }

    /// Mark as sacred (boosted relevance, never auto-evicted)
    pub fn as_sacred(mut self) -> Self {
        self.is_sacred = true;
        self
    }

    /// Set custom half-life in days
    pub fn with_half_life_days(mut self, days: f32) -> Self {
        self.half_life_secs = days * 24.0 * 3600.0;
        self
    }

    /// Compute relevance score given an optional query embedding
    ///
    /// `score = similarity × confidence × importance × decay × sacred_boost`
    pub fn relevance_score(&self, query_embedding: Option<&[f32]>) -> f32 {
        let similarity = match (query_embedding, &self.embedding) {
            (Some(q), Some(e)) => cosine_similarity(q, e),
            _ => 1.0, // no embedding context — treat as fully relevant
        };

        let age_secs = {
            let now = now_ms();
            ((now.saturating_sub(self.last_accessed_ms)) as f32) / 1000.0
        };

        // Exponential decay: score × 0.5^(age / half_life)
        let decay = 0.5_f32.powf(age_secs / self.half_life_secs.max(1.0));
        let sacred_boost = if self.is_sacred { 1.2 } else { 1.0 };

        // Access frequency bonus (log scale)
        let freq_boost = 1.0 + (self.access_count as f32).ln_1p() * 0.05;

        (similarity * self.confidence * self.importance * decay * sacred_boost * freq_boost)
            .clamp(0.0, 1.0)
    }

    /// Mark the memory as accessed (updates timestamp and count)
    pub fn touch(&mut self) {
        self.access_count += 1;
        self.last_accessed_ms = now_ms();
    }

    /// Age in seconds since creation
    pub fn age_secs(&self) -> f64 {
        let now = now_ms();
        (now.saturating_sub(self.created_at_ms) as f64) / 1000.0
    }
}

// ============================================================================
// 3. MemoryQuery
// ============================================================================

/// Builder for querying the MemoryStore
#[derive(Debug, Clone, Default)]
pub struct MemoryQuery {
    /// Filter by type
    pub memory_type: Option<MemoryType>,
    /// Optional query embedding for similarity scoring
    pub embedding: Option<Vec<f32>>,
    /// Minimum confidence threshold
    pub min_confidence: f32,
    /// Minimum importance threshold
    pub min_importance: f32,
    /// Maximum results
    pub limit: usize,
    /// Only return sacred memories
    pub sacred_only: bool,
    /// Tag filter — memory must have ALL of these tags
    pub required_tags: Vec<String>,
}

impl MemoryQuery {
    /// Create a default query (no filters, limit 10)
    pub fn new() -> Self {
        Self {
            limit: 10,
            ..Default::default()
        }
    }

    /// Filter by memory type
    pub fn with_type(mut self, t: MemoryType) -> Self {
        self.memory_type = Some(t);
        self
    }

    /// Score results by similarity to this embedding
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// Set minimum confidence
    pub fn with_min_confidence(mut self, c: f32) -> Self {
        self.min_confidence = c;
        self
    }

    /// Set minimum importance
    pub fn with_min_importance(mut self, i: f32) -> Self {
        self.min_importance = i;
        self
    }

    /// Set result limit
    pub fn with_limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }

    /// Only return sacred memories
    pub fn sacred_only(mut self) -> Self {
        self.sacred_only = true;
        self
    }

    /// Require specific tags
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.required_tags.push(tag.into());
        self
    }
}

// ============================================================================
// 4. MemoryStore
// ============================================================================

/// In-memory store for scored, decaying memories
pub struct MemoryStore {
    /// All memories by ID
    memories: HashMap<Uuid, Memory>,
    /// Type index for fast type-filtered retrieval
    type_index: HashMap<MemoryType, Vec<Uuid>>,
}

impl MemoryStore {
    /// Create a new empty store
    pub fn new() -> Self {
        Self {
            memories: HashMap::new(),
            type_index: HashMap::new(),
        }
    }

    /// Insert a memory and return its ID
    pub fn store(&mut self, memory: Memory) -> Uuid {
        let id = memory.id;
        let memory_type = memory.memory_type;

        self.type_index
            .entry(memory_type)
            .or_default()
            .push(id);

        self.memories.insert(id, memory);
        id
    }

    /// Retrieve a memory by ID (touches access counter)
    pub fn get(&mut self, id: Uuid) -> Option<&Memory> {
        if let Some(memory) = self.memories.get_mut(&id) {
            memory.touch();
        }
        self.memories.get(&id)
    }

    /// Query memories with scoring and filtering
    pub fn query(&self, query: &MemoryQuery) -> Vec<&Memory> {
        let emb_ref = query.embedding.as_deref();

        let mut scored: Vec<(&Memory, f32)> = self
            .memories
            .values()
            .filter(|m| {
                if let Some(t) = query.memory_type {
                    if m.memory_type != t {
                        return false;
                    }
                }
                if m.confidence < query.min_confidence {
                    return false;
                }
                if m.importance < query.min_importance {
                    return false;
                }
                if query.sacred_only && !m.is_sacred {
                    return false;
                }
                for tag in &query.required_tags {
                    if !m.tags.contains(tag) {
                        return false;
                    }
                }
                true
            })
            .map(|m| {
                let score = m.relevance_score(emb_ref);
                (m, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored
            .into_iter()
            .take(query.limit)
            .map(|(m, _)| m)
            .collect()
    }

    /// Get all memories of a specific type (unscored)
    pub fn by_type(&self, memory_type: MemoryType) -> Vec<&Memory> {
        self.type_index
            .get(&memory_type)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.memories.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Delete a memory by ID
    pub fn delete(&mut self, id: Uuid) -> bool {
        if let Some(memory) = self.memories.remove(&id) {
            if let Some(ids) = self.type_index.get_mut(&memory.memory_type) {
                ids.retain(|i| *i != id);
            }
            return true;
        }
        false
    }

    /// Evict memories below a relevance threshold (excluding sacred)
    pub fn evict_below(&mut self, threshold: f32) {
        let to_remove: Vec<Uuid> = self
            .memories
            .values()
            .filter(|m| !m.is_sacred && m.relevance_score(None) < threshold)
            .map(|m| m.id)
            .collect();

        for id in to_remove {
            self.delete(id);
        }
    }

    /// Total number of stored memories
    pub fn len(&self) -> usize {
        self.memories.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.memories.is_empty()
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a < 1e-10 || norm_b < 1e-10 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve() {
        let mut store = MemoryStore::new();
        let id = store.store(
            Memory::new("The sky is blue", MemoryType::Semantic)
                .with_confidence(0.95)
                .with_importance(0.7),
        );
        assert_eq!(store.len(), 1);
        assert!(store.get(id).is_some());
    }

    #[test]
    fn test_query_by_type() {
        let mut store = MemoryStore::new();
        store.store(Memory::new("fact", MemoryType::Semantic));
        store.store(Memory::new("event", MemoryType::Episodic));
        store.store(Memory::new("skill", MemoryType::Procedural));

        let results = store.query(&MemoryQuery::new().with_type(MemoryType::Semantic));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "fact");
    }

    #[test]
    fn test_sacred_boost() {
        let mut store = MemoryStore::new();
        store.store(Memory::new("normal", MemoryType::Semantic).with_importance(0.5));
        store.store(Memory::new("sacred", MemoryType::Semantic).with_importance(0.5).as_sacred());

        let results = store.query(&MemoryQuery::new().with_limit(2));
        // Sacred should rank first
        assert_eq!(results[0].content, "sacred");
    }

    #[test]
    fn test_evict_below() {
        let mut store = MemoryStore::new();
        store.store(
            Memory::new("weak", MemoryType::Working)
                .with_confidence(0.1)
                .with_importance(0.1),
        );
        store.store(
            Memory::new("strong", MemoryType::Semantic)
                .with_confidence(0.9)
                .with_importance(0.9),
        );

        store.evict_below(0.5);
        assert_eq!(store.len(), 1);
        assert_eq!(store.by_type(MemoryType::Semantic).len(), 1);
    }

    #[test]
    fn test_sacred_not_evicted() {
        let mut store = MemoryStore::new();
        store.store(
            Memory::new("sacred_weak", MemoryType::Working)
                .with_confidence(0.1)
                .as_sacred(),
        );
        store.evict_below(0.99);
        assert_eq!(store.len(), 1); // sacred survives
    }
}
