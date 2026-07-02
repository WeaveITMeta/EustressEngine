// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
#[allow(unused_imports)]
use bevy::render::RenderPlugin;
use bevy::gltf::{GltfExtras, GltfSceneExtras, GltfMeshExtras, GltfMaterialExtras, GltfMeshName, GltfMaterialName};
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin, EntityCountDiagnosticsPlugin};
// Window icon: embedded in exe via winres (build.rs), runtime set in setup_slint_overlay
use eustress_engine::plugins::lighting_plugin::LightingPlugin;
use eustress_common::services::{TeamServicePlugin, PlayerService};

#[cfg(feature = "streaming")]
use std::sync::Arc;
#[cfg(feature = "streaming")]
use eustress_common::change_queue::{ChangeQueueConfig, StreamingPlugin};
#[cfg(feature = "streaming")]
use eustress_common::sim_stream::SimStreamWriter;

// ── Thin-bin module imports (dual-compile untangling, 2026-07-02) ────
// The engine used to DUAL-COMPILE ~104 modules: declared `mod X;` here
// AND `pub mod X;` in lib.rs, producing TWO instances of every type
// with different TypeIds. Systems added from one instance never saw
// resources/messages registered by the other — engine_bridge had to be
// made bin-local to see any resource at all, and billboard_gui's
// DoubleClickedPart readers (lib) never received part_selection's
// writes (bin), leaving double-click billboard editing silently dead.
// The bin is now a THIN SHELL over the lib: one compilation of every
// module, one TypeId universe, and the engine compiles ONCE instead of
// twice. The five formerly bin-only modules (engine_bridge,
// history_stream, light_sync, photoreal, soul_script_migration) were
// promoted to lib.rs — engine_bridge in the lib is also the
// HEADLESS_RUNTIME plan's keystone.
#[allow(unused_imports)]
use eustress_engine::{
    accessibility, adornment_renderer, ai_camera, align_distribute, array_tools,
    attachment_editor_tool, attribute_tag_migration, auth, bliss_tracker, camera,
    camera_controller, class_registry, classes, clipboard, commands, commit_flash,
    constraint_editor_tool, cursor_badge, default_scene, duplicate_place_tool,
    editor_settings, embedded_client, embedvec_dispatch, engine_bridge, entity_utils,
    forge, frame_diagnostics, generative_pipeline, geom_snap, gizmo_tools, grouping,
    history_stream, interaction, io_manager, keybindings, lasso_paint_select,
    light_cull, light_sync, lock_tool, manufacturing, material_sync, math_utils,
    measure_tool, mesh_import, mirror_link, modal_tool, move_handles, move_tool,
    network_benchmark, notifications, numeric_input, part_selection, part_to_terrain,
    parts, photoreal, physics, pivot_mode, play_mode, play_mode_runtime, play_server,
    plugins, profiler, properties, rendering, rotate_handles, rotate_tool,
    rune_tool_sandbox, runtime, saved_viewpoints, scale_handles, scale_tool, scenes,
    seats, select_tool, selection_box, selection_sets, selection_sync, serialization,
    shaders, simulation, smart_guides, soul, soul_script_migration, space,
    spatial_query_bridge, spawn, spawners, startup, studio_plugins, telemetry,
    terrain_plugin, timeline_animation, timeline_panel, timeline_slint_sync,
    toast_undo, toolbox, tools_smart, transform_constraints, transform_space,
    txt_to_toml_watcher, ui, undo, updater, video, viga, window_focus, workshop,
};
// Wave 9.C — imported-terrain voxel loader. Whole module is
// `#![cfg(feature = "world-db")]` in the lib; mirror the gate here.
#[cfg(feature = "world-db")]
#[allow(unused_imports)]
use eustress_engine::terrain_voxel_load;

