//! Advanced Terrain Brushes
//!
//! Provides noise-based stamps, erosion simulation, and other advanced
//! terrain editing tools beyond basic raise/lower/smooth.
//!
//! ## Brush Types
//! - **Noise Stamps**: Apply procedural noise patterns (craters, roads, hills)
//! - **Erosion**: Hydraulic and thermal erosion simulation
//! - **Terrace**: Create stepped terrain
//! - **Cliff**: Create sharp vertical faces
//!
//! ## Usage
//! ```rust,ignore
//! let brush = NoiseBrush::crater(10.0, 5.0);
//! brush.apply(&mut height_cache, hit_point, config);
//! ```

use bevy::prelude::*;
use noise::{NoiseFn, Perlin, Fbm, RidgedMulti, MultiFractal};

// ============================================================================
// Noise Brush Types
// ============================================================================

/// Types of noise patterns for stamps
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NoisePattern {
    /// Smooth hills using Perlin noise
    #[default]
    PerlinHills,
    /// Sharp ridges using ridged multifractal
    Ridges,
    /// Crater/bowl shape
    Crater,
    /// Road/path (linear depression)
    Road,
    /// Plateau (flat top with slopes)
    Plateau,
    /// Random rocks/boulders
    Rocks,
    /// Dunes (wave-like)
    Dunes,
    /// Terraces (stepped)
    Terraces,
}

/// Configuration for noise-based brushes
#[derive(Clone, Debug)]
pub struct NoiseBrush {
    /// Pattern type
    pub pattern: NoisePattern,
    
    /// Brush radius in world units
    pub radius: f32,
    
    /// Height scale (amplitude)
    pub amplitude: f32,
    
    /// Noise frequency (detail level)
    pub frequency: f32,
    
    /// Number of noise octaves
    pub octaves: u32,
    
    /// Edge falloff (0 = hard edge, 1 = smooth)
    pub falloff: f32,
    
    /// Blend mode with existing terrain
    pub blend: BlendMode,
    
    /// Random seed for noise
    pub seed: u32,
    
    /// Rotation angle in radians
    pub rotation: f32,
}

/// How to blend brush effect with existing terrain
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlendMode {
    /// Add to existing height
    #[default]
    Add,
    /// Subtract from existing height
    Subtract,
    /// Replace existing height
    Replace,
    /// Take maximum of brush and existing
    Max,
    /// Take minimum of brush and existing
    Min,
    /// Multiply existing by brush factor
    Multiply,
    /// Blend based on brush strength
    Lerp,
}

impl Default for NoiseBrush {
    fn default() -> Self {
        Self {
            pattern: NoisePattern::PerlinHills,
            radius: 20.0,
            amplitude: 5.0,
            frequency: 0.1,
            octaves: 4,
            falloff: 0.5,
            blend: BlendMode::Add,
            seed: 42,
            rotation: 0.0,
        }
    }
}

impl NoiseBrush {
    /// Create a crater brush
    pub fn crater(radius: f32, depth: f32) -> Self {
        Self {
            pattern: NoisePattern::Crater,
            radius,
            amplitude: depth,
            frequency: 0.05,
            octaves: 2,
            falloff: 0.3,
            blend: BlendMode::Subtract,
            ..default()
        }
    }
    
    /// Create a road brush
    pub fn road(width: f32, depth: f32) -> Self {
        Self {
            pattern: NoisePattern::Road,
            radius: width,
            amplitude: depth,
            frequency: 0.02,
            octaves: 1,
            falloff: 0.8,
            blend: BlendMode::Subtract,
            ..default()
        }
    }
    
    /// Create a plateau brush
    pub fn plateau(radius: f32, height: f32) -> Self {
        Self {
            pattern: NoisePattern::Plateau,
            radius,
            amplitude: height,
            frequency: 0.1,
            octaves: 1,
            falloff: 0.2,
            blend: BlendMode::Max,
            ..default()
        }
    }
    
