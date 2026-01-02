// ============================================================================
// Elevation Data Import for Eustress Terrain
// ============================================================================
//
// Converts elevation data (DEM, GeoTIFF, HGT, ASC) to Eustress Terrain format.
// Supports:
// - Direct heightmap generation for TerrainData
// - Point cloud conversion for visualization
// - Automatic chunking for large datasets
// - Coordinate system transformation (geographic to local)
// - Multi-resolution LOD generation
//
// Table of Contents:
// 1. Import Configuration
// 2. Terrain Generation
// 3. Point Cloud Conversion
// 4. Chunking and Tiling
// 5. Coordinate Transforms
// ============================================================================

use bevy::prelude::*;
use std::path::Path;

use super::formats::{ElevationData, ElevationFormat, import_elevation, import_geotiff, import_asc, import_hgt};
use super::core::PointCloudPoint;
use crate::terrain::{TerrainConfig, TerrainData};

// ============================================================================
// 1. Import Configuration
// ============================================================================

/// Configuration for elevation import
#[derive(Debug, Clone)]
pub struct ElevationImportConfig {
    /// Target terrain chunk size in meters
    pub chunk_size: f32,
    
    /// Target resolution per chunk (vertices per side)
    pub chunk_resolution: u32,
    
    /// Height scale multiplier (1.0 = real-world meters)
    pub height_scale: f32,
    
    /// Vertical exaggeration (1.0 = no exaggeration)
    pub vertical_exaggeration: f32,
    
    /// Offset to apply to heights (useful for sea level adjustment)
    pub height_offset: f32,
    
    /// Whether to generate LOD levels
    pub generate_lods: bool,
    
    /// Number of LOD levels to generate
    pub lod_levels: u32,
    
    /// Whether to fill nodata values with interpolation
    pub fill_nodata: bool,
    
    /// Nodata fill value (if not interpolating)
    pub nodata_fill_value: f32,
    
    /// Whether to smooth the terrain
    pub smooth_terrain: bool,
    
    /// Smoothing iterations
    pub smooth_iterations: u32,
    
    /// Coordinate system (for geographic data)
    pub coord_system: CoordinateSystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinateSystem {
    /// Local coordinates (meters, Y-up)
    Local,
    /// Geographic (lat/lon in degrees)
    Geographic,
    /// UTM projection
    UTM { zone: u8, northern: bool },
}

impl Default for ElevationImportConfig {
    fn default() -> Self {
        Self {
            // High resolution by default
            chunk_size: 64.0,
            chunk_resolution: 256,
            // 1:1 real-world scale (no exaggeration)
            height_scale: 1.0,
            vertical_exaggeration: 1.0,
            height_offset: 0.0,
            // LOD for performance
            generate_lods: true,
            lod_levels: 5,
            // Data handling
            fill_nodata: true,
            nodata_fill_value: 0.0,
            smooth_terrain: false,
            smooth_iterations: 0,
            coord_system: CoordinateSystem::Local,
        }
    }
}

// ============================================================================
// 2. Terrain Generation
// ============================================================================

/// Result of elevation import
#[derive(Debug, Clone)]
pub struct ElevationImportResult {
    /// Generated terrain configuration
    pub config: TerrainConfig,
    
    /// Terrain data with heightmap cache
    pub data: TerrainData,
    
    /// Original elevation bounds (min, max)
    pub elevation_bounds: (f32, f32),
    
    /// World bounds (min_x, min_z, max_x, max_z)
    pub world_bounds: (f32, f32, f32, f32),
    
    /// Number of chunks generated
    pub chunk_count: (u32, u32),
    
