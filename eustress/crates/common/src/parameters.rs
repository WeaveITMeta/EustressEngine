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

        // When streaming is enabled, bridge Bevy parameter events to
        // EustressStream topics so the MCP server, export targets, and
        // external processes can observe parameter changes.
        #[cfg(feature = "streaming")]
        {
            app.add_observer(bridge_parameter_changed_to_stream)
               .add_observer(bridge_export_requests_to_stream);
        }
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

impl DomainSchema {
    /// Build a schema from a `Domain` class instance's `_instance.toml`.
    ///
    /// Domains are class objects, not registry entries, so the file on disk is
    /// the source of truth and [`DomainRegistry`] is only an index over the
    /// loaded instances — the same relationship `MaterialRegistry` has with
    /// `.mat.toml` files. Keeping the store on the instance is what lets a
    /// domain fork with a copy-on-write branch instead of leaking across every
    /// branch as global mutable state.
    ///
    /// `name` is the instance name, which is the domain's identity; the file
    /// never repeats it.
    pub fn from_instance_toml(name: &str, source: &str) -> Result<Self, String> {
        let doc: toml::Value = source
            .parse()
            .map_err(|e| format!("domain '{name}': {e}"))?;

        let attrs = doc.get("attributes");
        let s = |k: &str| -> Option<&str> { attrs?.get(k)?.as_str() };

        let export_targets = s("export_targets")
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect();

        let mut keys = HashMap::new();
        if let Some(table) = doc.get("keys").and_then(|k| k.as_table()) {
            for (key_name, def) in table {
                keys.insert(
                    key_name.clone(),
                    DomainKeyDef::from_toml(key_name, def)
                        .map_err(|e| format!("domain '{name}', key '{key_name}': {e}"))?,
                );
            }
        }

        Ok(Self {
            id: name.to_string(),
            name: name.to_string(),
            description: s("description").unwrap_or("").to_string(),
            keys,
            export_targets,
            requires_ai_consent: attrs
                .and_then(|a| a.get("requires_ai_consent"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            version: attrs
                .and_then(|a| a.get("version"))
                .and_then(|v| v.as_integer())
                .unwrap_or(1) as u32,
        })
    }

    /// Whether a value satisfies this domain's contract for `key`.
    ///
    /// An unknown key is allowed: a domain describes the keys it governs, not
    /// an exhaustive whitelist, so an instance may carry extra ones. A key that
    /// IS governed must match its declared type.
    pub fn validate(&self, key: &str, value: &ParameterValue) -> Result<(), String> {
        let Some(def) = self.keys.get(key) else { return Ok(()) };
        let expected = &def.value_type;
        let actual = value.type_name();
        let matches = matches!(
            (expected, value),
            (ParameterValueType::Bool, ParameterValue::Bool(_))
                | (ParameterValueType::Int, ParameterValue::Int(_))
                | (ParameterValueType::Float, ParameterValue::Float(_))
                | (ParameterValueType::String, ParameterValue::String(_))
                | (ParameterValueType::Vector3, ParameterValue::Vector3(_))
                | (ParameterValueType::Color, ParameterValue::Color(_))
                | (ParameterValueType::EntityRef, ParameterValue::EntityRef(_))
                | (ParameterValueType::Json, ParameterValue::Json(_))
                | (ParameterValueType::Binary, ParameterValue::Binary(_))
        );
        if !matches {
            return Err(format!("expected {expected:?}, got {actual}"));
        }
        Ok(())
    }

    /// Keys this domain requires that the given parameters do not define.
    pub fn missing_required(&self, values: &HashMap<String, ParameterValue>) -> Vec<&str> {
        self.keys
            .values()
            .filter(|d| d.required && !values.contains_key(&d.name))
            .map(|d| d.name.as_str())
            .collect()
    }
}

impl DomainRegistry {
    /// Rebuild the index from the loaded `Domain` instances.
    ///
    /// Called with `(instance name, file source)` for every Domain in the
    /// Space. Replaces the contents wholesale, so deleting a Domain instance
    /// removes it from the index rather than leaving a ghost.
    pub fn reindex<'a>(&mut self, domains: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
        self.domains.clear();
        let mut errors = Vec::new();
        for (name, source) in domains {
            match DomainSchema::from_instance_toml(name, source) {
                Ok(schema) => {
                    self.domains.insert(name.to_string(), schema);
                }
                // A malformed domain is reported, never silently skipped — an
                // unparsed contract would let every value in it through
                // unvalidated.
                Err(e) => errors.push(e),
            }
        }
        errors
    }
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

impl DomainKeyDef {
    /// Parse one `[keys.<name>]` entry from a Domain instance's TOML.
    ///
    /// `min` / `max` become [`ValidationRule`]s rather than dedicated fields,
    /// so a domain can grow new rule kinds without changing this struct.
    pub fn from_toml(name: &str, def: &toml::Value) -> Result<Self, String> {
        let type_name = def
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("missing `type`")?;
        let value_type = ParameterValueType::parse(type_name)
            .ok_or_else(|| format!("unknown type '{type_name}'"))?;

        let mut validation = Vec::new();
        for rule in ["min", "max"] {
            if let Some(v) = def.get(rule) {
                let as_text = match v {
                    toml::Value::Integer(i) => i.to_string(),
                    toml::Value::Float(f) => f.to_string(),
                    toml::Value::String(s) => s.clone(),
                    other => return Err(format!("`{rule}` must be a number, got {other}")),
                };
                validation.push(ValidationRule {
                    field: name.to_string(),
                    rule_type: rule.to_string(),
                    value: as_text,
                });
            }
        }

        Ok(Self {
            name: name.to_string(),
            value_type,
            default: def.get("default").and_then(|v| v.as_str()).map(str::to_string),
            required: def.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
            validation,
            description: def
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }
}

impl ParameterValueType {
    /// Parse the type name a Domain's TOML uses. Matches
    /// [`ParameterValue::type_name`], so a declared type and an authored value
    /// are named the same thing everywhere.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Bool" => Self::Bool,
            "Int" => Self::Int,
            "Float" => Self::Float,
            "String" => Self::String,
            "Vector3" => Self::Vector3,
            "Color" => Self::Color,
            "EntityRef" => Self::EntityRef,
            "Json" => Self::Json,
            "Binary" => Self::Binary,
            _ => return None,
        })
    }
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
    /// Domain → key → binding, for the keys wired to the outside world.
    ///
    /// A SIBLING map rather than a field on the value, so an unbound parameter
    /// costs exactly what it did before and older serialized data still loads.
    /// Bindings are per-key state, not things in their own right — which is why
    /// they live here instead of becoming class objects. One instance can carry
    /// hundreds; making each an Explorer node is the mistake Roblox made with
    /// ValueObjects and corrected with Attributes.
    #[serde(default)]
    #[reflect(ignore)]
    pub bindings: HashMap<String, HashMap<String, ParameterBinding>>,
}

/// How one parameter key reaches the outside world.
///
/// Names a [`ClassName::Connector`](crate::classes::ClassName::Connector) and a
/// field within it for the inbound direction; outbound routing is NOT named
/// here — it belongs to the domain, so `publish` only decides whether this key
/// participates in the targets its domain already declares.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ParameterBinding {
    /// Connector instance name. `None` means the value is authored locally.
    pub connector: Option<String>,
    /// Field / column within that source (e.g. `zone_3.temp_c`).
    pub field: Option<String>,
    /// Seconds between reads. `None` = manual only, never a background poll.
    pub refresh_seconds: Option<u64>,
    /// Whether this key participates in its domain's export targets.
    pub publish: bool,
    /// Simulation time of the last successful read, for the row's age display.
    pub last_read_s: Option<f64>,
    /// Why the last attempt failed. A binding that cannot reach its source must
    /// say so rather than let the previous value pass as current.
    pub last_error: Option<String>,
}

