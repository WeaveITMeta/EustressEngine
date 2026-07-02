//! # Eustress Engine Library
//! 
//! Desktop editor/studio functionality.

pub mod auth;
pub mod bliss_tracker;
pub mod forge;
pub mod parts;
pub mod rendering;
pub mod camera;
pub mod commands;
pub mod scenes;
pub mod classes;
pub mod properties;
pub mod serialization;
pub mod plugins;
pub mod shaders;
pub mod spawn;
pub mod soul;
pub mod korah;
pub mod telemetry;
pub mod play_server;
pub mod hot_reload;
pub mod pbr_materials;
pub mod particles;
pub mod beams;
pub mod decals;
pub mod billboard_gui;
pub mod billboard_pipeline;
pub mod light_cookies;
pub mod csg;
pub mod attachments;
pub mod motor6d;
pub mod humanoid;
pub mod animation_state_machine;
pub mod foot_ik;
pub mod physics_constraints;
pub mod play_mode;
pub mod play_mode_runtime;
pub mod script_editor;
pub mod lsp_launcher;
// `engine_bridge` is compiled as a BIN-LOCAL module (see main.rs) so its
// handlers share the bin's TypeIds for live resources/components. It is
// intentionally NOT a lib module: the lib lacks the bin-only modules its
// handlers reference (e.g. `ai_camera`), and nothing in the lib uses it.
pub mod notifications;
pub mod ui;
pub mod seats;
pub mod keybindings;
pub mod editor_settings;
pub mod undo;
pub mod camera_controller;
// Off-screen AI camera (marker/state/plugin). Also declared in main.rs; mirror
// it here so the LIBRARY compile of `play_mode` can resolve
// `crate::ai_camera::AiCamera` (gap-3 Play→Edit camera-disable filter). Its only
// crate dependency is `crate::default_scene`, which is already in this lib.
pub mod ai_camera;
pub mod runtime;
pub mod gizmo_tools;
pub mod move_tool;
pub mod rotate_tool;
pub mod scale_tool;
pub mod select_tool;
pub mod space;
pub mod toolbox;
pub mod selection_box;
pub mod selection_sync;
pub mod adornment_renderer;
pub mod move_handles;
pub mod scale_handles;
pub mod rotate_handles;
pub mod modal_tool;
pub mod numeric_input;
pub mod align_distribute;
pub mod array_tools;
pub mod measure_tool;
pub mod duplicate_place_tool;
pub mod selection_sets;
pub mod pivot_mode;
pub mod geom_snap;
pub mod smart_guides;
pub mod mirror_link;
pub mod part_to_terrain;
pub mod lasso_paint_select;
pub mod saved_viewpoints;
pub mod attachment_editor_tool;
pub mod constraint_editor_tool;
pub mod transform_constraints;
pub mod toast_undo;
pub mod commit_flash;
pub mod embedvec_dispatch;
pub mod rune_tool_sandbox;
pub mod mesh_import;
pub mod cursor_badge;
pub mod timeline_panel;
pub mod timeline_slint_sync;
pub mod timeline_animation;
pub mod attribute_tag_migration;
pub mod accessibility;
pub mod tools_smart;
pub mod part_selection;
pub mod material_sync;
pub mod lock_tool;
pub mod video;
pub mod transform_space;
pub mod default_scene;
pub mod startup;
pub mod terrain_plugin;
// Wave 9.C — imported-terrain voxel loader. `terrain_plugin` (compiled into
// BOTH this lib and the bin) calls `crate::terrain_voxel_load::register`, so
// the module must exist in the lib crate root too, not just `main.rs`.
// Whole module is `#![cfg(feature = "world-db")]`.
#[cfg(feature = "world-db")]
pub mod terrain_voxel_load;
// Perf QW1/QW2 — nearest-N shadow + intensity light cull. `lighting_plugin`
// (compiled into BOTH this lib and the bin) registers
// `crate::light_cull::cull_lights_to_nearest`, so the module must exist in the
// lib crate root too, not just `main.rs`.
pub mod light_cull;
pub mod clipboard;
pub mod grouping;
pub mod embedded_client;
pub mod studio_plugins;
pub mod grid_snapping;
pub mod collision_snapping;
pub mod mesh_optimizer;
pub mod replication;
pub mod asset_resolver;
pub mod xr_support;
pub mod backend_services;
pub mod platform_support;
pub mod network_benchmark;
pub mod math_utils;
pub mod entity_utils;
pub mod spatial_query_bridge;
pub mod usd_loader;
pub mod physics_proxy;
pub mod generative_pipeline;
pub mod viga;
pub mod scenarios;
pub mod circumstances;
pub mod workshop;
pub mod manufacturing;
pub mod class_conversion;
/// ClassRegistry plugin + LOOP-5 startup assertion.
///
/// Plumbing only at Wave 2.3 — the registry boots empty, the LOOP-5
/// drain-resource checklist is wired but has zero entries until other
/// plugins opt in via `app.add_drain_resource::<R>(...)`. Wave 3 ships
/// the per-class `ClassSpawner` impls; the plugin's build hook is
/// where each one's `register_class::<...>()` line goes.
///
/// Mount via `app.add_plugins(class_registry::ClassRegistryPlugin)`
/// inside `SlintUiPlugin::build`. Per `docs/process/AGENT_DISPATCH.md`
/// LOOP 5: never mount inside the legacy `StudioUiPlugin` — resources
/// registered there are invisible to the live engine, and that's the
/// exact silent-failure mode the LOOP-5 breaker exists to catch.
pub mod class_registry;
/// Per-ClassName ClassSpawner implementations (Wave 3 fan-out).
/// Each subdirectory exposes its own sub-plugin that registers its
/// spawners with the ClassRegistry resource at plugin-build time.
/// See `spawners/mod.rs` for the per-group layout.
pub mod spawners;
pub mod physics;         // Wave 6.B — mover runtime systems (MoversPlugin)
pub mod interaction;     // Wave 6.D — interaction runtime systems (InteractionPlugin)
pub mod txt_to_toml_watcher;
pub mod stream_node_plugin;
pub mod updater;
pub mod simulation;
pub mod frame_diagnostics;
/// Opt-in per-system frame micro-profiler (feature `profiling`).
///
/// Compiles to an empty plugin unless the `profiling` feature is on (which
/// also turns on `bevy/trace` so Bevy emits the per-system `tracing` spans
/// this reads). Even when built in, capture stays dormant until the
/// `EUSTRESS_PROFILE` env var is set. See the module docs for the full
/// gating + output-file contract.
pub mod profiler;
pub mod io_manager;
pub mod window_focus;

