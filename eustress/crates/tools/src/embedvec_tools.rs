//! Embedvec-backed AI tools — four MCP endpoints that surface the
//! existing `eustress-embedvec` infrastructure as user-invocable
//! actions. Each is a thin wrapper; the heavy lifting
//! (`EmbedvecResource`, `SpatialContextEmbedder`, HNSW index) is
//! already in place in the embedvec crate.
//!
//! ## Tools shipped
//!
//! 1. **`find_similar_entities`** — given an entity id + k, return
//!    the k nearest entities by spatial/property cosine similarity.
//!    Unblocks **AI Select Similar** (TOOLSET.md Phase 2).
//!
//! 2. **`suggest_swap_template`** — given selected part(s), return
//!    ranked Toolbox templates whose embeddings best match. Unblocks
//!    **AI template suggest for Part Swap** (TOOLSET.md Phase 1).
//!
//! 3. **`suggest_contextual_edits`** — given a scene + optional
//!    focus entity, return a short list of suggested edits (rotate
//!    X°, reposition, swap material, etc.). Unblocks **AI-suggested
//!    edits in context** (TOOLSET.md Phase 2). This one hands off
//!    to the Claude API through the spatial-llm crate; hash
//!    embeddings alone aren't rich enough for contextual reasoning.
//!
//! 4. **`suggest_tool_defaults`** — given a tool id + selection,
//!    return suggested Options Bar default values based on recent
//!    similar commit patterns. Unblocks **AI-suggested Options Bar
//!    defaults** (TOOLSET_UX.md Phase 2).
//!
//! ## Architecture note
//!
//! These tools emit `structured_data` suitable for downstream
//! consumers (Workshop panel, MCP clients, Rune scripts). The
//! actual EmbedvecResource lookup happens engine-side — the tool
//! here passes through the request shape + expected response
//! shape. In-engine dispatcher (`eustress-engine` side) translates
//! each tool call into a `EmbedvecResource::find_similar(...)` call
//! and attaches the result.
//!
//! This keeps the tools crate Bevy-free — it doesn't import the
//! Bevy/ECS-dependent embedvec types. The tool surface is a
//! protocol.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

// ============================================================================
// 1. find_similar_entities — unblocks AI Select Similar
// ============================================================================

pub struct FindSimilarEntitiesTool;

impl ToolHandler for FindSimilarEntitiesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "find_similar_entities",
            description: "Find the k entities most similar to a reference entity by spatial + property cosine similarity (HNSW index via eustress-embedvec). Use to select all parts resembling a chosen example, to seed Part Swap template suggestions, or to power 'find like this' UX affordances.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": { "type": "string", "description": "Stable entity id (e.g. `'12v3'`)" },
                    "k":         { "type": "integer", "default": 5, "description": "Number of neighbours to return (1..50)" },
                    "class_filter": {
                        "type": "string",
                        "description": "Optional — restrict results to entities of this class name",
                    }
                },
                "required": ["entity_id"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.embedvec.query"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let entity_id = input.get("entity_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if entity_id.is_empty() {
            return err("find_similar_entities", "entity_id required");
        }
        let k = input.get("k").and_then(|v| v.as_u64()).unwrap_or(5).clamp(1, 50);
        let class_filter = input.get("class_filter").and_then(|v| v.as_str()).map(String::from);

        ToolResult {
            tool_name: "find_similar_entities".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!(
                "Querying embedvec for {} nearest neighbours of '{}'{}",
                k, entity_id,
                class_filter.as_ref().map(|c| format!(" (class={})", c)).unwrap_or_default()
            ),
            structured_data: Some(serde_json::json!({
                "action":       "embedvec_find_similar",
                "entity_id":    entity_id,
                "k":            k,
                "class_filter": class_filter,
            })),
            stream_topic: Some("workshop.embedvec.query".to_string()),
        }
    }
}

// ============================================================================
// 2. suggest_swap_template — unblocks AI template suggest for Part Swap
// ============================================================================

pub struct SuggestSwapTemplateTool;

impl ToolHandler for SuggestSwapTemplateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "suggest_swap_template",
            description: "Given a selected part's embedding, rank Toolbox templates by cosine similarity. Returns the top-k closest template paths (relative to the Toolbox root). Integrates with the Part Swap tool's 'AI Suggest' Options Bar toggle.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Selected part's entity id — the query seed"
                    },
                    "k": {
                        "type": "integer",
                        "default": 5,
                        "description": "Number of template candidates to return (1..20)"
                    },
                    "size_weight": {
                        "type": "number",
                        "default": 0.4,
                        "description": "0..1 — how much to favour templates whose bounding box matches the selected part's size"
                    }
                },
                "required": ["entity_id"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.embedvec.template_suggest"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let entity_id = input.get("entity_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if entity_id.is_empty() {
            return err("suggest_swap_template", "entity_id required");
        }
        let k = input.get("k").and_then(|v| v.as_u64()).unwrap_or(5).clamp(1, 20);
        let size_weight = input.get("size_weight").and_then(|v| v.as_f64()).unwrap_or(0.4).clamp(0.0, 1.0);

        ToolResult {
            tool_name: "suggest_swap_template".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!(
                "Ranking Toolbox templates for '{}' (k={}, size_weight={:.2})",
                entity_id, k, size_weight
            ),
            structured_data: Some(serde_json::json!({
                "action":      "embedvec_suggest_template",
                "entity_id":   entity_id,
                "k":           k,
                "size_weight": size_weight,
            })),
            stream_topic: Some("workshop.embedvec.template_suggest".to_string()),
        }
    }
}