    /// Create a hills brush
    pub fn hills(radius: f32, height: f32) -> Self {
        Self {
            pattern: NoisePattern::PerlinHills,
            radius,
            amplitude: height,
            frequency: 0.15,
            octaves: 4,
            falloff: 0.5,
            blend: BlendMode::Add,
            ..default()
        }
    }
    
    /// Create a ridges brush
    pub fn ridges(radius: f32, height: f32) -> Self {
        Self {
            pattern: NoisePattern::Ridges,
            radius,
            amplitude: height,
            frequency: 0.08,
            octaves: 5,
            falloff: 0.4,
            blend: BlendMode::Add,
            ..default()
        }
    }
    
    /// Create a dunes brush
    pub fn dunes(radius: f32, height: f32) -> Self {
        Self {
            pattern: NoisePattern::Dunes,
            radius,
            amplitude: height,
            frequency: 0.2,
            octaves: 2,
            falloff: 0.6,
            blend: BlendMode::Add,
            ..default()
        }
    }
    
    /// Sample the brush pattern at a point
    pub fn sample(&self, local_x: f32, local_z: f32) -> f32 {
        let dist = (local_x * local_x + local_z * local_z).sqrt();
        if dist > self.radius {
            return 0.0;
        }
        
        // Apply rotation
        let (sin_r, cos_r) = self.rotation.sin_cos();
        let rx = local_x * cos_r - local_z * sin_r;
        let rz = local_x * sin_r + local_z * cos_r;
        
        // Normalized distance (0 at center, 1 at edge)
        let t = dist / self.radius;
        
        // Calculate falloff
        let falloff = if self.falloff > 0.0 {
            (1.0 - t).powf(1.0 / self.falloff)
        } else {
            if t < 0.99 { 1.0 } else { 0.0 }
        };
        
        // Sample pattern
        let pattern_value = match self.pattern {
            NoisePattern::PerlinHills => self.sample_perlin(rx, rz),
            NoisePattern::Ridges => self.sample_ridges(rx, rz),
            NoisePattern::Crater => self.sample_crater(t),
            NoisePattern::Road => self.sample_road(rx, rz),
            NoisePattern::Plateau => self.sample_plateau(t),
            NoisePattern::Rocks => self.sample_rocks(rx, rz),
            NoisePattern::Dunes => self.sample_dunes(rx, rz),
            NoisePattern::Terraces => self.sample_terraces(rx, rz),
        };
        
        pattern_value * falloff * self.amplitude
    }
    
    fn sample_perlin(&self, x: f32, z: f32) -> f32 {
        let fbm = Fbm::<Perlin>::new(self.seed)
            .set_octaves(self.octaves as usize)
            .set_frequency(self.frequency as f64);
        
        fbm.get([x as f64, z as f64]) as f32 * 0.5 + 0.5
    }
    
    fn sample_ridges(&self, x: f32, z: f32) -> f32 {
        let ridged = RidgedMulti::<Perlin>::new(self.seed)
            .set_octaves(self.octaves as usize)
            .set_frequency(self.frequency as f64);
        
        ridged.get([x as f64, z as f64]) as f32 * 0.5 + 0.5
    }
    
    fn sample_crater(&self, t: f32) -> f32 {
        // Crater profile: rim at edge, depression in center
        if t < 0.7 {
            // Inner depression
            let inner_t = t / 0.7;
            -(1.0 - inner_t * inner_t)
        } else {
            // Outer rim
            let rim_t = (t - 0.7) / 0.3;
            (1.0 - rim_t) * 0.3
        }
    }
    
    fn sample_road(&self, x: f32, _z: f32) -> f32 {
        // Road is a linear depression along Z axis
        let half_width = self.radius * 0.3;
        let dist_from_center = x.abs();
        
        if dist_from_center < half_width {
            -1.0
        } else if dist_from_center < self.radius {
            let t = (dist_from_center - half_width) / (self.radius - half_width);
            -(1.0 - t)
        } else {
            0.0
        }
    }
    
