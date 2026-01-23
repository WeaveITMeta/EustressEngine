//! # Unified Scene Format for Eustress Engine
//! 
//! Proprietary binary scene format (.eustress) that combines:
//! - Studio's Eustress-style class system (Part, Model, Humanoid, etc.)
//! - AI enhancement pipeline features (prompts, detail levels, quest graphs)
//! - Atmosphere and environment settings
//!
//! ## Table of Contents
//! 1. Scene - Root container with metadata and atmosphere
//! 2. SceneMetadata - Name, author, version, tags
//! 3. AtmosphereSettings - Time of day, weather, lighting
//! 4. Entity - Base entity with class-specific data
//! 5. EntityClass - Enum of all supported class types
//! 6. Class-specific data structs (PartData, ModelData, etc.)
//! 7. AI Enhancement types (DetailLevel, NodeCategory)
//! 8. Quest/Narrative types (Connection, ConnectionType)

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// 1. Scene - Root Container
// ============================================================================

/// Complete scene definition - the unified format for Studio and Client
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct Scene {
    /// Format version for compatibility checking
    pub format: String,
    
    /// Scene metadata
    pub metadata: SceneMetadata,
    
    /// Global theme for AI generation context
    /// e.g. "dark fantasy ruins, volumetric fog, bioluminescent plants"
    pub global_theme: String,
    
    /// Atmosphere and environment settings
    pub atmosphere: AtmosphereSettings,
    
    /// Workspace settings (gravity, speed limits, etc.)
    #[serde(default)]
    pub workspace_settings: WorkspaceSettings,
    
    /// Player defaults (walk speed, jump power, etc.)
    #[serde(default)]
    pub player_settings: PlayerSettings,
    
    /// Spawn locations for players
    #[serde(default)]
    pub spawn_locations: Vec<SpawnLocationData>,
    
    /// Orbital coordinate settings (for Earth One / geospatial scenes)
    #[serde(default)]
    pub orbital_settings: OrbitalSettings,
    
    /// All entities in the scene (flat list, hierarchy via parent field)
    pub entities: Vec<Entity>,
    
    /// Connections between entities for quest/narrative graph
    pub connections: Vec<Connection>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            format: "eustress_v3".to_string(),
            metadata: SceneMetadata::default(),
            global_theme: "fantasy medieval".to_string(),
            atmosphere: AtmosphereSettings::default(),
            workspace_settings: WorkspaceSettings::default(),
            player_settings: PlayerSettings::default(),
            spawn_locations: Vec::new(),
            orbital_settings: OrbitalSettings::default(),
            entities: Vec::new(),
            connections: Vec::new(),
        }
    }
}

// ============================================================================
// Orbital Settings (Earth One / Geospatial)
// ============================================================================

/// Orbital coordinate system settings for geospatial/planetary-scale scenes.
///
/// When enabled, the scene uses WGS84/ECEF global coordinates with floating-origin
/// regions for seamless planetary-scale positioning without f32 precision issues.
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct OrbitalSettings {
    /// Enable orbital coordinate system (WGS84/ECEF + floating origin)
    pub enabled: bool,
    
    /// Scene origin in geodetic coordinates (latitude, longitude, altitude)
    /// This is the "center" of the scene in global coordinates
    pub origin_geodetic: [f64; 3],
    
    /// Region/chunk size in meters (for floating origin management)
    pub region_size: f32,
    
    /// Use n-body gravity simulation (Earth + Moon + Sun)
    /// If false, uses simple Earth-centric point mass approximation
    pub nbody_gravity: bool,
    
    /// Custom gravity override (if Some, ignores orbital gravity calculation)
    /// Format: [x, y, z] in m/s²
    pub custom_gravity: Option<[f32; 3]>,
    
    /// Whether this is an abstract (non-Earth) space
    /// Abstract spaces use Euclidean coordinates with custom gravity
    pub is_abstract_space: bool,
    
    /// Parent region ID for abstract spaces (links to Earth location)
    /// Format: "L{level}F{face}({x},{y},{z})" or "Abstract({x},{y})"
    pub parent_region: Option<String>,
    
    /// Offset from parent region origin (for abstract spaces)
    pub parent_offset: Option<[f32; 3]>,
    
    /// Maximum detail level for region tiling (0-24, higher = more detail)
    pub max_detail_level: u8,
    
    /// Enable camera gravity alignment (orient camera "up" to local gravity)
    pub camera_gravity_alignment: bool,
}

impl Default for OrbitalSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            origin_geodetic: [0.0, 0.0, 0.0], // Null Island
            region_size: 1000.0,              // 1km chunks
            nbody_gravity: false,
            custom_gravity: None,
            is_abstract_space: false,
            parent_region: None,
            parent_offset: None,
            max_detail_level: 16,             // ~600m tiles
            camera_gravity_alignment: true,
        }
    }
}

impl OrbitalSettings {
    /// Create settings for an Earth-surface scene at given coordinates
    pub fn earth_surface(lat: f64, lon: f64, alt: f64) -> Self {
        Self {
            enabled: true,
            origin_geodetic: [lat, lon, alt],
            region_size: 1000.0,
            nbody_gravity: false,
            custom_gravity: None,
            is_abstract_space: false,
            parent_region: None,
            parent_offset: None,
            max_detail_level: 16,
            camera_gravity_alignment: true,
        }
    }
    
