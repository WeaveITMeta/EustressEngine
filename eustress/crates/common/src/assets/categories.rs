// ============================================================================
// Asset Categories - Roblox-style Asset Organization
// ============================================================================
//
// Organizes assets into categories like Roblox's asset library:
// - Meshes: 3D models (GLTF, FBX, OBJ)
// - Images: Textures, decals, skyboxes
// - Audio: Sounds, music
// - Animations: Keyframe sequences
// - Terrain: Heightmaps, regions, splatmaps
// - Scripts: Soul scripts
// - Packages: Bundled assets (models with textures)
//
// Table of Contents:
// 1. Asset Category Enum
// 2. Asset Metadata
// 3. Category Registry
// 4. Upload/Download Handlers
// ============================================================================

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::collections::HashMap;

use super::{ContentHash, AssetSource};

// ============================================================================
// 1. Asset Category Enum
// ============================================================================

/// Asset categories (like Roblox's asset types)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub enum AssetCategory {
    /// 3D meshes (GLTF, FBX, OBJ, STL)
    Mesh,
    /// Images/textures (PNG, JPG, HDR)
    Image,
    /// Audio files (WAV, OGG, MP3)
    Audio,
    /// Animation data
    Animation,
    /// Terrain heightmaps and regions
    Terrain,
    /// Soul scripts
    Script,
    /// Bundled packages (model + textures)
    Package,
    /// Point cloud data
    PointCloud,
    /// Video files
    Video,
    /// Font files
    Font,
    /// Generic/unknown
    Other,
}

impl AssetCategory {
    /// Detect category from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            // Meshes
            "gltf" | "glb" | "fbx" | "obj" | "stl" | "dae" | "usdz" => Self::Mesh,
            
            // Images
            "png" | "jpg" | "jpeg" | "hdr" | "exr" | "tga" | "bmp" | "webp" | "svg" => Self::Image,
            
            // Audio
            "wav" | "ogg" | "mp3" | "flac" | "aac" => Self::Audio,
            
            // Animation
            "anim" | "bvh" => Self::Animation,
            
            // Terrain
            "dem" | "dsm" | "dtm" | "hgt" | "asc" | "terrain" | "heightmap" => Self::Terrain,
            
            // Scripts
            "soul" | "lua" => Self::Script,
            
            // Packages
            "eustresspack" | "rbxm" | "rbxmx" => Self::Package,
            
            // Point clouds
            "ply" | "las" | "laz" | "pts" | "xyz" | "pcd" | "e57" => Self::PointCloud,
            
            // Video
            "mp4" | "webm" | "mov" | "avi" => Self::Video,
            
            // Fonts
            "ttf" | "otf" | "woff" | "woff2" => Self::Font,
            
            _ => Self::Other,
        }
    }
    
    /// Get category from path
    pub fn from_path(path: &Path) -> Self {
        path.extension()
            .and_then(|e| e.to_str())
            .map(Self::from_extension)
            .unwrap_or(Self::Other)
    }
    
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Mesh => "Meshes",
            Self::Image => "Images",
            Self::Audio => "Audio",
            Self::Animation => "Animations",
            Self::Terrain => "Terrain",
            Self::Script => "Scripts",
            Self::Package => "Packages",
            Self::PointCloud => "Point Clouds",
            Self::Video => "Videos",
            Self::Font => "Fonts",
            Self::Other => "Other",
        }
    }
    
    /// Get icon name for UI
    pub fn icon_name(&self) -> &'static str {
        match self {
            Self::Mesh => "mesh",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Animation => "animation",
            Self::Terrain => "terrain",
            Self::Script => "script",
            Self::Package => "package",
            Self::PointCloud => "pointcloud",
            Self::Video => "video",
            Self::Font => "font",
            Self::Other => "file",
        }
    }
    
    /// Get supported extensions for this category
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Mesh => &["gltf", "glb", "fbx", "obj", "stl", "dae", "usdz"],
            Self::Image => &["png", "jpg", "jpeg", "hdr", "exr", "tga", "bmp", "webp"],
            Self::Audio => &["wav", "ogg", "mp3", "flac"],
            Self::Animation => &["anim", "bvh"],
            Self::Terrain => &["dem", "dsm", "dtm", "hgt", "asc", "tif", "terrain"],
            Self::Script => &["soul"],
            Self::Package => &["eustresspack"],
            Self::PointCloud => &["ply", "las", "laz", "pts", "xyz", "pcd", "e57"],
            Self::Video => &["mp4", "webm"],
            Self::Font => &["ttf", "otf", "woff2"],
            Self::Other => &[],
        }
    }
}

// ============================================================================
// 2. Asset Metadata
// ============================================================================

