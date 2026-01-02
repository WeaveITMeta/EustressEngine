//! # Play Mode Runtime
//!
//! Runtime systems for Play Mode that use the SHARED character plugin.
//! This ensures identical gameplay behavior between Studio and Client.
//!
//! ## Architecture
//!
//! When Play Mode starts:
//! 1. Snapshot the entire world state
//! 2. Use SharedCharacterPlugin for character spawning and control
//! 3. Switch camera to follow mode
//! 4. Enable physics simulation
//!
//! When Play Mode stops:
//! 1. Despawn all play-mode entities (player, character, etc.)
//! 2. Restore world from snapshot
//! 3. Restore editor camera
//! 4. Disable physics simulation

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use bevy::input::mouse::MouseMotion;
use avian3d::prelude::*;
use eustress_common::classes::{Humanoid, Instance, ClassName};
use eustress_common::services::player::{
    Player, Character, CharacterRoot, CharacterHead,
    BiologicalSex,
};
use eustress_common::services::animation::{
    AnimationStateMachine, LocomotionController, ProceduralAnimation,
};
use eustress_common::plugins::skinned_character::{
    spawn_skinned_character, CharacterModel, CharacterGender,
};

// Re-export shared character types from common
pub use eustress_common::plugins::character_plugin::{
    CharacterPhysics, MovementIntent, CharacterFacing,
    PlayModeCharacter, PlayModeCamera,
    spawn_play_mode_character as shared_spawn_character,
    spawn_play_mode_camera as shared_spawn_camera,
    // Note: cleanup_play_mode_entities is defined locally for runtime tracking
};

use super::play_mode::{PlayModeState, PlayMode};

// ============================================================================
// Play Mode Configuration
// ============================================================================

/// Configuration for play mode character system
#[derive(Resource, Debug)]
pub struct PlayModeCharacterConfig {
    /// Use skinned GLB characters instead of procedural primitives
    pub use_skinned_characters: bool,
    /// Default biological sex for spawned characters
    pub default_biological_sex: BiologicalSex,
}

impl Default for PlayModeCharacterConfig {
    fn default() -> Self {
        Self {
            use_skinned_characters: true, // Enable skinned characters by default
            default_biological_sex: BiologicalSex::Female,
        }
    }
}

// ============================================================================
// Play Mode Runtime Resource
// ============================================================================

/// Runtime state for play mode - tracks all spawned entities and services
#[derive(Resource, Debug, Default)]
pub struct PlayModeRuntime {
    /// Local player entity
    pub local_player: Option<Entity>,
    /// Character root entity (physics body)
    pub character_entity: Option<Entity>,
    /// All entities spawned during play mode (for cleanup)
    pub spawned_entities: Vec<Entity>,
    /// Play mode camera entity
    pub play_camera: Option<Entity>,
    /// Original editor camera transform (to restore)
    pub editor_camera_transform: Option<Transform>,
    /// Original cursor grab mode
    pub editor_cursor_mode: Option<CursorGrabMode>,
    /// Is cursor locked for play mode
    pub cursor_locked: bool,
    /// Camera yaw (horizontal rotation)
    pub camera_yaw: f32,
    /// Camera pitch (vertical rotation)
    pub camera_pitch: f32,
    /// Camera distance from character
    pub camera_distance: f32,
    /// Physics enabled during play
    pub physics_enabled: bool,
}

impl PlayModeRuntime {
    pub fn new() -> Self {
        Self {
            camera_distance: 8.0,
            camera_pitch: 0.3,
            ..default()
        }
    }
    
    /// Track a spawned entity for cleanup
    pub fn track_entity(&mut self, entity: Entity) {
        self.spawned_entities.push(entity);
    }
    
    /// Clear all tracked entities
    pub fn clear(&mut self) {
        self.local_player = None;
        self.character_entity = None;
        self.spawned_entities.clear();
        self.play_camera = None;
        self.cursor_locked = false;
        self.physics_enabled = false;
    }
}

