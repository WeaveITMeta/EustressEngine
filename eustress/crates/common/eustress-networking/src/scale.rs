//! # World Scale System
//!
//! Unit conversions and network quantization for Eustress.
//!
//! ## Unit: The Stud
//!
//! The **stud** is Eustress's base unit, matching Roblox for familiarity:
//! - **1 stud = 0.28 meters** (28 centimeters)
//! - A standard character is ~5.5 studs tall (~1.54m)
//! - A door is ~7 studs tall (~1.96m)
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
//! - Unit conversion constants (stud ↔ meter)
//! - Network quantization utilities
//! - Reference dimensions for modeling

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// Unit Conversion Constants
// ============================================================================

/// Meters per stud (1 stud = 0.28 meters)
pub const STUD_TO_METERS: f32 = 0.28;

/// Studs per meter (1 meter ≈ 3.571 studs)
pub const METERS_TO_STUDS: f32 = 1.0 / STUD_TO_METERS;

// ============================================================================
// Reference Dimensions (for modeling, not gameplay)
// ============================================================================

/// Standard character height in studs (~1.54m real-world)
pub const REF_CHARACTER_HEIGHT: f32 = 5.5;

/// Standard character width in studs
pub const REF_CHARACTER_WIDTH: f32 = 2.0;

/// Standard door height in studs
pub const REF_DOOR_HEIGHT: f32 = 7.0;

/// Standard door width in studs
pub const REF_DOOR_WIDTH: f32 = 4.0;

/// Standard baseplate unit (4x4 studs)
pub const REF_BASEPLATE_UNIT: f32 = 4.0;

/// Standard stair step height in studs
pub const REF_STAIR_HEIGHT: f32 = 1.0;

// ============================================================================
// World Bounds (for network validation)
// ============================================================================

/// Maximum world extent in studs (±32768 studs = ±9.17km)
pub const WORLD_EXTENT: f32 = 32768.0;

/// Minimum world coordinate
pub const WORLD_MIN: f32 = -WORLD_EXTENT;

/// Maximum world coordinate
pub const WORLD_MAX: f32 = WORLD_EXTENT;

/// Maximum entity speed in studs/second (default, can be overridden by Workspace)
/// 500 studs/s = 140 m/s = 504 km/h - reasonable for vehicles
pub const MAX_SPEED: f32 = 500.0;

// ============================================================================
// Network Quantization
// ============================================================================

/// Position quantization step (0.01 studs = 2.8mm precision)
pub const POSITION_QUANTUM: f32 = 0.01;

/// Velocity quantization step (0.1 studs/s)
pub const VELOCITY_QUANTUM: f32 = 0.1;

/// Rotation quantization (1/65536 of a full rotation)
pub const ROTATION_QUANTUM: f32 = std::f32::consts::TAU / 65536.0;

// ============================================================================
// Part Size Constraints
// ============================================================================

/// Standard gravity constant (m/s²)
pub const GRAVITY_CONSTANT: f32 = 9.80665;

/// Minimum part size in studs (gravity / 100 for clean physics calculations)
/// 9.80665 / 100 ≈ 0.0980665 studs
pub const MIN_PART_SIZE: f32 = GRAVITY_CONSTANT / 100.0;

/// Maximum part size in studs (2048 studs = ~573m)
pub const MAX_PART_SIZE: f32 = 2048.0;

// ============================================================================
// Common Object Sizes
// ============================================================================

/// Standard baseplate size (4x4 studs)
pub const BASEPLATE_UNIT: f32 = 4.0;

/// Standard door height in studs
pub const DOOR_HEIGHT: f32 = 7.0;

/// Standard door width in studs
pub const DOOR_WIDTH: f32 = 4.0;

/// Standard stair step height in studs
pub const STAIR_HEIGHT: f32 = 1.0;

/// Standard stair step depth in studs
pub const STAIR_DEPTH: f32 = 2.0;

// ============================================================================
// Stud Type (Newtype for Type Safety)
// ============================================================================

/// A distance measured in studs.
///
/// Use this for type-safe unit handling:
/// ```rust,ignore
/// let height = Stud(5.5);
/// let meters = height.to_meters();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Reflect)]
pub struct Stud(pub f32);

impl Stud {
    /// Create a new Stud value
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Convert to meters
    pub fn to_meters(self) -> f32 {
        self.0 * STUD_TO_METERS
    }

    /// Create from meters
    pub fn from_meters(meters: f32) -> Self {
        Self(meters * METERS_TO_STUDS)
    }

    /// Get the raw stud value
    pub fn value(self) -> f32 {
        self.0
    }

    /// Quantize for network transmission
    pub fn quantized(self) -> i32 {
        (self.0 / POSITION_QUANTUM).round() as i32
    }

    /// Reconstruct from quantized value
    pub fn from_quantized(q: i32) -> Self {
        Self(q as f32 * POSITION_QUANTUM)
    }
}

impl Default for Stud {
    fn default() -> Self {
        Self(0.0)
    }
}

impl std::ops::Add for Stud {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Stud {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl std::ops::Mul<f32> for Stud {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl std::ops::Div<f32> for Stud {
    type Output = Self;
    fn div(self, rhs: f32) -> Self::Output {
        Self(self.0 / rhs)
    }
}

// ============================================================================
// Vec3 Extensions
// ============================================================================

/// Extension trait for Vec3 stud/meter conversions
pub trait Vec3StudExt {
    /// Convert from studs to meters
    fn studs_to_meters(self) -> Self;
    /// Convert from meters to studs
    fn meters_to_studs(self) -> Self;
    /// Quantize position for network transmission
    fn quantize_position(self) -> IVec3;
    /// Reconstruct from quantized position
    fn from_quantized_position(q: IVec3) -> Self;
    /// Check if within world bounds
    fn in_world_bounds(self) -> bool;
    /// Clamp to world bounds
    fn clamp_to_world(self) -> Self;
}

impl Vec3StudExt for Vec3 {
    fn studs_to_meters(self) -> Self {
        self * STUD_TO_METERS
    }

    fn meters_to_studs(self) -> Self {
        self * METERS_TO_STUDS
    }

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
    fn test_stud_conversion() {
        let stud = Stud(10.0);
        assert!((stud.to_meters() - 2.8).abs() < 0.001);

        let from_meters = Stud::from_meters(2.8);
        assert!((from_meters.0 - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_quantization() {
        let stud = Stud(123.456);
        let quantized = stud.quantized();
        let reconstructed = Stud::from_quantized(quantized);
        assert!((stud.0 - reconstructed.0).abs() < POSITION_QUANTUM);
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

    #[test]
    fn test_jump_height() {
        // Verify jump height calculation: h = v²/2g
        let expected = JUMP_IMPULSE * JUMP_IMPULSE / (2.0 * GRAVITY_STUDS);
        assert!((JUMP_HEIGHT - expected).abs() < 0.001);
        // Should be approximately 2.5 studs
        assert!(JUMP_HEIGHT > 2.0 && JUMP_HEIGHT < 3.0);
    }
}

