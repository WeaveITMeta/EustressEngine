//! # Progressive Asset Loading
//!
//! LOD-based streaming for smooth loading experiences.
//! Shows placeholder immediately, then streams higher quality.

use super::ContentHash;
use serde::{Deserialize, Serialize};

/// Progressive asset with multiple quality levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressiveAsset {
    /// Asset name
    pub name: String,
    
    /// Placeholder asset (tiny, embedded in scene)
    /// Shown immediately while loading
    pub placeholder: Option<PlaceholderData>,
    
    /// LOD levels (index 0 = highest quality)
    pub lods: Vec<LodLevel>,
    
    /// Full quality asset ID
    pub full_id: ContentHash,
}

/// Placeholder data (embedded in scene for instant display)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceholderData {
    /// Tiny preview (e.g., 16x16 blurred image, simplified mesh)
    pub data: Vec<u8>,
    /// MIME type
    pub mime_type: String,
}

/// Level of detail definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LodLevel {
    /// LOD index (0 = highest quality)
    pub level: u8,
    /// Asset ID for this LOD
    pub id: ContentHash,
    /// Maximum distance to use this LOD (studs)
    pub max_distance: f32,
    /// Approximate size in bytes
    pub size_hint: u64,
}

impl ProgressiveAsset {
    /// Create a new progressive asset
    pub fn new(name: &str, full_id: ContentHash) -> Self {
        Self {
            name: name.to_string(),
            placeholder: None,
            lods: Vec::new(),
            full_id,
        }
    }
    
    /// Add a placeholder
    pub fn with_placeholder(mut self, data: Vec<u8>, mime_type: &str) -> Self {
        self.placeholder = Some(PlaceholderData {
            data,
            mime_type: mime_type.to_string(),
        });
        self
    }
    
    /// Add a LOD level
    pub fn with_lod(mut self, level: u8, id: ContentHash, max_distance: f32, size_hint: u64) -> Self {
        self.lods.push(LodLevel {
            level,
            id,
            max_distance,
            size_hint,
        });
        // Keep sorted by level
        self.lods.sort_by_key(|l| l.level);
        self
    }
    
    /// Get the appropriate LOD for a given distance
    pub fn get_lod_for_distance(&self, distance: f32) -> Option<&LodLevel> {
        // Find the lowest quality LOD that covers this distance
        for lod in self.lods.iter().rev() {
            if distance <= lod.max_distance {
                return Some(lod);
            }
        }
        // If beyond all LODs, use the lowest quality
        self.lods.last()
    }
    
    /// Get the asset ID for a given distance
    pub fn get_id_for_distance(&self, distance: f32) -> ContentHash {
        self.get_lod_for_distance(distance)
            .map(|l| l.id.clone())
            .unwrap_or_else(|| self.full_id.clone())
    }
    
    /// Estimate total download size for all LODs
    pub fn total_size(&self) -> u64 {
        self.lods.iter().map(|l| l.size_hint).sum()
    }
    
    /// Get placeholder data if available
    pub fn get_placeholder(&self) -> Option<&[u8]> {
        self.placeholder.as_ref().map(|p| p.data.as_slice())
    }
}

/// Standard LOD distances (in studs)
pub mod lod_distances {
    /// Close range - full quality
    pub const LOD0: f32 = 10.0;
    /// Medium range - high quality
    pub const LOD1: f32 = 50.0;
    /// Far range - medium quality
    pub const LOD2: f32 = 200.0;
    /// Very far - low quality
    pub const LOD3: f32 = 500.0;
    /// Extreme distance - minimal quality
    pub const LOD4: f32 = 1000.0;
}

/// Builder for progressive assets
pub struct ProgressiveAssetBuilder {
    asset: ProgressiveAsset,
}

impl ProgressiveAssetBuilder {
    /// Create a new builder
    pub fn new(name: &str, full_id: ContentHash) -> Self {
        Self {
            asset: ProgressiveAsset::new(name, full_id),
        }
    }
    
    /// Add placeholder (tiny preview)
    pub fn placeholder(mut self, data: Vec<u8>, mime_type: &str) -> Self {
        self.asset = self.asset.with_placeholder(data, mime_type);
        self
    }
    
    /// Add LOD0 (highest quality after full)
    pub fn lod0(mut self, id: ContentHash, size_hint: u64) -> Self {
        self.asset = self.asset.with_lod(0, id, lod_distances::LOD0, size_hint);
        self
    }
    
    /// Add LOD1
    pub fn lod1(mut self, id: ContentHash, size_hint: u64) -> Self {
        self.asset = self.asset.with_lod(1, id, lod_distances::LOD1, size_hint);
        self
    }
    
    /// Add LOD2
    pub fn lod2(mut self, id: ContentHash, size_hint: u64) -> Self {
        self.asset = self.asset.with_lod(2, id, lod_distances::LOD2, size_hint);
        self
    }
    
    /// Add LOD3
    pub fn lod3(mut self, id: ContentHash, size_hint: u64) -> Self {
        self.asset = self.asset.with_lod(3, id, lod_distances::LOD3, size_hint);
        self
    }
    
    /// Add custom LOD
    pub fn lod(mut self, level: u8, id: ContentHash, max_distance: f32, size_hint: u64) -> Self {
        self.asset = self.asset.with_lod(level, id, max_distance, size_hint);
        self
    }
    
    /// Build the progressive asset
    pub fn build(self) -> ProgressiveAsset {
        self.asset
    }
}

/// Streaming state for a progressive asset
#[derive(Debug, Clone, Default)]
pub struct StreamingState {
    /// Currently loaded LOD level (None = placeholder only)
    pub current_lod: Option<u8>,
    /// Target LOD level based on distance
    pub target_lod: Option<u8>,
    /// Is currently loading?
    pub loading: bool,
    /// Bytes downloaded so far
    pub bytes_downloaded: u64,
    /// Total bytes to download
    pub bytes_total: u64,
}

impl StreamingState {
    /// Get download progress (0.0 - 1.0)
    pub fn progress(&self) -> f32 {
        if self.bytes_total == 0 {
            1.0
        } else {
            self.bytes_downloaded as f32 / self.bytes_total as f32
        }
    }
    
    /// Check if fully loaded
    pub fn is_complete(&self) -> bool {
        self.current_lod == Some(0) || (self.current_lod.is_some() && !self.loading)
    }
    
    /// Check if needs upgrade
    pub fn needs_upgrade(&self) -> bool {
        match (self.current_lod, self.target_lod) {
            (None, Some(_)) => true,
            (Some(current), Some(target)) => current > target,
            _ => false,
        }
    }
}
