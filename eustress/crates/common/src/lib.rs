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

pub mod adornments;
pub mod assets;
pub mod attributes;
// Iggy streaming: scene delta types (always available — rkyv is non-optional)
pub mod iggy_delta;
// Iggy streaming: Bevy Resource + producer/consumer tasks (feature-gated)
#[cfg(feature = "iggy-streaming")]
pub mod iggy_queue;
// Iggy streaming: background TOML materializer (feature-gated)
#[cfg(feature = "iggy-streaming")]
pub mod toml_materializer;
// Simulation record types — rkyv payload structs for all simulation data
// (always available — rkyv is non-optional; replaces removed file cache)
pub mod sim_record;
// Simulation stream — Iggy read/write for SimRecord, IterationRecord, RuneScriptRecord
// and WorkshopIterationRecord (feature-gated: requires iggy + tokio + bytes)
#[cfg(feature = "iggy-streaming")]
pub mod sim_stream;
pub mod classes;
pub mod default_scene;
pub mod eustress_format;
pub mod generation;
pub mod parameters;
pub mod plugins;
pub mod pointcloud;
pub mod project_manifest;
pub mod properties;
pub mod scene;
pub mod scene_ops;
pub mod services;
pub mod soul;
#[cfg(feature = "luau")]
pub mod luau;
pub mod scripting;
pub mod terrain;
pub mod types;
pub mod usd;
pub mod utils;
pub mod xr;
pub mod orbital;
pub mod physics;
pub mod realism;
pub mod simulation;
#[cfg(feature = "streaming")]
pub mod streaming;

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
    // Parameter Router
    ParameterRouter, RouterStats, ExportRecord, ExportTransform, CreatorInfo, CreatorType,
    // Events
    ParameterChangedEvent, ExportRequestEvent,
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

// Re-export orbital coordinate system for Earth One
pub use orbital::{
    OrbitalCoords, GlobalPosition, RegionId,
    Region, RegionRegistry,
    OrbitalGravity, GravityAligned, CelestialBody,
    OrbitalPlugin, OrbitalFocus, OrbitalFocusMarker,
    // WGS84 constants and conversions
    geodetic_to_ecef, ecef_to_geodetic, haversine_distance,
    WGS84_A, WGS84_B, EARTH_MEAN_RADIUS, EARTH_GM,
};

// Re-export orbital class components
pub use classes::{
    SolarSystem, CelestialBodyClass, RegionChunk,
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
