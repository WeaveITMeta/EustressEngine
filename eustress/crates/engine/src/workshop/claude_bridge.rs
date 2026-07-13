//! # Claude Bridge — Workshop ↔ ClaudeClient async communication
//!
//! Routes Workshop chat messages through the BYOK API key via the existing
//! ClaudeClient infrastructure. Uses std::thread::spawn with Arc<Mutex<Option>>
//! polling, matching the build_pipeline.rs pattern.
//!
//! ## Table of Contents
//!
//! 1. WorkshopClaudeTask — shared state for an in-flight Claude request
//! 2. System: dispatch_claude_requests — spawns background threads for pending messages
//! 3. System: poll_claude_responses — polls for completed responses each frame
//!
//! ## Architecture
//!
//! - All AI calls use the BYOK API key from SoulServiceSettings.effective_api_key()
//! - Conversational chat uses Sonnet tier (~$0.01-0.02 per exchange)
//! - Normalization step uses Sonnet tier (~$0.03)
//! - Each background thread creates its own ClaudeClient with a cloned ClaudeConfig
//! - Results are polled each frame and dispatched as ClaudeResponseEvent / ClaudeErrorEvent

use bevy::prelude::*;
use std::sync::{Arc, Mutex};
use eustress_common::soul::ClaudeConfig;
use serde_json::{json, Value};

use super::{
    IdeationPipeline, IdeationState, ClaudeResponseEvent, ClaudeErrorEvent,
    McpCommandStatus, MessageRole, normalizer,
};
use super::tools::ToolRegistry;
use eustress_tools::registry::{LuauCreatedEntity, LuauExecutionResult, LuauExecutor};
use super::modes::ActiveModes;
use crate::soul::claude_client::{AgenticResponse, ClaudeClient, ClaudeTool};
use crate::soul::{Provider, WorkshopModel, XaiClient, XaiConfig};

// ============================================================================
// 1. WorkshopClaudeTask — in-flight request state
// ============================================================================

/// A single in-flight Claude API request
#[derive(Debug)]
pub(super) struct InFlightRequest {
    /// Shared result container (polled each frame)
    pub(super) result: Arc<Mutex<Option<Result<String, String>>>>,
    /// Which pipeline step this is for (None = conversational chat)
    pub(super) step_index: Option<u32>,
    /// If this was triggered by an MCP command, the message ID
    pub(super) mcp_message_id: Option<u32>,
    /// Whether this is a normalization request (result is TOML, not chat text)
    pub(super) is_normalization: bool,
}

impl InFlightRequest {
    /// Create a new in-flight request
    pub(super) fn new(
        result: Arc<Mutex<Option<Result<String, String>>>>,
        step_index: Option<u32>,
        mcp_message_id: Option<u32>,
        is_normalization: bool,
    ) -> Self {
        Self { result, step_index, mcp_message_id, is_normalization }
    }
}

/// Agentic (tool-use) in-flight request. Holds the parsed `AgenticResponse`
/// once the background thread finishes. Separate from the legacy
/// [`InFlightRequest`] which carries plain text (used by the normalize step).
#[derive(Debug)]
pub(super) struct AgenticInFlight {
    pub(super) result: Arc<Mutex<Option<Result<AgenticResponse, String>>>>,
    /// Which model this turn was dispatched to — used to stamp
    /// `ChatMessage::model_used` and to price the turn's cost correctly
    /// once it resolves.
    pub(super) model: WorkshopModel,
}

/// An in-flight `consult_advisor` call. Resolves into the `tool_result` of
/// the `Mcp` card at `message_id` — text-only (the advisor gets no tools),
/// so no `AgenticResponse`/tool-loop machinery needed here.
#[derive(Debug)]
pub(super) struct AdvisorInFlight {
    pub(super) message_id: u32,
    pub(super) result: Arc<Mutex<Option<Result<String, String>>>>,
}

/// Resource tracking all in-flight Claude requests for the Workshop
#[derive(Resource, Default)]
pub struct WorkshopClaudeTasks {
    /// Legacy text-mode requests (normalize, artifact gen)
    pub(super) in_flight: Vec<InFlightRequest>,
    /// Agentic tool-use requests (the conversational chat loop)
    pub(super) agentic_in_flight: Vec<AgenticInFlight>,
    /// Pending `consult_advisor` calls — resolved asynchronously, same
    /// thread+poll shape as `agentic_in_flight`, tracked separately so
    /// `dispatch_chat_request` can block redispatch while any are pending.
    pub(super) advisor_in_flight: Vec<AdvisorInFlight>,
    /// Whether a conversational response is pending (prevent duplicate sends)
    pub chat_pending: bool,
}

// ============================================================================
// 2. System prompt for conversational ideation
// ============================================================================

/// System prompt for the Workshop conversational AI
const WORKSHOP_SYSTEM_PROMPT: &str = r#"You are the Workshop assistant in Eustress Engine, helping users design and create products through conversation.

Your role:
- Ask clarifying questions about the user's product idea: materials, dimensions, chemistry, form factor, target market, manufacturing process
- Suggest improvements and alternatives based on engineering knowledge
- When you have enough information, tell the user you're ready to normalize their idea into a structured brief
- Be concise and technical — this is an engineering tool, not a chatbot
- Reference specific materials, chemistries, and manufacturing processes when relevant
- Always confirm key specifications before proceeding

Keep responses under 200 words. Ask at most 3-4 clarifying questions at a time.
Do NOT generate TOML or structured data — that happens in a separate normalization step."#;

// ============================================================================
// 3. dispatch_claude_requests — spawn background threads
// ============================================================================

