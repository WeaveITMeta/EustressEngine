//! # Eustress Common
//!
//! Shared types, scene definitions, and utilities used across all Eustress crates.
//! 
//! ## Modules
//! 
//! - `classes`: ECS class system (Instance, Part, Model, etc.)
//! - `plugins`: Shared Bevy plugins (lighting, etc.)
//! - `scene`: Unified RON-based scene format (v3)
//! - `services`: Service-oriented data types (Player, Lighting, etc.)
//! - `types`: Common type definitions
//! - `utils`: Shared utility functions
//!
//! ## Architecture
//! 
//! - **Classes**: ECS components (Instance, BasePart, Humanoid, etc.)
//! - **Plugins**: Shared Bevy plugins for common functionality
//! - **Services**: Runtime resources (PlayerService, LightingService, etc.)
//! - **Scene**: Serialization format for saving/loading

// World model: goal hierarchy, salience filtering, memory tier routing
pub mod goals;
pub mod salience;
pub mod memory;
// Sandbox: WorldState trait + Hypothesis/Branch search tree for external solvers
pub mod sandbox;

// Named event bus — bridges Rust, Luau, Rune, and EustressStream
pub mod events;
// Cross-crate filesystem-change broadcast — one watcher, many subscribers.
// See module docs for the architectural rationale.
pub mod file_events;
// Dynamic unit system: every dimensional value passes through this module
// at the disk and display boundaries. ECS / Avian / rendering stay in
// meters; authoring unit per instance, display unit per session.
pub mod units;
/// General SI dimension system (Data Platform D3) — see [`dimension::Dimension`].
pub mod dimension;
pub mod adornments;
pub mod assets;
pub mod attributes;
/// Where the Eustress API is: `EUSTRESS_API_URL`, checked, else production.
pub mod api_base;
// Roblox-parity 2D UI types: UDim, UDim2 with serde round-trip.
pub mod ui_types;
// Native Eustress BrickColor palette: sRGB-keyed swatches in 7 wheels, the
// Roblox-BrickColor -> Eustress-token map, and a pure-Rust sRGB->OKLCH helper.
pub mod brick_palette;
// Seven-wheel categorical color-picker state (ActiveColorWheel / ColorFavorites).
pub mod color_wheels;
pub mod wheel_lexicons;
// Scene delta types (always available — rkyv is non-optional)
pub mod scene_delta;
// EustressStream change queue: Bevy Resource + producer/consumer (feature-gated)
#[cfg(feature = "streaming")]
pub mod change_queue;
// EustressStream TOML materializer: delta subscriber + debounced file write (feature-gated)
#[cfg(feature = "streaming")]
pub mod toml_materializer;
// Simulation record types — rkyv payload structs for all simulation data
// (always available — rkyv is non-optional)
pub mod sim_record;
// Simulation stream — EustressStream read/write for SimRecord, IterationRecord, etc.
// (feature-gated: requires eustress-stream + tokio + bytes)
#[cfg(feature = "streaming")]
pub mod sim_stream;
pub mod classes;
// The live instance tree scripts read and write during Play: one tree shared
// by the Luau and Rune bindings and synced with the ECS each frame.
pub mod datamodel;
// The sealed avatar runtime — one character implementation shared by Studio
// Play Mode and the Client. See `avatar::AvatarRuntimePlugin` for why this
// replaces the convention-based `plugins::SharedCharacterPlugin`.
pub mod avatar;
// Animation built from instances (Animator, AnimationTrack, KeyframeSequence):
// the pose evaluator both shells add, and the local character both share.
pub mod animation;
// Authoritative per-class TOML schema — embedded templates + self-heal +
// extra-section claimants. Single source of truth shared between engine,
// client, and external tooling.
pub mod class_schema;
// Per-ClassName spawner trait + registry scaffold (Wave 2.2 — scaffold
// only). The trait, PropertyBag, SpawnCtx, and LOD types live here; no
// spawners are registered yet, and no engine system consumes them. Wave
// 2.3 wires the Bevy plugin; Wave 3 migrates the 80+ hardcoded
// `spawn_*` paths over to spawner impls. See
// `docs/architecture/CLASS_REGISTRY.md`.
pub mod class_registry;
// Canonical entity-creation pipeline — every "create an instance of class X"
// surface (Insert menu, Model ribbon, Toolbox, MCP `create_entity`, …) routes
// through `instance_create::create_instance` so the resulting folder + TOML
// shape is identical regardless of caller.
pub mod instance_create;
pub mod default_scene;
pub mod eustress_format;
pub mod generation;
pub mod parameters;
pub mod plugins;
pub mod pointcloud;
pub mod project_manifest;
/// What a part's DataMesh child (SpecialMesh, BlockMesh, CylinderMesh) draws,
/// the same in Studio and the Player.
pub mod data_mesh;
/// Reading a Space's geometry — the Part subset both shells share.
pub mod space_read;
/// Reading a world's records (or a Space on disk) into that tree.
pub mod tree_read;
pub mod pose_migration;
/// A Play session's tree, frame sets and draw markers, shared by both apps.
pub mod play_session;
/// What a drawn part carries: its mesh file and the part it stands for.
pub mod part_draw;
/// Whether a Space is still loading, for work that waits until it settles.
pub mod space_load;
/// Property changes as undoable commands, shared by the editor and Play.
pub mod property_command;
/// The editor's named actions, shared by the engine and the permission gate.
pub mod editor_action;
/// Running a replicated DataModel tree's LocalScripts on a Player.
#[cfg(feature = "luau")]
pub mod tree_scripts;
/// This machine's input, camera and cursor into a Play tree, the same on
/// both apps.
#[cfg(feature = "luau")]
pub mod machine_input;
pub mod properties;
pub mod scene;
pub mod scene_ops;
pub mod services;
pub mod soul;
#[cfg(feature = "luau")]
pub mod luau;
/// Shared (language-agnostic) script-plugin data types — used by both the
/// `luau` and `realism-scripting` backends, so it stays unconditional
/// (its own types are per-variant `#[cfg]`-gated internally instead).
pub mod script_plugins;
pub mod scripting;
pub mod terrain;
#[cfg(feature = "gui")]
pub mod gui;
pub mod types;
pub mod usd;
pub mod utils;
pub mod xr;
pub mod physics;
pub mod realism;
pub mod simulation;
#[cfg(feature = "streaming")]
pub mod streaming;

