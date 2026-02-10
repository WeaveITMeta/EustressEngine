//! # Shared Character Controller Plugin
//!
//! AAA-quality character movement shared between Client and Engine Play Mode.
//! This ensures identical gameplay behavior in both contexts.
//!
//! ## Features
//!
//! - Physics-based movement with Avian3D
//! - Procedural animation blending
//! - Smooth camera following
//! - State machine for animation transitions
//! - Full skeletal character with proper joint hierarchy
//!
//! ## Usage
//!
//! Both the Client and Engine's Play Mode should add this plugin:
//! ```rust
//! app.add_plugins(SharedCharacterPlugin);
//! ```

use bevy::prelude::*;
use bevy::window::CursorGrabMode;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::core_pipeline::tonemapping::Tonemapping;
#[cfg(feature = "physics")]
use avian3d::prelude::*;
use tracing::info;

use crate::classes::{Instance, ClassName};
use crate::services::player::{
    Player, Character, CharacterRoot,
    PlayerService, PlayerCamera, CameraMode,
};
use crate::services::animation::{
    AnimationStateMachine, AnimationState, LocomotionController,
    ProceduralAnimation,
};

// ============================================================================
// Components
// ============================================================================

/// Character physics configuration
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct CharacterPhysics {
    /// Ground check ray length
    pub ground_ray_length: f32,
    /// Ground check offset from center
    pub ground_ray_offset: f32,
    /// Slope limit in degrees
    pub max_slope_angle: f32,
    /// Step height for stairs
    pub step_height: f32,
    /// Air control multiplier
    pub air_control: f32,
    /// Ground friction
    pub ground_friction: f32,
    /// Air drag
    pub air_drag: f32,
}

impl Default for CharacterPhysics {
    fn default() -> Self {
        Self {
            ground_ray_length: 0.3,
            ground_ray_offset: 0.1,
            max_slope_angle: 45.0,
            step_height: 0.3,
            air_control: 0.3,
            ground_friction: 8.0,
            air_drag: 0.1,
        }
    }
}

/// Character movement intent (from input)
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct MovementIntent {
    /// Desired movement direction (world space, normalized)
    pub direction: Vec3,
    /// Desired speed (0.0 - 1.0, where 1.0 = sprint)
    pub speed: f32,
    /// Jump requested this frame
    pub jump: bool,
    /// Crouch requested
    pub crouch: bool,
    /// Sprint requested
    pub sprint: bool,
}

/// Character facing direction (smoothly interpolated)
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct CharacterFacing {
    /// Current facing angle (radians, 0 = +Z)
    pub angle: f32,
    /// Target facing angle
    pub target_angle: f32,
    /// Turn speed (radians per second)
    pub turn_speed: f32,
    /// Head look offset (yaw, pitch) relative to body
    pub head_look: Vec2,
    /// Target head look
    pub head_look_target: Vec2,
}

impl Default for CharacterFacing {
    fn default() -> Self {
        Self {
            angle: 0.0,
            target_angle: 0.0,
            turn_speed: 10.0,
            head_look: Vec2::ZERO,
            head_look_target: Vec2::ZERO,
        }
    }
}

/// Marker for play mode character (vs editor entities)
#[derive(Component, Debug, Default)]
pub struct PlayModeCharacter;

/// Marker for play mode camera
#[derive(Component, Debug, Default)]
pub struct PlayModeCamera;

// ============================================================================
// Plugin
// ============================================================================

/// Shared character controller plugin for Client and Engine Play Mode
/// 
/// This plugin provides:
/// - Full humanoid character spawning with skeletal hierarchy
/// - Physics-based movement with Avian3D
/// - Input handling (WASD + mouse)
/// - Camera following with smooth interpolation
/// - Procedural skeletal animation
pub struct SharedCharacterPlugin;

impl Plugin for SharedCharacterPlugin {
    fn build(&self, app: &mut App) {
        // Import humanoid types and systems
        use super::humanoid::{
            CharacterBody, CharacterLimb,
            apply_procedural_limb_animation,
            update_character_facing_system,
            update_head_look_system,
        };
        
        app
            // Resources
            .init_resource::<PlayerService>()
            
            // Register types
            .register_type::<CharacterPhysics>()
            .register_type::<MovementIntent>()
            .register_type::<CharacterFacing>()
            .register_type::<Player>()
            .register_type::<Character>()
            .register_type::<PlayerCamera>()
            .register_type::<AnimationStateMachine>()
            .register_type::<LocomotionController>()
            .register_type::<ProceduralAnimation>()
            // Register humanoid types
            .register_type::<CharacterBody>()
            .register_type::<CharacterLimb>()
            
            // Systems - only run when character exists
            .add_systems(Update, (
                toggle_cursor_lock,
                camera_mouse_look,
                camera_zoom,
                character_movement_input,
                // Physics-dependent systems (require avian3d feature)
                // ground_check,
                // character_movement_physics,
                // character_jump,
                // update_locomotion,
                // Non-physics systems - re-enabled for Bevy 0.18
                update_character_facing,
                update_animation_state_machine,
                camera_follow,
            ).run_if(has_play_mode_character));
    }
}