// ── Promoted from the bin (dual-compile untangling, 2026-07-02) ──────
// These five modules were declared ONLY in main.rs, which forced the
// bridge/history/light-sync systems to live in the BIN's type universe
// while ~104 other modules were compiled TWICE (bin `mod X;` + lib
// `pub mod X;`) with different TypeIds — writers and readers of the
// same nominal type silently never connected (engine_bridge saw None
// for every resource until it was made bin-local; billboard_gui's
// DoubleClickedPart readers never received part_selection's writes).
// The bin is now a THIN SHELL over the lib (`use eustress_engine::*`
// in main.rs), so there is exactly ONE instance of every type — and
// promoting engine_bridge here is the HEADLESS_RUNTIME plan's keystone
// (the future eustress-headless bin needs lib-side bridge TypeIds).
pub mod engine_bridge;
pub mod history_stream;
pub mod light_sync;
pub mod photoreal;
pub mod soul_script_migration;

// SimWriterResource must live in the lib so scenarios/plugin.rs and viga/pipeline.rs
// can reference it via `crate::SimWriterResource` from library code.
#[cfg(feature = "streaming")]
pub use sim_writer::SimWriterResource;
#[cfg(feature = "streaming")]
pub mod sim_writer {
    use std::sync::Arc;
    use bevy::prelude::*;
    use eustress_common::sim_stream::SimStreamWriter;

    /// Bevy Resource wrapper so `Arc<SimStreamWriter>` can be stored in ECS.
    /// Inserted by the startup system in `main.rs`; read as
    /// `Option<Res<SimWriterResource>>` in scenarios and VIGA pipeline.
    #[derive(Resource)]
    pub struct SimWriterResource(pub Arc<SimStreamWriter>);
}

// Re-exports for convenience
pub use rendering::{PartRenderingPlugin, PartChanged};
pub use commands::{SelectionManager, TransformManager};
pub use serialization::{save_scene, load_scene, load_scene_from_world, Scene, SceneMetadata};
pub use classes::{PropertyAccess, PropertyValue, PropertyDescriptor};

// Re-export plugins
pub use plugins::{
    WorkspacePlugin, LightingPlugin, SoundPlugin,
    PhysicsPlugin, InputPlugin, RunPlugin,
    AllServicesPlugin,
};
