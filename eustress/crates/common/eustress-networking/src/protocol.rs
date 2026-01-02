//! # Eustress Network Protocol
//!
//! Defines the network protocol for Eustress:
//! - Replicated components (Transform, Velocity, etc.)
//! - Messages (RPC, ownership, input)
//! - Channels (reliable, unreliable, ordered)
//!
//! ## Design
//!
//! This module defines the core network types that can be used with
//! any networking backend (Lightyear, custom QUIC, etc.).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::scale::{MAX_SPEED, POSITION_QUANTUM};

// ============================================================================
// Protocol Version
// ============================================================================

/// Protocol version for compatibility checking.
pub const PROTOCOL_VERSION: u32 = 1;

/// Protocol ID (unique identifier for this game).
pub const PROTOCOL_ID: u64 = 0xE057_0001;

// ============================================================================
// Channels
// ============================================================================

/// Network channel types for different message priorities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EustressChannel {
    /// Reliable ordered channel for critical messages (ownership, spawn, despawn).
    Reliable,
    /// Unreliable channel for frequent updates (position, velocity).
    Unreliable,
    /// Sequenced channel for inputs (drop old, keep newest).
    Input,
    /// Reliable unordered for events (can arrive out of order but guaranteed).
    Events,
}

// ============================================================================
// Replicated Components
// ============================================================================

/// Network-replicated position component.
///
/// Uses quantization for bandwidth efficiency.
/// Separate from Bevy's Transform for network-specific handling.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
#[reflect(Component)]
pub struct NetworkTransform {
    /// Position in studs (quantized for network)
    pub position: Vec3,
    /// Rotation as quaternion
    pub rotation: Quat,
    /// Scale (usually 1.0 for physics objects)
    pub scale: Vec3,
}

impl Default for NetworkTransform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl NetworkTransform {
    /// Create from Bevy Transform
    pub fn from_transform(transform: &Transform) -> Self {
        Self {
            position: transform.translation,
            rotation: transform.rotation,
            scale: transform.scale,
        }
    }

    /// Convert to Bevy Transform
    pub fn to_transform(&self) -> Transform {
        Transform {
            translation: self.position,
            rotation: self.rotation,
            scale: self.scale,
        }
    }

    /// Check if significantly different from another (for delta compression)
    pub fn differs_from(&self, other: &Self, threshold: f32) -> bool {
        self.position.distance_squared(other.position) > threshold * threshold
            || self.rotation.angle_between(other.rotation) > threshold
            || self.scale.distance_squared(other.scale) > threshold * threshold
    }
}

/// Network-replicated velocity component.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkVelocity {
    /// Linear velocity in studs/second
    pub linear: Vec3,
    /// Angular velocity in radians/second
    pub angular: Vec3,
}

impl NetworkVelocity {
    /// Check if within valid bounds
    pub fn is_valid(&self) -> bool {
        self.linear.length() <= MAX_SPEED && self.angular.length() <= MAX_SPEED
    }

    /// Clamp to valid bounds
    pub fn clamped(&self) -> Self {
        Self {
            linear: if self.linear.length() > MAX_SPEED {
                self.linear.normalize() * MAX_SPEED
            } else {
                self.linear
            },
            angular: if self.angular.length() > MAX_SPEED {
                self.angular.normalize() * MAX_SPEED
            } else {
                self.angular
            },
        }
    }
}

/// Network-replicated entity metadata.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
#[reflect(Component)]
pub struct NetworkEntity {
    /// Unique network ID (stable across clients)
    pub net_id: u64,
    /// Entity class name (Part, Model, Humanoid, etc.)
    pub class_name: String,
    /// Display name
    pub name: String,
    /// Parent entity network ID (0 = root)
    pub parent_net_id: u64,
}

impl Default for NetworkEntity {
    fn default() -> Self {
        Self {
            net_id: 0,
            class_name: "Instance".to_string(),
            name: "Entity".to_string(),
            parent_net_id: 0,
        }
    }
}

/// Network-replicated health/state component.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkHealth {
    pub current: f32,
    pub max: f32,
}

