//! # registry
//!
//! Tool registry backed entirely by `.tool.toml` files on disk.
//! The `ToolIndex` is an in-memory structure rebuilt from these files at startup
//! and kept live via file watching. The AI can read, write, and search `.tool.toml`
//! files directly — no migration scripts, no schema upgrades.
//!
//! ## Table of Contents
//!
//! | Section            | Purpose                                                       |
//! |--------------------|---------------------------------------------------------------|
//! | `ToolCategory`     | Enum classifying tools by workshop domain                     |
//! | `ToolCapability`   | What operations a tool can perform (drilling, cutting, etc.)  |
//! | `ToolMeshConfig`   | 3D mesh + transform for the digital twin entity spawn         |
//! | `ToolIotConfig`    | IoT chip binding (MQTT topic, GPS device ID, poll interval)   |
//! | `ToolSpec`         | Physical specifications shown in the Properties Panel         |
//! | `RegisteredTool`   | Full tool definition — serialises to/from .tool.toml          |
//! | `ToolIndex`        | In-memory search index built from all .tool.toml files        |
//! | `ToolRegistry`     | Filesystem manager — load, save, watch, bulk-scan             |

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// 1. Enumerations
// ============================================================================

/// Broad category classifying what kind of tool this is
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    /// Hand tools: hammers, wrenches, screwdrivers, chisels
    HandTool,
    /// Power tools: drills, saws, grinders, sanders
    PowerTool,
    /// Precision measurement: calipers, micrometers, levels, squares
    Measurement,
    /// Cutting tools: blades, drill bits, router bits, end mills
    CuttingTool,
    /// Fastening: torque wrenches, impact drivers, nail guns
    Fastening,
    /// Welding and joining: MIG, TIG, soldering stations
    Welding,
    /// Computer-Numeric Control machinery: CNC routers, mills, lathes, laser cutters
    CncMachine,
    /// 3D printing and additive manufacturing
    Additive,
    /// Finishing: sanders, polishers, spray guns, paint equipment
    Finishing,
    /// Safety: PPE, fire extinguishers, first aid
    Safety,
    /// Clamping and workholding: vises, clamps, fixtures
    Workholding,
    /// Electronics: soldering irons, multimeters, oscilloscopes
    Electronics,
    /// Custom / user-defined category (value stored as string)
    Custom(String),
}

impl ToolCategory {
    /// Returns a human-readable display label for the Properties Panel
    pub fn display_label(&self) -> String {
        match self {
            ToolCategory::HandTool => "Hand Tool".into(),
            ToolCategory::PowerTool => "Power Tool".into(),
            ToolCategory::Measurement => "Measurement".into(),
            ToolCategory::CuttingTool => "Cutting Tool".into(),
            ToolCategory::Fastening => "Fastening".into(),
            ToolCategory::Welding => "Welding & Joining".into(),
            ToolCategory::CncMachine => "CNC Machine".into(),
            ToolCategory::Additive => "Additive Manufacturing".into(),
            ToolCategory::Finishing => "Finishing".into(),
            ToolCategory::Safety => "Safety".into(),
            ToolCategory::Workholding => "Workholding".into(),
            ToolCategory::Electronics => "Electronics".into(),
            ToolCategory::Custom(name) => name.clone(),
        }
    }
}

/// A discrete operation this tool is capable of performing.
/// Used by the build guide generator to match steps to tools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    /// Drilling holes — specifies maximum drill diameter in mm
    Drilling { max_diameter_mm: Option<u32> },
    /// Cutting material — specifies material types supported
    Cutting { materials: Vec<String> },
    /// Fastening with torque — specifies max torque in Newton-metres
    Fastening { max_torque_nm: Option<u32> },
    /// Measuring dimensions — specifies measurement range and precision
    Measuring { precision_mm: Option<f32> },
    /// Welding / joining metal — specifies weld process
    Welding { process: String },
    /// Soldering electronics — specifies temperature range
    Soldering { max_temp_celsius: Option<u32> },
    /// Grinding / shaping material
    Grinding,
    /// Sanding / finishing surfaces
    Sanding,
    /// 3D printing — specifies max build volume
    Printing { build_volume_mm: Option<[u32; 3]> },
    /// CNC milling / routing
    Milling { travel_mm: Option<[u32; 3]> },
    /// Clamping / workholding
    Clamping { max_force_kg: Option<u32> },
    /// Custom capability described in free text
    Custom(String),
}