    /// Create settings for an orbital/space scene
    pub fn orbital(lat: f64, lon: f64, altitude_km: f64) -> Self {
        Self {
            enabled: true,
            origin_geodetic: [lat, lon, altitude_km * 1000.0],
            region_size: 10000.0, // 10km chunks for space
            nbody_gravity: true,
            custom_gravity: None,
            is_abstract_space: false,
            parent_region: None,
            parent_offset: None,
            max_detail_level: 12,
            camera_gravity_alignment: true,
        }
    }
    
    /// Create settings for an abstract (non-Earth) space
    pub fn abstract_space(gravity: [f32; 3]) -> Self {
        Self {
            enabled: true,
            origin_geodetic: [0.0, 0.0, 0.0],
            region_size: 1000.0,
            nbody_gravity: false,
            custom_gravity: Some(gravity),
            is_abstract_space: true,
            parent_region: None,
            parent_offset: None,
            max_detail_level: 16,
            camera_gravity_alignment: false,
        }
    }
    
    /// Create settings for an abstract space linked to an Earth location
    pub fn abstract_linked(parent_region: &str, offset: [f32; 3], gravity: [f32; 3]) -> Self {
        Self {
            enabled: true,
            origin_geodetic: [0.0, 0.0, 0.0],
            region_size: 1000.0,
            nbody_gravity: false,
            custom_gravity: Some(gravity),
            is_abstract_space: true,
            parent_region: Some(parent_region.to_string()),
            parent_offset: Some(offset),
            max_detail_level: 16,
            camera_gravity_alignment: false,
        }
    }
}

// ============================================================================
// Workspace Settings
// ============================================================================

/// Workspace/physics settings for the scene
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct WorkspaceSettings {
    /// Gravity strength (studs/s²) - default 35.0 (Roblox-style)
    pub gravity: f32,
    
    /// Maximum entity speed (studs/s)
    pub max_entity_speed: f32,
    
    /// Maximum fall speed (studs/s)
    pub max_fall_speed: f32,
    
    /// Air density for drag calculations
    pub air_density: f32,
    
    /// Allow streaming (load/unload distant parts)
    pub streaming_enabled: bool,
    
    /// Streaming target radius (studs)
    pub streaming_target_radius: f32,
    
    /// Streaming minimum radius (studs)
    pub streaming_min_radius: f32,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            gravity: 35.0,              // Roblox default: 196.2 studs/s² but we use 35 for gameplay
            max_entity_speed: 100.0,
            max_fall_speed: 200.0,
            air_density: 0.0,           // No air resistance by default
            streaming_enabled: false,
            streaming_target_radius: 1024.0,
            streaming_min_radius: 256.0,
        }
    }
}

// ============================================================================
// Player Settings
// ============================================================================

/// Default player/character settings for the scene
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct PlayerSettings {
    /// Default walk speed (studs/s)
    pub walk_speed: f32,
    
    /// Default run speed (studs/s) - when shift is held
    pub run_speed: f32,
    
    /// Jump power (studs/s initial velocity)
    pub jump_power: f32,
    
    /// Maximum health
    pub max_health: f32,
    
    /// Auto-jump when walking into obstacles
    pub auto_jump_enabled: bool,
    
    /// Character height (studs)
    pub character_height: f32,
    
    /// Character width (studs)
    pub character_width: f32,
    
    /// Respawn time (seconds)
    pub respawn_time: f32,
    
    /// Allow player to reset character
    pub allow_reset: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            walk_speed: 16.0,           // Roblox default
            run_speed: 24.0,
            jump_power: 50.0,           // Roblox default
            max_health: 100.0,
            auto_jump_enabled: true,
            character_height: 5.0,      // ~5 studs tall
            character_width: 2.0,
            respawn_time: 5.0,
            allow_reset: true,
        }
    }
}

// ============================================================================
// Spawn Location Data
// ============================================================================

/// Spawn location for players
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SpawnLocationData {
    /// Position in world space
    pub position: [f32; 3],
    
    /// Rotation (euler angles in degrees)
    pub rotation: [f32; 3],
    
    /// Team name (empty = neutral/all teams)
    pub team: String,
    
    /// Is this spawn enabled
    pub enabled: bool,
    
    /// Duration player is invulnerable after spawning (seconds)
    pub spawn_protection_duration: f32,
    
    /// Allow team change when touching
    pub allow_team_change: bool,
}

impl Default for SpawnLocationData {
    fn default() -> Self {
        Self {
            position: [0.0, 5.0, 0.0],  // 5 studs above origin
            rotation: [0.0, 0.0, 0.0],
            team: String::new(),
            enabled: true,
            spawn_protection_duration: 3.0,
            allow_team_change: false,
        }
    }
}

// ============================================================================
// 2. Scene Metadata
// ============================================================================

/// Scene metadata - information about the scene file
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SceneMetadata {
    pub name: String,
    pub description: String,
    pub author: String,
    pub created: String,
    pub modified: String,
    pub engine_version: String,
    pub tags: Vec<String>,
}

impl Default for SceneMetadata {
    fn default() -> Self {
        Self {
            name: "Untitled Scene".to_string(),
            description: String::new(),
            author: String::new(),
            created: String::new(),
            modified: String::new(),
            engine_version: "0.1.0".to_string(),
            tags: Vec::new(),
        }
    }
}

// ============================================================================
// 3. Atmosphere Settings
// ============================================================================

