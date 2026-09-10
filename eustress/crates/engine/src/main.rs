// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
#[allow(unused_imports)]
use bevy::render::RenderPlugin;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin, EntityCountDiagnosticsPlugin};
// Window icon: embedded in exe via winres (build.rs), runtime set in setup_slint_overlay
use eustress_engine::plugins::lighting_plugin::LightingPlugin;

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
    accessibility, adornment_renderer, ai_camera, align_distribute, array_tools, cad_assembly,
    cad_mate_tool, cad_plugin, csg,
    attachment_editor_tool, attribute_tag_migration, auth, bliss_tracker, camera,
    camera_controller, class_registry, classes, clipboard, commands, commit_flash,
    constraint_editor_tool, cursor_badge, decal_place_tool, default_scene, duplicate_place_tool,
    editor_settings, embedded_client, embedvec_dispatch, engine_bridge, entity_utils,
    forge, frame_diagnostics, generative_arch, generative_pipeline, geom_snap, gizmo_tools, grouping,
    history_stream, interaction, io_manager, keybindings, lasso_paint_select,
    light_cull, light_sync, lock_tool, manufacturing, material_sync, math_utils,
    measure_tool, mesh_import, mirror_link, modal_tool, move_handles, move_tool,
    network_benchmark, notifications, numeric_input, part_selection, part_to_terrain,
    parts, photoreal, physics, pivot_mode, play_mode, play_mode_runtime, play_server,
    plugins, profiler, properties, rendering, road_tool, rotate_handles, rotate_tool,
    rune_tool_sandbox, runtime, saved_viewpoints, scale_handles, scale_tool, scenes,
    script_plugin_host,
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
use eustress_engine::keybindings::KeyBindingsPlugin;
use eustress_engine::clipboard::ClipboardPlugin;
use eustress_engine::grouping::GroupingPlugin;
use eustress_engine::material_sync::MaterialSyncPlugin;
use eustress_engine::terrain_plugin::EngineTerrainPlugin;
use eustress_engine::play_mode::PlayModeUiPlugin;
use eustress_engine::script_editor;
use eustress_engine::window_focus::WindowFocusPlugin;
use eustress_engine::startup::{StartupPlugin, StartupArgs};
// ServicePropertiesPlugin removed - now handled by Slint UI
use eustress_engine::workshop::WorkshopPlugin;
use eustress_engine::space::SpaceRoot;

// ─────────────────────────────────────────────────────────────────────────────
// Engine log file
// ─────────────────────────────────────────────────────────────────────────────

/// How many runs' logs to keep. Older ones are pruned when the engine starts.
const ENGINE_LOG_RETAIN: usize = 5;

/// Directory holding the engine's rolling logs.
///
/// `~/.eustress_engine/logs/`, next to the engine's other per-user state
/// (`settings.json`, `bliss_tracker.toml`, `soul_settings.json`), so there is
/// ONE place to ask a user for when a bug report arrives.
///
/// A Space-relative `<space>/.eustress/output.log` is deliberately not used.
/// `LogPlugin` builds before the Space tree is opened, and `SpaceRoot` moves
/// again on every Universe/Space switch, so a Space-relative file would capture
/// the first few seconds of a session and then quietly stop following it — the
/// half-log is worse than no log because it looks complete.
///
/// Falls back to an exe-adjacent `logs/` directory if no home directory
/// resolves, and to no log at all if even that fails.
fn engine_log_dir() -> Option<std::path::PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".eustress_engine").join("logs"))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|d| d.join("logs")))
        })
}

/// Drop everything but the newest `ENGINE_LOG_RETAIN` logs, so the directory
/// stays bounded without any single run's file being truncated mid-session.
///
/// A file another live instance still holds open cannot be deleted on Windows,
/// which is exactly the outcome wanted: pruning never touches a log that is
/// still being written.
fn prune_engine_logs(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("engine-") || !name.ends_with(".log") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    for (_, stale) in logs.into_iter().skip(ENGINE_LOG_RETAIN) {
        let _ = std::fs::remove_file(stale);
    }
}