/// Metadata for an uploaded asset
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct AssetMetadata {
    /// Content hash (unique identifier)
    pub hash: String,
    
    /// Original filename
    pub name: String,
    
    /// Asset category
    pub category: AssetCategory,
    
    /// File size in bytes
    pub size: u64,
    
    /// MIME type
    pub mime_type: String,
    
    /// Upload timestamp
    pub uploaded_at: String,
    
    /// Uploader user ID (optional)
    pub uploader_id: Option<String>,
    
    /// Description
    pub description: String,
    
    /// Tags for search
    pub tags: Vec<String>,
    
    /// Thumbnail hash (for preview)
    pub thumbnail_hash: Option<String>,
    
    /// Category-specific metadata
    pub extra: AssetExtraMetadata,
}

/// Category-specific metadata
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct AssetExtraMetadata {
    // Mesh metadata
    pub vertex_count: Option<u32>,
    pub triangle_count: Option<u32>,
    pub has_normals: Option<bool>,
    pub has_uvs: Option<bool>,
    pub has_colors: Option<bool>,
    pub bounds_min: Option<[f32; 3]>,
    pub bounds_max: Option<[f32; 3]>,
    
    // Image metadata
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub channels: Option<u8>,
    pub is_hdr: Option<bool>,
    
    // Audio metadata
    pub duration_secs: Option<f32>,
    pub sample_rate: Option<u32>,
    pub channels_audio: Option<u8>,
    
    // Terrain metadata
    pub terrain_width: Option<u32>,
    pub terrain_height: Option<u32>,
    pub cell_size: Option<f32>,
    pub min_elevation: Option<f32>,
    pub max_elevation: Option<f32>,
    
    // Point cloud metadata
    pub point_count: Option<u64>,
    pub has_point_colors: Option<bool>,
    pub has_point_normals: Option<bool>,
}

impl AssetMetadata {
    /// Create new metadata
    pub fn new(hash: String, name: String, category: AssetCategory, size: u64) -> Self {
        Self {
            hash,
            name,
            category,
            size,
            mime_type: String::new(),
            uploaded_at: chrono::Utc::now().to_rfc3339(),
            uploader_id: None,
            description: String::new(),
            tags: Vec::new(),
            thumbnail_hash: None,
            extra: AssetExtraMetadata::default(),
        }
    }
    
    /// Set MIME type from extension
    pub fn with_mime_from_extension(mut self, ext: &str) -> Self {
        self.mime_type = mime_from_extension(ext);
        self
    }
}

/// Get MIME type from extension
fn mime_from_extension(ext: &str) -> String {
    match ext.to_lowercase().as_str() {
        // Meshes
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "fbx" => "application/octet-stream",
        "obj" => "model/obj",
        "stl" => "model/stl",
        
        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "hdr" => "image/vnd.radiance",
        "webp" => "image/webp",
        
        // Audio
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        
        // Video
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        
        _ => "application/octet-stream",
    }.to_string()
}

// ============================================================================
// 3. Category Registry
// ============================================================================

/// Registry of assets organized by category
#[derive(Resource, Default)]
pub struct AssetRegistry {
    /// Assets indexed by hash
    pub by_hash: HashMap<String, AssetMetadata>,
    
    /// Assets indexed by category
    pub by_category: HashMap<AssetCategory, Vec<String>>,
    
    /// Search index (tag -> hashes)
    pub by_tag: HashMap<String, Vec<String>>,
    
    /// Recently used assets
    pub recent: Vec<String>,
    
    /// Favorites
    pub favorites: Vec<String>,
}

impl AssetRegistry {
    /// Register a new asset
    pub fn register(&mut self, metadata: AssetMetadata) {
        let hash = metadata.hash.clone();
        let category = metadata.category;
        
        // Index by tag
        for tag in &metadata.tags {
            self.by_tag
                .entry(tag.clone())
                .or_default()
                .push(hash.clone());
        }
        
        // Index by category
        self.by_category
            .entry(category)
            .or_default()
            .push(hash.clone());
        
        // Store metadata
        self.by_hash.insert(hash.clone(), metadata);
        
        // Add to recent
        self.recent.retain(|h| h != &hash);
        self.recent.insert(0, hash);
        if self.recent.len() > 50 {
            self.recent.truncate(50);
        }
    }
    
    /// Get assets by category
    pub fn get_by_category(&self, category: AssetCategory) -> Vec<&AssetMetadata> {
        self.by_category
            .get(&category)
            .map(|hashes| {
                hashes.iter()
                    .filter_map(|h| self.by_hash.get(h))
                    .collect()
            })
            .unwrap_or_default()
    }
    