/// What a bound key is doing, derived rather than stored so the two can never
/// disagree. Backs the Properties row's appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingState {
    /// No binding: the instance owns the value, and the row reads like an
    /// Attribute row.
    Local,
    /// A source owns the value; the field is read-only.
    Sourced,
    /// The instance owns the value and every change exports.
    Published,
    /// Read in, forward out.
    Relay,
    /// Bound, but the last attempt failed.
    Faulted,
}

impl ParameterBinding {
    /// Whether a source feeds this key.
    pub fn is_sourced(&self) -> bool {
        self.connector.as_deref().is_some_and(|c| !c.is_empty())
    }

    /// Current state. A fault outranks everything: a binding that errored is
    /// reported as faulted even though it is still configured, because the
    /// alternative is showing a stale number as if it were live.
    pub fn state(&self) -> BindingState {
        if self.last_error.is_some() {
            return BindingState::Faulted;
        }
        match (self.is_sourced(), self.publish) {
            (true, true) => BindingState::Relay,
            (true, false) => BindingState::Sourced,
            (false, true) => BindingState::Published,
            (false, false) => BindingState::Local,
        }
    }

    /// A sourced key is owned by its source, so the panel must refuse edits
    /// rather than accept one the next poll would silently discard.
    pub fn value_is_read_only(&self) -> bool {
        self.is_sourced()
    }
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

    /// Read the binding for one key, if it has one.
    pub fn binding(&self, domain: &str, key: &str) -> Option<&ParameterBinding> {
        self.bindings.get(domain)?.get(key)
    }

    /// Attach or replace a key's binding.
    pub fn bind(&mut self, domain: &str, key: &str, binding: ParameterBinding) {
        self.bindings
            .entry(domain.to_string())
            .or_default()
            .insert(key.to_string(), binding);
    }

    /// Drop a key's binding, returning the instance to owning the value. The
    /// VALUE is deliberately left in place: unbinding should hand back the last
    /// known number, not blank the field.
    pub fn unbind(&mut self, domain: &str, key: &str) -> Option<ParameterBinding> {
        let removed = self.bindings.get_mut(domain)?.remove(key);
        if self.bindings.get(domain).is_some_and(|m| m.is_empty()) {
            self.bindings.remove(domain);
        }
        removed
    }

