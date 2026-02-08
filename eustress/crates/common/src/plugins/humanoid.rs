//! # Humanoid Character System
//!
//! Shared humanoid character spawning and animation for both client and engine.

use bevy::prelude::*;
#[cfg(feature = "physics")]
use avian3d::prelude::*;
use tracing::info;

use crate::classes::BasePart;
use crate::services::player::{
    Character, CharacterRoot
};
use crate::services::animation::{
    LocomotionController, AnimationStateMachine, AnimationState,
    ProceduralAnimation
};
use crate::plugins::character_plugin::{
    PlayModeCharacter, CharacterPhysics, CharacterFacing, MovementIntent
};
use crate::plugins::animation_plugin::{
    AnimationLayers, RootMotion
};

/// Game layer for collision filtering
pub struct GameLayer;

impl GameLayer {
    pub const GROUND: u32 = 1;
    pub const CHARACTER: u32 = 2;
    pub const PROP: u32 = 4;
}

// ============================================================================
// Types
// ============================================================================

/// Marker for character body parts
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterLimb {
    Hips, Spine, Chest, Neck,
    LeftShoulder, LeftUpperArm, LeftLowerArm, LeftHand,
    RightShoulder, RightUpperArm, RightLowerArm, RightHand,
    LeftUpperLeg, LeftLowerLeg, LeftFoot,
    RightUpperLeg, RightLowerLeg, RightFoot,
}

/// Marker for head
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
pub struct CharacterHead;

/// Body part entity references
#[derive(Component, Reflect, Clone, Copy, Debug)]
pub struct CharacterBody {
    pub hips: Entity,
    pub spine: Entity,
    pub chest: Entity,
    pub neck: Entity,
    pub head: Entity,
    pub left_shoulder: Entity,
    pub left_upper_arm: Entity,
    pub left_lower_arm: Entity,
    pub left_hand: Entity,
    pub right_shoulder: Entity,
    pub right_upper_arm: Entity,
    pub right_lower_arm: Entity,
    pub right_hand: Entity,
    pub left_upper_leg: Entity,
    pub left_lower_leg: Entity,
    pub left_foot: Entity,
    pub right_upper_leg: Entity,
    pub right_lower_leg: Entity,
    pub right_foot: Entity,
}

impl CharacterBody {
    pub fn new(hips: Entity) -> Self {
        Self {
            hips,
            spine: hips,
            chest: hips,
            neck: hips,
            head: hips,
            left_shoulder: hips,
            left_upper_arm: hips,
            left_lower_arm: hips,
            left_hand: hips,
            right_shoulder: hips,
            right_upper_arm: hips,
            right_lower_arm: hips,
            right_hand: hips,
            left_upper_leg: hips,
            left_lower_leg: hips,
            left_foot: hips,
            right_upper_leg: hips,
            right_lower_leg: hips,
            right_foot: hips,
        }
    }
}

/// Configuration for humanoid character spawning
#[derive(Clone, Debug)]
pub struct HumanoidConfig {
    pub scale: f32,
    pub skin_color: Color,
    pub shirt_color: Color,
    pub pants_color: Color,
    pub shoe_color: Color,
    pub eye_color: Color,
}

impl Default for HumanoidConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            skin_color: Color::srgb(0.96, 0.80, 0.69),
            shirt_color: Color::srgb(0.2, 0.5, 0.9),
            pants_color: Color::srgb(0.15, 0.15, 0.2),
            shoe_color: Color::srgb(0.1, 0.1, 0.1),
            eye_color: Color::srgb(0.1, 0.1, 0.1),
        }
    }
}

/// Helper to create a beveled box mesh
fn create_beveled_box(width: f32, height: f32, depth: f32, bevel: f32) -> Mesh {
    // Simplified beveled box - just return a regular box for now
    Cuboid::new(width, height, depth).into()
}

/// Spawn a full humanoid character - DISABLED for Bevy 0.19
pub fn spawn_humanoid_character(
    commands: &mut Commands,
    _meshes: &mut Assets<Mesh>,
    _materials: &mut Assets<StandardMaterial>,
    spawn_pos: Vec3,
    _config: &HumanoidConfig,
) -> Entity {
    // Disabled - Mesh3d Bundle API changed in Bevy 0.19
    commands.spawn((
        Transform::from_translation(spawn_pos),
        Visibility::default(),
        Name::new("HumanoidPlaceholder"),
    )).id()
}

