//! # Shared Humanoid Character System
//!
//! AAA-quality humanoid character shared between Client and Engine Play Mode.
//! This ensures identical gameplay behavior and visual appearance in both contexts.
//!
//! ## Features
//!
//! - Full skeletal hierarchy (hips, spine, chest, neck, head, arms, legs)
//! - Physics-based movement with Avian3D
//! - Procedural animation blending
//! - Smooth camera following
//! - State machine for animation transitions
//!
//! ## Architecture
//!
//! ```text
//! Root (physics capsule)
//! └── Hips (pivot point)
//!     ├── Spine
//!     │   └── Chest
//!     │       ├── Neck
//!     │       │   └── Head
//!     │       ├── LeftShoulder → LeftUpperArm → LeftLowerArm → LeftHand
//!     │       └── RightShoulder → RightUpperArm → RightLowerArm → RightHand
//!     ├── LeftUpperLeg → LeftLowerLeg → LeftFoot
//!     └── RightUpperLeg → RightLowerLeg → RightFoot
//! ```

use bevy::prelude::*;
use bevy::mesh::{Indices, PrimitiveTopology};
use avian3d::prelude::*;

use crate::classes::{Instance, ClassName};
use crate::services::player::{Character, CharacterRoot, CharacterHead};
use crate::services::animation::{
    AnimationStateMachine, LocomotionController, ProceduralAnimation,
};

// Re-export from character_plugin for compatibility
pub use super::character_plugin::{
    CharacterPhysics, MovementIntent, CharacterFacing,
    PlayModeCharacter, PlayModeCamera,
};

// ============================================================================
// Components
// ============================================================================

/// Full humanoid skeleton with proper joint hierarchy
/// 
/// Each bone pivots from its TOP (where it connects to parent).
/// The hierarchy propagates transforms automatically.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct CharacterBody {
    pub root: Entity,
    // Spine chain
    pub hips: Entity,
    pub spine: Entity,
    pub chest: Entity,
    pub neck: Entity,
    pub head: Entity,
    // Left arm chain
    pub left_shoulder: Entity,
    pub left_upper_arm: Entity,
    pub left_lower_arm: Entity,
    pub left_hand: Entity,
    // Right arm chain
    pub right_shoulder: Entity,
    pub right_upper_arm: Entity,
    pub right_lower_arm: Entity,
    pub right_hand: Entity,
    // Left leg chain
    pub left_upper_leg: Entity,
    pub left_lower_leg: Entity,
    pub left_foot: Entity,
    // Right leg chain
    pub right_upper_leg: Entity,
    pub right_lower_leg: Entity,
    pub right_foot: Entity,
}

/// Bone type for skeletal animation
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Eq)]
#[reflect(Component)]
pub enum CharacterLimb {
    // Spine chain
    Hips,
    Spine,
    Chest,
    Neck,
    Head,
    // Left arm chain
    LeftShoulder,
    LeftUpperArm,
    LeftLowerArm,
    LeftHand,
    // Right arm chain
    RightShoulder,
    RightUpperArm,
    RightLowerArm,
    RightHand,
    // Left leg chain
    LeftUpperLeg,
    LeftLowerLeg,
    LeftFoot,
    // Right leg chain
    RightUpperLeg,
    RightLowerLeg,
    RightFoot,
}

/// Configuration for humanoid character spawning
#[derive(Clone, Debug)]
pub struct HumanoidConfig {
    /// Character scale (1.0 = normal human ~1.75m)
    pub scale: f32,
    /// Skin color
    pub skin_color: Color,
    /// Shirt/upper body color
    pub shirt_color: Color,
    /// Pants/lower body color
    pub pants_color: Color,
    /// Shoe color
    pub shoe_color: Color,
    /// Eye color (for smiley face)
    pub eye_color: Color,
}

impl Default for HumanoidConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            skin_color: Color::srgb(0.85, 0.70, 0.55),
            shirt_color: Color::srgb(0.2, 0.4, 0.7),
            pants_color: Color::srgb(0.15, 0.15, 0.2),
            shoe_color: Color::srgb(0.1, 0.1, 0.1),
            eye_color: Color::srgb(0.1, 0.1, 0.1),
        }
    }
}