/// Dispatches the Workshop's agentic chat turn:
/// * Builds the Anthropic-format `messages` array from the pipeline history
///   (text, tool_use blocks from prior assistant turns, tool_result blocks
///   from resolved tools).
/// * Assembles the system prompt from the currently active modes' fragments.
/// * Filters the [`ToolRegistry`] to just the tools the active modes expose.
/// * Spawns a background thread that calls `call_with_tools` and delivers
///   the `AgenticResponse` into a shared container.
///
/// Dispatch is suppressed while a previous turn is still in flight OR while
/// the pipeline is waiting on user approval for one or more tool_use cards.
pub fn dispatch_chat_request(
    mut pipeline: ResMut<IdeationPipeline>,
    mut tasks: ResMut<WorkshopClaudeTasks>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
    tool_registry: Option<Res<ToolRegistry>>,
    mention_index: Option<Res<super::mention::MentionIndex>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    display_unit: Option<Res<eustress_common::units::DisplayUnit>>,
) {
    // Only dispatch in the conversing state, only when we're not already
    // waiting, and only when the user has not gated us behind a tool
    // approval that hasn't been resolved yet.
    //
    // `!tasks.advisor_in_flight.is_empty()` blocks redispatch the same way
    // `chat_pending` does — not folded into `ready_to_dispatch` below,
    // because that walk-back only inspects the MOST RECENT Claude-relevant
    // message. If a turn emits `[consult_advisor, run_bash]`, `run_bash`
    // resolves synchronously and becomes the most-recent-with-a-result
    // message while the advisor is still pending, which would otherwise
    // make `ready_to_dispatch` true and redispatch before the advisor's
    // `tool_result` exists — producing a `tool_use` with no matching
    // `tool_result` and an API 400.
    if pipeline.state != IdeationState::Conversing
        || tasks.chat_pending
        || !tasks.advisor_in_flight.is_empty()
        || pipeline.awaiting_tool_approval
    {
        return;
    }

    // A dispatch is valid when the pipeline's most recent Claude-bound
    // message is either:
    //   - a user text message (the "send" case), or
    //   - a resolved tool_result (the "continue after tool execution" case).
    //
    // Critical: we walk backwards but stop at the FIRST message Claude would
    // see — an assistant reply (System role) means Claude already answered
    // this turn, so we MUST NOT redispatch. Previously this code kept
    // scanning past System messages until it found an older User, which
    // caused runaway dispatch: every frame after Claude replied, the guard
    // would re-find the original user message and fire another request.
    // At ~1 dispatch/frame this racked up $50+ in a minute.
    let ready_to_dispatch = pipeline
        .messages
        .iter()
        .rev()
        .find_map(|m| match m.role {
            MessageRole::User => Some(true),
            // A resolved tool_use round-trip is ready; an unresolved one (no
            // tool_result yet) means we're still waiting on approval/exec.
            MessageRole::Mcp => Some(m.tool_result.is_some()),
            // Assistant turn already happened for this user message.
            MessageRole::System => Some(false),
            // UI-only rows — transparent to the dispatch guard.
            MessageRole::Artifact | MessageRole::Error | MessageRole::Approval => None,
        })
        .unwrap_or(false);
    if !ready_to_dispatch { return; }

    // Resolve which model executes this turn, then fetch the matching
    // provider's key. Model choice is a global UI preference (see
    // `GlobalSoulSettings::workshop_model`), not a per-space setting.
    let model = global_settings
        .as_ref()
        .map(|g| g.effective_workshop_model())
        .unwrap_or_default();

    let api_key = match model.provider() {
        Provider::Anthropic => match (&global_settings, &space_settings) {
            (Some(global), Some(space)) => {
                let key = space.effective_api_key(global);
                if key.is_empty() {
                    pipeline.add_error_message(
                        "No API key configured. Open Soul Settings to add your BYOK key.".to_string()
                    );
                    return;
                }
                key
            }
            _ => return,
        },
        Provider::Xai => match global_settings.as_ref().and_then(|g| g.effective_xai_api_key()) {
            Some(key) => key,
            None => {
                pipeline.add_error_message(
                    "No xAI API key configured. Open Soul Settings to add your Grok API key.".to_string()
                );
                return;
            }
        },
    };

    // Assemble Claude-API-shaped messages from the pipeline's chat log.
    let mut messages = build_anthropic_messages(&pipeline);
    if messages.is_empty() {
        return;
    }

    // Resolve @ mentions on the latest user message. Extra content blocks
    // (entity summaries, inlined file bytes, base64 images) ride alongside
    // the original text so the user-visible chat log stays clean while
    // Claude sees full context.
    if let Some(ref index) = mention_index {
        let space = space_root.as_ref().map(|sr| sr.0.as_path());
        let universe = space.and_then(crate::space::universe_root_for_path);
        attach_mention_blocks_to_last_user(
            &mut messages,
            index.as_ref(),
            space,
            universe.as_deref(),
        );
    }

    // Build system prompt: base + active mode fragments + current
    // DisplayUnit hint so the model speaks the user's unit naturally.
    let system_prompt = build_system_prompt(&pipeline.active_modes, display_unit.as_deref());

    // Advertise every registered tool to Claude regardless of which
    // modes are currently active. The mode system still drives system-
    // prompt fragments + badge rendering — but gating the *tool list*
    // by mode meant Claude answered "I have 27 tools" when it in fact
    // had access to 40+, because mode-specific tools were hidden until
    // a keyword triggered their mode. One surface, one complete list.
    // `ToolRegistry` is the shared one from `eustress-tools`; the
    // Claude-shape conversion stays engine-side via
    // `tools::claude_tools_for` because `ClaudeTool` is engine-only.
    let mut tools: Vec<ClaudeTool> = tool_registry
        .as_ref()
        .map(|r| super::tools::claude_tools_for(r, eustress_tools::modes::WorkshopMode::ALL))
        .unwrap_or_default();

    // Bridge-local advisor tool — Sonnet 5 or Grok 4.5 (as executor) can
    // consult Fable 5 on hard architecture/design calls, mirroring the
    // user's own Sonnet<->Fable pairing. Deliberately NOT in the shared
    // `eustress-tools` registry: keeps the out-of-process MCP server's tool
    // surface untouched, and — like an Agent-tool advisor — it's a harness
    // construct, not a registry tool. Omitted when the executor IS Fable 5
    // (consulting itself is degenerate) or no Anthropic key is configured
    // (the advisor always targets Anthropic, regardless of the executor's
    // own provider).
    let advisor_available = model != WorkshopModel::Fable5
        && global_settings.as_ref().zip(space_settings.as_ref())
            .map(|(g, s)| !s.effective_api_key(g).is_empty())
            .unwrap_or(false);
    if advisor_available {
        tools.push(consult_advisor_tool());
    }

    let tool_count = tools.len();
    let message_count = messages.len();

    // Shared result container
    let result_container: Arc<Mutex<Option<Result<AgenticResponse, String>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = result_container.clone();

    let provider = model.provider();

    std::thread::spawn(move || {
        let result = match provider {
            Provider::Anthropic => {
                let config = ClaudeConfig { api_key: Some(api_key), ..ClaudeConfig::default() };
                ClaudeClient::new(config)
                    .call_with_tools(&messages, &tools, Some(&system_prompt), &model)
                    .map_err(|e| e.to_string())
            }
            Provider::Xai => {
                let config = XaiConfig { api_key: Some(api_key) };
                XaiClient::new(config)
                    .call_with_tools(&messages, &tools, Some(&system_prompt), &model)
                    .map_err(|e| e.to_string())
            }
        };

        match result_clone.lock() {
            Ok(mut lock) => *lock = Some(result),
            Err(poisoned) => {
                tracing::error!("Workshop: Mutex poisoned in agentic thread, recovering");
                *poisoned.into_inner() = Some(Err("Internal error: thread lock poisoned".to_string()));
            }
        }
    });

    tasks.agentic_in_flight.push(AgenticInFlight { result: result_container, model });
    tasks.chat_pending = true;

    info!(
        "Workshop: Dispatched agentic {:?} request on {} ({} messages, {} tools, modes: {})",
        provider,
        model.display_name(),
        message_count,
        tool_count,
        pipeline.active_modes.badges_text()
    );
}

