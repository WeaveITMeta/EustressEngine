// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
#[allow(unused_imports)]
use bevy::render::RenderPlugin;
use bevy::winit::WinitWindows;
use bevy_egui::EguiPlugin;
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

use ui::StudioUiPlugin;
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
use ui::ServicePropertiesPlugin;
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
    
    app // Bevy plugins with optimized window settings
        .add_plugins(DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: window_title,
                    resolution: bevy::window::WindowResolution::new(1600, 900),
                    // Force VSync to prevent screen tearing
                    present_mode: bevy::window::PresentMode::Fifo,
                    mode: bevy::window::WindowMode::Windowed,
                    decorations: true,
                    resizable: true,
                    // Icon is set via build.rs (winres) on Windows
                    ..default()
                }),
                // Disable automatic window close - we handle it manually for unsaved changes prompt
                close_when_requested: false,
                ..default()
            })
            // Use common assets folder for shared character models and animations
            .set(AssetPlugin {
                file_path: "../common/assets".to_string(),
                ..default()
            })
        )
        // NOTE: WireframePlugin removed - causes "node PostProcessing does not exist" crash in Bevy 0.17
        // TODO: Re-enable when Bevy fixes the render graph dependency issue
        // .add_plugins(bevy::pbr::wireframe::WireframePlugin::default())
        // .insert_resource(bevy::pbr::wireframe::WireframeConfig {
        //     global: false,
        //     default_color: bevy::color::Color::WHITE,
        // })
        // Explorer expand/collapse state
        .init_resource::<ui::ExplorerExpanded>()
        .init_resource::<ui::ExplorerState>()
        // PlayerService for play mode character spawning
        .init_resource::<PlayerService>()
        // Startup args (parsed above) - must be inserted early for DefaultScenePlugin
        .insert_resource(args.clone())
        // egui plugin for UI
        .add_plugins(EguiPlugin::default())
        // Studio UI (panels, tools, etc.)
        .add_plugins(StudioUiPlugin {
            selection_manager: selection_manager.clone(),
            transform_manager: transform_manager.clone(),
        })
        // 3D rendering (uses ECS queries)
        .add_plugins(PartRenderingPlugin {
            selection_manager: selection_manager.clone(),
            transform_manager: transform_manager.clone(),
        })
        // Material sync (real-time property changes to materials)
        .add_plugins(MaterialSyncPlugin)
        // Shared lighting (same as client) - skybox, sun, ambient
        .add_plugins(SharedLightingPlugin)
        // Default scene setup (camera, baseplate, welcome cube)
        .add_plugins(DefaultScenePlugin)
        // Camera controls (orbit, pan, zoom)
        .add_plugins(CameraControllerPlugin)
        .add_systems(Startup, setup_camera_controller.after(default_scene::setup_default_scene))
        // Editor settings (snap, grid, auto-save)
        .add_plugins(EditorSettingsPlugin)
        // Keybindings system
        .add_plugins(KeyBindingsPlugin)
        // Clipboard system (copy/paste)
        .add_plugins(ClipboardPlugin)
        // Workspace service (gravity, bounds, physics settings)
        .add_plugins(WorkspacePlugin)
        // Service properties (Workspace, Lighting, Players, etc.)
        .add_plugins(ServicePropertiesPlugin)
        // Transform space (World/Local) system
        .add_plugins(TransformSpacePlugin)
        // Undo/Redo system
        .add_plugins(UndoPlugin)
        // Toast notifications
        .add_plugins(NotificationPlugin)
        // Transform gizmos (visual tool indicators)
        .add_plugins(GizmoToolsPlugin)
        // Selection box visuals (Roblox-like highlighting)
        .add_plugins(SelectionBoxPlugin)
        // Transformation tools
        .add_plugins(SelectToolPlugin)
        .add_plugins(MoveToolPlugin)
        .add_plugins(RotateToolPlugin)
        .add_plugins(ScaleToolPlugin)
        // Synchronize selection state with visual components
        .add_plugins(SelectionSyncPlugin {
            selection_manager: selection_manager.clone(),
        })
        // Terrain system with editor UI
        .add_plugins(EngineTerrainPlugin)
        // Physics (Avian3D) - needed for play mode character physics
        .add_plugins(avian3d::PhysicsPlugins::default())
        .insert_resource(avian3d::prelude::Gravity(bevy::math::Vec3::NEG_Y * 9.80665))
        // Gamepad/Controller service (connection events, input state, notifications)
        .add_plugins(eustress_common::services::GamepadServicePlugin)
        // Notification UI (toast messages for gamepad connect/disconnect, etc.)
        .add_plugins(ui::notifications::NotificationPlugin)
        // Play mode (F5 to play with character, F7 solo, F8 stop)
        .add_plugins(PlayModePlugin)
        // In-process server + client for Play Server mode
        .add_plugins(play_server::PlayServerPlugin)
        // Embedded client runtime (same codebase as standalone client)
        .add_plugins(embedded_client::EmbeddedClientPlugin)
        // Team system (team colors, spawn filtering, etc.)
        .add_plugins(TeamServicePlugin)
        // Runtime systems (physics events, lighting time, script lifecycle)
        .add_plugins(runtime::RuntimePlugin)
        // Seat systems (auto-sit, controller input for vehicles)
        .add_plugins(seats::SeatPlugin)
        // Soul scripting (Claude API, hot compile, global settings)
        .add_plugins(EngineSoulPlugin)
        // Generative AI pipeline (Soul/Abstract modes, text-to-mesh)
        .add_plugins(generative_pipeline::GenerativePipelinePlugin)
        // VIGA: Vision-as-Inverse-Graphics Agent (image-to-scene)
        .add_plugins(viga::VigaPlugin)
        // MoonDisk rendering - DISABLED: Bevy's atmosphere shader already renders the moon
        // .add_plugins(shaders::MoonDiskPlugin)
        // IoManager for async data fetching (Parameters system)
        .add_plugins(io_manager::IoManagerPlugin)
        // Telemetry (opt-in error reporting)
        .add_plugins(telemetry::TelemetryPlugin)
        // Window focus management (reduce CPU when unfocused)
        .add_plugins(WindowFocusPlugin)
        // Startup handling (command-line args, file associations, scene loading)
        .add_plugins(StartupPlugin)
        // Studio Plugin System (MindSpace, custom plugins)
        .add_plugins(studio_plugins::StudioPluginSystem);
        
    // Left-click part selection with raycasting (native only) - MODERN ECS!
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.add_systems(Update, part_selection::part_selection_system);
    }
    
    // Set window icon after window is created (Windows/Linux taskbar and title bar icon)
    app.add_systems(PostStartup, set_window_icon);
    
    app.run();
    
    println!("✅ Eustress Engine closed gracefully");
}

/// Set the window icon for taskbar and title bar (Windows/Linux)
/// Uses the embedded icon.png from assets folder
fn set_window_icon(
    windows: Option<NonSend<WinitWindows>>,
) {
    let Some(windows) = windows else {
        return;
    };
    
    // Load icon from embedded bytes or file
    let icon_bytes = include_bytes!("../assets/icon.png");
    
    let image = match image::load_from_memory(icon_bytes) {
        Ok(img) => img.into_rgba8(),
        Err(e) => {
            warn!("Failed to load window icon: {}", e);
            return;
        }
    };
    
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    
    let icon = match winit::window::Icon::from_rgba(rgba, width, height) {
        Ok(icon) => icon,
        Err(e) => {
            warn!("Failed to create window icon: {}", e);
            return;
        }
    };
    
    // Set icon for all windows
    for window in windows.windows.values() {
        window.set_window_icon(Some(icon.clone()));
    }
    
    info!("✅ Window icon set successfully");
}
