//! PlayerService Plugin - Client-side player systems
//! 
//! Uses shared types from eustress_common::services::player
//! Implements client-specific systems:
//! - Character spawning with physics (Avian3D)
//! - Procedural animation (walk, run, jump)
//! - Camera following with smooth interpolation
//! - Input handling (WASD + mouse)
//!
//! ## Animation System
//! 
//! Inspired by AAA games like Uncharted 4 and GTA V:
//! - Procedural limb animation based on velocity
//! - State machine for animation transitions
//! - Hip bob and body lean during movement
//! - Smooth blending between states

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::mesh::{Indices, PrimitiveTopology};
use avian3d::prelude::*;

// Import shared types from common
#[allow(unused_imports)]
pub use eustress_common::services::player::{
    Player, Character, CharacterRoot, CharacterHead,
    PlayerService, PlayerCamera, CameraMode,
    find_spawn_position, get_spawn_position_or_default,
    find_spawn_position_by_team_id, get_spawn_position_by_team_id_or_default,
};
pub use eustress_common::classes::SpawnLocation;
pub use eustress_common::services::{TeamService, TeamMember, TeamColor};
#[allow(unused_imports)]
pub use eustress_common::services::animation::{
    AnimationService, AnimationStateMachine, AnimationState,
    LocomotionController, ProceduralAnimation, FootIK,
    CharacterAnimationBundle,
};

// Re-export character controller components
#[allow(unused_imports)]
pub use super::character_controller::{
    CharacterPhysics, MovementIntent, CharacterBody, CharacterLimb, CharacterFacing,
};

// Import shared character markers from common (for 1:1 parity with Play Mode)
pub use eustress_common::plugins::character_plugin::{
    PlayModeCharacter, PlayModeCamera,
};

// Import skinned character system
use eustress_common::plugins::skinned_character::{
    spawn_skinned_character, CharacterModel, CharacterGender,
    CharacterAnimationPaths, SkinnedCharacter,
};
use eustress_common::services::player::{BiologicalSex, PlayerProfile};

// ============================================================================
// Character Type Configuration
// ============================================================================