// ============================================================================
// Asset resolution — canonical paths to bundled templates
// ============================================================================
//
// Every engine surface that needs a class default / service template / service
// property TOML should call into one of these helpers, **not** join its own
// crate's `CARGO_MANIFEST_DIR/assets/...`. Common is the source of truth;
// engine assets directories were deleted as part of the 2026-05-12
// consolidation.
//
// In dev builds `CARGO_MANIFEST_DIR` resolves to the common crate's path on
// disk. An installed copy has no source tree: the installer puts this
// directory beside the executable as `common/assets/`, and `assets_dir`
// prefers that copy whenever it is there.

/// The folders a shipped app keeps its files in, best first: beside the
/// executable (the Windows and Linux installs), then a macOS bundle's
/// `Contents/Resources` (the executable runs from `Contents/MacOS`).
pub fn shipped_bases() -> Vec<std::path::PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
    shipped_bases_from(exe_dir.as_deref())
}

fn shipped_bases_from(exe_dir: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    let Some(dir) = exe_dir else { return Vec::new() };
    let mut bases = vec![dir.to_path_buf()];
    if let Some(contents) = dir.parent() {
        bases.push(contents.join("Resources"));
    }
    bases
}

/// Where an app finds a folder it ships, `relative` to a shipped base (such
/// as `assets` or `common/assets`), recognised by `marker` inside it so a
/// stray folder of the same name never wins: beside the executable, then in
/// a macOS bundle's `Contents/Resources`, else `source_tree` for a run from
/// a checkout. `CARGO_MANIFEST_DIR` is the BUILD machine's path, baked in at
/// compile time, so as the only answer it worked on that machine alone.
pub fn locate_shipped(relative: &str, marker: &str, source_tree: std::path::PathBuf) -> std::path::PathBuf {
    locate_in(&shipped_bases(), relative, marker, source_tree)
}