    /// State of one key. An unbound key is [`BindingState::Local`], which is
    /// what makes a parameter usable before any source or domain exists.
    pub fn binding_state(&self, domain: &str, key: &str) -> BindingState {
        self.binding(domain, key)
            .map(ParameterBinding::state)
            .unwrap_or(BindingState::Local)
    }

    /// Record a successful read from a bound source.
    pub fn record_read(&mut self, domain: &str, key: &str, value: ParameterValue, at_s: f64) {
        self.set(domain, key, value);
        if let Some(b) = self.bindings.get_mut(domain).and_then(|m| m.get_mut(key)) {
            b.last_read_s = Some(at_s);
            b.last_error = None;
        }
    }

    /// Record a failed read. The previous VALUE is left untouched so a caller
    /// can still show it, but the binding is now faulted and the panel is
    /// obliged to say so rather than present it as current.
    pub fn record_error(&mut self, domain: &str, key: &str, error: impl Into<String>) {
        if let Some(b) = self.bindings.get_mut(domain).and_then(|m| m.get_mut(key)) {
            b.last_error = Some(error.into());
        }
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

impl ParameterValue {
    /// Type badge shown in the Properties panel — mirrors
    /// [`crate::attributes::AttributeValue::type_name`] so a Parameter row and
    /// an Attribute row read the same way.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "Bool",
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::String(_) => "String",
            Self::Vector3(_) => "Vector3",
            Self::Color(_) => "Color",
            Self::EntityRef(_) => "EntityRef",
            Self::Json(_) => "Json",
            Self::Binary(_) => "Binary",
        }
    }

    /// Round-trippable text for the editable Properties row. Binary is opaque,
    /// so it reports size rather than pretending to be editable text.
    pub fn edit_string(&self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => format!("{f}"),
            Self::String(s) => s.clone(),
            Self::Vector3(v) => format!("{}, {}, {}", v[0], v[1], v[2]),
            Self::Color(c) => format!("{}, {}, {}, {}", c[0], c[1], c[2], c[3]),
            Self::EntityRef(e) => e.map(|v| v.to_string()).unwrap_or_default(),
            Self::Json(j) => j.clone(),
            Self::Binary(b) => format!("<{} bytes>", b.len()),
        }
    }

    /// Parse a value back from the type badge + edited text. Returns `None`
    /// when the text does not fit the type, so a bad edit is rejected rather
    /// than silently coerced.
    pub fn parse(type_name: &str, text: &str) -> Option<Self> {
        let t = text.trim();
        let nums = |n: usize| -> Option<Vec<f32>> {
            let parts: Vec<f32> = t
                .split(',')
                .filter_map(|p| p.trim().parse::<f32>().ok())
                .collect();
            (parts.len() == n).then_some(parts)
        };
        Some(match type_name {
            "Bool" => Self::Bool(matches!(t, "true" | "True" | "1")),
            "Int" => Self::Int(t.parse().ok()?),
            "Float" => Self::Float(t.parse().ok()?),
            "String" => Self::String(t.to_string()),
            "Vector3" => {
                let v = nums(3)?;
                Self::Vector3([v[0], v[1], v[2]])
            }
            "Color" => {
                let v = nums(4)?;
                Self::Color([v[0], v[1], v[2], v[3]])
            }
            "EntityRef" => Self::EntityRef(if t.is_empty() { None } else { Some(t.parse().ok()?) }),
            "Json" => Self::Json(t.to_string()),
            // Binary is not editable as text — refuse rather than corrupt it.
            _ => return None,
        })
    }

    /// The type names offered in the add/edit dialog, in menu order. `Binary`
    /// is deliberately excluded: it has no text representation to author.
    pub const EDITABLE_TYPES: [&'static str; 8] = [
        "String", "Float", "Int", "Bool", "Vector3", "Color", "EntityRef", "Json",
    ];
}

/// The domain a Parameter lands in when the author has not chosen one.
///
/// Parameters are domain-scoped by design (domain → key → value), but the
/// Properties panel must stay usable before any Domain has been defined — so an
/// un-domained Parameter is basic-by-default and lives here until it is
/// promoted into a real domain.
pub const DEFAULT_PARAMETER_DOMAIN: &str = "instance";

// ============================================================================
// Parameter Router (Change Detection & Export via EustressStream)
// ============================================================================

/// Well-known EustressStream topic names for parameter events.
///
/// Subscribe to these on any `EustressStream` clone to observe parameter
/// changes and export requests. Payloads are JSON-serialized.
pub mod parameter_topics {
    /// Parameter change notifications (payload: JSON `ExportRecord`).
    /// Published by `ParameterRouter::publish_export()`.
    pub const PARAMETER_EXPORTS: &str = "parameter.exports";
    /// Parameter changed events (payload: JSON `ParameterChangedSerialized`).
    /// Published by the `bridge_parameter_events` system.
    pub const PARAMETER_CHANGED: &str = "parameter.changed";
    /// Export request events (payload: JSON `ExportRequestSerialized`).
    /// Published by the `bridge_export_requests` system.
    pub const EXPORT_REQUESTS: &str = "parameter.export_requests";
}

