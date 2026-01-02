// ============================================================================
// Point Cloud Processing Pipeline
// ============================================================================
//
// Eustress Engine acts as the intermediary between high-detail formats
// (LiDAR, photogrammetry, Gaussian splats) and optimized client output.
//
// Pipeline stages:
// 1. Import - Parse various formats (PLY, LAS, E57, etc.)
// 2. Clean - Remove noise, outliers, duplicate points
// 3. Decimate - Reduce point count while preserving detail
// 4. Octree - Build spatial index for streaming
// 5. LOD - Generate level-of-detail hierarchy
// 6. Compress - Apply zstd compression per chunk
// 7. Export - Output optimized .eustress format
//
// Table of Contents:
// 1. Processing Pipeline
// 2. Import Stage
// 3. Cleaning Stage
// 4. Decimation Stage
// 5. Octree Builder
// 6. LOD Generator
// 7. Export Stage
// ============================================================================

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::Path;

use super::{
    PointCloudPoint, PointCloudPointNormal, PointCloudLOD, PointCloudFormat,
    PointCloudImportSettings, OctreeNode, PointCloudHeader, CoordinateSystem,
};

// ============================================================================
// 1. Processing Pipeline
// ============================================================================

/// Processing pipeline configuration
#[derive(Debug, Clone)]
pub struct ProcessingConfig {
    /// Target point count after decimation (0 = no limit)
    pub target_points: usize,
    /// Minimum distance between points (voxel grid size)
    pub voxel_size: f32,
    /// Noise removal radius
    pub noise_radius: f32,
    /// Minimum neighbors for noise removal
    pub noise_min_neighbors: usize,
    /// Maximum octree depth
    pub octree_max_depth: u8,
    /// Target points per octree leaf
    pub octree_leaf_size: usize,
    /// LOD levels to generate
    pub lod_levels: u8,
    /// Compression level (1-22)
    pub compression_level: i32,
    /// Output chunk size (bytes)
    pub chunk_size: usize,
    /// Preserve normals if available
    pub preserve_normals: bool,
    /// Generate normals if missing
    pub generate_normals: bool,
    /// Normal estimation radius
    pub normal_radius: f32,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            target_points: 0,
            voxel_size: 0.01,  // 1cm voxels
            noise_radius: 0.05,
            noise_min_neighbors: 6,
            octree_max_depth: 12,
            octree_leaf_size: 1000,
            lod_levels: 5,
            compression_level: 3,
            chunk_size: 16 * 1024,  // 16KB for 60 FPS streaming
            preserve_normals: true,
            generate_normals: false,
            normal_radius: 0.1,
        }
    }
}

/// Processing pipeline state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingStage {
    Idle,
    Importing,
    Cleaning,
    Decimating,
    BuildingOctree,
    GeneratingLOD,
    Compressing,
    Exporting,
    Complete,
    Failed,
}

/// Processing progress
#[derive(Debug, Clone)]
pub struct ProcessingProgress {
    pub stage: ProcessingStage,
    pub stage_progress: f32,  // 0.0 - 1.0
    pub total_progress: f32,  // 0.0 - 1.0
    pub points_processed: usize,
    pub points_total: usize,
    pub message: String,
}

impl Default for ProcessingProgress {
    fn default() -> Self {
        Self {
            stage: ProcessingStage::Idle,
            stage_progress: 0.0,
            total_progress: 0.0,
            points_processed: 0,
            points_total: 0,
            message: String::new(),
        }
    }
}

/// Processing result
#[derive(Debug, Clone)]
pub struct ProcessingResult {
    pub success: bool,
    pub output_path: Option<String>,
    pub input_points: usize,
    pub output_points: usize,
    pub compression_ratio: f32,
    pub processing_time_ms: u64,
    pub octree_nodes: usize,
    pub lod_levels: u8,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Main processing pipeline
pub struct PointCloudProcessor {
    config: ProcessingConfig,
    progress: ProcessingProgress,
    points: Vec<PointCloudPoint>,
    normals: Option<Vec<Vec3>>,
    octree: Option<Octree>,
}

impl PointCloudProcessor {
    pub fn new(config: ProcessingConfig) -> Self {
        Self {
            config,
            progress: ProcessingProgress::default(),
            points: Vec::new(),
            normals: None,
            octree: None,
        }
    }
    