/// Configuration for which character system to use
#[derive(Resource, Default)]
pub struct CharacterSystemConfig {
    /// Use skinned GLB characters instead of procedural primitives
    pub use_skinned_characters: bool,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct PlayerServicePlugin;

impl Plugin for PlayerServicePlugin {
    fn build(&self, app: &mut App) {
        // Use the SHARED character plugin from common crate
        // This ensures 1:1 parity between Client and Engine Play Mode
        use eustress_common::plugins::character_plugin::SharedCharacterPlugin;
        
        app
            // Add the shared character plugin - provides all movement, camera, input, animation systems
            .add_plugins(SharedCharacterPlugin)
            
            // Client-specific resources
            .init_resource::<AnimationService>()
            .insert_resource(CharacterSystemConfig { use_skinned_characters: true })
            
            // Client-specific startup systems
            .add_systems(Startup, (
                spawn_local_player,
                lock_cursor,
            ))
            
            // Client-specific update systems (first-person body hiding)
            .add_systems(Update, update_first_person_mode);
    }
}

// ============================================================================
// Startup Systems
// ============================================================================

/// Create a beveled box mesh with chamfered top/bottom edges
/// Uses a simple approach: regular box with beveled horizontal edges
fn create_beveled_box(width: f32, height: f32, depth: f32, bevel: f32) -> Mesh {
    let hw = width / 2.0;
    let hh = height / 2.0;
    let hd = depth / 2.0;
    let b = bevel.min(hw.min(hh.min(hd)) * 0.4);
    
    // Positions: 16 vertices for beveled box
    // Top ring (4 verts at y = hh, inset by bevel)
    // Upper ring (4 verts at y = hh - b, full width)  
    // Lower ring (4 verts at y = -hh + b, full width)
    // Bottom ring (4 verts at y = -hh, inset by bevel)
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
    
    // Simple normals (will be approximate but look fine)
    let n_up: [f32; 3] = [0.0, 1.0, 0.0];
    let n_down: [f32; 3] = [0.0, -1.0, 0.0];
    let n_bevel_up: [f32; 3] = [0.0, 0.707, 0.0];  // Approximate
    let n_bevel_down: [f32; 3] = [0.0, -0.707, 0.0];
    
    let normals: Vec<[f32; 3]> = vec![
        n_up, n_up, n_up, n_up,  // top ring
        n_bevel_up, n_bevel_up, n_bevel_up, n_bevel_up,  // upper ring
        n_bevel_down, n_bevel_down, n_bevel_down, n_bevel_down,  // lower ring
        n_down, n_down, n_down, n_down,  // bottom ring
    ];
    
    let uvs: Vec<[f32; 2]> = vec![
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
    ];
    
    let indices: Vec<u32> = vec![
        // Top face (CCW when viewed from above)
        0, 2, 1, 0, 3, 2,
        // Top bevel (connects top ring to upper ring) - flip winding
        0, 1, 5, 0, 5, 4,  // back
        1, 2, 6, 1, 6, 5,  // right
        2, 3, 7, 2, 7, 6,  // front
        3, 0, 4, 3, 4, 7,  // left
        // Middle sides (connects upper ring to lower ring) - flip winding
        4, 5, 9, 4, 9, 8,  // back
        5, 6, 10, 5, 10, 9,  // right
        6, 7, 11, 6, 11, 10,  // front
        7, 4, 8, 7, 8, 11,  // left
        // Bottom bevel (connects lower ring to bottom ring) - flip winding
        8, 9, 13, 8, 13, 12,  // back
        9, 10, 14, 9, 14, 13,  // right
        10, 11, 15, 10, 15, 14,  // front
        11, 8, 12, 11, 12, 15,  // left
        // Bottom face (CCW when viewed from below)
        12, 13, 14, 12, 14, 15,
    ];
    
    Mesh::new(PrimitiveTopology::TriangleList, default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Spawn the local player with physics-enabled character
fn spawn_local_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut player_service: ResMut<PlayerService>,
    spawn_locations: Query<(&Transform, &SpawnLocation)>,
    config: Res<CharacterSystemConfig>,
) {
    info!("🎮 PlayerService: Spawning local player...");
    
    // Find spawn position from SpawnLocation entities, or use default
    let (spawn_pos, spawn_protection) = get_spawn_position_or_default(
        spawn_locations.iter(),
        None, // TODO: Get player team when team system is implemented
        player_service.spawn_position,
    );
    
    if spawn_protection > 0.0 {
        info!("🛡️ Spawn protection: {:.1}s", spawn_protection);
    }
    
    info!("📍 Spawning at position: {:?}", spawn_pos);
    
    // Check if we should use skinned GLB characters
    if config.use_skinned_characters {
        spawn_skinned_local_player(
            &mut commands,
            &asset_server,
            &mut player_service,
            spawn_pos,
        );
        return;
    }
    
    // Otherwise use procedural primitive character (legacy)
    
    // =========================================================================
    // REALISTIC HUMANOID SKELETON
    // Based on average human proportions - SCALABLE
    // All bones pivot from their TOP (joint location)
    // =========================================================================
    
    // CHARACTER SCALE - Change this to scale the entire character
    let character_scale = 1.0;  // 1.0 = normal human, 0.5 = half size, 2.0 = double
    
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
    let upper_leg_length = 0.40 * character_scale;  // Thigh
    let lower_leg_length = 0.40 * character_scale;  // Shin
    let foot_height = 0.08 * character_scale;
    
    // Calculate hip height from leg lengths (legs attach 0.08m below hip center)
    let hip_joint_offset = 0.08 * character_scale;
    let hip_height_from_ground = hip_joint_offset + upper_leg_length + lower_leg_length + foot_height;
    
    // Widths scale with character
    let shoulder_width = 0.48 * character_scale;  // Total shoulder span (wider)
    let hip_width = 0.28 * character_scale;       // Total hip span
    let chest_width = 0.36 * character_scale;     // Slightly wider chest
    let chest_depth = 0.22 * character_scale;     // Slightly deeper chest
    let head_radius = 0.13 * character_scale;     // Bigger head
    let neck_radius = 0.055 * character_scale;    // Slightly thicker neck
    let upper_arm_radius = 0.04 * character_scale;
    let lower_arm_radius = 0.035 * character_scale;
    let hand_width = 0.08 * character_scale;
    let upper_leg_radius = 0.07 * character_scale;
    let lower_leg_radius = 0.05 * character_scale;
    let foot_length = 0.25 * character_scale;
    let foot_width = 0.09 * character_scale;
    
    // Physics capsule - sized to match FULL character height
    // Capsule total height = 2 * half_height + 2 * radius (cylinder + 2 hemispheres)
    // For 1.75m character: we want capsule to cover from feet to top of head
    // Using radius = shoulder_width/2 for body width, calculate half_height from remaining
    let capsule_radius = (shoulder_width / 2.0_f32).max(0.2 * character_scale);  // At least 0.2m radius
    let capsule_total_height = total_height;
    let capsule_half_height = (capsule_total_height / 2.0) - capsule_radius;  // Cylinder half-height
    
    // Eye height for camera (roughly 94% of total height)
    let _eye_height = total_height * 0.94;
    
    // Materials
    let skin_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.70, 0.55),
        perceptual_roughness: 0.7,
        ..default()
    });
    let shirt_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.4, 0.7),
        perceptual_roughness: 0.8,
        ..default()
    });
    let pants_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.2),
        perceptual_roughness: 0.9,
        ..default()
    });
    let shoe_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.1, 0.1),
        perceptual_roughness: 0.95,
        ..default()
    });
    
    // Spawn physics root
    // Capsule center from ground = radius + half_height
    // This ensures the capsule bottom touches the ground
    let capsule_center_height = capsule_radius + capsule_half_height;
    // Spawn higher to avoid any ground penetration - will fall to ground naturally
    let spawn_height = capsule_center_height + 0.5;
    
    info!("Capsule: radius={:.2}, half_height={:.2}, center_from_ground={:.2}, spawn_height={:.2}", 
          capsule_radius, capsule_half_height, capsule_center_height, spawn_height);
    let character_entity = commands.spawn((
        Transform::from_translation(spawn_pos + Vec3::Y * spawn_height),
        Visibility::default(),
        RigidBody::Dynamic,
        Collider::capsule(capsule_radius, capsule_half_height),
        CollisionMargin(0.02),  // Small margin to prevent penetration
        LockedAxes::ROTATION_LOCKED,
        Friction::new(1.0),  // Good friction for ground contact
        Restitution::new(0.0),
        GravityScale(1.0),  // Use realistic gravity (9.80665 m/s²)
        LinearVelocity::default(),
        // CCD for fast-moving character to prevent tunneling
        SweptCcd::default(),
        Name::new("LocalCharacter"),
    )).id();
    
    commands.entity(character_entity).insert((
        Character::default(),
        CharacterRoot,
        AnimationStateMachine::default(),
        LocomotionController::default(),
        ProceduralAnimation::default(),
        CharacterFacing::default(),
        CharacterPhysics::default(),
        MovementIntent::default(),
        PlayModeCharacter, // Marker for shared systems from common crate
    ));
    
    // Spawn player entity
    let player_entity = commands.spawn((
        Player {
            name: "LocalPlayer".to_string(),
            user_id: 1,
            is_local: true,
            ..default()
        },
        Name::new("LocalPlayer"),
    )).id();
    
    // Visual offset from capsule center to hip pivot
    // The capsule center is at the physics body's transform.translation
    // Capsule bottom is at: center - capsule_center_height (relative to center)
    // 
    // When standing on ground, capsule bottom = ground level
    // So relative to capsule center:
    // - Ground is at: -capsule_center_height
    // - Hips should be at: hip_height_from_ground - capsule_center_height
    // 
    // BUT: We also added spawn_height margin, so we need to account for that
    // The skeleton should be positioned so feet are at capsule bottom
    // Feet are at: hips_y - hip_joint_offset - upper_leg - lower_leg - foot_height
    // = visual_y_offset - hip_joint_offset - upper_leg - lower_leg - foot_height
    // This should equal -capsule_center_height (capsule bottom)
    // So: visual_y_offset = -capsule_center_height + hip_joint_offset + upper_leg + lower_leg + foot_height
    //                     = -capsule_center_height + hip_height_from_ground
    //                     = hip_height_from_ground - capsule_center_height
    // Physics engines often allow slight penetration due to collision margins and settling.
    // To prevent feet from visually clipping into ground, we raise the visual mesh.
    // Use enough clearance to account for collision margin (0.02) and any physics settling
    let ground_clearance = 0.5;  // Clearance to keep feet above ground
    let visual_y_offset = hip_height_from_ground - capsule_center_height + ground_clearance;
    
    // =========================================================================
    // SPINE CHAIN: Hips -> Spine -> Chest -> Neck -> Head
    // Each bone's transform is relative to its PARENT
    // Bones pivot from their TOP (where they connect to parent)
    // =========================================================================
    
    // HIPS - root of skeleton, child of physics capsule
    // Pivot is at hip center, mesh extends down slightly
    let hips = commands.spawn((
        Transform::from_xyz(0.0, visual_y_offset, 0.0),
        Visibility::default(),
        ChildOf(character_entity),
        CharacterLimb::Hips,
        Name::new("Hips"),
    )).id();
    // Hip mesh (pelvis area) - beveled for softer look
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(hip_width, 0.15, chest_depth * 0.9, 0.02))),
        MeshMaterial3d(pants_mat.clone()),
        Transform::from_xyz(0.0, -0.075, 0.0),
        ChildOf(hips),
        Name::new("HipsMesh"),
    ));
    
    // SPINE - connects hips to chest
    let spine = commands.spawn((
        Transform::from_xyz(0.0, 0.0, 0.0),  // Pivot at top of hips
        Visibility::default(),
        ChildOf(hips),
        CharacterLimb::Spine,
        Name::new("Spine"),
    )).id();
    // Spine/waist mesh - beveled
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(chest_width * 0.8, spine_height, chest_depth * 0.8, 0.015))),
        MeshMaterial3d(shirt_mat.clone()),
        Transform::from_xyz(0.0, spine_height / 2.0, 0.0),
        ChildOf(spine),
        Name::new("SpineMesh"),
    ));
    
    // CHEST - main upper body
    let chest = commands.spawn((
        Transform::from_xyz(0.0, spine_height, 0.0),  // Pivot at top of spine
        Visibility::default(),
        ChildOf(spine),
        CharacterLimb::Chest,
        Name::new("Chest"),
    )).id();
    // Chest mesh - beveled for softer torso
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(chest_width, chest_height, chest_depth, 0.025))),
        MeshMaterial3d(shirt_mat.clone()),
        Transform::from_xyz(0.0, chest_height / 2.0, 0.0),
        ChildOf(chest),
        Name::new("ChestMesh"),
    ));
    
    // NECK
    let neck = commands.spawn((
        Transform::from_xyz(0.0, chest_height, 0.0),  // Pivot at top of chest
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
        Transform::from_xyz(0.0, neck_length, 0.0),  // Pivot at top of neck
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
    
    // SMILEY FACE DECAL - on front of head
    let face_z = head_radius * 0.95;  // Slightly in front of head surface
    let face_y = head_radius;  // Center of head
    
    // Eye material (black)
    let eye_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.1, 0.1),
        unlit: true,
        ..default()
    });
    
    // Left eye
    let eye_radius = head_radius * 0.08;
    let eye_spacing = head_radius * 0.25;
    let eye_height = head_radius * 0.15;
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
    
    // Smile - curved arc, lower on face, black color
    let smile_radius = head_radius * 0.035;  // Slightly thicker
    let smile_curve_radius = head_radius * 0.3;  // Wider smile
    let smile_y_offset = -head_radius * 0.25;  // Lower on face
    for i in 0..9 {  // More segments for smoother curve
        let angle = std::f32::consts::PI * 0.2 + (i as f32 / 8.0) * std::f32::consts::PI * 0.6;
        let x = angle.cos() * smile_curve_radius;
        let y = face_y + smile_y_offset - angle.sin() * smile_curve_radius * 0.4;
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(smile_radius))),
            MeshMaterial3d(eye_mat.clone()),  // Use black eye material
            Transform::from_xyz(x, y, face_z),
            ChildOf(head),
            Name::new(format!("Smile{}", i)),
        ));
    }
    
    // =========================================================================
    // ARM CHAINS (from chest): Shoulder -> UpperArm -> LowerArm -> Hand
    // =========================================================================
    
    let shoulder_y = chest_height - 0.03;  // Slightly below top of chest
    let shoulder_x = chest_width / 2.0 + 0.02;
    
    // --- LEFT ARM ---
    let left_shoulder = commands.spawn((
        Transform::from_xyz(-shoulder_x, shoulder_y, 0.0),
        Visibility::default(),
        ChildOf(chest),
        CharacterLimb::LeftShoulder,
        Name::new("LeftShoulder"),
    )).id();
    
    // Left Upper Arm - pivots at shoulder, hangs down
    let left_upper_arm = commands.spawn((
        Transform::IDENTITY,  // Pivot at shoulder
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
    
    // Left Lower Arm (forearm) - pivots at elbow
    let left_lower_arm = commands.spawn((
        Transform::from_xyz(0.0, -upper_arm_length, 0.0),  // Pivot at elbow
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
    
    // Left Hand - pivots at wrist, rotated so palm faces inward (toward body)
    let left_hand = commands.spawn((
        Transform::from_xyz(0.0, -lower_arm_length, 0.0)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2)),  // Palm faces right (inward)
        Visibility::default(),
        ChildOf(left_lower_arm),
        CharacterLimb::LeftHand,
        Name::new("LeftHand"),
    )).id();
    
    // Hand palm dimensions
    let palm_width = hand_width * 0.9;
    let palm_length = hand_length * 0.55;
    let palm_depth = 0.025 * character_scale;
    
    // Palm mesh
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(palm_width, palm_length, palm_depth, 0.008))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, -palm_length / 2.0, 0.0),
        ChildOf(left_hand),
        Name::new("LeftPalmMesh"),
    ));
    
    // Finger dimensions (proportionate to hand)
    let finger_radius = 0.012 * character_scale;
    let finger_length = hand_length * 0.5;  // Fingers are about half hand length
    let finger_spacing = palm_width / 5.0;  // Space fingers across palm
    
    // Four fingers (index, middle, ring, pinky)
    let finger_names = ["Index", "Middle", "Ring", "Pinky"];
    let finger_lengths = [0.95, 1.0, 0.95, 0.8];  // Relative lengths
    for (i, (name, len_mult)) in finger_names.iter().zip(finger_lengths.iter()).enumerate() {
        let x_offset = (i as f32 - 1.5) * finger_spacing;  // Center the 4 fingers
        let this_finger_len = finger_length * len_mult;
        commands.spawn((
            Mesh3d(meshes.add(Capsule3d::new(finger_radius, this_finger_len - finger_radius * 2.0))),
            MeshMaterial3d(skin_mat.clone()),
            Transform::from_xyz(x_offset, -palm_length - this_finger_len / 2.0, 0.0),
            ChildOf(left_hand),
            Name::new(format!("Left{}Finger", name)),
        ));
    }
    
    // Thumb - positioned on side, angled outward
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
    
    // Right Hand - rotated so palm faces inward (toward body)
    let right_hand = commands.spawn((
        Transform::from_xyz(0.0, -lower_arm_length, 0.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),  // Palm faces left (inward)
        Visibility::default(),
        ChildOf(right_lower_arm),
        CharacterLimb::RightHand,
        Name::new("RightHand"),
    )).id();
    
    // Right palm mesh
    commands.spawn((
        Mesh3d(meshes.add(create_beveled_box(palm_width, palm_length, palm_depth, 0.008))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(0.0, -palm_length / 2.0, 0.0),
        ChildOf(right_hand),
        Name::new("RightPalmMesh"),
    ));
    
    // Right hand fingers
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
    
    // Right thumb - mirrored position (positive X)
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(finger_radius * 1.1, thumb_length - finger_radius * 2.0))),
        MeshMaterial3d(skin_mat.clone()),
        Transform::from_xyz(palm_width / 2.0 + finger_radius, -palm_length * 0.3, palm_depth * 0.5)
            .with_rotation(Quat::from_rotation_z(-0.5) * Quat::from_rotation_x(0.2)),
        ChildOf(right_hand),
        Name::new("RightThumb"),
    ));
    
    // =========================================================================
    // LEG CHAINS (from hips): UpperLeg -> LowerLeg -> Foot
    // =========================================================================
    
    let hip_joint_x = hip_width / 2.0 - 0.02 * character_scale;
    let hip_joint_y = -hip_joint_offset;  // Below hip center
    
    // --- LEFT LEG ---
    let left_upper_leg = commands.spawn((
        Transform::from_xyz(-hip_joint_x, hip_joint_y, 0.0),  // Pivot at hip joint
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
    
    // Left Lower Leg (shin) - pivots at knee
    let left_lower_leg = commands.spawn((
        Transform::from_xyz(0.0, -upper_leg_length, 0.0),  // Pivot at knee
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
    
    // Left Foot - pivots at ankle
    let left_foot = commands.spawn((
        Transform::from_xyz(0.0, -lower_leg_length, 0.0),  // Pivot at ankle
        Visibility::default(),
        ChildOf(left_lower_leg),
        CharacterLimb::LeftFoot,
        Name::new("LeftFoot"),
    )).id();
    // Left foot mesh - beveled shoe
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
    // Right foot mesh - beveled shoe
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
    
    // Spawn camera with proper tonemapping (avoid magenta bug)
    let _camera_entity = commands.spawn((
        Camera3d::default(),
        Camera::default(),
        // Use Reinhard tonemapping - works without LUT textures
        // TonyMcMapface requires tonemapping_luts feature which may not be enabled
        Tonemapping::Reinhard,
        Transform::from_translation(spawn_pos + Vec3::new(0.0, 5.0, 10.0))
            .looking_at(spawn_pos, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            fov: 70.0_f32.to_radians(),
            ..default()
        }),
        PlayerCamera {
            target: Some(character_entity),
            ..default()
        },
        PlayModeCamera, // Marker for shared systems from common crate
        Name::new("PlayerCamera"),
    )).id();
    
    player_service.local_player = Some(player_entity);
    
    info!("✅ Local player spawned at {:?}", spawn_pos);
    info!("🎮 Controls: WASD=Move, SHIFT=Sprint, SPACE=Jump, Mouse=Look");
    info!("   Click window to focus, ESC to unfocus");
}

/// Spawn local player using skinned GLB character model
fn spawn_skinned_local_player(
    commands: &mut Commands,
    asset_server: &AssetServer,
    player_service: &mut PlayerService,
    spawn_pos: Vec3,
) {
    info!("🎭 Spawning skinned GLB character...");
    
    // TODO: Get biological sex from player profile
    // For now, default to Male (X-Bot)
    let biological_sex = BiologicalSex::Male;
    let model = biological_sex.character_model();
    let gender = biological_sex.character_gender();
    
    info!("📦 Loading character model: {:?} with {:?} animations", model, gender);
    
    // Spawn the skinned character using the common module
    let character_entity = spawn_skinned_character(
        commands,
        asset_server,
        spawn_pos,
        model,
        gender,
    );
    
    // Add additional components for player control and shared systems
    commands.entity(character_entity).insert((
        CharacterFacing::default(),
        CharacterPhysics::default(),
        MovementIntent::default(),
        PlayModeCharacter, // Marker for shared systems from common crate
    ));
    
    // Spawn player entity
    let player_entity = commands.spawn((
        Player {
            name: "LocalPlayer".to_string(),
            user_id: 1,
            is_local: true,
            ..default()
        },
        Name::new("LocalPlayer"),
    )).id();
    
    // Spawn camera with proper tonemapping (avoid magenta bug)
    // Use Reinhard tonemapping - works without LUT textures
    // TonyMcMapface requires tonemapping_luts feature which may not be enabled
    commands.spawn((
        Camera3d::default(),
        Camera::default(),
        Tonemapping::Reinhard,
        Transform::from_translation(spawn_pos + Vec3::new(0.0, 5.0, 10.0))
            .looking_at(spawn_pos, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            fov: 70.0_f32.to_radians(),
            ..default()
        }),
        PlayerCamera {
            target: Some(character_entity),
            distance: 5.0,
            pitch: -15.0_f32.to_radians(),
            yaw: 0.0,
            ..default()
        },
        PlayModeCamera, // Marker for shared systems from common crate
        Name::new("PlayerCamera"),
    ));
    
    player_service.local_player = Some(player_entity);
    
    info!("✅ Skinned character spawned at {:?}", spawn_pos);
    info!("🎮 Controls: WASD=Move, SHIFT=Sprint, SPACE=Jump, Mouse=Look");
}

/// Start with cursor unlocked - free mouse movement
fn lock_cursor(
    mut cursor_options: Query<&mut CursorOptions, With<Window>>,
    mut player_service: ResMut<PlayerService>,
) {
    if let Ok(mut cursor) = cursor_options.single_mut() {
        // Start unlocked - free mouse, right-click to orbit
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        player_service.cursor_locked = false;  // Not used for orbit anymore
    }
    info!("🎮 Controls: WASD=Move, SHIFT=Sprint, SPACE=Jump");
    info!("   Right-click + drag = Orbit camera, Scroll = Zoom");
}

// ============================================================================
// Update Systems (LEGACY - now provided by SharedCharacterPlugin)
// These are kept for reference but marked dead_code since SharedCharacterPlugin
// provides identical systems for 1:1 parity with Play Mode.
// ============================================================================

/// Handle cursor lock based on camera mode
/// - First person: cursor locked and centered (FPS style)
/// - Third person: cursor free, right-click to orbit
#[allow(dead_code)]
fn toggle_cursor_lock(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut player_service: ResMut<PlayerService>,
    mut cursor_options: Query<&mut CursorOptions, With<Window>>,
    camera_query: Query<&PlayerCamera>,
    window_query: Query<&Window>,
) {
    let Ok(window) = window_query.single() else { return };
    if !window.focused { return };
    let Ok(camera) = camera_query.single() else { return };
    
    if camera.is_first_person {
        // First person: always lock cursor for free look
        if !player_service.cursor_locked {
            player_service.cursor_locked = true;
            if let Ok(mut cursor) = cursor_options.single_mut() {
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            }
        }
        
        // ESC to unlock cursor temporarily in first person
        if keys.just_pressed(KeyCode::Escape) {
            player_service.cursor_locked = false;
            if let Ok(mut cursor) = cursor_options.single_mut() {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            }
        }
        
        // Click to re-lock in first person
        if mouse.just_pressed(MouseButton::Left) && !player_service.cursor_locked {
            player_service.cursor_locked = true;
            if let Ok(mut cursor) = cursor_options.single_mut() {
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            }
        }
    } else {
        // Third person: right-click to orbit
        if mouse.just_pressed(MouseButton::Right) {
            player_service.cursor_locked = true;
        }
        
        if mouse.just_released(MouseButton::Right) {
            player_service.cursor_locked = false;
        }
    }
    
    // TAB to toggle camera mode (handled elsewhere, but also toggle lock)
    if keys.just_pressed(KeyCode::Tab) {
        // Toggle will be handled by update_first_person_mode
        // Just ensure cursor state matches new mode
    }
}

/// Handle mouse look for camera
/// - First person: always active (cursor locked)
/// - Third person: only when right-click held
#[allow(dead_code)]
fn camera_mouse_look(
    mut mouse_motion: MessageReader<MouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    player_service: Res<PlayerService>,
    mut camera_query: Query<&mut PlayerCamera>,
) {
    let Ok(camera) = camera_query.single() else { 
        mouse_motion.clear();
        return; 
    };
    
    // First person: always look. Third person: right-click to orbit
    let can_look = if camera.is_first_person {
        player_service.cursor_locked
    } else {
        mouse.pressed(MouseButton::Right)
    };
    
    if !can_look {
        mouse_motion.clear();
        return;
    }
    
    let mut delta = Vec2::ZERO;
    for event in mouse_motion.read() {
        delta += event.delta;
    }
    
    if delta == Vec2::ZERO {
        return;
    }
    
    for mut camera in camera_query.iter_mut() {
        camera.yaw -= delta.x * camera.sensitivity;
        camera.pitch += delta.y * camera.sensitivity;  // mouse up = look up
        camera.pitch = camera.pitch.clamp(camera.pitch_min, camera.pitch_max);
    }
}

/// Handle mouse wheel zoom - always active
/// Switching to first-person locks cursor, switching to third-person unlocks
#[allow(dead_code)]
fn camera_zoom(
    mut scroll_events: MessageReader<MouseWheel>,
    mut camera_query: Query<&mut PlayerCamera>,
    mut player_service: ResMut<PlayerService>,
    mut cursor_options: Query<&mut CursorOptions, With<Window>>,
) {
    // Zoom always works, no need for cursor lock
    
    let mut scroll_delta = 0.0;
    for event in scroll_events.read() {
        scroll_delta += event.y;
    }
    
    if scroll_delta == 0.0 {
        return;
    }
    
    for mut camera in camera_query.iter_mut() {
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
}

/// Update first-person mode - hide entire body locally when in first person
fn update_first_person_mode(
    camera_query: Query<&PlayerCamera, Changed<PlayerCamera>>,
    character_query: Query<&CharacterBody, With<CharacterRoot>>,
    mut visibility_query: Query<&mut Visibility>,
) {
    let Ok(camera) = camera_query.single() else { return };
    let Ok(body) = character_query.single() else { return };
    
    let vis = if camera.is_first_person {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    
    // Hide/show entire body in first person mode
    // Start from hips - this will hide the entire skeleton since all parts are children
    if let Ok(mut hips_vis) = visibility_query.get_mut(body.hips) {
        *hips_vis = vis;
    }
}

/// Handle WASD movement with physics - always active
#[allow(dead_code)]
fn character_movement(
    keys: Res<ButtonInput<KeyCode>>,
    camera_query: Query<&PlayerCamera>,
    mut character_query: Query<(&mut LinearVelocity, &Character), With<CharacterRoot>>,
) {
    // Movement always works - no cursor lock required
    let Ok(camera) = camera_query.single() else { return };
    let Ok((mut velocity, character)) = character_query.single_mut() else { return };
    
    // Get input
    let mut input = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { input.z -= 1.0; }
    if keys.pressed(KeyCode::KeyS) { input.z += 1.0; }
    if keys.pressed(KeyCode::KeyA) { input.x -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { input.x += 1.0; }
    
    if input == Vec3::ZERO {
        // Apply friction when not moving
        velocity.x *= 0.9;
        velocity.z *= 0.9;
        return;
    }
    
    input = input.normalize();
    
    // Calculate movement direction based on camera yaw
    let forward = Vec3::new(-camera.yaw.sin(), 0.0, -camera.yaw.cos());
    let right = Vec3::new(camera.yaw.cos(), 0.0, -camera.yaw.sin());
    let movement = forward * -input.z + right * input.x;
    
    // Apply speed
    let mut speed = character.walk_speed;
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        speed *= character.sprint_multiplier;
    }
    
    // Set horizontal velocity (preserve vertical for gravity/jumping)
    velocity.x = movement.x * speed;
    velocity.z = movement.z * speed;
}

/// Handle jumping - always active
#[allow(dead_code)]
fn character_jump(
    keys: Res<ButtonInput<KeyCode>>,
    mut character_query: Query<(&mut LinearVelocity, &Character), With<CharacterRoot>>,
) {
    // Jump always works - no cursor lock required
    if keys.just_pressed(KeyCode::Space) {
        for (mut velocity, character) in character_query.iter_mut() {
            if character.grounded {
                velocity.y = character.jump_power;
            }
        }
    }
}

/// Check if character is on ground using raycast
#[allow(dead_code)]
fn ground_check(
    spatial_query: SpatialQuery,
    mut character_query: Query<(Entity, &Transform, &mut Character), With<CharacterRoot>>,
) {
    // Character dimensions (should match skinned_character.rs)
    let character_height = 1.83;
    let capsule_radius = 0.33;
    let capsule_half_height = (character_height / 2.0) - capsule_radius;
    // Distance from capsule center to bottom
    let capsule_center_to_feet = capsule_half_height + capsule_radius;
    
    for (entity, transform, mut character) in character_query.iter_mut() {
        let ray_origin = transform.translation;
        let ray_dir = Dir3::NEG_Y;
        // Ray from center to slightly below feet (with small tolerance)
        let max_dist = capsule_center_to_feet + 0.1;
        
        // Use shape cast filter to exclude self
        let filter = SpatialQueryFilter::default().with_excluded_entities([entity]);
        
        character.grounded = spatial_query
            .cast_ray(ray_origin, ray_dir, max_dist, true, &filter)
            .is_some();
    }
}

/// Update locomotion controller from velocity
#[allow(dead_code)]
fn update_locomotion(
    time: Res<Time>,
    camera_query: Query<&PlayerCamera>,
    mut character_query: Query<(
        &LinearVelocity,
        &Character,
        &mut LocomotionController,
        &mut CharacterFacing,
    ), With<CharacterRoot>>,
) {
    let delta = time.delta_secs();
    let Ok(camera) = camera_query.single() else { return };
    
    for (velocity, character, mut locomotion, mut facing) in character_query.iter_mut() {
        let horizontal_vel = Vec3::new(velocity.x, 0.0, velocity.z);
        
        // Calculate forward direction from camera
        let forward = Vec3::new(-camera.yaw.sin(), 0.0, -camera.yaw.cos());
        
        // Update locomotion from velocity
        locomotion.update_from_velocity(velocity.0, forward, character.grounded, delta);
        
        // Update facing direction based on movement
        if horizontal_vel.length_squared() > 0.5 {
            // Face movement direction
            let move_dir = horizontal_vel.normalize();
            facing.target_angle = move_dir.x.atan2(move_dir.z);
        }
        
        // Update head look target based on camera and character orientation
        // head_yaw_offset = how much the camera is rotated relative to body
        // Small offset = character facing same direction as camera (back to camera)
        // Large offset (~PI) = character facing opposite to camera (face to camera)
        let head_yaw_offset = angle_difference(camera.yaw, facing.angle);
        let abs_offset = head_yaw_offset.abs();
        
        // When abs_offset < PI/2: character's back is toward camera (facing away)
        //   -> Head should turn to look in camera direction (same as camera)
        //   -> head_look_target.x = head_yaw_offset (turn head to match camera)
        //
        // When abs_offset > PI/2: character's face is toward camera (facing camera)
        //   -> Head should look AT the camera (toward the viewer)
        //   -> Need to turn head back toward camera, which is ~PI from body forward
        
        let facing_away_from_camera = abs_offset < std::f32::consts::FRAC_PI_2;
        
        if facing_away_from_camera {
            // Back to camera - head looks where camera looks (forward)
            // head_yaw_offset tells us how much to turn head relative to body
            facing.head_look_target.x = head_yaw_offset.clamp(-1.2, 1.2);
        } else {
            // Face toward camera - head looks AT camera (back over shoulder toward viewer)
            // We need to turn head toward camera, which is behind the character
            // The offset to look back at camera is (PI - abs_offset), with correct sign
            let look_back = if head_yaw_offset > 0.0 {
                std::f32::consts::PI - head_yaw_offset
            } else {
                -std::f32::consts::PI - head_yaw_offset
            };
            facing.head_look_target.x = look_back.clamp(-1.2, 1.2);
        }
        facing.head_look_target.y = camera.pitch.clamp(-0.7, 0.7); // ~40 degrees
    }
}

/// Helper to get shortest angle difference
#[allow(dead_code)]
fn angle_difference(a: f32, b: f32) -> f32 {
    let diff = (a - b) % std::f32::consts::TAU;
    if diff > std::f32::consts::PI {
        diff - std::f32::consts::TAU
    } else if diff < -std::f32::consts::PI {
        diff + std::f32::consts::TAU
    } else {
        diff
    }
}

/// Update character facing - rotates the HIPS which propagates to all children
#[allow(dead_code)]
fn update_character_facing(
    time: Res<Time>,
    mut query: Query<(&mut CharacterFacing, &CharacterBody)>,
    mut limb_query: Query<&mut Transform, With<CharacterLimb>>,
) {
    let delta = time.delta_secs();
    
    for (mut facing, body) in query.iter_mut() {
        // Smoothly interpolate facing angle
        let angle_diff = angle_difference(facing.target_angle, facing.angle);
        facing.angle += angle_diff * facing.turn_speed * delta;
        facing.angle = facing.angle % std::f32::consts::TAU;
        
        // Only rotate HIPS - children inherit rotation through hierarchy
        if let Ok(mut transform) = limb_query.get_mut(body.hips) {
            transform.rotation = Quat::from_rotation_y(facing.angle);
        }
    }
}

/// Update head look (follows camera within neck limits)
/// Head rotates left/right (yaw) to follow camera direction
/// Limited to ~70 degrees each way before body must turn
#[allow(dead_code)]
fn update_head_look(
    time: Res<Time>,
    mut query: Query<(&mut CharacterFacing, &CharacterBody)>,
    mut limb_query: Query<&mut Transform, With<CharacterLimb>>,
) {
    let delta = time.delta_secs();
    
    for (mut facing, body) in query.iter_mut() {
        // Smoothly interpolate head look
        facing.head_look = facing.head_look.lerp(facing.head_look_target, 8.0 * delta);
        
        // Neck rotates for part of the yaw (left/right look) - about 40% of total
        // This gives a natural look where neck and head both contribute
        // Negate the yaw so head turns in the same direction as camera look
        if let Ok(mut transform) = limb_query.get_mut(body.neck) {
            let neck_yaw = -facing.head_look.x * 0.4;
            transform.rotation = Quat::from_rotation_y(neck_yaw);
        }
        
        // Head rotates for remaining yaw (60%) - Pitch removed to prevent "tilt"
        // Yaw = rotation around Y axis (left/right)
        // Negate the yaw so head turns in the same direction as camera look
        if let Ok(mut transform) = limb_query.get_mut(body.head) {
            let head_yaw = -facing.head_look.x * 0.6;  // Remaining 60% of horizontal look
            // Pitch (looking up/down) removed as it was perceived as "tilting"
            
            // Apply only yaw
            transform.rotation = Quat::from_rotation_y(head_yaw);
        }
    }
}

/// Update animation state machine based on locomotion
#[allow(dead_code)]
fn update_animation_state_machine(
    time: Res<Time>,
    mut query: Query<(
        &LocomotionController,
        &mut AnimationStateMachine,
    ), With<CharacterRoot>>,
) {
    let delta = time.delta_secs();
    
    for (locomotion, mut state_machine) in query.iter_mut() {
        state_machine.update(delta);
        let target_state = locomotion.get_animation_state();
        if state_machine.current_state != target_state {
            state_machine.request_transition(target_state);
        }
    }
}

/// Apply procedural skeletal animation with proper joint bending
/// 
/// Key insight: Each joint only rotates locally. The hierarchy propagates transforms.
/// - Upper arm rotates at shoulder
/// - Lower arm rotates at elbow (relative to upper arm)
/// - Upper leg rotates at hip
/// - Lower leg rotates at knee (relative to upper leg)
#[allow(dead_code)]
fn apply_procedural_limb_animation(
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
        
        // Walk cycle speed scales with movement speed, faster = quicker steps
        let walk_frequency = 8.0 + speed_factor * 4.0;  // 8-16 Hz based on speed
        let walk_cycle = elapsed * walk_frequency;
        
        // =====================================================================
        // SPINE ANIMATION
        // =====================================================================
        
        // Spine - forward lean when moving/jumping, breathing when idle
        if let Ok(mut transform) = limb_query.get_mut(body.spine) {
            let breath = procedural.get_breathing_offset();
            let lean = if is_jumping {
                0.15  // Lean forward during jump
            } else if is_falling {
                -0.1  // Lean back slightly when falling
            } else if is_moving {
                0.08 * speed_factor  // More lean at higher speeds
            } else {
                breath * 0.02
            };
            transform.rotation = Quat::from_rotation_x(lean);
        }
        
        // Chest - counter-rotation for natural movement
        if let Ok(mut transform) = limb_query.get_mut(body.chest) {
            if is_airborne {
                // Slight arch back in air
                transform.rotation = Quat::from_rotation_x(-0.05);
            } else if is_moving {
                // Slight twist opposite to arm swing
                let twist = (walk_cycle).sin() * 0.08 * speed_factor;
                transform.rotation = Quat::from_rotation_y(twist);
            } else {
                transform.rotation = Quat::IDENTITY;
            }
        }
        
        // =====================================================================
        // ARM ANIMATION - Proper elbow bending
        // =====================================================================
        
        // Left arm chain
        animate_arm(
            &mut limb_query,
            body.left_upper_arm,
            body.left_lower_arm,
            body.left_hand,
            walk_cycle,
            speed_factor,
            is_moving,
            is_airborne,
            is_jumping,
            true,  // is_left
            elapsed,
        );
        
        // Right arm chain
        animate_arm(
            &mut limb_query,
            body.right_upper_arm,
            body.right_lower_arm,
            body.right_hand,
            walk_cycle,
            speed_factor,
            is_moving,
            is_airborne,
            is_jumping,
            false, // is_left
            elapsed,
        );
        
        // =====================================================================
        // LEG ANIMATION - Proper knee bending
        // =====================================================================
        
        // Left leg chain
        animate_leg(
            &mut limb_query,
            body.left_upper_leg,
            body.left_lower_leg,
            body.left_foot,
            walk_cycle,
            speed_factor,
            is_moving,
            is_airborne,
            is_jumping,
            locomotion.air_time,
            true,  // is_left
        );
        
        // Right leg chain
        animate_leg(
            &mut limb_query,
            body.right_upper_leg,
            body.right_lower_leg,
            body.right_foot,
            walk_cycle,
            speed_factor,
            is_moving,
            is_airborne,
            is_jumping,
            locomotion.air_time,
            false, // is_left
        );
    }
}

/// Helper to animate an arm chain (upper arm -> lower arm -> hand)
#[allow(dead_code)]
fn animate_arm(
    limb_query: &mut Query<&mut Transform, With<CharacterLimb>>,
    upper_arm: Entity,
    lower_arm: Entity,
    hand: Entity,
    walk_cycle: f32,
    speed_factor: f32,
    is_moving: bool,
    is_airborne: bool,
    is_jumping: bool,
    is_left: bool,
    elapsed: f32,
) {
    // Arms swing opposite to legs
    let phase = if is_left { 0.0 } else { std::f32::consts::PI };
    
    if is_airborne {
        // =====================================================================
        // AIRBORNE ARM POSE
        // =====================================================================
        if is_jumping {
            // Arms pump back during jump ascent - athletic motion
            // Negative X = arms swing forward, Positive X = arms swing back
            let arm_back = -0.5;  // Arms swing back (negative pulls upper arm back)
            
            if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
                transform.rotation = Quat::from_rotation_x(arm_back);
            }
            
            // Elbows bent tightly - pumping motion
            // Negative X = forearm bends up toward shoulder
            if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
                transform.rotation = Quat::from_rotation_x(-1.4);
            }
            
            if let Ok(mut transform) = limb_query.get_mut(hand) {
                transform.rotation = Quat::IDENTITY;
            }
        } else {
            // Falling - arms forward and slightly out, ready for landing
            let arm_forward = 0.4;  // Arms slightly forward/down
            
            if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
                transform.rotation = Quat::from_rotation_x(arm_forward);
            }
            
            // Elbows bent, hands ready to brace
            if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
                transform.rotation = Quat::from_rotation_x(-0.6);
            }
            
            if let Ok(mut transform) = limb_query.get_mut(hand) {
                transform.rotation = Quat::IDENTITY;
            }
        }
    } else if is_moving {
        // =====================================================================
        // WALKING/RUNNING ARM SWING
        // =====================================================================
        // Upper arm swings forward/back at shoulder
        let shoulder_swing = (walk_cycle + phase).sin();
        let shoulder_angle = shoulder_swing * 0.6 * speed_factor;  // More swing
        
        if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
            transform.rotation = Quat::from_rotation_x(shoulder_angle);
        }
        
        // Elbow bends more when arm swings back (natural arm motion)
        // Also bends more at higher speeds (running form)
        // Negative X rotation = forearm bends forward/up (correct for elbow)
        let elbow_base = 0.3 * speed_factor;  // Base bend increases with speed
        let elbow_swing = ((-shoulder_swing + 1.0) * 0.5) * 0.5 * speed_factor;
        let elbow_bend = -(elbow_base + elbow_swing);  // Negative to bend correctly
        
        if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
            transform.rotation = Quat::from_rotation_x(elbow_bend.min(-0.1));
        }
        
        // Hand stays relatively straight
        if let Ok(mut transform) = limb_query.get_mut(hand) {
            transform.rotation = Quat::IDENTITY;
        }
    } else {
        // =====================================================================
        // IDLE POSE - Relaxed standing
        // =====================================================================
        // Subtle breathing-linked sway, arms hang naturally
        let breath_phase = elapsed * 0.8;  // Slow breathing rhythm
        let sway_offset = if is_left { 0.0 } else { 0.3 };  // Slight phase offset between arms
        
        // Very subtle forward/back sway synced with breathing
        let idle_sway = (breath_phase + sway_offset).sin() * 0.02;
        // Tiny side-to-side drift
        let side_drift = (breath_phase * 0.7 + sway_offset).cos() * 0.01;
        
        if let Ok(mut transform) = limb_query.get_mut(upper_arm) {
            transform.rotation = Quat::from_rotation_x(idle_sway) 
                * Quat::from_rotation_z(side_drift);
        }
        
        // Natural elbow bend at rest - arms don't hang perfectly straight
        // Negative X = forearm bends forward (natural resting position)
        if let Ok(mut transform) = limb_query.get_mut(lower_arm) {
            transform.rotation = Quat::from_rotation_x(-0.2);
        }
        
        // Hands relaxed
        if let Ok(mut transform) = limb_query.get_mut(hand) {
            transform.rotation = Quat::from_rotation_x(-0.05);
        }
    }
}