    /// Search assets by name or tag
    pub fn search(&self, query: &str) -> Vec<&AssetMetadata> {
        let query_lower = query.to_lowercase();
        
        self.by_hash.values()
            .filter(|m| {
                m.name.to_lowercase().contains(&query_lower)
                    || m.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
                    || m.description.to_lowercase().contains(&query_lower)
            })
            .collect()
    }
    
    /// Get recent assets
    pub fn get_recent(&self, limit: usize) -> Vec<&AssetMetadata> {
        self.recent.iter()
            .take(limit)
            .filter_map(|h| self.by_hash.get(h))
            .collect()
    }
    
    /// Toggle favorite
    pub fn toggle_favorite(&mut self, hash: &str) {
        if let Some(pos) = self.favorites.iter().position(|h| h == hash) {
            self.favorites.remove(pos);
        } else {
            self.favorites.push(hash.to_string());
        }
    }
    
    /// Check if favorited
    pub fn is_favorite(&self, hash: &str) -> bool {
        self.favorites.contains(&hash.to_string())
    }
}

// ============================================================================
// 4. Mesh-Specific Types
// ============================================================================

/// Uploaded mesh asset (like Roblox MeshId)
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct MeshAsset {
    /// Content hash
    pub hash: String,
    
    /// Original format
    pub format: MeshFormat,
    
    /// Vertex count
    pub vertex_count: u32,
    
    /// Triangle count
    pub triangle_count: u32,
    
    /// Bounding box
    pub bounds: MeshBounds,
    
    /// LOD levels available
    pub lod_levels: Vec<MeshLOD>,
    
    /// Material slots
    pub material_slots: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Reflect)]
pub enum MeshFormat {
    GLTF,
    GLB,
    FBX,
    OBJ,
    STL,
}

#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct MeshBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl MeshBounds {
    pub fn size(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
    
    pub fn center(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
            (self.min[2] + self.max[2]) / 2.0,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct MeshLOD {
    /// LOD level (0 = highest detail)
    pub level: u8,
    
    /// Content hash for this LOD
    pub hash: String,
    
    /// Triangle count at this LOD
    pub triangle_count: u32,
    
    /// Screen size threshold (0.0 - 1.0)
    pub screen_size: f32,
}

// ============================================================================
// 5. Terrain Region Types
// ============================================================================

/// Terrain region asset (saveable/loadable terrain chunk)
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct TerrainRegion {
    /// Content hash
    pub hash: String,
    
    /// Region name
    pub name: String,
    
    /// Grid position in terrain
    pub grid_position: [i32; 2],
    
    /// World size in meters
    pub world_size: [f32; 2],
    
    /// Resolution (vertices per side)
    pub resolution: u32,
    
    /// Height data hash
    pub heightmap_hash: String,
    
    /// Splatmap hash (optional)
    pub splatmap_hash: Option<String>,
    
    /// Elevation bounds
    pub min_elevation: f32,
    pub max_elevation: f32,
    
    /// Source format (DEM, GeoTIFF, etc.)
    pub source_format: Option<String>,
    
    /// Geographic bounds (optional)
    pub geo_bounds: Option<GeoBounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct GeoBounds {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

/// Collection of terrain regions
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct TerrainRegionSet {
    /// All regions
    pub regions: Vec<TerrainRegion>,
    
    /// Global terrain settings
    pub settings: TerrainRegionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct TerrainRegionSettings {
    /// Chunk size in meters
    pub chunk_size: f32,
    
    /// Default resolution
    pub default_resolution: u32,
    
    /// Height scale
    pub height_scale: f32,
    
    /// Material layers
    pub material_layers: Vec<TerrainMaterialLayer>,
}

impl Default for TerrainRegionSettings {
    fn default() -> Self {
        Self {
            chunk_size: 128.0,
            default_resolution: 128,
            height_scale: 1.0,
            material_layers: vec![
                TerrainMaterialLayer {
                    name: "Grass".to_string(),
                    texture_hash: None,
                    normal_hash: None,
                    tiling: 10.0,
                },
                TerrainMaterialLayer {
                    name: "Rock".to_string(),
                    texture_hash: None,
                    normal_hash: None,
                    tiling: 5.0,
                },
                TerrainMaterialLayer {
                    name: "Sand".to_string(),
                    texture_hash: None,
                    normal_hash: None,
                    tiling: 8.0,
                },
                TerrainMaterialLayer {
                    name: "Snow".to_string(),
                    texture_hash: None,
                    normal_hash: None,
                    tiling: 12.0,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct TerrainMaterialLayer {
    pub name: String,
    pub texture_hash: Option<String>,
    pub normal_hash: Option<String>,
    pub tiling: f32,
}