/// Bridge-local `consult_advisor` tool definition — see the doc comment at
/// its call site in `dispatch_chat_request` for why this isn't in the
/// shared `eustress-tools` registry.
fn consult_advisor_tool() -> ClaudeTool {
    ClaudeTool {
        name: "consult_advisor".to_string(),
        description: "Consult Fable 5 as an on-demand advisor before committing to a decision \
            that's genuinely hard, high-stakes, or ambiguous — architecture/design calls, tricky \
            bugs with more than one plausible cause, plans with real tradeoffs. Brief it with \
            full self-contained context (it has no memory of this conversation) and a specific \
            question, then weigh its answer against your own judgment. You make the final call \
            and do the work either way — the advisor never executes or edits anything. Skip this \
            for routine work: mechanical edits, straightforward lookups, anything with one \
            obvious right answer.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The specific question to ask the advisor."
                },
                "context": {
                    "type": "string",
                    "description": "Full self-contained background for the advisor — it has no memory of this conversation."
                }
            },
            "required": ["question", "context"]
        }),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Helpers — system prompt + message array assembly
// ─────────────────────────────────────────────────────────────────────────

/// Base system prompt that applies regardless of active mode. Per-mode
/// fragments stack on top via `ActiveModes::system_prompt_fragments`.
const BASE_SYSTEM_PROMPT: &str = r#"You are the Eustress Workshop agent — an AI pair-programmer embedded inside EustressEngine with live access to the user's Universe via MCP tools.

Your behaviour:
- When the user asks for something that requires state changes (entities, files, scripts, simulation), USE TOOLS. Don't describe what you would do; call the tool and let the user approve or see the result.
- When the user asks a question, answer concisely. Text responses stay under 200 words unless explicitly asked for detail.
- When mode-specific knowledge applies, prefix your response with the active-mode badges you see in the Active Mode sections below.
- Never fabricate tool names. Only call tools defined in the tools array.
- If a tool requires approval, the UI will prompt the user; continue your reasoning afterwards once results arrive.

@ References and Path Resolution:
- The user can attach items via `@kind:space/path` tokens. Resolved content blocks (entity summaries, file contents, images) ride alongside the user message automatically.
- When the user references an image with `@file:...` and wants code from it, call `image_to_code`. For textual documents, call `document_to_code`.
- Cross-Space references are valid — any `@kind:` token can point to any Space in the Universe.

PATH RESOLUTION — CRITICAL (avoid wasting tool calls):
@ tokens follow the format `@kind:<SpaceName>/<relative_path>`. Tools use two path roots:

  1. `read_file`, `list_directory`, `write_file`, `run_bash` (cwd):
     Paths are relative to the SANDBOX ROOT. On first use, call `list_directory(path="")` ONCE
     to discover the layout. The root is typically one of:
       a. The Universe root → contains `Spaces/` → each Space is `Spaces/<SpaceName>/...`
       b. The current Space root → contains `Workspace/`, `SoulService/`, etc. directly
     Whichever layout you see, use it consistently for all subsequent calls.

     Converting @ references:
       Layout (a): `@file:<Space>/<path>` → `Spaces/<Space>/<path>`
       Layout (b): `@file:<Space>/<path>` → `<path>` (drop the Space prefix)

     Entity folders contain `_instance.toml`. To read properties:
       `read_file(path="<resolved_entity_folder>/_instance.toml")`
     Script folders contain a source file named after the folder (e.g. `<name>/<name>.rune`).

  2. `list_space_contents`:
     Paths are always relative to the CURRENT SPACE ROOT (no Space prefix, no `Spaces/`).
     `@entity:<Space>/Workspace/Foo/Bar` → `list_space_contents(path="Workspace/Foo")`

  RULES:
  - Call `list_directory(path="")` exactly ONCE if your first path-based tool call errors. Use
    the result to determine layout (a) or (b). Never guess repeatedly.
  - If @ resolved content blocks are already attached to the message, read them directly —
    they contain the file contents. Only call read_file for files NOT already resolved.