/// Routes parameter changes to the `"parameter.exports"` EustressStream topic.
///
/// Instead of buffering into a `Vec`, this publishes each export record
/// directly to the stream. Any number of subscribers (MCP router, file
/// exporters, training pipelines) can observe records with <1 µs latency.
///
/// When `streaming` feature is disabled, falls back to a `Vec` buffer.
#[derive(Resource, Clone, Debug)]
pub struct ParameterRouter {
    /// Export statistics
    pub stats: RouterStats,
    /// Fallback buffer for non-streaming builds
    pub pending_exports: Vec<ExportRecord>,
}

impl Default for ParameterRouter {
    fn default() -> Self {
        Self {
            stats: RouterStats::default(),
            pending_exports: Vec::new(),
        }
    }
}

impl ParameterRouter {
    /// Publish an export record.
    ///
    /// With the `streaming` feature: publishes to the `"parameter.exports"`
    /// topic on the provided `EustressStream`. Without streaming: appends to
    /// the internal `pending_exports` vec for manual draining.
    #[cfg(feature = "streaming")]
    pub fn publish_export(
        &mut self,
        record: &ExportRecord,
        stream: &eustress_stream::EustressStream,
    ) {
        match serde_json::to_vec(record) {
            Ok(json_bytes) => {
                stream.producer(parameter_topics::PARAMETER_EXPORTS)
                    .send_bytes(bytes::Bytes::from(json_bytes));
                self.stats.total_exports += 1;
                self.stats.successful_exports += 1;
                self.stats.last_export_time = Some(std::time::Instant::now());
            }
            Err(e) => {
                tracing::error!("ParameterRouter: failed to serialize ExportRecord: {e}");
                self.stats.total_exports += 1;
                self.stats.failed_exports += 1;
            }
        }
    }

    /// Fallback: buffer an export record when streaming is unavailable.
    pub fn buffer_export(&mut self, record: ExportRecord) {
        self.pending_exports.push(record);
        self.stats.total_exports += 1;
        self.stats.successful_exports += 1;
        self.stats.last_export_time = Some(std::time::Instant::now());
    }

    /// Drain all buffered exports (for non-streaming builds or manual flush).
    pub fn drain_pending(&mut self) -> Vec<ExportRecord> {
        std::mem::take(&mut self.pending_exports)
    }
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
#[derive(Event, Message, Clone, Debug)]
pub struct ParameterChangedEvent {
    pub entity: bevy::prelude::Entity,
    pub domain: String,
    pub key: String,
    pub old_value: Option<ParameterValue>,
    pub new_value: ParameterValue,
}

/// Event requesting an export
#[derive(Event, Message, Clone, Debug)]
pub struct ExportRequestEvent {
    pub entity: bevy::prelude::Entity,
    pub domain: String,
    pub target_ids: Vec<String>,
}

// ============================================================================
// Stream Bridge — Bevy Messages → EustressStream topics
// ============================================================================

/// Serializable form of `ParameterChangedEvent` for stream publishing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterChangedSerialized {
    pub entity_bits: u64,
    pub domain: String,
    pub key: String,
    pub old_value: Option<ParameterValue>,
    pub new_value: ParameterValue,
}

/// Serializable form of `ExportRequestEvent` for stream publishing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportRequestSerialized {
    pub entity_bits: u64,
    pub domain: String,
    pub target_ids: Vec<String>,
}

/// Bevy system: bridges `ParameterChangedEvent` messages to the
/// `"parameter.changed"` EustressStream topic.
///
/// Only compiled when the `streaming` feature is enabled.
#[cfg(feature = "streaming")]
pub fn bridge_parameter_changed_to_stream(
    trigger: On<ParameterChangedEvent>,
    queue: Option<Res<crate::change_queue::ChangeQueue>>,
) {
    let Some(queue) = queue else { return; };
    let event = trigger.event();
    let serialized = ParameterChangedSerialized {
        entity_bits: event.entity.to_bits(),
        domain: event.domain.clone(),
        key: event.key.clone(),
        old_value: event.old_value.clone(),
        new_value: event.new_value.clone(),
    };
    if let Ok(json_bytes) = serde_json::to_vec(&serialized) {
        queue.stream.producer(parameter_topics::PARAMETER_CHANGED)
            .send_bytes(bytes::Bytes::from(json_bytes));
    }
}

/// Bevy system: bridges `ExportRequestEvent` messages to the
/// `"parameter.export_requests"` EustressStream topic.
///
/// Only compiled when the `streaming` feature is enabled.
#[cfg(feature = "streaming")]
pub fn bridge_export_requests_to_stream(
    trigger: On<ExportRequestEvent>,
    queue: Option<Res<crate::change_queue::ChangeQueue>>,
) {
    let Some(queue) = queue else { return; };
    let event = trigger.event();
    let serialized = ExportRequestSerialized {
        entity_bits: event.entity.to_bits(),
        domain: event.domain.clone(),
        target_ids: event.target_ids.clone(),
    };
    if let Ok(json_bytes) = serde_json::to_vec(&serialized) {
        queue.stream.producer(parameter_topics::EXPORT_REQUESTS)
            .send_bytes(bytes::Bytes::from(json_bytes));
    }
}