    /// Warnings during import
    pub warnings: Vec<String>,
}

/// Import elevation data and convert to Eustress Terrain
pub fn import_elevation_to_terrain(
    path: &Path,
    config: &ElevationImportConfig,
) -> Result<ElevationImportResult, String> {
    // Load elevation data
    let elevation = import_elevation(path)?;
    
    // Convert to terrain
    elevation_to_terrain(&elevation, config)
}

/// Convert ElevationData to Eustress Terrain
pub fn elevation_to_terrain(
    elevation: &ElevationData,
    config: &ElevationImportConfig,
) -> Result<ElevationImportResult, String> {
    let mut warnings = Vec::new();
    
    // Calculate terrain dimensions
    let world_width = elevation.width as f32 * elevation.cell_size;
    let world_depth = elevation.height as f32 * elevation.cell_size;
    
    // Calculate number of chunks needed
    let chunks_x = ((world_width / config.chunk_size).ceil() as u32).max(1);
    let chunks_z = ((world_depth / config.chunk_size).ceil() as u32).max(1);
    
    // Process heights
    let mut heights = elevation.heights.clone();
    
    // Fill nodata values
    if config.fill_nodata {
        fill_nodata_values(&mut heights, elevation.width, elevation.height, elevation.nodata);
    } else {
        // Replace nodata with fill value
        for h in &mut heights {
            if (*h - elevation.nodata).abs() < 0.001 {
                *h = config.nodata_fill_value;
            }
        }
    }
    
    // Apply height transformations
    let (min_h, max_h) = heights.iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), &h| {
            (min.min(h), max.max(h))
        });
    
    for h in &mut heights {
        *h = (*h + config.height_offset) * config.height_scale * config.vertical_exaggeration;
    }
    
    // Smooth terrain if requested
    if config.smooth_terrain {
        for _ in 0..config.smooth_iterations {
            smooth_heightmap(&mut heights, elevation.width, elevation.height);
        }
    }
    
    // Resample to target resolution if needed
    let target_width = chunks_x * config.chunk_resolution;
    let target_height = chunks_z * config.chunk_resolution;
    
    let resampled = if elevation.width as u32 != target_width || elevation.height as u32 != target_height {
        warnings.push(format!(
            "Resampling from {}x{} to {}x{}",
            elevation.width, elevation.height, target_width, target_height
        ));
        resample_heightmap(&heights, elevation.width, elevation.height, target_width as usize, target_height as usize)
    } else {
        heights
    };
    
    // Create terrain config
    let terrain_config = TerrainConfig {
        chunk_size: config.chunk_size,
        chunk_resolution: config.chunk_resolution,
        chunks_x: chunks_x / 2,  // TerrainConfig uses half-extents from center
        chunks_z: chunks_z / 2,
        lod_levels: config.lod_levels,
        lod_distances: generate_lod_distances(config.lod_levels, config.chunk_size),
        view_distance: config.chunk_size * (chunks_x.max(chunks_z) as f32) * 1.5,
        height_scale: 1.0,  // Already applied to heights
        seed: 0,
    };
    
    // Create terrain data
    let terrain_data = TerrainData {
        heightmap: None,  // Will be created as Image asset
        splatmap: None,
        height_cache: resampled,
        cache_width: target_width,
        cache_height: target_height,
    };
    
    Ok(ElevationImportResult {
        config: terrain_config,
        data: terrain_data,
        elevation_bounds: (min_h, max_h),
        world_bounds: (
            elevation.origin_x as f32,
            elevation.origin_y as f32,
            elevation.origin_x as f32 + world_width,
            elevation.origin_y as f32 + world_depth,
        ),
        chunk_count: (chunks_x, chunks_z),
        warnings,
    })
}

/// Generate LOD distance thresholds
fn generate_lod_distances(levels: u32, chunk_size: f32) -> Vec<f32> {
    (0..levels)
        .map(|i| chunk_size * (2.0_f32.powi(i as i32 + 1)))
        .collect()
}

