// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
#[allow(unused_imports)]
use bevy::render::RenderPlugin;
use bevy::gltf::{GltfExtras, GltfSceneExtras, GltfMeshExtras, GltfMaterialExtras};
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin, EntityCountDiagnosticsPlugin};
// Window icon: embedded in exe via winres (build.rs), runtime set in setup_slint_overlay
use eustress_common::plugins::lighting_plugin::SharedLightingPlugin;
use eustress_common::services::{TeamServicePlugin, PlayerService};

#[cfg(feature = "iggy-streaming")]
use std::sync::Arc;
#[cfg(feature = "iggy-streaming")]
use eustress_common::iggy_queue::{IggyConfig, IggyPlugin};
#[cfg(feature = "iggy-streaming")]
use eustress_common::sim_stream::SimStreamWriter;

mod auth;
mod terrain_plugin;
mod ui;
mod parts;
mod classes;        // Roblox-style class system
mod properties;     // Property access system
mod rendering;
mod camera;
mod commands;
mod scenes;
mod default_scene;
mod serialization;  // Scene save/load
mod spawn;          // Entity spawn helpers
mod camera_controller;
mod gizmo_tools;
mod selection_box;
mod select_tool;
mod move_tool;
mod rotate_tool;
mod scale_tool;
mod selection_sync;
mod editor_settings;
mod undo;
mod notifications;
mod keybindings;
mod part_selection;
mod transform_space;
mod clipboard;
mod material_sync;
mod play_mode;          // Play mode with character spawning
mod play_mode_runtime;  // Client-like runtime systems for play mode
mod play_server;        // In-process server + client for Play Server mode
mod embedded_client;    // Embedded client runtime (same as standalone client)
mod runtime;            // Runtime systems (physics events, lighting, scripts)
mod seats;              // Seat and VehicleSeat systems (auto-sit, controller input)
mod soul;               // Soul scripting integration
mod telemetry;          // Opt-in error reporting via Sentry
mod window_focus;       // Window focus management (sleep when unfocused)
mod startup;            // Command-line args and file associations
mod studio_plugins;     // Studio plugin system (MindSpace, etc.)
mod math_utils;         // Shared math utilities (ray intersection, AABB, etc.)
mod entity_utils;       // Entity ID helpers
mod spatial_query_bridge; // Unified raycasting bridge for Rune + Luau scripting
mod io_manager;         // Async data fetching for Parameters
mod space;              // Space file-system-first architecture
mod simulation;         // Tick-based simulation with time compression
mod toolbox;            // Toolbox mesh insertion system
mod txt_to_toml_watcher; // Automatic .txt to .toml converter
mod workshop;           // Workshop Panel (System 0: Ideation)
mod manufacturing;      // Manufacturing Program: investor + manufacturer registry + AI allocation
mod frame_diagnostics;  // Frame time tracking to identify stutters

mod plugins;
mod shaders;
mod generative_pipeline;
mod viga;  // VIGA: Vision-as-Inverse-Graphics Agent
// mod slint_bevy_adapter;  // Disabled - Skia ICU conflicts on Windows

use rendering::PartRenderingPlugin;
use commands::{SelectionManager, TransformManager}; // Production-ready managers
use default_scene::DefaultScenePlugin;
use plugins::WorkspacePlugin;
use camera_controller::{CameraControllerPlugin, setup_camera_controller};
use gizmo_tools::GizmoToolsPlugin;
use selection_box::SelectionBoxPlugin;
use select_tool::SelectToolPlugin;
use move_tool::MoveToolPlugin;
use transform_space::TransformSpacePlugin;
use rotate_tool::RotateToolPlugin;
use scale_tool::ScaleToolPlugin;
use selection_sync::SelectionSyncPlugin;
use editor_settings::EditorSettingsPlugin;
use undo::UndoPlugin;
use notifications::NotificationPlugin;
use keybindings::KeyBindingsPlugin;
use clipboard::ClipboardPlugin;
use material_sync::MaterialSyncPlugin;
use terrain_plugin::EngineTerrainPlugin;
use play_mode::PlayModePlugin;
use window_focus::WindowFocusPlugin;
use startup::{StartupPlugin, StartupArgs};
// ServicePropertiesPlugin removed - now handled by Slint UI
use soul::EngineSoulPlugin;
use workshop::WorkshopPlugin;
use space::SpaceFileLoaderPlugin;