Units and Scale:
  Eustress uses METERS as the base unit. 1 unit = 1 meter in all axes.
  - Position [x, y, z]: world coordinates in meters
  - Size/Scale [x, y, z]: dimensions in meters
  - A human is ~1.8m tall. A table is ~0.75m tall. A building floor is ~3m.
  - When the user says "studs" they mean meters (legacy Roblox terminology).
  - Luau scripts: Vector3.new(x, y, z) values are in meters.
  - create_entity: position and size arrays are in meters.

Entity Creation — Hierarchy and Grouping:
  When building multi-part assemblies (products, vehicles, machines), ALWAYS group
  Parts under a Model folder using the `parent` parameter on `create_entity`:

  1. Create the Model container FIRST:
     create_entity(class="Model", name="MyProduct", parent="")
  2. Create each child Part WITH the parent path:
     create_entity(class="Part", name="Housing", parent="MyProduct", ...)
     create_entity(class="Part", name="Anode", parent="MyProduct", ...)

  For nested hierarchies (e.g. versioned products):
     create_entity(class="Model", name="V2", parent="V-Cell")
     create_entity(class="Part", name="Housing", parent="V-Cell/V2", ...)

  NEVER create all Parts at the Workspace root then try to move them — there is no
  move/reparent tool. Always specify `parent` at creation time.

Gradio-Hosted Mesh Generation (via run_bash):
  External Gradio APIs (e.g. Hugging Face Spaces) use a TWO-STEP async pattern.
  You MUST issue separate run_bash calls for each step — never pipe or chain them.

  Step 1 — Submit (fast):
    curl -s --connect-timeout 15 --max-time 30 -X POST "<GRADIO_API_URL>" \
      -H "Content-Type: application/json" -d '{"data":[...]}'
    Use timeout_seconds=60. Returns JSON with an event_id.

  Step 2 — Poll for result (slow — generation can take minutes):
    curl -s --connect-timeout 15 --max-time 480 -N "<GRADIO_API_URL>/<EVENT_ID>"
    Use timeout_seconds=600. Streams SSE events; extract the result URL from the last `data:` line.

  Step 3 — Download the artifact:
    curl -s --connect-timeout 15 --max-time 120 -L -o "<TARGET_PATH>" "<RESULT_URL>"
    Use timeout_seconds=180. Save to the correct sandbox-relative path for the target Space.

  Step 4 — Write any config files (e.g. _instance.toml) referencing the downloaded artifact.

  RULES:
  - Always use --connect-timeout and --max-time on EVERY curl command.
  - Never combine steps into a single piped command — extract intermediate values between steps.
  - Set timeout_seconds generously — complex generation can take 5+ minutes.
"#;

fn build_system_prompt(active_modes: &ActiveModes, display_unit: Option<&eustress_common::units::DisplayUnit>) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str(BASE_SYSTEM_PROMPT);
    out.push_str(&active_modes.system_prompt_fragments());
    // Tell the model which unit the user is currently working in so
    // its conversational answers ("a 5 ft wall is …") use the same
    // unit, AND so that `create_entity`/`update_entity` calls without
    // an explicit `unit` arg default sensibly (the tool's fallback
    // chain also reads ctx.display_unit, but stating it here keeps
    // the model from second-guessing the user's frame of reference).
    if let Some(du) = display_unit {
        out.push_str(&format!(
            "\n\nCurrent display unit: {} (\"{}\"). The user's positions, sizes, \
             distances, and gizmo readouts are all rendered in this unit. When you call \
             `create_entity` or `update_entity` and the user hasn't explicitly named a unit, \
             OMIT the `unit` arg — the tool defaults to the current display unit. Only set \
             `unit` when the user explicitly asks for something in a different unit \
             (e.g. \"make it 2 m wide\" while the display is \"ft\").\n",
            du.0.display_name(),
            du.0.symbol(),
        ));
    }
    out
}

