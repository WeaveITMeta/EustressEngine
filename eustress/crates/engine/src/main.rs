// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
#[allow(unused_imports)]
use bevy::render::RenderPlugin;
// Window icon: embedded in exe via winres (build.rs), runtime set in setup_slint_overlay
use eustress_common::plugins::lighting_plugin::SharedLightingPlugin;
use eustress_common::services::{TeamServicePlugin, PlayerService};

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
mod io_manager;         // Async data fetching for Parameters

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
                ..default()
            })
            .set(AssetPlugin {
                file_path: "../common/assets".to_string(),
                ..default()
            })
        )
        // PlayerService for play mode character spawning
        .init_resource::<PlayerService>()
        // Startup args
        .insert_resource(args.clone())
        // Notifications (must be before SlintUiPlugin which uses NotificationManager)
        .add_plugins(NotificationPlugin)
        // Undo/Redo (must be before SlintUiPlugin which uses UndoStack)
        .add_plugins(UndoPlugin)
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
        .add_plugins(studio_plugins::StudioPluginSystem);
        
    // Left-click part selection with raycasting
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.add_systems(Update, part_selection::part_selection_system);
    }
    
    app.run();
    
    println!("✅ Eustress Engine closed gracefully");
}

