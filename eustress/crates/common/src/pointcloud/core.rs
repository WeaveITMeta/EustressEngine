// ============================================================================
// Point Cloud Support - Streaming Reality at 60 FPS
// ============================================================================
//
// High-performance point cloud system for:
// - LiDAR scans (iPhone Pro, Android depth sensors)
// - Photogrammetry (Gaussian splatting, NeRF exports)
// - Real-time depth camera streams (Azure Kinect, Intel RealSense)
// - Pre-scanned environments (Matterport, Polycam exports)
//
// Table of Contents:
// 1. Point Cloud Data Structures
// 2. Streaming Format (LOD-based octree)
// 3. Import/Export Formats
// 4. Real-time Streaming Protocol
// ============================================================================

use bevy::prelude::*;
use serde::{Serialize, Deserialize};

// ============================================================================
// 1. Point Cloud Data Structures
// ============================================================================

/// Single point in a point cloud
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct PointCloudPoint {
    /// Position (12 bytes)
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Color (4 bytes) - RGBA packed
    pub color: u32,
}

impl PointCloudPoint {
    pub const SIZE: usize = 16;
    
    pub fn new(position: Vec3, color: Color) -> Self {
        let rgba = color.to_srgba();
        let packed = ((rgba.red * 255.0) as u32) << 24
            | ((rgba.green * 255.0) as u32) << 16
            | ((rgba.blue * 255.0) as u32) << 8
            | ((rgba.alpha * 255.0) as u32);
        Self {
            x: position.x,
            y: position.y,
            z: position.z,
            color: packed,
        }
    }
    
    pub fn position(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
    
    pub fn color_rgba(&self) -> [u8; 4] {
        [
            ((self.color >> 24) & 0xFF) as u8,
            ((self.color >> 16) & 0xFF) as u8,
            ((self.color >> 8) & 0xFF) as u8,
            (self.color & 0xFF) as u8,
        ]
    }
}

/// Extended point with normal (for surface reconstruction)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct PointCloudPointNormal {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub nx: f32,
    pub ny: f32,
    pub nz: f32,
    pub color: u32,
    pub _padding: u32, // Align to 32 bytes
}

impl PointCloudPointNormal {
    pub const SIZE: usize = 32;
}

/// Point cloud Level of Detail
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointCloudLOD {
    /// Full resolution - all points
    LOD0,
    /// 1/8 points (every 2nd in each dimension)
    LOD1,
    /// 1/64 points
    LOD2,
    /// 1/512 points
    LOD3,
    /// 1/4096 points (preview only)
    LOD4,
}

impl PointCloudLOD {
    pub fn decimation_factor(&self) -> usize {
        match self {
            PointCloudLOD::LOD0 => 1,
            PointCloudLOD::LOD1 => 8,
            PointCloudLOD::LOD2 => 64,
            PointCloudLOD::LOD3 => 512,
            PointCloudLOD::LOD4 => 4096,
        }
    }
    
    /// Get LOD for distance from camera
    pub fn from_distance(distance: f32) -> Self {
        match distance {
            d if d < 5.0 => PointCloudLOD::LOD0,
            d if d < 20.0 => PointCloudLOD::LOD1,
            d if d < 50.0 => PointCloudLOD::LOD2,
            d if d < 100.0 => PointCloudLOD::LOD3,
            _ => PointCloudLOD::LOD4,
        }
    }
}

// ============================================================================
// 2. Streaming Format (LOD-based Octree)
// ============================================================================

/// Octree node for spatial streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctreeNode {
    /// Bounding box center
    pub center: Vec3,
    /// Half-size of bounding box
    pub half_size: f32,
    /// Number of points in this node (all LODs combined)
    pub point_count: u32,
    /// File offset for this node's point data
    pub data_offset: u64,
    /// Compressed size of point data
    pub compressed_size: u32,
    /// Child node indices (0 = no child)
    pub children: [u32; 8],
    /// LOD level this node represents
    pub lod: u8,
}

impl OctreeNode {
    /// Check if point is within this node's bounds
    pub fn contains(&self, point: Vec3) -> bool {
        let min = self.center - Vec3::splat(self.half_size);
        let max = self.center + Vec3::splat(self.half_size);
        point.x >= min.x && point.x <= max.x
            && point.y >= min.y && point.y <= max.y
            && point.z >= min.z && point.z <= max.z
    }
    