/// Walk the pipeline's chat log and produce an Anthropic-API-shaped
/// messages array. The API requires alternating user/assistant roles, so
/// consecutive same-role entries get merged into one message with multiple
/// content blocks. Tool_use blocks live in assistant content, tool_result
/// blocks live in user content.
///
/// Rules applied:
/// * `User` → text block under role "user".
/// * `System` (prior assistant reply) → text block under role "assistant".
///   System messages without a user preceding them are treated as context.
/// * `Mcp` with `tool_use_id` + `tool_input` → tool_use block under "assistant".
///   If the same message also has `tool_result` → emits a paired tool_result
///   block under the next "user" message.
/// * `Approval`, `Error`, `Artifact` → skipped (UI-only).
/// * System mode-badge messages (role=System, content starts with an emoji
///   mode icon) are also elided so they don't pollute the Claude context.
pub(crate) fn build_anthropic_messages(pipeline: &IdeationPipeline) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut current_role: Option<&'static str> = None;
    let mut buffer: Vec<Value> = Vec::new();

    fn flush(out: &mut Vec<Value>, role: Option<&'static str>, buffer: &mut Vec<Value>) {
        if let Some(role) = role {
            if !buffer.is_empty() {
                out.push(json!({ "role": role, "content": std::mem::take(buffer) }));
            }
        }
    }

    // `target` is always a string literal ("user" / "assistant"), so it lives
    // for `'static`. Match that so `flush` (which needs `'static`) can reuse
    // the role without lifetime laundering.
    fn switch(
        role: &mut Option<&'static str>,
        buffer: &mut Vec<Value>,
        out: &mut Vec<Value>,
        target: &'static str,
    ) {
        if *role != Some(target) {
            flush(out, *role, buffer);
            *role = Some(target);
        }
    }

    for msg in &pipeline.messages {
        match msg.role {
            MessageRole::User => {
                switch(&mut current_role, &mut buffer, &mut out, "user");
                buffer.push(json!({ "type": "text", "text": msg.content.clone() }));
            }
            MessageRole::System => {
                // Skip mode-activation badges — they're UI-only. Heuristic:
                // content starts with a mode badge (emoji icon followed by " ").
                if is_mode_badge(&msg.content) { continue; }
                switch(&mut current_role, &mut buffer, &mut out, "assistant");
                buffer.push(json!({ "type": "text", "text": msg.content.clone() }));
            }
            MessageRole::Mcp => {
                // A tool_use block from a past assistant turn.
                let (Some(tool_id), Some(tool_name)) =
                    (msg.tool_use_id.as_deref(), msg.mcp_endpoint.as_deref())
                else { continue };
                let input = msg.tool_input.clone().unwrap_or_else(|| json!({}));

                switch(&mut current_role, &mut buffer, &mut out, "assistant");
                buffer.push(json!({
                    "type": "tool_use",
                    "id": tool_id,
                    "name": tool_name,
                    "input": input,
                }));

                // If the tool has already executed and we have a result, the
                // next Claude turn must see a tool_result on the user side.
                if let Some(result) = &msg.tool_result {
                    switch(&mut current_role, &mut buffer, &mut out, "user");
                    buffer.push(json!({
                        "type": "tool_result",
                        "tool_use_id": tool_id,
                        "content": result.clone(),
                    }));
                }
            }
            MessageRole::Approval | MessageRole::Artifact | MessageRole::Error => {
                // Don't send UI-only messages to Claude.
            }
        }
    }
    flush(&mut out, current_role, &mut buffer);

    out
}

/// Heuristic: does this content look like a "🏭 Manufacturing — mode activated"
/// badge? Those messages are UI-only and shouldn't round-trip to Claude.
fn is_mode_badge(content: &str) -> bool {
    content.contains("— mode activated")
}

