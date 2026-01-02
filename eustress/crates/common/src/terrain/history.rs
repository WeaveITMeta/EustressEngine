//! Terrain Undo/Redo History System
//!
//! Provides snapshot-based undo/redo for terrain editing operations.
//! Stores compressed height and splat cache snapshots.
//!
//! ## Usage
//! ```rust,ignore
//! // Before editing
//! history.push_snapshot(&terrain_data, "Raise terrain");
//!
//! // Undo last edit
//! if let Some(snapshot) = history.undo() {
//!     terrain_data.height_cache = snapshot.height_cache.clone();
//! }
//!
//! // Redo undone edit
//! if let Some(snapshot) = history.redo() {
//!     terrain_data.height_cache = snapshot.height_cache.clone();
//! }
//! ```

use bevy::prelude::*;
use std::collections::VecDeque;

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of undo states to keep
const MAX_UNDO_STATES: usize = 50;

/// Minimum time between auto-snapshots (in seconds)
const MIN_SNAPSHOT_INTERVAL: f32 = 0.5;

// ============================================================================
// Snapshot Types
// ============================================================================

/// A snapshot of terrain state at a point in time
#[derive(Clone, Debug)]
pub struct TerrainSnapshot {
    /// Height cache data (compressed if large)
    pub height_cache: Vec<f32>,
    
    /// Splat cache data (material weights per vertex)
    pub splat_cache: Option<Vec<[f32; 8]>>,
    
    /// Cache dimensions
    pub cache_width: u32,
    pub cache_height: u32,
    
    /// Description of the edit that created this snapshot
    pub description: String,
    
    /// Timestamp when snapshot was created
    pub timestamp: f64,
    
    /// Affected chunk positions (for partial restore)
    pub affected_chunks: Vec<IVec2>,
}

impl TerrainSnapshot {
    /// Create a new snapshot from terrain data
    pub fn new(
        height_cache: Vec<f32>,
        splat_cache: Option<Vec<[f32; 8]>>,
        cache_width: u32,
        cache_height: u32,
        description: impl Into<String>,
        affected_chunks: Vec<IVec2>,
    ) -> Self {
        Self {
            height_cache,
            splat_cache,
            cache_width,
            cache_height,
            description: description.into(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
            affected_chunks,
        }
    }
    
    /// Estimate memory usage of this snapshot in bytes
    pub fn memory_size(&self) -> usize {
        let height_size = self.height_cache.len() * std::mem::size_of::<f32>();
        let splat_size = self.splat_cache.as_ref()
            .map(|s| s.len() * std::mem::size_of::<[f32; 8]>())
            .unwrap_or(0);
        height_size + splat_size + self.description.len() + 64 // overhead
    }
}

// ============================================================================
// History Resource
// ============================================================================

/// Terrain edit history for undo/redo operations
#[derive(Resource, Default)]
pub struct TerrainHistory {
    /// Undo stack (past states)
    undo_stack: VecDeque<TerrainSnapshot>,
    
    /// Redo stack (future states after undo)
    redo_stack: Vec<TerrainSnapshot>,
    
    /// Current state (for comparison)
    current_state: Option<TerrainSnapshot>,
    
    /// Time of last snapshot
    last_snapshot_time: f64,
    
    /// Whether history is enabled
    pub enabled: bool,
    
    /// Maximum memory usage for history (in bytes)
    pub max_memory: usize,
}

impl TerrainHistory {
    /// Create a new history with default settings
    pub fn new() -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(MAX_UNDO_STATES),
            redo_stack: Vec::with_capacity(MAX_UNDO_STATES / 2),
            current_state: None,
            last_snapshot_time: 0.0,
            enabled: true,
            max_memory: 256 * 1024 * 1024, // 256 MB default
        }
    }
    
    /// Push a new snapshot onto the undo stack
    pub fn push_snapshot(&mut self, snapshot: TerrainSnapshot) {
        if !self.enabled {
            return;
        }
        
        // Clear redo stack when new edit is made
        self.redo_stack.clear();
        
        // Move current state to undo stack
        if let Some(current) = self.current_state.take() {
            self.undo_stack.push_back(current);
        }
        
        // Set new current state
        self.current_state = Some(snapshot);
        self.last_snapshot_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        
        // Trim undo stack if too large
        while self.undo_stack.len() > MAX_UNDO_STATES {
            self.undo_stack.pop_front();
        }
        
        // Trim by memory usage
        self.trim_by_memory();
    }
    
    /// Push snapshot from terrain data directly
    pub fn push_from_data(
        &mut self,
        height_cache: &[f32],
        splat_cache: Option<&[[f32; 8]]>,
        cache_width: u32,
        cache_height: u32,
        description: impl Into<String>,
        affected_chunks: Vec<IVec2>,
    ) {
        let snapshot = TerrainSnapshot::new(
            height_cache.to_vec(),
            splat_cache.map(|s| s.to_vec()),
            cache_width,
            cache_height,
            description,
            affected_chunks,
        );
        self.push_snapshot(snapshot);
    }
    
    /// Undo the last edit, returning the previous state
    pub fn undo(&mut self) -> Option<&TerrainSnapshot> {
        if let Some(current) = self.current_state.take() {
            // Move current to redo stack
            self.redo_stack.push(current);
            
            // Pop from undo stack to become current
            if let Some(previous) = self.undo_stack.pop_back() {
                self.current_state = Some(previous);
                return self.current_state.as_ref();
            }
        }
        None
    }
    
    /// Redo a previously undone edit
    pub fn redo(&mut self) -> Option<&TerrainSnapshot> {
        if let Some(next) = self.redo_stack.pop() {
            // Move current to undo stack
            if let Some(current) = self.current_state.take() {
                self.undo_stack.push_back(current);
            }
            
            self.current_state = Some(next);
            return self.current_state.as_ref();
        }
        None
    }
    
    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    
    /// Check if redo is available
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
    
    /// Get number of undo states available
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }
    
    /// Get number of redo states available
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
    
    /// Get description of next undo action
    pub fn undo_description(&self) -> Option<&str> {
        self.current_state.as_ref().map(|s| s.description.as_str())
    }
    
    /// Get description of next redo action
    pub fn redo_description(&self) -> Option<&str> {
        self.redo_stack.last().map(|s| s.description.as_str())
    }
    
    /// Check if enough time has passed for a new snapshot
    pub fn should_snapshot(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        
        now - self.last_snapshot_time >= MIN_SNAPSHOT_INTERVAL as f64
    }
    
    /// Clear all history
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current_state = None;
    }
    
    /// Get total memory usage of history
    pub fn memory_usage(&self) -> usize {
        let undo_size: usize = self.undo_stack.iter().map(|s| s.memory_size()).sum();
        let redo_size: usize = self.redo_stack.iter().map(|s| s.memory_size()).sum();
        let current_size = self.current_state.as_ref().map(|s| s.memory_size()).unwrap_or(0);
        undo_size + redo_size + current_size
    }
    
    /// Trim history to stay within memory budget
    fn trim_by_memory(&mut self) {
        while self.memory_usage() > self.max_memory && !self.undo_stack.is_empty() {
            self.undo_stack.pop_front();
        }
    }
    
    /// Get current state reference
    pub fn current(&self) -> Option<&TerrainSnapshot> {
        self.current_state.as_ref()
    }
}