/// Atmosphere rendering mode for scene serialization
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum AtmosphereMode {
    /// Fast lookup-texture based rendering (default)
    #[default]
    LookupTexture,
    /// Raymarched rendering for accurate atmosphere (space views, flight sims)
    Raymarched,
}

/// Atmosphere and environment settings for the scene
/// 
/// Combines Roblox-like properties with Bevy 0.17's raymarched atmosphere features.
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct AtmosphereSettings {
    /// Time of day: "HH:MM:SS" format or descriptive ("dawn", "noon", "dusk", "midnight")
    pub time_of_day: String,
    
    /// Weather: "clear", "overcast", "rain", "storm", "fog", "snow"
    pub weather: String,
    
    /// Sun/directional light color
    pub sun_color: [f32; 4],
    
    /// Sun intensity (lux)
    pub sun_intensity: f32,
    
    /// Ambient light color
    pub ambient_color: [f32; 4],
    
    /// Ambient light intensity (brightness multiplier)
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    
    /// Fog density (0.0 = no fog, 1.0 = thick fog)
    pub fog_density: f32,
    
    /// Fog color
    pub fog_color: [f32; 4],
    
    /// Fog start distance (studs)
    #[serde(default = "default_fog_start")]
    pub fog_start: f32,
    
    /// Fog end distance (studs)
    #[serde(default = "default_fog_end")]
    pub fog_end: f32,
    
    /// Sky color (zenith)
    #[serde(default = "default_sky_color")]
    pub sky_color: [f32; 4],
    
    /// Horizon color
    #[serde(default = "default_horizon_color")]
    pub horizon_color: [f32; 4],
    
    /// Enable shadows
    #[serde(default = "default_shadows_enabled")]
    pub shadows_enabled: bool,
    
    /// Shadow softness (0.0 = hard, 1.0 = soft)
    #[serde(default = "default_shadow_softness")]
    pub shadow_softness: f32,
    
    // === Bevy 0.17 Atmosphere Properties ===
    
    /// Atmosphere density (Roblox-like, 0.0 - 1.0)
    #[serde(default = "default_atmo_density")]
    pub density: f32,
    
    /// Atmosphere color tint
    #[serde(default = "default_atmo_color")]
    pub atmo_color: [f32; 4],
    
    /// Horizon decay color
    #[serde(default = "default_atmo_decay")]
    pub decay: [f32; 4],
    
    /// Sun glare intensity (0.0 - 1.0)
    #[serde(default)]
    pub glare: f32,
    
    /// Haze amount (0.0 - 1.0)
    #[serde(default)]
    pub haze: f32,
    
    /// Rendering mode: LookupTexture (fast) or Raymarched (accurate)
    #[serde(default)]
    pub rendering_mode: AtmosphereMode,
    
    /// Maximum raymarching samples (8-128, only for Raymarched mode)
    #[serde(default = "default_sky_samples")]
    pub sky_max_samples: u32,
    
    /// Enable realtime-filtered environment map for reflections
    #[serde(default = "default_true")]
    pub environment_map_enabled: bool,
    
    /// Enable atmosphere-based environment lighting
    #[serde(default = "default_true")]
    pub atmosphere_environment_light: bool,
}

fn default_brightness() -> f32 { 1.0 }
fn default_fog_start() -> f32 { 100.0 }
fn default_fog_end() -> f32 { 500.0 }
fn default_sky_color() -> [f32; 4] { [0.4, 0.6, 0.9, 1.0] }
fn default_horizon_color() -> [f32; 4] { [0.7, 0.8, 0.95, 1.0] }
fn default_shadows_enabled() -> bool { true }
fn default_shadow_softness() -> f32 { 0.5 }
fn default_atmo_density() -> f32 { 0.4 }
fn default_atmo_color() -> [f32; 4] { [0.5, 0.7, 1.0, 1.0] }
fn default_atmo_decay() -> [f32; 4] { [0.9, 0.85, 0.8, 1.0] }
fn default_sky_samples() -> u32 { 32 }
fn default_true() -> bool { true }

impl Default for AtmosphereSettings {
    fn default() -> Self {
        Self {
            time_of_day: "12:00:00".to_string(),
            weather: "clear".to_string(),
            sun_color: [1.0, 0.98, 0.95, 1.0],
            sun_intensity: 15000.0,
            ambient_color: [0.4, 0.45, 0.5, 1.0],
            brightness: 1.0,
            fog_density: 0.0,
            fog_color: [0.8, 0.85, 0.9, 1.0],
            fog_start: 100.0,
            fog_end: 500.0,
            sky_color: [0.4, 0.6, 0.9, 1.0],
            horizon_color: [0.7, 0.8, 0.95, 1.0],
            shadows_enabled: true,
            shadow_softness: 0.5,
            // Bevy 0.17 atmosphere defaults
            density: 0.4,
            atmo_color: [0.5, 0.7, 1.0, 1.0],
            decay: [0.9, 0.85, 0.8, 1.0],
            glare: 0.0,
            haze: 0.0,
            rendering_mode: AtmosphereMode::LookupTexture,
            sky_max_samples: 32,
            environment_map_enabled: true,
            atmosphere_environment_light: true,
        }
    }
}

// ============================================================================
// 4. Entity - Base Entity Structure
// ============================================================================