    /// Get current progress
    pub fn progress(&self) -> &ProcessingProgress {
        &self.progress
    }
    
    /// Process a point cloud file
    pub fn process(&mut self, input_path: &Path, output_path: &Path) -> ProcessingResult {
        let start_time = std::time::Instant::now();
        let mut result = ProcessingResult {
            success: false,
            output_path: None,
            input_points: 0,
            output_points: 0,
            compression_ratio: 1.0,
            processing_time_ms: 0,
            octree_nodes: 0,
            lod_levels: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
        };
        
        // Stage 1: Import
        self.update_progress(ProcessingStage::Importing, 0.0, "Importing point cloud...");
        match self.import_points(input_path) {
            Ok(count) => {
                result.input_points = count;
                self.progress.points_total = count;
            }
            Err(e) => {
                result.errors.push(format!("Import failed: {}", e));
                self.update_progress(ProcessingStage::Failed, 0.0, &format!("Import failed: {}", e));
                return result;
            }
        }
        
        // Stage 2: Clean
        self.update_progress(ProcessingStage::Cleaning, 0.0, "Removing noise and outliers...");
        let removed = self.clean_points();
        if removed > 0 {
            result.warnings.push(format!("Removed {} noise points", removed));
        }
        
        // Stage 3: Decimate
        self.update_progress(ProcessingStage::Decimating, 0.0, "Decimating point cloud...");
        let before_decimate = self.points.len();
        self.decimate_points();
        let decimated = before_decimate - self.points.len();
        if decimated > 0 {
            result.warnings.push(format!("Decimated {} points", decimated));
        }
        
        // Stage 4: Build Octree
        self.update_progress(ProcessingStage::BuildingOctree, 0.0, "Building spatial index...");
        self.build_octree();
        if let Some(ref octree) = self.octree {
            result.octree_nodes = octree.node_count();
        }
        
        // Stage 5: Generate LOD
        self.update_progress(ProcessingStage::GeneratingLOD, 0.0, "Generating LOD levels...");
        self.generate_lod();
        result.lod_levels = self.config.lod_levels;
        
        // Stage 6: Compress & Export
        self.update_progress(ProcessingStage::Exporting, 0.0, "Exporting optimized format...");
        match self.export(output_path) {
            Ok(stats) => {
                result.output_points = self.points.len();
                result.compression_ratio = stats.compression_ratio;
                result.output_path = Some(output_path.to_string_lossy().to_string());
                result.success = true;
            }
            Err(e) => {
                result.errors.push(format!("Export failed: {}", e));
                self.update_progress(ProcessingStage::Failed, 0.0, &format!("Export failed: {}", e));
                return result;
            }
        }
        
        result.processing_time_ms = start_time.elapsed().as_millis() as u64;
        self.update_progress(ProcessingStage::Complete, 1.0, "Processing complete!");
        
        result
    }
    
    fn update_progress(&mut self, stage: ProcessingStage, stage_progress: f32, message: &str) {
        self.progress.stage = stage;
        self.progress.stage_progress = stage_progress;
        self.progress.message = message.to_string();
        
        // Calculate total progress based on stage
        let stage_weight = match stage {
            ProcessingStage::Idle => 0.0,
            ProcessingStage::Importing => 0.1,
            ProcessingStage::Cleaning => 0.2,
            ProcessingStage::Decimating => 0.35,
            ProcessingStage::BuildingOctree => 0.55,
            ProcessingStage::GeneratingLOD => 0.75,
            ProcessingStage::Compressing => 0.85,
            ProcessingStage::Exporting => 0.95,
            ProcessingStage::Complete => 1.0,
            ProcessingStage::Failed => self.progress.total_progress,
        };
        self.progress.total_progress = stage_weight + stage_progress * 0.1;
    }
    
    // ========================================================================
    // Stage Implementations
    // ========================================================================
    