    /// Get child index for a point (0-7)
    pub fn child_index(&self, point: Vec3) -> usize {
        let mut index = 0;
        if point.x >= self.center.x { index |= 1; }
        if point.y >= self.center.y { index |= 2; }
        if point.z >= self.center.z { index |= 4; }
        index
    }
}

/// Point cloud file header for streaming format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointCloudHeader {
    /// Magic number "EUSTPCD\0"
    pub magic: [u8; 8],
    /// Format version
    pub version: u32,
    /// Total point count (all LODs)
    pub total_points: u64,
    /// Bounding box min
    pub bounds_min: Vec3,
    /// Bounding box max
    pub bounds_max: Vec3,
    /// Number of octree nodes
    pub node_count: u32,
    /// Offset to octree node array
    pub nodes_offset: u64,
    /// Offset to point data start
    pub data_offset: u64,
    /// Coordinate system (0 = Y-up, 1 = Z-up)
    pub coord_system: u8,
    /// Point format (0 = XYZ+RGBA, 1 = XYZ+Normal+RGBA)
    pub point_format: u8,
    /// Compression (0 = none, 1 = zstd, 2 = lz4)
    pub compression: u8,
}

impl Default for PointCloudHeader {
    fn default() -> Self {
        Self {
            magic: *b"EUSTPCD\0",
            version: 1,
            total_points: 0,
            bounds_min: Vec3::ZERO,
            bounds_max: Vec3::ZERO,
            node_count: 0,
            nodes_offset: 0,
            data_offset: 0,
            coord_system: 0,
            point_format: 0,
            compression: 1, // zstd default
        }
    }
}

// ============================================================================
// 3. Import/Export Formats
// ============================================================================

/// Supported point cloud import formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointCloudFormat {
    /// Stanford PLY (ASCII or binary)
    PLY,
    /// LAS/LAZ (LiDAR standard)
    LAS,
    /// E57 (3D imaging standard)
    E57,
    /// XYZ (simple ASCII)
    XYZ,
    /// PCD (Point Cloud Library format)
    PCD,
    /// Potree (octree-based web format)
    Potree,
    /// Gaussian Splatting format
    GaussianSplat,
    /// Apple Reality Capture (USDZ with points)
    RealityCapture,
    /// Polycam export
    Polycam,
}

impl PointCloudFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "ply" => Some(Self::PLY),
            "las" | "laz" => Some(Self::LAS),
            "e57" => Some(Self::E57),
            "xyz" | "txt" => Some(Self::XYZ),
            "pcd" => Some(Self::PCD),
            "json" if ext.contains("potree") => Some(Self::Potree),
            "splat" => Some(Self::GaussianSplat),
            "usdz" | "reality" => Some(Self::RealityCapture),
            _ => None,
        }
    }
}

/// Point cloud import settings
#[derive(Debug, Clone)]
pub struct PointCloudImportSettings {
    /// Target coordinate system
    pub coord_system: CoordinateSystem,
    /// Scale factor (1.0 = meters)
    pub scale: f32,
    /// Offset to apply
    pub offset: Vec3,
    /// Maximum points to import (0 = unlimited)
    pub max_points: usize,
    /// Generate LOD levels
    pub generate_lod: bool,
    /// Target LOD levels
    pub lod_levels: u8,
    /// Voxel size for deduplication (0 = no dedup)
    pub voxel_size: f32,
}