/// Walk the built `messages` array and append resolved mention blocks to
/// the most recent `user` message. Tool_result blocks inside that user
/// message are left untouched — we only append after them.
fn attach_mention_blocks_to_last_user(
    messages: &mut [Value],
    index: &super::mention::MentionIndex,
    space_root: Option<&std::path::Path>,
    universe_root: Option<&std::path::Path>,
) {
    // Find the last "user"-role message. Iterate in reverse so we target
    // the freshest user turn.
    let Some(user_msg) = messages.iter_mut().rev().find(|m| {
        m.get("role").and_then(|r| r.as_str()) == Some("user")
    }) else { return };

    // Content blocks live under `content` as an array. Gather the combined
    // text of every `text` block so we can scan for @refs once.
    let content_arr = match user_msg.get_mut("content").and_then(|c| c.as_array_mut()) {
        Some(c) => c,
        None => return,
    };
    let combined_text: String = content_arr.iter()
        .filter_map(|b| {
            if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                b.get("text").and_then(|t| t.as_str()).map(str::to_string)
            } else { None }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let extra = super::mention_resolver::resolve_mentions_to_blocks(
        &combined_text, index, space_root, universe_root,
    );

    for block in extra {
        content_arr.push(block);
    }
}

/// Spawns a normalization Claude call when an MCP normalize command is approved
pub fn dispatch_normalize_request(
    mut pipeline: ResMut<IdeationPipeline>,
    mut tasks: ResMut<WorkshopClaudeTasks>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
) {
    // Check if there's an approved normalize MCP command
    let normalize_msg = pipeline.messages.iter().find(|m| {
        m.role == super::MessageRole::Mcp
            && m.mcp_endpoint.as_deref() == Some("/mcp/ideation/normalize")
            && m.mcp_status == Some(McpCommandStatus::Approved)
    });
    
    let msg_id = match normalize_msg {
        Some(m) => m.id,
        None => return,
    };
    
    // Mark as running
    pipeline.update_mcp_status(msg_id, McpCommandStatus::Running);
    pipeline.state = IdeationState::Normalizing;
    
    // Get API key
    let api_key = match (&global_settings, &space_settings) {
        (Some(global), Some(space)) => {
            let key = space.effective_api_key(global);
            if key.is_empty() { return; }
            key
        }
        _ => return,
    };
    
    // Build normalization prompt
    let prompt = normalizer::build_normalize_prompt(&pipeline.conversation_context);
    
    // Create shared result container
    let result_container: Arc<Mutex<Option<Result<String, String>>>> = 
        Arc::new(Mutex::new(None));
    let result_clone = result_container.clone();
    
    let config = ClaudeConfig {
        api_key: Some(api_key),
        ..ClaudeConfig::default()
    };
    
    // Spawn background thread with normalization system prompt
    std::thread::spawn(move || {
        let client = crate::soul::ClaudeClient::new(config);
        let result = client.call_api_for_workshop(
            &prompt,
            normalizer::NORMALIZER_SYSTEM_PROMPT,
        );
        
        match result_clone.lock() {
            Ok(mut lock) => *lock = Some(result),
            Err(poisoned) => {
                tracing::error!("Workshop: Mutex poisoned in normalize thread, recovering");
                *poisoned.into_inner() = Some(Err("Internal error: thread lock poisoned".to_string()));
            }
        }
    });

    // Track the in-flight request
    tasks.in_flight.push(InFlightRequest::new(
        result_container,
        Some(0), // Step 0 = normalize
        Some(msg_id),
        true,
    ));
    
    info!("Workshop: Dispatched normalization Claude request");
}

// ============================================================================
// 4. poll_agentic_responses — handle tool-use round trips
// ============================================================================

/// Polls the agentic in-flight requests (chat turns using `call_with_tools`).
/// On completion:
/// * Appends the response's text content as a System message (assistant reply).
/// * For each `tool_use` block:
///     - Creates an `Mcp` message with the tool name + input + use_id.
///     - If the tool is `requires_approval: false`, executes it *immediately*
///       via `ToolRegistry.dispatch()`, stores the result on the message,
///       and clears `awaiting_tool_approval` so `dispatch_chat_request` will
///       continue the loop next frame.
///     - If `requires_approval: true`, sets `awaiting_tool_approval = true`
///       and leaves the card pending — user must approve or skip.
///
/// When the response has no tool_uses, the turn ends naturally — Claude has
/// given its final text answer.
pub fn poll_agentic_responses(
    mut tasks: ResMut<WorkshopClaudeTasks>,
    mut pipeline: ResMut<IdeationPipeline>,
    tool_registry: Option<Res<ToolRegistry>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    auth: Option<Res<crate::auth::AuthState>>,
    display_unit: Option<Res<eustress_common::units::DisplayUnit>>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
) {
    let mut completed_indices = Vec::new();

    // Destructure into disjoint field borrows: the loop below iterates
    // `agentic_in_flight` immutably for its whole duration, while
    // `consult_advisor` handling needs to push onto `advisor_in_flight` —
    // borrowing `&mut tasks` (the whole struct) inside the loop would
    // conflict with the still-live `agentic_in_flight.iter()` borrow.
    let WorkshopClaudeTasks { agentic_in_flight, advisor_in_flight, chat_pending, .. } = &mut *tasks;

    for (i, req) in agentic_in_flight.iter().enumerate() {
        let result = {
            let lock = req.result.lock().ok();
            lock.and_then(|mut g| g.take())
        };
        let Some(result) = result else { continue };
        completed_indices.push(i);
        let model = req.model;

        match result {
            Ok(agentic) => {
                // Estimate cost from token usage at the model that actually
                // answered — Sonnet 5, Fable 5, and Grok 4.5 have distinct
                // per-token pricing (see `WorkshopModel::estimate_cost`).
                let cost = model.estimate_cost(agentic.input_tokens, agentic.output_tokens);
                pipeline.total_cost += cost;

                // 1. Emit the assistant's text reply if any, stamping which
                //    model produced it (provenance + accurate per-message cost).
                if !agentic.text.trim().is_empty() {
                    let msg_id = pipeline.add_system_message(agentic.text.clone(), cost);
                    pipeline.set_message_model(msg_id, model.api_id());
                }

                // 2. Emit each tool_use as an Mcp card. Auto-execute when
                //    the registered definition doesn't require approval.
                let tool_context = build_tool_context(&space_root, &auth, &display_unit);
                let mut any_awaiting_approval = false;

                for tool_use in &agentic.tool_uses {
                    // `consult_advisor` is intercepted here, before the
                    // registry lookup below — it's bridge-local (see
                    // `consult_advisor_tool`), not in `ToolRegistry`, so
                    // falling through to that lookup would hit the "unknown
                    // tool" path and dispatch it synchronously into an
                    // error. Instead: create the card with no `tool_result`,
                    // spawn the Fable 5 call async, and let
                    // `poll_advisor_responses` fill in the result — the
                    // `advisor_in_flight` guard in `dispatch_chat_request`
                    // blocks redispatch until it does.
                    if tool_use.name == "consult_advisor" {
                        let question = tool_use.input.get("question")
                            .and_then(|v| v.as_str()).unwrap_or_default().to_string();
                        let context = tool_use.input.get("context")
                            .and_then(|v| v.as_str()).unwrap_or_default().to_string();

                        let card_content = format!("consult_advisor({})", compact_input_preview(&tool_use.input));
                        let msg_id = pipeline.add_mcp_command(
                            card_content,
                            "consult_advisor".to_string(),
                            "tool_use".to_string(),
                            0.0,
                        );
                        if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == msg_id) {
                            msg.tool_use_id = Some(tool_use.id.clone());
                            msg.tool_input = Some(tool_use.input.clone());
                            msg.is_assistant_turn = true;
                        }
                        pipeline.update_mcp_status(msg_id, McpCommandStatus::Running);

                        let advisor_key = global_settings.as_ref().zip(space_settings.as_ref())
                            .map(|(g, s)| s.effective_api_key(g))
                            .filter(|k| !k.is_empty());
                        match advisor_key {
                            Some(api_key) => spawn_advisor_call(advisor_in_flight, msg_id, api_key, question, context),
                            None => {
                                // Shouldn't normally happen — the tool is omitted
                                // from `tools` when no key is configured — but
                                // never leave the turn wedged if it does.
                                if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == msg_id) {
                                    msg.tool_result = Some("Advisor unavailable: no Anthropic API key configured.".to_string());
                                }
                                pipeline.update_mcp_status(msg_id, McpCommandStatus::Error);
                            }
                        }
                        continue;
                    }

                    // Look up the tool's approval requirement against the
                    // FULL catalogue (`all_tools`), not the active-mode
                    // subset. Claude is given every tool up-front
                    // (`claude_tools_for(..., WorkshopMode::ALL)`), so the
                    // filter used for approval has to match — otherwise a
                    // cross-mode call (e.g. Claude picks `run_scenario`
                    // while the user is in General mode) misses the
                    // lookup, falls through to `requires_approval=true`,
                    // and hangs the pipeline forever because
                    // `awaiting_tool_approval` never clears. Unknown
                    // tools (truly unregistered) drop to `false` now so
                    // dispatch runs and returns an "Unknown tool" error
                    // Claude can recover from, rather than hanging.
                    let requires_approval = tool_registry.as_ref()
                        .and_then(|r| r.all_tools()
                            .into_iter()
                            .find(|d| d.name == tool_use.name))
                        .map(|d| d.requires_approval)
                        .unwrap_or(false);

                    // Create the Mcp message card.
                    let card_content = format!("{}({})", tool_use.name,
                        compact_input_preview(&tool_use.input));
                    let msg_id = pipeline.add_mcp_command(
                        card_content,
                        tool_use.name.clone(),
                        "tool_use".to_string(),
                        0.0,
                    );
                    // Stash the tool_use metadata on the card for later reconstruction.
                    if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == msg_id) {
                        msg.tool_use_id = Some(tool_use.id.clone());
                        msg.tool_input = Some(tool_use.input.clone());
                        msg.is_assistant_turn = true;
                    }

                    if !requires_approval {
                        // Execute immediately and attach result.
                        if let (Some(registry), Some(ref ctx)) = (&tool_registry, &tool_context) {
                            let result = registry.dispatch(
                                &tool_use.name,
                                &tool_use.id,
                                tool_use.input.clone(),
                                ctx,
                            );
                            let status = if result.success {
                                McpCommandStatus::Done
                            } else {
                                McpCommandStatus::Error
                            };
                            pipeline.update_mcp_status(msg_id, status);
                            if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == msg_id) {
                                msg.tool_result = Some(result.content.clone());
                            }
                            info!("Workshop: auto-executed '{}' → success={}", tool_use.name, result.success);
                        } else {
                            // Tool context missing — can't execute, mark as needing approval so
                            // something shows in the UI instead of silently failing.
                            if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == msg_id) {
                                msg.tool_result = Some(
                                    "Workshop: no SpaceRoot resource — tool cannot execute.".to_string()
                                );
                            }
                            pipeline.update_mcp_status(msg_id, McpCommandStatus::Error);
                        }
                    } else {
                        any_awaiting_approval = true;
                    }
                }

                pipeline.awaiting_tool_approval = any_awaiting_approval;
            }
            Err(err) => {
                pipeline.add_error_message(format!("Workshop: Claude error — {}", err));
                pipeline.awaiting_tool_approval = false;
            }
        }
    }

    for i in completed_indices.into_iter().rev() {
        agentic_in_flight.remove(i);
    }
    if agentic_in_flight.is_empty() {
        *chat_pending = false;
    }
}

