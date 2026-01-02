// ============================================================================
// Point Cloud Module - Eustress Engine Intermediary Pipeline
// ============================================================================
//
// Eustress Engine serves as the intermediary between high-detail reality
// capture formats and optimized client-ready geometry.
//
// ## Pipeline Overview
//
// ```text
// ┌─────────────────────────────────────────────────────────────────────────┐
// │                        INPUT FORMATS                                     │
// │  LiDAR (LAS/LAZ) │ Photogrammetry (PLY) │ NeRF/Gaussian │ Depth Cameras │
// └─────────────────────────────────────────────────────────────────────────┘
//                                    │
//                                    ▼
// ┌─────────────────────────────────────────────────────────────────────────┐
// │                    EUSTRESS ENGINE PROCESSING                            │
// │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐       │
// │  │ Import  │→ │  Clean  │→ │Decimate │→ │ Octree  │→ │   LOD   │       │
// │  └─────────┘  └─────────┘  └─────────┘  └─────────┘  └─────────┘       │
// │                                                            │            │
// │  ┌─────────────────────────────────────────────────────────┘            │
// │  │                                                                      │
// │  ▼                                                                      │
// │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                     │
// │  │   Surface   │  │    Mesh     │  │   Texture   │                     │
// │  │Reconstruct  │→ │ Simplify    │→ │    Bake     │                     │
// │  └─────────────┘  └─────────────┘  └─────────────┘                     │
// └─────────────────────────────────────────────────────────────────────────┘
//                                    │
//                                    ▼
// ┌─────────────────────────────────────────────────────────────────────────┐
// │                       OUTPUT FORMATS                                     │
// │  .eustress (Client) │ .eustressengine (Engine) │ Streaming Chunks       │
// └─────────────────────────────────────────────────────────────────────────┘
// ```
//
// ## Modules
//
// - `core` - Point cloud data structures, formats, streaming protocol
// - `processing` - Import, clean, decimate, octree, LOD pipeline
// - `mesh_optimization` - Surface reconstruction, simplification, texturing
//
// ## Usage
//
// ```rust
// use eustress_common::pointcloud::{
//     PointCloudProcessor, ProcessingConfig,
//     reconstruct_surface, simplify_mesh,
// };
//
// // Process a point cloud for client
// let config = ProcessingConfig::client_optimized();
// let mut processor = PointCloudProcessor::new(config);
// let result = processor.process(input_path, output_path);
// ```
// ============================================================================

pub mod core;
pub mod processing;
pub mod mesh_optimization;
pub mod formats;
pub mod elevation_import;

// Re-export core types
pub use core::{
    // Point data
    PointCloudPoint, PointCloudPointNormal, PointCloudLOD,
    // Formats
    PointCloudFormat, PointCloudImportSettings, CoordinateSystem,
    // Streaming
    OctreeNode, PointCloudHeader,
    StreamingRequest, StreamingChunk, StreamingStats,
    // Components
    PointCloud, PointCloudChunk,
    // Performance
    performance,
    // Utilities
    calculate_bandwidth_requirements, BandwidthRequirements,
};

// Re-export processing types
pub use processing::{
    // Pipeline
    PointCloudProcessor, ProcessingConfig, ProcessingStage, ProcessingProgress, ProcessingResult,
    // Batch processing
    BatchProcessor, BatchJob, BatchJobStatus,
    // Octree
    Octree,
};

// Re-export mesh optimization types
pub use mesh_optimization::{
    // Reconstruction
    ReconstructionMethod, ReconstructionSettings, ReconstructedMesh,
    reconstruct_surface,
    // Simplification
    SimplificationSettings, simplify_mesh,
    // LOD
    MeshLODSet, generate_mesh_lods,
    // Texturing
    bake_colors_to_texture,
    // Instancing
    InstanceGroup, detect_instances,
};

// Re-export format types
pub use formats::{
    // Format enums
    MeshFormat, PointCloudFileFormat, ElevationFormat, ImportFormat,
    // Mesh import/export
    ImportedMesh, MeshImportResult, ImportedMaterial,
    import_mesh, export_obj, export_stl, export_gltf,
    // Point cloud import/export
    import_pts, import_dxf_points, export_ply,
    // Elevation data
    ElevationData, import_elevation, import_asc, import_hgt, import_geotiff,
    // Utilities
    detect_format, supported_extensions, is_supported,
};

// Re-export elevation import for terrain
pub use elevation_import::{
    // Configuration
    ElevationImportConfig,
    CoordinateSystem as ElevationCoordSystem,
    // Results
    ElevationImportResult, TerrainChunk,
    // Main functions
    import_elevation_to_terrain, elevation_to_terrain,
    import_dem_to_terrain, import_geotiff_to_terrain,
    import_to_terrain_with_config,
    // Point cloud conversion
    elevation_to_point_cloud, preview_elevation_as_points,
    // Chunking
    split_into_chunks,
    // Coordinate transforms
    geographic_to_local, utm_to_local,
};

// ============================================================================
// High-Level API
// ============================================================================

use std::path::Path;

/// Process a point cloud file with default settings
pub fn process_point_cloud(
    input: &Path,
    output: &Path,
) -> ProcessingResult {
    let config = ProcessingConfig::default();
    let mut processor = PointCloudProcessor::new(config);
    processor.process(input, output)
}

/// Process a point cloud optimized for client delivery
pub fn process_for_client(
    input: &Path,
    output: &Path,
) -> ProcessingResult {
    let config = ProcessingConfig::client_optimized();
    let mut processor = PointCloudProcessor::new(config);
    processor.process(input, output)
}

/// Process a point cloud optimized for XR/VR
pub fn process_for_xr(
    input: &Path,
    output: &Path,
) -> ProcessingResult {
    let config = ProcessingConfig::xr_optimized();
    let mut processor = PointCloudProcessor::new(config);
    processor.process(input, output)
}

/// Quick preview processing
pub fn process_preview(
    input: &Path,
    output: &Path,
) -> ProcessingResult {
    let config = ProcessingConfig::preview();
    let mut processor = PointCloudProcessor::new(config);
    processor.process(input, output)
}

/// Full quality processing
pub fn process_high_quality(
    input: &Path,
    output: &Path,
) -> ProcessingResult {
    let config = ProcessingConfig::high_quality();
    let mut processor = PointCloudProcessor::new(config);
    processor.process(input, output)
}

// ============================================================================
// Conversion Pipeline
// ============================================================================

/// Convert point cloud to optimized mesh
pub fn point_cloud_to_mesh(
    points: &[bevy::prelude::Vec3],
    normals: Option<&[bevy::prelude::Vec3]>,
    settings: &ReconstructionSettings,
) -> Result<ReconstructedMesh, String> {
    reconstruct_surface(points, normals, settings)
}

/// Full conversion pipeline: points → mesh → simplified → LOD
pub fn full_conversion_pipeline(
    points: &[bevy::prelude::Vec3],
    normals: Option<&[bevy::prelude::Vec3]>,
    target_triangles: usize,
    lod_count: usize,
) -> Result<MeshLODSet, String> {
    // Reconstruct surface
    let settings = ReconstructionSettings::default();
    let mesh = reconstruct_surface(points, normals, &settings)?;
    
    // Simplify
    let simp_settings = SimplificationSettings {
        target_triangles,
        ..Default::default()
    };
    let simplified = simplify_mesh(&mesh, &simp_settings)?;
    
    // Generate LODs
    let ratios: Vec<f32> = (1..lod_count)
        .map(|i| 1.0 / (2.0_f32).powi(i as i32))
        .collect();
    
    generate_mesh_lods(&simplified, lod_count, &ratios)
}