// ============================================================================
// Mesh Helpers
// ============================================================================

/// Create a beveled box mesh with chamfered top/bottom edges
pub fn create_beveled_box(width: f32, height: f32, depth: f32, bevel: f32) -> Mesh {
    let hw = width / 2.0;
    let hh = height / 2.0;
    let hd = depth / 2.0;
    let b = bevel.min(hw.min(hh.min(hd)) * 0.4);
    
    let positions: Vec<[f32; 3]> = vec![
        // Top ring (inset) - 0,1,2,3
        [-hw + b, hh, -hd + b], [hw - b, hh, -hd + b], [hw - b, hh, hd - b], [-hw + b, hh, hd - b],
        // Upper ring (full) - 4,5,6,7
        [-hw, hh - b, -hd], [hw, hh - b, -hd], [hw, hh - b, hd], [-hw, hh - b, hd],
        // Lower ring (full) - 8,9,10,11
        [-hw, -hh + b, -hd], [hw, -hh + b, -hd], [hw, -hh + b, hd], [-hw, -hh + b, hd],
        // Bottom ring (inset) - 12,13,14,15
        [-hw + b, -hh, -hd + b], [hw - b, -hh, -hd + b], [hw - b, -hh, hd - b], [-hw + b, -hh, hd - b],
    ];
    
    let n_up: [f32; 3] = [0.0, 1.0, 0.0];
    let n_down: [f32; 3] = [0.0, -1.0, 0.0];
    let n_bevel_up: [f32; 3] = [0.0, 0.707, 0.0];
    let n_bevel_down: [f32; 3] = [0.0, -0.707, 0.0];
    
    let normals: Vec<[f32; 3]> = vec![
        n_up, n_up, n_up, n_up,
        n_bevel_up, n_bevel_up, n_bevel_up, n_bevel_up,
        n_bevel_down, n_bevel_down, n_bevel_down, n_bevel_down,
        n_down, n_down, n_down, n_down,
    ];
    
    let uvs: Vec<[f32; 2]> = vec![
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
    ];
    
    let indices: Vec<u32> = vec![
        // Top face
        0, 2, 1, 0, 3, 2,
        // Top bevel
        0, 1, 5, 0, 5, 4,
        1, 2, 6, 1, 6, 5,
        2, 3, 7, 2, 7, 6,
        3, 0, 4, 3, 4, 7,
        // Middle sides
        4, 5, 9, 4, 9, 8,
        5, 6, 10, 5, 10, 9,
        6, 7, 11, 6, 11, 10,
        7, 4, 8, 7, 8, 11,
        // Bottom bevel
        8, 9, 13, 8, 13, 12,
        9, 10, 14, 9, 14, 13,
        10, 11, 15, 10, 15, 14,
        11, 8, 12, 11, 12, 15,
        // Bottom face
        12, 13, 14, 12, 14, 15,
    ];
    
    Mesh::new(PrimitiveTopology::TriangleList, default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

// ============================================================================
// Character Spawning
// ============================================================================

/// Spawn a full humanoid character with physics and skeletal hierarchy
/// 
/// This is the SHARED spawn function used by both Client and Engine Play Mode.
/// Returns the root character entity.
pub fn spawn_humanoid_character(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    spawn_pos: Vec3,
    config: &HumanoidConfig,
) -> Entity {
    let character_scale = config.scale;
    
    // Base proportions (in meters, based on ~1.75m human at scale 1.0)
    let base_height = 1.75;
    let total_height = base_height * character_scale;
    
    // All proportions scale with character
    let _head_height = 0.23 * character_scale;
    let neck_length = 0.08 * character_scale;
    let chest_height = 0.25 * character_scale;
    let spine_height = 0.12 * character_scale;
    let upper_arm_length = 0.30 * character_scale;
    let lower_arm_length = 0.26 * character_scale;
    let hand_length = 0.10 * character_scale;
    let upper_leg_length = 0.40 * character_scale;
    let lower_leg_length = 0.40 * character_scale;
    let foot_height = 0.08 * character_scale;
    
    // Calculate hip height from leg lengths
    let hip_joint_offset = 0.08 * character_scale;
    let hip_height_from_ground = hip_joint_offset + upper_leg_length + lower_leg_length + foot_height;
    
    // Widths scale with character
    let shoulder_width = 0.48 * character_scale;
    let hip_width = 0.28 * character_scale;
    let chest_width = 0.36 * character_scale;
    let chest_depth = 0.22 * character_scale;
    let head_radius = 0.13 * character_scale;
    let neck_radius = 0.055 * character_scale;
    let upper_arm_radius = 0.04 * character_scale;
    let lower_arm_radius = 0.035 * character_scale;
    let hand_width = 0.08 * character_scale;
    let upper_leg_radius = 0.07 * character_scale;
    let lower_leg_radius = 0.05 * character_scale;
    let foot_length = 0.25 * character_scale;
    let foot_width = 0.09 * character_scale;
    
    // Physics capsule
    let capsule_radius = (shoulder_width / 2.0_f32).max(0.2 * character_scale);
    let capsule_total_height = total_height;
    let capsule_half_height = (capsule_total_height / 2.0) - capsule_radius;
    let capsule_center_height = capsule_radius + capsule_half_height;
    let spawn_height = capsule_center_height + 0.5;
    
    // Materials
    let skin_mat = materials.add(StandardMaterial {
        base_color: config.skin_color,
        perceptual_roughness: 0.7,
        ..default()
    });
    let shirt_mat = materials.add(StandardMaterial {
        base_color: config.shirt_color,
        perceptual_roughness: 0.8,
        ..default()
    });
    let pants_mat = materials.add(StandardMaterial {
        base_color: config.pants_color,
        perceptual_roughness: 0.9,
        ..default()
    });
    let shoe_mat = materials.add(StandardMaterial {
        base_color: config.shoe_color,
        perceptual_roughness: 0.95,
        ..default()
    });
    let eye_mat = materials.add(StandardMaterial {
        base_color: config.eye_color,
        unlit: true,
        ..default()
    });
    
    // Spawn physics root
    let character_entity = commands.spawn((
        Transform::from_translation(spawn_pos + Vec3::Y * spawn_height),
        Visibility::default(),
        RigidBody::Dynamic,
        Collider::capsule(capsule_radius, capsule_half_height),
        CollisionMargin(0.02),
        LockedAxes::ROTATION_LOCKED,
        Friction::new(1.0),
        Restitution::new(0.0),
        GravityScale(1.0),
        LinearVelocity::default(),
        SweptCcd::default(),
        Name::new("HumanoidCharacter"),
    )).id();
    
    // Add character components
    commands.entity(character_entity).insert((
        Character::default(),
        CharacterRoot,
        CharacterPhysics::default(),
        CharacterFacing::default(),
        MovementIntent::default(),
        AnimationStateMachine::default(),
        LocomotionController::default(),
        ProceduralAnimation::default(),
        PlayModeCharacter,
        Instance {
            name: "Player".to_string(),
            class_name: ClassName::Model,
            archivable: false,
            ai: false,
            id: 0,
        },
    ));
    
    // Visual offset from capsule center to hip pivot
    let ground_clearance = 0.5;
    let visual_y_offset = hip_height_from_ground - capsule_center_height + ground_clearance;
    
    // =========================================================================
    // SPINE CHAIN: Hips -> Spine -> Chest -> Neck -> Head
    // =========================================================================
    
    // HIPS
    let hips = commands.spawn((
        Transform::from_xyz(0.0, visual_y_offset, 0.0),
        Visibility::default(),
        ChildOf(character_entity),
        CharacterLimb::Hips,
        Name::new("Hips"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(hip_width, 0.15, chest_depth * 0.9, 0.02))),
        MeshMaterial3d(pants_mat.clone()),
        Transform::from_xyz(0.0, -0.075, 0.0),
        ChildOf(hips),
        Name::new("HipsMesh"),
    ));
    
    // SPINE
    let spine = commands.spawn((
        Transform::from_xyz(0.0, 0.0, 0.0),
        Visibility::default(),
        ChildOf(hips),
        CharacterLimb::Spine,
        Name::new("Spine"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(chest_width * 0.8, spine_height, chest_depth * 0.8, 0.015))),
        MeshMaterial3d(shirt_mat.clone()),
        Transform::from_xyz(0.0, spine_height / 2.0, 0.0),
        ChildOf(spine),
        Name::new("SpineMesh"),
    ));
    
    // CHEST
    let chest = commands.spawn((
        Transform::from_xyz(0.0, spine_height, 0.0),
        Visibility::default(),
        ChildOf(spine),
        CharacterLimb::Chest,
        Name::new("Chest"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(chest_width, chest_height, chest_depth, 0.025))),
        MeshMaterial3d(shirt_mat.clone()),
        Transform::from_xyz(0.0, chest_height / 2.0, 0.0),
        ChildOf(chest),
        Name::new("ChestMesh"),
    ));
    
    // NECK
    let neck = commands.spawn((
        Transform::from_xyz(0.0, chest_height, 0.0),
        Visibility::default(),
        ChildOf(chest),
        CharacterLimb::Neck,
        Name::new("Neck"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(neck_radius, neck_length))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, neck_length / 2.0, 0.0),
        ChildOf(neck),
        Name::new("NeckMesh"),
    ));
    
    // HEAD
    let head = commands.spawn((
        Transform::from_xyz(0.0, neck_length, 0.0),
        Visibility::default(),
        ChildOf(neck),
        CharacterLimb::Head,
        CharacterHead,
        Name::new("Head"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(head_radius))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, head_radius, 0.0),
        ChildOf(head),
        Name::new("HeadMesh"),
    ));
    
    // SMILEY FACE
    let face_z = head_radius * 0.95;
    let face_y = head_radius;
    let eye_radius = head_radius * 0.08;
    let eye_spacing = head_radius * 0.25;
    let eye_height = head_radius * 0.15;
    
    // Left eye
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(eye_radius))),
        MeshMaterial3d(eye_mat.clone()),
        Transform::from_xyz(-eye_spacing, face_y + eye_height, face_z),
        ChildOf(head),
        Name::new("LeftEye"),
    ));
    
    // Right eye
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(eye_radius))),
        MeshMaterial3d(eye_mat.clone()),
        Transform::from_xyz(eye_spacing, face_y + eye_height, face_z),
        ChildOf(head),
        Name::new("RightEye"),
    ));
    
    // Smile
    let smile_radius = head_radius * 0.035;
    let smile_curve_radius = head_radius * 0.3;
    let smile_y_offset = -head_radius * 0.25;
    for i in 0..9 {
        let angle = std::f32::consts::PI * 0.2 + (i as f32 / 8.0) * std::f32::consts::PI * 0.6;
        let x = angle.cos() * smile_curve_radius;
        let y = face_y + smile_y_offset - angle.sin() * smile_curve_radius * 0.4;
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(smile_radius))),
            MeshMaterial3d(eye_mat.clone()),
            Transform::from_xyz(x, y, face_z),
            ChildOf(head),
            Name::new(format!("Smile{}", i)),
        ));
    }
    
    // =========================================================================
    // ARM CHAINS
    // =========================================================================
    
    let shoulder_y = chest_height - 0.03;
    let shoulder_x = chest_width / 2.0 + 0.02;
    
    // --- LEFT ARM ---
    let left_shoulder = commands.spawn((
        Transform::from_xyz(-shoulder_x, shoulder_y, 0.0),
        Visibility::default(),
        ChildOf(chest),
        CharacterLimb::LeftShoulder,
        Name::new("LeftShoulder"),
    )).id();
    
    let left_upper_arm = commands.spawn((
        Transform::IDENTITY,
        Visibility::default(),
        ChildOf(left_shoulder),
        CharacterLimb::LeftUpperArm,
        Name::new("LeftUpperArm"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(upper_arm_radius, upper_arm_length - upper_arm_radius * 2.0))),
        MeshMaterial3d(shirt_mat.clone()),
        Transform::from_xyz(0.0, -upper_arm_length / 2.0, 0.0),
        ChildOf(left_upper_arm),
        Name::new("LeftUpperArmMesh"),
    ));
    
    let left_lower_arm = commands.spawn((
        Transform::from_xyz(0.0, -upper_arm_length, 0.0),
        Visibility::default(),
        ChildOf(left_upper_arm),
        CharacterLimb::LeftLowerArm,
        Name::new("LeftLowerArm"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(lower_arm_radius, lower_arm_length - lower_arm_radius * 2.0))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, -lower_arm_length / 2.0, 0.0),
        ChildOf(left_lower_arm),
        Name::new("LeftLowerArmMesh"),
    ));
    
    let left_hand = commands.spawn((
        Transform::from_xyz(0.0, -lower_arm_length, 0.0)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2)),
        Visibility::default(),
        ChildOf(left_lower_arm),
        CharacterLimb::LeftHand,
        Name::new("LeftHand"),
    )).id();
    
    // Left hand details
    let palm_width = hand_width * 0.9;
    let palm_length = hand_length * 0.55;
    let palm_depth = 0.025 * character_scale;
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(palm_width, palm_length, palm_depth, 0.008))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, -palm_length / 2.0, 0.0),
        ChildOf(left_hand),
        Name::new("LeftPalmMesh"),
    ));
    
    let finger_radius = 0.012 * character_scale;
    let finger_length = hand_length * 0.5;
    let finger_spacing = palm_width / 5.0;
    let finger_names = ["Index", "Middle", "Ring", "Pinky"];
    let finger_lengths = [0.95, 1.0, 0.95, 0.8];
    for (i, (name, len_mult)) in finger_names.iter().zip(finger_lengths.iter()).enumerate() {
        let x_offset = (i as f32 - 1.5) * finger_spacing;
        let this_finger_len = finger_length * len_mult;
        commands.spawn((
            Mesh3d(meshes.add(Capsule3d::new(finger_radius, this_finger_len - finger_radius * 2.0))),
            MeshMaterial3d(skin_mat.clone()),
            Transform::from_xyz(x_offset, -palm_length - this_finger_len / 2.0, 0.0),
            ChildOf(left_hand),
            Name::new(format!("Left{}Finger", name)),
        ));
    }
    
    let thumb_length = finger_length * 0.7;
    let thumb_x = -palm_width / 2.0 - finger_radius;
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(finger_radius * 1.1, thumb_length - finger_radius * 2.0))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(thumb_x, -palm_length * 0.3, palm_depth * 0.5)
            .with_rotation(Quat::from_rotation_z(0.5) * Quat::from_rotation_x(0.2)),
        ChildOf(left_hand),
        Name::new("LeftThumb"),
    ));
    
    // --- RIGHT ARM ---
    let right_shoulder = commands.spawn((
        Transform::from_xyz(shoulder_x, shoulder_y, 0.0),
        Visibility::default(),
        ChildOf(chest),
        CharacterLimb::RightShoulder,
        Name::new("RightShoulder"),
    )).id();
    
    let right_upper_arm = commands.spawn((
        Transform::IDENTITY,
        Visibility::default(),
        ChildOf(right_shoulder),
        CharacterLimb::RightUpperArm,
        Name::new("RightUpperArm"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(upper_arm_radius, upper_arm_length - upper_arm_radius * 2.0))),
        MeshMaterial3d(shirt_mat.clone()),
        Transform::from_xyz(0.0, -upper_arm_length / 2.0, 0.0),
        ChildOf(right_upper_arm),
        Name::new("RightUpperArmMesh"),
    ));
    
    let right_lower_arm = commands.spawn((
        Transform::from_xyz(0.0, -upper_arm_length, 0.0),
        Visibility::default(),
        ChildOf(right_upper_arm),
        CharacterLimb::RightLowerArm,
        Name::new("RightLowerArm"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(lower_arm_radius, lower_arm_length - lower_arm_radius * 2.0))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, -lower_arm_length / 2.0, 0.0),
        ChildOf(right_lower_arm),
        Name::new("RightLowerArmMesh"),
    ));
    
    let right_hand = commands.spawn((
        Transform::from_xyz(0.0, -lower_arm_length, 0.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
        Visibility::default(),
        ChildOf(right_lower_arm),
        CharacterLimb::RightHand,
        Name::new("RightHand"),
    )).id();
    
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(palm_width, palm_length, palm_depth, 0.008))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, -palm_length / 2.0, 0.0),
        ChildOf(right_hand),
        Name::new("RightPalmMesh"),
    ));
    
    for (i, (name, len_mult)) in finger_names.iter().zip(finger_lengths.iter()).enumerate() {
        let x_offset = (i as f32 - 1.5) * finger_spacing;
        let this_finger_len = finger_length * len_mult;
        commands.spawn((
            Mesh3d(meshes.add(Capsule3d::new(finger_radius, this_finger_len - finger_radius * 2.0))),
            MeshMaterial3d(skin_mat.clone()),
            Transform::from_xyz(x_offset, -palm_length - this_finger_len / 2.0, 0.0),
            ChildOf(right_hand),
            Name::new(format!("Right{}Finger", name)),
        ));
    }
    
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(finger_radius * 1.1, thumb_length - finger_radius * 2.0))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(palm_width / 2.0 + finger_radius, -palm_length * 0.3, palm_depth * 0.5)
            .with_rotation(Quat::from_rotation_z(-0.5) * Quat::from_rotation_x(0.2)),
        ChildOf(right_hand),
        Name::new("RightThumb"),
    ));
    
    // =========================================================================
    // LEG CHAINS
    // =========================================================================
    
    let hip_joint_x = hip_width / 2.0 - 0.02 * character_scale;
    let hip_joint_y = -hip_joint_offset;
    
    // --- LEFT LEG ---
    let left_upper_leg = commands.spawn((
        Transform::from_xyz(-hip_joint_x, hip_joint_y, 0.0),
        Visibility::default(),
        ChildOf(hips),
        CharacterLimb::LeftUpperLeg,
        Name::new("LeftUpperLeg"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(upper_leg_radius, upper_leg_length - upper_leg_radius * 2.0))),
        MeshMaterial3d(pants_mat.clone()),
        Transform::from_xyz(0.0, -upper_leg_length / 2.0, 0.0),
        ChildOf(left_upper_leg),
        Name::new("LeftUpperLegMesh"),
    ));
    
    let left_lower_leg = commands.spawn((
        Transform::from_xyz(0.0, -upper_leg_length, 0.0),
        Visibility::default(),
        ChildOf(left_upper_leg),
        CharacterLimb::LeftLowerLeg,
        Name::new("LeftLowerLeg"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(lower_leg_radius, lower_leg_length - lower_leg_radius * 2.0))),
        MeshMaterial3d(pants_mat.clone()),
        Transform::from_xyz(0.0, -lower_leg_length / 2.0, 0.0),
        ChildOf(left_lower_leg),
        Name::new("LeftLowerLegMesh"),
    ));
    
    let left_foot = commands.spawn((
        Transform::from_xyz(0.0, -lower_leg_length, 0.0),
        Visibility::default(),
        ChildOf(left_lower_leg),
        CharacterLimb::LeftFoot,
        Name::new("LeftFoot"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(foot_width, foot_height, foot_length, 0.015))),
        MeshMaterial3d(shoe_mat.clone()),
        Transform::from_xyz(0.0, -foot_height / 2.0, foot_length / 2.0 - 0.04),
        ChildOf(left_foot),
        Name::new("LeftFootMesh"),
    ));
    
    // --- RIGHT LEG ---
    let right_upper_leg = commands.spawn((
        Transform::from_xyz(hip_joint_x, hip_joint_y, 0.0),
        Visibility::default(),
        ChildOf(hips),
        CharacterLimb::RightUpperLeg,
        Name::new("RightUpperLeg"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(upper_leg_radius, upper_leg_length - upper_leg_radius * 2.0))),
        MeshMaterial3d(pants_mat.clone()),
        Transform::from_xyz(0.0, -upper_leg_length / 2.0, 0.0),
        ChildOf(right_upper_leg),
        Name::new("RightUpperLegMesh"),
    ));
    
    let right_lower_leg = commands.spawn((
        Transform::from_xyz(0.0, -upper_leg_length, 0.0),
        Visibility::default(),
        ChildOf(right_upper_leg),
        CharacterLimb::RightLowerLeg,
        Name::new("RightLowerLeg"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(lower_leg_radius, lower_leg_length - lower_leg_radius * 2.0))),
        MeshMaterial3d(pants_mat.clone()),
        Transform::from_xyz(0.0, -lower_leg_length / 2.0, 0.0),
        ChildOf(right_lower_leg),
        Name::new("RightLowerLegMesh"),
    ));
    
    let right_foot = commands.spawn((
        Transform::from_xyz(0.0, -lower_leg_length, 0.0),
        Visibility::default(),
        ChildOf(right_lower_leg),
        CharacterLimb::RightFoot,
        Name::new("RightFoot"),
    )).id();
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(foot_width, foot_height, foot_length, 0.015))),
        MeshMaterial3d(shoe_mat.clone()),
        Transform::from_xyz(0.0, -foot_height / 2.0, foot_length / 2.0 - 0.04),
        ChildOf(right_foot),
        Name::new("RightFootMesh"),
    ));
    
    // Store full skeleton reference
    commands.entity(character_entity).insert(CharacterBody {
        root: character_entity,
        hips,
        spine,
        chest,
        neck,
        head,
        left_shoulder,
        left_upper_arm,
        left_lower_arm,
        left_hand,
        right_shoulder,
        right_upper_arm,
        right_lower_arm,
        right_hand,
        left_upper_leg,
        left_lower_leg,
        left_foot,
        right_upper_leg,
        right_lower_leg,
        right_foot,
    });
    
    character_entity
}