/// Fill nodata values using neighbor interpolation
fn fill_nodata_values(heights: &mut [f32], width: usize, height: usize, nodata: f32) {
    let mut filled = heights.to_vec();
    
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if (heights[idx] - nodata).abs() < 0.001 {
                // Find valid neighbors
                let mut sum = 0.0;
                let mut count = 0;
                
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 { continue; }
                        
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        
                        if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                            let nidx = ny as usize * width + nx as usize;
                            if (heights[nidx] - nodata).abs() > 0.001 {
                                sum += heights[nidx];
                                count += 1;
                            }
                        }
                    }
                }
                
                if count > 0 {
                    filled[idx] = sum / count as f32;
                } else {
                    filled[idx] = 0.0;
                }
            }
        }
    }
    
    heights.copy_from_slice(&filled);
}

/// Smooth heightmap using box blur
fn smooth_heightmap(heights: &mut [f32], width: usize, height: usize) {
    let mut smoothed = heights.to_vec();
    
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let idx = y * width + x;
            
            let mut sum = 0.0;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nidx = (y as i32 + dy) as usize * width + (x as i32 + dx) as usize;
                    sum += heights[nidx];
                }
            }
            
            smoothed[idx] = sum / 9.0;
        }
    }
    
    heights.copy_from_slice(&smoothed);
}