impl Default for PointCloudImportSettings {
    fn default() -> Self {
        Self {
            coord_system: CoordinateSystem::YUp,
            scale: 1.0,
            offset: Vec3::ZERO,
            max_points: 0,
            generate_lod: true,
            lod_levels: 5,
            voxel_size: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinateSystem {
    YUp,  // Bevy/Eustress default
    ZUp,  // CAD, some scanners
}

// ============================================================================
// 4. Real-time Streaming Protocol
// ============================================================================

/// Streaming chunk request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingRequest {
    /// Camera position for LOD calculation
    pub camera_position: Vec3,
    /// Camera frustum planes (for culling)
    pub frustum: [Vec4; 6],
    /// Maximum points to stream this frame
    pub budget_points: u32,
    /// Maximum bytes to stream this frame
    pub budget_bytes: u32,
    /// Priority nodes (user is looking at)
    pub priority_nodes: Vec<u32>,
}

/// Streaming chunk response
#[derive(Debug, Clone)]
pub struct StreamingChunk {
    /// Node ID this chunk belongs to
    pub node_id: u32,
    /// LOD level
    pub lod: PointCloudLOD,
    /// Compressed point data
    pub data: Vec<u8>,
    /// Number of points in this chunk
    pub point_count: u32,
}

/// Streaming statistics
#[derive(Debug, Default, Clone)]
pub struct StreamingStats {
    /// Points currently loaded
    pub points_loaded: u64,
    /// Bytes in memory
    pub memory_bytes: u64,
    /// Points streamed this frame
    pub points_this_frame: u32,
    /// Bytes streamed this frame
    pub bytes_this_frame: u32,
    /// Average latency (ms)
    pub avg_latency_ms: f32,
    /// Cache hit rate
    pub cache_hit_rate: f32,
}

// ============================================================================
// 5. Bevy Components
// ============================================================================

/// Point cloud asset component
#[derive(Component, Debug, Clone)]
pub struct PointCloud {
    /// Asset path or URL
    pub source: String,
    /// Current LOD being rendered
    pub current_lod: PointCloudLOD,
    /// Transform offset
    pub offset: Vec3,
    /// Scale
    pub scale: f32,
    /// Whether streaming is enabled
    pub streaming: bool,
    /// Point size for rendering
    pub point_size: f32,
    /// Render as splats (for Gaussian splatting)
    pub splat_rendering: bool,
}

impl Default for PointCloud {
    fn default() -> Self {
        Self {
            source: String::new(),
            current_lod: PointCloudLOD::LOD2,
            offset: Vec3::ZERO,
            scale: 1.0,
            streaming: true,
            point_size: 2.0,
            splat_rendering: false,
        }
    }
}

/// Marker for entities that are part of a streamed point cloud
#[derive(Component)]
pub struct PointCloudChunk {
    pub cloud_entity: Entity,
    pub node_id: u32,
    pub lod: PointCloudLOD,
}

// ============================================================================
// 6. Performance Targets
// ============================================================================

/// Performance budget for 60 FPS streaming
pub mod performance {
    /// Target frame time (16.67ms for 60 FPS)
    pub const TARGET_FRAME_MS: f32 = 16.67;
    
    /// Max time for point cloud streaming per frame
    pub const STREAMING_BUDGET_MS: f32 = 2.0;
    
    /// Max points to upload to GPU per frame
    pub const MAX_POINTS_PER_FRAME: u32 = 500_000;
    
    /// Max bytes to decompress per frame
    pub const MAX_DECOMPRESS_BYTES: u32 = 8 * 1024 * 1024; // 8 MB
    
    /// Target memory budget for point clouds
    pub const MEMORY_BUDGET_MB: u64 = 512;
    
    /// Cache size for recently viewed chunks
    pub const CHUNK_CACHE_SIZE: usize = 256;
    
    /// Prefetch distance (in node half-sizes)
    pub const PREFETCH_DISTANCE: f32 = 2.0;
}

// ============================================================================
// 7. Bandwidth Calculations
// ============================================================================

/// Calculate required bandwidth for streaming
pub fn calculate_bandwidth_requirements(
    total_points: u64,
    view_distance: f32,
    movement_speed: f32, // studs per second
) -> BandwidthRequirements {
    // Estimate visible points based on view distance
    let visible_fraction = (view_distance / 1000.0).min(1.0);
    let visible_points = (total_points as f64 * visible_fraction as f64) as u64;
    
    // Points that need streaming when moving
    let refresh_rate = movement_speed / view_distance;
    let points_per_second = (visible_points as f64 * refresh_rate as f64) as u64;
    
    // Bytes per second (16 bytes per point, ~4x compression)
    let raw_bytes_per_second = points_per_second * 16;
    let compressed_bytes_per_second = raw_bytes_per_second / 4;
    
    BandwidthRequirements {
        visible_points,
        points_per_second,
        raw_mbps: raw_bytes_per_second as f64 / 1_000_000.0,
        compressed_mbps: compressed_bytes_per_second as f64 / 1_000_000.0,
        feasible_60fps: compressed_bytes_per_second < 100_000_000, // < 100 MB/s
    }
}

#[derive(Debug, Clone)]
pub struct BandwidthRequirements {
    pub visible_points: u64,
    pub points_per_second: u64,
    pub raw_mbps: f64,
    pub compressed_mbps: f64,
    pub feasible_60fps: bool,
}
