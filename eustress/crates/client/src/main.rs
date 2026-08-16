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
mod soul;

use bevy::prelude::*;
use avian3d::prelude::*;
use eustress_common::{spawn_baseplate, spawn_welcome_cube};
use eustress_common::services::TeamServicePlugin;
// DISABLED for bevy 0.19 — bevy_quinnet (p2p QUIC) has no 0.19 release yet.
// Re-enable with the `p2p` feature once upstream ships (see BEVY_019_MIGRATION.md).
// use eustress_networking::p2p::DistributedWorldPlugin;
use plugins::{
    EnhancementPlugin, PlayerServicePlugin, LightingServicePlugin,
    PauseMenuPlugin, ClientTerrainPlugin, CharacterAnimationPlugin,
};
use eustress_common::plugins::SkinnedCharacterPlugin;
use eustress_common::avatar::{
    AvatarDescriptor, AvatarHost, AvatarRuntimePlugin, SpawnAvatar,
};
use eustress_common::avatar::control::AvatarEscapePressed;
use systems::LoadSceneEvent;
use std::path::PathBuf;

/// Command line arguments
#[derive(Resource, Default)]
struct ClientArgs {
    scene_path: Option<PathBuf>,
}

/// Build the Client app without running it.
///
/// Extracted so the golden parity test can construct both shells as *values*
/// and diff their registered systems, resources and component sets. Two
/// binaries that can only be launched cannot be compared; two `App`s can.
pub fn build_app(scene_path: Option<PathBuf>) -> App {
    let mut app = App::new();

    // ── Asset sources MUST be registered before AssetPlugin ────────────────
    //
    // Bevy freezes the asset-source table when AssetPlugin builds. The Client
    // never registered `bundled://` at all, so every character GLB and all
    // eight animation clips 404'd and the player was invisible. Three separate
    // audit findings were this one omission.
    eustress_common::avatar::boot::register_avatar_asset_sources(&mut app);

    app
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
            // Absolute paths (imported meshes, user content) are refused under
            // Bevy 0.19's default UnapprovedPathMode::Forbid.
            .set(eustress_common::avatar::boot::permissive_asset_plugin("../common/assets"))
        )

        // Physics (Avian3D) - Realistic Earth gravity: 9.80665 m/s²
        .add_plugins(PhysicsPlugins::default())
        .insert_resource(Gravity(Vec3::NEG_Y * eustress_common::avatar::GRAVITY_MPS2))
        // Match Studio's fixed timestep. The Client ran at Bevy's 64 Hz default
        // while Studio pinned 60 Hz, so identical inputs integrated differently.
        .insert_resource(Time::<Fixed>::from_hz(60.0))

        // Services (Roblox-style)
        .add_plugins(LightingServicePlugin)  // Skybox, sun, ambient, fog
        .add_plugins(PlayerServicePlugin)    // Player, character, camera
        .add_plugins(ClientTerrainPlugin)    // Terrain rendering with physics
        .add_plugins(PauseMenuPlugin)        // ESC menu with Resume/Reset/Settings/Exit
        .add_plugins(TeamServicePlugin)      // Team system (colors, spawns, etc.)

        // ── The sealed avatar runtime ─────────────────────────────────────
        // Same plugin, same descriptor, same physics body as Studio Play Mode.
        .add_plugins(AvatarRuntimePlugin::new(AvatarHost::Client))

        // Skinned character animation system (GLB models)
        .add_plugins(SkinnedCharacterPlugin)
        .add_plugins(CharacterAnimationPlugin)

        // Window / taskbar icon. The .ico in the PE resource table only
        // reaches Explorer; winit's window class ships with no icon at all.
        .add_plugins(systems::window_icon::WindowIconPlugin)
        // Frame-burst capture so an agent can SEE the running client.
        .add_plugins(systems::frame_capture::FrameCapturePlugin)
        // Agent control surface (opt-in via EUSTRESS_AGENT_PORT).
        .add_plugins(systems::agent_control::AgentControlPlugin)
        // Published-content fetch: download + unpack a .pak from R2.
        .add_plugins(systems::space_fetch::SpaceFetchPlugin)
        // Open a real Eustress Space (Movement/Climbing by default) instead of
        // the two hardcoded demo primitives.
        .add_plugins(systems::space_world::SpaceWorldPlugin)

        // Enhancement pipeline
        .add_plugins(EnhancementPlugin)
        
        // Soul scripting — Rune + Luau + GUI bridge
        .add_plugins(soul::ClientSoulPlugin)
        .add_plugins(soul::ClientPhysicsBridgePlugin)

        // P2P Distributed World (CRDT-based chunk sync) — DISABLED for bevy 0.19
        // until bevy_quinnet ships a 0.19-compatible release (BEVY_019_MIGRATION.md)
        // .add_plugins(DistributedWorldPlugin)

        // Register types needed for glTF scene spawning.
        //
        // Was three types; the spawner needs the full set (Gltf* extras,
        // hierarchy, visibility, skinning, animation) or it panics with
        // "unregistered type" the moment a rigged character glTF loads.
        .add_plugins(|app: &mut App| {
            eustress_common::avatar::boot::register_gltf_scene_types(app)
        })
        
        // Resources
        .insert_resource(ClientArgs { scene_path })

        // World setup - loads default scene (same as Studio)
        // Only when no Space is open: otherwise the demo baseplate and cube
        // spawn inside the authored level.
        .add_systems(
            Startup,
            setup_default_scene.run_if(|| !systems::space_world::space_is_available()),
        )
        .add_systems(Startup, spawn_local_avatar.after(setup_default_scene))
        .add_systems(Update, handle_escape);

    app
}