/// An entity in the scene - combines Eustress-style classes with AI enhancement
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct Entity {
    /// Unique identifier
    pub id: u32,
    
    /// Display name
    pub name: String,
    
    /// Parent entity ID (None for root entities)
    pub parent: Option<u32>,
    
    /// Child entity IDs
    pub children: Vec<u32>,
    
    /// Class type and class-specific data
    pub class: EntityClass,
    
    /// Transform (position, rotation, scale)
    pub transform: TransformData,
    
    // --- Network Ownership ---
    
    /// Default network ownership rule for this entity
    /// Determines who can own/control this entity at runtime
    pub network_ownership: NetworkOwnershipRule,
    
    // --- AI Enhancement Fields ---
    
    /// AI generation prompt for this entity
    /// e.g. "ancient elven temple overgrown with vines"
    pub prompt: String,
    
    /// Detail level for AI generation
    pub detail_level: DetailLevel,
    
    /// Category for AI pipeline routing
    pub category: NodeCategory,
    
    /// Quest/narrative flags
    /// e.g. {"locked": "true", "key_item": "rusty_key"}
    #[reflect(ignore)]
    pub quest_flags: HashMap<String, String>,
    
    // --- Generated Asset References ---
    
    /// Generated mesh asset ID (content hash)
    /// Populated after AI generation completes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_mesh_id: Option<String>,
    
    /// Generated texture asset ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_texture_id: Option<String>,
    
    /// Generated LOD chain (index 0 = highest quality)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_lods: Vec<String>,
    
    /// Current generation status
    #[serde(default)]
    pub generation_status: GenerationStatus,
    
    /// Whether this entity can be serialized/saved
    pub archivable: bool,
    
    /// AI training opt-in flag (default: false)
    /// When true, this entity is included in SpatialVortex training data exports
    /// Controls quality of AI training by allowing selective data inclusion
    #[serde(default)]
    pub ai: bool,
}

// ============================================================================
// Network Ownership Rule
// ============================================================================

/// Defines default network ownership behavior for an entity.
/// 
/// This is persisted in the scene file and determines how ownership
/// is resolved when the scene loads or when clients interact with entities.
/// 
/// # Examples
/// - NPCs: `ServerOnly` - always server-controlled
/// - Player spawns: `SpawnOwner` - owned by spawning client
/// - Physics props: `ClientClaimable` - can be claimed on interaction
/// - Static geometry: `ServerOnly` - never client-owned
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum NetworkOwnershipRule {
    /// Server always owns this entity. Clients cannot claim ownership.
    /// Use for: NPCs, critical game objects, static geometry.
    #[default]
    ServerOnly,
    
    /// Clients can request ownership of this entity.
    /// Server arbitrates based on proximity and availability.
    /// Use for: Physics props, vehicles, interactive objects.
    ClientClaimable,
    
    /// Entity is owned by the client that spawned it.
    /// Use for: Player characters, projectiles, client-spawned effects.
    SpawnOwner,
    
    /// Inherit ownership rule from parent entity.
    /// Use for: Child parts of a Model, attachments.
    Inherit,
    
    /// No network replication - local only.
    /// Use for: Client-side effects, UI elements, preview objects.
    LocalOnly,
}

impl Default for Entity {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Entity".to_string(),
            parent: None,
            children: Vec::new(),
            class: EntityClass::Folder,
            transform: TransformData::default(),
            network_ownership: NetworkOwnershipRule::default(),
            prompt: String::new(),
            detail_level: DetailLevel::Medium,
            category: NodeCategory::Empty,
            quest_flags: HashMap::new(),
            generated_mesh_id: None,
            generated_texture_id: None,
            generated_lods: Vec::new(),
            generation_status: GenerationStatus::default(),
            archivable: true,
            ai: false,
        }
    }
}

// ============================================================================
// Generation Status
// ============================================================================

/// Status of AI asset generation for an entity
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Reflect, Default)]
pub enum GenerationStatus {
    /// No generation requested
    #[default]
    NotRequested,
    /// Generation queued, waiting for AI server
    Pending,
    /// Currently generating
    InProgress {
        /// Progress percentage (0.0 - 1.0)
        progress: f32,
        /// Current stage description
        stage: String,
    },
    /// Generation completed successfully
    Complete {
        /// When generation finished (Unix timestamp)
        completed_at: u64,
        /// Generation time in milliseconds
        generation_time_ms: u64,
    },
    /// Generation failed
    Failed {
        /// Error message
        error: String,
        /// When it failed (Unix timestamp)
        failed_at: u64,
    },
}

/// Transform data for serialization
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 4], // Quaternion (x, y, z, w)
    pub scale: [f32; 3],
}

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl From<Transform> for TransformData {
    fn from(t: Transform) -> Self {
        Self {
            position: [t.translation.x, t.translation.y, t.translation.z],
            rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
            scale: [t.scale.x, t.scale.y, t.scale.z],
        }
    }
}

impl From<TransformData> for Transform {
    fn from(t: TransformData) -> Self {
        Transform {
            translation: Vec3::new(t.position[0], t.position[1], t.position[2]),
            rotation: Quat::from_xyzw(t.rotation[0], t.rotation[1], t.rotation[2], t.rotation[3]),
            scale: Vec3::new(t.scale[0], t.scale[1], t.scale[2]),
        }
    }
}

// ============================================================================
// 5. Entity Class Enum
// ============================================================================