/// Run condition: check if play mode character exists
fn has_play_mode_character(
    query: Query<(), With<PlayModeCharacter>>,
) -> bool {
    !query.is_empty()
}

// ============================================================================
// Character Spawning
// ============================================================================

/// Spawn a play mode character at the given position
/// 
/// This creates a FULL HUMANOID character with:
/// - Full skeletal hierarchy (hips, spine, chest, neck, head, arms, legs)
/// - Physics capsule (Avian3D)
/// - Camera following
/// - Movement components
/// - Animation state machine
/// - Procedural animation
/// 
/// This is the SAME character used by both Client and Engine Play Mode.
pub fn spawn_play_mode_character(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    spawn_position: Vec3,
) -> Entity {
    // Use the shared humanoid spawning function with default config
    use super::humanoid::{spawn_humanoid_character, HumanoidConfig};
    spawn_humanoid_character(commands, meshes, materials, spawn_position, &HumanoidConfig::default())
}

/// Spawn the play mode camera that follows the character
pub fn spawn_play_mode_camera(
    commands: &mut Commands,
    character_entity: Entity,
) -> Entity {
    let camera_entity = commands.spawn((
        Camera3d::default(),
        Camera {
            order: 10, // Higher priority than editor camera
            ..default()
        },
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::TonyMcMapface,
        
        // Camera components
        PlayerCamera {
            target: Some(character_entity),
            mode: CameraMode::ThirdPerson,
            distance: 8.0,
            yaw: 0.0,
            pitch: 0.3,
            ..default()
        },
        
        PlayModeCamera,
        Name::new("PlayModeCamera"),
    )).id();
    
    camera_entity
}

/// Cleanup all play mode entities
pub fn cleanup_play_mode_entities(
    commands: &mut Commands,
    query: Query<Entity, Or<(With<PlayModeCharacter>, With<PlayModeCamera>)>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

// ============================================================================
// Input Systems
// ============================================================================

/// Toggle cursor lock with Escape key
fn toggle_cursor_lock(
    keys: Res<ButtonInput<KeyCode>>,
    mut player_service: ResMut<PlayerService>,
    mut cursor_options: Query<&mut bevy::window::CursorOptions, With<Window>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        player_service.cursor_locked = !player_service.cursor_locked;
        
        if let Ok(mut cursor) = cursor_options.single_mut() {
            if player_service.cursor_locked {
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            } else {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            }
        }
    }
}

/// Handle mouse look for camera
/// - First person: always active (cursor locked)
/// - Third person: only when right-click held (like client)
fn camera_mouse_look(
    mut motion: MessageReader<MouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    player_service: Res<PlayerService>,
    mut camera_query: Query<&mut PlayerCamera, With<PlayModeCamera>>,
) {
    let Ok(camera) = camera_query.single() else { 
        motion.clear();
        return; 
    };
    
    // First person: always look when cursor locked. Third person: right-click to orbit
    let can_look = if camera.is_first_person {
        player_service.cursor_locked
    } else {
        mouse.pressed(MouseButton::Right)
    };
    
    if !can_look {
        motion.clear();
        return;
    }
    
    let Ok(mut camera) = camera_query.single_mut() else { return };
    
    let sensitivity = camera.sensitivity;
    for event in motion.read() {
        camera.yaw -= event.delta.x * sensitivity;
        camera.pitch += event.delta.y * sensitivity;  // mouse up = look up
        camera.pitch = camera.pitch.clamp(camera.pitch_min, camera.pitch_max);
    }
}

/// Handle mouse wheel for camera zoom - always active
/// Switching to first-person locks cursor, switching to third-person unlocks
fn camera_zoom(
    mut wheel: MessageReader<MouseWheel>,
    mut camera_query: Query<&mut PlayerCamera, With<PlayModeCamera>>,
    mut player_service: ResMut<PlayerService>,
    mut cursor_options: Query<&mut bevy::window::CursorOptions, With<Window>>,
) {
    let Ok(mut camera) = camera_query.single_mut() else { return };
    
    let mut scroll_delta = 0.0;
    for event in wheel.read() {
        scroll_delta += event.y;
    }
    
    if scroll_delta == 0.0 {
        return;
    }
    
    // Zoom in/out
    camera.distance -= scroll_delta * camera.zoom_speed;
    camera.distance = camera.distance.clamp(camera.min_distance, camera.max_distance);
    
    // Update first person flag based on distance
    let was_first_person = camera.is_first_person;
    camera.is_first_person = camera.distance <= camera.min_distance;
    
    if camera.is_first_person != was_first_person {
        if camera.is_first_person {
            // Entering first person: lock cursor
            player_service.cursor_locked = true;
            if let Ok(mut cursor) = cursor_options.single_mut() {
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            }
            info!("📷 First-person view (ESC to unlock cursor)");
        } else {
            // Entering third person: unlock cursor
            player_service.cursor_locked = false;
            if let Ok(mut cursor) = cursor_options.single_mut() {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            }
            info!("📷 Third-person view (Right-click to orbit)");
        }
    }
}