// ============================================================================
// Legacy Compatibility (Original Parameters struct)
// ============================================================================

/// Parameters component for entity-level data source configuration
// 0.19: Resource is now a subtrait of Component — a type can no longer derive
// both. Parameters is only ever used as a Component (the resource is the
// distinct GlobalParameters), so drop Resource.
#[derive(Component, Default, Clone, Debug, Serialize, Deserialize)]
pub struct Parameters {
    pub sources: HashMap<String, DataSourceConfig>,
    pub domain: String,
    pub global_source_ref: Option<String>,
    pub sync_config: Option<DomainSyncConfig>,
}

/// Domain sync configuration
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DomainSyncConfig {
    pub enabled: bool,
    pub interval_ms: u64,
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
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum DataSourceType {
    #[default]
    None,
    Api,
    Database,
    File,
    Stream,
    Postgres,
    Firebase,
    REST,
    GraphQL,
    WebSocket,
    MQTT,
    CSV,
    JSON,
    FHIR,
    // Additional data source types
    XML,
    Parquet,
    Excel,
    GRPC,
    Kafka,
    AMQP,
    WebTransport,
    SSE,
    PostgreSQL,
    MySQL,
    SQLite,
    MongoDB,
    Redis,
    Snowflake,
    BigQuery,
    S3,
    AzureBlob,
    GCS,
    Supabase,
    Oracle,
    DigitalOcean,
    // IoT/Industrial protocols
    OPCUA,
    Modbus,
    BACnet,
    CoAP,
    LwM2M,
    // Healthcare protocols
    HL7v2,
    HL7v3,
    DICOM,
    CDA,
    OMOP,
    OpenEHR,
    IHE,
    X12,
    NCPDP,
    // Additional protocols
    LDAP,
    SFTP,
    FTP,
    Email,
    RSS,
    Atom,
    SOAP,
}

impl DataSourceType {
    pub fn all_variants() -> &'static [Self] {
        &[
            Self::None, Self::Api, Self::Database, Self::File, Self::Stream,
            Self::Postgres, Self::Firebase, Self::REST, Self::GraphQL, Self::WebSocket,
            Self::MQTT, Self::CSV, Self::JSON, Self::FHIR, Self::XML, Self::Parquet,
            Self::Excel, Self::GRPC, Self::Kafka, Self::AMQP, Self::WebTransport,
            Self::SSE, Self::PostgreSQL, Self::MySQL, Self::SQLite, Self::MongoDB,
            Self::Redis, Self::Snowflake, Self::BigQuery, Self::S3, Self::AzureBlob,
            Self::GCS, Self::Supabase, Self::Oracle, Self::DigitalOcean,
            Self::OPCUA, Self::Modbus, Self::BACnet, Self::CoAP, Self::LwM2M,
            Self::HL7v2, Self::HL7v3, Self::DICOM, Self::CDA, Self::OMOP, Self::OpenEHR, Self::IHE,
            Self::X12, Self::NCPDP, Self::LDAP, Self::SFTP, Self::FTP,
            Self::Email, Self::RSS, Self::Atom, Self::SOAP,
        ]
    }
    
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Api => "API",
            Self::Database => "Database",
            Self::File => "File",
            Self::Stream => "Stream",
            Self::Postgres => "PostgreSQL",
            Self::Firebase => "Firebase",
            Self::REST => "REST API",
            Self::GraphQL => "GraphQL",
            Self::WebSocket => "WebSocket",
            Self::MQTT => "MQTT",
            Self::CSV => "CSV",
            Self::JSON => "JSON",
            Self::FHIR => "FHIR",
            Self::XML => "XML",
            Self::Parquet => "Parquet",
            Self::Excel => "Excel",
            Self::GRPC => "gRPC",
            Self::Kafka => "Kafka",
            Self::AMQP => "AMQP",
            Self::WebTransport => "WebTransport",
            Self::SSE => "Server-Sent Events",
            Self::PostgreSQL => "PostgreSQL",
            Self::MySQL => "MySQL",
            Self::SQLite => "SQLite",
            Self::MongoDB => "MongoDB",
            Self::Redis => "Redis",
            Self::Snowflake => "Snowflake",
            Self::BigQuery => "BigQuery",
            Self::S3 => "Amazon S3",
            Self::AzureBlob => "Azure Blob",
            Self::GCS => "Google Cloud Storage",
            Self::Supabase => "Supabase",
            Self::Oracle => "Oracle",
            Self::DigitalOcean => "DigitalOcean",
            Self::OPCUA => "OPC UA",
            Self::Modbus => "Modbus",
            Self::BACnet => "BACnet",
            Self::CoAP => "CoAP",
            Self::LwM2M => "LwM2M",
            Self::HL7v2 => "HL7 v2",
            Self::HL7v3 => "HL7 v3",
            Self::DICOM => "DICOM",
            Self::CDA => "CDA",
            Self::OMOP => "OMOP",
            Self::OpenEHR => "OpenEHR",
            Self::IHE => "IHE",
            Self::X12 => "X12",
            Self::NCPDP => "NCPDP",
            Self::LDAP => "LDAP",
            Self::SFTP => "SFTP",
            Self::FTP => "FTP",
            Self::Email => "Email",
            Self::RSS => "RSS",
            Self::Atom => "Atom",
            Self::SOAP => "SOAP",
        }
    }
    
    pub fn category(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Api | Self::REST | Self::GraphQL | Self::GRPC | Self::SOAP => "API",
            Self::Database | Self::Postgres | Self::PostgreSQL | Self::MySQL | 
            Self::SQLite | Self::MongoDB | Self::Redis | Self::Snowflake | 
            Self::BigQuery | Self::Oracle | Self::LDAP => "Database",
            Self::File | Self::CSV | Self::JSON | Self::XML | Self::Parquet | Self::Excel | Self::SFTP | Self::FTP => "File",
            Self::Stream | Self::WebSocket | Self::MQTT | Self::Kafka | 
            Self::AMQP | Self::WebTransport | Self::SSE | Self::RSS | Self::Atom => "Stream",
            Self::Firebase | Self::Supabase => "BaaS",
            Self::S3 | Self::AzureBlob | Self::GCS | Self::DigitalOcean => "Cloud Storage",
            Self::FHIR | Self::HL7v2 | Self::HL7v3 | Self::DICOM | Self::CDA | Self::OMOP | Self::OpenEHR | Self::IHE | Self::X12 | Self::NCPDP => "Healthcare",
            Self::OPCUA | Self::Modbus | Self::BACnet | Self::CoAP | Self::LwM2M => "IoT",
            Self::Email => "Communication",
        }
    }
}