/// All supported entity classes - Eustress-style with extensions
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub enum EntityClass {
    // --- Core Classes ---
    Folder,
    Model(ModelData),
    Part(PartData),
    MeshPart(MeshPartData),
    UnionOperation(UnionOperationData),
    
    // --- Characters ---
    Humanoid(HumanoidData),
    
    // --- Lighting ---
    PointLight(PointLightData),
    SpotLight(SpotLightData),
    SurfaceLight(SurfaceLightData),
    
    // --- Environment ---
    Terrain(TerrainData),
    Sky(SkyData),
    
    // --- Audio ---
    Sound(SoundData),
    
    // --- Effects ---
    ParticleEmitter(ParticleEmitterData),
    Beam(BeamData),
    
    // --- Attachments & Constraints ---
    Attachment(AttachmentData),
    WeldConstraint(WeldConstraintData),
    Motor6D(Motor6DData),
    
    // --- Mesh & Decals ---
    SpecialMesh(SpecialMeshData),
    Decal(DecalData),
    
    // --- Animation ---
    Animator(AnimatorData),
    KeyframeSequence(KeyframeSequenceData),
    
    // --- UI ---
    BillboardGui(BillboardGuiData),
    TextLabel(TextLabelData),
    
    // --- Camera ---
    Camera(CameraData),
    
    // --- AI/Narrative (new) ---
    Trigger(TriggerData),
    Portal(PortalData),
    NPC(NPCData),
    
    // --- Scripting ---
    SoulScript(SoulScriptData),
    
    // --- Orbital Coordinate Grid ---
    SolarSystem(SolarSystemData),
    CelestialBody(CelestialBodyData),
    RegionChunk(RegionChunkData),
}

impl Default for EntityClass {
    fn default() -> Self {
        EntityClass::Folder
    }
}

// ============================================================================
// 6. Class-Specific Data Structs
// ============================================================================

/// Part data - basic 3D primitive
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct PartData {
    pub size: [f32; 3],
    pub color: [f32; 4],
    pub material: String,
    pub shape: String, // "Block", "Ball", "Cylinder", "Wedge", "CornerWedge"
    pub transparency: f32,
    pub reflectance: f32,
    pub anchored: bool,
    pub can_collide: bool,
    pub cast_shadow: bool,
}

/// MeshPart data - custom mesh
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct MeshPartData {
    pub size: [f32; 3],
    pub color: [f32; 4],
    pub material: String,
    pub mesh_id: String,
    pub texture_id: String,
    pub transparency: f32,
    pub anchored: bool,
    pub can_collide: bool,
}

/// Model data - container/group
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct ModelData {
    pub primary_part: Option<u32>,
}

/// Humanoid data - character controller
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct HumanoidData {
    pub health: f32,
    pub max_health: f32,
    pub walk_speed: f32,
    pub jump_power: f32,
    pub rig_type: String, // "R6", "R15"
}

impl Default for HumanoidData {
    fn default() -> Self {
        Self {
            health: 100.0,
            max_health: 100.0,
            walk_speed: 16.0,
            jump_power: 50.0,
            rig_type: "R15".to_string(),
        }
    }
}

/// PointLight data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct PointLightData {
    pub brightness: f32,
    pub color: [f32; 4],
    pub range: f32,
    pub shadows: bool,
    pub enabled: bool,
}

impl Default for PointLightData {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            range: 8.0,
            shadows: true,
            enabled: true,
        }
    }
}

/// SpotLight data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SpotLightData {
    pub brightness: f32,
    pub color: [f32; 4],
    pub range: f32,
    pub angle: f32,
    pub shadows: bool,
    pub enabled: bool,
}

impl Default for SpotLightData {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            range: 16.0,
            angle: 45.0,
            shadows: true,
            enabled: true,
        }
    }
}

/// SurfaceLight data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SurfaceLightData {
    pub brightness: f32,
    pub color: [f32; 4],
    pub range: f32,
    pub face: String, // "Top", "Bottom", "Front", "Back", "Left", "Right"
    pub shadows: bool,
    pub enabled: bool,
}

impl Default for SurfaceLightData {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            range: 8.0,
            face: "Front".to_string(),
            shadows: false,
            enabled: true,
        }
    }
}

/// Terrain data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct TerrainData {
    pub water_color: [f32; 4],
    pub water_reflectance: f32,
    pub water_transparency: f32,
    pub water_wave_size: f32,
    pub water_wave_speed: f32,
}

/// Sky data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SkyData {
    pub skybox_top: String,
    pub skybox_bottom: String,
    pub skybox_front: String,
    pub skybox_back: String,
    pub skybox_left: String,
    pub skybox_right: String,
    pub sun_angular_size: f32,
    pub moon_angular_size: f32,
    pub stars_count: u32,
    pub celestial_bodies_shown: bool,
}

impl Default for SkyData {
    fn default() -> Self {
        Self {
            skybox_top: String::new(),
            skybox_bottom: String::new(),
            skybox_front: String::new(),
            skybox_back: String::new(),
            skybox_left: String::new(),
            skybox_right: String::new(),
            sun_angular_size: 21.0,
            moon_angular_size: 11.0,
            stars_count: 3000,
            celestial_bodies_shown: true,
        }
    }
}

/// Sound data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SoundData {
    pub sound_id: String,
    pub volume: f32,
    pub pitch: f32,
    pub looped: bool,
    pub playing: bool,
    pub spatial: bool,
    pub rolloff_min: f32,
    pub rolloff_max: f32,
}