// ============================================================================
// Character Components - Use shared types from eustress_common::plugins::character_plugin
// CharacterPhysics, MovementIntent, CharacterFacing, PlayModeCharacter, PlayModeCamera
// are all re-exported above from the shared plugin
// ============================================================================

/// Character physics body marker (local to runtime for tracking)
#[derive(Component, Debug)]
pub struct PlayModePhysicsBody;

// ============================================================================
// Character Spawning - Uses SHARED spawn functions from character_plugin
// ============================================================================

/// Spawn a full character for play mode
/// Supports both skinned GLB characters and procedural primitives
pub fn spawn_play_mode_character(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    spawn_pos: Vec3,
    runtime: &mut PlayModeRuntime,
    config: &PlayModeCharacterConfig,
) -> Entity {
    let character_entity = if config.use_skinned_characters {
        // Use skinned GLB character
        info!("🎭 Spawning skinned GLB character at {:?}", spawn_pos);
        
        let model = config.default_biological_sex.character_model();
        let gender = config.default_biological_sex.character_gender();
        
        info!("📦 Loading character model: {:?} with {:?} animations", model, gender);
        
        spawn_skinned_character(commands, asset_server, spawn_pos, model, gender)
    } else {
        // Use procedural primitive character (legacy)
        info!("🎮 Spawning procedural character at {:?} (using SHARED plugin)", spawn_pos);
        shared_spawn_character(commands, meshes, materials, spawn_pos)
    };
    
    // Track for cleanup
    runtime.track_entity(character_entity);
    runtime.character_entity = Some(character_entity);
    
    // Add PlayModeCharacter marker (skinned characters don't have it by default)
    // and our local physics body marker
    commands.entity(character_entity).insert((PlayModeCharacter, PlayModePhysicsBody));
    
    info!("✅ Play mode character spawned: {:?}", character_entity);
    
    character_entity
}

/// Spawn play mode camera using the SHARED character plugin
pub fn spawn_play_mode_camera(
    commands: &mut Commands,
    character_entity: Entity,
    runtime: &mut PlayModeRuntime,
) -> Entity {
    // Use the SHARED spawn function from character_plugin
    let camera_entity = shared_spawn_camera(commands, character_entity);
    
    runtime.track_entity(camera_entity);
    runtime.play_camera = Some(camera_entity);
    
    info!("📷 Play mode camera spawned (using SHARED plugin)");
    
    camera_entity
}

// ============================================================================
// Runtime Systems - Most character systems are now in SharedCharacterPlugin!
// These are only play-mode-specific systems for Studio integration
// ============================================================================

/// Cleanup all play mode entities (uses runtime tracking)
/// Alias for backward compatibility
pub fn cleanup_play_mode_entities(
    commands: &mut Commands,
    runtime: &mut PlayModeRuntime,
) {
    info!("🧹 Cleaning up {} play mode entities", runtime.spawned_entities.len());
    
    for entity in runtime.spawned_entities.drain(..) {
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.despawn();
        }
    }
    
    runtime.clear();
}

// ============================================================================
// Plugin
// ============================================================================

/// Play Mode Runtime Plugin - tracks play mode state for Studio
/// 
/// NOTE: Character movement, camera, and input systems are provided by
/// SharedCharacterPlugin (added in PlayModePlugin). This plugin only
/// handles Studio-specific runtime tracking.
pub struct PlayModeRuntimePlugin;

impl Plugin for PlayModeRuntimePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<PlayModeRuntime>()
            .init_resource::<PlayModeCharacterConfig>();
        
        // Character systems are now in SharedCharacterPlugin!
        // Animation systems are in SharedAnimationPlugin!
        // No duplicate systems here - we just track runtime state
        info!("🎮 PlayModeRuntimePlugin: Using SHARED character + animation systems from common");
    }
}
