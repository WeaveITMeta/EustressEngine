//! MCP Server types for entity data and operations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Entity data representation for MCP operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    /// Entity ID (unique within space)
    pub id: String,
    /// Entity name
    pub name: String,
    /// Entity class type (Part, Model, Humanoid, etc.)
    pub class: String,
    /// Parent entity ID (if any)
    pub parent: Option<String>,
    /// Child entity IDs
    #[serde(default)]
    pub children: Vec<String>,
    /// Transform data
    pub transform: TransformData,
    /// Properties (class-specific)
    #[serde(default)]
    pub properties: PropertyMap,
    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Attributes (custom key-value data)
    #[serde(default)]
    pub attributes: HashMap<String, AttributeValue>,
    /// AI training opt-in flag
    #[serde(default)]
    pub ai: bool,
    /// Network ownership rule
    #[serde(default)]
    pub network_ownership: NetworkOwnership,
    /// Instance parameters (domain → key → value)
    #[serde(default)]
    pub parameters: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl Default for EntityData {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: "Entity".to_string(),
            class: "Part".to_string(),
            parent: None,
            children: Vec::new(),
            transform: TransformData::default(),
            properties: PropertyMap::default(),
            tags: Vec::new(),
            attributes: HashMap::new(),
            ai: false,
            network_ownership: NetworkOwnership::ServerOnly,
            parameters: HashMap::new(),
        }
    }
}

/// Transform data for entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformData {
    /// Position [x, y, z]
    pub position: [f32; 3],
    /// Rotation as quaternion [x, y, z, w]
    pub rotation: [f32; 4],
    /// Scale [x, y, z]
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

/// Property map for class-specific properties
pub type PropertyMap = HashMap<String, serde_json::Value>;

/// Attribute value types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    String(String),
    Number(f64),
    Int(i64),
    Bool(bool),
    Vector3([f32; 3]),
    Color([f32; 4]),
    Object(Option<String>),
}

/// Network ownership rules
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkOwnership {
    #[default]
    ServerOnly,
    ClientClaimable,
    SpawnOwner,
    Inherit,
    LocalOnly,
}

/// Space information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceData {
    /// Space ID
    pub id: String,
    /// Space name
    pub name: String,
    /// Space description
    pub description: String,
    /// Entity count
    pub entity_count: u64,
    /// Active player count
    pub player_count: u32,
    /// Space settings
    pub settings: SpaceSettings,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last modified timestamp
    pub modified_at: chrono::DateTime<chrono::Utc>,
}

/// Space settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpaceSettings {
    /// Gravity vector
    pub gravity: [f32; 3],
    /// Max entity speed
    pub max_entity_speed: f32,
    /// World bounds (min, max)
    pub world_bounds: Option<([f32; 3], [f32; 3])>,
    /// Whether AI entities are allowed
    pub ai_enabled: bool,
}

/// Entity change event for streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityChangeEvent {
    /// Event type
    pub event_type: ChangeEventType,
    /// Entity data
    pub entity: EntityData,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Change source
    pub source: ChangeSource,
}

/// Types of entity change events
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeEventType {
    Created,
    Updated,
    Deleted,
    Moved,
    Reparented,
}

/// Source of a change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSource {
    /// Source type
    pub source_type: SourceType,
    /// Source ID (user ID, model ID, etc.)
    pub id: String,
    /// Source name
    pub name: String,
}

/// Types of change sources
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    User,
    AiModel,
    System,
    Script,
}

/// Batch operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    /// Total operations
    pub total: usize,
    /// Successful operations
    pub succeeded: usize,
    /// Failed operations
    pub failed: usize,
    /// Individual results
    pub results: Vec<OperationResult>,
}

/// Individual operation result
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