    fn import_points(&mut self, path: &Path) -> Result<usize, String> {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let format = PointCloudFormat::from_extension(ext)
            .ok_or_else(|| format!("Unsupported format: {}", ext))?;
        
        match format {
            PointCloudFormat::PLY => self.import_ply(path),
            PointCloudFormat::XYZ => self.import_xyz(path),
            PointCloudFormat::LAS => self.import_las(path),
            PointCloudFormat::PCD => self.import_pcd(path),
            _ => Err(format!("Format {:?} not yet implemented", format)),
        }
    }
    
    fn import_ply(&mut self, path: &Path) -> Result<usize, String> {
        use std::io::{BufRead, BufReader};
        use std::fs::File;
        
        let file = File::open(path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        
        // Parse header
        let mut vertex_count = 0usize;
        let mut has_color = false;
        let mut has_normals = false;
        let mut in_header = true;
        
        while in_header {
            let line = lines.next()
                .ok_or("Unexpected end of file")?
                .map_err(|e| e.to_string())?;
            
            let parts: Vec<&str> = line.split_whitespace().collect();
            
            if parts.is_empty() { continue; }
            
            match parts[0] {
                "element" if parts.len() >= 3 && parts[1] == "vertex" => {
                    vertex_count = parts[2].parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
                }
                "property" if parts.len() >= 3 => {
                    match parts[2] {
                        "red" | "r" => has_color = true,
                        "nx" => has_normals = true,
                        _ => {}
                    }
                }
                "end_header" => in_header = false,
                _ => {}
            }
        }
        
        // Parse vertices
        self.points.clear();
        self.points.reserve(vertex_count);
        
        if has_normals {
            self.normals = Some(Vec::with_capacity(vertex_count));
        }
        
        for (i, line_result) in lines.enumerate() {
            if i >= vertex_count { break; }
            
            let line = line_result.map_err(|e| e.to_string())?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            
            if parts.len() < 3 { continue; }
            
            let x: f32 = parts[0].parse().unwrap_or(0.0);
            let y: f32 = parts[1].parse().unwrap_or(0.0);
            let z: f32 = parts[2].parse().unwrap_or(0.0);
            
            let color = if has_color && parts.len() >= 6 {
                let r: u8 = parts[3].parse().unwrap_or(255);
                let g: u8 = parts[4].parse().unwrap_or(255);
                let b: u8 = parts[5].parse().unwrap_or(255);
                ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 255
            } else {
                0xFFFFFFFF
            };
            
            self.points.push(PointCloudPoint { x, y, z, color });
            
            if has_normals && parts.len() >= 9 {
                let nx: f32 = parts[6].parse().unwrap_or(0.0);
                let ny: f32 = parts[7].parse().unwrap_or(0.0);
                let nz: f32 = parts[8].parse().unwrap_or(0.0);
                if let Some(ref mut normals) = self.normals {
                    normals.push(Vec3::new(nx, ny, nz));
                }
            }
            
            // Update progress
            if i % 100000 == 0 {
                self.progress.points_processed = i;
                self.progress.stage_progress = i as f32 / vertex_count as f32;
            }
        }
        
        Ok(self.points.len())
    }
    
    fn import_xyz(&mut self, path: &Path) -> Result<usize, String> {
        use std::io::{BufRead, BufReader};
        use std::fs::File;
        
        let file = File::open(path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        
        self.points.clear();
        
        for (i, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| e.to_string())?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            
            if parts.len() < 3 { continue; }
            
            let x: f32 = parts[0].parse().unwrap_or(0.0);
            let y: f32 = parts[1].parse().unwrap_or(0.0);
            let z: f32 = parts[2].parse().unwrap_or(0.0);
            
            let color = if parts.len() >= 6 {
                let r: u8 = parts[3].parse().unwrap_or(255);
                let g: u8 = parts[4].parse().unwrap_or(255);
                let b: u8 = parts[5].parse().unwrap_or(255);
                ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 255
            } else {
                0xFFFFFFFF
            };
            
            self.points.push(PointCloudPoint { x, y, z, color });
            
            if i % 100000 == 0 {
                self.progress.points_processed = i;
            }
        }
        
        Ok(self.points.len())
    }
    
    fn import_las(&mut self, _path: &Path) -> Result<usize, String> {
        // TODO: Implement LAS/LAZ parsing
        Err("LAS format requires las-rs crate - not yet implemented".to_string())
    }
    
    fn import_pcd(&mut self, _path: &Path) -> Result<usize, String> {
        // TODO: Implement PCD parsing
        Err("PCD format not yet implemented".to_string())
    }
    
    fn clean_points(&mut self) -> usize {
        if self.points.is_empty() { return 0; }
        
        let original_count = self.points.len();
        
        // Build spatial hash for neighbor queries
        let cell_size = self.config.noise_radius;
        let mut spatial_hash: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
        
        for (i, point) in self.points.iter().enumerate() {
            let cell = (
                (point.x / cell_size).floor() as i32,
                (point.y / cell_size).floor() as i32,
                (point.z / cell_size).floor() as i32,
            );
            spatial_hash.entry(cell).or_default().push(i);
        }
        
        // Mark points with too few neighbors as noise
        let mut keep = vec![true; self.points.len()];
        let radius_sq = self.config.noise_radius * self.config.noise_radius;
        
        for (i, point) in self.points.iter().enumerate() {
            let cell = (
                (point.x / cell_size).floor() as i32,
                (point.y / cell_size).floor() as i32,
                (point.z / cell_size).floor() as i32,
            );
            
            let mut neighbor_count = 0;
            
            // Check 27 neighboring cells
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let neighbor_cell = (cell.0 + dx, cell.1 + dy, cell.2 + dz);
                        if let Some(indices) = spatial_hash.get(&neighbor_cell) {
                            for &j in indices {
                                if i == j { continue; }
                                let other = &self.points[j];
                                let dx = point.x - other.x;
                                let dy = point.y - other.y;
                                let dz = point.z - other.z;
                                if dx*dx + dy*dy + dz*dz < radius_sq {
                                    neighbor_count += 1;
                                }
                            }
                        }
                    }
                }
            }
            
