//! # Parameters Module
//!
//! Eustress Parameters Architecture: Global → Domain → Instance + External Sync
//!
//! The universal data fabric that ties everything together. Eustress Parameters are
//! the **single source of truth** for all data flow — internal (properties, tags,
//! attributes) and external (Postgres, Firebase, JSON, CSV, etc.).
//!
//! ## 3-Tier Hierarchy
//!
//! | Level    | Scope       | Purpose                                                    |
//! |----------|-------------|------------------------------------------------------------|
//! | Global   | System-wide | Data types and connection templates (auth, schema, format) |
//! | Domain   | Logical group | Key-value schema per use case (AI training, analytics)   |
//! | Instance | Per-entity  | Specific value + pattern applied to entity in a domain    |
//!
//! ## Data Flow
//!
//! ```text
//! EustressEngine (3D Scene)
//!         ↓
//! Entity with Instance Parameter (AI enabled = true)
//!         ↓
//! Parameter Router Module
//!         ↓
//! Exports to External Data Source (via Global config)
//!   → Postgres table
//!   → Firebase collection
//!   → JSON/CSV file
//!   → AI Model MCP Server
//!         ↓
//! Eustress Forge Server calls API through MCP
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for the 3-tier parameters system
pub struct ParametersPlugin;

impl Plugin for ParametersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GlobalParameters>()
            .init_resource::<DomainRegistry>()
            .init_resource::<ParameterRouter>()
            .add_message::<ParameterChangedEvent>()
            .add_message::<ExportRequestEvent>();
    }
}

// ============================================================================
// 1. Global Parameters (System-wide)
// ============================================================================

/// Global parameter definitions - connection templates, auth configs, schemas
/// Stored on Eustress Forge Server (central authority)
#[derive(Resource, Default, Clone, Debug, Serialize, Deserialize)]
pub struct GlobalParameters {
    /// Data source connection configurations
    pub sources: HashMap<String, DataSourceConfig>,
    /// Export target configurations (where data flows to)
    pub export_targets: HashMap<String, ExportTargetConfig>,
    /// MCP server configurations for AI model integration
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// Configuration for an MCP (Model Control Protocol) server
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique identifier for this MCP server
    pub id: String,
    /// Display name
    pub name: String,
    /// Server endpoint URL
    pub endpoint: String,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Protocol version (e.g., "eep_v1")
    pub protocol_version: String,
    /// Supported capabilities
    pub capabilities: McpCapabilities,
    /// Whether this server is enabled
    pub enabled: bool,
}

/// MCP server capabilities
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct McpCapabilities {
    /// Can receive entity CRUD operations
    pub entity_crud: bool,
    /// Can receive spatial data exports
    pub spatial_export: bool,
    /// Can receive training data (AI opt-in entities)
    pub training_data: bool,
    /// Can execute Rune scripts
    pub rune_execution: bool,
    /// Supports real-time streaming
    pub realtime_streaming: bool,
    /// Supports batch export
    pub batch_export: bool,
}

/// Export target configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportTargetConfig {
    /// Unique identifier
    pub id: String,
    /// Target type
    pub target_type: ExportTargetType,
    /// Connection string or endpoint
    pub connection: String,
    /// Authentication configuration
    pub auth: AuthConfig,
    /// Schema/table/collection name
    pub schema: String,
    /// Field mappings
    pub mappings: Vec<FieldMapping>,
    /// Whether this target is enabled
    pub enabled: bool,
}

/// Export target types
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExportTargetType {
    /// PostgreSQL database
    Postgres,
    /// Firebase Firestore/Realtime DB
    Firebase,
    /// JSON file export
    JsonFile,
    /// CSV file export
    CsvFile,
    /// MCP server (AI model endpoint)
    McpServer,
    /// Custom webhook
    Webhook,
    /// S3/Cloud storage
    CloudStorage,
}

/// Authentication configuration
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    /// Auth type
    pub auth_type: AuthType,
    /// Credentials (encrypted at rest)
    pub credentials: HashMap<String, String>,
}

// ============================================================================
// 2. Domain Parameters (Logical Groups)
// ============================================================================

/// Registry of all domain parameter schemas
#[derive(Resource, Default, Clone, Debug, Serialize, Deserialize)]
pub struct DomainRegistry {
    /// All registered domains
    pub domains: HashMap<String, DomainSchema>,
}

/// Domain schema definition - key-value schema per use case
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainSchema {
    /// Domain identifier (e.g., "ai_training", "user_preferences", "spatial_metrics")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of this domain's purpose
    pub description: String,
    /// Key definitions with types and validation
    pub keys: HashMap<String, DomainKeyDef>,
    /// Export targets this domain routes to
    pub export_targets: Vec<String>,
    /// Whether entities in this domain require AI opt-in
    pub requires_ai_consent: bool,
    /// Version for schema evolution
    pub version: u32,
}

/// Definition of a key within a domain
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainKeyDef {
    /// Key name
    pub name: String,
    /// Value type
    pub value_type: ParameterValueType,
    /// Default value (serialized)
    pub default: Option<String>,
    /// Whether this key is required
    pub required: bool,
    /// Validation rules
    pub validation: Vec<ValidationRule>,
    /// Description
    pub description: String,
}

/// Parameter value types
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParameterValueType {
    Bool,
    Int,
    Float,
    String,
    Vector3,
    Color,
    EntityRef,
    Json,
    Binary,
}

// ============================================================================
// 3. Instance Parameters (Per-Entity)
// ============================================================================

/// Instance parameters attached to a specific entity
#[derive(Component, Clone, Debug, Default, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct InstanceParameters {
    /// Domain → key → value mappings for this entity
    pub domains: HashMap<String, HashMap<String, ParameterValue>>,
}

