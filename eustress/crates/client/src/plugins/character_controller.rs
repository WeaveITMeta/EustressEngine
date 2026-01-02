//! # Advanced Character Controller
//! 
//! AAA-quality character movement inspired by Uncharted 4 and GTA V.
//! 
//! ## Features
//! 
//! - Physics-based movement with Avian3D
//! - Procedural animation blending
//! - Foot IK for ground adaptation
//! - Smooth camera following
//! - State machine for animation transitions
//! 
//! ## Architecture
//! 
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Character Controller                         │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Input → Locomotion → Physics → Animation → Rendering           │
//! │                                                                 │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐         │
//! │  │  Input   │→ │ Movement │→ │ Avian3D  │→ │ Animator │         │
//! │  │ Handler  │  │ Intent   │  │ Physics  │  │ Blending │         │
//! │  └──────────┘  └──────────┘  └──────────┘  └──────────┘         │
//! │       ↓                           ↓              ↓              │
//! │  ┌──────────┐              ┌──────────┐  ┌──────────┐           │
//! │  │  Camera  │              │ Ground   │  │ Foot IK  │           │
//! │  │ Control  │              │ Check    │  │ Adjust   │           │
//! │  └──────────┘              └──────────┘  └──────────┘           │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

use bevy::prelude::*;
#[allow(unused_imports)]
use avian3d::prelude::*;
#[allow(unused_imports)]
use eustress_common::services::{
    Character, CharacterRoot, CharacterHead, PlayerCamera, PlayerService,
    AnimationStateMachine, AnimationState, LocomotionController,
    ProceduralAnimation, FootIK, CharacterAnimationBundle,
};

// ============================================================================
// Re-export shared types from eustress_common
// ============================================================================

// Re-export character physics and movement types from common
pub use eustress_common::plugins::character_plugin::{
    CharacterPhysics, MovementIntent, CharacterFacing,
};

// Re-export humanoid skeleton types from common
pub use eustress_common::plugins::humanoid::{
    CharacterBody, CharacterLimb, HumanoidConfig,
    spawn_humanoid_character, create_beveled_box,
    apply_procedural_limb_animation,
    update_character_facing_system,
    update_head_look_system,
    angle_difference,
};

// ============================================================================
// Movement Systems (kept for reference, client uses player_plugin.rs systems)
// ============================================================================

