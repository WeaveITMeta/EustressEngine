//! Eustress Client - Generative Player & Renderer
//! 
//! This is the client that plays Eustress scenes with AI-enhanced rendering,
//! procedural generation, and next-gen visual effects.
//!
//! ## Features
//! - Uses the SAME default scene as Eustress Studio (shared code)
//! - Universal character that works in any scene
//! - Physics-based movement with Avian3D
//!
//! ## Plugins (Roblox-style services)
//! - PlayerServicePlugin: Player spawning, character controller, local player
//! - LightingServicePlugin: Skybox, sun, ambient, fog
//! - PhysicsPlugins: Avian3D physics

mod components;
mod systems;
mod plugins;

use bevy::prelude::*;
use avian3d::prelude::*;
use eustress_common::{spawn_baseplate, spawn_welcome_cube};
use eustress_common::services::TeamServicePlugin;
use eustress_networking::p2p::DistributedWorldPlugin;
use plugins::{
    EnhancementPlugin, PlayerServicePlugin, LightingServicePlugin, 
    PauseMenuPlugin, ClientTerrainPlugin, CharacterAnimationPlugin,
};
use eustress_common::plugins::SkinnedCharacterPlugin;
use systems::LoadSceneEvent;
use std::path::PathBuf;

/// Command line arguments
#[derive(Resource, Default)]
struct ClientArgs {
    scene_path: Option<PathBuf>,
}

fn main() {
    // Parse command line args
    let args: Vec<String> = std::env::args().collect();
    let scene_path = args.get(1).map(PathBuf::from);
    
    App::new()
        // Core Bevy plugins
        .add_plugins(DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Eustress Client".to_string(),
                    resolution: bevy::window::WindowResolution::new(1920, 1080),
                    present_mode: bevy::window::PresentMode::Fifo, // VSync
                    ..default()
                }),
                ..default()
            })
            // Use common assets folder for shared character models and animations
            .set(AssetPlugin {
                file_path: "../common/assets".to_string(),
                ..default()
            })
        )
        
        // Physics (Avian3D) - Realistic Earth gravity: 9.80665 m/s²
        .add_plugins(PhysicsPlugins::default())
        .insert_resource(Gravity(Vec3::NEG_Y * 9.80665))
        
        // Services (Roblox-style)
        .add_plugins(LightingServicePlugin)  // Skybox, sun, ambient, fog
        .add_plugins(PlayerServicePlugin)    // Player, character, camera
        .add_plugins(ClientTerrainPlugin)    // Terrain rendering with physics
        .add_plugins(PauseMenuPlugin)        // ESC menu with Resume/Reset/Settings/Exit
        .add_plugins(TeamServicePlugin)      // Team system (colors, spawns, etc.)
        
        // Skinned character animation system (GLB models)
        .add_plugins(SkinnedCharacterPlugin)
        .add_plugins(CharacterAnimationPlugin)
        
        // Enhancement pipeline
        .add_plugins(EnhancementPlugin)
        
        // P2P Distributed World (CRDT-based chunk sync)
        .add_plugins(DistributedWorldPlugin)
        
        // Register types needed for GLTF scene spawning
        .register_type::<Transform>()
        .register_type::<GlobalTransform>()
        .register_type::<Name>()
        
        // Resources
        .insert_resource(ClientArgs { scene_path })
        
        // World setup - loads default scene (same as Studio)
        .add_systems(Startup, setup_default_scene)
        .run();
}

/// Setup default scene - uses the SAME code as Eustress Studio
/// Spawns baseplate and welcome cube from shared eustress_common
fn setup_default_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    args: Res<ClientArgs>,
    mut load_events: MessageWriter<LoadSceneEvent>,
) {
    info!("🌍 Setting up default scene (same as Studio)...");
    
    // =========================================================================
    // SPAWN DEFAULT SCENE - Same code as engine/src/default_scene.rs
    // Uses shared functions from eustress_common::default_scene
    // =========================================================================
    
    // Spawn baseplate (512x1x512 dark gray) and add physics collider
    let baseplate = spawn_baseplate(&mut commands, &mut meshes, &mut materials);
    commands.entity(baseplate).insert((
        RigidBody::Static,
        Collider::cuboid(256.0, 0.5, 256.0),  // Half-extents of 512x1x512
    ));
    
    // Spawn welcome cube (2x2x2 green) and add physics collider
    let cube = spawn_welcome_cube(&mut commands, &mut meshes, &mut materials);
    commands.entity(cube).insert((
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),  // Half-extents of 2x2x2
    ));
    
    // =========================================================================
    // SCENE LOADING - Load custom scene if provided via command line
    // =========================================================================
    
    if let Some(path) = &args.scene_path {
        info!("📂 Loading custom scene: {:?}", path);
        load_events.write(LoadSceneEvent { path: path.clone() });
    }
    
    info!("✅ Default scene ready (Baseplate + Welcome Cube)!");
}
