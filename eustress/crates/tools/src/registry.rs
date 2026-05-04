//! Core registry types — `ToolDefinition`, `ToolResult`, `ToolHandler`,
//! `ToolContext`, `ToolRegistry`. No Bevy dependency here (the engine
//! wraps the registry in a Resource newtype); no Claude-API types here
//! (engine-side `claude_tools()` helper lives next to the Claude
//! client).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Tool Definition (declarative, struct-based)
// ---------------------------------------------------------------------------

/// Static definition of an MCP tool a caller (Claude agent, external
/// IDE, script) can invoke.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// Tool name used in the Anthropic `tool_use` frame and in MCP
    /// `tools/list` + `tools/call`.
    pub name: &'static str,
    /// Human-readable description injected into the Claude system
    /// prompt / advertised by MCP's `tools/list`.
    pub description: &'static str,
    /// JSON Schema for the input parameters.
    pub input_schema: serde_json::Value,
    /// Which Workshop modes include this tool. `General` tools are
    /// always exposed regardless of active mode.
    pub modes: &'static [WorkshopMode],
    /// Whether the tool requires user approval before execution in
    /// the Workshop agent loop. MCP clients interpret this as a
    /// hint — external IDEs may always require approval.
    pub requires_approval: bool,
    /// EustressStream topics this tool publishes to when executed.
    pub stream_topics: &'static [&'static str],
}

// ---------------------------------------------------------------------------
// Tool Result
// ---------------------------------------------------------------------------

/// Result of executing a tool. Shaped to mirror the Anthropic
/// `tool_result` frame so callers that speak Claude's API need zero
/// translation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub tool_use_id: String,
    pub success: bool,
    /// Human-readable result content — what the AI sees as the tool
    /// outcome.
    pub content: String,
    /// Optional machine-readable payload for UI rendering or
    /// downstream processing.
    pub structured_data: Option<serde_json::Value>,
    /// The EustressStream topic the result was published to (for
    /// subscribers listening on that topic).
    pub stream_topic: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool Handler (trait)
// ---------------------------------------------------------------------------

/// The handler trait each tool implements. Input is a JSON value; the
/// returned `ToolResult` carries both human-readable content and
/// optional structured data.
///
/// Handlers must be `Send + Sync + 'static` so the registry can store
/// them in a trait-object map shared across threads (the MCP server's
/// async runtime + the engine's Bevy main thread both dispatch into
/// the same registry).
pub trait ToolHandler: Send + Sync + 'static {
    /// Return the tool's definition. Built fresh each call because the
    /// `input_schema: serde_json::Value` isn't `const`-constructible.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given JSON input.
    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

// ---------------------------------------------------------------------------
// Tool Context
// ---------------------------------------------------------------------------

/// An entity created by a Luau script via `Instance.new("Part")`.
/// Returned by the `LuauExecutor` callback so `execute_luau` can
/// materialize the instances as on-disk entity folders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuauCreatedEntity {
    pub class_name: String,
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub size: [f32; 3],
    pub color: [f32; 4],
    pub material: String,
    /// Part shape — "Block", "Ball", "Cylinder", "Wedge", "CornerWedge", "Cone".
    #[serde(default = "default_shape")]
    pub shape: String,
    pub transparency: f32,
    pub anchored: bool,
    pub can_collide: bool,
}

fn default_shape() -> String { "Block".to_string() }

/// Result of running a Luau script via the executor callback.
#[derive(Debug, Clone)]
pub struct LuauExecutionResult {
    /// Whether the script ran without errors.
    pub success: bool,
    /// Human-readable output or error message.
    pub message: String,
    /// Entities created during execution via Instance.new().
    pub created_entities: Vec<LuauCreatedEntity>,
}

/// Callback type for inline Luau execution. The engine provides this;
/// the MCP server leaves it `None`. Takes (source_code, chunk_name)
/// and returns the execution result with any created instances.
pub type LuauExecutor = Arc<dyn Fn(&str, &str) -> LuauExecutionResult + Send + Sync>;

/// Context passed to handlers during execution. Carries paths + auth
/// identity; all fields are `Send + Sync + 'static` so the context
/// can cross the tokio/Bevy boundary without issue.
#[derive(Clone)]
pub struct ToolContext {
    /// Current Space root path (Universe-locked).
    pub space_root: PathBuf,
    /// Universe root path (the sandbox boundary).
    pub universe_root: PathBuf,
    /// Current user ID (from auth).
    pub user_id: Option<String>,
    /// Current username.
    pub username: Option<String>,
    /// Optional Luau VM executor — populated by the engine when the
    /// `luau` feature is enabled. When present, `execute_luau` runs
    /// the script inline and materializes created instances as entity
    /// folders. When `None`, the tool only writes the script file for
    /// later hot-reload.
    pub luau_executor: Option<LuauExecutor>,
}

// ---------------------------------------------------------------------------
// Tool Registry
// ---------------------------------------------------------------------------

/// Registry of all available tools, indexed by name.
///
/// Built once at startup by calling `register(handler)` for each tool.
/// Both the engine (via a Bevy-Resource newtype) and the MCP server
/// hold a registry built with the same default handlers, so a tool
/// added once is exposed through every surface.
#[cfg_attr(feature = "bevy", derive(bevy_ecs::resource::Resource))]
pub struct ToolRegistry {
    handlers: HashMap<&'static str, Box<dyn ToolHandler>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self { handlers: HashMap::new() }
    }
}

impl ToolRegistry {
    /// Register a tool handler.
    pub fn register(&mut self, handler: impl ToolHandler) {
        let name = handler.definition().name;
        self.handlers.insert(name, Box::new(handler));
    }

    /// Return definitions for every tool exposed under the given active
    /// modes. `General` tools are always included; additional modes
    /// stack additively.
    pub fn tools_for_modes(&self, active_modes: &[WorkshopMode]) -> Vec<ToolDefinition> {
        self.handlers
            .values()
            .map(|h| h.definition())
            .filter(|d| {
                d.modes.contains(&WorkshopMode::General)
                    || d.modes.iter().any(|m| active_modes.contains(m))
            })
            .collect()
    }

    /// Return every registered tool's definition, regardless of mode.
    /// Used by MCP's `tools/list` where we want the full catalogue.
    pub fn all_tools(&self) -> Vec<ToolDefinition> {
        self.handlers.values().map(|h| h.definition()).collect()
    }

    /// Dispatch a tool call by name. On unknown tools returns a
    /// failed `ToolResult` rather than panicking — the caller can
    /// surface the error to the AI, which will try a different tool.
    pub fn dispatch(
        &self,
        tool_name: &str,
        tool_use_id: &str,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> ToolResult {
        match self.handlers.get(tool_name) {
            Some(handler) => {
                let mut result = handler.execute(input, ctx);
                result.tool_use_id = tool_use_id.to_string();
                result
            }
            None => ToolResult {
                tool_name: tool_name.to_string(),
                tool_use_id: tool_use_id.to_string(),
                success: false,
                content: format!("Unknown tool: {}", tool_name),
                structured_data: None,
                stream_topic: None,
            },
        }
    }

    pub fn tool_count(&self) -> usize {
        self.handlers.len()
    }

    pub fn tool_names(&self) -> Vec<&'static str> {
        self.handlers.keys().copied().collect()
    }
}