/// Resample heightmap to new resolution using bilinear interpolation
fn resample_heightmap(
    heights: &[f32],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<f32> {
    let mut result = vec![0.0; dst_width * dst_height];
    
    let scale_x = (src_width - 1) as f32 / (dst_width - 1) as f32;
    let scale_y = (src_height - 1) as f32 / (dst_height - 1) as f32;
    
    for y in 0..dst_height {
        for x in 0..dst_width {
            let src_x = x as f32 * scale_x;
            let src_y = y as f32 * scale_y;
            
            let x0 = src_x.floor() as usize;
            let y0 = src_y.floor() as usize;
            let x1 = (x0 + 1).min(src_width - 1);
            let y1 = (y0 + 1).min(src_height - 1);
            
            let fx = src_x.fract();
            let fy = src_y.fract();
            
            let h00 = heights[y0 * src_width + x0];
            let h10 = heights[y0 * src_width + x1];
            let h01 = heights[y1 * src_width + x0];
            let h11 = heights[y1 * src_width + x1];
            
            let h = h00 * (1.0 - fx) * (1.0 - fy)
                  + h10 * fx * (1.0 - fy)
                  + h01 * (1.0 - fx) * fy
                  + h11 * fx * fy;
            
            result[y * dst_width + x] = h;
        }
    }
    
    result
}

// ============================================================================
// 3. Point Cloud Conversion
// ============================================================================

/// Convert elevation data to point cloud for visualization
pub fn elevation_to_point_cloud(
    elevation: &ElevationData,
    config: &ElevationImportConfig,
    max_points: Option<usize>,
) -> Vec<PointCloudPoint> {
    let total_points = elevation.width * elevation.height;
    let step = if let Some(max) = max_points {
        ((total_points as f32 / max as f32).sqrt().ceil() as usize).max(1)
    } else {
        1
    };
    
    let mut points = Vec::new();
    
    for row in (0..elevation.height).step_by(step) {
        for col in (0..elevation.width).step_by(step) {
            let idx = row * elevation.width + col;
            let h = elevation.heights[idx];
            
            // Skip nodata
            if (h - elevation.nodata).abs() < 0.001 { continue; }
            
            let x = elevation.origin_x as f32 + col as f32 * elevation.cell_size;
            let z = elevation.origin_y as f32 + row as f32 * elevation.cell_size;
            let y = (h + config.height_offset) * config.height_scale * config.vertical_exaggeration;
            
            // Color by height
            let color = height_to_terrain_color(h, elevation.min_height(), elevation.max_height());
            
            points.push(PointCloudPoint { x, y, z, color });
        }
    }
    
    points
}

/// Convert normalized height to terrain color
fn height_to_terrain_color(height: f32, min: f32, max: f32) -> u32 {
    let range = max - min;
    if range < 0.001 { return 0xFF808080; }
    
    let normalized = ((height - min) / range).clamp(0.0, 1.0);
    
    let (r, g, b) = if normalized < 0.15 {
        // Water (blue)
        let t = normalized / 0.15;
        (
            (20.0 + t * 30.0) as u8,
            (80.0 + t * 40.0) as u8,
            (150.0 + t * 50.0) as u8,
        )
    } else if normalized < 0.25 {
        // Beach/sand (tan)
        let t = (normalized - 0.15) / 0.1;
        (
            (194.0 + t * 20.0) as u8,
            (178.0 + t * 10.0) as u8,
            (128.0 - t * 30.0) as u8,
        )
    } else if normalized < 0.5 {
        // Grass/forest (green)
        let t = (normalized - 0.25) / 0.25;
        (
            (50.0 + t * 40.0) as u8,
            (150.0 - t * 30.0) as u8,
            (50.0 + t * 20.0) as u8,
        )
    } else if normalized < 0.75 {
        // Mountain/rock (brown/gray)
        let t = (normalized - 0.5) / 0.25;
        (
            (139.0 - t * 20.0) as u8,
            (119.0 - t * 30.0) as u8,
            (101.0 - t * 30.0) as u8,
        )
    } else {
        // Snow (white)
        let t = (normalized - 0.75) / 0.25;
        (
            (180.0 + t * 75.0) as u8,
            (180.0 + t * 75.0) as u8,
            (190.0 + t * 65.0) as u8,
        )
    };
    
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 255
}

// ============================================================================
// 4. Chunking and Tiling
// ============================================================================

/// Chunk of terrain data for streaming
#[derive(Debug, Clone)]
pub struct TerrainChunk {
    /// Chunk coordinates (grid position)
    pub coord: (i32, i32),
    
    /// World position (center of chunk)
    pub world_pos: Vec3,
    
    /// Height data for this chunk
    pub heights: Vec<f32>,
    
    /// Resolution of this chunk
    pub resolution: u32,
    
    /// Size in world units
    pub size: f32,
}

/// Split elevation data into terrain chunks
pub fn split_into_chunks(
    elevation: &ElevationData,
    config: &ElevationImportConfig,
) -> Vec<TerrainChunk> {
    let world_width = elevation.width as f32 * elevation.cell_size;
    let world_depth = elevation.height as f32 * elevation.cell_size;
    
    let chunks_x = ((world_width / config.chunk_size).ceil() as i32).max(1);
    let chunks_z = ((world_depth / config.chunk_size).ceil() as i32).max(1);
    
    let mut chunks = Vec::new();
    
    for cz in 0..chunks_z {
        for cx in 0..chunks_x {
            // Calculate world bounds for this chunk
            let min_x = cx as f32 * config.chunk_size;
            let min_z = cz as f32 * config.chunk_size;
            let max_x = min_x + config.chunk_size;
            let max_z = min_z + config.chunk_size;
            
            // Sample heights for this chunk
            let mut heights = Vec::with_capacity(
                (config.chunk_resolution * config.chunk_resolution) as usize
            );
            
            for z in 0..config.chunk_resolution {
                for x in 0..config.chunk_resolution {
                    let world_x = min_x + (x as f32 / (config.chunk_resolution - 1) as f32) * config.chunk_size;
                    let world_z = min_z + (z as f32 / (config.chunk_resolution - 1) as f32) * config.chunk_size;
                    
                    let h = sample_elevation(elevation, world_x, world_z, config);
                    heights.push(h);
                }
            }
            
            chunks.push(TerrainChunk {
                coord: (cx - chunks_x / 2, cz - chunks_z / 2),
                world_pos: Vec3::new(
                    min_x + config.chunk_size / 2.0 - world_width / 2.0,
                    0.0,
                    min_z + config.chunk_size / 2.0 - world_depth / 2.0,
                ),
                heights,
                resolution: config.chunk_resolution,
                size: config.chunk_size,
            });
        }
    }
    
    chunks
}

/// Sample elevation at world coordinates
fn sample_elevation(
    elevation: &ElevationData,
    world_x: f32,
    world_z: f32,
    config: &ElevationImportConfig,
) -> f32 {
    // Convert world coords to elevation grid coords
    let grid_x = (world_x / elevation.cell_size).clamp(0.0, (elevation.width - 1) as f32);
    let grid_z = (world_z / elevation.cell_size).clamp(0.0, (elevation.height - 1) as f32);
    
    // Bilinear interpolation
    let x0 = grid_x.floor() as usize;
    let z0 = grid_z.floor() as usize;
    let x1 = (x0 + 1).min(elevation.width - 1);
    let z1 = (z0 + 1).min(elevation.height - 1);
    
    let fx = grid_x.fract();
    let fz = grid_z.fract();
    
    let h00 = elevation.heights[z0 * elevation.width + x0];
    let h10 = elevation.heights[z0 * elevation.width + x1];
    let h01 = elevation.heights[z1 * elevation.width + x0];
    let h11 = elevation.heights[z1 * elevation.width + x1];
    
    // Handle nodata
    let get_valid = |h: f32| {
        if (h - elevation.nodata).abs() < 0.001 { 0.0 } else { h }
    };
    
    let h = get_valid(h00) * (1.0 - fx) * (1.0 - fz)
          + get_valid(h10) * fx * (1.0 - fz)
          + get_valid(h01) * (1.0 - fx) * fz
          + get_valid(h11) * fx * fz;
    
    (h + config.height_offset) * config.height_scale * config.vertical_exaggeration
}

// ============================================================================
// 5. Coordinate Transforms
// ============================================================================

/// Transform geographic coordinates (lat/lon) to local meters
pub fn geographic_to_local(lat: f64, lon: f64, origin_lat: f64, origin_lon: f64) -> (f32, f32) {
    // Approximate conversion using equirectangular projection
    // More accurate for small areas
    const EARTH_RADIUS: f64 = 6_371_000.0; // meters
    
    let lat_rad = lat.to_radians();
    let origin_lat_rad = origin_lat.to_radians();
    
    let x = (lon - origin_lon).to_radians() * EARTH_RADIUS * origin_lat_rad.cos();
    let z = (lat - origin_lat).to_radians() * EARTH_RADIUS;
    
    (x as f32, z as f32)
}

/// Transform UTM coordinates to local meters
pub fn utm_to_local(easting: f64, northing: f64, origin_easting: f64, origin_northing: f64) -> (f32, f32) {
    ((easting - origin_easting) as f32, (northing - origin_northing) as f32)
}

// ============================================================================
// High-Level API
// ============================================================================

/// Import DEM file directly to terrain
pub fn import_dem_to_terrain(path: &Path) -> Result<ElevationImportResult, String> {
    import_elevation_to_terrain(path, &ElevationImportConfig::default())
}

/// Import GeoTIFF file directly to terrain
pub fn import_geotiff_to_terrain(path: &Path) -> Result<ElevationImportResult, String> {
    let elevation = import_geotiff(path)?;
    elevation_to_terrain(&elevation, &ElevationImportConfig::default())
}

/// Import any elevation format to terrain with custom config
pub fn import_to_terrain_with_config(
    path: &Path,
    config: ElevationImportConfig,
) -> Result<ElevationImportResult, String> {
    import_elevation_to_terrain(path, &config)
}

/// Quick preview: Import elevation as point cloud (limited points)
pub fn preview_elevation_as_points(path: &Path, max_points: usize) -> Result<Vec<PointCloudPoint>, String> {
    let elevation = import_elevation(path)?;
    Ok(elevation_to_point_cloud(&elevation, &ElevationImportConfig::default(), Some(max_points)))
}