fn main() {
    println!("Starting Eustress Engine...");
    
    // Parse command-line arguments first (may exit for --help, --register, etc.)
    let args = StartupArgs::parse();
    
    // Generate window title - include scene name if opening a file
    let instance_id = std::process::id();
    let window_title = if let Some(ref scene_path) = args.scene_file {
        let scene_name = scene_path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        format!("{} - Eustress Engine", scene_name)
    } else {
        format!("Eustress Engine - Instance {}", instance_id)
    };
    
    // Initialize managers with Arc for thread-safe sharing
    let selection_manager = std::sync::Arc::new(parking_lot::RwLock::new(SelectionManager::default()));
    let transform_manager = std::sync::Arc::new(parking_lot::RwLock::new(TransformManager::default()));
    
    let mut app = App::new();
    
    // Silently ignore missing-resource errors instead of spamming WARN logs every frame.
    // Systems that access Res<T> without Option wrappers will simply skip execution.
    // The default `warn` handler was emitting hundreds of log lines per frame, tanking FPS.
    app.set_error_handler(bevy::ecs::error::ignore);
    
    // Register the Space asset source BEFORE DefaultPlugins
    // This must happen before AssetPlugin is initialized
    let space_root = space::default_space_root();
    info!("📁 Registering Space asset source at: {:?}", space_root);
    app.register_asset_source(
        "space",
        bevy::asset::io::AssetSourceBuilder::platform_default(&space_root.to_string_lossy(), None),
    );
    
    app // Bevy plugins with optimized window settings
        .add_plugins(DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: window_title,
                    resolution: bevy::window::WindowResolution::new(1600, 900),
                    present_mode: bevy::window::PresentMode::AutoNoVsync,
                    mode: bevy::window::WindowMode::Windowed,
                    decorations: true,
                    resizable: true,
                    ..default()
                }),
                close_when_requested: false,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(
                    bevy::render::settings::WgpuSettings {
                        // Request discrete GPU (NVIDIA/AMD) over integrated
                        power_preference: bevy::render::settings::PowerPreference::HighPerformance,
                        // Use all available backends (Vulkan/DX12/Metal)
                        backends: Some(bevy::render::settings::Backends::all()),
                        ..default()
                    }
                ),
                // Compile all shader pipelines synchronously to prevent mid-session
                // GPU pipeline stall stutters (750ms spikes visible in frame diagnostics)
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .set(AssetPlugin {
                file_path: "assets".to_string(),
                ..default()
            })
        )
        // Diagnostic plugins for FPS and performance profiling
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(EntityCountDiagnosticsPlugin::default())
        .add_plugins(LogDiagnosticsPlugin {
            wait_duration: std::time::Duration::from_secs(5),
            ..default()
        })
        // Register GLTF types for scene spawning (prevents panic on unregistered types)
        .register_type::<GltfExtras>()
        .register_type::<GltfSceneExtras>()
        .register_type::<GltfMeshExtras>()
        .register_type::<GltfMaterialExtras>()
        // PlayerService for play mode character spawning
        .init_resource::<PlayerService>()
        // Startup args
        .insert_resource(args.clone())
        // Notifications (must be before SlintUiPlugin which uses NotificationManager)
        .add_plugins(NotificationPlugin)
        // Undo/Redo (must be before SlintUiPlugin which uses UndoStack)
        .add_plugins(UndoPlugin)
        // (Iggy streaming registered below — cfg-block required, not inline chain)
        // Play mode (must be before SlintUiPlugin which uses PlayModeState)
        .add_plugins(PlayModePlugin)
        // Slint UI (software renderer overlay)
        .add_plugins(ui::slint_ui::SlintUiPlugin)
        // Floating windows
        .add_plugins(ui::floating_windows::FloatingWindowsPlugin)
        // 3D rendering
        .add_plugins(PartRenderingPlugin {
            selection_manager: selection_manager.clone(),
            transform_manager: transform_manager.clone(),
        })
        // Material sync
        .add_plugins(MaterialSyncPlugin)
        // Shared lighting
        .add_plugins(SharedLightingPlugin)
        // Default scene
        .add_plugins(DefaultScenePlugin)
        // Automatic .txt to .toml converter (file system workaround)
        .add_plugins(txt_to_toml_watcher::TxtToTomlWatcherPlugin)
        // Space file loader (dynamic file-system-first loading)
        .add_plugins(SpaceFileLoaderPlugin)
        // Instance streaming (three-tier: Cold disk → Hot RAM → Active ECS)
        .add_plugins(eustress_common::streaming::StreamingPlugin {
            config: eustress_common::streaming::StreamingConfig::default(),
            instances_dir: space::default_space_root().join("Workspace"),
        })
        // Toolbox (mesh insertion system)
        .add_plugins(toolbox::ToolboxPlugin)
        // Camera controls
        .add_plugins(CameraControllerPlugin)
        .add_systems(Startup, setup_camera_controller.after(default_scene::setup_default_scene))
        // Editor settings
        .add_plugins(EditorSettingsPlugin)
        // Keybindings
        .add_plugins(KeyBindingsPlugin)
        // Clipboard
        .add_plugins(ClipboardPlugin)
        // Workspace
        .add_plugins(WorkspacePlugin)
        // Service properties
        .add_plugins(ui::service_properties::ServicePropertiesPlugin)
        // Transform space
        .add_plugins(TransformSpacePlugin)
        // Gizmo tools
        .add_plugins(GizmoToolsPlugin)
        // Selection box
        .add_plugins(SelectionBoxPlugin)
        // Tools
        .add_plugins(SelectToolPlugin)
        .add_plugins(MoveToolPlugin)
        .add_plugins(RotateToolPlugin)
        .add_plugins(ScaleToolPlugin)
        // Selection sync
        .add_plugins(SelectionSyncPlugin {
            selection_manager: selection_manager.clone(),
        })
        // Terrain
        .add_plugins(EngineTerrainPlugin)
        // Physics (avian3d from git main - supports Bevy 0.18)
        .add_plugins(avian3d::PhysicsPlugins::default())
        .insert_resource(avian3d::prelude::Gravity(bevy::math::Vec3::NEG_Y * 9.80665))
        // Realism Physics System (materials, thermodynamics, fluids, deformation, visualizers)
        .add_plugins(eustress_common::realism::RealismPlugin)
        // Tick-based simulation with time compression (integrates with PlayModeState)
        .add_plugins(simulation::SimulationPlugin::default())
        // Gamepad
        .add_plugins(eustress_common::services::GamepadServicePlugin)
        // Notifications UI
        .add_plugins(ui::notifications::NotificationsPlugin)
        // Play server
        .add_plugins(play_server::PlayServerPlugin)
        // Embedded client
        .add_plugins(embedded_client::EmbeddedClientPlugin)
        // Team service
        .add_plugins(TeamServicePlugin)
        // Runtime
        .add_plugins(runtime::RuntimePlugin)
        // Seats
        .add_plugins(seats::SeatPlugin)
        // Soul scripting
        .add_plugins(EngineSoulPlugin)
        // Workshop (System 0: Ideation — conversational product creation)
        .add_plugins(WorkshopPlugin)
        // Generative pipeline
        .add_plugins(generative_pipeline::GenerativePipelinePlugin)
        // VIGA
        .add_plugins(viga::VigaPlugin)
        // IoManager
        .add_plugins(io_manager::IoManagerPlugin)
        // Telemetry
        .add_plugins(telemetry::TelemetryPlugin)
        // Geospatial (file-system-first: GeoJSON, GeoTIFF, HGT → 3D terrain + vectors)
        .add_plugins(eustress_geo::GeoPlugin)
        // Window focus
        .add_plugins(WindowFocusPlugin)
        // Startup
        .add_plugins(StartupPlugin)
        // Studio plugins
        .add_plugins(studio_plugins::StudioPluginSystem)
        // Frame diagnostics to identify stutters
        .add_plugins(frame_diagnostics::FrameDiagnosticsPlugin);
        
    // Left-click part selection with raycasting
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.add_systems(Update, part_selection::part_selection_system);
    }

    // Iggy streaming — must use a separate block because #[cfg] cannot gate
    // individual method calls inside a builder chain.
    #[cfg(feature = "iggy-streaming")]
    {
        app.add_plugins(IggyPlugin);
        app.add_systems(Startup, setup_sim_stream_writer);
    }

    app.run();
    
    println!("✅ Eustress Engine closed gracefully");
}