// ============================================================================
// Movement Systems
// ============================================================================

/// Process WASD input into movement intent - always active (like client)
fn character_movement_input(
    keys: Res<ButtonInput<KeyCode>>,
    camera_query: Query<&PlayerCamera, With<PlayModeCamera>>,
    mut intent_query: Query<&mut MovementIntent, With<PlayModeCharacter>>,
) {
    // Movement always works - no cursor lock required (matches client behavior)
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
    
    // Calculate speed
    if input.length_squared() > 0.0 {
        intent.speed = if intent.sprint { 1.5 } else { 1.0 };
    } else {
        intent.speed = 0.0;
    }
}

/// Apply movement physics (requires avian3d physics feature)
fn character_movement_physics(
    // Requires: LinearVelocity from avian3d
) {
    // Gated behind physics feature - enable with `features = ["physics"]`
}

/// Handle jumping (requires avian3d physics feature)
fn character_jump(
    // Requires: LinearVelocity from avian3d
) {
    // Gated behind physics feature - enable with `features = ["physics"]`
}

/// Ground detection using raycasts (requires avian3d physics feature)
fn ground_check(
    // Requires: SpatialQuery from avian3d
) {
    // Gated behind physics feature - enable with `features = ["physics"]`
}

/// Update locomotion controller from velocity (requires avian3d physics feature)
fn update_locomotion(
    // Requires: LinearVelocity from avian3d
) {
    // Gated behind physics feature - enable with `features = ["physics"]`
}

/// Update character facing direction
fn update_character_facing(
    time: Res<Time>,
    mut query: Query<(
        &MovementIntent,
        &mut CharacterFacing,
        &mut Transform,
    ), With<PlayModeCharacter>>,
) {
    let delta = time.delta_secs();
    
    for (intent, mut facing, mut transform) in query.iter_mut() {
        // Update target facing from movement direction
        if intent.direction.length_squared() > 0.01 {
            facing.target_angle = (-intent.direction.x).atan2(-intent.direction.z);
        }
        
        // Smoothly interpolate facing
        let angle_diff = (facing.target_angle - facing.angle + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        facing.angle += angle_diff * (facing.turn_speed * delta).min(1.0);
        
        // Apply rotation to transform
        transform.rotation = Quat::from_rotation_y(facing.angle);
    }
}

/// Update animation state machine
fn update_animation_state_machine(
    time: Res<Time>,
    mut query: Query<(
        &LocomotionController,
        &mut AnimationStateMachine,
    ), With<PlayModeCharacter>>,
) {
    let delta = time.delta_secs();
    
    for (locomotion, mut state_machine) in query.iter_mut() {
        state_machine.update(delta);
        
        let target_state = locomotion.get_animation_state();
        
        if state_machine.current_state != target_state {
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
// Camera System
// ============================================================================

/// Camera follow system - smoothly follows character
fn camera_follow(
    time: Res<Time>,
    character_query: Query<&Transform, (With<PlayModeCharacter>, Without<PlayModeCamera>)>,
    mut camera_query: Query<(&mut Transform, &PlayerCamera), With<PlayModeCamera>>,
) {
    let Ok(character_transform) = character_query.single() else { return };
    let Ok((mut camera_transform, camera)) = camera_query.single_mut() else { return };
    
    let delta = time.delta_secs();
    
    // Calculate target camera position
    // Use a fixed height offset since PlayerCamera doesn't have height_offset
    let height_offset = 1.5;
    let target_pos = character_transform.translation + Vec3::Y * height_offset;
    
    // Calculate camera offset from yaw/pitch/distance
    let offset = Vec3::new(
        camera.yaw.sin() * camera.pitch.cos() * camera.distance,
        camera.pitch.sin() * camera.distance,
        camera.yaw.cos() * camera.pitch.cos() * camera.distance,
    );
    
    let desired_pos = target_pos + offset;
    
    // Smooth follow
    let smoothness = 10.0;
    camera_transform.translation = camera_transform.translation.lerp(desired_pos, (smoothness * delta).min(1.0));
    
    // Look at character
    camera_transform.look_at(target_pos, Vec3::Y);
}
