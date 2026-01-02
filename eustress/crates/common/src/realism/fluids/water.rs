//! # Water Simulation
//!
//! Water-specific simulation features.
//!
//! ## Table of Contents
//!
//! 1. **WaterBody** - Water volume component
//! 2. **Waves** - Wave simulation
//! 3. **Phase Changes** - Freezing, boiling

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::realism::constants;
use crate::realism::laws::thermodynamics;

// ============================================================================
// Water Body Component
// ============================================================================

/// Represents a body of water (ocean, lake, pool)
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct WaterBody {
    /// Water surface level (Y coordinate)
    pub surface_level: f32,
    /// Water temperature (K)
    pub temperature: f32,
    /// Salinity (g/kg, 0 for fresh water)
    pub salinity: f32,
    /// Current flow velocity
    pub current: Vec3,
    /// Wave amplitude
    pub wave_amplitude: f32,
    /// Wave frequency (Hz)
    pub wave_frequency: f32,
    /// Wave direction
    pub wave_direction: Vec3,
    /// Turbidity (0-1, clarity)
    pub turbidity: f32,
}

impl Default for WaterBody {
    fn default() -> Self {
        Self {
            surface_level: 0.0,
            temperature: 293.15, // 20°C
            salinity: 0.0,       // Fresh water
            current: Vec3::ZERO,
            wave_amplitude: 0.1,
            wave_frequency: 0.5,
            wave_direction: Vec3::X,
            turbidity: 0.1,
        }
    }
}

impl WaterBody {
    /// Create ocean water
    pub fn ocean(surface_level: f32) -> Self {
        Self {
            surface_level,
            temperature: 288.15, // 15°C
            salinity: 35.0,      // Typical ocean salinity
            wave_amplitude: 1.0,
            wave_frequency: 0.1,
            ..default()
        }
    }
    
    /// Create lake water
    pub fn lake(surface_level: f32) -> Self {
        Self {
            surface_level,
            temperature: 293.15,
            salinity: 0.0,
            wave_amplitude: 0.05,
            wave_frequency: 0.3,
            ..default()
        }
    }
    
    /// Create pool water
    pub fn pool(surface_level: f32) -> Self {
        Self {
            surface_level,
            temperature: 299.15, // 26°C
            salinity: 0.0,
            wave_amplitude: 0.01,
            wave_frequency: 1.0,
            turbidity: 0.01,
            ..default()
        }
    }
    
    /// Get water density based on temperature and salinity
    pub fn density(&self) -> f32 {
        // UNESCO equation (simplified)
        let t = self.temperature - 273.15; // Celsius
        let s = self.salinity;
        
        // Pure water density
        let rho_w = 999.842594 
            + 6.793952e-2 * t 
            - 9.095290e-3 * t.powi(2)
            + 1.001685e-4 * t.powi(3);
        
        // Salinity correction
        let rho = rho_w + s * (0.824493 - 4.0899e-3 * t);
        
        rho as f32
    }
    
    /// Get water viscosity based on temperature
    pub fn viscosity(&self) -> f32 {
        let t = self.temperature - 273.15;
        // Vogel equation approximation
        0.00179 / (1.0 + 0.0337 * t + 0.000221 * t * t)
    }
    
    /// Get wave height at position and time
    pub fn wave_height(&self, position: Vec3, time: f32) -> f32 {
        let phase = self.wave_direction.dot(position.xz().extend(0.0).xzy()) * 0.1
            + time * self.wave_frequency * std::f32::consts::TAU;
        self.surface_level + self.wave_amplitude * phase.sin()
    }
    
    /// Check if position is underwater
    pub fn is_underwater(&self, position: Vec3, time: f32) -> bool {
        position.y < self.wave_height(position, time)
    }
    
    /// Get depth at position
    pub fn depth_at(&self, position: Vec3, time: f32) -> f32 {
        self.wave_height(position, time) - position.y
    }
    
    /// Get water phase at current temperature
    pub fn phase(&self) -> WaterPhase {
        if self.temperature < 273.15 {
            WaterPhase::Ice
        } else if self.temperature > 373.15 {
            WaterPhase::Steam
        } else {
            WaterPhase::Liquid
        }
    }
}

/// Water phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum WaterPhase {
    Ice,
    Liquid,
    Steam,
}

// ============================================================================
// Wave Simulation
// ============================================================================

/// Gerstner wave parameters
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct GerstnerWave {
    /// Wave direction (normalized XZ)
    pub direction: Vec2,
    /// Wavelength (m)
    pub wavelength: f32,
    /// Amplitude (m)
    pub amplitude: f32,
    /// Steepness (0-1)
    pub steepness: f32,
    /// Phase offset
    pub phase: f32,
}

impl Default for GerstnerWave {
    fn default() -> Self {
        Self {
            direction: Vec2::X,
            wavelength: 10.0,
            amplitude: 0.5,
            steepness: 0.5,
            phase: 0.0,
        }
    }
}

impl GerstnerWave {
    /// Calculate wave number: k = 2π/λ
    pub fn wave_number(&self) -> f32 {
        std::f32::consts::TAU / self.wavelength
    }
    
    /// Calculate angular frequency: ω = √(gk) (deep water)
    pub fn angular_frequency(&self) -> f32 {
        (9.81 * self.wave_number()).sqrt()
    }
    
    /// Calculate phase speed: c = ω/k
    pub fn phase_speed(&self) -> f32 {
        self.angular_frequency() / self.wave_number()
    }
    
