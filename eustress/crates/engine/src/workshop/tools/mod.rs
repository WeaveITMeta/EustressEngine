//! Workshop Tool Registry — declarative MCP tool system.
//!
//! Every tool the AI agent can call is a struct implementing `ToolHandler`.
//! Tools are registered per `WorkshopMode` and discovered by the Claude API
//! via the `tools` array in the request body.
//!
//! The `ToolRegistry` builds the Claude-compatible tools JSON and dispatches
//! tool calls by name, publishing results to EustressStream topics.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Tool Definition (declarative, struct-based)
// ---------------------------------------------------------------------------

/// Static definition of an MCP tool the AI agent can call.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// Tool name used in Claude tool_use calls (e.g. "create_entity").
    pub name: &'static str,
    /// Human-readable description injected into the Claude system prompt.
    pub description: &'static str,
    /// JSON Schema for the input parameters.
    pub input_schema: serde_json::Value,
    /// Which modes include this tool. `General` tools are available in ALL modes.
    pub modes: &'static [WorkshopMode],
    /// Whether this tool requires user approval before execution.
    pub requires_approval: bool,
    /// EustressStream topics this tool publishes to.
    pub stream_topics: &'static [&'static str],
}

// ---------------------------------------------------------------------------
// Tool Result
// ---------------------------------------------------------------------------

/// Result of executing a tool — sent back to Claude as `tool_result`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool name that was called.
    pub tool_name: String,
    /// tool_use_id from Claude's response (echoed back).
    pub tool_use_id: String,
    /// Whether execution succeeded.
    pub success: bool,
    /// Result content (shown to AI as tool_result content).
    pub content: String,
    /// Optional structured data for UI rendering.
    pub structured_data: Option<serde_json::Value>,
    /// Stream topic this result was published to.
    pub stream_topic: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool Handler (trait)
// ---------------------------------------------------------------------------

/// Trait implemented by each tool. Handlers receive JSON input and return
/// a `ToolResult`. They have read access to engine state via the passed
/// context, and signal writes via the returned result.
///
/// Tools run on the main thread (inside a Bevy system), so they must be
/// fast. Long-running operations should spawn background tasks and return
/// an immediate "started" result.
pub trait ToolHandler: Send + Sync + 'static {
    /// Return the tool's definition (built on call since JSON Schema isn't const).
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given JSON input.
    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

/// Context passed to tool handlers during execution.
/// Provides read access to engine state without requiring &World directly.
pub struct ToolContext {
    /// Current Space root path (Universe-locked).
    pub space_root: std::path::PathBuf,
    /// Universe root path (the sandbox boundary).
    pub universe_root: std::path::PathBuf,
    /// Current user ID (from auth).
    pub user_id: Option<String>,
    /// Current username.
    pub username: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool Registry
// ---------------------------------------------------------------------------

/// Registry of all available tools, indexed by name.
/// Built at startup by each module registering its tools.
#[derive(Resource)]
pub struct ToolRegistry {
    handlers: HashMap<&'static str, Box<dyn ToolHandler>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }
}

impl ToolRegistry {
    /// Register a tool handler.
    pub fn register(&mut self, handler: impl ToolHandler) {
        let name = handler.definition().name;
        self.handlers.insert(name, Box::new(handler));
    }

    /// Get all tool definitions available for the given active modes.
    /// General tools are always included. Multiple modes stack additively.
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

    /// Build Claude API tool definitions for the given active modes.
    /// Returns `Vec<ClaudeTool>` ready for the agentic request.
    pub fn claude_tools(&self, active_modes: &[WorkshopMode]) -> Vec<crate::soul::claude_client::ClaudeTool> {
        self.tools_for_modes(active_modes)
            .into_iter()
            .map(|t| crate::soul::claude_client::ClaudeTool {
                name: t.name.to_string(),
                description: t.description.to_string(),
                input_schema: t.input_schema,
            })
            .collect()
    }

    /// Dispatch a tool call by name. Returns the tool result.
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

    /// Get the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.handlers.len()
    }

    /// List all registered tool names.
    pub fn tool_names(&self) -> Vec<&'static str> {
        self.handlers.keys().copied().collect()
    }
}

// ---------------------------------------------------------------------------
// Sub-modules (tool implementations)
// ---------------------------------------------------------------------------

pub mod entity_tools;
pub mod file_tools;
pub mod script_tools;
pub mod memory_tools;
pub mod diff_tools;
pub mod git_tools;
pub mod simulation_tools;
pub mod physics_tools;
pub mod spatial_tools;
