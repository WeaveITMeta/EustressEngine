//! # App composition — the headless-safe core tier
//!
//! `add_core_sim_plugins` is the single source of truth for "what it means
//! to simulate a Space": space loading, WorldDb, Avian physics + the
//! determinism pins, realism, the tick-based simulation clock, Rune/Luau
//! script execution, platform services, streaming/op-log, and the Engine
//! Bridge (the TCP agent-drive surface). Both shells compose over it:
//!
//! * `eustress-engine` (windowed editor): base `DefaultPlugins` → this →
//!   the Slint/tools/render stack (still inline in `main.rs`).
//! * `eustress-headless` (`src/bin/headless.rs`): base `MinimalPlugins` +
//!   asset/state/transform shims → this → a tick-limit driver.
//!
//! Everything registered here is Slint-free. Systems that *optionally*
//! report into the editor UI do so via `Option<Res<OutputConsole>>`-style
//! soft deps that degrade to no-ops when the UI tier is absent — that
//! pattern (not a cfg) is the seam between tiers. See
//! `docs/architecture/HEADLESS_RUNTIME.md` §5.
//!
//! ## Ordering contract
//!
//! * [`register_asset_sources`] MUST run before `AssetPlugin` is added
//!   (Bevy freezes the source table when `AssetPlugin` builds).
//! * [`add_core_sim_plugins`] MUST run after the shell's base plugin set
//!   (it needs `Time`, assets, and states infrastructure present) and,
//!   in the editor, before `SlintUiPlugin` (which reads `UndoStack` and
//!   `PlayModeState` at build time).

use bevy::prelude::*;
use std::path::Path;

use eustress_common::services::{PlayerService, TeamServicePlugin};

#[cfg(feature = "streaming")]
use std::sync::Arc;
#[cfg(feature = "streaming")]
use eustress_common::change_queue::ChangeQueueConfig;
#[cfg(feature = "streaming")]
use eustress_common::sim_stream::SimStreamWriter;

// ─────────────────────────────────────────────────────────────────────────────
// Asset sources — must precede AssetPlugin
// ─────────────────────────────────────────────────────────────────────────────