/// Network-replicated Parameters component.
/// 
/// This is a lightweight version of Parameters for network replication.
/// Only the essential fields are replicated; full Parameters can be
/// fetched from the server on demand.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkParameters {
    /// Domain identifier (e.g., "patient", "sensor", "inventory")
    pub domain: String,
    /// Resource identifier within the domain
    pub resource_id: String,
    /// Reference to global data source
    pub source_ref: Option<String>,
    /// Last update timestamp (Unix epoch seconds)
    pub last_updated: f64,
    /// Serialized custom parameters (JSON string for flexibility)
    pub custom_data: String,
    /// Hash of the full Parameters for change detection
    pub data_hash: u64,
}

impl NetworkParameters {
    /// Create from full Parameters component
    pub fn from_parameters(params: &eustress_common::parameters::Parameters) -> Self {
        // Compute a simple hash for change detection
        let hash = Self::compute_hash(params);
        
        // Serialize custom params to JSON
        let custom_data = serde_json::to_string(&params.custom_params).unwrap_or_default();
        
        Self {
            domain: params.domain.clone(),
            resource_id: params.resource_id.clone(),
            source_ref: params.global_source_ref.clone(),
            last_updated: params.last_updated.unwrap_or(0.0),
            custom_data,
            data_hash: hash,
        }
    }
    
    /// Check if significantly different from another (for delta compression)
    pub fn differs_from(&self, other: &Self) -> bool {
        self.data_hash != other.data_hash
    }
    
    /// Compute a hash of Parameters for change detection
    fn compute_hash(params: &eustress_common::parameters::Parameters) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        
        let mut hasher = DefaultHasher::new();
        params.domain.hash(&mut hasher);
        params.resource_id.hash(&mut hasher);
        params.global_source_ref.hash(&mut hasher);
        
        // Hash custom params keys and values
        let mut keys: Vec<_> = params.custom_params.keys().collect();
        keys.sort();
        for key in keys {
            key.hash(&mut hasher);
            if let Some(value) = params.custom_params.get(key) {
                value.hash(&mut hasher);
            }
        }
        
        hasher.finish()
    }
}

/// Network-replicated domain scope data from Parameters.
/// 
/// Replicates domain scope and sync configuration for data-bound containers.
/// Domain scope is now stored in Parameters component, not Folder/Model.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkDomainScope {
    /// Domain this entity scopes to
    pub domain: String,
    /// Source override for this scope
    pub source_override: Option<String>,
    /// Whether this is a domain scope container
    pub is_domain_scope: bool,
    /// Whether sync config exists (full config fetched on demand)
    pub has_sync_config: bool,
}

impl NetworkDomainScope {
    /// Create from Parameters component
    pub fn from_parameters(params: &eustress_common::parameters::Parameters) -> Self {
        Self {
            domain: params.domain.clone(),
            source_override: params.global_source_ref.clone(),
            is_domain_scope: params.is_domain_scope,
            has_sync_config: params.sync_config.is_some(),
        }
    }
    
    /// Check if differs from another
    pub fn differs_from(&self, other: &Self) -> bool {
        self.domain != other.domain 
            || self.source_override != other.source_override
            || self.is_domain_scope != other.is_domain_scope
            || self.has_sync_config != other.has_sync_config
    }
}

/// Network-replicated Attributes component.
/// 
/// Lightweight version of Attributes for network replication.
/// Only replicates a hash and count; full attributes fetched on demand.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkAttributes {
    /// Number of attributes
    pub count: u32,
    /// Hash of all attributes for change detection
    pub data_hash: u64,
    /// Serialized attributes (JSON, only sent on full sync)
    pub serialized: Option<String>,
}

impl NetworkAttributes {
    /// Create from Attributes component
    pub fn from_attributes(attrs: &eustress_common::attributes::Attributes) -> Self {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        
        let mut hasher = DefaultHasher::new();
        let mut keys: Vec<_> = attrs.keys().collect();
        keys.sort();
        for key in &keys {
            key.hash(&mut hasher);
            if let Some(value) = attrs.get(key) {
                // Hash the debug representation
                format!("{:?}", value).hash(&mut hasher);
            }
        }
        
        Self {
            count: attrs.len() as u32,
            data_hash: hasher.finish(),
            serialized: None, // Only populated for full sync
        }
    }
    
