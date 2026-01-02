//! # Skinned Character System
//!
//! AAA-quality skinned mesh characters using GLTF/GLB models with skeletal animation.
//! Replaces procedural primitive-based characters with proper rigged meshes.
//!
//! ## Features
//!
//! - Load Mixamo X-Bot/Y-Bot or custom rigged characters
//! - Animation clip playback with blending
//! - Animation state machine integration
//! - Retargeting support for shared animations
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Spawn a skinned character
//! spawn_skinned_character(&mut commands, &asset_server, Vec3::ZERO, CharacterModel::YBot, CharacterGender::Female);
//! ```

use bevy::prelude::*;
use avian3d::prelude::*;

use crate::services::animation::{AnimationStateMachine, LocomotionController, ProceduralAnimation};
use crate::services::player::{Character, CharacterRoot};
use super::character_plugin::{CharacterFacing, CharacterPhysics, MovementIntent};

// ============================================================================
// Character Model Types
// ============================================================================

/// Available character models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Reflect)]
pub enum CharacterModel {
    /// Mixamo X-Bot (masculine, robotic)
    XBot,
    /// Mixamo Y-Bot (feminine, robotic)
    #[default]
    YBot,
    /// Custom model (specify path)
    Custom,
}

impl CharacterModel {
    /// Get the asset path for this character model
    pub fn asset_path(&self) -> &'static str {
        match self {
            CharacterModel::XBot => "characters/x_bot.glb",
            CharacterModel::YBot => "characters/y_bot.glb",
            CharacterModel::Custom => "characters/custom.glb",
        }
    }
    
    /// Get the scene label for GLTF loading
    pub fn scene_label(&self) -> &'static str {
        "Scene0"
    }
}

/// Character gender for animation selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Reflect)]
pub enum CharacterGender {
    #[default]
    Male,
    Female,
}

// ============================================================================
// Animation Assets
// ============================================================================

/// Paths to animation clips for a character
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component)]
pub struct CharacterAnimationPaths {
    /// Idle animation path
    pub idle: String,
    /// Walking animation path
    pub walk: String,
    /// Running animation path
    pub run: String,
    /// Sprinting animation path
    pub sprint: String,
    /// Jump animation path
    pub jump: String,
}

impl CharacterAnimationPaths {
    /// Create animation paths for a character gender
    pub fn for_gender(gender: CharacterGender) -> Self {
        let prefix = match gender {
            CharacterGender::Male => "male",
            CharacterGender::Female => "female",
        };
        
        Self {
            idle: format!("characters/animations/{}_idle.glb#Animation0", prefix),
            walk: format!("characters/animations/{}_walking.glb#Animation0", prefix),
            run: format!("characters/animations/{}_running.glb#Animation0", prefix),
            // Use running animation for sprint (no separate sprint animation yet)
            sprint: format!("characters/animations/{}_running.glb#Animation0", prefix),
            jump: format!("characters/animations/{}_jump.glb#Animation0", prefix),
        }
    }
}

// ============================================================================
// Skinned Character Component
// ============================================================================

/// Marker component for skinned mesh characters
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct SkinnedCharacter {
    /// Which model to use
    pub model: CharacterModel,
    /// Character gender (for animation selection)
    pub gender: CharacterGender,
    /// Scale multiplier
    pub scale: f32,
    /// Vertical offset for mesh (positive = up, negative = down)
    /// Use this to adjust feet position relative to ground
    pub vertical_offset: f32,
    /// Whether the scene has been spawned
    pub scene_spawned: bool,
    /// Entity holding the scene root (for animation)
    pub scene_entity: Option<Entity>,
}

impl SkinnedCharacter {
    pub fn new(model: CharacterModel) -> Self {
        Self {
            model,
            gender: match model {
                CharacterModel::XBot => CharacterGender::Male,
                CharacterModel::YBot => CharacterGender::Female,
                CharacterModel::Custom => CharacterGender::Male,
            },
            scale: 1.0,
            vertical_offset: 0.0,
            scene_spawned: false,
            scene_entity: None,
        }
    }
    
    pub fn with_gender(mut self, gender: CharacterGender) -> Self {
        self.gender = gender;
        self
    }
    
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }
    
    /// Set vertical offset for mesh positioning (positive = up)
    pub fn with_vertical_offset(mut self, offset: f32) -> Self {
        self.vertical_offset = offset;
        self
    }
}

// ============================================================================
// Character Spawning
// ============================================================================