// ============================================================================
// 4b. Advisor — async `consult_advisor` dispatch + poll
// ============================================================================

/// System prompt for the Fable 5 advisor call. Deliberately distinct from
/// Workshop's own executor prompt (`BASE_SYSTEM_PROMPT`) — the advisor
/// answers one briefed question, it isn't a Workshop agent itself, and it
/// gets no tools.
const ADVISOR_SYSTEM_PROMPT: &str = "You are an advisor consulted by another AI (the \"executor\") \
    working inside Eustress Engine's Workshop panel. The executor has briefed you with full \
    context and a specific question below. Give your best judgment and reasoning — the executor \
    weighs your answer against its own and makes the final call regardless; you have no tools and \
    cannot take any action yourself. Be direct and concrete.";

/// Spawn the background thread for one `consult_advisor` call (always
/// Fable 5, always text-only) and register it so `poll_advisor_responses`
/// picks up the result. Same `std::thread::spawn` + `Arc<Mutex<Option<...>>>`
/// shape as `dispatch_chat_request`'s own thread — an LLM call is exactly
/// the kind of slow operation that shape exists for.
fn spawn_advisor_call(
    advisor_in_flight: &mut Vec<AdvisorInFlight>,
    message_id: u32,
    api_key: String,
    question: String,
    context: String,
) {
    let result_container: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let result_clone = result_container.clone();

    std::thread::spawn(move || {
        let config = ClaudeConfig { api_key: Some(api_key), ..ClaudeConfig::default() };
        let messages = vec![json!({
            "role": "user",
            "content": [{
                "type": "text",
                "text": format!("Context:\n{}\n\nQuestion:\n{}", context, question),
            }]
        })];
        // No tools for the advisor — it's advisory only, never executes.
        let result = ClaudeClient::new(config)
            .call_with_tools(&messages, &[], Some(ADVISOR_SYSTEM_PROMPT), &WorkshopModel::Fable5)
            .map(|resp| resp.text)
            .map_err(|e| e.to_string());

        match result_clone.lock() {
            Ok(mut lock) => *lock = Some(result),
            Err(poisoned) => {
                tracing::error!("Workshop: Mutex poisoned in advisor thread, recovering");
                *poisoned.into_inner() = Some(Err("Internal error: thread lock poisoned".to_string()));
            }
        }
    });

    advisor_in_flight.push(AdvisorInFlight { message_id, result: result_container });
}