impl Default for SoundData {
    fn default() -> Self {
        Self {
            sound_id: String::new(),
            volume: 0.5,
            pitch: 1.0,
            looped: false,
            playing: false,
            spatial: true,
            rolloff_min: 10.0,
            rolloff_max: 100.0,
        }
    }
}

/// ParticleEmitter data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct ParticleEmitterData {
    pub texture: String,
    pub color: [f32; 4],
    pub rate: f32,
    pub lifetime_min: f32,
    pub lifetime_max: f32,
    pub speed_min: f32,
    pub speed_max: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub enabled: bool,
}

impl Default for ParticleEmitterData {
    fn default() -> Self {
        Self {
            texture: String::new(),
            color: [1.0, 1.0, 1.0, 1.0],
            rate: 20.0,
            lifetime_min: 1.0,
            lifetime_max: 2.0,
            speed_min: 1.0,
            speed_max: 5.0,
            size_min: 0.1,
            size_max: 0.5,
            enabled: true,
        }
    }
}

/// Beam data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct BeamData {
    pub attachment0: Option<u32>,
    pub attachment1: Option<u32>,
    pub color: [f32; 4],
    pub width0: f32,
    pub width1: f32,
    pub texture: String,
    pub enabled: bool,
}

impl Default for BeamData {
    fn default() -> Self {
        Self {
            attachment0: None,
            attachment1: None,
            color: [1.0, 1.0, 1.0, 1.0],
            width0: 1.0,
            width1: 1.0,
            texture: String::new(),
            enabled: true,
        }
    }
}

/// Attachment data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct AttachmentData {
    pub position: [f32; 3],
    pub orientation: [f32; 3],
    pub visible: bool,
}

/// WeldConstraint data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct WeldConstraintData {
    pub part0: Option<u32>,
    pub part1: Option<u32>,
    pub enabled: bool,
}

/// Motor6D data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct Motor6DData {
    pub part0: Option<u32>,
    pub part1: Option<u32>,
    pub c0: TransformData,
    pub c1: TransformData,
    pub enabled: bool,
}

/// SpecialMesh data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SpecialMeshData {
    pub mesh_id: String,
    pub mesh_type: String, // "FileMesh", "Head", "Sphere", "Cylinder", etc.
    pub scale: [f32; 3],
    pub offset: [f32; 3],
    pub texture_id: String,
}

impl Default for SpecialMeshData {
    fn default() -> Self {
        Self {
            mesh_id: String::new(),
            mesh_type: "FileMesh".to_string(),
            scale: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            texture_id: String::new(),
        }
    }
}

/// Decal data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct DecalData {
    pub texture: String,
    pub face: String,
    pub color: [f32; 4],
    pub transparency: f32,
}

impl Default for DecalData {
    fn default() -> Self {
        Self {
            texture: String::new(),
            face: "Front".to_string(),
            color: [1.0, 1.0, 1.0, 1.0],
            transparency: 0.0,
        }
    }
}

/// Animator data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct AnimatorData {
    // Animator is mostly runtime state
}

/// KeyframeSequence data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct KeyframeSequenceData {
    pub priority: String,
    pub looped: bool,
}

/// UnionOperation data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct UnionOperationData {
    pub size: [f32; 3],
    pub color: [f32; 4],
    pub material: String,
    pub transparency: f32,
    pub anchored: bool,
    pub can_collide: bool,
}

/// BillboardGui data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct BillboardGuiData {
    pub size: [f32; 2],
    pub study_distance: f32,
    pub max_distance: f32,
    pub always_on_top: bool,
    pub enabled: bool,
}

impl Default for BillboardGuiData {
    fn default() -> Self {
        Self {
            size: [200.0, 50.0],
            study_distance: 10.0,
            max_distance: 100.0,
            always_on_top: false,
            enabled: true,
        }
    }
}

/// TextLabel data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct TextLabelData {
    pub text: String,
    pub text_color: [f32; 4],
    pub text_size: f32,
    pub font: String,
    pub background_color: [f32; 4],
    pub background_transparency: f32,
}

impl Default for TextLabelData {
    fn default() -> Self {
        Self {
            text: "Label".to_string(),
            text_color: [1.0, 1.0, 1.0, 1.0],
            text_size: 14.0,
            font: "SourceSans".to_string(),
            background_color: [0.0, 0.0, 0.0, 0.5],
            background_transparency: 0.5,
        }
    }
}

/// Camera data
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct CameraData {
    pub field_of_view: f32,
    pub camera_type: String, // "Custom", "Scriptable", "Track", etc.
    pub near_clip: f32,
    pub far_clip: f32,
}

impl Default for CameraData {
    fn default() -> Self {
        Self {
            field_of_view: 70.0,
            camera_type: "Custom".to_string(),
            near_clip: 0.1,
            far_clip: 10000.0,
        }
    }
}

// --- AI/Narrative Classes (new) ---

/// Trigger data - invisible gameplay zone
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct TriggerData {
    pub size: [f32; 3],
    pub on_enter: String,  // Event/script name
    pub on_exit: String,
    pub enabled: bool,
}

impl Default for TriggerData {
    fn default() -> Self {
        Self {
            size: [4.0, 4.0, 4.0],
            on_enter: String::new(),
            on_exit: String::new(),
            enabled: true,
        }
    }
}