fn locate_in(
    bases: &[std::path::PathBuf],
    relative: &str,
    marker: &str,
    source_tree: std::path::PathBuf,
) -> std::path::PathBuf {
    bases
        .iter()
        .map(|base| base.join(relative))
        .find(|dir| dir.join(marker).exists())
        .unwrap_or(source_tree)
}

/// Path to the `common/assets/` directory — the single source of truth for
/// bundled engine templates (class schemas, service templates, service
/// properties) and the shared material and character assets.
///
/// Found by [`locate_shipped`] (`class_schema` must be inside): the
/// installer's copy beside the executable or in a macOS bundle's
/// Resources, else this crate's source tree. Decided once per process.
pub fn assets_dir() -> std::path::PathBuf {
    static DIR: std::sync::LazyLock<std::path::PathBuf> = std::sync::LazyLock::new(|| {
        locate_shipped(
            "common/assets",
            "class_schema",
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
        )
    });
    DIR.clone()
}

#[cfg(test)]
mod shipped_tests {
    use super::{locate_in, shipped_bases_from};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress-shipped-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_shipped_folder_is_found_in_each_layout() {
        let source = PathBuf::from("source-tree-assets");
        let find = |exe_dir: &PathBuf| locate_in(&shipped_bases_from(Some(exe_dir)), "common/assets", "class_schema", source.clone());

        // Windows and Linux: beside the executable.
        let flat = scratch("flat");
        std::fs::create_dir_all(flat.join("common/assets/class_schema")).unwrap();
        assert_eq!(find(&flat), flat.join("common/assets"));

        // macOS: the executable runs from Contents/MacOS, the files sit in
        // Contents/Resources.
        let mac = scratch("mac");
        let macos = mac.join("Eustress.app/Contents/MacOS");
        let resources = mac.join("Eustress.app/Contents/Resources");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(resources.join("common/assets/class_schema")).unwrap();
        assert_eq!(find(&macos), resources.join("common/assets"));

        // A checkout: nothing shipped, so the source tree; a folder without
        // the marker never wins.
        let dev = scratch("dev").join("target/debug");
        std::fs::create_dir_all(&dev).unwrap();
        assert_eq!(find(&dev), source);
        std::fs::create_dir_all(dev.join("common/assets")).unwrap();
        assert_eq!(find(&dev), source);

        for dir in [flat, mac] {
            let _ = std::fs::remove_dir_all(dir);
        }
        let _ = std::fs::remove_dir_all(dev.parent().unwrap().parent().unwrap());
    }
}

/// `common/assets/class_schema/` — per-class default TOMLs.
pub fn class_schema_dir() -> std::path::PathBuf {
    assets_dir().join("class_schema")
}

/// `common/assets/service_templates/` — per-service `_service.toml` +
/// service-scoped templates (e.g. MaterialService/*.mat.toml).
pub fn service_templates_dir() -> std::path::PathBuf {
    assets_dir().join("service_templates")
}

/// `common/assets/service_properties/` — Roblox-style service property
/// definition files (loaded by the engine's Properties panel for service
/// entities like Lighting / Workspace / Chat).
pub fn service_properties_dir() -> std::path::PathBuf {
    assets_dir().join("service_properties")
}

// Re-export Attributes and Parameters for convenience
pub use attributes::{
    Attributes, AttributeValue, Tags, CollectionService, AttributesPlugin,
    StringValue, NumberValue, IntValue, BoolValue, Vector3Value, Color3Value,
    CFrameValue, ObjectValue, NumberSequenceKeypoint, ColorSequenceKeypoint,
};
pub use parameters::{
    // Legacy types
    Parameters, ParametersPlugin, DataSourceType, AuthType, AnonymizationMode,
    UpdateMode, DataMapping, FieldMapping, ValidationRule, ValidationRules,
    // 3-Tier Parameter Architecture
    GlobalParameters, DomainRegistry, DomainSchema, DomainKeyDef,
    InstanceParameters, ParameterValue, ParameterValueType,
    // MCP Server Configuration
    McpServerConfig, McpCapabilities, ExportTargetConfig, ExportTargetType, AuthConfig,
    // Parameter Router (now EustressStream-backed)
    ParameterRouter, RouterStats, ExportRecord, ExportTransform, CreatorInfo, CreatorType,
    // Events (Bevy Messages)
    ParameterChangedEvent, ExportRequestEvent,
    // Serializable event types for stream bridging
    ParameterChangedSerialized, ExportRequestSerialized,
    // Well-known stream topic names
    parameter_topics,
};