/// Polls in-flight `consult_advisor` calls. On completion, writes the
/// advisor's text — or a graceful error string on failure/timeout — into
/// the `tool_result` of the Mcp card at `message_id`. Never leaves a card
/// unresolved: since `dispatch_chat_request` now blocks redispatch while
/// `advisor_in_flight` is non-empty, an unresolved card would wedge the
/// turn forever rather than just degrading gracefully.
pub fn poll_advisor_responses(
    mut tasks: ResMut<WorkshopClaudeTasks>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    let mut completed_indices = Vec::new();

    for (i, req) in tasks.advisor_in_flight.iter().enumerate() {
        let result = {
            let lock = req.result.lock().ok();
            lock.and_then(|mut g| g.take())
        };
        let Some(result) = result else { continue };
        completed_indices.push(i);

        let (advice, status) = match result {
            Ok(text) if !text.trim().is_empty() => (text, McpCommandStatus::Done),
            Ok(_) => ("Advisor returned an empty response.".to_string(), McpCommandStatus::Done),
            Err(err) => (format!("Advisor unavailable: {}", err), McpCommandStatus::Error),
        };

        if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == req.message_id) {
            msg.tool_result = Some(advice);
        }
        pipeline.update_mcp_status(req.message_id, status);
    }

    for i in completed_indices.into_iter().rev() {
        tasks.advisor_in_flight.remove(i);
    }
}

/// Build a `ToolContext` from Bevy resources. Returns `None` when Space isn't
/// loaded (early startup / between spaces).
fn build_tool_context(
    space_root: &Option<Res<crate::space::SpaceRoot>>,
    auth: &Option<Res<crate::auth::AuthState>>,
    display_unit: &Option<Res<eustress_common::units::DisplayUnit>>,
) -> Option<super::tools::ToolContext> {
    let sr = space_root.as_ref()?;
    let universe_root = crate::space::universe_root_for_path(&sr.0)
        .unwrap_or_else(|| sr.0.clone());
    let (user_id, username) = auth.as_ref()
        .and_then(|a| a.user.as_ref())
        .map(|u| (Some(u.id.clone()), Some(u.username.clone())))
        .unwrap_or((None, None));
    let display_unit_sym = display_unit.as_ref()
        .map(|d| d.0.symbol().to_string());
    // Build a Luau executor that creates a fresh sandboxed runtime per
    // invocation. LuauRuntime is !Send so we can't share one across
    // threads — creating a fresh VM per script is ~1 ms and avoids
    // cross-thread issues entirely.
    let luau_executor: Option<LuauExecutor> = Some(Arc::new(
        |source: &str, chunk_name: &str| -> LuauExecutionResult {
            let mut runtime = match eustress_common::luau::runtime::LuauRuntime::new() {
                Ok(r) => r,
                Err(e) => return LuauExecutionResult {
                    success: false,
                    message: format!("Failed to create Luau runtime: {}", e),
                    created_entities: Vec::new(),
                },
            };
            if let Err(e) = runtime.execute_chunk(source, chunk_name) {
                return LuauExecutionResult {
                    success: false,
                    message: e,
                    created_entities: Vec::new(),
                };
            }
            let instances = runtime.drain_created_instances();
            let entities: Vec<LuauCreatedEntity> = instances.into_iter().map(|i| {
                LuauCreatedEntity {
                    class_name: i.class_name,
                    name: i.name,
                    position: i.position,
                    rotation: i.rotation,
                    size: i.size,
                    color: i.color,
                    material: i.material,
                    shape: i.shape,
                    transparency: i.transparency,
                    anchored: i.anchored,
                    can_collide: i.can_collide,
                    units_offset: i.units_offset,
                    ui_size: i.ui_size,
                    adornee_name: i.adornee_name,
                    always_on_top: i.always_on_top,
                    max_distance: i.max_distance,
                    text: i.text,
                    text_color: i.text_color,
                    text_size: i.text_size,
                    font: i.font,
                    luau_entity_id: i.luau_entity_id,
                    parent_entity_id: i.parent_entity_id,
                }
            }).collect();
            let count = entities.len();
            LuauExecutionResult {
                success: true,
                message: format!("OK — {} instances created", count),
                created_entities: entities,
            }
        }
    ));

    Some(super::tools::ToolContext {
        space_root: sr.0.clone(),
        universe_root,
        user_id,
        username,
        luau_executor,
        display_unit: display_unit_sym,
    })
}

/// Produce a compact one-line preview of a tool's JSON input for the card
/// content. Falls back to the raw JSON if compaction fails.
fn compact_input_preview(input: &Value) -> String {
    let s = input.to_string();
    if s.len() <= 140 { s } else { format!("{}…", &s[..140.min(s.len())]) }
}

// ============================================================================
// 5. poll_claude_responses — legacy text-mode poller (normalize/artifact flows)
// ============================================================================

/// Polls all in-flight Claude requests and fires events for completed ones
pub fn poll_claude_responses(
    mut tasks: ResMut<WorkshopClaudeTasks>,
    mut response_events: MessageWriter<ClaudeResponseEvent>,
    mut error_events: MessageWriter<ClaudeErrorEvent>,
) {
    let mut completed_indices = Vec::new();
    
    for (i, request) in tasks.in_flight.iter().enumerate() {
        let result = {
            let lock = request.result.lock().ok();
            lock.and_then(|mut guard| guard.take())
        };
        
        if let Some(result) = result {
            match result {
                Ok(content) => {
                    // Estimate cost: ~$0.01-0.02 per conversational exchange,
                    // ~$0.03 for normalization
                    let cost = if request.is_normalization { 0.03 } else { 0.015 };
                    
                    response_events.write(ClaudeResponseEvent {
                        content,
                        cost,
                        step_index: request.step_index,
                        mcp_message_id: request.mcp_message_id,
                    });
                }
                Err(error) => {
                    error_events.write(ClaudeErrorEvent {
                        error,
                        step_index: request.step_index,
                        mcp_message_id: request.mcp_message_id,
                    });
                }
            }
            completed_indices.push(i);
        }
    }
    
    // Remove completed requests (reverse order to preserve indices)
    for i in completed_indices.into_iter().rev() {
        let was_chat = !tasks.in_flight[i].is_normalization 
            && tasks.in_flight[i].step_index.is_none();
        tasks.in_flight.remove(i);
        if was_chat {
            tasks.chat_pending = false;
        }
    }
}
