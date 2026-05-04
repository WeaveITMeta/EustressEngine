//! # eustress-tools — shared tool registry
//!
//! The single source of truth for MCP tool definitions and handlers.
//! Both the in-engine Workshop agent and the out-of-process MCP server
//! import this crate, so a tool added here is immediately available to:
//!
//! * the Claude-driven Workshop agent (Bevy plugin, in-process),
//! * external IDEs speaking MCP (Claude Desktop, Cursor, Windsurf),
//! * any other sibling that connects to the Engine Bridge or spawns
//!   the MCP server directly.
//!
//! ## Design
//!
//! `ToolHandler` implementations take a `ToolContext` — a small struct
//! of `PathBuf`s + auth fields — and return a `ToolResult`. No Bevy
//! types cross the handler boundary, which is why the same code runs
//! in-engine and out-of-process: when invoked by the MCP server,
//! `space_root` points at the Universe the engine is running against,
//! and filesystem writes land in the same place the engine's file
//! watcher will pick them up.
//!
//! Tools that need truly live ECS state (raycasts, live entity
//! queries) can be fulfilled via the optional `live` field on
//! `ToolContext` — the engine populates it with a direct World
//! accessor; the MCP server populates it with an RPC client that
//! round-trips through `<universe>/.eustress/engine.port`.
//!
//! ## Feature: `bevy`
//!
//! When enabled, `ToolRegistry` derives Bevy's `Resource` so the
//! engine can insert it directly. Off by default so the MCP server
//! can depend on this crate without pulling in the Bevy tree.

pub mod modes;
pub mod registry;

pub use modes::WorkshopMode;
pub use registry::{
    ToolContext, ToolDefinition, ToolHandler, ToolRegistry, ToolResult,
};

// Tool implementation modules. Each hosts a family of related tools;
// see the struct docs in the individual files for the full surface.
pub mod diff_tools;
pub mod embedvec_tools;
pub mod entity_tools;
pub mod file_tools;
pub mod git_tools;
pub mod memory_tools;
pub mod physics_tools;
pub mod script_tools;
pub mod shell_tools;
pub mod simulation_tools;
pub mod spatial_tools;
pub mod universe_tools;

/// Build a `ToolRegistry` preloaded with every tool this crate ships.
///
/// Both the engine's Workshop plugin and the MCP server call this at
/// startup. Mode-specific tools (Manufacturing, Finance, etc.) still
/// live in the engine crate because they touch engine-private state —
/// the engine registers those on top of this baseline.
pub fn register_all_tools(registry: &mut ToolRegistry) {
    // Entity manipulation.
    registry.register(entity_tools::CreateEntityTool);
    registry.register(entity_tools::QueryEntitiesTool);
    registry.register(entity_tools::UpdateEntityTool);
    registry.register(entity_tools::DeleteEntityTool);

    // File I/O.
    registry.register(file_tools::ReadFileTool);
    registry.register(file_tools::ListDirectoryTool);
    registry.register(file_tools::WriteFileTool);

    // Shell (approval-gated — runs arbitrary bash). Enables Claude
    // to orchestrate external HTTP APIs via curl, inspect git state,
    // call Blender headless, etc. The Roblox Cube3D generate-object
    // flow runs through this tool (see its description for the
    // exact curl recipe).
    registry.register(shell_tools::RunBashTool);

    // Scripting.
    registry.register(script_tools::ExecuteRuneTool);
    registry.register(script_tools::ExecuteLuauTool);
    registry.register(script_tools::ImageToCodeTool);
    // Scene-geometry reconstruction via VIGA (Vision-as-Inverse-Graphics).
    // Distinct from ImageToCodeTool — that returns a Rune script; this
    // spawns 3D entities.
    registry.register(script_tools::ImageToGeometryTool);
    registry.register(script_tools::DocumentToCodeTool);
    registry.register(script_tools::GenerateDocsTool);

    // Persistent memory + rule / workflow introspection.
    registry.register(memory_tools::RememberTool);
    registry.register(memory_tools::RecallTool);
    registry.register(memory_tools::ListRulesTool);
    registry.register(memory_tools::ListWorkflowsTool);
    registry.register(memory_tools::QueryStreamEventsTool);

    // Staged file-change proposals.
    registry.register(diff_tools::StageFileChangeTool);

    // Git state.
    registry.register(git_tools::GitStatusTool);
    registry.register(git_tools::GitCommitTool);
    registry.register(git_tools::GitLogTool);
    registry.register(git_tools::GitDiffTool);
    registry.register(git_tools::GitBranchTool);
    registry.register(git_tools::FeedbackDiffTool);

    // Simulation bridge (shared with Rune/Luau scripting API).
    registry.register(simulation_tools::GetSimValueTool);
    registry.register(simulation_tools::SetSimValueTool);
    registry.register(simulation_tools::ListSimValuesTool);
    registry.register(simulation_tools::GetTaggedEntitiesTool);
    registry.register(simulation_tools::RaycastTool);
    registry.register(simulation_tools::HttpRequestTool);
    registry.register(simulation_tools::DataStoreGetTool);
    registry.register(simulation_tools::DataStoreSetTool);
    registry.register(simulation_tools::AddTagTool);
    registry.register(simulation_tools::RemoveTagTool);

    // Simulation control — proactive Workshop can start/stop/inspect sims.
    registry.register(simulation_tools::RunSimulationTool);
    registry.register(simulation_tools::StopSimulationTool);
    registry.register(simulation_tools::GetSimulationStateTool);

    // Telemetry — tail live watchpoint streams for feedback loops.
    registry.register(simulation_tools::TailTelemetryTool);

    // Audit log — query the Claude API call audit trail.
    registry.register(simulation_tools::QueryAuditLogTool);

    // Physics + spatial.
    registry.register(physics_tools::QueryMaterialTool);
    registry.register(physics_tools::CalculatePhysicsTool);
    registry.register(spatial_tools::MeasureDistanceTool);
    registry.register(spatial_tools::ListSpaceContentsTool);

    // Universe / Space / Script browsing (historically MCP-only —
    // now available to Workshop too via WorkshopMode::UniverseBrowsing).
    registry.register(universe_tools::ListUniversesTool);
    registry.register(universe_tools::ListSpacesTool);
    registry.register(universe_tools::ListScriptsTool);
    registry.register(universe_tools::ReadScriptTool);
    registry.register(universe_tools::ListAssetsTool);
    registry.register(universe_tools::FindEntityTool);
    registry.register(universe_tools::SearchUniverseTool);
    registry.register(universe_tools::CreateScriptTool);
    registry.register(universe_tools::SetDefaultUniverseTool);
    registry.register(universe_tools::GetConversationTool);

    // Embedvec-backed AI tools — unblock AI Select Similar,
    // Part Swap template suggestion, contextual edit suggestions,
    // and tool-defaults suggestions. All four are thin protocol
    // wrappers over the engine-side `EmbedvecResource`.
    registry.register(embedvec_tools::FindSimilarEntitiesTool);
    registry.register(embedvec_tools::SuggestSwapTemplateTool);
    registry.register(embedvec_tools::SuggestContextualEditsTool);
    registry.register(embedvec_tools::SuggestToolDefaultsTool);
}

/// Convenience constructor — returns a `ToolRegistry` with every
/// shipped tool registered. Equivalent to:
/// ```ignore
/// let mut r = ToolRegistry::default();
/// register_all_tools(&mut r);
/// ```
pub fn default_registry() -> ToolRegistry {
    let mut r = ToolRegistry::default();
    register_all_tools(&mut r);
    r
}