/// Apply procedural skeletal animation - DISABLED for Bevy 0.19
pub fn apply_procedural_limb_animation(
    // _time: Res<Time>,
    // _character_query: Query<(
    //     &LocomotionController,
    //     &CharacterBody,
    // )>,
    // _limb_query: Query<&mut Transform, With<CharacterLimb>>,
) {
    // Disabled - ProceduralAnimation not available in Bevy 0.19
}

fn animate_arm(
    limb_query: &mut Query<&mut Transform, With<CharacterLimb>>,
    upper_arm: Entity, lower_arm: Entity, hand: Entity,
    walk_cycle: f32, speed_factor: f32, is_moving: bool, is_airborne: bool, is_jumping: bool,
    is_left: bool, elapsed: f32,
) {
    let phase = if is_left { 0.0 } else { std::f32::consts::PI };
    
    if is_airborne {
        if is_jumping {
            if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
                transform.rotation = Quat::from_rotation_x(-0.5);
            }
            if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
                transform.rotation = Quat::from_rotation_x(-1.4);
            }
            if let Ok(mut transform) = limb_query.get_mut(hand) {
                transform.rotation = Quat::IDENTITY;
            }
        } else {
            if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
                transform.rotation = Quat::from_rotation_x(0.4);
            }
            if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
                transform.rotation = Quat::from_rotation_x(-0.6);
            }
            if let Ok(mut transform) = limb_query.get_mut(hand) {
                transform.rotation = Quat::IDENTITY;
            }
        }
    } else if is_moving {
        let shoulder_swing = (walk_cycle + phase).sin();
        let shoulder_angle = shoulder_swing * 0.6 * speed_factor;
        
        if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
            transform.rotation = Quat::from_rotation_x(shoulder_angle);
        }
        
        let elbow_base = 0.3 * speed_factor;
        let elbow_swing = ((-shoulder_swing + 1.0) * 0.5) * 0.5 * speed_factor;
        let elbow_bend = -(elbow_base + elbow_swing);
        
        if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
            transform.rotation = Quat::from_rotation_x(elbow_bend.min(-0.1));
        }
        
        if let Ok(mut transform) = limb_query.get_mut(hand) {
            transform.rotation = Quat::IDENTITY;
        }
    } else {
        let breath_phase = elapsed * 0.8;
        let sway_offset = if is_left { 0.0 } else { 0.3 };
        let idle_sway = (breath_phase + sway_offset).sin() * 0.02;
        let side_drift = (breath_phase * 0.7 + sway_offset).cos() * 0.01;
        
        if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
            transform.rotation = Quat::from_rotation_x(idle_sway) * Quat::from_rotation_z(side_drift);
        }
        if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
            transform.rotation = Quat::from_rotation_x(-0.2);
        }
        if let Ok(mut transform) = limb_query.get_mut(hand) {
            transform.rotation = Quat::from_rotation_x(-0.05);
        }
    }
}