/// Animate a leg chain with proper knee bending
#[allow(dead_code)]
fn animate_leg(
    limb_query: &mut Query<&mut Transform, With<CharacterLimb>>,
    upper_leg: Entity,
    lower_leg: Entity,
    foot: Entity,
    walk_cycle: f32,
    speed_factor: f32,
    is_moving: bool,
    is_airborne: bool,
    is_jumping: bool,
    air_time: f32,
    is_left: bool,
) {
    let phase = if is_left { std::f32::consts::PI } else { 0.0 };
    
    if is_airborne {
        // =====================================================================
        // AIRBORNE LEG POSE
        // =====================================================================
        if is_jumping {
            // Jump ascent - legs tucked up, knees bent
            // One leg slightly more forward than the other for dynamic pose
            let leg_offset = if is_left { 0.1 } else { -0.1 };
            let hip_tuck = -0.5 + leg_offset;  // Legs come up (negative = forward)
            let knee_bend = 0.9;  // Knees bent
            
            if let Ok(mut transform) = limb_query.get_mut(upper_leg) {
                transform.rotation = Quat::from_rotation_x(hip_tuck);
            }
            
            if let Ok(mut transform) = limb_query.get_mut(lower_leg) {
                transform.rotation = Quat::from_rotation_x(knee_bend);
            }
            
            // Feet point down slightly
            if let Ok(mut transform) = limb_query.get_mut(foot) {
                transform.rotation = Quat::from_rotation_x(0.3);
            }
        } else {
            // Falling - legs extend down, ready for landing
            // Athletic stance - legs together, knees slightly bent for impact
            let extend_factor = (air_time * 3.0).min(1.0);
            
            // Legs extend down and slightly forward to prepare for landing
            let hip_angle = -0.2 * (1.0 - extend_factor) + 0.1 * extend_factor;
            // Knees stay bent for shock absorption on landing
            let knee_bend = 0.6 * (1.0 - extend_factor) + 0.3 * extend_factor;
            
            if let Ok(mut transform) = limb_query.get_mut(upper_leg) {
                transform.rotation = Quat::from_rotation_x(hip_angle);
            }
            
            if let Ok(mut transform) = limb_query.get_mut(lower_leg) {
                transform.rotation = Quat::from_rotation_x(knee_bend);
            }
            
            // Feet ready for ground contact
            if let Ok(mut transform) = limb_query.get_mut(foot) {
                transform.rotation = Quat::from_rotation_x(-0.1);  // Slight dorsiflexion for heel strike
            }
        }
    } else if is_moving {
        // =====================================================================
        // WALKING/RUNNING GAIT
        // =====================================================================
        // Walk cycle phases:
        // 0 = leg forward (hip flexed, knee slightly bent)
        // PI/2 = leg passing under body
        // PI = leg back (hip extended, knee straight)
        // 3PI/2 = leg lifting (hip neutral, knee bent)
        
        let cycle = walk_cycle + phase;
        let hip_swing = cycle.sin();
        let cycle_phase = cycle % std::f32::consts::TAU;
        
        // Hip rotation (upper leg swings forward/back)
        // More swing at higher speeds
        let hip_amplitude = 0.4 + 0.2 * speed_factor;
        let hip_angle = hip_swing * hip_amplitude;
        
        if let Ok(mut transform) = limb_query.get_mut(upper_leg) {
            transform.rotation = Quat::from_rotation_x(hip_angle);
        }
        
        // Knee bend - bends during swing phase (when leg is moving forward)
        // More bend at higher speeds for running form
        let knee_bend = if cycle_phase > std::f32::consts::PI {
            // Swing phase - knee bends to clear ground
            let swing_progress = (cycle_phase - std::f32::consts::PI) / std::f32::consts::PI;
            let bend_curve = (swing_progress * std::f32::consts::PI).sin();
            let bend_amplitude = 0.8 + 0.6 * speed_factor;  // More bend when running
            bend_curve * bend_amplitude
        } else {
            // Stance phase - slight knee bend for shock absorption
            0.1 + 0.05 * speed_factor
        };
        
        if let Ok(mut transform) = limb_query.get_mut(lower_leg) {
            transform.rotation = Quat::from_rotation_x(knee_bend);
        }
        
        // Foot - compensate to stay relatively flat, with toe-off at back
        let foot_angle = if cycle_phase < std::f32::consts::FRAC_PI_2 {
            // Toe-off phase - foot points down
            0.3 * speed_factor
        } else if cycle_phase > std::f32::consts::PI * 1.5 {
            // Heel strike preparation - foot dorsiflexed
            -0.2
        } else {
            // Mid-stance/swing - neutral
            -hip_angle * 0.3 - knee_bend * 0.15
        };
        
        if let Ok(mut transform) = limb_query.get_mut(foot) {
            transform.rotation = Quat::from_rotation_x(foot_angle);
        }
    } else {
        // =====================================================================
        // IDLE STANCE
        // =====================================================================
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

/// Update camera position to follow character
/// Supports both third-person (orbiting) and first-person (inside head) modes
#[allow(dead_code)]
fn camera_follow(
    time: Res<Time>,
    character_query: Query<(&Transform, &LocomotionController, &CharacterFacing), (With<CharacterRoot>, Without<PlayerCamera>)>,
    mut camera_query: Query<(&mut Transform, &PlayerCamera), Without<CharacterRoot>>,
) {
    let delta = time.delta_secs();
    let Ok((mut cam_transform, camera)) = camera_query.single_mut() else { return };
    
    let Some(target) = camera.target else { return };
    let Ok((char_transform, locomotion, _facing)) = character_query.get(target) else { return };
    
    // Character dimensions (should match skinned_character.rs)
    let character_height = 1.83;  // Y-Bot height
    
    // Eye height from character root entity
    // The mesh is offset, but we want camera at actual eye position
    // Eye is at ~94% of character height = 1.72m from ground
    // Character root spawns at (character_height / 2.0 + 0.1) above spawn point
    // So eye offset from root = 1.72 - 0.915 - 0.1 = ~0.70m
    // But simpler: just use a fixed offset that looks right
    let eye_height_from_root = 0.75;  // Tuned for first-person eye level
    
    // Head center for third person (slightly lower than eyes)
    let head_height_from_root = 0.65;
    
    let target_pos = if camera.is_first_person {
        // First person: Camera at eye level, centered on head
        let eye_offset = Vec3::new(0.0, eye_height_from_root, 0.0);
        char_transform.translation + eye_offset
    } else {
        // Third person: Camera orbits around character head
        let offset = Vec3::new(
            camera.distance * camera.pitch.cos() * camera.yaw.sin(),
            camera.distance * camera.pitch.sin() + head_height_from_root,
            camera.distance * camera.pitch.cos() * camera.yaw.cos(),
        );
        char_transform.translation + offset
    };
    
    // Smooth follow (faster in first person for responsiveness)
    let smoothing = if camera.is_first_person {
        50.0  // Very responsive in first person - almost instant
    } else if locomotion.speed > 0.5 {
        10.0
    } else {
        6.0
    };
    cam_transform.translation = cam_transform.translation.lerp(target_pos, (smoothing * delta).min(1.0));
    
    if camera.is_first_person {
        // First person: Look in the direction of camera yaw/pitch
        // Yaw 0 = looking along -Z (forward in Bevy's coordinate system)
        let look_dir = Vec3::new(
            -camera.pitch.cos() * camera.yaw.sin(),
            -camera.pitch.sin(),
            -camera.pitch.cos() * camera.yaw.cos(),
        );
        let look_target = cam_transform.translation + look_dir * 10.0;
        cam_transform.look_at(look_target, Vec3::Y);
    } else {
        // Third person: Look at character head
        let look_target = char_transform.translation + Vec3::Y * head_height_from_root;
        cam_transform.look_at(look_target, Vec3::Y);
    }
}