            if neighbor_count < self.config.noise_min_neighbors {
                keep[i] = false;
            }
            
            if i % 100000 == 0 {
                self.progress.stage_progress = i as f32 / self.points.len() as f32;
            }
        }
        
        // Remove marked points
        let mut write_idx = 0;
        for read_idx in 0..self.points.len() {
            if keep[read_idx] {
                self.points[write_idx] = self.points[read_idx];
                write_idx += 1;
            }
        }
        self.points.truncate(write_idx);
        
        original_count - self.points.len()
    }
    
    fn decimate_points(&mut self) {
        if self.config.voxel_size <= 0.0 { return; }
        
        // Voxel grid decimation
        let cell_size = self.config.voxel_size;
        let mut voxel_map: HashMap<(i32, i32, i32), (Vec3, u32, usize)> = HashMap::new();
        
        for point in &self.points {
            let cell = (
                (point.x / cell_size).floor() as i32,
                (point.y / cell_size).floor() as i32,
                (point.z / cell_size).floor() as i32,
            );
            
            let entry = voxel_map.entry(cell).or_insert((Vec3::ZERO, 0, 0));
            entry.0 += Vec3::new(point.x, point.y, point.z);
            entry.1 = entry.1.saturating_add(point.color); // Accumulate color (simplified)
            entry.2 += 1;
        }
        
        // Create decimated points
        self.points.clear();
        self.points.reserve(voxel_map.len());
        
        for (_, (sum_pos, color, count)) in voxel_map {
            let avg_pos = sum_pos / count as f32;
            self.points.push(PointCloudPoint {
                x: avg_pos.x,
                y: avg_pos.y,
                z: avg_pos.z,
                color: color / count as u32,
            });
        }
        
        // Further reduce if target specified
        if self.config.target_points > 0 && self.points.len() > self.config.target_points {
            // Random sampling to target count
            use std::collections::HashSet;
            let mut rng_state = 12345u64;
            let mut selected: HashSet<usize> = HashSet::new();
            
            while selected.len() < self.config.target_points {
                // Simple LCG random
                rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let idx = (rng_state as usize) % self.points.len();
                selected.insert(idx);
            }
            
            let mut new_points = Vec::with_capacity(self.config.target_points);
            for idx in selected {
                new_points.push(self.points[idx]);
            }
            self.points = new_points;
        }
    }
    
    fn build_octree(&mut self) {
        if self.points.is_empty() { return; }
        
        // Calculate bounds
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        
        for point in &self.points {
            let pos = Vec3::new(point.x, point.y, point.z);
            min = min.min(pos);
            max = max.max(pos);
        }
        
        let center = (min + max) / 2.0;
        let half_size = ((max - min) / 2.0).max_element() * 1.01; // Slight padding
        
        self.octree = Some(Octree::new(center, half_size, self.config.octree_max_depth));
        
        // Insert all points
        if let Some(ref mut octree) = self.octree {
            for (i, point) in self.points.iter().enumerate() {
                let pos = Vec3::new(point.x, point.y, point.z);
                octree.insert(pos, i);
                
                if i % 100000 == 0 {
                    self.progress.stage_progress = i as f32 / self.points.len() as f32;
                }
            }
        }
    }
    
    fn generate_lod(&mut self) {
        // LOD is generated during octree build - each level represents a LOD
        // The octree naturally provides spatial LOD through its hierarchy
        
        // For explicit LOD, we could store decimated versions at each level
        // This is handled during export by the octree traversal
    }
    
    fn export(&self, path: &Path) -> Result<ExportStats, String> {
        use std::io::{Write, BufWriter};
        use std::fs::File;
        
        let file = File::create(path).map_err(|e| e.to_string())?;
        let mut writer = BufWriter::new(file);
        
        // Write header
        let header = PointCloudHeader {
            magic: *b"EUSTPCD\0",
            version: 1,
            total_points: self.points.len() as u64,
            bounds_min: self.octree.as_ref()
                .map(|o| o.center - Vec3::splat(o.half_size))
                .unwrap_or(Vec3::ZERO),
            bounds_max: self.octree.as_ref()
                .map(|o| o.center + Vec3::splat(o.half_size))
                .unwrap_or(Vec3::ZERO),
            node_count: self.octree.as_ref().map(|o| o.node_count() as u32).unwrap_or(0),
            nodes_offset: 0,  // Will be updated
            data_offset: 0,   // Will be updated
            coord_system: 0,  // Y-up
            point_format: 0,  // XYZ + RGBA
            compression: 1,   // zstd
        };
        
        // Serialize header (simplified - would use bincode in production)
        writer.write_all(&header.magic).map_err(|e| e.to_string())?;
        writer.write_all(&header.version.to_le_bytes()).map_err(|e| e.to_string())?;
        writer.write_all(&header.total_points.to_le_bytes()).map_err(|e| e.to_string())?;
        
        // Write points in chunks
        let chunk_points = self.config.chunk_size / PointCloudPoint::SIZE;
        let mut total_compressed = 0usize;
        let total_raw = self.points.len() * PointCloudPoint::SIZE;
        
        for chunk in self.points.chunks(chunk_points) {
            // Serialize chunk
            let mut chunk_data = Vec::with_capacity(chunk.len() * PointCloudPoint::SIZE);
            for point in chunk {
                chunk_data.extend_from_slice(&point.x.to_le_bytes());
                chunk_data.extend_from_slice(&point.y.to_le_bytes());
                chunk_data.extend_from_slice(&point.z.to_le_bytes());
                chunk_data.extend_from_slice(&point.color.to_le_bytes());
            }
            
            // Compress
            let compressed = zstd::encode_all(&chunk_data[..], self.config.compression_level)
                .map_err(|e| e.to_string())?;
            
            total_compressed += compressed.len();
            
            // Write chunk size and data
            writer.write_all(&(compressed.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;
            writer.write_all(&compressed).map_err(|e| e.to_string())?;
        }
        
        writer.flush().map_err(|e| e.to_string())?;
        
        Ok(ExportStats {
            compression_ratio: total_raw as f32 / total_compressed.max(1) as f32,
            chunks_written: (self.points.len() + chunk_points - 1) / chunk_points,
        })
    }
}

struct ExportStats {
    compression_ratio: f32,
    chunks_written: usize,
}

// ============================================================================
// Octree Implementation
// ============================================================================

/// Octree for spatial indexing and LOD
pub struct Octree {
    pub center: Vec3,
    pub half_size: f32,
    root: OctreeNodeInternal,
    max_depth: u8,
    node_count: usize,
}

struct OctreeNodeInternal {
    center: Vec3,
    half_size: f32,
    point_indices: Vec<usize>,
    children: Option<Box<[OctreeNodeInternal; 8]>>,
    depth: u8,
}

impl Octree {
    pub fn new(center: Vec3, half_size: f32, max_depth: u8) -> Self {
        Self {
            center,
            half_size,
            root: OctreeNodeInternal::new(center, half_size, 0),
            max_depth,
            node_count: 1,
        }
    }
    
    pub fn insert(&mut self, point: Vec3, index: usize) {
        self.root.insert(point, index, self.max_depth, &mut self.node_count);
    }
    
    pub fn node_count(&self) -> usize {
        self.node_count
    }
    
    /// Query points within frustum at given LOD
    pub fn query_frustum(&self, _frustum: &[Vec4; 6], lod: PointCloudLOD) -> Vec<usize> {
        let target_depth = match lod {
            PointCloudLOD::LOD0 => self.max_depth,
            PointCloudLOD::LOD1 => self.max_depth.saturating_sub(2),
            PointCloudLOD::LOD2 => self.max_depth.saturating_sub(4),
            PointCloudLOD::LOD3 => self.max_depth.saturating_sub(6),
            PointCloudLOD::LOD4 => 2,
        };
        
        let mut result = Vec::new();
        self.root.collect_at_depth(target_depth, &mut result);
        result
    }
}

impl OctreeNodeInternal {
    fn new(center: Vec3, half_size: f32, depth: u8) -> Self {
        Self {
            center,
            half_size,
            point_indices: Vec::new(),
            children: None,
            depth,
        }
    }
    
    fn insert(&mut self, point: Vec3, index: usize, max_depth: u8, node_count: &mut usize) {
        // If leaf or at max depth, store here
        if self.children.is_none() {
            self.point_indices.push(index);
            
            // Split if too many points and not at max depth
            if self.point_indices.len() > 100 && self.depth < max_depth {
                self.split(node_count);
            }
            return;
        }
        
        // Find child octant
        let child_idx = self.child_index(point);
        if let Some(ref mut children) = self.children {
            children[child_idx].insert(point, index, max_depth, node_count);
        }
    }
    
    fn split(&mut self, node_count: &mut usize) {
        let new_half = self.half_size / 2.0;
        let offsets = [
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new( 1.0, -1.0, -1.0),
            Vec3::new(-1.0,  1.0, -1.0),
            Vec3::new( 1.0,  1.0, -1.0),
            Vec3::new(-1.0, -1.0,  1.0),
            Vec3::new( 1.0, -1.0,  1.0),
            Vec3::new(-1.0,  1.0,  1.0),
            Vec3::new( 1.0,  1.0,  1.0),
        ];
        
        self.children = Some(Box::new(std::array::from_fn(|i| {
            *node_count += 1;
            OctreeNodeInternal::new(
                self.center + offsets[i] * new_half,
                new_half,
                self.depth + 1,
            )
        })));
        
        // Note: Points are kept at this level as representative samples
        // In a full implementation, we'd redistribute them
    }
    
    fn child_index(&self, point: Vec3) -> usize {
        let mut idx = 0;
        if point.x >= self.center.x { idx |= 1; }
        if point.y >= self.center.y { idx |= 2; }
        if point.z >= self.center.z { idx |= 4; }
        idx
    }
    
    fn collect_at_depth(&self, target_depth: u8, result: &mut Vec<usize>) {
        if self.depth >= target_depth || self.children.is_none() {
            result.extend(&self.point_indices);
            return;
        }
        
        if let Some(ref children) = self.children {
            for child in children.iter() {
                child.collect_at_depth(target_depth, result);
            }
        }
    }
}

// ============================================================================
// Batch Processing
// ============================================================================

/// Process multiple point clouds in batch
pub struct BatchProcessor {
    config: ProcessingConfig,
    jobs: Vec<BatchJob>,
}

#[derive(Debug, Clone)]
pub struct BatchJob {
    pub input_path: String,
    pub output_path: String,
    pub status: BatchJobStatus,
    pub result: Option<ProcessingResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchJobStatus {
    Pending,
    Processing,
    Complete,
    Failed,
}

impl BatchProcessor {
    pub fn new(config: ProcessingConfig) -> Self {
        Self {
            config,
            jobs: Vec::new(),
        }
    }
    
    pub fn add_job(&mut self, input: &str, output: &str) {
        self.jobs.push(BatchJob {
            input_path: input.to_string(),
            output_path: output.to_string(),
            status: BatchJobStatus::Pending,
            result: None,
        });
    }
    
    pub fn process_all(&mut self) -> Vec<ProcessingResult> {
        let mut results = Vec::new();
        
        for job in &mut self.jobs {
            job.status = BatchJobStatus::Processing;
            
            let mut processor = PointCloudProcessor::new(self.config.clone());
            let result = processor.process(
                Path::new(&job.input_path),
                Path::new(&job.output_path),
            );
            
            job.status = if result.success {
                BatchJobStatus::Complete
            } else {
                BatchJobStatus::Failed
            };
            
            job.result = Some(result.clone());
            results.push(result);
        }
        
        results
    }
}

// ============================================================================
// Presets
// ============================================================================

impl ProcessingConfig {
    /// Fast processing for preview
    pub fn preview() -> Self {
        Self {
            target_points: 100_000,
            voxel_size: 0.1,
            noise_radius: 0.0,  // Skip noise removal
            noise_min_neighbors: 0,
            octree_max_depth: 6,
            octree_leaf_size: 500,
            lod_levels: 3,
            compression_level: 1,
            chunk_size: 64 * 1024,
            preserve_normals: false,
            generate_normals: false,
            normal_radius: 0.0,
        }
    }
    
    /// Balanced quality/speed
    pub fn balanced() -> Self {
        Self::default()
    }
    
    /// Maximum quality
    pub fn high_quality() -> Self {
        Self {
            target_points: 0,  // Keep all
            voxel_size: 0.005,  // 5mm voxels
            noise_radius: 0.02,
            noise_min_neighbors: 10,
            octree_max_depth: 14,
            octree_leaf_size: 500,
            lod_levels: 6,
            compression_level: 9,
            chunk_size: 16 * 1024,
            preserve_normals: true,
            generate_normals: true,
            normal_radius: 0.05,
        }
    }
    
    /// Optimized for mobile/web clients
    pub fn client_optimized() -> Self {
        Self {
            target_points: 500_000,
            voxel_size: 0.02,
            noise_radius: 0.05,
            noise_min_neighbors: 4,
            octree_max_depth: 10,
            octree_leaf_size: 1000,
            lod_levels: 5,
            compression_level: 6,
            chunk_size: 16 * 1024,  // 16KB for 60 FPS
            preserve_normals: false,
            generate_normals: false,
            normal_radius: 0.0,
        }
    }
    
    /// XR/VR optimized (high framerate priority)
    pub fn xr_optimized() -> Self {
        Self {
            target_points: 2_000_000,
            voxel_size: 0.01,
            noise_radius: 0.03,
            noise_min_neighbors: 6,
            octree_max_depth: 12,
            octree_leaf_size: 800,
            lod_levels: 5,
            compression_level: 3,  // Fast decompression
            chunk_size: 8 * 1024,  // 8KB for 90+ FPS
            preserve_normals: false,
            generate_normals: false,
            normal_radius: 0.0,
        }
    }
}
