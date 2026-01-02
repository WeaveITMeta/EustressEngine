//! Terrain configuration and data structures

use bevy::prelude::*;

/// Configuration for terrain generation and rendering
#[derive(Component, Clone, Reflect, Debug)]
#[reflect(Component)]
pub struct TerrainConfig {
    /// Size of each chunk in world units
    pub chunk_size: f32,
    
    /// Resolution of each chunk (vertices per side)
    pub chunk_resolution: u32,
    
    /// Number of chunks in X direction (from center)
    pub chunks_x: u32,
    
    /// Number of chunks in Z direction (from center)
    pub chunks_z: u32,
    
    /// Number of LOD levels (0 = highest detail)
    pub lod_levels: u32,
    
    /// Distance thresholds for each LOD level
    pub lod_distances: Vec<f32>,
    
    /// Maximum view distance for chunk culling
    pub view_distance: f32,
    
    /// Height scale multiplier
    pub height_scale: f32,
    
    /// Seed for procedural generation
    pub seed: u32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64.0,
            chunk_resolution: 64,
            chunks_x: 4,
            chunks_z: 4,
            lod_levels: 4,
            lod_distances: vec![100.0, 200.0, 400.0, 800.0],
            view_distance: 1000.0,
            height_scale: 50.0,
            seed: 42,
        }
    }
}

impl TerrainConfig {
    /// Create a small terrain for testing
    pub fn small() -> Self {
        Self {
            chunk_size: 32.0,
            chunk_resolution: 32,
            chunks_x: 2,
            chunks_z: 2,
            lod_levels: 4,
            lod_distances: vec![250.0, 500.0, 750.0, 1000.0],
            view_distance: 2500.0,
            height_scale: 20.0,
            ..default()
        }
    }
    
    /// Create a large terrain for production
    pub fn large() -> Self {
        Self {
            chunk_size: 128.0,
            chunk_resolution: 128,
            chunks_x: 8,
            chunks_z: 8,
            lod_levels: 5,
            lod_distances: vec![200.0, 400.0, 800.0, 1600.0, 3200.0],
            view_distance: 4000.0,
            height_scale: 100.0,
            ..default()
        }
    }
    
    /// Create a massive 10km² terrain (3.16km x 3.16km)
    /// 
    /// Total area: ~10,000,000 m² (10 km²)
    /// Chunk layout: 25x25 chunks = 625 chunks total
    /// Each chunk: 128m x 128m = 16,384 m²
    /// Total size: 3,200m x 3,200m = 10,240,000 m²
    pub fn massive_10km() -> Self {
        Self {
            chunk_size: 128.0,           // 128m per chunk
            chunk_resolution: 64,         // 64x64 vertices per chunk (balanced for performance)
            chunks_x: 12,                 // 25 chunks across (12 on each side of center + center)
            chunks_z: 12,                 // 25 chunks deep
            lod_levels: 6,                // 6 LOD levels for massive view distances
            lod_distances: vec![
                200.0,   // LOD 0: Full detail within 200m
                500.0,   // LOD 1: High detail within 500m
                1000.0,  // LOD 2: Medium detail within 1km
                2000.0,  // LOD 3: Low detail within 2km
                4000.0,  // LOD 4: Very low detail within 4km
                8000.0,  // LOD 5: Minimal detail beyond 4km
            ],
            view_distance: 10000.0,       // 10km view distance
            height_scale: 500.0,          // Tall mountains (500m max height)
            seed: 12345,                  // Reproducible seed
        }
    }
    
    /// Create an epic 10km² terrain with extreme detail
    /// 
    /// Higher resolution for more detailed terrain at the cost of performance.
    /// Recommended for high-end systems only.
    pub fn epic_10km() -> Self {
        Self {
            chunk_size: 128.0,           // 128m per chunk
            chunk_resolution: 128,        // 128x128 vertices per chunk (high detail)
            chunks_x: 12,                 // 25 chunks across
            chunks_z: 12,                 // 25 chunks deep
            lod_levels: 6,
            lod_distances: vec![
                300.0,   // LOD 0: Full detail within 300m
                600.0,   // LOD 1
                1200.0,  // LOD 2
                2400.0,  // LOD 3
                4800.0,  // LOD 4
                9600.0,  // LOD 5
            ],
            view_distance: 12000.0,       // 12km view distance
            height_scale: 800.0,          // Very tall mountains (800m max)
            seed: 54321,
        }
    }
    
    /// Calculate total terrain size in meters
    pub fn total_size(&self) -> (f32, f32) {
        let width = (self.chunks_x * 2 + 1) as f32 * self.chunk_size;
        let depth = (self.chunks_z * 2 + 1) as f32 * self.chunk_size;
        (width, depth)
    }
    