// Re-export default scene functions
pub use default_scene::{spawn_baseplate, spawn_welcome_cube, spawn_default_scene};

// Re-export project manifest types for file-system-first Spaces and publishing
pub use project_manifest::{
    AssetIndexEntry, AssetIndexManifest,
    CameraSettings, EditorSettings,
    LocalFirstSettings,
    PackageIndexEntry, PackageIndexManifest,
    ProjectFormat, ProjectInfo, ProjectManifest, ProjectSettingsManifest,
    PublishedExperienceDetail, PublishedExperienceSummary, PublishedExperienceSyncRequest,
    PublishedPackageRef, PublishedReleaseManifest,
    PublishCheckpoint, PublishJournalManifest, PublishJournalState,
    PublishManifest, PublishState, PublishVisibility, ReleaseEntry,
    RenderingSettings,
    RemoteState, SyncManifest, SyncState,
    load_toml_file, save_toml_file,
};

// Re-export eustress format as the canonical file format
pub use eustress_format::{
    // Core functions
    load_eustress, save_eustress, save_for_engine, save_for_client,
    new_default_scene,
    // Validation
    is_eustress_file, is_client_scene, is_engine_scene, is_legacy_format,
    // Path conversion
    to_eustress_path, to_engine_path, to_client_path,
    // Constants
    EXTENSION, EXTENSION_PROJECT,
    VALID_EXTENSIONS, LEGACY_EXTENSIONS,
    FORMAT_VERSION,
    DEFAULT_EXTENSION,
    // Deprecated aliases (kept for backward compat, will be removed)
    EXTENSION_CLIENT, EXTENSION_ENGINE,
    DEFAULT_ENGINE_EXTENSION, DEFAULT_CLIENT_EXTENSION,
    // Error type
    EustressError,
};

// Re-export commonly used types for convenience
pub use scene::{
    Scene, SceneMetadata, AtmosphereSettings,
    Entity, EntityClass, TransformData,
    DetailLevel, NodeCategory, GenerationStatus,
    Connection as SceneConnection, ConnectionType,
    // Class data types
    PartData, ModelData, HumanoidData,
    PointLightData, SpotLightData, SurfaceLightData,
    TerrainData, SkyData, SoundData,
    ParticleEmitterData, BeamData,
    AttachmentData, WeldConstraintData, Motor6DData,
    SpecialMeshData, DecalData,
    AnimatorData, KeyframeSequenceData, UnionOperationData,
    BillboardGuiData, TextLabelData, CameraData,
    TriggerData, PortalData, NPCData,
    load_scene_from_file, save_scene_to_file,
    // Orbital settings for Earth One / geospatial scenes
    OrbitalSettings,
    // Orbital class data types
    SolarSystemData, CelestialBodyData, RegionChunkData,
};

// Re-export orbital class components
pub use classes::{
    SolarSystem, CelestialBodyClass, RegionChunk,
    // Two-tier streaming cold marker (P2 Update-bound lag fix) — re-exported
    // so both the engine spawn/promote sites and the common-side
    // `change_queue::emit_scene_change_deltas` filter can reach it.
    ColdStreamed,
};

// Re-export event bus for convenience
pub use events::{
    EventBus, EventBusResource, EventBusPlugin,
    set_event_bus_for_rune, clear_event_bus_for_rune, with_event_bus,
    topics as event_topics,
};

// Re-export scripting types for Rune/Luau API
pub use scripting::{
    // Data types
    Vector2, Vector3, CFrame, Color3, UDim, UDim2, Ray, NumberRange,
    TweenInfo, EasingStyle, EasingDirection,
    // Events
    Signal, Connection, SignalArg, BindableEvent, BindableFunction,
    RemoteEvent, RemoteFunction, PropertyChangedSignal,
    // Instance API
    InstanceRef, InstanceData, InstanceRegistry, InstanceFactory, PropertyValue,
    // Services
    RunService, FrameTime, TaskScheduler, DebrisService, TweenService, Tween, TweenStatus,
    ScriptingServices,
};