    /// Create with serialized data (for full sync)
    pub fn with_serialized(mut self, attrs: &eustress_common::attributes::Attributes) -> Self {
        // Serialize via iter
        let map: std::collections::HashMap<_, _> = attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        self.serialized = serde_json::to_string(&map).ok();
        self
    }
    
    /// Check if differs from another
    pub fn differs_from(&self, other: &Self) -> bool {
        self.data_hash != other.data_hash
    }
}

/// Network-replicated Tags component.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkTags {
    /// All tags as a sorted list
    pub tags: Vec<String>,
}

impl NetworkTags {
    /// Create from Tags component
    pub fn from_tags(tags: &eustress_common::attributes::Tags) -> Self {
        let mut tag_list: Vec<_> = tags.iter().cloned().collect();
        tag_list.sort();
        Self { tags: tag_list }
    }
    
    /// Check if differs from another
    pub fn differs_from(&self, other: &Self) -> bool {
        self.tags != other.tags
    }
}

// ============================================================================
// Media Asset Network Components
// ============================================================================

/// Network-replicated Document component.
/// Replicates document metadata for collaborative viewing.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkDocument {
    /// Document type as string
    pub document_type: String,
    /// Source type
    pub source_type: String,
    /// Asset ID (for asset pipeline)
    pub asset_id: Option<String>,
    /// Cloud URL (resolved)
    pub cloud_url: Option<String>,
    /// File size
    pub file_size: u64,
    /// Page count
    pub page_count: Option<u32>,
    /// Content hash for sync
    pub content_hash: Option<String>,
}

impl NetworkDocument {
    pub fn from_document(doc: &eustress_common::classes::Document) -> Self {
        Self {
            document_type: format!("{:?}", doc.document_type),
            source_type: format!("{:?}", doc.source_type),
            asset_id: doc.asset_id.clone(),
            cloud_url: if doc.cloud_bucket.is_some() && doc.cloud_key.is_some() {
                Some(format!("s3://{}/{}", 
                    doc.cloud_bucket.as_ref().unwrap_or(&String::new()),
                    doc.cloud_key.as_ref().unwrap_or(&String::new())))
            } else {
                None
            },
            file_size: doc.file_size,
            page_count: doc.page_count,
            content_hash: doc.content_hash.clone(),
        }
    }
    
    pub fn differs_from(&self, other: &Self) -> bool {
        self.content_hash != other.content_hash || self.asset_id != other.asset_id
    }
}

/// Network-replicated ImageAsset component.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkImageAsset {
    /// Image format
    pub format: String,
    /// Source type
    pub source_type: String,
    /// Asset ID
    pub asset_id: Option<String>,
    /// Cloud URL
    pub cloud_url: Option<String>,
    /// Dimensions
    pub width: u32,
    pub height: u32,
    /// File size
    pub file_size: u64,
    /// Animated
    pub animated: bool,
    /// Content hash
    pub content_hash: Option<String>,
}

impl NetworkImageAsset {
    pub fn from_image_asset(img: &eustress_common::classes::ImageAsset) -> Self {
        Self {
            format: format!("{:?}", img.format),
            source_type: format!("{:?}", img.source_type),
            asset_id: img.asset_id.clone(),
            cloud_url: if img.cloud_bucket.is_some() && img.cloud_key.is_some() {
                Some(format!("s3://{}/{}", 
                    img.cloud_bucket.as_ref().unwrap_or(&String::new()),
                    img.cloud_key.as_ref().unwrap_or(&String::new())))
            } else {
                None
            },
            width: img.width,
            height: img.height,
            file_size: img.file_size,
            animated: img.animated,
            content_hash: img.content_hash.clone(),
        }
    }
    
    pub fn differs_from(&self, other: &Self) -> bool {
        self.content_hash != other.content_hash || self.asset_id != other.asset_id
    }
}