use eustress_engine::rendering::PartRenderingPlugin;
use eustress_engine::commands::{SelectionManager, TransformManager}; // Production-ready managers
use eustress_engine::default_scene::DefaultScenePlugin;
use eustress_engine::plugins::WorkspacePlugin;
use eustress_engine::camera_controller::{CameraControllerPlugin, setup_camera_controller};
use eustress_engine::gizmo_tools::GizmoToolsPlugin;
use eustress_engine::selection_box::SelectionBoxPlugin;
use eustress_engine::select_tool::SelectToolPlugin;
use eustress_engine::move_tool::MoveToolPlugin;
use eustress_engine::transform_space::TransformSpacePlugin;
use eustress_engine::rotate_tool::RotateToolPlugin;
use eustress_engine::scale_tool::ScaleToolPlugin;
use eustress_engine::selection_sync::SelectionSyncPlugin;
use eustress_engine::editor_settings::EditorSettingsPlugin;
use eustress_engine::undo::UndoPlugin;
use eustress_engine::notifications::NotificationPlugin;
use eustress_engine::keybindings::KeyBindingsPlugin;
use eustress_engine::clipboard::ClipboardPlugin;
use eustress_engine::grouping::GroupingPlugin;
use eustress_engine::material_sync::MaterialSyncPlugin;
use eustress_engine::terrain_plugin::EngineTerrainPlugin;
use eustress_engine::play_mode::PlayModePlugin;
use eustress_engine::script_editor;
use eustress_engine::window_focus::WindowFocusPlugin;
use eustress_engine::startup::{StartupPlugin, StartupArgs};
// ServicePropertiesPlugin removed - now handled by Slint UI
use eustress_engine::soul::EngineSoulPlugin;
use eustress_engine::workshop::WorkshopPlugin;
use eustress_engine::space::SpaceFileLoaderPlugin;
use eustress_engine::space::{SpaceRoot, UniverseRegistryPlugin};

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
    // This must happen before AssetPlugin is initialized.
    //
    // Use a RUNTIME-SWAPPABLE reader (`DynamicSpaceReader`) instead of
    // `platform_default(&launch_root)`. `platform_default` bakes the launch
    // Space root into a FileAssetReader once; after a Space/Universe switch the
    // reader keeps resolving `space://` paths against the OLD root → "Path not
    // found: ...\<old space>\...\*.glb" → no meshes → black screen. The dynamic
    // reader resolves the live root (`space_asset_source::space_asset_root()`)
    // on every read, and `sync_space_asset_root_on_change` keeps that global in
    // step with `SpaceRoot` on every switch. The `space://` source is
    // read-only (all call sites only `asset_server.load(...)`), and the Bevy
    // asset watcher is gated on the `file_watcher` cargo feature (not enabled
    // here — hot-reload uses the engine's own `notify` watcher), so no
    // writer/watcher is needed.
    let space_root = space::default_space_root();
    info!("📁 Registering Space asset source at: {:?}", space_root);
    // Seed the swappable global to the launch root before AssetPlugin runs.
    space::space_asset_source::set_space_asset_root(space_root.clone());
    app.register_asset_source(
        "space",
        // `AssetSourceBuilder::new(reader_factory)` — reader-only source.
        // The factory is `FnMut() -> Box<dyn ErasedAssetReader>`; any
        // `T: AssetReader` auto-implements `ErasedAssetReader`. The explicit
        // return annotation forces the `Box<DynamicSpaceReader>` → trait-object
        // unsize coercion in the closure body (closures can otherwise pin the
        // concrete return type before coercion).
        bevy::asset::io::AssetSourceBuilder::new(
            || -> Box<dyn bevy::asset::io::ErasedAssetReader> {
                Box::new(space::space_asset_source::DynamicSpaceReader)
            },
        ),
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
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings {
                        // Request discrete GPU (NVIDIA/AMD) over integrated
                        power_preference: bevy::render::settings::PowerPreference::HighPerformance,
                        // Use all available backends (Vulkan/DX12/Metal)
                        backends: Some(bevy::render::settings::Backends::all()),
                        ..default()
                    }
                )), // 0.19: RenderCreation::Automatic now takes Box<WgpuSettings>
                // Compile all shader pipelines synchronously to prevent mid-session
                // GPU pipeline stall stutters (750ms spikes visible in frame diagnostics)
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .set(AssetPlugin {
                file_path: "assets".to_string(),
                ..default()
            })
            // Per-system frame micro-profiler hook (feature `profiling`).
            // `profiler::custom_layer` adds a tracing Layer to THIS subscriber
            // that times each Bevy `"system"` span. With the feature off it is
            // `|_| None`, so this `.set` is a no-op clone of the default
            // LogPlugin and changes nothing about logging. With the feature on
            // it still does nothing until `EUSTRESS_PROFILE` is set. All other
            // LogPlugin fields stay at their defaults (filter/level/fmt_layer).
            .set(bevy::log::LogPlugin {
                custom_layer: profiler::custom_layer,
                ..default()
            })
        )
        // Diagnostic plugins for FPS and performance profiling
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(EntityCountDiagnosticsPlugin::default())
        // Periodic console FPS / frame-time / entity-count readout (~2s
        // interval) so the perf "quick wins" below can be validated with
        // real numbers in the log. Lightweight — does NOT enable the heavy
        // `bevy/trace` feature; just prints the already-collected
        // FrameTimeDiagnostics + EntityCountDiagnostics. The Slint overlay
        // still shows live FPS too; this adds a loggable trail.
        .add_plugins(LogDiagnosticsPlugin {
            wait_duration: std::time::Duration::from_secs(2),
            ..default()
        })
        // Register GLTF types for scene spawning (prevents panic on unregistered types)
        .register_type::<GltfExtras>()
        .register_type::<GltfSceneExtras>()
        .register_type::<GltfMeshExtras>()
        .register_type::<GltfMaterialExtras>()
        // Bevy 0.18 added this transform-propagation marker; it lands inside
        // every glb's scene graph, so `SceneSpawner` panics with "unregistered
        // type" on any imported mesh / CSG glb unless we register it here.
        // (This was crashing the engine on Vehicle Simulator's CSG glbs.)
        .register_type::<bevy::transform::components::TransformTreeChanged>()
        // glb scene graphs also carry hierarchy/transform/visibility/name
        // components, and `SceneSpawner` reflects EVERY one — so register the
        // full set up front (idempotent for any already registered by a plugin)
        // to end the one-panic-per-missing-type whack-a-mole on import.
        .register_type::<bevy::prelude::Children>()
        .register_type::<bevy::prelude::ChildOf>()
        .register_type::<bevy::prelude::Transform>()
        .register_type::<bevy::prelude::GlobalTransform>()
        .register_type::<bevy::prelude::Visibility>()
        .register_type::<bevy::prelude::InheritedVisibility>()
        .register_type::<bevy::prelude::ViewVisibility>()
        .register_type::<bevy::prelude::Name>()
        // Render components carried by glb mesh entities in the scene graph.
        .register_type::<bevy::prelude::Mesh3d>()
        .register_type::<bevy::prelude::MeshMaterial3d<bevy::pbr::StandardMaterial>>()
        // Aabb (frustum-culling bounds; moved to `bevy_camera` in Bevy 0.18) —
        // glb mesh entities carry it in the scene graph.
        .register_type::<bevy::camera::primitives::Aabb>()
        // gltf naming components the loader stamps on mesh/material scene
        // entities — the last category a static glb scene carries.
        .register_type::<GltfMeshName>()
        .register_type::<GltfMaterialName>()
        // PlayerService for play mode character spawning
        .init_resource::<PlayerService>()
        // DisplayUnit — user-selected display unit for the Properties
        // panel, status-bar readout, and Measure tool. Cosmetic only;
        // ECS / Avian / disk stay in engine-native meters regardless.
        // Defaults to Meter (see Default impl).
        .init_resource::<eustress_common::units::DisplayUnit>()
        // Categorical color picker (status-bar widget) — session-scoped
        // state behind the ribbon's "Colors" badge. Cosmetic/session only;
        // no disk persistence this pass.
        .init_resource::<eustress_common::color_wheels::ActiveColorWheel>()
        .init_resource::<eustress_common::color_wheels::ColorFavorites>()
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
        // Bin-local (`engine_bridge`, not `eustress_engine::engine_bridge`)
        // so its handlers share the bin's `TypeId`s — see the `mod
        // engine_bridge` note above.
        .add_plugins(engine_bridge::EngineBridgePlugin)
        // Guarantee `SpaceRoot` is always a resource. On a fresh launch the
        // space loader resolves `default_space_root()` directly and only the
        // runtime space-SWITCH path inserts `SpaceRoot`; without this the
        // resource is absent at boot, breaking bridge handlers
        // (viewport.capture, tools.call) and the port-file resync. Default
        // = last-opened/default space; `--space`/`--universe` and runtime
        // switches override it (init_resource is a no-op if already set).
        .init_resource::<space::SpaceRoot>()
        // Keep the swappable `space://` asset root in lock-step with
        // `SpaceRoot`. This `Changed<SpaceRoot>` chokepoint is the
        // authoritative coverage: every Space-switch site (runtime
        // `open_space`, `--space`/`--universe` overrides, "Save As") mutates
        // the resource, so the asset reader can never be left resolving mesh
        // paths against a stale launch root. Without this, switching Space at
        // runtime black-screens because `space://*.glb` resolves under the
        // OLD Space folder.
        .add_systems(Update, space::space_asset_source::sync_space_asset_root_on_change)
        // Independent off-screen AI camera — the AI's own eyes (renders to an
        // image, never the window, so it can't displace the editor camera).
        .add_plugins(ai_camera::AiCameraPlugin)
        // Slint UI (software renderer overlay)
        .add_plugins(ui::slint_ui::SlintUiPlugin)
        // Studio auth + Bliss node — starts the local Bliss node API.
        // Must come after SlintUiPlugin (which owns `auth_poll_system`;
        // this plugin deliberately does not re-register it).
        .add_plugins(auth::StudioAuthPlugin)
        // Bliss contribution tracker — attributes real work (scene edits,
        // script edits, active time) to contribution buckets, submits them
        // to the witness for co-signing, and syncs the authoritative BLS
        // balance into the ribbon's top-right Bliss badge.
        .add_plugins(bliss_tracker::BlissTrackerPlugin)
        // Floating windows
        .add_plugins(ui::floating_windows::FloatingWindowsPlugin)
        // 3D rendering
        .add_plugins(PartRenderingPlugin {
            selection_manager: selection_manager.clone(),
            transform_manager: transform_manager.clone(),
        })
        // Material sync
        .add_plugins(MaterialSyncPlugin)
        // Light-class sync: Eustress light components -> real Bevy lights + live edit
        .add_plugins(light_sync::LightClassPlugin)
        // R1 photoreal: registers AutoExposurePlugin (DefaultPlugins omits it)
        // + PhotorealSettings. The post-effect COMPONENTS live in
        // studio_camera_bundle so editor + AI camera stay in lockstep.
        .add_plugins(photoreal::PhotorealPlugin)
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
        // Group (Ctrl+G) / Ungroup (Ctrl+U)
        .add_plugins(GroupingPlugin)
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
        // a 3D quad. The pipeline plugin owns the WGSL shader + render-graph
        // hookup for the Transparent3d phase (with proper depth-testing for
        // occlusion); the gui plugin owns the per-entity texture allocation
        // and the Slint-card→texture blit pump.
        .add_plugins(eustress_engine::billboard_pipeline::BillboardPipelinePlugin)
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
        // Physics (Avian 0.7 — runs at a fixed timestep)
        .add_plugins(avian3d::PhysicsPlugins::default())
        .insert_resource(avian3d::prelude::Gravity(bevy::math::Vec3::NEG_Y * 9.80665))
        // ── Determinism pins (C2) ──────────────────────────────────────────
        // Avian's step = Time<Fixed>.delta() * relative_speed. Pin the fixed
        // timestep explicitly (was relying on Bevy's version-dependent implicit
        // default) so per-step dt is a fixed contract ("Avian (Deterministic)").
        // 60 Hz matches the sim clock (common::simulation).
        .insert_resource(Time::<bevy::time::Fixed>::from_hz(60.0))
        // Pin substep count + solver config at Avian defaults so a future Avian
        // default change can't silently alter trajectories. (SubstepCount is in
        // avian3d::prelude; SolverConfig is not — use its full module path.)
        .insert_resource(avian3d::prelude::SubstepCount(6))
        .insert_resource(avian3d::dynamics::solver::SolverConfig::default())
        // Cap virtual-time max-delta to break the fixed-timestep DEATH SPIRAL.
        // Bevy's default (250 ms) lets `FixedUpdate` run ~16× per render frame at
        // low FPS to "catch up" — and the profiler showed EVERY Fixed-schedule
        // system (transform + collider propagation, mark_dirty_trees, …) running
        // ~16×/frame over the 301K-entity import, the dominant editor cost.
        // Clamping to ~2 fixed steps collapses that 16× → 2× (graceful slow-mo
        // instead of meltdown) and is harmless in the editor, where physics is
        // already paused.
        .insert_resource({
            let mut vt = Time::<bevy::time::Virtual>::default();
            vt.set_max_delta(std::time::Duration::from_millis(33));
            vt
        })
        // Determinism — registers the GlobalRngSeed resource (C7). Cheap,
        // resource-only; up before any simulation RNG consumer reads it.
        .add_plugins(eustress_common::physics::DeterminismPlugin)
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
        .add_plugins(frame_diagnostics::FrameDiagnosticsPlugin)
        // Opt-in per-system frame micro-profiler. Empty plugin unless the
        // `profiling` feature is on; even then it only captures when the
        // `EUSTRESS_PROFILE` env var is set. Complements the stutter detector
        // above by attributing the frame budget to individual systems and
        // writing eustress_profile.{txt,svg}.
        .add_plugins(profiler::ProfilerPlugin);

    // Gaussian Splatting / radiance-field rendering (battle plan:
    // docs/architecture/GAUSSIAN_SPLATTING_BATTLE_PLAN.md, Phase 0). Gated by the
    // default-off `gaussian-splatting` feature so the everyday editor build is
    // unaffected; wraps `bevy_gaussian_splatting` behind `eustress-radiance`.
    // Activate with `--features gaussian-splatting`.
    #[cfg(feature = "gaussian-splatting")]
    {
        app.add_plugins(eustress_radiance::RadiancePlugin);
        // Env-driven demo spawn (EUSTRESS_SPLAT=<cloud path>) for eyeballing the
        // Phase-0 render path. No-op when the var is unset.
        app.add_plugins(eustress_radiance::RadianceDemoPlugin);
        // Make splat clouds browsable: tag them with an Instance so the unified
        // Explorer sync lists + nests them under Workspace.
        app.add_systems(Update, tag_splats_for_explorer);
    }

    // WorldDb — Fjall-backed authoritative ECS store (2026-05-15 binary
    // pivot; memory project_eustress_binary_pivot). Gated by the
    // `world-db` feature so the engine still boots on TOML when the
    // feature is off. The plugin opens `<SpaceRoot>/world.fjalldb/`
    // and mirrors Transform writes alongside the legacy TOML path.
    #[cfg(feature = "world-db")]
    app.add_plugins(space::world_db_plugin::WorldDbPlugin);

    // Sim orchestration (Phase 6 engine seam + thin Phase 3 driver). Gated by
    // `sim-orchestration` (implies `world-db`), so the default build is
    // unaffected. Registered AFTER WorldDbPlugin so its `register(app)` (which
    // owns `ResidencyChainSet`) has run. DISTINCT from the connect-only
    // `ForgePlugin` (SDK game-server deployment) — separate crate, separate
    // concern. Activate with `--features sim-orchestration`.
    #[cfg(feature = "sim-orchestration")]
    app.add_plugins(space::sim_orchestration::SimOrchestrationPlugin);

    // Left-click part selection with raycasting
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.add_message::<part_selection::DoubleClickedPart>();
        app.add_systems(Update, part_selection::part_selection_system
            .after(ui::slint_ui::SlintSystems::Drain)
            .after(ui::slint_ui::update_slint_ui_focus));
        // Ctrl+Shift+Alt + mouse-wheel resizes the part under the cursor
        // (no click/selection). Runs after the UI-focus update so it sees
        // the authoritative cursor-over-viewport signal; fires
        // ResizePartEvent which ScaleToolPlugin applies.
        app.add_systems(Update, part_selection::hover_resize_system
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

    // Wrap App::run() so GPU surface-lost panics (swap chain unavailable,
    // uniform buffer unwrap, wgpu buffer invalid) don't show a crash dialog.
    // These happen when: window minimized → zero-size surface, GPU driver
    // TDR reset, or display mode change mid-frame. They are transient but
    // Bevy 0.18 panics instead of recovering. We catch them and exit cleanly.
    let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.run();
    }));

    match run_result {
        Ok(()) => {
            println!("✅ Eustress Engine closed gracefully");
        }
        Err(payload) => {
            let msg = payload.downcast_ref::<String>()
                .map(|s| s.as_str())
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("");

            let is_gpu_surface_panic =
                msg.contains("swap chain")
                || msg.contains("Acquiring a texture")
                || msg.contains("unrecoverable")
                || msg.contains("operation unrecoverable")
                || (msg.contains("None value")
                    && (msg.contains("uniform_buffer") || msg.contains("bevy_render")))
                || msg.contains("Buffer") && msg.contains("invalid");

            if is_gpu_surface_panic {
                // GPU surface was lost (minimized window, driver reset, display change).
                // This is not a code bug — exit cleanly without a crash dialog.
                eprintln!("⚠️  GPU surface lost — exiting cleanly (not a crash).");
                std::process::exit(0);
            } else {
                // Real panic — re-raise so dev gets a proper backtrace.
                std::panic::resume_unwind(payload);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamWriter — one persistent connection, shared via Arc<Resource>
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Window title — "Universe > Space - Eustress Engine"
// ─────────────────────────────────────────────────────────────────────────────

/// Make Gaussian-Splatting clouds appear in the Explorer. radiance spawns each
/// GPU cloud with only `Name` + `SplatCloud` (it cannot reference the engine's
/// scene-tree types), so the engine tags every newly-spawned cloud with an
/// `Instance` (class `GaussianSplats`) and parents it under Workspace — the two
/// things the unified-explorer sync (`Query<(Entity, &Instance)>`) needs to list
/// and nest it. Selectable + inspectable like any other instance.
#[cfg(feature = "gaussian-splatting")]
fn tag_splats_for_explorer(
    mut commands: Commands,
    new_splats: Query<
        (Entity, &Name),
        (
            Added<eustress_radiance::SplatCloud>,
            Without<eustress_common::classes::Instance>,
        ),
    >,
    services: Query<(Entity, &space::service_loader::ServiceComponent)>,
) {
    if new_splats.is_empty() {
        return;
    }
    let workspace = services
        .iter()
        .find(|(_, s)| s.class_name == "Workspace")
        .map(|(e, _)| e);
    for (entity, name) in &new_splats {
        commands
            .entity(entity)
            .insert(eustress_common::classes::Instance {
                name: name.as_str().to_string(),
                class_name: eustress_common::classes::ClassName::GaussianSplats,
                ..Default::default()
            });
        if let Some(ws) = workspace {
            commands.entity(entity).insert(ChildOf(ws));
        }
    }
}

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