/// The `LogPlugin::fmt_layer` hook — console output PLUS a log file.
///
/// Release builds set `windows_subsystem = "windows"`, which detaches the
/// console, so stderr goes nowhere a user can reach. Every diagnostic the
/// engine already emits (billboard atlas capacity, sub-1.0 content scale,
/// asset-load failures) is therefore invisible in exactly the builds where it
/// matters most. `fmt_layer` REPLACES `LogPlugin`'s single formatting layer, so
/// this returns two of them in a `Vec` (which is itself a `Layer`): Bevy's own
/// stderr layer, unchanged, and a second, ANSI-free layer over the file. Both
/// sit above the plugin's `EnvFilter`, so the file records exactly what the
/// console records — no second filter to keep in sync.
///
/// Each run gets its own `engine-<pid>.log` and the directory is pruned to the
/// last few, so the logs stay bounded. A fixed file name would not survive the
/// second engine window: nothing stops two instances running at once (hence the
/// "Instance {pid}" window title), and the second one's truncating open would
/// silently wipe the first one's live log.
///
/// The `Mutex` around the handle serialises writes from the render and async
/// task pools. `File` is a `MakeWriter` on its own, but concurrent writes to a
/// single handle share one file cursor and can interleave mid-line; the lock is
/// held only for the duration of one already-formatted event.
///
/// Returning `None` on any I/O failure hands `LogPlugin` back its own default
/// layer, so a read-only home directory costs the log file and nothing else.
fn engine_log_fmt_layer(_app: &mut App) -> Option<bevy::log::BoxedFmtLayer> {
    use bevy::log::tracing_subscriber::fmt;

    let dir = engine_log_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    prune_engine_logs(&dir);

    let path = dir.join(format!("engine-{}.log", std::process::id()));
    let file = std::fs::File::create(&path).ok()?;
    println!("Engine log: {}", path.display());

    // Byte-for-byte Bevy's default: `Layer::default()` reads NO_COLOR to decide
    // on ANSI, and stderr keeps log output off the stdout the CLI writes to.
    let console: bevy::log::BoxedFmtLayer =
        Box::new(fmt::Layer::default().with_writer(std::io::stderr));
    let to_file: bevy::log::BoxedFmtLayer = Box::new(
        fmt::Layer::default()
            // Escape codes in a file are noise in every reader that opens it.
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file)),
    );
    let both: bevy::log::BoxedFmtLayer = Box::new(vec![console, to_file]);
    Some(both)
}

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
    app.set_error_handler(eustress_engine::app_core::rate_limited_error_handler);
    
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
    eustress_engine::app_core::register_asset_sources(&mut app, &space_root);

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
                file_path: {
                    // Prefer the exe-adjacent `assets/` (the packaged/release
                    // layout — assets ship next to the binary). Falls back to
                    // the dev source tree (`crates/engine/assets`, resolved
                    // via CARGO_MANIFEST_DIR — same pattern as the `bundled`
                    // asset source registered above) so `cargo run` keeps
                    // resolving exactly as it does today. A bare relative
                    // "assets" only resolves when the process CWD happens to
                    // be the crate dir; launching the built .exe directly (or
                    // a packaged build) resolves it against the exe's own
                    // dir instead, where no assets/ exists — every
                    // asset-loaded entity (part meshes, Gaussian-splat
                    // clouds) then silently fails to load while procedural
                    // content (terrain) keeps working, which reads as
                    // "missing geometry" with no error pointing at the cause.
                    let exe_adjacent = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|d| d.join("assets")));
                    match exe_adjacent {
                        Some(p) if p.is_dir() => p.to_string_lossy().to_string(),
                        _ => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("assets")
                            .to_string_lossy()
                            .to_string(),
                    }
                },
                // Allow loading assets via ABSOLUTE paths. Bevy 0.16+ defaults
                // this to `Forbid`, which silently rejects every
                // `asset_server.load(<absolute path>)` — exactly how
                // user-imported Universe assets load (images under
                // `<Universe>/assets/images/`, Gaussian-splat clouds under
                // `<Universe>/assets/splats/`). Those dirs sit ABOVE the
                // `space://` root (the live SpaceRoot) and outside the
                // `default://` asset root, so a relative-through-a-source load
                // can't reach them — they are genuinely addressed absolutely.
                // With the default `Forbid`, importing a `.ply`/`.png` copies
                // the file and spawns the entity, then the load is rejected
                // with "Asset path … is unapproved" and nothing renders (the
                // symptom that looks like "import did nothing"). Eustress is a
                // local-first desktop tool loading files the user explicitly
                // picked through a native dialog, and asset loading is
                // local-only (no network egress), so the guard's threat model
                // (loading untrusted remote/script-supplied paths) does not
                // apply here. `Allow` restores the pre-0.16 behaviour the rest
                // of the asset pipeline already assumes.
                unapproved_path_mode: bevy::asset::UnapprovedPathMode::Allow,
                ..default()
            })
            // Per-system frame micro-profiler hook (feature `profiling`).
            // `profiler::custom_layer` adds a tracing Layer to THIS subscriber
            // that times each Bevy `"system"` span. With the feature off it is
            // `|_| None`, so it changes nothing about logging. With the feature
            // on it still does nothing until `EUSTRESS_PROFILE` is set.
            //
            // `engine_log_fmt_layer` is the FORMATTING side: it emits Bevy's
            // usual stderr layer plus a second one over a log file, so the
            // diagnostics the engine already prints survive a release build
            // where `windows_subsystem = "windows"` hides the console.
            // `filter`/`level` stay at their defaults.
            .set(bevy::log::LogPlugin {
                custom_layer: profiler::custom_layer,
                fmt_layer: engine_log_fmt_layer,
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
        });

    // ── Core simulation tier ────────────────────────────────────────────
    // Everything headless-safe — space loading, WorldDb, Avian + the
    // determinism pins, realism, simulation clock, Rune/Luau, services,
    // streaming/op-log, Engine Bridge, glb reflect registrations. Shared
    // verbatim with the eustress-headless bin (HEADLESS_RUNTIME.md §5).
    // Must come before SlintUiPlugin below, which reads UndoStack and
    // PlayModeState at build time.
    eustress_engine::app_core::add_core_sim_plugins(&mut app, &space_root);

    app // ── Editor tier: Slint UI, tools, gizmos, rendering ────────────
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
        // Play-mode editor seam: Slint StudioState flags, F5-F8 keyboard
        // shortcuts, in-viewport GUI click dispatch. PlayModeCorePlugin
        // (state/snapshots/physics/scripts) came in with
        // add_core_sim_plugins above.
        .add_plugins(PlayModeUiPlugin)
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
        // (Engine Bridge, SpaceRoot init, and the space:// root sync now
        // come in with add_core_sim_plugins above.)
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
        // (SpaceFileLoaderPlugin + instance streaming now come in with
        // add_core_sim_plugins above.)
        // Toolbox (mesh insertion system)
        .add_plugins(toolbox::ToolboxPlugin)
        // Camera controls
        .add_plugins(CameraControllerPlugin)
        .add_systems(Startup, setup_camera_controller.after(default_scene::setup_default_scene))
        // Editor settings
        .add_plugins(EditorSettingsPlugin)
        // TOML theme engine (loads built-in + user themes; resolves active)
        .add_plugins(eustress_engine::studio_theme::StudioThemePlugin)
        // Eustress Modes (task layouts: ribbon subset + layout + accent)
        .add_plugins(eustress_engine::studio_modes::StudioModesPlugin)
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
        // Surface placement (Decal / Texture) — radial-chosen media follows
        // the raycasted face with a ghost preview, applies on a click to an
        // unlocked BasePart.
        .add_plugins(decal_place_tool::SurfacePlacementPlugin)
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
        // (AttributeTagMigrationPlugin now comes in with add_core_sim_plugins.)
        // Smart Build Tools (Gap Fill, Resize Align, Edge Align,
        // Part Swap, Model Reflect). Each registers its factory with
        // ModalToolRegistry.
        .add_plugins(tools_smart::SmartToolsPlugin)
        // Drafting Boolean (Union / Subtract / Intersect / Separate) —
        // real truck CSG via eustress-cad, Model-group fallback.
        .add_plugins(csg::CsgPlugin)
        // Parametric CadPart (feature-tree → mesh). Drafting tab
        // Plate / Box / Cylinder inserts + variable regen.
        .add_plugins(cad_plugin::CadPlugin)
        // Assembly mates → live Avian joints.
        .add_plugins(cad_assembly::CadAssemblyPlugin)
        // Viewport mate pick tool (anchors = hit points).
        .add_plugins(cad_mate_tool::MateToolPlugin)
        // Selection sync
        .add_plugins(SelectionSyncPlugin {
            selection_manager: selection_manager.clone(),
        })
        // Terrain
        .add_plugins(EngineTerrainPlugin)
        // (Avian + determinism pins + realism + simulation + play server +
        // team service + soul scripting/physics-bridge/ECS-bindings now
        // come in with add_core_sim_plugins above.)
        // Gamepad
        .add_plugins(eustress_common::services::GamepadServicePlugin)
        // Notifications UI
        .add_plugins(ui::notifications::NotificationsPlugin)
        // Embedded client
        .add_plugins(embedded_client::EmbeddedClientPlugin)
        // Runtime
        .add_plugins(runtime::RuntimePlugin)
        // Seats
        .add_plugins(seats::SeatPlugin)
        // Soul GUI bridge (scripts ↔ Slint in-game GUI) — editor tier.
        .add_plugins(soul::gui_bridge::GuiBridgePlugin)
        // Workshop (System 0: Ideation — conversational product creation)
        .add_plugins(WorkshopPlugin)
        // In-app updater (checks releases.eustress.dev on startup)
        .add_plugins(updater::UpdaterPlugin)
        // Generative pipeline
        .add_plugins(generative_pipeline::GenerativePipelinePlugin)
        // Generative architecture overlay (editor tier only). Draws the best
        // candidate from GenerativeArchLedger as gizmo lines coloured by FEA
        // utilization. Split out of GenerativeArchPlugin because the headless
        // shell has no GizmoPlugin, and a `Gizmos` system param there fails
        // validation and silently skips.
        .add_plugins(generative_arch::GenerativeArchGizmoPlugin)
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
        // (UniverseRegistryPlugin now comes in with add_core_sim_plugins.)
        // Startup
        .add_plugins(StartupPlugin)
        // Window title: derives "Universe > Space - Eustress Engine" from SpaceRoot
        .add_systems(Update, update_window_title)
        // Studio plugins
        .add_plugins(studio_plugins::StudioPluginSystem)
        .add_plugins(road_tool::RoadToolEnginePlugin)
        .add_plugins(script_plugin_host::ScriptPluginHostPlugin)
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
        // Convert radiance's Avian-free collider proxy into a real Avian compound
        // collider so ANY imported splat becomes physical + click-selectable via
        // its true geometry (not the Aabb box). radiance extracts the proxy;
        // this attaches it.
        app.add_systems(Update, eustress_engine::space::instance_loader::apply_splat_colliders);
        // PERSISTENCE: DB-primary cold-load spawns instances from binary cores,
        // but file-natured GaussianSplats have no core (their `[gaussian_splats]`
        // path can't survive it) — so an imported splat vanished on the next
        // launch. Re-spawn disk splat folders on Space open (once, gated), the
        // same way the file-watcher hot-creates them.
        app.add_systems(Update, eustress_engine::space::instance_loader::load_disk_gaussian_splats_on_open);
    }

    // (WorldDbPlugin now comes in with add_core_sim_plugins above.)

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

    // (EustressStream change-queue plugin, the persistent SimStreamWriter,
    // and the TCP stream node now come in with add_core_sim_plugins above.)

    // Wrap App::run() so GPU surface-lost panics (swap chain unavailable,
    // uniform buffer unwrap, wgpu buffer invalid) don't show a crash dialog.
    // These happen when: window minimized → zero-size surface, GPU driver
    // TDR reset, or display mode change mid-frame. They are transient but
    // Bevy 0.18 panics instead of recovering. We catch them and exit cleanly.
    // Record EVERY panic in the engine log before anything decides what it
    // means. The default hook writes only to stderr, which a desktop launch
    // discards, and the swallow path below then exits silently — so a crash of
    // that class left the log file ending mid-line with no explanation and no
    // Windows error report. Chain to the previous hook so the telemetry beacon
    // and the default backtrace still run.
    {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());
            error!(
                target: "eustress_engine::crash",
                location = %location,
                payload = %panic_payload_str(info.payload()),
                "PANIC — engine is going down"
            );
            previous(info);
        }));
    }

    let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.run();
    }));

    match run_result {
        Ok(()) => {
            println!("✅ Eustress Engine closed gracefully");
        }
        Err(payload) => {
            let msg = panic_payload_str(&*payload);

            // Resource exhaustion PRESENTS as a graphics error, and the old
            // heuristic swallowed it: `unrecoverable` and `Buffer`+`invalid`
            // both match a failed allocation or a device lost to VRAM
            // pressure. Loading a large imported place is exactly when that
            // happens, so a real failure exited 0 and looked like a clean
            // quit. Check for it FIRST and make it loud.
            let exhaustion = msg.contains("Out of memory")
                || msg.contains("OutOfMemory")
                || msg.contains("out of device memory")
                || msg.contains("OutOfDeviceMemory")
                || msg.contains("Device is lost")
                || msg.contains("device is lost")
                || msg.contains("DeviceLost");

            // Only a TRANSIENT surface loss is safe to swallow: minimizing the
            // window, a display-mode change, or a driver TDR invalidates the
            // swap chain for a frame. Deliberately narrow — anything that
            // merely SOUNDS graphics-y must reach the developer instead.
            let transient_surface = msg.contains("swap chain")
                || msg.contains("Acquiring a texture")
                || msg.contains("Surface timed out")
                || msg.contains("Outdated");

            if exhaustion {
                error!(
                    target: "eustress_engine::crash",
                    panic = %msg,
                    "OUT OF GPU/SYSTEM MEMORY — a real failure, not a surface loss"
                );
                eprintln!("❌ Eustress ran out of GPU/system memory — this is a crash, not a clean exit.");
                eprintln!("   {msg}");
                std::process::exit(1);
            }

            if transient_surface {
                warn!(
                    target: "eustress_engine::crash",
                    panic = %msg,
                    "GPU surface lost (transient) — exiting cleanly"
                );
                eprintln!("⚠️  GPU surface lost — exiting cleanly (not a crash).");
                std::process::exit(0);
            }

            // Real panic — re-raise so dev gets a proper backtrace.
            std::panic::resume_unwind(payload);
        }
    }
}

/// Best-effort human text for a panic payload (`panic!` produces either a
/// `String` or a `&'static str`).
fn panic_payload_str(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("<non-string panic payload>")
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

// `setup_sim_stream_writer` and `rate_limited_error_handler` moved to
// `eustress_engine::app_core` so the headless bin shares them.