// ============================================================================
// 2. Sub-Structures
// ============================================================================

/// 3D mesh configuration for the digital twin entity.
/// When spawned into a Bevy Space, these fields drive the instanced mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMeshConfig {
    /// Path to the .glb or .gltf mesh file, relative to the workspace root
    pub mesh_path: String,
    /// Optional material override path (.mat.toml)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_path: Option<String>,
    /// Scale applied to the mesh on spawn (default 1.0)
    #[serde(default = "default_one")]
    pub scale: f32,
    /// Rotation offset in degrees [x, y, z] applied on spawn
    #[serde(default)]
    pub rotation_offset: [f32; 3],
    /// Whether this tool casts shadows in the Scene
    #[serde(default = "default_true")]
    pub cast_shadow: bool,
}

fn default_one() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

impl Default for ToolMeshConfig {
    fn default() -> Self {
        Self {
            mesh_path: "assets/models/tools/generic_tool.glb".into(),
            material_path: None,
            scale: 1.0,
            rotation_offset: [0.0, 0.0, 0.0],
            cast_shadow: true,
        }
    }
}

/// IoT chip binding configuration.
/// Defines how the live telemetry stream is connected to this tool's entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolIotConfig {
    /// Unique hardware chip identifier (MAC address, serial, or custom label)
    pub chip_id: String,
    /// MQTT topic this chip broadcasts on (e.g. "workshop/tools/chip-abc123/telemetry")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mqtt_topic: Option<String>,
    /// HTTP REST endpoint for polling (fallback when MQTT is unavailable)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_poll_url: Option<String>,
    /// How often to poll in seconds (applies to HTTP fallback only)
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u32,
    /// Whether this chip has GPS capability (position tracking)
    #[serde(default = "default_true")]
    pub has_gps: bool,
    /// Whether this chip has an in-use / idle status sensor
    #[serde(default)]
    pub has_status_sensor: bool,
}

fn default_poll_interval() -> u32 {
    30
}

/// Physical and technical specifications of the tool.
/// These fields are rendered directly in the Properties Panel as editable entries.
/// New entries can be added freely — the panel renders all key-value pairs from `extra`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolSpec {
    /// Manufacturer brand name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// Model number or product name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Serial number for warranty / tracking
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    /// Power source: "battery", "corded", "pneumatic", "manual"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_source: Option<String>,
    /// Voltage rating (e.g. "18V", "120V")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage: Option<String>,
    /// Weight in kilograms
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight_kg: Option<f32>,
    /// Year of purchase
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year_purchased: Option<u32>,
    /// Purchase price in USD
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchase_price_usd: Option<f32>,
    /// ASIN for Amazon reorder / procurement
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amazon_asin: Option<String>,
    /// Arbitrary additional key-value specification fields.
    /// The Properties Panel renders these as editable rows.
    /// New entries can be added by the AI or user without a code change.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, String>,
}

// ============================================================================
// 3. RegisteredTool — the .tool.toml root struct
// ============================================================================