    /// Calculate displacement at position and time
    pub fn displacement(&self, position: Vec2, time: f32) -> Vec3 {
        let k = self.wave_number();
        let omega = self.angular_frequency();
        let d = self.direction.normalize();
        
        let phase = k * d.dot(position) - omega * time + self.phase;
        let q = self.steepness / (k * self.amplitude);
        
        Vec3::new(
            q * self.amplitude * d.x * phase.cos(),
            self.amplitude * phase.sin(),
            q * self.amplitude * d.y * phase.cos(),
        )
    }
    
    /// Calculate normal at position and time
    pub fn normal(&self, position: Vec2, time: f32) -> Vec3 {
        let k = self.wave_number();
        let omega = self.angular_frequency();
        let d = self.direction.normalize();
        
        let phase = k * d.dot(position) - omega * time + self.phase;
        let wa = k * self.amplitude;
        
        Vec3::new(
            -d.x * wa * phase.cos(),
            1.0 - self.steepness * wa * phase.sin(),
            -d.y * wa * phase.cos(),
        ).normalize()
    }
}

/// Sum of multiple Gerstner waves
pub fn sum_gerstner_waves(waves: &[GerstnerWave], position: Vec2, time: f32) -> Vec3 {
    waves.iter()
        .map(|w| w.displacement(position, time))
        .sum()
}

/// Combined normal from multiple waves
pub fn sum_gerstner_normals(waves: &[GerstnerWave], position: Vec2, time: f32) -> Vec3 {
    let sum: Vec3 = waves.iter()
        .map(|w| w.normal(position, time))
        .sum();
    sum.normalize()
}

// ============================================================================
// Water Properties
// ============================================================================

/// Speed of sound in water (m/s)
pub fn speed_of_sound_water(temperature: f32, salinity: f32, depth: f32) -> f32 {
    let t = temperature - 273.15;
    let s = salinity;
    let d = depth;
    
    // Mackenzie equation (simplified)
    1449.2 + 4.6 * t - 0.055 * t * t + 0.00029 * t.powi(3)
        + (1.34 - 0.01 * t) * (s - 35.0)
        + 0.016 * d
}

/// Pressure at depth (Pa)
pub fn pressure_at_depth(depth: f32, surface_pressure: f32, density: f32) -> f32 {
    surface_pressure + density * 9.81 * depth
}

/// Light attenuation in water (Beer-Lambert)
pub fn light_attenuation(depth: f32, attenuation_coefficient: f32) -> f32 {
    (-attenuation_coefficient * depth).exp()
}

/// Typical attenuation coefficients (1/m)
pub mod attenuation_coefficients {
    /// Clear ocean water
    pub const CLEAR_OCEAN: f32 = 0.02;
    /// Coastal water
    pub const COASTAL: f32 = 0.1;
    /// Turbid water
    pub const TURBID: f32 = 0.5;
    /// Pool water
    pub const POOL: f32 = 0.01;
}

// ============================================================================
// Phase Transitions
// ============================================================================

/// Heat required to melt ice: Q = m * L_f
pub fn heat_to_melt(mass: f32) -> f32 {
    mass * constants::WATER_LATENT_HEAT_FUSION
}

/// Heat required to vaporize water: Q = m * L_v
pub fn heat_to_vaporize(mass: f32) -> f32 {
    mass * constants::WATER_LATENT_HEAT_VAPORIZATION
}

/// Evaporation rate (simplified): dm/dt ∝ (P_sat - P_vapor) * A
pub fn evaporation_rate(
    surface_area: f32,
    water_temperature: f32,
    air_temperature: f32,
    relative_humidity: f32,
) -> f32 {
    // Saturation vapor pressure (Tetens equation)
    let t_c = water_temperature - 273.15;
    let p_sat = 610.78 * (17.27 * t_c / (t_c + 237.3)).exp();
    
    let t_air_c = air_temperature - 273.15;
    let p_sat_air = 610.78 * (17.27 * t_air_c / (t_air_c + 237.3)).exp();
    let p_vapor = relative_humidity * p_sat_air;
    
    // Evaporation coefficient (simplified)
    let k = 2.5e-8; // kg/(m²·s·Pa)
    
    k * surface_area * (p_sat - p_vapor).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_water_density() {
        let fresh = WaterBody::default();
        let ocean = WaterBody::ocean(0.0);
        
        // Fresh water at 20°C should be ~998 kg/m³
        assert!((fresh.density() - 998.0).abs() < 5.0);
        
        // Ocean water should be denser
        assert!(ocean.density() > fresh.density());
    }
    
    #[test]
    fn test_wave_height() {
        let water = WaterBody {
            surface_level: 10.0,
            wave_amplitude: 1.0,
            wave_frequency: 1.0,
            ..default()
        };
        
        // Wave height should oscillate around surface level
        let h1 = water.wave_height(Vec3::ZERO, 0.0);
        let h2 = water.wave_height(Vec3::ZERO, 0.25);
        
        assert!((h1 - 10.0).abs() <= 1.0);
        assert!((h2 - 10.0).abs() <= 1.0);
    }
    
    #[test]
    fn test_pressure_at_depth() {
        let p = pressure_at_depth(10.0, 101325.0, 1025.0);
        // At 10m depth: P ≈ 101325 + 1025 * 9.81 * 10 ≈ 201900 Pa
        assert!((p - 201900.0).abs() < 1000.0);
    }
}