    fn sample_plateau(&self, t: f32) -> f32 {
        // Flat top with steep sides
        if t < 0.6 {
            1.0
        } else {
            let slope_t = (t - 0.6) / 0.4;
            1.0 - slope_t * slope_t
        }
    }
    
    fn sample_rocks(&self, x: f32, z: f32) -> f32 {
        let perlin = Perlin::new(self.seed);
        
        // Multiple scales of noise for rocky appearance
        let large = perlin.get([x as f64 * 0.1, z as f64 * 0.1]) as f32;
        let medium = perlin.get([x as f64 * 0.3, z as f64 * 0.3]) as f32;
        let small = perlin.get([x as f64 * 0.8, z as f64 * 0.8]) as f32;
        
        // Threshold to create distinct rocks
        let combined = large * 0.5 + medium * 0.3 + small * 0.2;
        if combined > 0.2 {
            (combined - 0.2) * 2.0
        } else {
            0.0
        }
    }
    
    fn sample_dunes(&self, x: f32, z: f32) -> f32 {
        // Wave-like pattern
        let wave1 = ((x * self.frequency + z * self.frequency * 0.5) * std::f32::consts::PI).sin();
        let wave2 = ((x * self.frequency * 0.7 - z * self.frequency * 0.3) * std::f32::consts::PI * 1.5).sin();
        
        (wave1 * 0.6 + wave2 * 0.4) * 0.5 + 0.5
    }
    
    fn sample_terraces(&self, x: f32, z: f32) -> f32 {
        let perlin = Perlin::new(self.seed);
        let base = perlin.get([x as f64 * self.frequency as f64, z as f64 * self.frequency as f64]) as f32;
        
        // Quantize to create steps
        let steps = 5.0;
        ((base * 0.5 + 0.5) * steps).floor() / steps
    }
    
    /// Apply brush to height cache
    pub fn apply_to_cache(
        &self,
        height_cache: &mut [f32],
        cache_width: u32,
        cache_height: u32,
        center_x: f32,
        center_z: f32,
        world_size: f32,
        strength: f32,
    ) {
        let pixels_per_unit = cache_width as f32 / world_size;
        let radius_pixels = (self.radius * pixels_per_unit) as i32;
        
        let center_px = ((center_x / world_size + 0.5) * cache_width as f32) as i32;
        let center_pz = ((center_z / world_size + 0.5) * cache_height as f32) as i32;
        
        for pz in (center_pz - radius_pixels).max(0)..=(center_pz + radius_pixels).min(cache_height as i32 - 1) {
            for px in (center_px - radius_pixels).max(0)..=(center_px + radius_pixels).min(cache_width as i32 - 1) {
                let local_x = (px - center_px) as f32 / pixels_per_unit;
                let local_z = (pz - center_pz) as f32 / pixels_per_unit;
                
                let brush_value = self.sample(local_x, local_z) * strength;
                
                if brush_value.abs() > 0.001 {
                    let idx = (pz as u32 * cache_width + px as u32) as usize;
                    if idx < height_cache.len() {
                        let current = height_cache[idx];
                        height_cache[idx] = match self.blend {
                            BlendMode::Add => current + brush_value,
                            BlendMode::Subtract => current - brush_value,
                            BlendMode::Replace => brush_value,
                            BlendMode::Max => current.max(brush_value),
                            BlendMode::Min => current.min(brush_value),
                            BlendMode::Multiply => current * (1.0 + brush_value),
                            BlendMode::Lerp => current * (1.0 - strength) + brush_value,
                        };
                    }
                }
            }
        }
    }
}

// ============================================================================
// Erosion Simulation
// ============================================================================

/// Configuration for erosion simulation
#[derive(Clone, Debug, Resource)]
pub struct ErosionConfig {
    /// Number of erosion iterations
    pub iterations: u32,
    
    /// Hydraulic erosion strength (water-based)
    pub hydraulic_strength: f32,
    
    /// Thermal erosion strength (gravity-based)
    pub thermal_strength: f32,
    