/// A fully registered workshop tool. Serialises to/from a `.tool.toml` file.
///
/// Every field maps directly to a row in the Properties Panel.
/// The `mesh` section drives the 3D instanced entity in the digital twin Space.
/// The `iot` section binds the live GPS + status telemetry stream.
/// The `capabilities` list is used by the build guide generator to match steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredTool {
    /// Stable unique identifier — never changes after registration
    pub id: Uuid,
    /// Human-readable display name (e.g. "Milwaukee M18 Drill")
    pub name: String,
    /// One-sentence description of what this tool is
    pub description: String,
    /// Broad category used for grouping in the Explorer and guide generation
    pub category: ToolCategory,
    /// What this tool can do — used to match steps in build guides
    pub capabilities: Vec<ToolCapability>,
    /// Where this tool lives when not in use (e.g. "Bench 3, right shelf")
    /// This is the human-readable label — used in build guides and AI context
    pub home_location: String,
    /// UUID of the `StorageUnit` (container folder) this tool lives in by default.
    /// When set, the tool's `.tool.toml` file should be inside that container's folder.
    /// When `None`, the tool lives in the root `tools/` folder.
    /// Updated automatically by `StorageManager::relocate_tool_by_position()` on GPS movement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_container_id: Option<Uuid>,
    /// Physical and technical specifications (shown in Properties Panel)
    pub spec: ToolSpec,
    /// 3D mesh configuration for the digital twin entity spawn
    #[serde(default)]
    pub mesh: ToolMeshConfig,
    /// IoT chip binding — optional, present only if the tool has a GPS chip
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iot: Option<ToolIotConfig>,
    /// Usage instructions injected into AI build guide context
    pub how_to_use: String,
    /// Safety notes injected into AI build guide context
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub safety_notes: Vec<String>,
    /// Tags for freeform search and AI capability matching
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Whether this tool is currently available for assignment to a build guide step
    #[serde(default = "default_true")]
    pub available: bool,
    /// ISO 8601 timestamp when this tool was first registered
    pub registered_at: DateTime<Utc>,
    /// ISO 8601 timestamp when this TOML file was last modified
    pub updated_at: DateTime<Utc>,
}

impl RegisteredTool {
    /// Create a new tool with required fields — all optional fields use sensible defaults
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        category: ToolCategory,
        home_location: impl Into<String>,
        how_to_use: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            category,
            capabilities: Vec::new(),
            home_location: home_location.into(),
            home_container_id: None,
            spec: ToolSpec::default(),
            mesh: ToolMeshConfig::default(),
            iot: None,
            how_to_use: how_to_use.into(),
            safety_notes: Vec::new(),
            tags: Vec::new(),
            available: true,
            registered_at: now,
            updated_at: now,
        }
    }

    /// Derive the canonical `.tool.toml` filename from the tool name
    /// (e.g. "Milwaukee M18 Drill" → "milwaukee-m18-drill.tool.toml")
    pub fn canonical_filename(&self) -> String {
        let slug = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        // Collapse consecutive dashes
        let slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        format!("{}.tool.toml", slug)
    }

    /// Build the AI context string for this tool — injected into build guide prompts
    pub fn ai_context(&self) -> String {
        let caps: Vec<String> = self
            .capabilities
            .iter()
            .map(|c| format!("{:?}", c))
            .collect();
        let location = if self.available {
            format!("currently at {} (available)", self.home_location)
        } else {
            format!("currently at {} (in use)", self.home_location)
        };
        format!(
            "TOOL: {}\nDESCRIPTION: {}\nLOCATION: {}\nCAPABILITIES: {}\nHOW TO USE: {}\nSAFETY: {}",
            self.name,
            self.description,
            location,
            caps.join(", "),
            self.how_to_use,
            self.safety_notes.join("; "),
        )
    }
}

// ============================================================================
// 4. ToolIndex — in-memory search index
// ============================================================================

/// In-memory index of all registered tools. Rebuilt from `.tool.toml` files
/// via `ToolRegistry::load_all()`. Supports fast search without file I/O.
#[derive(Debug, Default)]
pub struct ToolIndex {
    /// Primary map: tool ID → full tool definition
    by_id: HashMap<Uuid, RegisteredTool>,
    /// Secondary index: lowercase name fragment → list of tool IDs (for text search)
    by_name: HashMap<String, Vec<Uuid>>,
    /// Secondary index: capability key → list of tool IDs
    by_capability: HashMap<String, Vec<Uuid>>,
    /// Secondary index: tag → list of tool IDs
    by_tag: HashMap<String, Vec<Uuid>>,
    /// Source file paths: tool ID → .tool.toml path on disk
    file_paths: HashMap<Uuid, PathBuf>,
}

