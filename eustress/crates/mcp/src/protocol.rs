//! Eustress Export Protocol (EEP) v1.0 - MCP Protocol Types
//!
//! Defines the standard protocol for exporting consented spatial instances from
//! EustressEngine to MCP Servers hosted on AI Models.
//!
//! ## Core Principles
//!
//! - **Consented**: Opt-in only (AI = true)
//! - **Hierarchical**: Preserves full parent/child structure
//! - **Multimodal**: Includes geometry, properties, tags, attributes, parameters
//! - **Real-time capable**: Supports live streaming and batch export
//! - **Vendor-neutral**: Works with any AI model/provider

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::{EntityData, PropertyMap, SpaceData, TransformData};

// ============================================================================
// Protocol Version & Capabilities
// ============================================================================

/// MCP Protocol version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpProtocolVersion {
    /// Major version
    pub major: u32,
    /// Minor version
    pub minor: u32,
    /// Protocol name
    pub name: String,
}

impl Default for McpProtocolVersion {
    fn default() -> Self {
        Self {
            major: 1,
            minor: 0,
            name: "eep_v1".to_string(),
        }
    }
}

/// MCP Server capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCapability {
    /// Capability name
    pub name: String,
    /// Whether this capability is supported
    pub supported: bool,
    /// Capability version
    pub version: Option<String>,
}

/// Standard capability names
pub mod capabilities {
    pub const ENTITY_CRUD: &str = "entity_crud";
    pub const SPATIAL_EXPORT: &str = "spatial_export";
    pub const TRAINING_DATA: &str = "training_data";
    pub const RUNE_EXECUTION: &str = "rune_execution";
    pub const REALTIME_STREAMING: &str = "realtime_streaming";
    pub const BATCH_EXPORT: &str = "batch_export";
    pub const QUERY: &str = "query";
}

// ============================================================================
// Request Types
// ============================================================================

/// Create entity request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntityRequest {
    /// Space ID to create entity in
    pub space_id: String,
    /// Entity class type
    pub class: String,
    /// Entity name
    #[serde(default)]
    pub name: Option<String>,
    /// Parent entity ID
    pub parent: Option<String>,
    /// Position [x, y, z]
    #[serde(default)]
    pub position: Option<[f32; 3]>,
    /// Rotation as Euler angles [x, y, z] in degrees
    #[serde(default)]
    pub rotation: Option<[f32; 3]>,
    /// Scale [x, y, z]
    #[serde(default)]
    pub scale: Option<[f32; 3]>,
    /// Class-specific properties
    #[serde(default)]
    pub properties: PropertyMap,
    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// AI training opt-in (default: false)
    #[serde(default)]
    pub ai: bool,
    /// Instance parameters
    #[serde(default)]
    pub parameters: HashMap<String, HashMap<String, serde_json::Value>>,
}

/// Update entity request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEntityRequest {
    /// Space ID
    pub space_id: String,
    /// Entity ID to update
    pub entity_id: String,
    /// Properties to update (partial update)
    #[serde(default)]
    pub properties: Option<PropertyMap>,
    /// Transform updates
    #[serde(default)]
    pub transform: Option<TransformUpdate>,
    /// Tags to add
    #[serde(default)]
    pub add_tags: Vec<String>,
    /// Tags to remove
    #[serde(default)]
    pub remove_tags: Vec<String>,
    /// AI training opt-in update
    #[serde(default)]
    pub ai: Option<bool>,
    /// New parent entity ID
    #[serde(default)]
    pub parent: Option<String>,
    /// Parameter updates
    #[serde(default)]
    pub parameters: Option<HashMap<String, HashMap<String, serde_json::Value>>>,
}

/// Transform update (partial)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransformUpdate {
    /// Position [x, y, z]
    pub position: Option<[f32; 3]>,
    /// Rotation as Euler angles [x, y, z] in degrees
    pub rotation: Option<[f32; 3]>,
    /// Scale [x, y, z]
    pub scale: Option<[f32; 3]>,
}

/// Delete entity request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEntityRequest {
    /// Space ID
    pub space_id: String,
    /// Entity ID to delete
    pub entity_id: String,
    /// Whether to delete children recursively
    #[serde(default = "default_true")]
    pub recursive: bool,
}

fn default_true() -> bool {
    true
}

/// Query entities request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEntitiesRequest {
    /// Space ID to query
    pub space_id: String,
    /// Filter by class types
    #[serde(default)]
    pub classes: Vec<String>,
    /// Filter by tags (AND)
    #[serde(default)]
    pub tags: Vec<String>,
    /// Filter by AI opt-in status
    #[serde(default)]
    pub ai_only: bool,
    /// Filter by parent entity ID
    #[serde(default)]
    pub parent: Option<String>,
    /// Bounding box filter (min, max)
    #[serde(default)]
    pub bounds: Option<([f32; 3], [f32; 3])>,
    /// Property filters
    #[serde(default)]
    pub property_filters: Vec<PropertyFilter>,
    /// Maximum results
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Offset for pagination
    #[serde(default)]
    pub offset: u32,
    /// Include children in response
    #[serde(default)]
    pub include_children: bool,
}

fn default_limit() -> u32 {
    100
}

/// Property filter for queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyFilter {
    /// Property key
    pub key: String,
    /// Filter operator
    pub operator: FilterOperator,
    /// Filter value
    pub value: serde_json::Value,
}