impl InstanceParameters {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a parameter value in a domain
    pub fn set(&mut self, domain: &str, key: &str, value: ParameterValue) {
        self.domains
            .entry(domain.to_string())
            .or_default()
            .insert(key.to_string(), value);
    }

    /// Get a parameter value from a domain
    pub fn get(&self, domain: &str, key: &str) -> Option<&ParameterValue> {
        self.domains.get(domain)?.get(key)
    }

    /// Check if entity is opted into a domain
    pub fn has_domain(&self, domain: &str) -> bool {
        self.domains.contains_key(domain)
    }

    /// Get all domains this entity participates in
    pub fn active_domains(&self) -> impl Iterator<Item = &String> {
        self.domains.keys()
    }

    /// Check if AI training is enabled (convenience method)
    pub fn ai_enabled(&self) -> bool {
        self.get("ai_training", "enabled")
            .map(|v| matches!(v, ParameterValue::Bool(true)))
            .unwrap_or(false)
    }

    /// Enable AI training for this entity
    pub fn enable_ai(&mut self) {
        self.set("ai_training", "enabled", ParameterValue::Bool(true));
    }

    /// Disable AI training for this entity
    pub fn disable_ai(&mut self) {
        self.set("ai_training", "enabled", ParameterValue::Bool(false));
    }
}

/// Parameter value (runtime representation)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Reflect)]
pub enum ParameterValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Vector3([f32; 3]),
    Color([f32; 4]),
    EntityRef(Option<u64>),
    Json(String),
    Binary(Vec<u8>),
}

impl Default for ParameterValue {
    fn default() -> Self {
        Self::Bool(false)
    }
}

// ============================================================================
// Parameter Router (Change Detection & Export)
// ============================================================================

/// Routes parameter changes to appropriate export targets
#[derive(Resource, Default, Clone, Debug)]
pub struct ParameterRouter {
    /// Pending exports (batched for efficiency)
    pub pending_exports: Vec<ExportRecord>,
    /// Export statistics
    pub stats: RouterStats,
}

/// Statistics for the parameter router
#[derive(Clone, Debug, Default)]
pub struct RouterStats {
    pub total_exports: u64,
    pub successful_exports: u64,
    pub failed_exports: u64,
    pub last_export_time: Option<std::time::Instant>,
}

/// Record of data to export
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportRecord {
    /// Unique export ID
    pub id: Uuid,
    /// Entity ID
    pub entity_id: u64,
    /// Entity name
    pub entity_name: String,
    /// Domain this export belongs to
    pub domain: String,
    /// Changed parameters
    pub parameters: HashMap<String, ParameterValue>,
    /// Entity hierarchy path
    pub hierarchy_path: Vec<String>,
    /// Entity transform
    pub transform: ExportTransform,
    /// Entity class type
    pub class_type: String,
    /// Timestamp (milliseconds since epoch)
    pub timestamp_ms: i64,
    /// Space/scene ID
    pub space_id: String,
    /// Creator info (user or AI model)
    pub creator: CreatorInfo,
}

/// Transform data for export
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExportTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// Information about who created/modified the entity
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatorInfo {
    /// Creator type
    pub creator_type: CreatorType,
    /// Creator ID (user ID or model ID)
    pub id: String,
    /// Display name
    pub name: String,
}

impl Default for CreatorInfo {
    fn default() -> Self {
        Self {
            creator_type: CreatorType::User,
            id: "unknown".to_string(),
            name: "Unknown".to_string(),
        }
    }
}

/// Type of entity creator
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CreatorType {
    User,
    AiModel,
    System,
}

// ============================================================================
// Events
// ============================================================================

/// Event fired when a parameter changes
#[derive(Message, Clone, Debug)]
pub struct ParameterChangedEvent {
    pub entity: bevy::prelude::Entity,
    pub domain: String,
    pub key: String,
    pub old_value: Option<ParameterValue>,
    pub new_value: ParameterValue,
}

/// Event requesting an export
#[derive(Message, Clone, Debug)]
pub struct ExportRequestEvent {
    pub entity: bevy::prelude::Entity,
    pub domain: String,
    pub target_ids: Vec<String>,
}

// ============================================================================
// Legacy Compatibility (Original Parameters struct)
// ============================================================================

/// Parameters resource for data source configuration (legacy)
#[derive(Resource, Default, Clone, Debug, Serialize, Deserialize)]
pub struct Parameters {
    pub sources: HashMap<String, DataSourceConfig>,
}

/// Data source configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceConfig {
    pub source_type: DataSourceType,
    pub auth: AuthType,
    pub anonymization: AnonymizationMode,
    pub update_mode: UpdateMode,
    pub mappings: Vec<DataMapping>,
}

/// Type of data source
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum DataSourceType {
    #[default]
    None,
    Api,
    Database,
    File,
    Stream,
}

/// Authentication type
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum AuthType {
    #[default]
    None,
    ApiKey,
    OAuth,
    Basic,
    Token,
}

/// Data anonymization mode
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum AnonymizationMode {
    #[default]
    None,
    Hash,
    Mask,
    Redact,
}

/// Update mode for data
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum UpdateMode {
    #[default]
    Manual,
    Polling,
    Realtime,
    OnDemand,
}

/// Data mapping configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataMapping {
    pub source_field: String,
    pub target_field: String,
    pub field_mappings: Vec<FieldMapping>,
}

/// Field mapping
#[derive(Clone, Debug, Serialize, Deserialize, Reflect)]
pub struct FieldMapping {
    pub from: String,
    pub to: String,
    pub transform: Option<String>,
}

/// Validation rule
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationRule {
    pub field: String,
    pub rule_type: String,
    pub value: String,
}

/// Collection of validation rules
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ValidationRules {
    pub rules: Vec<ValidationRule>,
}