    /// Talus angle for thermal erosion (radians)
    pub talus_angle: f32,
    
    /// Water evaporation rate
    pub evaporation: f32,
    
    /// Sediment capacity factor
    pub sediment_capacity: f32,
    
    /// Sediment deposition rate
    pub deposition_rate: f32,
    
    /// Minimum slope for erosion
    pub min_slope: f32,
    
    /// Rain amount per iteration
    pub rain_amount: f32,
    
    /// Use GPU compute (if available)
    pub use_gpu: bool,
}

impl Default for ErosionConfig {
    fn default() -> Self {
        Self {
            iterations: 50,
            hydraulic_strength: 0.3,
            thermal_strength: 0.2,
            talus_angle: 0.5, // ~30 degrees
            evaporation: 0.02,
            sediment_capacity: 4.0,
            deposition_rate: 0.3,
            min_slope: 0.01,
            rain_amount: 0.01,
            use_gpu: false,
        }
    }
}

/// Erosion simulation state
pub struct ErosionSimulation {
    /// Water height at each cell
    water: Vec<f32>,
    
    /// Sediment amount at each cell
    sediment: Vec<f32>,
    
    /// Water velocity (for visualization)
    velocity: Vec<Vec2>,
    
    /// Grid dimensions
    width: u32,
    height: u32,
}

impl ErosionSimulation {
    /// Create new erosion simulation
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height) as usize;
        Self {
            water: vec![0.0; size],
            sediment: vec![0.0; size],
            velocity: vec![Vec2::ZERO; size],
            width,
            height,
        }
    }
    
    /// Run erosion simulation on height cache
    pub fn simulate(
        &mut self,
        height_cache: &mut [f32],
        config: &ErosionConfig,
    ) {
        for _ in 0..config.iterations {
            self.add_rain(config.rain_amount);
            self.hydraulic_erosion(height_cache, config);
            self.thermal_erosion(height_cache, config);
            self.evaporate(config.evaporation);
        }
    }
    
    /// Add rain to the simulation
    fn add_rain(&mut self, amount: f32) {
        for w in self.water.iter_mut() {
            *w += amount;
        }
    }
    
    /// Simulate hydraulic (water) erosion
    fn hydraulic_erosion(&mut self, heights: &mut [f32], config: &ErosionConfig) {
        let w = self.width as i32;
        let h = self.height as i32;
        
        // Calculate water flow and erosion
        for z in 1..h-1 {
            for x in 1..w-1 {
                let idx = (z * w + x) as usize;
                let current_height = heights[idx] + self.water[idx];
                
                // Find lowest neighbor
                let neighbors = [
                    ((z-1) * w + x) as usize,     // North
                    ((z+1) * w + x) as usize,     // South
                    (z * w + x - 1) as usize,     // West
                    (z * w + x + 1) as usize,     // East
                ];
                
                let mut lowest_idx = idx;
                let mut lowest_height = current_height;
                
                for &n_idx in &neighbors {
                    let n_height = heights[n_idx] + self.water[n_idx];
                    if n_height < lowest_height {
                        lowest_height = n_height;
                        lowest_idx = n_idx;
                    }
                }
                
                // Flow water downhill
                if lowest_idx != idx {
                    let height_diff = current_height - lowest_height;
                    let flow = (self.water[idx] * 0.5).min(height_diff * 0.5);
                    
                    if flow > 0.0 {
                        // Move water
                        self.water[idx] -= flow;
                        self.water[lowest_idx] += flow;
                        
                        // Erode based on slope
                        let slope = height_diff / 1.0; // Assuming unit cell size
                        if slope > config.min_slope {
                            let erosion = slope * config.hydraulic_strength * flow;
                            let capacity = flow * config.sediment_capacity * slope;
                            
                            if self.sediment[idx] < capacity {
                                // Pick up sediment
                                let pickup = erosion.min(heights[idx] * 0.1);
                                heights[idx] -= pickup;
                                self.sediment[idx] += pickup;
                            } else {
                                // Deposit sediment
                                let deposit = (self.sediment[idx] - capacity) * config.deposition_rate;
                                heights[idx] += deposit;
                                self.sediment[idx] -= deposit;
                            }
                        }
                        
                        // Move sediment with water
                        let sediment_flow = self.sediment[idx] * (flow / self.water[idx].max(0.001));
                        self.sediment[idx] -= sediment_flow;
                        self.sediment[lowest_idx] += sediment_flow;
                    }
                }
            }
        }
    }
    
    /// Simulate thermal (gravity) erosion
    fn thermal_erosion(&mut self, heights: &mut [f32], config: &ErosionConfig) {
        let w = self.width as i32;
        let h = self.height as i32;
        let talus = config.talus_angle.tan();
        
        for z in 1..h-1 {
            for x in 1..w-1 {
                let idx = (z * w + x) as usize;
                let current = heights[idx];
                
                // Check all neighbors
                let neighbors = [
                    ((z-1) * w + x) as usize,
                    ((z+1) * w + x) as usize,
                    (z * w + x - 1) as usize,
                    (z * w + x + 1) as usize,
                ];
                
                for &n_idx in &neighbors {
                    let diff = current - heights[n_idx];
                    
                    // If slope exceeds talus angle, move material
                    if diff > talus {
                        let move_amount = (diff - talus) * 0.5 * config.thermal_strength;
                        heights[idx] -= move_amount;
                        heights[n_idx] += move_amount;
                    }
                }
            }
        }
    }
    
    /// Evaporate water
    fn evaporate(&mut self, rate: f32) {
        for w in self.water.iter_mut() {
            *w = (*w - rate).max(0.0);
        }
        
        // Deposit remaining sediment when water evaporates
        for i in 0..self.sediment.len() {
            if self.water[i] < 0.001 && self.sediment[i] > 0.0 {
                // Sediment is deposited (handled in height cache externally)
                self.sediment[i] = 0.0;
            }
        }
    }
    
    /// Get water map for visualization
    pub fn water_map(&self) -> &[f32] {
        &self.water
    }
    
    /// Get velocity map for visualization
    pub fn velocity_map(&self) -> &[Vec2] {
        &self.velocity
    }
}