/// Filter operators
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    GreaterOrEqual,
    LessOrEqual,
    Contains,
    StartsWith,
    EndsWith,
    Exists,
    NotExists,
}

/// Batch create request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCreateRequest {
    /// Space ID
    pub space_id: String,
    /// Entities to create
    pub entities: Vec<CreateEntityRequest>,
}

/// Batch update request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchUpdateRequest {
    /// Space ID
    pub space_id: String,
    /// Updates to apply
    pub updates: Vec<UpdateEntityRequest>,
}

/// Batch delete request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchDeleteRequest {
    /// Space ID
    pub space_id: String,
    /// Entity IDs to delete
    pub entity_ids: Vec<String>,
    /// Whether to delete children recursively
    #[serde(default = "default_true")]
    pub recursive: bool,
}

// ============================================================================
// Response Types
// ============================================================================

/// Entity response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityResponse {
    /// Success flag
    pub success: bool,
    /// Entity data (if successful)
    pub entity: Option<EntityData>,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Query response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    /// Success flag
    pub success: bool,
    /// Matching entities
    pub entities: Vec<EntityData>,
    /// Total count (for pagination)
    pub total: u64,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Space info response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceInfo {
    /// Success flag
    pub success: bool,
    /// Space data
    pub space: Option<SpaceData>,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Delete response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteResponse {
    /// Success flag
    pub success: bool,
    /// Number of entities deleted
    pub deleted_count: u32,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Batch response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    /// Success flag (all operations succeeded)
    pub success: bool,
    /// Total operations
    pub total: usize,
    /// Successful operations
    pub succeeded: usize,
    /// Failed operations
    pub failed: usize,
    /// Individual results
    pub results: Vec<OperationResult>,
}

/// Individual operation result in batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    /// Operation index
    pub index: usize,
    /// Success flag
    pub success: bool,
    /// Entity ID (if successful)
    pub entity_id: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
}

// ============================================================================
// WebSocket Messages
// ============================================================================

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum WsMessage {
    /// Subscribe to entity changes
    Subscribe(SubscribeRequest),
    /// Unsubscribe from entity changes
    Unsubscribe(UnsubscribeRequest),
    /// Entity change event
    EntityChange(EntityChangeMessage),
    /// Ping/pong for keepalive
    Ping,
    Pong,
    /// Error message
    Error(ErrorMessage),
}

/// Subscribe request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    /// Space ID to subscribe to
    pub space_id: String,
    /// Filter by entity classes
    #[serde(default)]
    pub classes: Vec<String>,
    /// Filter by tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Only AI-opted entities
    #[serde(default)]
    pub ai_only: bool,
}

/// Unsubscribe request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    /// Space ID to unsubscribe from
    pub space_id: String,
}

/// Entity change message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityChangeMessage {
    /// Change type
    pub change_type: ChangeType,
    /// Space ID
    pub space_id: String,
    /// Entity data
    pub entity: EntityData,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Change source
    pub source: ChangeSource,
}

/// Types of entity changes
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Created,
    Updated,
    Deleted,
    Moved,
    Reparented,
}

/// Change source information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSource {
    /// Source type
    pub source_type: SourceType,
    /// Source ID
    pub id: String,
    /// Source name
    pub name: String,
}

/// Source types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    User,
    AiModel,
    System,
    Script,
}

/// Error message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    /// Error code
    pub code: String,
    /// Error message
    pub message: String,
}

// ============================================================================
// EEP Export Format
// ============================================================================

/// Eustress Export Protocol (EEP) export record
/// This is the standard format for exporting entity data to AI training pipelines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EepExportRecord {
    /// Protocol version
    pub protocol_version: String,
    /// Export ID
    pub export_id: String,
    /// Export timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Space information
    pub space: EepSpaceInfo,
    /// Entity data
    pub entity: EepEntityData,
    /// Hierarchy path (root to entity)
    pub hierarchy: Vec<EepHierarchyNode>,
    /// Creator information
    pub creator: ChangeSource,
    /// Consent verification
    pub consent: EepConsent,
}

/// Space info for EEP export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EepSpaceInfo {
    /// Space ID
    pub id: String,
    /// Space name
    pub name: String,
    /// Space settings
    pub settings: serde_json::Value,
}

/// Entity data for EEP export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EepEntityData {
    /// Entity ID
    pub id: String,
    /// Entity name
    pub name: String,
    /// Entity class
    pub class: String,
    /// Transform
    pub transform: TransformData,
    /// Properties
    pub properties: PropertyMap,
    /// Tags
    pub tags: Vec<String>,
    /// Attributes
    pub attributes: HashMap<String, serde_json::Value>,
    /// Instance parameters
    pub parameters: HashMap<String, HashMap<String, serde_json::Value>>,
    /// Child count
    pub child_count: u32,
}

/// Hierarchy node for EEP export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EepHierarchyNode {
    /// Entity ID
    pub id: String,
    /// Entity name
    pub name: String,
    /// Entity class
    pub class: String,
    /// Depth in hierarchy (0 = root)
    pub depth: u32,
}

/// Consent verification for EEP export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EepConsent {
    /// AI training consent flag
    pub ai_training: bool,
    /// Consent timestamp
    pub consented_at: chrono::DateTime<chrono::Utc>,
    /// Consent source (user who enabled AI flag)
    pub consented_by: String,
}