impl ToolIndex {
    /// Build a fresh index from a list of (tool, source_path) pairs
    pub fn build(entries: Vec<(RegisteredTool, PathBuf)>) -> Self {
        let mut index = Self::default();
        for (tool, path) in entries {
            index.insert(tool, path);
        }
        index
    }

    /// Insert or update a single tool into the index
    pub fn insert(&mut self, tool: RegisteredTool, path: PathBuf) {
        let id = tool.id;

        // Name index — tokenise by word for substring search
        for word in tool.name.to_lowercase().split_whitespace() {
            self.by_name
                .entry(word.to_string())
                .or_default()
                .push(id);
        }

        // Capability index — key by debug representation
        for cap in &tool.capabilities {
            let key = capability_key(cap);
            self.by_capability.entry(key).or_default().push(id);
        }

        // Tag index
        for tag in &tool.tags {
            self.by_tag
                .entry(tag.to_lowercase())
                .or_default()
                .push(id);
        }

        self.file_paths.insert(id, path);
        self.by_id.insert(id, tool);
    }

    /// Remove a tool from all indexes by its ID
    pub fn remove(&mut self, id: &Uuid) {
        if let Some(tool) = self.by_id.remove(id) {
            // Clean name index
            for word in tool.name.to_lowercase().split_whitespace() {
                if let Some(ids) = self.by_name.get_mut(word) {
                    ids.retain(|i| i != id);
                }
            }
            // Clean capability index
            for cap in &tool.capabilities {
                let key = capability_key(cap);
                if let Some(ids) = self.by_capability.get_mut(&key) {
                    ids.retain(|i| i != id);
                }
            }
            // Clean tag index
            for tag in &tool.tags {
                if let Some(ids) = self.by_tag.get_mut(&tag.to_lowercase()) {
                    ids.retain(|i| i != id);
                }
            }
        }
        self.file_paths.remove(id);
    }

    /// Look up a tool by its exact UUID
    pub fn get(&self, id: &Uuid) -> Option<&RegisteredTool> {
        self.by_id.get(id)
    }

    /// Return all registered tools
    pub fn all(&self) -> impl Iterator<Item = &RegisteredTool> {
        self.by_id.values()
    }

    /// Full-text search by name fragment — returns all tools whose name contains the query
    pub fn search_by_name(&self, query: &str) -> Vec<&RegisteredTool> {
        let query = query.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for word in query.split_whitespace() {
            if let Some(ids) = self.by_name.get(word) {
                for id in ids {
                    if seen.insert(*id) {
                        if let Some(tool) = self.by_id.get(id) {
                            results.push(tool);
                        }
                    }
                }
            }
        }
        // Also do substring scan for partial matches not caught by word tokenisation
        for tool in self.by_id.values() {
            if tool.name.to_lowercase().contains(&query) && seen.insert(tool.id) {
                results.push(tool);
            }
        }
        results
    }

    /// Find all tools that have a given capability (by variant name prefix)
    pub fn search_by_capability(&self, capability_prefix: &str) -> Vec<&RegisteredTool> {
        let prefix = capability_prefix.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for (key, ids) in &self.by_capability {
            if key.to_lowercase().starts_with(&prefix) {
                for id in ids {
                    if seen.insert(*id) {
                        if let Some(tool) = self.by_id.get(id) {
                            results.push(tool);
                        }
                    }
                }
            }
        }
        results
    }