/// Process movement input and update intent
#[allow(dead_code)]
pub fn process_movement_input(
    keys: Res<ButtonInput<KeyCode>>,
    player_service: Res<PlayerService>,
    camera_query: Query<&PlayerCamera>,
    mut intent_query: Query<&mut MovementIntent, With<CharacterRoot>>,
) {
    if !player_service.cursor_locked {
        // Clear intent when not controlling
        for mut intent in intent_query.iter_mut() {
            *intent = MovementIntent::default();
        }
        return;
    }
    
    let Ok(camera) = camera_query.single() else { return };
    let Ok(mut intent) = intent_query.single_mut() else { return };
    
    // Get input direction
    let mut input = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { input.z -= 1.0; }
    if keys.pressed(KeyCode::KeyS) { input.z += 1.0; }
    if keys.pressed(KeyCode::KeyA) { input.x -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { input.x += 1.0; }
    
    // Normalize input
    if input.length_squared() > 0.0 {
        input = input.normalize();
    }
    
    // Transform to world space based on camera yaw
    let forward = Vec3::new(-camera.yaw.sin(), 0.0, -camera.yaw.cos());
    let right = Vec3::new(camera.yaw.cos(), 0.0, -camera.yaw.sin());
    intent.direction = forward * -input.z + right * input.x;
    
    // Speed and modifiers
    intent.sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    intent.crouch = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    intent.jump = keys.just_pressed(KeyCode::Space);
    
    // Calculate speed (0.0 = idle, 0.5 = walk, 1.0 = run, 1.5 = sprint)
    if input.length_squared() > 0.0 {
        intent.speed = if intent.sprint { 1.5 } else { 1.0 };
    } else {
        intent.speed = 0.0;
    }
}

/// Apply movement physics
#[allow(dead_code)]
pub fn apply_character_movement(
    time: Res<Time>,
    mut query: Query<(
        &MovementIntent,
        &Character,
        &CharacterPhysics,
        &mut LinearVelocity,
        &mut LocomotionController,
    ), With<CharacterRoot>>,
) {
    let delta = time.delta_secs();
    
    for (intent, character, physics, mut velocity, mut locomotion) in query.iter_mut() {
        let grounded = locomotion.grounded;
        
        // Calculate target velocity
        let mut target_speed = character.walk_speed;
        if intent.sprint {
            target_speed *= character.sprint_multiplier;
        }
        
        let target_velocity = intent.direction * target_speed * intent.speed;
        
        // Apply movement with different handling for ground/air
        if grounded {
            // Ground movement - responsive and snappy
            let acceleration = physics.ground_friction * delta;
            velocity.x = velocity.x.lerp(target_velocity.x, acceleration.min(1.0));
            velocity.z = velocity.z.lerp(target_velocity.z, acceleration.min(1.0));
            
            // Apply friction when no input
            if intent.speed < 0.1 {
                velocity.x *= 1.0 - (physics.ground_friction * delta).min(1.0);
                velocity.z *= 1.0 - (physics.ground_friction * delta).min(1.0);
            }
        } else {
            // Air movement - limited control
            let air_accel = physics.air_control * delta * 10.0;
            velocity.x += (target_velocity.x - velocity.x) * air_accel;
            velocity.z += (target_velocity.z - velocity.z) * air_accel;
            
            // Air drag
            velocity.x *= 1.0 - physics.air_drag * delta;
            velocity.z *= 1.0 - physics.air_drag * delta;
        }
        
        // Update locomotion controller
        let forward = if intent.direction.length_squared() > 0.01 {
            intent.direction.normalize()
        } else {
            Vec3::NEG_Z
        };
        locomotion.update_from_velocity(velocity.0, forward, grounded, delta);
    }
}

/// Handle jumping
#[allow(dead_code)]
pub fn handle_jumping(
    mut query: Query<(
        &MovementIntent,
        &Character,
        &LocomotionController,
        &mut LinearVelocity,
        &mut AnimationStateMachine,
    ), With<CharacterRoot>>,
) {
    for (intent, character, locomotion, mut velocity, mut state_machine) in query.iter_mut() {
        if intent.jump && locomotion.grounded && character.can_jump {
            // Apply jump impulse
            velocity.y = character.jump_power;
            
            // Trigger jump animation
            state_machine.request_transition(AnimationState::JumpStart);
        }
    }
}

/// Ground detection using raycasts
#[allow(dead_code)]
pub fn ground_check(
    spatial_query: SpatialQuery,
    mut query: Query<(
        &Transform,
        &CharacterPhysics,
        &mut LocomotionController,
        &mut Character,
    ), With<CharacterRoot>>,
) {
    for (transform, physics, mut locomotion, mut character) in query.iter_mut() {
        let ray_origin = transform.translation + Vec3::Y * physics.ground_ray_offset;
        let ray_dir = Dir3::NEG_Y;
        let max_dist = physics.ground_ray_length + physics.ground_ray_offset;
        
        // Cast ray to detect ground
        let hit = spatial_query.cast_ray(
            ray_origin,
            ray_dir,
            max_dist,
            true,
            &SpatialQueryFilter::default(),
        );
        
        locomotion.grounded = hit.is_some();
        character.grounded = hit.is_some();
    }
}

/// Update animation state machine based on locomotion
#[allow(dead_code)]
pub fn update_animation_state(
    time: Res<Time>,
    mut query: Query<(
        &LocomotionController,
        &mut AnimationStateMachine,
    ), With<CharacterRoot>>,
) {
    let delta = time.delta_secs();
    
    for (locomotion, mut state_machine) in query.iter_mut() {
        // Update state machine timing
        state_machine.update(delta);
        
        // Determine target state from locomotion
        let target_state = locomotion.get_animation_state();
        
        // Request transition if different
        if state_machine.current_state != target_state {
            // Don't interrupt jump sequence
            let in_jump = matches!(
                state_machine.current_state,
                AnimationState::JumpStart | AnimationState::JumpAir | AnimationState::JumpLand
            );
            
            if !in_jump || locomotion.grounded {
                state_machine.request_transition(target_state);
            }
        }
    }
}

// ============================================================================
// Procedural Animation Systems
// ============================================================================

// NOTE: Procedural animation is now handled in player_plugin.rs with the full skeletal hierarchy
// These functions have been removed as they used the old simple rig

// ============================================================================
// Plugin
// ============================================================================

/// Advanced character controller plugin
/// 
/// NOTE: This plugin is not currently used - the player_plugin.rs handles
/// character spawning and animation with the full skeletal hierarchy.
/// This plugin is kept for reference and potential future use.
#[allow(dead_code)]
pub struct CharacterControllerPlugin;

impl Plugin for CharacterControllerPlugin {
    fn build(&self, app: &mut App) {
        app
            // Register types (all re-exported from eustress_common)
            .register_type::<CharacterPhysics>()
            .register_type::<MovementIntent>()
            .register_type::<CharacterBody>()
            .register_type::<CharacterLimb>()
            .register_type::<CharacterFacing>();
            
            // NOTE: Systems are handled by player_plugin.rs
    }
}
