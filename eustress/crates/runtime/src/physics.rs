//! # Physics Integration
//!
//! Integrates with Avian physics, reading configuration from `Workspace` service.
//! Handles gravity application and anti-exploit validation.
//!
//! ## Service-Driven Configuration
//!
//! All physics bounds come from `Workspace`:
//! - `gravity` -> Applied to all dynamic RigidBodies
//! - `max_entity_speed` -> Velocity clamping
//! - `max_acceleration` -> Acceleration validation
//! - `teleport_threshold` -> Large movement detection

use bevy::prelude::*;
use avian3d::prelude::*;
use eustress_common::services::workspace::Workspace;

// ============================================================================
// Physics Plugin
// ============================================================================

/// Plugin for runtime physics integration.
pub struct RuntimePhysicsPlugin;

impl Plugin for RuntimePhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            apply_workspace_gravity,
            validate_velocities,
            clamp_to_world_bounds,
        ));
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Apply gravity from Workspace to Avian's Gravity resource.
///
/// This allows per-scene gravity configuration.
pub fn apply_workspace_gravity(
    workspace: Option<Res<Workspace>>,
    mut gravity: ResMut<Gravity>,
) {
    if let Some(ws) = workspace {
        // Only update if changed
        if gravity.0 != ws.gravity {
            gravity.0 = ws.gravity;
            info!("Gravity updated to {:?} studs/s²", ws.gravity);
        }
    }
}

/// Validate velocities against Workspace limits.
///
/// Clamps velocities exceeding `max_entity_speed`.
/// Logs warnings for potential exploits.
pub fn validate_velocities(
    workspace: Option<Res<Workspace>>,
    mut query: Query<(Entity, &mut LinearVelocity), Changed<LinearVelocity>>,
) {
    let Some(ws) = workspace else { return };
    
    for (entity, mut velocity) in query.iter_mut() {
        let speed = velocity.0.length();
        
        if speed > ws.max_entity_speed {
            // Clamp to max speed
            velocity.0 = velocity.0.normalize_or_zero() * ws.max_entity_speed;
            
            warn!(
                "Entity {:?} exceeded max speed ({:.1} > {:.1}), clamped",
                entity, speed, ws.max_entity_speed
            );
        }
    }
}

/// Clamp entities to world bounds.
///
/// Entities outside bounds are teleported back to the edge.
pub fn clamp_to_world_bounds(
    workspace: Option<Res<Workspace>>,
    mut query: Query<(Entity, &mut Transform), Changed<Transform>>,
) {
    let Some(ws) = workspace else { return };
    
    for (entity, mut transform) in query.iter_mut() {
        let pos = transform.translation;
        
        // Check if outside bounds
        if !ws.is_in_bounds(pos) {
            // Clamp to bounds
            transform.translation = pos.clamp(ws.world_bounds_min, ws.world_bounds_max);
            
            warn!(
                "Entity {:?} outside world bounds, clamped from {:?} to {:?}",
                entity, pos, transform.translation
            );
        }
        
        // Check for fall death
        if pos.y < ws.fall_height {
            // Entity fell out of world - could emit an event here
            warn!("Entity {:?} fell below fall height ({:.1})", entity, ws.fall_height);
        }
    }
}

// ============================================================================
// Validation Functions
// ============================================================================

/// Validate a velocity change for anti-exploit.
///
/// Returns `true` if the change is valid, `false` if suspicious.
pub fn validate_velocity_change(
    workspace: &Workspace,
    old_velocity: Vec3,
    new_velocity: Vec3,
    dt: f32,
) -> bool {
    // Check speed limit
    if new_velocity.length() > workspace.max_entity_speed {
        return false;
    }
    
    // Check acceleration limit
    let acceleration = (new_velocity - old_velocity) / dt;
    if acceleration.length() > workspace.max_acceleration {
        return false;
    }
    
    true
}

/// Validate a position change for anti-exploit.
///
/// Returns `true` if the change is valid, `false` if suspicious (teleport).
pub fn validate_position_change(
    workspace: &Workspace,
    old_position: Vec3,
    new_position: Vec3,
) -> bool {
    let delta = new_position - old_position;
    
    // Check for teleport
    if workspace.is_teleport(delta) {
        return false;
    }
    
    // Check bounds
    if !workspace.is_in_bounds(new_position) {
        return false;
    }
    
    true
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Calculate jump height from jump power and gravity.
///
/// Uses kinematic equation: h = v² / (2g)
pub fn calculate_jump_height(jump_power: f32, gravity: f32) -> f32 {
    if gravity.abs() < 0.001 {
        return f32::INFINITY; // No gravity = infinite jump
    }
    
    (jump_power * jump_power) / (2.0 * gravity.abs())
}

/// Calculate time to reach peak of jump.
///
/// Uses kinematic equation: t = v / g
pub fn calculate_jump_time(jump_power: f32, gravity: f32) -> f32 {
    if gravity.abs() < 0.001 {
        return f32::INFINITY;
    }
    
    jump_power / gravity.abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_jump_height() {
        // Default: jump_power=50, gravity=35
        // Expected height ≈ 35.7 studs
        let height = calculate_jump_height(50.0, 35.0);
        assert!((height - 35.7).abs() < 0.1);
    }
    
    #[test]
    fn test_velocity_validation() {
        let workspace = Workspace::default();
        
        // Valid change
        assert!(validate_velocity_change(
            &workspace,
            Vec3::ZERO,
            Vec3::new(10.0, 0.0, 0.0),
            0.1,
        ));
        
        // Exceeds max speed
        assert!(!validate_velocity_change(
            &workspace,
            Vec3::ZERO,
            Vec3::new(200.0, 0.0, 0.0),
            0.1,
        ));
    }
}