fn main() {
    // Parse command line args
    let args: Vec<String> = std::env::args().collect();
    let scene_path = args.get(1).map(PathBuf::from);

    build_app(scene_path).run();
}

/// Spawn the player's avatar through the sealed runtime.
///
/// The descriptor is the ONLY input. There is no model path and no sex here —
/// the old code hardcoded `BiologicalSex::Male` on this side and
/// `BiologicalSex::Female` in Studio, which selected different bodies AND
/// different animation clip sets in the two shells.
fn spawn_local_avatar(mut spawn: MessageWriter<SpawnAvatar>) {
    // TODO(P6): load the signed-in user's saved descriptor from the profile
    // API instead of the default. Until that route exists, both shells spawn
    // the identical default — which is itself the parity property we want.
    let descriptor = AvatarDescriptor::default();
    spawn.write(SpawnAvatar::new(descriptor, Vec3::new(0.0, 2.0, 8.0)));
}

/// The Client's meaning of Escape. `HostSeams` guarantees Studio's differs.
fn handle_escape(mut events: MessageReader<AvatarEscapePressed>) {
    for e in events.read() {
        debug_assert_eq!(e.0, eustress_common::avatar::EscapeAction::PauseMenu);
        // PauseMenuPlugin owns the menu itself; this is the single seam that
        // tells it Escape happened, replacing a third competing Escape binding.
        info!("⏸️ Escape → pause menu");
    }
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
        // FULL extents. `Collider::cuboid(x, y, z)` halves its arguments
        // internally (avian3d-0.7.0 .../parry/mod.rs:747), so passing
        // half-extents built a 256x0.5x256 box: its top sat at +0.25 while the
        // visible baseplate top is at +0.5, sinking every character a quarter
        // metre into the floor — and leaving three quarters of the plate with
        // no collision at all.
        Collider::cuboid(512.0, 1.0, 512.0),
    ));
    
    // Spawn welcome cube (2x2x2 green) and add physics collider
    // Same full-extent convention as the baseplate above.
    let cube = spawn_welcome_cube(&mut commands, &mut meshes, &mut materials);
    commands.entity(cube).insert((
        RigidBody::Static,
        Collider::cuboid(2.0, 2.0, 2.0),  // FULL extents of the 2x2x2 cube
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