/// Spawn a complete skinned character with physics
pub fn spawn_skinned_character(
    commands: &mut Commands,
    asset_server: &AssetServer,
    spawn_pos: Vec3,
    model: CharacterModel,
    gender: CharacterGender,
) -> Entity {
    // Vertical offset to adjust mesh position (positive = up, tweak this if feet are in ground)
    const MESH_VERTICAL_OFFSET: f32 = 0.48;
    
    let character = SkinnedCharacter::new(model)
        .with_gender(gender)
        .with_vertical_offset(MESH_VERTICAL_OFFSET);
    let animation_paths = CharacterAnimationPaths::for_gender(gender);
    let scale = character.scale;
    let vertical_offset = character.vertical_offset;
    
    // Character dimensions for physics capsule
    let character_height = 1.83 * scale;
    let capsule_radius = 0.33 * scale;
    let capsule_half_height = (character_height / 2.0) - capsule_radius;
    
    // Spawn the character root with physics
    let character_entity = commands.spawn((
        Transform::from_translation(spawn_pos + Vec3::Y * (character_height / 2.0 + 0.1)),
        Visibility::default(),
        Name::new("SkinnedCharacter"),
    )).id();
    
    // Add physics components
    commands.entity(character_entity).insert((
        RigidBody::Dynamic,
        Collider::capsule(capsule_radius, capsule_half_height),
        CollisionMargin(0.02),
        LockedAxes::ROTATION_LOCKED,
        Friction::new(1.0),
        Restitution::new(0.0),
        GravityScale(1.0),
        LinearVelocity::default(),
    ));
    
    // Add character components (including movement components for SharedCharacterPlugin)
    commands.entity(character_entity).insert((
        character,
        animation_paths,
        Character::default(),
        CharacterRoot,
        AnimationStateMachine::default(),
        LocomotionController::default(),
        CharacterFacing::default(),
        // Movement components required by SharedCharacterPlugin
        MovementIntent::default(),
        CharacterPhysics::default(),
        ProceduralAnimation::default(),
    ));
    
    // Load and spawn the GLTF scene as a child
    let scene_path = format!("{}#{}", model.asset_path(), model.scene_label());
    let scene_handle: Handle<Scene> = asset_server.load(scene_path);
    
    // Y-Bot/X-Bot models have origin at feet, so offset mesh down to align with capsule bottom
    // Capsule center is at character_height/2, so mesh needs to go down by capsule_half_height + capsule_radius
    // Add vertical_offset for manual adjustment (positive = up)
    let mesh_offset = -(capsule_half_height + capsule_radius) + vertical_offset;
    
    // Apply rotation to fix model orientation (model is Z-up, we need Y-up)
    let rotation_fix = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    
    let scene_entity = commands.spawn((
        SceneRoot(scene_handle),
        Transform::from_scale(Vec3::splat(scale))
            .with_rotation(rotation_fix)
            .with_translation(Vec3::Y * mesh_offset),
        Visibility::default(),
        Name::new("CharacterMesh"),
    )).id();
    
    // Parent the scene to the character
    commands.entity(scene_entity).insert(ChildOf(character_entity));
    
    // Update character with scene entity reference
    commands.entity(character_entity).insert(SceneEntityRef(scene_entity));
    
    character_entity
}

/// Reference to the scene entity for a skinned character
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct SceneEntityRef(pub Entity);

// ============================================================================
// Animation Systems (to be implemented in client/engine crates with full Bevy animation)
// ============================================================================

/// Marker for characters that need animation setup
#[derive(Component)]
pub struct NeedsAnimationSetup;

/// System to mark newly spawned characters for animation setup
pub fn mark_new_characters_for_animation(
    mut commands: Commands,
    new_characters: Query<Entity, (With<SkinnedCharacter>, Without<NeedsAnimationSetup>, Without<super::animation_plugin::CharacterAnimationLink>)>,
    children: Query<&Children>,
) {
    for entity in new_characters.iter() {
        // Check if scene has loaded (has children)
        if let Ok(kids) = children.get(entity) {
            if !kids.is_empty() {
                info!("🎬 Marking character {:?} for animation setup ({} children)", entity, kids.len());
                commands.entity(entity).insert(NeedsAnimationSetup);
            }
        }
    }
}

/// System to apply character facing to the skinned mesh
/// This rotates the mesh child entity based on the CharacterFacing component
pub fn apply_skinned_character_facing(
    time: Res<Time>,
    mut character_query: Query<(&mut CharacterFacing, &SceneEntityRef), With<SkinnedCharacter>>,
    mut mesh_query: Query<&mut Transform>,
) {
    let delta = time.delta_secs();
    let turn_speed = 8.0; // Radians per second for smooth turning
    
    for (mut facing, scene_ref) in character_query.iter_mut() {
        // Calculate shortest angle difference (handles wrap-around correctly)
        let mut angle_diff = facing.target_angle - facing.angle;
        // Normalize to [-PI, PI]
        while angle_diff > std::f32::consts::PI {
            angle_diff -= std::f32::consts::TAU;
        }
        while angle_diff < -std::f32::consts::PI {
            angle_diff += std::f32::consts::TAU;
        }
        
        // Smoothly interpolate toward target
        let max_turn = turn_speed * delta;
        if angle_diff.abs() < max_turn {
            facing.angle = facing.target_angle;
        } else {
            facing.angle += angle_diff.signum() * max_turn;
        }
        
        // Normalize angle to [0, TAU)
        facing.angle = facing.angle.rem_euclid(std::f32::consts::TAU);
        
        // Apply rotation to the mesh entity (facing + orientation fix)
        if let Ok(mut mesh_transform) = mesh_query.get_mut(scene_ref.0) {
            let facing_rotation = Quat::from_rotation_y(facing.angle);
            let orientation_fix = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            mesh_transform.rotation = facing_rotation * orientation_fix;
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for skinned character functionality
pub struct SkinnedCharacterPlugin;

impl Plugin for SkinnedCharacterPlugin {
    fn build(&self, app: &mut App) {
        // Register custom types
        app
            .register_type::<SkinnedCharacter>()
            .register_type::<CharacterAnimationPaths>()
            .register_type::<CharacterModel>()
            .register_type::<CharacterGender>()
            .register_type::<SceneEntityRef>();
        
        // Register Bevy types needed for GLTF scene spawning (Bevy 0.17 requirement)
        // These are internal types that the scene spawner needs for reflection
        app
            .register_type::<Transform>()
            .register_type::<GlobalTransform>()
            .register_type::<Visibility>()
            .register_type::<InheritedVisibility>()
            .register_type::<ViewVisibility>()
            .register_type::<Name>();
        
        app.add_systems(Update, (
            mark_new_characters_for_animation,
            apply_skinned_character_facing,
        ));
    }
}