/// Authentication type
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum AuthType {
    #[default]
    None,
    ApiKey,
    APIKey,
    OAuth,
    OAuth2,
    Basic,
    Token,
    Bearer,
}

impl AuthType {
    pub fn all_variants() -> &'static [Self] {
        &[Self::None, Self::ApiKey, Self::APIKey, Self::OAuth, Self::OAuth2, Self::Basic, Self::Token, Self::Bearer]
    }
    
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::ApiKey => "API Key",
            Self::APIKey => "API Key",
            Self::OAuth => "OAuth",
            Self::OAuth2 => "OAuth 2.0",
            Self::Basic => "Basic Auth",
            Self::Token => "Token",
            Self::Bearer => "Bearer Token",
        }
    }
}

/// Data anonymization mode
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum AnonymizationMode {
    #[default]
    None,
    Hash,
    Mask,
    Redact,
    Synthetic,
}

impl AnonymizationMode {
    pub fn all_variants() -> &'static [Self] {
        &[Self::None, Self::Hash, Self::Mask, Self::Redact, Self::Synthetic]
    }
    
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Hash => "Hash",
            Self::Mask => "Mask",
            Self::Redact => "Redact",
            Self::Synthetic => "Synthetic",
        }
    }
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

// ============================================================================
// Global Parameters Registry (for serialization)
// ============================================================================

/// Global parameters registry for scene serialization
#[derive(Resource, Clone, Debug, Serialize, Deserialize, Default)]
pub struct GlobalParametersRegistry {
    pub sources: Vec<GlobalDataSource>,
    pub domains: Vec<DomainConfig>,
    pub global_variables: HashMap<String, serde_json::Value>,
}

impl GlobalParametersRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Register a new data source
    pub fn register_source(&mut self, source: GlobalDataSource) {
        self.sources.push(source);
    }
    
    /// Register a new domain
    pub fn register_domain(&mut self, id: String, config: DomainConfig) {
        self.domains.push(config);
    }
    
    /// Set a global variable
    pub fn set_variable(&mut self, key: String, value: serde_json::Value) {
        self.global_variables.insert(key, value);
    }
    
    /// Get a global variable
    pub fn get_variable(&self, key: &str) -> Option<&serde_json::Value> {
        self.global_variables.get(key)
    }
}

/// Global data source configuration
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GlobalDataSource {
    pub id: String,
    pub name: String,
    pub source_type: DataSourceType,
    pub connection_string: String,
}

/// Domain configuration
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DomainConfig {
    pub id: String,
    pub name: String,
    pub schema: HashMap<String, String>,
}

impl DomainConfig {
    /// Create a new domain config
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            schema: HashMap::new(),
        }
    }
}

/// Mapping target type
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum MappingTargetType {
    #[default]
    Property,
    Tag,
    Attribute,
    Color,
    Vector3,
    Number,
    String,
    Boolean,
    // BasePart properties
    Anchored,
    CanCollide,
    CanTouch,
    Locked,
    Visible,
    Transparency,
    Reflectance,
}