/// Network-replicated VideoAsset component.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Reflect)]
#[reflect(Component)]
pub struct NetworkVideoAsset {
    /// Video format
    pub format: String,
    /// Source type
    pub source_type: String,
    /// Asset ID
    pub asset_id: Option<String>,
    /// Cloud/streaming URL
    pub url: Option<String>,
    /// Dimensions
    pub width: u32,
    pub height: u32,
    /// Duration in seconds
    pub duration: f32,
    /// File size
    pub file_size: u64,
    /// Playback state
    pub looping: bool,
    pub autoplay: bool,
    pub volume: f32,
    /// Content hash
    pub content_hash: Option<String>,
}

impl NetworkVideoAsset {
    pub fn from_video_asset(vid: &eustress_common::classes::VideoAsset) -> Self {
        Self {
            format: format!("{:?}", vid.format),
            source_type: format!("{:?}", vid.source_type),
            asset_id: vid.asset_id.clone(),
            url: vid.streaming_url.clone().or_else(|| {
                if vid.cloud_bucket.is_some() && vid.cloud_key.is_some() {
                    Some(format!("s3://{}/{}", 
                        vid.cloud_bucket.as_ref().unwrap_or(&String::new()),
                        vid.cloud_key.as_ref().unwrap_or(&String::new())))
                } else {
                    None
                }
            }),
            width: vid.width,
            height: vid.height,
            duration: vid.duration,
            file_size: vid.file_size,
            looping: vid.looping,
            autoplay: vid.autoplay,
            volume: vid.volume,
            content_hash: vid.content_hash.clone(),
        }
    }
    
    pub fn differs_from(&self, other: &Self) -> bool {
        self.content_hash != other.content_hash 
            || self.asset_id != other.asset_id
            || self.looping != other.looping
            || self.volume != other.volume
    }
}

// ============================================================================
// Messages
// ============================================================================

/// All network messages for Eustress.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum EustressMessage {
    // === Connection ===
    /// Client requests to join
    JoinRequest { player_name: String, version: u32 },
    /// Server accepts join
    JoinAccepted { client_id: u64, tick: u64 },
    /// Server rejects join
    JoinRejected { reason: String },
    /// Client disconnecting gracefully
    Disconnect { reason: String },

    // === Input ===
    /// Client input for owned entities
    Input(PlayerInput),

    // === Ownership ===
    /// Request ownership of an entity
    OwnershipRequest { net_id: u64 },
    /// Server grants ownership
    OwnershipGranted { net_id: u64, client_id: u64 },
    /// Server denies ownership
    OwnershipDenied { net_id: u64, reason: String },
    /// Server transfers ownership
    OwnershipTransfer { net_id: u64, from_client: u64, to_client: u64 },
    /// Client releases ownership
    OwnershipRelease { net_id: u64 },

    // === Entity Lifecycle ===
    /// Server spawns entity
    EntitySpawn { net_id: u64, class_name: String, transform: NetworkTransform },
    /// Server despawns entity
    EntityDespawn { net_id: u64 },
    /// Entity reparented
    EntityReparent { net_id: u64, new_parent: u64 },

    // === State ===
    /// Full world state (on join or periodic sync)
    WorldState { tick: u64, entities: Vec<EntityState> },
    /// Delta update (frequent)
    DeltaUpdate { tick: u64, updates: Vec<EntityDelta> },

    // === RPC ===
    /// Generic RPC call
    Rpc { name: String, args: Vec<u8> },

    // === Admin ===
    /// Kick client
    Kick { client_id: u64, reason: String },
    /// Server announcement
    Announcement { message: String },
}

/// Player input state (sent every tick from client).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerInput {
    /// Input tick number
    pub tick: u64,
    /// Movement direction (normalized)
    pub movement: Vec3,
    /// Look direction (yaw, pitch)
    pub look: Vec2,
    /// Jump pressed
    pub jump: bool,
    /// Sprint pressed
    pub sprint: bool,
    /// Primary action (e.g., attack)
    pub primary: bool,
    /// Secondary action (e.g., aim)
    pub secondary: bool,
    /// Additional action flags
    pub actions: u32,
}

