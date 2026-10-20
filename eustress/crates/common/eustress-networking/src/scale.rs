//! # World Scale System
//!
//! World bounds and network quantization for Eustress.
//!
//! ## Units
//!
//! Positions and velocities travel in the engine's world unit, the meter
//! (`eustress_common::units::ENGINE_NATIVE_UNIT`), so every length here is in
//! meters. The stud is an authoring and display unit, defined once as
//! `eustress_common::units::Unit::Stud`.
//!
//! ## Service-Driven Configuration
//!
//! **Game constants are NOT defined here.** They come from services:
//! - `Workspace.gravity` - Physics gravity vector
//! - `Workspace.max_entity_speed` - Anti-exploit speed limit
//! - `Humanoid.walk_speed` - Character walk speed
//! - `Humanoid.run_speed` - Character run speed
//! - `Humanoid.jump_power` - Character jump impulse
//!
//! This module only provides:
//! - World bounds for network validation
//! - Network quantization utilities

use bevy::prelude::*;

// ============================================================================
// World Bounds (for network validation)
// ============================================================================

/// Maximum world extent in meters (±32768 m = ±32.8 km)
pub const WORLD_EXTENT: f32 = 32768.0;

/// Minimum world coordinate
pub const WORLD_MIN: f32 = -WORLD_EXTENT;

/// Maximum world coordinate
pub const WORLD_MAX: f32 = WORLD_EXTENT;

/// Maximum entity speed in m/s (default, can be overridden by Workspace)
/// 500 m/s = 1800 km/h
pub const MAX_SPEED: f32 = 500.0;

// ============================================================================
// Network Quantization
// ============================================================================

/// Position quantization step (0.01 m = 1 cm precision)
pub const POSITION_QUANTUM: f32 = 0.01;

/// Velocity quantization step (0.1 m/s)
pub const VELOCITY_QUANTUM: f32 = 0.1;

/// Rotation quantization (1/65536 of a full rotation)
pub const ROTATION_QUANTUM: f32 = std::f32::consts::TAU / 65536.0;

// ============================================================================
// Vec3 Extensions
// ============================================================================

/// Extension trait for Vec3 network quantization and world bounds
pub trait Vec3NetExt {
    /// Quantize position for network transmission
    fn quantize_position(self) -> IVec3;
    /// Reconstruct from quantized position
    fn from_quantized_position(q: IVec3) -> Self;
    /// Check if within world bounds
    fn in_world_bounds(self) -> bool;
    /// Clamp to world bounds
    fn clamp_to_world(self) -> Self;
}

impl Vec3NetExt for Vec3 {
    fn quantize_position(self) -> IVec3 {
        IVec3::new(
            (self.x / POSITION_QUANTUM).round() as i32,
            (self.y / POSITION_QUANTUM).round() as i32,
            (self.z / POSITION_QUANTUM).round() as i32,
        )
    }

    fn from_quantized_position(q: IVec3) -> Self {
        Vec3::new(
            q.x as f32 * POSITION_QUANTUM,
            q.y as f32 * POSITION_QUANTUM,
            q.z as f32 * POSITION_QUANTUM,
        )
    }

    fn in_world_bounds(self) -> bool {
        self.x >= -WORLD_EXTENT && self.x <= WORLD_EXTENT &&
        self.y >= -WORLD_EXTENT && self.y <= WORLD_EXTENT &&
        self.z >= -WORLD_EXTENT && self.z <= WORLD_EXTENT
    }

    fn clamp_to_world(self) -> Self {
        self.clamp(Vec3::splat(WORLD_MIN), Vec3::splat(WORLD_MAX))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantization() {
        let position = Vec3::new(123.456, -7.891, 0.004);
        let reconstructed = Vec3::from_quantized_position(position.quantize_position());
        assert!((position - reconstructed).abs().max_element() <= POSITION_QUANTUM / 2.0 + 1e-4);
    }

    #[test]
    fn test_vec3_bounds() {
        let inside = Vec3::new(100.0, 200.0, 300.0);
        assert!(inside.in_world_bounds());

        let outside = Vec3::new(50000.0, 0.0, 0.0);
        assert!(!outside.in_world_bounds());

        let clamped = outside.clamp_to_world();
        assert!(clamped.in_world_bounds());
    }
}

