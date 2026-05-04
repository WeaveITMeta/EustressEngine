// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
#[allow(unused_imports)]
use bevy::render::RenderPlugin;
use bevy::gltf::{GltfExtras, GltfSceneExtras, GltfMeshExtras, GltfMaterialExtras};
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin, EntityCountDiagnosticsPlugin};
// Window icon: embedded in exe via winres (build.rs), runtime set in setup_slint_overlay
use crate::plugins::lighting_plugin::LightingPlugin;
use eustress_common::services::{TeamServicePlugin, PlayerService};

#[cfg(feature = "streaming")]
use std::sync::Arc;
#[cfg(feature = "streaming")]
use eustress_common::change_queue::{ChangeQueueConfig, StreamingPlugin};
#[cfg(feature = "streaming")]
use eustress_common::sim_stream::SimStreamWriter;

mod auth;
mod forge;
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
mod move_handles;
mod scale_handles;
mod rotate_handles;
mod adornment_renderer;
mod modal_tool;
mod numeric_input;
mod align_distribute;
mod array_tools;
mod measure_tool;
mod duplicate_place_tool;
mod selection_sets;
mod pivot_mode;
mod geom_snap;
mod smart_guides;
mod mirror_link;
mod part_to_terrain;
mod lasso_paint_select;
mod saved_viewpoints;
mod attachment_editor_tool;
mod constraint_editor_tool;
mod transform_constraints;
mod toast_undo;
mod commit_flash;
mod embedvec_dispatch;
mod rune_tool_sandbox;
mod mesh_import;
mod cursor_badge;
mod timeline_panel;
mod timeline_slint_sync;
mod timeline_animation;
mod attribute_tag_migration;
mod accessibility;
mod tools_smart;
mod selection_sync;
mod editor_settings;
mod undo;
mod history_stream;
mod soul_script_migration;
mod notifications;
mod keybindings;
mod part_selection;
mod transform_space;
mod clipboard;
mod material_sync;
mod lock_tool;
mod video;
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
mod network_benchmark;  // Stress test with sysinfo hardware detection
mod updater;            // In-app self-update system

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
use eustress_engine::script_editor;
use window_focus::WindowFocusPlugin;
use startup::{StartupPlugin, StartupArgs};
// ServicePropertiesPlugin removed - now handled by Slint UI
use soul::EngineSoulPlugin;
use workshop::WorkshopPlugin;
use space::SpaceFileLoaderPlugin;
use space::{SpaceRoot, UniverseRegistryPlugin};

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
    
    // Rate-limited error handler: logs each unique error source once, then suppresses repeats.
    // The default `warn` handler spams hundreds of lines per frame. `ignore` hides everything
    // and makes debugging impossible. This handler shows each error once so you know what's
    // broken without drowning in log output.
    app.set_error_handler(rate_limited_error_handler);
    
    // Register the Space asset source BEFORE DefaultPlugins
    // This must happen before AssetPlugin is initialized
    let space_root = space::default_space_root();
    info!("📁 Registering Space asset source at: {:?}", space_root);
    app.register_asset_source(
        "space",
        bevy::asset::io::AssetSourceBuilder::platform_default(&space_root.to_string_lossy(), None),
    );

    // Register bundled common assets (material textures, fonts, etc.)
    let common_assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../common/assets");
    info!("📦 Registering bundled asset source at: {:?}", common_assets);
    app.register_asset_source(
        "bundled",
        bevy::asset::io::AssetSourceBuilder::platform_default(&common_assets.to_string_lossy(), None),
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
        // LogDiagnosticsPlugin disabled — FPS/frame_time shown in Slint overlay instead
        // .add_plugins(LogDiagnosticsPlugin { wait_duration: std::time::Duration::from_secs(5), ..default() })
        // Register GLTF types for scene spawning (prevents panic on unregistered types)
        .register_type::<GltfExtras>()
        .register_type::<GltfSceneExtras>()
        .register_type::<GltfMeshExtras>()
        .register_type::<GltfMaterialExtras>()
        // PlayerService for play mode character spawning
        .init_resource::<PlayerService>()
        // Startup args
        .insert_resource(args.clone())
        // NotificationManager resource — always registered. The toast bridge
        // system inside this plugin is itself gated on `notifications`.
        .add_plugins(NotificationPlugin)
        // Undo/Redo (must be before SlintUiPlugin which uses UndoStack)
        .add_plugins(UndoPlugin)
        // Tees every UndoStack push onto the `history.<kind>` topic so
        // MCP / LSP / CLI subscribers see the edit log in real-time.
        .add_plugins(history_stream::HistoryStreamPlugin)
        // One-shot migration: promotes legacy flat `SoulService/*.rune`
        // files to the folder-per-script convention on space load.
        .add_plugins(soul_script_migration::SoulScriptMigrationPlugin)
        // (Streaming registered below — cfg-block required, not inline chain)
        // Play mode (must be before SlintUiPlugin which uses PlayModeState)
        .add_plugins(PlayModePlugin)
        // Script analyzer (Rune diagnostics + symbol index on AsyncComputeTaskPool)
        .add_plugins(script_editor::ScriptAnalysisPlugin)
        // Runtime snapshot — writes live play-state + sim values to
        // `<universe>/.eustress/runtime-snapshot.json` at 4 Hz so the
        // LSP (separate process) can surface live values in hover.
        .add_plugins(script_editor::runtime_snapshot::RuntimeSnapshotPlugin)
        // LSP child-process launcher — spawns `eustress-lsp --tcp` so
        // external IDEs can connect to a live server while Studio is up.
        // No-op if the companion binary isn't on disk.
        .add_plugins(eustress_engine::lsp_launcher::LspLauncherPlugin)
        // Engine Bridge — JSON-RPC 2.0 over localhost TCP for sibling
        // processes (MCP server, plugins) to query live ECS / sim /
        // embedvec. Port handoff via `<universe>/.eustress/engine.port`.
        .add_plugins(eustress_engine::engine_bridge::EngineBridgePlugin)
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
        .add_plugins(lock_tool::LockToolPlugin)
        .add_plugins(video::VideoPlugin)
        // Lighting — SharedLightingPlugin (sun/ambient/skybox) + engine-side
        // hydrate_lighting_entities (attaches DirectionalLight, markers, etc.
        // to file-loaded Lighting/ Instance entities on each Space switch).
        .add_plugins(LightingPlugin)
        // Analytical sun/moon disc shader (resolution-independent, replaces cubemap baking)
        .add_plugins(shaders::SunDiscPlugin)
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
        // Mesh optimization (runtime meshopt on loaded GLBs)
        .add_plugins(eustress_engine::mesh_optimizer::MeshOptPlugin)
        // Slint-based in-game GUI rendering (ScreenGui, BillboardGui, SurfaceGui)
        .add_plugins(eustress_common::gui::SlintGuiPlugin)
        // BillboardGui: per-entity Slint BillboardCard software-rendered onto
        // a 3D quad. Replaces the deprecated fontdue/atlas path; the legacy
        // BillboardRendererPlugin in `common` is now a no-op shim.
        .add_plugins(eustress_engine::billboard_gui::BillboardGuiPlugin)
        // Adornments — Roblox-style mesh-based tool handles. Registers the
        // HandleAdornment / BoxHandle / ConeHandle / CylinderHandle /
        // ArcHandles / Handles component types so tools can spawn them and
        // the renderer can attach meshes.
        .add_plugins(eustress_common::adornments::AdornmentPlugin)
        // Adornment renderer — watches Added<*HandleAdornment> markers and
        // attaches the right Mesh3d + MeshMaterial3d + NotShadowCaster. Keeps
        // the tool code free of mesh-asset details.
        .add_plugins(adornment_renderer::AdornmentRendererPlugin)
        // Selection box
        .add_plugins(SelectionBoxPlugin)
        // Tools
        .add_plugins(SelectToolPlugin)
        .add_plugins(MoveToolPlugin)
        .add_plugins(RotateToolPlugin)
        .add_plugins(ScaleToolPlugin)
        // Mesh-based Move handles (replaces gizmo-based draw_move_gizmos).
        .add_plugins(move_handles::MoveHandlesPlugin)
        // Mesh-based Scale handles (replaces gizmo-based draw_scale_gizmos).
        .add_plugins(scale_handles::ScaleHandlesPlugin)
        // Mesh-based Rotate handles — torus rings per axis.
        .add_plugins(rotate_handles::RotateHandlesPlugin)
        // Modal tool framework — ModalTool trait, ActiveModalTool
        // resource, ToolOptionsBarState reflection, activation/cancel
        // event handlers. Required by every Smart Build Tool.
        .add_plugins(modal_tool::ModalToolPlugin)
        // Floating Numeric Input — live numeric entry during gizmo drag.
        // Blender / Maya parity: type `2.5 <Enter>` during a Move axis
        // drag to commit exactly 2.5 units. Independent of ModalTool —
        // operates on Move/Scale/Rotate drag state directly.
        .add_plugins(numeric_input::NumericInputPlugin)
        // Align & Distribute — last Phase-0 tool feature. Event-driven
        // (`AlignEntitiesEvent` / `DistributeEntitiesEvent`); fired
        // from ribbon buttons + keybindings. Uses the same signed-write
        // TOML persistence path Move does.
        .add_plugins(align_distribute::AlignDistributePlugin)
        // Array Tools (Phase 1) — Linear / Radial / Grid array
        // ModalTool implementations. Registered with ModalToolRegistry
        // at startup; activated via CAD-tab Pattern group or keybinding.
        .add_plugins(array_tools::ArrayToolsPlugin)
        // Measure distance tool (Phase 1). Pure read-only ModalTool —
        // click two viewport points, get a distance readout.
        .add_plugins(measure_tool::MeasureToolPlugin)
        // Duplicate & Place (Phase 1) — clone selection, follow-cursor
        // placement on click. Repeatable until user Esc.
        .add_plugins(duplicate_place_tool::DuplicatePlaceToolPlugin)
        // Selection Sets (Phase 1) — named, persistent selections per
        // universe. Save/Load/Delete events, TOML-backed storage at
        // `.eustress/selection_sets.toml`.
        .add_plugins(selection_sets::SelectionSetsPlugin)
        // Pivot Modes (Phase 1) — Median/Active/Individual/Cursor.
        // v1 ships the resource + events + helper; per-tool drag-math
        // integration to honor non-Median modes lands in follow-ups.
        .add_plugins(pivot_mode::PivotModePlugin)
        // Vertex / Edge / Face Snap (Phase 1) — hold V/E/F during
        // drag to force the snap category. v1 ships resolver +
        // modifier-key detection; Move-tool integration to actually
        // apply the snap during drag is a follow-up.
        .add_plugins(geom_snap::GeomSnapPlugin)
        // Smart Alignment Guides (Phase 1) — per-frame AABB plane
        // sensor. v1 scans all unselected parts; R-tree acceleration
        // lands in v2 when universe size warrants.
        .add_plugins(smart_guides::SmartGuidesPlugin)
        // Model Reflect Linked (Phase 1) — live-mirror link propagation.
        // When ModelReflect's "Linked" option is enabled, it inserts a
        // MirrorLink on each clone; the runtime keeps the pair in sync.
        .add_plugins(mirror_link::MirrorLinkPlugin)
        // Part to Terrain (Phase 1 scaffold) — event + handler skeleton.
        // Actual voxel rasterization lands in a follow-up using the
        // common/terrain chunk APIs.
        .add_plugins(part_to_terrain::PartToTerrainPlugin)
        // Lasso + Paint Select (Phase 2) — screen-space selection
        // gestures. Events + handlers ship; cursor-sample collection
        // UI wiring lives in select_tool / MCP.
        .add_plugins(lasso_paint_select::LassoPaintSelectPlugin)
        // Saved Viewpoints (Phase 2) — named camera poses persisted
        // to `.eustress/viewpoints.toml` per universe.
        .add_plugins(saved_viewpoints::SavedViewpointsPlugin)
        // Attachment Editor (Phase 2) — click-to-place `Attachment`
        // children on part surfaces, oriented to hit normal.
        .add_plugins(attachment_editor_tool::AttachmentEditorPlugin)
        // Constraint Editor (Phase 2) — visual joint authoring.
        .add_plugins(constraint_editor_tool::ConstraintEditorPlugin)
        // Transform Constraints (Phase 2) — non-physical authoring
        // constraints: AlignToAxis, DistributeAlong, LockAxis.
        .add_plugins(transform_constraints::TransformConstraintsPlugin)
        // Toast Undo (UX polish) — surfaces a top-center toast with
        // inline Undo on labeled commits.
        .add_plugins(toast_undo::ToastUndoPlugin)
        // Commit-success flash (UX polish) — 150ms accent-green-bright
        // border pulse anchored to ToolOptionsBar on every commit.
        .add_plugins(commit_flash::CommitFlashPlugin)
        // Embedvec dispatcher (UX + AI) — routes MCP tool calls into
        // EmbedvecResource lookups + emits typed results back to UI.
        .add_plugins(embedvec_dispatch::EmbedvecDispatchPlugin)
        // Rune tool sandbox (Phase 2) — script-authored ModalTools.
        // Registration is runtime via `RegisterRuneToolEvent`; VM
        // callback routing is the follow-up.
        .add_plugins(rune_tool_sandbox::RuneToolSandboxPlugin)
        // Mesh Import Watcher — auto-converts STL / STEP / OBJ /
        // PLY / FBX files dropped into a Space to canonical GLB,
        // hides the source from the Explorer view.
        .add_plugins(mesh_import::MeshImportWatcherPlugin)
        // Cursor Badge — in-viewport cursor-follower. Workaround for
        // the Slint OS-cursor blocker.
        .add_plugins(cursor_badge::CursorBadgePlugin)
        // Timeline panel (Phase 2) — data-agnostic marker timeline.
        // Subscribes to the Stream topic `"timeline/*"`; shares the
        // bottom-panel slot with Output via `BottomPanelMode`.
        .add_plugins(timeline_panel::TimelinePanelPlugin)
        // Timeline → Slint sync. Separate plugin so the timeline
        // feature iterates without touching the 8k-line slint_ui.rs.
        .add_plugins(timeline_slint_sync::TimelineSlintSyncPlugin)
        // Timeline animation (Phase 2+) — keyframed + procedural
        // tracks playback via AnimationClock.
        .add_plugins(timeline_animation::TimelineAnimationPlugin)
        // Accessibility manifest — design-time ARIA-style role +
        // label registry. Populates immediately; applies to Slint's
        // accessibility tree when the upstream API lands.
        .add_plugins(accessibility::AccessibilityPlugin)
        // Attribute + Tag migration — ensures every non-service
        // instance's TOML has `[attributes]` + `[tags]` sections.
        // Fire `RunAttributeTagMigrationEvent` to invoke (Settings
        // UI + scripted tests wire the event).
        .add_plugins(attribute_tag_migration::AttributeTagMigrationPlugin)
        // Smart Build Tools (Gap Fill, Resize Align, Edge Align,
        // Part Swap, Model Reflect). Each registers its factory with
        // ModalToolRegistry.
        .add_plugins(tools_smart::SmartToolsPlugin)
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
        .add_plugins(simulation::ElectrochemistryPlugin)
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
        // Soul scripting + physics bridge
        .add_plugins(EngineSoulPlugin)
        .add_plugins(soul::physics_bridge::RunePhysicsBridgePlugin)
        .add_plugins(soul::gui_bridge::GuiBridgePlugin)
        .add_plugins(ui::rune_ecs_bindings::RuneECSBindingsPlugin)
        // Workshop (System 0: Ideation — conversational product creation)
        .add_plugins(WorkshopPlugin)
        // In-app updater (checks releases.eustress.dev on startup)
        .add_plugins(updater::UpdaterPlugin)
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
        // Universe registry (periodic Universe→Space tree scan)
        .add_plugins(UniverseRegistryPlugin)
        // Startup
        .add_plugins(StartupPlugin)
        // Window title: derives "Universe > Space - Eustress Engine" from SpaceRoot
        .add_systems(Update, update_window_title)
        // Studio plugins
        .add_plugins(studio_plugins::StudioPluginSystem)
        // Frame diagnostics to identify stutters
        .add_plugins(frame_diagnostics::FrameDiagnosticsPlugin);
        
    // Left-click part selection with raycasting
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.add_systems(Update, part_selection::part_selection_system
            .after(ui::slint_ui::SlintSystems::Drain)
            .after(ui::slint_ui::update_slint_ui_focus));
    }

    // Streaming — must use a separate block because #[cfg] cannot gate
    // individual method calls inside a builder chain.
    #[cfg(feature = "streaming")]
    {
        app.add_plugins(StreamingPlugin);
        app.add_systems(Startup, setup_sim_stream_writer);

        // Cross-process pub/sub over TCP. The in-process EustressStream
        // (set up by StreamingPlugin above) is exposed on 33000+ so MCP,
        // LSP, visualizers, and remote agents can subscribe to live
        // scene_deltas / mcp.entity.* / sim_watchpoints / etc. without
        // polling filesystem snapshots. The plugin itself writes the
        // primary TCP port to `<universe>/.eustress/engine.stream.port`
        // after startup so siblings can discover it.
        app.add_plugins(eustress_engine::stream_node_plugin::StreamNodePlugin::default());
    }

    app.run();
    
    println!("✅ Eustress Engine closed gracefully");
}

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamWriter — one persistent connection, shared via Arc<Resource>
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Window title — "Universe > Space - Eustress Engine"
// ─────────────────────────────────────────────────────────────────────────────