/// Portal data - teleport/transition point
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct PortalData {
    pub destination: Option<u32>, // Target entity ID
    pub destination_scene: String, // Or another scene file
    pub enabled: bool,
}

impl Default for PortalData {
    fn default() -> Self {
        Self {
            destination: None,
            destination_scene: String::new(),
            enabled: true,
        }
    }
}

/// NPC data - AI-driven character
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct NPCData {
    pub display_name: String,
    pub dialogue_prompt: String, // LLM prompt for dialogue generation
    pub personality: String,     // e.g. "wise elder", "mischievous merchant"
    pub voice_id: String,        // TTS voice identifier
    pub idle_animation: String,
    pub walk_speed: f32,
}

impl Default for NPCData {
    fn default() -> Self {
        Self {
            display_name: "NPC".to_string(),
            dialogue_prompt: String::new(),
            personality: String::new(),
            voice_id: String::new(),
            idle_animation: String::new(),
            walk_speed: 8.0,
        }
    }
}

// --- Scripting Classes ---

/// SoulScript data for client runtime
/// Only contains the generated Rust code needed for execution
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SoulScriptData {
    /// Generated Rust code to execute at runtime
    pub generated_code: String,
    /// Whether the script is enabled
    pub enabled: bool,
}

impl Default for SoulScriptData {
    fn default() -> Self {
        Self {
            generated_code: String::new(),
            enabled: true,
        }
    }
}

// ============================================================================
// 7. AI Enhancement Types
// ============================================================================

/// Detail level for AI generation quality vs speed
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Reflect, Hash, Default)]
pub enum DetailLevel {
    /// Fast preview, low poly, <500ms generation
    Low,
    /// Balanced quality/speed, ~1s generation
    #[default]
    Medium,
    /// Maximum quality, 2-4s generation, for hero assets
    High,
}

/// Node category - determines AI enhancement pipeline strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Hash, Default)]
pub enum NodeCategory {
    /// No special handling
    #[default]
    Empty,
    /// Natural terrain - generates heightmap + textures
    Terrain,
    /// Buildings, architecture - full mesh replacement
    Structure,
    /// Objects, items - mesh + textures
    Prop,
    /// Characters, creatures - mesh + rig + animations
    Character,
    /// Invisible gameplay zones
    Trigger,
    /// Teleport/transition points
    Portal,
    /// Dynamic lights
    LightSource,
    /// Spatial audio emitters
    AudioSource,
    /// AI-driven NPCs
    NPC,
}

// ============================================================================
// 8. Quest/Narrative Types
// ============================================================================

/// Connection between entities for quest/narrative graph
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct Connection {
    pub id: u32,
    pub from: u32,
    pub to: u32,
    
    /// Condition for this connection to activate
    /// e.g. "player has item:rusty_key" or LLM prompt fragment
    pub condition: String,
    
    /// Narrative text shown when traversed
    pub narrative: String,
    
    /// Type of connection
    pub connection_type: ConnectionType,
}

/// Type of connection between entities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum ConnectionType {
    /// Simple directional link
    OneWay,
    /// Bidirectional link
    TwoWay,
    /// Quest progression step
    QuestStep,
    /// Dialogue choice
    DialogueOption,
    /// Teleport connection
    Portal,
}

impl Default for ConnectionType {
    fn default() -> Self {
        ConnectionType::OneWay
    }
}

// ============================================================================
// 8b. Orbital Coordinate Grid Data Structs
// ============================================================================

/// SolarSystem data - orbital hierarchy container
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SolarSystemData {
    /// Time scale for orbital simulation (1.0 = real-time)
    pub time_scale: f64,
    /// Custom gravitational constant (default: 6.67430e-11)
    pub gravity_constant: f64,
    /// Whether n-body simulation is active
    pub simulation_active: bool,
    /// Reference frame origin (ECEF position of system barycenter)
    pub barycenter_ecef: [f64; 3],
}

impl Default for SolarSystemData {
    fn default() -> Self {
        Self {
            time_scale: 1.0,
            gravity_constant: 6.67430e-11,
            simulation_active: true,
            barycenter_ecef: [0.0, 0.0, 0.0],
        }
    }
}

/// CelestialBody data - orbital object with n-body gravity
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct CelestialBodyData {
    /// Global ECEF position (high precision)
    pub global_ecef: [f64; 3],
    /// Orbital velocity (m/s)
    pub orbital_velocity: [f64; 3],
    /// Mass in kilograms
    pub mass: f64,
    /// Gravitational parameter GM (m³/s²)
    pub gm: f64,
    /// Mean radius in meters
    pub radius: f64,
    /// Rotation period in seconds
    pub rotation_period: f64,
    /// Axial tilt in degrees
    pub axial_tilt: f32,
    /// Current rotation angle (degrees)
    pub rotation_angle: f32,
    /// Atmosphere thickness in meters
    pub atmosphere_height: f32,
    /// Surface material preset
    pub surface_material: String,
    /// Whether this body contributes to gravity
    pub gravitational: bool,
    /// Semi-major axis of orbit (meters)
    pub semi_major_axis: f64,
    /// Orbital eccentricity
    pub eccentricity: f64,
    /// Orbital inclination (degrees)
    pub inclination: f32,
}