    /// Calculate total terrain area in square meters
    pub fn total_area_m2(&self) -> f32 {
        let (w, d) = self.total_size();
        w * d
    }
    
    /// Calculate total terrain area in square kilometers
    pub fn total_area_km2(&self) -> f32 {
        self.total_area_m2() / 1_000_000.0
    }
    
    /// Get total chunk count
    pub fn total_chunks(&self) -> u32 {
        (self.chunks_x * 2 + 1) * (self.chunks_z * 2 + 1)
    }
    
    /// Get LOD level for a given distance
    pub fn lod_for_distance(&self, distance: f32) -> u32 {
        for (i, &threshold) in self.lod_distances.iter().enumerate() {
            if distance < threshold {
                return i as u32;
            }
        }
        self.lod_levels.saturating_sub(1)
    }
    
    /// Get resolution for a given LOD level
    pub fn resolution_for_lod(&self, lod: u32) -> u32 {
        (self.chunk_resolution >> lod).max(4)
    }
}

/// Runtime terrain data (heightmap, splatmap)
#[derive(Component, Clone, Reflect, Default, Debug)]
#[reflect(Component)]
pub struct TerrainData {
    /// Heightmap image handle (grayscale, 16-bit preferred)
    pub heightmap: Option<Handle<Image>>,
    
    /// Splatmap for texture blending (RGBA = 4 layers)
    pub splatmap: Option<Handle<Image>>,
    
    /// Cached height values (populated from heightmap or procedural)
    #[reflect(ignore)]
    pub height_cache: Vec<f32>,
    
    /// Width of height cache
    pub cache_width: u32,
    
    /// Height of height cache
    pub cache_height: u32,
}

impl TerrainData {
    /// Create procedural terrain data (no heightmap)
    pub fn procedural() -> Self {
        Self::default()
    }
    
    /// Create terrain data from heightmap
    pub fn from_heightmap(heightmap: Handle<Image>) -> Self {
        Self {
            heightmap: Some(heightmap),
            ..default()
        }
    }
    
    /// Sample height at normalized world UV coordinates (0-1 across entire terrain)
    /// Uses bilinear interpolation for smooth sampling
    pub fn sample_height(&self, world_u: f32, world_v: f32) -> f32 {
        if self.height_cache.is_empty() || self.cache_width == 0 || self.cache_height == 0 {
            return 0.0;
        }
        
        // Convert to pixel coordinates
        let px = world_u * (self.cache_width - 1) as f32;
        let pz = world_v * (self.cache_height - 1) as f32;
        
        // Integer and fractional parts for bilinear interpolation
        let x0 = px.floor() as usize;
        let z0 = pz.floor() as usize;
        let x1 = (x0 + 1).min(self.cache_width as usize - 1);
        let z1 = (z0 + 1).min(self.cache_height as usize - 1);
        let fx = px - px.floor();
        let fz = pz - pz.floor();
        
        // Sample four corners
        let stride = self.cache_width as usize;
        let h00 = self.height_cache.get(z0 * stride + x0).copied().unwrap_or(0.0);
        let h10 = self.height_cache.get(z0 * stride + x1).copied().unwrap_or(0.0);
        let h01 = self.height_cache.get(z1 * stride + x0).copied().unwrap_or(0.0);
        let h11 = self.height_cache.get(z1 * stride + x1).copied().unwrap_or(0.0);
        
        // Bilinear interpolation
        let h0 = h00 + (h10 - h00) * fx;
        let h1 = h01 + (h11 - h01) * fx;
        h0 + (h1 - h0) * fz
    }
    
    /// Initialize/resize height cache for world size
    pub fn resize_cache(&mut self, config: &TerrainConfig) {
        let world_width = (config.chunks_x * 2 + 1) * config.chunk_resolution;
        let world_height = (config.chunks_z * 2 + 1) * config.chunk_resolution;
        
        self.cache_width = world_width;
        self.cache_height = world_height;
        self.height_cache.resize((world_width * world_height) as usize, 0.0);
    }
    
    /// Set height at world UV coordinates (for editing)
    pub fn set_height(&mut self, world_u: f32, world_v: f32, height: f32) {
        if self.height_cache.is_empty() || self.cache_width == 0 || self.cache_height == 0 {
            return;
        }
        
        let x = (world_u * (self.cache_width - 1) as f32).round() as usize;
        let z = (world_v * (self.cache_height - 1) as f32).round() as usize;
        let idx = z * self.cache_width as usize + x;
        
        if idx < self.height_cache.len() {
            self.height_cache[idx] = height;
        }
    }
}