fn animate_leg(
    limb_query: &mut Query<&mut Transform, With<CharacterLimb>>,
    upper_leg: Entity, lower_leg: Entity, foot: Entity,
    walk_cycle: f32, speed_factor: f32, is_moving: bool, is_airborne: bool, is_jumping: bool,
    air_time: f32, is_left: bool,
) {
    let phase = if is_left { std::f32::consts::PI } else { 0.0 };
    
    if is_airborne {
        if is_jumping {
            let leg_offset = if is_left { 0.1 } else { -0.1 };
            let hip_tuck = -0.5 + leg_offset;
            let knee_bend = 0.9;
            
            if let Ok(mut transform) = limb_query.get_mut(upper_leg) {
                transform.rotation = Quat::from_rotation_x(hip_tuck);
            }
            if let Ok(mut transform) = limb_query.get_mut(lower_leg) {
                transform.rotation = Quat::from_rotation_x(knee_bend);
            }
            if let Ok(mut transform) = limb_query.get_mut(foot) {
                transform.rotation = Quat::from_rotation_x(0.3);
            }
        } else {
            let extend_factor = (air_time * 3.0).min(1.0);
            let hip_angle = -0.2 * (1.0 - extend_factor) + 0.1 * extend_factor;
            let knee_bend = 0.6 * (1.0 - extend_factor) + 0.3 * extend_factor;
            
            if let Ok(mut transform) = limb_query.get_mut(upper_leg) {
                transform.rotation = Quat::from_rotation_x(hip_angle);
            }
            if let Ok(mut transform) = limb_query.get_mut(lower_leg) {
                transform.rotation = Quat::from_rotation_x(knee_bend);
            }
            if let Ok(mut transform) = limb_query.get_mut(foot) {
                transform.rotation = Quat::from_rotation_x(-0.1);
            }
        }
    } else if is_moving {
        let cycle = walk_cycle + phase;
        let hip_swing = cycle.sin();
        let cycle_phase = cycle % std::f32::consts::TAU;
        
        let hip_amplitude = 0.4 + 0.2 * speed_factor;
        let hip_angle = hip_swing * hip_amplitude;
        
        if let Ok(mut transform) = limb_query.get_mut(upper_leg) {
            transform.rotation = Quat::from_rotation_x(hip_angle);
        }
        
        let knee_bend = if cycle_phase > std::f32::consts::PI {
            let swing_progress = (cycle_phase - std::f32::consts::PI) / std::f32::consts::PI;
            let bend_curve = (swing_progress * std::f32::consts::PI).sin();
            let bend_amplitude = 0.8 + 0.6 * speed_factor;
            bend_curve * bend_amplitude
        } else {
            0.1 + 0.05 * speed_factor
        };
        
        if let Ok(mut transform) = limb_query.get_mut(lower_leg) {
            transform.rotation = Quat::from_rotation_x(knee_bend);
        }
        
        let foot_angle = if cycle_phase < std::f32::consts::FRAC_PI_2 {
            0.3 * speed_factor
        } else if cycle_phase > std::f32::consts::PI * 1.5 {
            -0.2
        } else {
            -hip_angle * 0.3 - knee_bend * 0.15
        };
        
        if let Ok(mut transform) = limb_query.get_mut(foot) {
            transform.rotation = Quat::from_rotation_x(foot_angle);
        }
    } else {
        if let Ok(mut transform) = limb_query.get_mut(upper_leg) {
            transform.rotation = Quat::IDENTITY;
        }
        if let Ok(mut transform) = limb_query.get_mut(lower_leg) {
            transform.rotation = Quat::IDENTITY;
        }
        if let Ok(mut transform) = limb_query.get_mut(foot) {
            transform.rotation = Quat::IDENTITY;
        }
    }
}

/// Update character facing - rotates the HIPS which propagates to all children
pub fn update_character_facing_system(
    time: Res<Time>,
    mut query: Query<(&mut CharacterFacing, &CharacterBody)>,
    mut limb_query: Query<&mut Transform, With<CharacterLimb>>,
) {
    let delta = time.delta_secs();
    
    for (mut facing, body) in query.iter_mut() {
        let angle_diff = angle_difference(facing.target_angle, facing.angle);
        facing.angle += angle_diff * facing.turn_speed * delta;
        facing.angle = facing.angle % std::f32::consts::TAU;
        
        if let Ok(mut transform) = limb_query.get_mut(body.hips) {
            transform.rotation = Quat::from_rotation_y(facing.angle);
        }
    }
}

/// Update head look (follows camera within neck limits)
pub fn update_head_look_system(
    time: Res<Time>,
    mut query: Query<(&mut CharacterFacing, &CharacterBody)>,
    mut limb_query: Query<&mut Transform, With<CharacterLimb>>,
) {
    let delta = time.delta_secs();
    
    for (mut facing, body) in query.iter_mut() {
        facing.head_look = facing.head_look.lerp(facing.head_look_target, 8.0 * delta);
        
        if let Ok(mut transform) = limb_query.get_mut(body.neck) {
            let neck_yaw = -facing.head_look.x * 0.4;
            transform.rotation = Quat::from_rotation_y(neck_yaw);
        }
        
        if let Ok(mut transform) = limb_query.get_mut(body.head) {
            let head_yaw = -facing.head_look.x * 0.6;
            transform.rotation = Quat::from_rotation_y(head_yaw);
        }
    }
}

/// Helper to get shortest angle difference
pub fn angle_difference(a: f32, b: f32) -> f32 {
    let diff = (a - b) % std::f32::consts::TAU;
    if diff > std::f32::consts::PI {
        diff - std::f32::consts::TAU
    } else if diff < -std::f32::consts::PI {
        diff + std::f32::consts::TAU
    } else {
        diff
    }
}