impl MappingTargetType {
    pub fn all_variants() -> &'static [Self] {
        &[
            Self::Property, Self::Tag, Self::Attribute, Self::Color, Self::Vector3,
            Self::Number, Self::String, Self::Boolean, Self::Anchored, Self::CanCollide,
            Self::CanTouch, Self::Locked, Self::Visible, Self::Transparency, Self::Reflectance,
        ]
    }
    
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Property => "Property",
            Self::Tag => "Tag",
            Self::Attribute => "Attribute",
            Self::Color => "Color",
            Self::Vector3 => "Vector3",
            Self::Number => "Number",
            Self::String => "String",
            Self::Boolean => "Boolean",
            Self::Anchored => "Anchored",
            Self::CanCollide => "CanCollide",
            Self::CanTouch => "CanTouch",
            Self::Locked => "Locked",
            Self::Visible => "Visible",
            Self::Transparency => "Transparency",
            Self::Reflectance => "Reflectance",
        }
    }
    
    pub fn category(&self) -> &'static str {
        match self {
            Self::Property | Self::Tag | Self::Attribute => "General",
            Self::Color | Self::Vector3 => "Spatial",
            Self::Number | Self::String | Self::Boolean => "Primitive",
            Self::Anchored | Self::CanCollide | Self::CanTouch | 
            Self::Locked | Self::Visible | Self::Transparency | Self::Reflectance => "BasePart",
        }
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;

    fn sourced() -> ParameterBinding {
        ParameterBinding {
            connector: Some("Building Sensors".into()),
            field: Some("zone_3.temp_c".into()),
            refresh_seconds: Some(30),
            ..Default::default()
        }
    }

    #[test]
    fn an_unbound_key_is_local() {
        let p = InstanceParameters::new();
        assert_eq!(p.binding_state("hvac", "setpoint"), BindingState::Local);
    }

    #[test]
    fn direction_determines_state() {
        let mut b = ParameterBinding::default();
        assert_eq!(b.state(), BindingState::Local);
        b.publish = true;
        assert_eq!(b.state(), BindingState::Published);

        let mut s = sourced();
        assert_eq!(s.state(), BindingState::Sourced);
        s.publish = true;
        assert_eq!(s.state(), BindingState::Relay);
    }

    #[test]
    fn a_fault_outranks_every_other_state() {
        let mut s = sourced();
        s.publish = true;
        assert_eq!(s.state(), BindingState::Relay);
        s.last_error = Some("connection refused".into());
        // Still fully configured, but must not read as healthy.
        assert_eq!(s.state(), BindingState::Faulted);
    }

    #[test]
    fn an_empty_connector_name_does_not_count_as_sourced() {
        let b = ParameterBinding { connector: Some(String::new()), ..Default::default() };
        assert!(!b.is_sourced());
        assert_eq!(b.state(), BindingState::Local);
    }

    #[test]
    fn a_sourced_value_is_read_only_but_a_published_one_is_not() {
        assert!(sourced().value_is_read_only());
        let published = ParameterBinding { publish: true, ..Default::default() };
        assert!(!published.value_is_read_only());
    }

    #[test]
    fn a_successful_read_stores_the_value_and_clears_the_fault() {
        let mut p = InstanceParameters::new();
        p.bind("hvac", "temperature", sourced());
        p.record_error("hvac", "temperature", "timeout");
        assert_eq!(p.binding_state("hvac", "temperature"), BindingState::Faulted);

        p.record_read("hvac", "temperature", ParameterValue::Float(21.4), 12.0);
        assert_eq!(p.binding_state("hvac", "temperature"), BindingState::Sourced);
        assert_eq!(p.get("hvac", "temperature"), Some(&ParameterValue::Float(21.4)));
        assert_eq!(p.binding("hvac", "temperature").unwrap().last_read_s, Some(12.0));
    }

    #[test]
    fn a_failed_read_keeps_the_previous_value_visible() {
        let mut p = InstanceParameters::new();
        p.bind("hvac", "temperature", sourced());
        p.record_read("hvac", "temperature", ParameterValue::Float(21.4), 1.0);
        p.record_error("hvac", "temperature", "connection refused");
        // The number survives for display; the STATE is what tells the truth.
        assert_eq!(p.get("hvac", "temperature"), Some(&ParameterValue::Float(21.4)));
        assert_eq!(p.binding_state("hvac", "temperature"), BindingState::Faulted);
    }

    #[test]
    fn unbinding_hands_the_value_back_rather_than_blanking_it() {
        let mut p = InstanceParameters::new();
        p.bind("hvac", "temperature", sourced());
        p.record_read("hvac", "temperature", ParameterValue::Float(21.4), 1.0);

        let removed = p.unbind("hvac", "temperature");
        assert!(removed.is_some());
        assert_eq!(p.binding_state("hvac", "temperature"), BindingState::Local);
        assert_eq!(p.get("hvac", "temperature"), Some(&ParameterValue::Float(21.4)));
        // The empty domain map is cleaned up rather than left behind.
        assert!(p.bindings.get("hvac").is_none());
    }

    #[test]
    fn parameter_values_round_trip_through_their_edit_string() {
        for v in [
            ParameterValue::Bool(true),
            ParameterValue::Int(-7),
            ParameterValue::Float(21.4),
            ParameterValue::String("zone 3".into()),
            ParameterValue::Vector3([1.0, 2.0, 3.0]),
        ] {
            let parsed = ParameterValue::parse(v.type_name(), &v.edit_string());
            assert_eq!(parsed.as_ref(), Some(&v), "{} did not round-trip", v.type_name());
        }
    }

    #[test]
    fn a_value_that_does_not_fit_its_type_is_refused_not_coerced() {
        assert!(ParameterValue::parse("Int", "not a number").is_none());
        assert!(ParameterValue::parse("Float", "").is_none());
        assert!(ParameterValue::parse("Vector3", "1, 2").is_none());
        // Binary has no text form and must never be authored as one.
        assert!(ParameterValue::parse("Binary", "AAAA").is_none());
    }
}