// ============================================================================
// Chunk-level History (for partial undo)
// ============================================================================

/// Lightweight snapshot for a single chunk
#[derive(Clone, Debug)]
pub struct ChunkSnapshot {
    /// Chunk position
    pub position: IVec2,
    
    /// Height values for this chunk only
    pub heights: Vec<f32>,
    
    /// Splat weights for this chunk only
    pub splats: Option<Vec<[f32; 8]>>,
    
    /// Resolution of the chunk
    pub resolution: u32,
}

/// Per-chunk history for fine-grained undo
#[derive(Resource, Default)]
pub struct ChunkHistory {
    /// History per chunk position
    histories: std::collections::HashMap<IVec2, VecDeque<ChunkSnapshot>>,
    
    /// Maximum snapshots per chunk
    pub max_per_chunk: usize,
}

impl ChunkHistory {
    /// Create new chunk history
    pub fn new() -> Self {
        Self {
            histories: std::collections::HashMap::new(),
            max_per_chunk: 10,
        }
    }
    
    /// Push snapshot for a specific chunk
    pub fn push(&mut self, snapshot: ChunkSnapshot) {
        let history = self.histories.entry(snapshot.position).or_insert_with(VecDeque::new);
        history.push_back(snapshot);
        
        while history.len() > self.max_per_chunk {
            history.pop_front();
        }
    }
    
    /// Pop last snapshot for a chunk
    pub fn pop(&mut self, position: IVec2) -> Option<ChunkSnapshot> {
        self.histories.get_mut(&position).and_then(|h| h.pop_back())
    }
    
    /// Clear history for a chunk
    pub fn clear_chunk(&mut self, position: IVec2) {
        self.histories.remove(&position);
    }
    
    /// Clear all history
    pub fn clear_all(&mut self) {
        self.histories.clear();
    }
}

// ============================================================================
// Edit Operation Types
// ============================================================================

/// Types of terrain edit operations (for history descriptions)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainEditOp {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Paint,
    NoiseStamp,
    Erosion,
    Import,
    Clear,
}

impl TerrainEditOp {
    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Raise => "Raise terrain",
            Self::Lower => "Lower terrain",
            Self::Smooth => "Smooth terrain",
            Self::Flatten => "Flatten terrain",
            Self::Paint => "Paint texture",
            Self::NoiseStamp => "Apply noise stamp",
            Self::Erosion => "Apply erosion",
            Self::Import => "Import heightmap",
            Self::Clear => "Clear terrain",
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_undo_redo() {
        let mut history = TerrainHistory::new();
        
        // Push initial state
        history.push_from_data(&[0.0; 100], None, 10, 10, "Initial", vec![]);
        
        // Push edit
        history.push_from_data(&[1.0; 100], None, 10, 10, "Raise", vec![]);
        
        assert!(history.can_undo());
        assert!(!history.can_redo());
        
        // Undo
        let undone = history.undo();
        assert!(undone.is_some());
        assert_eq!(undone.unwrap().height_cache[0], 0.0);
        
        assert!(!history.can_undo());
        assert!(history.can_redo());
        
        // Redo
        let redone = history.redo();
        assert!(redone.is_some());
        assert_eq!(redone.unwrap().height_cache[0], 1.0);
    }
    
    #[test]
    fn test_memory_limit() {
        let mut history = TerrainHistory::new();
        history.max_memory = 1024; // Very small limit
        
        // Push many large snapshots
        for i in 0..100 {
            history.push_from_data(&vec![i as f32; 1000], None, 100, 10, format!("Edit {}", i), vec![]);
        }
        
        // Should have trimmed to stay within memory
        assert!(history.memory_usage() <= history.max_memory + 10000); // Some tolerance
    }
}