// ============================================================================
// Terrace Brush
// ============================================================================

/// Create terraced/stepped terrain
pub fn apply_terrace(
    height_cache: &mut [f32],
    step_height: f32,
    smoothing: f32,
) {
    for h in height_cache.iter_mut() {
        // Quantize to steps
        let step = (*h / step_height).round() * step_height;
        
        // Smooth transition
        *h = *h * (1.0 - smoothing) + step * smoothing;
    }
}

/// Create cliff faces at height thresholds
pub fn apply_cliffs(
    height_cache: &mut [f32],
    cache_width: u32,
    threshold: f32,
    cliff_steepness: f32,
) {
    let w = cache_width as i32;
    let h = (height_cache.len() / cache_width as usize) as i32;
    
    let mut deltas = vec![0.0f32; height_cache.len()];
    
    for z in 1..h-1 {
        for x in 1..w-1 {
            let idx = (z * w + x) as usize;
            let current = height_cache[idx];
            
            // Check neighbors for height differences
            let neighbors = [
                height_cache[((z-1) * w + x) as usize],
                height_cache[((z+1) * w + x) as usize],
                height_cache[(z * w + x - 1) as usize],
                height_cache[(z * w + x + 1) as usize],
            ];
            
            for &n in &neighbors {
                let diff = (current - n).abs();
                if diff > threshold {
                    // Steepen the cliff
                    let steepen = (diff - threshold) * cliff_steepness;
                    if current > n {
                        deltas[idx] += steepen * 0.25;
                    } else {
                        deltas[idx] -= steepen * 0.25;
                    }
                }
            }
        }
    }
    
    // Apply deltas
    for (h, d) in height_cache.iter_mut().zip(deltas.iter()) {
        *h += *d;
    }
}