// ============================================================================
// 3. suggest_contextual_edits — unblocks AI-suggested edits in context
// ============================================================================

pub struct SuggestContextualEditsTool;

impl ToolHandler for SuggestContextualEditsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "suggest_contextual_edits",
            description: "Given the current scene + optional focus entity, ask the spatial-LLM to propose a small set of edits (rotation nudge, reposition, swap material, etc.) that would improve cohesion. Combines embedvec spatial context with a Claude prompt through the spatial-llm crate. Rate-limited — call sparingly.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "focus_entity_id": {
                        "type": "string",
                        "description": "Optional — entity the suggestions should centre on. If omitted, suggestions address the whole scene."
                    },
                    "max_suggestions": {
                        "type": "integer",
                        "default": 3,
                        "description": "Number of suggestions to return (1..8)"
                    },
                    "style_hint": {
                        "type": "string",
                        "description": "Optional — 'realistic' / 'stylized' / 'low-poly' etc. — nudges the LLM prompt"
                    }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true, // network call — gate behind user approval
            stream_topics: &["workshop.embedvec.suggest_edits"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let focus = input.get("focus_entity_id").and_then(|v| v.as_str()).map(String::from);
        let max = input.get("max_suggestions").and_then(|v| v.as_u64()).unwrap_or(3).clamp(1, 8);
        let style = input.get("style_hint").and_then(|v| v.as_str()).map(String::from);

        ToolResult {
            tool_name: "suggest_contextual_edits".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!(
                "Requesting {} contextual edit suggestions{}{}",
                max,
                focus.as_ref().map(|f| format!(" focused on '{}'", f)).unwrap_or_default(),
                style.as_ref().map(|s| format!(" (style: {})", s)).unwrap_or_default(),
            ),
            structured_data: Some(serde_json::json!({
                "action":         "embedvec_suggest_edits",
                "focus_entity_id": focus,
                "max_suggestions": max,
                "style_hint":     style,
            })),
            stream_topic: Some("workshop.embedvec.suggest_edits".to_string()),
        }
    }
}

// ============================================================================
// 4. suggest_tool_defaults — unblocks AI-suggested Options Bar defaults
// ============================================================================

pub struct SuggestToolDefaultsTool;

impl ToolHandler for SuggestToolDefaultsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "suggest_tool_defaults",
            description: "Given a tool id (e.g. 'gap_fill') + the current selection, query recent similar commit patterns and return suggested Options Bar default values. Powers the 'smart defaults' UX where Gap Fill pre-fills `thickness` from the user's last few similar fills.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tool_id": {
                        "type": "string",
                        "description": "Tool registry id — e.g. 'gap_fill', 'resize_align', 'linear_array'"
                    },
                    "selection_entity_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Current selection — used to find similar historical commits"
                    },
                    "max_history": {
                        "type": "integer",
                        "default": 10,
                        "description": "How many recent similar commits to aggregate (1..50)"
                    }
                },
                "required": ["tool_id"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.embedvec.suggest_defaults"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let tool_id = input.get("tool_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if tool_id.is_empty() {
            return err("suggest_tool_defaults", "tool_id required");
        }
        let selection: Vec<String> = input.get("selection_entity_ids")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let max_history = input.get("max_history").and_then(|v| v.as_u64()).unwrap_or(10).clamp(1, 50);

        ToolResult {
            tool_name: "suggest_tool_defaults".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!(
                "Querying {} historical commits of '{}' against {} selected entities",
                max_history, tool_id, selection.len()
            ),
            structured_data: Some(serde_json::json!({
                "action":               "embedvec_suggest_tool_defaults",
                "tool_id":              tool_id,
                "selection_entity_ids": selection,
                "max_history":          max_history,
            })),
            stream_topic: Some("workshop.embedvec.suggest_defaults".to_string()),
        }
    }
}

// ============================================================================
// helpers
// ============================================================================

fn err(tool: &str, msg: &str) -> ToolResult {
    ToolResult {
        tool_name: tool.to_string(),
        tool_use_id: String::new(),
        success: false,
        content: msg.to_string(),
        structured_data: None,
        stream_topic: None,
    }
}