/// Register the `space://` (runtime-swappable, resolves the live
/// `SpaceRoot` on every read) and `bundled://` (common assets: material
/// textures, fonts) asset sources, and seed the swappable global to the
/// launch root.
///
/// MUST be called BEFORE the shell adds `AssetPlugin` (directly or via
/// `DefaultPlugins`) — Bevy freezes the asset-source table at
/// `AssetPlugin` build time.
pub fn register_asset_sources(app: &mut App, space_root: &Path) {
    info!("📁 Registering Space asset source at: {:?}", space_root);
    // Seed the swappable global to the launch root before AssetPlugin runs.
    crate::space::space_asset_source::set_space_asset_root(space_root.to_path_buf());
    app.register_asset_source(
        "space",
        // Reader-only source over the runtime-swappable root. The explicit
        // return annotation forces the `Box<DynamicSpaceReader>` →
        // trait-object unsize coercion in the closure body.
        bevy::asset::io::AssetSourceBuilder::new(
            || -> Box<dyn bevy::asset::io::ErasedAssetReader> {
                Box::new(crate::space::space_asset_source::DynamicSpaceReader)
            },
        ),
    );

    // Bundled common assets (material textures, fonts, etc.)
    let common_assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../common/assets");
    info!("📦 Registering bundled asset source at: {:?}", common_assets);
    app.register_asset_source(
        "bundled",
        bevy::asset::io::AssetSourceBuilder::platform_default(&common_assets.to_string_lossy(), None),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Reflect registrations for glb scene spawning
// ─────────────────────────────────────────────────────────────────────────────

/// Register every component type a static glb scene graph carries, so
/// `SceneSpawner` never panics with "unregistered type" mid-load. Needed
/// by any shell that spawns glb-backed instances (both do).
pub fn register_scene_reflect_types(app: &mut App) {
    use bevy::gltf::{
        GltfExtras, GltfMaterialExtras, GltfMaterialName, GltfMeshExtras, GltfMeshName,
        GltfSceneExtras, GltfSceneName,
    };
    app.register_type::<GltfExtras>()
        .register_type::<GltfSceneExtras>()
        .register_type::<GltfMeshExtras>()
        .register_type::<GltfMaterialExtras>()
        // Transform-propagation marker inside every glb's scene graph.
        .register_type::<bevy::transform::components::TransformTreeChanged>()
        // Hierarchy / transform / visibility / name set.
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
        // Frustum-culling bounds carried by glb mesh entities.
        .register_type::<bevy::camera::primitives::Aabb>()
        // gltf naming components — all THREE (scene root carries GltfSceneName).
        .register_type::<GltfSceneName>()
        .register_type::<GltfMeshName>()
        .register_type::<GltfMaterialName>();
}

// ─────────────────────────────────────────────────────────────────────────────
// The core simulation tier
// ─────────────────────────────────────────────────────────────────────────────

/// Add every headless-safe subsystem — the "simulate a Space" tier shared
/// by the windowed editor and the headless runner. See the module docs
/// for the ordering contract.
///
/// `space_root` seeds the instance-streaming plugin's cold-tier directory;
/// pass the launch Space root (the editor passes
/// `space::default_space_root()`, the headless bin its `--space` arg).
pub fn add_core_sim_plugins(app: &mut App, space_root: &Path) {
    register_scene_reflect_types(app);

    app
        // PlayerService for play mode character spawning
        .init_resource::<PlayerService>()
        // NotificationManager resource — always registered. The toast bridge
        // system inside this plugin is itself gated on `notifications`.
        .add_plugins(crate::notifications::NotificationPlugin)
        // Undo/Redo (must be before SlintUiPlugin, which uses UndoStack)
        .add_plugins(crate::undo::UndoPlugin)
        // Tees every UndoStack push onto the `history.<kind>` topic so
        // MCP / LSP / CLI subscribers see the edit log in real-time.
        .add_plugins(crate::history_stream::HistoryStreamPlugin)
        // One-shot migration: promotes legacy flat `SoulService/*.rune`
        // files to the folder-per-script convention on space load.
        .add_plugins(crate::soul_script_migration::SoulScriptMigrationPlugin)
        // Play mode core: state, snapshots, physics activation, script
        // lifecycle, embedded-server control. Message-driven — the editor
        // layers PlayModeUiPlugin on top for Slint buttons + shortcuts.
        .add_plugins(crate::play_mode::PlayModeCorePlugin)
        // Engine Bridge — JSON-RPC 2.0 over localhost TCP for sibling
        // processes (MCP server, CLI, plugins) to query live ECS / sim /
        // embedvec. Port handoff via `<universe>/.eustress/engine.port`.
        .add_plugins(crate::engine_bridge::EngineBridgePlugin)
        // Guarantee `SpaceRoot` is always a resource (bridge handlers and
        // the port-file resync need it at boot). init_resource is a no-op
        // if the shell already inserted an override.
        .init_resource::<crate::space::SpaceRoot>()
        // Keep the swappable `space://` asset root in lock-step with
        // `SpaceRoot` across runtime Space switches.
        .add_systems(Update, crate::space::space_asset_source::sync_space_asset_root_on_change)
        // Space file loader (dynamic file-system-first loading)
        .add_plugins(crate::space::SpaceFileLoaderPlugin)
        // Instance streaming (three-tier: Cold disk → Hot RAM → Active ECS)
        .add_plugins(eustress_common::streaming::StreamingPlugin {
            config: eustress_common::streaming::StreamingConfig::default(),
            instances_dir: space_root.join("Workspace"),
        })
        // Physics (Avian 0.7 — runs at a fixed timestep)
        .add_plugins(avian3d::PhysicsPlugins::default())
        // ── Avian static-scene gating (scale to 131K+ colliders) ────────
        // EVERY can_collide part carries a real collider (exact click-
        // selection + script raycasts everywhere — no deferral). What must
        // NOT scale with collider count is the per-frame bookkeeping:
        // Avian's collider-transform propagation and Transform→Position
        // sync walk the ENTIRE collider forest every FixedPostUpdate tick
        // even with the physics clock paused and nothing moving — measured
        // ~15.6 + ~6 ms/frame at 240K static colliders in Edit on Mountain
        // Ascension. Gate those two sweeps: they run when physics is
        // UNPAUSED (Play) or when a collider / collider-ancestor transform
        // actually changed (editor drag, spawn, hot-reload) or colliders
        // were added/removed. A static Edit scene pays two empty
        // change-probe checks; correctness is unchanged — any real change
        // reopens the gate the same frame, so the spatial-query BVH and
        // collider transforms are exactly as fresh as before.
        .configure_sets(
            bevy::app::FixedPostUpdate,
            (
                avian3d::physics_transform::PhysicsTransformSystems::Propagate,
                avian3d::physics_transform::PhysicsTransformSystems::TransformToPosition,
            )
                .run_if(avian_prepare_needed),
        )
        .insert_resource(avian3d::prelude::Gravity(bevy::math::Vec3::NEG_Y * 9.80665))
        // ── Determinism pins (C2) ──────────────────────────────────────
        // Pin the fixed timestep explicitly so per-step dt is a fixed
        // contract ("Avian (Deterministic)"). 60 Hz matches the sim clock.
        .insert_resource(Time::<bevy::time::Fixed>::from_hz(60.0))
        // Pin substep count + solver config at Avian defaults so a future
        // Avian default change can't silently alter trajectories.
        .insert_resource(avian3d::prelude::SubstepCount(6))
        .insert_resource(avian3d::dynamics::solver::SolverConfig::default())
        // Cap virtual-time max-delta to break the fixed-timestep death
        // spiral (~2 catch-up steps max instead of Bevy's default ~16).
        .insert_resource({
            let mut vt = Time::<bevy::time::Virtual>::default();
            vt.set_max_delta(std::time::Duration::from_millis(33));
            vt
        })
        // Determinism — registers the GlobalRngSeed resource (C7).
        .add_plugins(eustress_common::physics::DeterminismPlugin)
        // Realism Physics System (materials, thermodynamics, fluids, ...)
        .add_plugins(eustress_common::realism::RealismPlugin)
        // Tick-based simulation with time compression (integrates with
        // PlayModeState; drains MCP sim-commands.jsonl; writes telemetry).
        .add_plugins(crate::simulation::SimulationPlugin::default())
        .add_plugins(crate::simulation::ElectrochemistryPlugin)
        // Platform services
        .add_plugins(TeamServicePlugin)
        // In-process play server (PlayModeCorePlugin's server-mode
        // transitions write its Start/Stop messages).
        .add_plugins(crate::play_server::PlayServerPlugin)
        // Soul scripting + physics bridge + script-facing ECS snapshot.
        // RuneECSBindingsPlugin lives under `ui::` for historical reasons
        // but is Slint-free (resource + per-frame snapshot sync) — without
        // it, scripts lose entity access, so it belongs to this tier.
        .add_plugins(crate::soul::EngineSoulPlugin)
        .add_plugins(crate::soul::physics_bridge::RunePhysicsBridgePlugin)
        .add_plugins(crate::ui::rune_ecs_bindings::RuneECSBindingsPlugin)
        // Universe registry (periodic Universe→Space tree scan)
        .add_plugins(crate::space::UniverseRegistryPlugin)
        // Attribute + Tag migration (event-driven)
        .add_plugins(crate::attribute_tag_migration::AttributeTagMigrationPlugin);

    // WorldDb — Fjall-backed authoritative ECS store. Opens
    // `<SpaceRoot>/world.fjalldb/` and persists runtime edits.
    #[cfg(feature = "world-db")]
    app.add_plugins(crate::space::world_db_plugin::WorldDbPlugin);

    // Streaming — in-process EustressStream + TCP stream node + the
    // persistent SimStreamWriter connection.
    #[cfg(feature = "streaming")]
    {
        app.add_plugins(eustress_common::change_queue::StreamingPlugin);
        app.add_systems(Startup, setup_sim_stream_writer);
        // Cross-process pub/sub over TCP (port advertised via
        // `<universe>/.eustress/engine.stream.port`).
        app.add_plugins(crate::stream_node_plugin::StreamNodePlugin::default());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamWriter — one persistent connection, shared via Arc<Resource>
// ─────────────────────────────────────────────────────────────────────────────

/// Startup system: connect `SimStreamWriter` once and insert as a Bevy
/// Resource. Silently skipped if streaming is unavailable — the app
/// continues; simulation records fall back to one-shot connects.
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
            commands.insert_resource(crate::SimWriterResource(Arc::new(writer)));
        }
        Err(e) => {
            warn!(
                "SimStreamWriter: streaming unavailable ({e}). \
                 Simulation records will use fallback one-shot connects."
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Error handler
// ─────────────────────────────────────────────────────────────────────────────

/// Rate-limited error handler: logs each unique error source ONCE, then
/// suppresses. Replaces both `ignore` (hides everything) and `warn`
/// (spams every frame). Install via `app.set_error_handler(...)` in each
/// shell's `main`.
pub fn rate_limited_error_handler(
    error: bevy::ecs::error::BevyError,
    ctx: bevy::ecs::error::ErrorContext,
) {
    use std::collections::HashSet;
    use std::sync::Mutex;

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

/// Run condition for Avian's per-tick sweep sets (collider-transform
/// propagation + Transform→Position sync). True when physics is actually
/// simulating (unpaused → Play) or the collider world changed: a transform
/// write on a collider-bearing entity or a collider ANCESTOR (dragging a
/// whole Model must re-propagate its children), or collider add/remove.
/// On a static Edit scene this is two tick-level empty-probe checks —
/// O(changed), not O(colliders) — so 131K+ static colliders idle at ~zero
/// while staying fully raycastable (the spatial-query BVH only needs
/// refreshing when something ACTUALLY changed, which reopens this gate the
/// same frame).
fn avian_prepare_needed(
    physics_time: Res<Time<avian3d::prelude::Physics>>,
    moved: Query<
        (),
        (
            bevy::ecs::query::With<avian3d::collision::collider::ColliderMarker>,
            // Changed<Transform> catches direct writes; Changed<GlobalTransform>
            // catches ANCESTOR moves (dragging a Model re-propagates children's
            // GlobalTransform in PostUpdate, which this probe sees on the next
            // fixed tick — the same one-tick data flow Avian itself reads).
            bevy::ecs::query::Or<(
                bevy::ecs::query::Changed<Transform>,
                bevy::ecs::query::Changed<GlobalTransform>,
            )>,
        ),
    >,
    added: Query<(), bevy::ecs::query::Added<avian3d::prelude::Collider>>,
    mut removed: RemovedComponents<avian3d::prelude::Collider>,
) -> bool {
    use avian3d::prelude::PhysicsTime as _; // trait providing is_paused()
    let any_removed = !removed.is_empty();
    removed.clear();
    !physics_time.is_paused() || !moved.is_empty() || !added.is_empty() || any_removed
}