// ============================================================================
// Resource for Brush Selection
// ============================================================================

/// Currently selected advanced brush
#[derive(Resource, Default)]
pub struct AdvancedBrushState {
    /// Current noise brush configuration
    pub noise_brush: Option<NoiseBrush>,
    
    /// Current erosion configuration
    pub erosion_config: ErosionConfig,
    
    /// Whether erosion preview is active
    pub erosion_preview: bool,
    
    /// Selected brush type
    pub selected_type: AdvancedBrushType,
}

/// Types of advanced brushes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AdvancedBrushType {
    #[default]
    None,
    NoiseCrater,
    NoiseHills,
    NoiseRidges,
    NoiseDunes,
    NoiseRoad,
    NoisePlateau,
    NoiseRocks,
    NoiseTerrace,
    Erosion,
    Terrace,
    Cliff,
}

impl AdvancedBrushType {
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::NoiseCrater => "Crater",
            Self::NoiseHills => "Hills",
            Self::NoiseRidges => "Ridges",
            Self::NoiseDunes => "Dunes",
            Self::NoiseRoad => "Road",
            Self::NoisePlateau => "Plateau",
            Self::NoiseRocks => "Rocks",
            Self::NoiseTerrace => "Terrace Noise",
            Self::Erosion => "Erosion",
            Self::Terrace => "Terrace",
            Self::Cliff => "Cliff",
        }
    }
    
    /// Get all brush types
    pub fn all() -> &'static [AdvancedBrushType] {
        &[
            Self::None,
            Self::NoiseCrater,
            Self::NoiseHills,
            Self::NoiseRidges,
            Self::NoiseDunes,
            Self::NoiseRoad,
            Self::NoisePlateau,
            Self::NoiseRocks,
            Self::NoiseTerrace,
            Self::Erosion,
            Self::Terrace,
            Self::Cliff,
        ]
    }
    
    /// Create noise brush for this type
    pub fn create_brush(&self, radius: f32, strength: f32) -> Option<NoiseBrush> {
        match self {
            Self::None => None,
            Self::NoiseCrater => Some(NoiseBrush::crater(radius, strength)),
            Self::NoiseHills => Some(NoiseBrush::hills(radius, strength)),
            Self::NoiseRidges => Some(NoiseBrush::ridges(radius, strength)),
            Self::NoiseDunes => Some(NoiseBrush::dunes(radius, strength)),
            Self::NoiseRoad => Some(NoiseBrush::road(radius, strength)),
            Self::NoisePlateau => Some(NoiseBrush::plateau(radius, strength)),
            Self::NoiseRocks => Some(NoiseBrush {
                pattern: NoisePattern::Rocks,
                radius,
                amplitude: strength,
                ..default()
            }),
            Self::NoiseTerrace => Some(NoiseBrush {
                pattern: NoisePattern::Terraces,
                radius,
                amplitude: strength,
                ..default()
            }),
            Self::Erosion | Self::Terrace | Self::Cliff => None,
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
    fn test_noise_brush_sample() {
        let brush = NoiseBrush::crater(10.0, 5.0);
        
        // Center should be depressed
        let center = brush.sample(0.0, 0.0);
        assert!(center < 0.0, "Crater center should be negative");
        
        // Edge should be near zero
        let edge = brush.sample(10.0, 0.0);
        assert!(edge.abs() < 0.1, "Crater edge should be near zero");
    }
    
    #[test]
    fn test_erosion_simulation() {
        let mut sim = ErosionSimulation::new(32, 32);
        let mut heights = vec![0.5; 32 * 32];
        
        // Create a hill in the center
        for z in 12..20 {
            for x in 12..20 {
                heights[z * 32 + x] = 1.0;
            }
        }
        
        let config = ErosionConfig {
            iterations: 10,
            ..default()
        };
        
        sim.simulate(&mut heights, &config);
        
        // Hill should be somewhat eroded
        let center = heights[16 * 32 + 16];
        assert!(center < 1.0, "Hill center should be eroded");
    }
}