// ============================================================================
// Animation Systems
// ============================================================================

/// Apply procedural skeletal animation with proper joint bending
pub fn apply_procedural_limb_animation(
    time: Res<Time>,
    mut character_query: Query<(
        &LocomotionController,
        &mut ProceduralAnimation,
        &CharacterBody,
    ), With<CharacterRoot>>,
    mut limb_query: Query<&mut Transform, With<CharacterLimb>>,
) {
    let elapsed = time.elapsed_secs();
    let delta = time.delta_secs();
    
    for (locomotion, mut procedural, body) in character_query.iter_mut() {
        procedural.update(delta);
        
        let is_moving = locomotion.speed > 0.1;
        let is_airborne = !locomotion.grounded;
        let is_jumping = is_airborne && locomotion.vertical_velocity > 0.5;
        let is_falling = is_airborne && locomotion.vertical_velocity < -0.5;
        let speed_factor = locomotion.speed.clamp(0.0, 2.0);
        
        let walk_frequency = 8.0 + speed_factor * 4.0;
        let walk_cycle = elapsed * walk_frequency;
        
        // SPINE ANIMATION
        if let Ok(mut transform) = limb_query.get_mut(body.spine) {
            let breath = procedural.get_breathing_offset();
            let lean = if is_jumping {
                0.15
            } else if is_falling {
                -0.1
            } else if is_moving {
                0.08 * speed_factor
            } else {
                breath * 0.02
            };
            transform.rotation = Quat::from_rotation_x(lean);
        }
        
        if let Ok(mut transform) = limb_query.get_mut(body.chest) {
            if is_airborne {
                transform.rotation = Quat::from_rotation_x(-0.05);
            } else if is_moving {
                let twist = (walk_cycle).sin() * 0.08 * speed_factor;
                transform.rotation = Quat::from_rotation_y(twist);
            } else {
                transform.rotation = Quat::IDENTITY;
            }
        }
        
        // ARM ANIMATION
        animate_arm(
            &mut limb_query, body.left_upper_arm, body.left_lower_arm, body.left_hand,
            walk_cycle, speed_factor, is_moving, is_airborne, is_jumping, true, elapsed,
        );
        animate_arm(
            &mut limb_query, body.right_upper_arm, body.right_lower_arm, body.right_hand,
            walk_cycle, speed_factor, is_moving, is_airborne, is_jumping, false, elapsed,
        );
        
        // LEG ANIMATION
        animate_leg(
            &mut limb_query, body.left_upper_leg, body.left_lower_leg, body.left_foot,
            walk_cycle, speed_factor, is_moving, is_airborne, is_jumping, locomotion.air_time, true,
        );
        animate_leg(
            &mut limb_query, body.right_upper_leg, body.right_lower_leg, body.right_foot,
            walk_cycle, speed_factor, is_moving, is_airborne, is_jumping, locomotion.air_time, false,
        );
    }
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