/// Update the primary window title whenever `SpaceRoot` changes.
fn update_window_title(
    space_root: Option<Res<SpaceRoot>>,
    mut windows: Query<&mut bevy::window::Window, With<bevy::window::PrimaryWindow>>,
) {
    let Some(sr) = space_root else { return };
    if !sr.is_changed() { return; }

    let title = derive_window_title(&sr.0);
    for mut window in &mut windows {
        window.title = title.clone();
    }
}

fn derive_window_title(space_path: &std::path::Path) -> String {
    let space_name = space_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string());

    // Walk up: if immediate parent is "spaces/", skip it to get the Universe folder.
    let universe_name = space_path.parent().and_then(|p| {
        let pname = p.file_name()?.to_string_lossy().to_string();
        if pname == "Spaces" || pname == "spaces" {
            p.parent()?.file_name().map(|n| n.to_string_lossy().to_string())
        } else {
            Some(pname)
        }
    });

    match universe_name {
        Some(u) => format!("{u} > {space_name} - Eustress Engine"),
        None => format!("{space_name} - Eustress Engine"),
    }
}

/// Re-import from lib so this binary can insert the resource.
#[cfg(feature = "streaming")]
use eustress_engine::SimWriterResource;

/// Startup system: connect `SimStreamWriter` once and insert as a Bevy Resource.
///
/// Task 10 call sites (`run_simulation`, `process_feedback`, `execute_and_apply`)
/// read `Option<Res<SimWriterResource>>` and pass `Some(writer.0.clone())` to the
/// `publish_*_sync` helpers, replacing the `None` fallback connect.
///
/// Silently skipped if streaming is unavailable — engine continues without streaming.
#[cfg(feature = "streaming")]
fn setup_sim_stream_writer(mut commands: Commands, config: Option<Res<ChangeQueueConfig>>) {
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
                "SimStreamWriter: streaming unavailable ({e}). \
                 Simulation records will use fallback one-shot connects."
            );
        }
    }
}

/// Rate-limited error handler: logs each unique error source ONCE, then suppresses.
/// This replaces both `ignore` (hides everything) and `warn` (spams every frame).
/// Uses a static HashSet to track which system/command names have already been reported.
fn rate_limited_error_handler(error: bevy::ecs::error::BevyError, ctx: bevy::ecs::error::ErrorContext) {
    use std::sync::Mutex;
    use std::collections::HashSet;

    static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    let key = format!("{}", ctx);

    let mut guard = SEEN.lock().unwrap();
    let seen = guard.get_or_insert_with(HashSet::new);

    if seen.insert(key.clone()) {
        // First time seeing this error source — log it
        warn!("⚠ {} : {}", ctx, error);
    }
    // Subsequent occurrences are silently ignored
}

