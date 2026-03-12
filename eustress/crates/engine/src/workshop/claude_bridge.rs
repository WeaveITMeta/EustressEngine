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

use super::{
    IdeationPipeline, IdeationState, ClaudeResponseEvent, ClaudeErrorEvent,
    McpCommandStatus, normalizer,
};

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

/// Resource tracking all in-flight Claude requests for the Workshop
#[derive(Resource, Default)]
pub struct WorkshopClaudeTasks {
    /// Active requests being polled
    pub(super) in_flight: Vec<InFlightRequest>,
    /// Whether a conversational response is pending (prevent duplicate sends)
    pub chat_pending: bool,
}

// ============================================================================
// 2. System prompt for conversational ideation
// ============================================================================

/// System prompt for the Workshop conversational AI
const WORKSHOP_SYSTEM_PROMPT: &str = r#"You are the Workshop assistant in Eustress Engine, helping users design and create products through conversation.

Your role:
- Ask clarifying questions about the user's product idea (materials, dimensions, chemistry, form factor, etc.)
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

/// Spawns a conversational Claude call when the user sends a message
/// and the pipeline is in Conversing state
pub fn dispatch_chat_request(
    mut pipeline: ResMut<IdeationPipeline>,
    mut tasks: ResMut<WorkshopClaudeTasks>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
) {
    // Only dispatch if we're conversing and not already waiting for a response
    if pipeline.state != IdeationState::Conversing || tasks.chat_pending {
        return;
    }
    
    // Check if the last message was from the user (needs a response)
    let last_is_user = pipeline.messages.last()
        .map(|m| m.role == super::MessageRole::User)
        .unwrap_or(false);
    
    if !last_is_user {
        return;
    }
    
    // Get API key
    let api_key = match (&global_settings, &space_settings) {
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
    };
    
    // Build conversation prompt from accumulated context
    let prompt = pipeline.conversation_context.clone();
    let prompt_len = prompt.len();
    
    // Create shared result container
    let result_container: Arc<Mutex<Option<Result<String, String>>>> = 
        Arc::new(Mutex::new(None));
    let result_clone = result_container.clone();
    
    // Build ClaudeConfig for the background thread
    let config = ClaudeConfig {
        api_key: Some(api_key),
        ..ClaudeConfig::default()
    };
    
    // Spawn background thread
    std::thread::spawn(move || {
        let client = crate::soul::ClaudeClient::new(config);
        let result = client.call_api_for_workshop(&prompt, WORKSHOP_SYSTEM_PROMPT);
        
        if let Ok(mut lock) = result_clone.lock() {
            *lock = Some(result);
        }
    });
    
    // Track the in-flight request
    tasks.in_flight.push(InFlightRequest::new(
        result_container,
        None,
        None,
        false,
    ));
    tasks.chat_pending = true;
    
    info!("Workshop: Dispatched conversational Claude request ({} chars context)", prompt_len);
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
        
        if let Ok(mut lock) = result_clone.lock() {
            *lock = Some(result);
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
// 4. poll_claude_responses — check for completed requests each frame
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
