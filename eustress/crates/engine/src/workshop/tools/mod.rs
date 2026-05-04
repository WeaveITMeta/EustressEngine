//! Workshop tool registry — compatibility shim over the shared
//! `eustress-tools` crate.
//!
//! The `ToolRegistry`, `ToolHandler` trait, `ToolContext`,
//! `ToolDefinition`, `ToolResult`, and every tool handler module
//! (entity / file / script / memory / diff / git / simulation /
//! physics / spatial / universe) now live in the shared crate so the
//! out-of-process MCP server can register and dispatch the exact same
//! tools the Workshop agent ships.
//!
//! This module re-exports the shared types under the old path so
//! existing engine callers (`crate::workshop::tools::ToolRegistry`,
//! `crate::workshop::tools::entity_tools::CreateEntityTool`, etc.)
//! keep compiling without changes. The only engine-specific piece
//! that stays is `claude_tools_for` — it depends on
//! `crate::soul::claude_client::ClaudeTool` and that crate-internal
//! type has no place in the shared tool registry.

// Re-export core types.
pub use eustress_tools::{
    ToolContext, ToolDefinition, ToolHandler, ToolRegistry, ToolResult,
};

// Re-export Luau execution types for the Workshop agent.
pub use eustress_tools::registry::{
    LuauCreatedEntity, LuauExecutionResult, LuauExecutor,
};

// Re-export every tool handler module — callers continue to use
// `tools::entity_tools::CreateEntityTool` without edits.
pub use eustress_tools::diff_tools;
pub use eustress_tools::entity_tools;
pub use eustress_tools::file_tools;
pub use eustress_tools::git_tools;
pub use eustress_tools::memory_tools;
pub use eustress_tools::physics_tools;
pub use eustress_tools::script_tools;
pub use eustress_tools::simulation_tools;
pub use eustress_tools::spatial_tools;
pub use eustress_tools::universe_tools;

// Also re-export the one-shot helpers so the engine's Workshop plugin
// can call `tools::register_all_tools(&mut registry)` and get every
// shipped handler in one line.
pub use eustress_tools::{default_registry, register_all_tools};

use super::modes::WorkshopMode;

/// Build the Claude-API tool list for the given active modes.
///
/// This was previously an inherent method on `ToolRegistry`, but the
/// registry now lives in a Bevy-agnostic shared crate that can't
/// depend on `crate::soul::claude_client::ClaudeTool`. Callers
/// invoke it as a free function instead — the behaviour is
/// identical.
///
/// ```ignore
/// // Old:
/// registry.claude_tools(&active_modes.all())
/// // New:
/// claude_tools_for(&registry, &active_modes.all())
/// ```
pub fn claude_tools_for(
    registry: &ToolRegistry,
    active_modes: &[WorkshopMode],
) -> Vec<crate::soul::claude_client::ClaudeTool> {
    registry
        .tools_for_modes(active_modes)
        .into_iter()
        .map(|t| crate::soul::claude_client::ClaudeTool {
            name: t.name.to_string(),
            description: t.description.to_string(),
            input_schema: t.input_schema,
        })
        .collect()
}
