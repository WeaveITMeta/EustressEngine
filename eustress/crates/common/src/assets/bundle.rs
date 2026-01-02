//! # AssetBundle - Grouped Assets for Efficient Loading
//!
//! Bundles multiple assets into a single archive for:
//! - Fewer HTTP requests
//! - Better compression (shared dictionary)
//! - Atomic updates (all or nothing)

use super::ContentHash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Compression format for bundles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BundleCompression {
    /// No compression
    None,
    /// Gzip compression
    Gzip,
    /// Zstd compression (better ratio, faster)
    Zstd,
    /// LZ4 compression (fastest)
    Lz4,
}

impl Default for BundleCompression {
    fn default() -> Self {
        Self::Zstd
    }
}

/// Entry in a bundle manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEntry {
    /// Asset name/path within bundle
    pub name: String,
    /// Asset content hash
    pub id: ContentHash,
    /// Offset in archive (bytes)
    pub offset: u64,
    /// Size in archive (bytes, compressed)
    pub size: u64,
    /// Original size (uncompressed)
    pub original_size: u64,
    /// MIME type
    pub mime_type: String,
}

/// Bundle manifest - describes contents of an asset bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Bundle version
    pub version: u32,
    /// Bundle name
    pub name: String,
    /// Compression format
    pub compression: BundleCompression,
    /// Total compressed size
    pub total_size: u64,
    /// Total uncompressed size
    pub total_original_size: u64,
    /// Assets in this bundle
    pub entries: Vec<BundleEntry>,
    /// Creation timestamp
    pub created_at: u64,
    /// Optional description
    pub description: Option<String>,
    /// Tags for organization
    pub tags: Vec<String>,
}

impl BundleManifest {
    /// Create a new empty manifest
    pub fn new(name: &str) -> Self {
        Self {
            version: 1,
            name: name.to_string(),
            compression: BundleCompression::default(),
            total_size: 0,
            total_original_size: 0,
            entries: Vec::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            description: None,
            tags: Vec::new(),
        }
    }
    
    /// Add an entry
    pub fn add_entry(&mut self, entry: BundleEntry) {
        self.total_size += entry.size;
        self.total_original_size += entry.original_size;
        self.entries.push(entry);
    }
    
    /// Get entry by name
    pub fn get_entry(&self, name: &str) -> Option<&BundleEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
    
    /// Get entry by ContentHash
    pub fn get_entry_by_id(&self, id: &ContentHash) -> Option<&BundleEntry> {
        self.entries.iter().find(|e| &e.id == id)
    }
    
    /// List all asset names
    pub fn list_names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }
    
    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.total_original_size == 0 {
            1.0
        } else {
            self.total_size as f64 / self.total_original_size as f64
        }
    }
    
    /// Serialize to RON
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }
    
    /// Deserialize from RON
    pub fn from_ron(s: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(s)
    }
}

/// Asset bundle - manifest + archive data
#[derive(Debug, Clone)]
pub struct AssetBundle {
    /// Bundle manifest
    pub manifest: BundleManifest,
    /// Archive ID (hash of the archive data)
    pub archive_id: ContentHash,
}

impl AssetBundle {
    /// Create from manifest and archive
    pub fn new(manifest: BundleManifest, archive_data: &[u8]) -> Self {
        Self {
            manifest,
            archive_id: ContentHash::from_content(archive_data),
        }
    }
    
    /// Extract a single asset from archive data
    pub fn extract(&self, name: &str, archive_data: &[u8]) -> Option<Vec<u8>> {
        let entry = self.manifest.get_entry(name)?;
        
        let start = entry.offset as usize;
        let end = start + entry.size as usize;
        
        if end > archive_data.len() {
            return None;
        }
        
        let compressed = &archive_data[start..end];
        
        // Decompress based on format
        match self.manifest.compression {
            BundleCompression::None => Some(compressed.to_vec()),
            BundleCompression::Gzip => {
                use std::io::Read;
                let mut decoder = flate2::read::GzDecoder::new(compressed);
                let mut decompressed = Vec::new();
                decoder.read_to_end(&mut decompressed).ok()?;
                Some(decompressed)
            }
            BundleCompression::Zstd => {
                zstd::decode_all(compressed).ok()
            }
            BundleCompression::Lz4 => {
                // LZ4 requires knowing the decompressed size
                let mut decompressed = vec![0u8; entry.original_size as usize];
                lz4_flex::decompress_into(compressed, &mut decompressed).ok()?;
                Some(decompressed)
            }
        }
    }
    
    /// Extract all assets from archive
    pub fn extract_all(&self, archive_data: &[u8]) -> HashMap<String, Vec<u8>> {
        let mut result = HashMap::new();
        
        for entry in &self.manifest.entries {
            if let Some(data) = self.extract(&entry.name, archive_data) {
                result.insert(entry.name.clone(), data);
            }
        }
        
        result
    }
}

/// Builder for creating asset bundles
pub struct BundleBuilder {
    manifest: BundleManifest,
    data: Vec<u8>,
    compression: BundleCompression,
}

impl BundleBuilder {
    /// Create a new bundle builder
    pub fn new(name: &str) -> Self {
        Self {
            manifest: BundleManifest::new(name),
            data: Vec::new(),
            compression: BundleCompression::default(),
        }
    }
    
    /// Set compression format
    pub fn compression(mut self, compression: BundleCompression) -> Self {
        self.compression = compression;
        self.manifest.compression = compression;
        self
    }
    
    /// Add an asset to the bundle
    pub fn add_asset(mut self, name: &str, data: &[u8], mime_type: &str) -> Self {
        let original_size = data.len() as u64;
        
        // Compress
        let compressed = match self.compression {
            BundleCompression::None => data.to_vec(),
            BundleCompression::Gzip => {
                use std::io::Write;
                let mut encoder = flate2::write::GzEncoder::new(
                    Vec::new(),
                    flate2::Compression::default(),
                );
                encoder.write_all(data).unwrap();
                encoder.finish().unwrap()
            }
            BundleCompression::Zstd => {
                zstd::encode_all(data, 3).unwrap_or_else(|_| data.to_vec())
            }
            BundleCompression::Lz4 => {
                lz4_flex::compress_prepend_size(data)
            }
        };
        
        let offset = self.data.len() as u64;
        let size = compressed.len() as u64;
        
        // Add entry
        self.manifest.add_entry(BundleEntry {
            name: name.to_string(),
            id: ContentHash::from_content(data),
            offset,
            size,
            original_size,
            mime_type: mime_type.to_string(),
        });
        
        // Append data
        self.data.extend(compressed);
        
        self
    }
    
    /// Build the bundle
    pub fn build(self) -> (AssetBundle, Vec<u8>) {
        let bundle = AssetBundle::new(self.manifest, &self.data);
        (bundle, self.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bundle_roundtrip() {
        let (bundle, archive) = BundleBuilder::new("test-bundle")
            .compression(BundleCompression::None)
            .add_asset("model.gltf", b"gltf data here", "model/gltf+json")
            .add_asset("texture.png", b"png data here", "image/png")
            .build();
        
        assert_eq!(bundle.manifest.entries.len(), 2);
        
        // Extract
        let model = bundle.extract("model.gltf", &archive).unwrap();
        assert_eq!(model, b"gltf data here");
        
        let texture = bundle.extract("texture.png", &archive).unwrap();
        assert_eq!(texture, b"png data here");
    }
}