// ─────────────────────────────────────────────────────────────────────────────
// Iggy SimStreamWriter — one persistent connection, shared via Arc<Resource>
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy Resource wrapper so `Arc<SimStreamWriter>` can be stored in ECS.
#[cfg(feature = "iggy-streaming")]
#[derive(Resource)]
pub struct SimWriterResource(pub Arc<SimStreamWriter>);

/// Startup system: connect `SimStreamWriter` once and insert as a Bevy Resource.
///
/// Task 10 call sites (`run_simulation`, `process_feedback`, `execute_and_apply`)
/// read `Option<Res<SimWriterResource>>` and pass `Some(writer.0.clone())` to the
/// `publish_*_sync` helpers, replacing the `None` fallback connect.
///
/// Silently skipped if Iggy is unavailable — engine continues without streaming.
#[cfg(feature = "iggy-streaming")]
fn setup_sim_stream_writer(mut commands: Commands, config: Option<Res<IggyConfig>>) {
    let cfg = config.map(|c| c.clone()).unwrap_or_default();

    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("sim writer rt");
        rt.block_on(SimStreamWriter::connect(&cfg))
    })
    .join()
    .unwrap_or_else(|_| Err("SimStreamWriter init thread panicked".to_string()));

    match result {
        Ok(writer) => {
            info!("SimStreamWriter: persistent connection ready.");
            commands.insert_resource(SimWriterResource(Arc::new(writer)));
        }
        Err(e) => {
            warn!(
                "SimStreamWriter: Iggy unavailable ({e}). \
                 Simulation records will use fallback one-shot connects."
            );
        }
    }
}