    /// Find all tools tagged with the given tag
    pub fn search_by_tag(&self, tag: &str) -> Vec<&RegisteredTool> {
        let tag = tag.to_lowercase();
        match self.by_tag.get(&tag) {
            Some(ids) => ids
                .iter()
                .filter_map(|id| self.by_id.get(id))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Find all available tools (not currently in use)
    pub fn available_tools(&self) -> Vec<&RegisteredTool> {
        self.by_id.values().filter(|t| t.available).collect()
    }

    /// Find all tools that have IoT chip bindings registered
    pub fn iot_tracked_tools(&self) -> Vec<&RegisteredTool> {
        self.by_id.values().filter(|t| t.iot.is_some()).collect()
    }

    /// Total number of registered tools
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Returns true if no tools are registered
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Get the source file path for a tool
    pub fn file_path(&self, id: &Uuid) -> Option<&PathBuf> {
        self.file_paths.get(id)
    }

    /// Build the AI context block for all tools — injected into build guide prompts
    pub fn build_ai_context(&self) -> String {
        let mut lines = vec!["=== REGISTERED WORKSHOP TOOLS ===".to_string()];
        for tool in self.by_id.values() {
            lines.push(String::new());
            lines.push(tool.ai_context());
        }
        lines.join("\n")
    }
}

/// Derive a short string key from a ToolCapability variant for indexing
fn capability_key(cap: &ToolCapability) -> String {
    match cap {
        ToolCapability::Drilling { .. } => "drilling".into(),
        ToolCapability::Cutting { .. } => "cutting".into(),
        ToolCapability::Fastening { .. } => "fastening".into(),
        ToolCapability::Measuring { .. } => "measuring".into(),
        ToolCapability::Welding { .. } => "welding".into(),
        ToolCapability::Soldering { .. } => "soldering".into(),
        ToolCapability::Grinding => "grinding".into(),
        ToolCapability::Sanding => "sanding".into(),
        ToolCapability::Printing { .. } => "printing".into(),
        ToolCapability::Milling { .. } => "milling".into(),
        ToolCapability::Clamping { .. } => "clamping".into(),
        ToolCapability::Custom(s) => s.to_lowercase(),
    }
}

// ============================================================================
// 5. ToolRegistry — filesystem manager
// ============================================================================

/// Manages the `.tool.toml` files on disk and owns the in-memory `ToolIndex`.
///
/// # Usage
/// ```rust,no_run
/// let registry = ToolRegistry::open("/path/to/workshop/tools").unwrap();
/// let drills = registry.index().search_by_capability("drilling");
/// ```
pub struct ToolRegistry {
    /// Root directory where `.tool.toml` files are stored
    tools_dir: PathBuf,
    /// In-memory search index — always in sync with disk via file watcher
    index: ToolIndex,
}

impl ToolRegistry {
    /// Open (or create) a tool registry at the given directory.
    /// Scans all `.tool.toml` files in parallel and builds the in-memory index.
    pub fn open(tools_dir: impl AsRef<Path>) -> Result<Self> {
        let tools_dir = tools_dir.as_ref().to_path_buf();

        // Create the directory if it does not exist
        if !tools_dir.exists() {
            std::fs::create_dir_all(&tools_dir)
                .with_context(|| format!("Failed to create tools directory: {}", tools_dir.display()))?;
            tracing::info!("Created workshop tools directory: {}", tools_dir.display());
        }

        let mut registry = Self {
            tools_dir,
            index: ToolIndex::default(),
        };
        registry.reload()?;
        Ok(registry)
    }

    /// Scan the tools directory for all `.tool.toml` files and rebuild the index.
    /// Uses Rayon for parallel TOML parsing.
    pub fn reload(&mut self) -> Result<usize> {
        let paths: Vec<PathBuf> = std::fs::read_dir(&self.tools_dir)
            .with_context(|| format!("Cannot read tools directory: {}", self.tools_dir.display()))?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml")
                    && path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.ends_with(".tool"))
                        .unwrap_or(false)
                {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        // Parse all TOMLs in parallel
        let parsed: Vec<(RegisteredTool, PathBuf)> = paths
            .par_iter()
            .filter_map(|path| {
                let content = std::fs::read_to_string(path).ok()?;
                match toml::from_str::<RegisteredTool>(&content) {
                    Ok(tool) => Some((tool, path.clone())),
                    Err(err) => {
                        tracing::warn!("Skipping malformed tool file {}: {}", path.display(), err);
                        None
                    }
                }
            })
            .collect();

        let count = parsed.len();
        self.index = ToolIndex::build(parsed);
        tracing::info!("Workshop registry loaded {} tools from {}", count, self.tools_dir.display());
        Ok(count)
    }

    /// Register a new tool — writes a `.tool.toml` file and inserts into the index.
    pub fn register(&mut self, tool: RegisteredTool) -> Result<PathBuf> {
        let filename = tool.canonical_filename();
        let path = self.tools_dir.join(&filename);

        let content = toml::to_string_pretty(&tool)
            .with_context(|| format!("Failed to serialise tool: {}", tool.name))?;

        std::fs::write(&path, &content)
            .with_context(|| format!("Failed to write {}", path.display()))?;

        tracing::info!("Registered tool '{}' → {}", tool.name, path.display());
        self.index.insert(tool, path.clone());
        Ok(path)
    }

    /// Update an existing tool's TOML file and refresh the index entry.
    pub fn update(&mut self, mut tool: RegisteredTool) -> Result<()> {
        tool.updated_at = Utc::now();

        let path = self
            .index
            .file_path(&tool.id)
            .cloned()
            .unwrap_or_else(|| self.tools_dir.join(tool.canonical_filename()));

        let content = toml::to_string_pretty(&tool)
            .with_context(|| format!("Failed to serialise tool: {}", tool.name))?;

        std::fs::write(&path, &content)
            .with_context(|| format!("Failed to write {}", path.display()))?;

        self.index.remove(&tool.id);
        self.index.insert(tool, path);
        Ok(())
    }

    /// Remove a tool from the registry — deletes the `.tool.toml` file from disk.
    pub fn unregister(&mut self, id: &Uuid) -> Result<()> {
        if let Some(path) = self.index.file_path(id).cloned() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to delete {}", path.display()))?;
            tracing::info!("Unregistered tool {} (deleted {})", id, path.display());
        }
        self.index.remove(id);
        Ok(())
    }

    /// Read-only access to the in-memory index
    pub fn index(&self) -> &ToolIndex {
        &self.index
    }

    /// Mutable access to the in-memory index (for hot-reload updates from file watcher)
    pub fn index_mut(&mut self) -> &mut ToolIndex {
        &mut self.index
    }

    /// Returns the directory where `.tool.toml` files are stored
    pub fn tools_dir(&self) -> &Path {
        &self.tools_dir
    }
}

// ============================================================================
// 6. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_drill() -> RegisteredTool {
        let mut tool = RegisteredTool::new(
            "Milwaukee M18 Drill",
            "18V cordless drill/driver with 1/2\" chuck",
            ToolCategory::PowerTool,
            "Bench 3, right shelf",
            "Insert bit, set torque collar, squeeze trigger.",
        );
        tool.capabilities.push(ToolCapability::Drilling { max_diameter_mm: Some(38) });
        tool.capabilities.push(ToolCapability::Fastening { max_torque_nm: Some(60) });
        tool.tags = vec!["drill".into(), "cordless".into(), "18v".into()];
        tool
    }

    #[test]
    fn canonical_filename_slugifies_name() {
        let tool = sample_drill();
        assert_eq!(tool.canonical_filename(), "milwaukee-m18-drill.tool.toml");
    }

    #[test]
    fn index_search_by_name() {
        let tool = sample_drill();
        let id = tool.id;
        let mut index = ToolIndex::default();
        index.insert(tool, PathBuf::from("fake.tool.toml"));
        let results = index.search_by_name("Milwaukee");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }

    #[test]
    fn index_search_by_capability() {
        let tool = sample_drill();
        let id = tool.id;
        let mut index = ToolIndex::default();
        index.insert(tool, PathBuf::from("fake.tool.toml"));
        let results = index.search_by_capability("drilling");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }

    #[test]
    fn index_search_by_tag() {
        let tool = sample_drill();
        let id = tool.id;
        let mut index = ToolIndex::default();
        index.insert(tool, PathBuf::from("fake.tool.toml"));
        let results = index.search_by_tag("cordless");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }

    #[test]
    fn tool_roundtrips_through_toml() {
        let tool = sample_drill();
        let serialised = toml::to_string_pretty(&tool).unwrap();
        let deserialised: RegisteredTool = toml::from_str(&serialised).unwrap();
        assert_eq!(deserialised.name, tool.name);
        assert_eq!(deserialised.id, tool.id);
    }
}