#[cfg(test)]
mod domain_tests {
    use super::*;

    const HVAC: &str = r#"
[metadata]
class_name = "Domain"

[attributes]
description = "Building climate control"
version = 3
requires_ai_consent = true
export_targets = "warehouse_pg, training_mcp"

[keys.setpoint]
type = "Float"
required = true
min = -40.0
max = 120.0
description = "Target zone temperature"

[keys.mode]
type = "String"
"#;

    #[test]
    fn a_domain_instance_parses_into_its_schema() {
        let d = DomainSchema::from_instance_toml("hvac", HVAC).expect("parses");
        assert_eq!(d.id, "hvac");
        assert_eq!(d.version, 3);
        assert!(d.requires_ai_consent);
        assert_eq!(d.description, "Building climate control");
        assert_eq!(d.keys.len(), 2);
    }

    #[test]
    fn export_targets_split_and_trim() {
        let d = DomainSchema::from_instance_toml("hvac", HVAC).unwrap();
        assert_eq!(d.export_targets, vec!["warehouse_pg", "training_mcp"]);
    }

    #[test]
    fn a_key_carries_its_type_requiredness_and_bounds() {
        let d = DomainSchema::from_instance_toml("hvac", HVAC).unwrap();
        let sp = &d.keys["setpoint"];
        assert_eq!(sp.value_type, ParameterValueType::Float);
        assert!(sp.required);
        // min + max became validation rules
        assert_eq!(sp.validation.len(), 2);
        assert!(sp.validation.iter().any(|r| r.rule_type == "min" && r.value.starts_with("-40")));
        // an unspecified `required` defaults to false
        assert!(!d.keys["mode"].required);
    }

    #[test]
    fn an_unknown_key_type_is_an_error_not_a_silent_default() {
        let bad = "[keys.x]\ntype = \"Quaternion\"\n";
        let err = DomainSchema::from_instance_toml("d", bad).unwrap_err();
        assert!(err.contains("Quaternion"), "{err}");
    }

    #[test]
    fn validate_enforces_a_governed_key_but_allows_an_ungoverned_one() {
        let d = DomainSchema::from_instance_toml("hvac", HVAC).unwrap();
        assert!(d.validate("setpoint", &ParameterValue::Float(21.0)).is_ok());
        assert!(d.validate("setpoint", &ParameterValue::String("warm".into())).is_err());
        // A domain describes what it governs; extra keys are not an error.
        assert!(d.validate("not_in_schema", &ParameterValue::Bool(true)).is_ok());
    }

    #[test]
    fn missing_required_keys_are_reported() {
        let d = DomainSchema::from_instance_toml("hvac", HVAC).unwrap();
        let mut vals = HashMap::new();
        assert_eq!(d.missing_required(&vals), vec!["setpoint"]);
        vals.insert("setpoint".to_string(), ParameterValue::Float(21.0));
        assert!(d.missing_required(&vals).is_empty());
    }

    #[test]
    fn the_registry_is_an_index_that_rebuilds_wholesale() {
        let mut reg = DomainRegistry::default();
        let errs = reg.reindex([("hvac", HVAC)]);
        assert!(errs.is_empty(), "{errs:?}");
        assert!(reg.domains.contains_key("hvac"));

        // Re-indexing without hvac drops it, rather than leaving a ghost behind
        // after the instance is deleted from the Explorer.
        let errs = reg.reindex([("telemetry", "[keys]\n")]);
        assert!(errs.is_empty());
        assert!(!reg.domains.contains_key("hvac"));
        assert!(reg.domains.contains_key("telemetry"));
    }

    #[test]
    fn a_malformed_domain_is_reported_rather_than_skipped() {
        let mut reg = DomainRegistry::default();
        let errs = reg.reindex([("good", "[keys]\n"), ("bad", "[keys.x]\ntype = \"Nope\"\n")]);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("bad"), "{:?}", errs);
        // the good one still landed
        assert!(reg.domains.contains_key("good"));
    }
}