impl Default for CelestialBodyData {
    fn default() -> Self {
        Self {
            global_ecef: [0.0, 0.0, 0.0],
            orbital_velocity: [0.0, 0.0, 0.0],
            mass: 5.972e24,           // Earth mass
            gm: 3.986004418e14,       // Earth GM
            radius: 6.371e6,          // Earth mean radius
            rotation_period: 86164.1, // Sidereal day
            axial_tilt: 23.44,
            rotation_angle: 0.0,
            atmosphere_height: 100_000.0,
            surface_material: "Grass".to_string(),
            gravitational: true,
            semi_major_axis: 0.0,
            eccentricity: 0.0,
            inclination: 0.0,
        }
    }
}

/// RegionChunk data - geospatial fragment with floating origin
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct RegionChunkData {
    /// Global center in ECEF (high precision)
    pub center_ecef: [f64; 3],
    /// Geodetic center (latitude, longitude, altitude)
    pub center_geodetic: [f64; 3],
    /// Local bounds extents (half-size in meters)
    pub bounds_extents: [f32; 3],
    /// Tile level in 3D Tiles hierarchy (0-24)
    pub tile_level: u8,
    /// Tile face (cube-sphere mapping, 0-5)
    pub tile_face: u8,
    /// Tile X index
    pub tile_x: u32,
    /// Tile Y index
    pub tile_y: u32,
    /// Whether Gaussian Splatting overlay is enabled
    pub gs_overlay_enabled: bool,
    /// Path/URL to point cloud or GIS asset
    pub gis_data_ref: String,
    /// Heightmap resolution (vertices per side)
    pub heightmap_resolution: u32,
    /// Water level relative to local origin
    pub water_level: f32,
    /// Custom gravity override [x, y, z] or None
    pub custom_gravity: Option<[f32; 3]>,
    /// Whether this is an abstract (non-Earth) region
    pub is_abstract: bool,
    /// Offset from parent region origin
    pub parent_offset: Option<[f32; 3]>,
    /// Whether region is currently active
    pub active: bool,
}

impl Default for RegionChunkData {
    fn default() -> Self {
        Self {
            center_ecef: [6_371_000.0, 0.0, 0.0],
            center_geodetic: [0.0, 0.0, 0.0],
            bounds_extents: [500.0, 500.0, 500.0],
            tile_level: 16,
            tile_face: 0,
            tile_x: 0,
            tile_y: 0,
            gs_overlay_enabled: false,
            gis_data_ref: String::new(),
            heightmap_resolution: 256,
            water_level: 0.0,
            custom_gravity: None,
            is_abstract: false,
            parent_offset: None,
            active: true,
        }
    }
}

// ============================================================================
// 9. Serialization Helpers - Eustress Binary Format
// ============================================================================

/// Magic bytes for .eustress scene files
pub const EUSTRESS_MAGIC: &[u8; 8] = b"EUSTRESS";

/// Current scene format version
pub const SCENE_FORMAT_VERSION: u32 = 1;

/// Load a scene from a .eustress binary file
pub fn load_scene_from_file(path: &std::path::Path) -> Result<Scene, String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    
    load_scene_from_bytes(&data)
}

/// Load a scene from binary data
pub fn load_scene_from_bytes(data: &[u8]) -> Result<Scene, String> {
    // Check minimum size
    if data.len() < 12 {
        return Err("File too small to be a valid .eustress scene".to_string());
    }
    
    // Check magic bytes
    if &data[0..8] != EUSTRESS_MAGIC {
        return Err("Invalid file format: missing EUSTRESS magic bytes".to_string());
    }
    
    // Read version
    let version = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    if version > SCENE_FORMAT_VERSION {
        return Err(format!(
            "Scene format version {} is newer than supported version {}",
            version, SCENE_FORMAT_VERSION
        ));
    }
    
    // Decompress and deserialize (using bincode + lz4)
    let compressed = &data[12..];
    let decompressed = lz4_flex::decompress_size_prepended(compressed)
        .map_err(|e| format!("Failed to decompress scene: {}", e))?;
    
    bincode::deserialize(&decompressed)
        .map_err(|e| format!("Failed to deserialize scene: {}", e))
}

/// Save a scene to a .eustress binary file
pub fn save_scene_to_file(scene: &Scene, path: &std::path::Path) -> Result<(), String> {
    let data = save_scene_to_bytes(scene)?;
    std::fs::write(path, data)
        .map_err(|e| format!("Failed to write file: {}", e))
}

/// Save a scene to binary data
pub fn save_scene_to_bytes(scene: &Scene) -> Result<Vec<u8>, String> {
    // Serialize with bincode
    let serialized = bincode::serialize(scene)
        .map_err(|e| format!("Failed to serialize scene: {}", e))?;
    
    // Compress with lz4
    let compressed = lz4_flex::compress_prepend_size(&serialized);
    
    // Build final binary: magic + version + compressed data
    let mut output = Vec::with_capacity(12 + compressed.len());
    output.extend_from_slice(EUSTRESS_MAGIC);
    output.extend_from_slice(&SCENE_FORMAT_VERSION.to_le_bytes());
    output.extend_from_slice(&compressed);
    
    Ok(output)
}

/// Get scene file size estimate (for UI display)
pub fn estimate_scene_size(scene: &Scene) -> usize {
    // Rough estimate: entity count * avg bytes per entity
    let base = 256; // Header overhead
    let per_entity = 512; // Average entity size
    base + scene.entities.len() * per_entity
}
