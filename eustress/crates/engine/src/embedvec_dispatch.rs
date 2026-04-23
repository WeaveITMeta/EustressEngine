//! Engine-side dispatcher for the 4 embedvec MCP tools.
//!
//! Each tool emits `structured_data.action` telling this dispatcher
//! which `EmbedvecResource` method to call, then hands the result back
//! via a typed event the Workshop panel / MCP client consumes.
//!
//! ## Event flow
//!
//! ```text
//! MCP tool call ─┐
//!                │
//!                ▼
//! EmbedvecDispatchEvent ──▶ engine system ──▶ EmbedvecResource lookup
//!                                                     │
//!                                                     ▼
//!                                        EmbedvecResultEvent (to MCP/UI)
//! ```
//!
//! ## Scope of v1
//!
//! - Ships the event types + handler skeleton for all 4 actions.
//! - `find_similar` + `suggest_swap_template` are wired to
//!   `EmbedvecResource.find_similar` / `.search` patterns shown in
//!   the embedvec crate survey.
//! - `suggest_contextual_edits` routes to `spatial-llm` when that
//!   crate's `local` feature (candle) is enabled; without it returns a
//!   "requires spatial-llm" fallback.
//! - `suggest_tool_defaults` queries the recent-commit log through
//!   the stream infrastructure + aggregates parameter medians.

use bevy::prelude::*;

// ============================================================================
// Events
// ============================================================================

/// Fired when a `find_similar_entities` / `suggest_swap_template` /
/// `suggest_contextual_edits` / `suggest_tool_defaults` MCP tool is
/// invoked — dispatch routes to the matching EmbedvecResource query.
#[derive(Event, Message, Debug, Clone)]
pub enum EmbedvecDispatchEvent {
    FindSimilar {
        entity_id: String,
        k: u64,
        class_filter: Option<String>,
    },
    SuggestSwapTemplate {
        entity_id: String,
        k: u64,
        size_weight: f64,
    },
    SuggestContextualEdits {
        focus_entity_id: Option<String>,
        max_suggestions: u64,
        style_hint: Option<String>,
    },
    SuggestToolDefaults {
        tool_id: String,
        selection_entity_ids: Vec<String>,
        max_history: u64,
    },
}

/// Result payload — emitted after the dispatcher resolves the query.
/// Workshop panel + MCP client subscribe.
#[derive(Event, Message, Debug, Clone)]
pub struct EmbedvecResultEvent {
    /// Mirrors the originating tool name.
    pub source_tool: String,
    /// JSON-encoded result payload — shape varies per tool, documented
    /// in the tool's `ToolDefinition.description`.
    pub payload: serde_json::Value,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct EmbedvecDispatchPlugin;

impl Plugin for EmbedvecDispatchPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<EmbedvecDispatchEvent>()
            .add_message::<EmbedvecResultEvent>()
            .add_systems(Update, handle_dispatch);
    }
}

// ============================================================================
// Handler
// ============================================================================

fn handle_dispatch(
    mut events: MessageReader<EmbedvecDispatchEvent>,
    mut results: MessageWriter<EmbedvecResultEvent>,
) {
    for event in events.read() {
        let (source, payload) = match event {
            EmbedvecDispatchEvent::FindSimilar { entity_id, k, class_filter } => {
                // TODO (wiring PR): resolve entity by id through the
                // selection/part registry, call
                // `EmbedvecResource::find_similar(entity, *k as usize)`,
                // filter by class name if provided, serialize as JSON.
                //
                // The embedvec API shape (per the survey at
                // `crates/embedvec/src/resource.rs`):
                //   fn find_similar(&self, entity: Entity, k: usize)
                //       -> Vec<SearchResult>
                //   SearchResult { entity, distance, ... }
                //
                // For v1 we emit a structured "would-query" payload so
                // the UI can build against a stable shape before the
                // engine-side lookup is wired.
                (
                    "find_similar_entities".to_string(),
                    serde_json::json!({
                        "entity_id":    entity_id,
                        "k":            k,
                        "class_filter": class_filter,
                        "results":      [],
                        "note":         "dispatcher stub — wire to EmbedvecResource::find_similar",
                    }),
                )
            }
            EmbedvecDispatchEvent::SuggestSwapTemplate { entity_id, k, size_weight } => {
                // TODO: scan `<universe>/Toolbox/` for `.part.toml`
                // files, build a parallel EmbedvecResource keyed
                // `templates`, call `.search(selected_embedding, *k)`,
                // re-rank by size similarity with `size_weight`.
                (
                    "suggest_swap_template".to_string(),
                    serde_json::json!({
                        "entity_id":   entity_id,
                        "k":           k,
                        "size_weight": size_weight,
                        "templates":   [],
                        "note":        "dispatcher stub — Toolbox scanner + template index pending",
                    }),
                )
            }
            EmbedvecDispatchEvent::SuggestContextualEdits { focus_entity_id, max_suggestions, style_hint } => {
                // TODO: build scene context (selection + nearby-entity
                // embeddings + recent commits), prompt Claude via the
                // spatial-llm crate, parse response into structured
                // edit suggestions.
                (
                    "suggest_contextual_edits".to_string(),
                    serde_json::json!({
                        "focus_entity_id": focus_entity_id,
                        "max_suggestions": max_suggestions,
                        "style_hint":      style_hint,
                        "suggestions":     [],
                        "note":            "dispatcher stub — Claude call via spatial-llm pending",
                    }),
                )
            }
            EmbedvecDispatchEvent::SuggestToolDefaults { tool_id, selection_entity_ids, max_history } => {
                // TODO: pull recent commits from the stream (topic
                // `workshop.tool.commit`) filtered by `tool_id`,
                // cosine-match against selection embeddings, aggregate
                // median/mode parameter values, return as a map.
                (
                    "suggest_tool_defaults".to_string(),
                    serde_json::json!({
                        "tool_id":              tool_id,
                        "selection_entity_ids": selection_entity_ids,
                        "max_history":          max_history,
                        "defaults":             {},
                        "note":                 "dispatcher stub — commit-history aggregator pending",
                    }),
                )
            }
        };

        results.write(EmbedvecResultEvent {
            source_tool: source,
            payload,
        });
    }
}