/// Full entity state for world sync.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EntityState {
    pub net_id: u64,
    pub class_name: String,
    pub name: String,
    pub parent_net_id: u64,
    pub owner_client_id: u64,
    pub transform: NetworkTransform,
    pub velocity: NetworkVelocity,
    /// Serialized component data
    pub components: Vec<u8>,
}

/// Delta update for a single entity.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EntityDelta {
    pub net_id: u64,
    /// Bitmask of changed fields
    pub changed: u32,
    /// Optional transform update
    pub transform: Option<NetworkTransform>,
    /// Optional velocity update
    pub velocity: Option<NetworkVelocity>,
    /// Optional component updates
    pub components: Option<Vec<u8>>,
}

impl EntityDelta {
    pub const TRANSFORM_CHANGED: u32 = 1 << 0;
    pub const VELOCITY_CHANGED: u32 = 1 << 1;
    pub const COMPONENTS_CHANGED: u32 = 1 << 2;
}

// ============================================================================
// Protocol Plugin
// ============================================================================

/// Plugin that registers the Eustress protocol with Lightyear.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // Register replicated components
        app.register_type::<NetworkTransform>()
            .register_type::<NetworkVelocity>()
            .register_type::<NetworkEntity>()
            .register_type::<NetworkHealth>()
            .register_type::<NetworkParameters>();

        // Lightyear protocol registration happens in server/client plugins
        // This plugin just ensures types are available

        info!("Eustress Protocol v{} registered", PROTOCOL_VERSION);
    }
}

// ============================================================================
// Protocol Definition for Lightyear
// ============================================================================

/// Eustress protocol definition for Lightyear.
///
/// This struct defines all the components, messages, and channels
/// that Lightyear will use for replication.
#[derive(Debug, Clone)]
pub struct EustressProtocol;

impl EustressProtocol {
    /// Get the protocol ID
    pub fn id() -> u64 {
        PROTOCOL_ID
    }

    /// Get the protocol version
    pub fn version() -> u32 {
        PROTOCOL_VERSION
    }
}

// ============================================================================
// Sync Helpers
// ============================================================================

/// Sync Transform -> NetworkTransform
pub fn sync_transform_to_network(
    mut query: Query<(&Transform, &mut NetworkTransform), Changed<Transform>>,
) {
    for (transform, mut net_transform) in query.iter_mut() {
        let new = NetworkTransform::from_transform(transform);
        if net_transform.differs_from(&new, POSITION_QUANTUM) {
            *net_transform = new;
        }
    }
}

/// Sync NetworkTransform -> Transform
pub fn sync_network_to_transform(
    mut query: Query<(&NetworkTransform, &mut Transform), Changed<NetworkTransform>>,
) {
    for (net_transform, mut transform) in query.iter_mut() {
        *transform = net_transform.to_transform();
    }
}

/// Sync Parameters -> NetworkParameters (server-side)
pub fn sync_parameters_to_network(
    mut query: Query<(&eustress_common::parameters::Parameters, &mut NetworkParameters), Changed<eustress_common::parameters::Parameters>>,
) {
    for (params, mut net_params) in query.iter_mut() {
        let new = NetworkParameters::from_parameters(params);
        if net_params.differs_from(&new) {
            *net_params = new;
        }
    }
}

/// Sync NetworkParameters -> Parameters (client-side)
/// 
/// This applies received network data to the local Parameters component.
/// Note: This only updates custom_params from the network; other fields
/// are managed locally or fetched from the server on demand.
pub fn sync_network_to_parameters(
    mut query: Query<(&NetworkParameters, &mut eustress_common::parameters::Parameters), Changed<NetworkParameters>>,
) {
    for (net_params, mut params) in query.iter_mut() {
        // Update basic fields
        params.domain = net_params.domain.clone();
        params.resource_id = net_params.resource_id.clone();
        params.global_source_ref = net_params.source_ref.clone();
        params.last_updated = Some(net_params.last_updated);
        
        // Deserialize custom params from JSON
        if let Ok(custom) = serde_json::from_str(&net_params.custom_data) {
            params.custom_params = custom;
        }
    }
}

