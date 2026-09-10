//! # OpenAI API Client
//!
//! HTTP client for OpenAI's chat-completions endpoint, slotting into the same
//! call site as [`super::claude_client::ClaudeClient`] and
//! [`super::xai_client::XaiClient`]: same `ClaudeTool`/`ToolUseBlock`/
//! `AgenticResponse`/`ClaudeError` types, same `ureq`-blocking-call-from-a-
//! spawned-thread convention.
//!
//! ## Why this is thin
//!
//! xAI's API is OpenAI-compatible, so the wire translation this needs already
//! exists and is already unit-tested in [`super::xai_client`]:
//! `anthropic_history_to_openai`, `claude_tools_to_openai` and
//! `parse_openai_response` are shared verbatim rather than copied. What
//! differs between the two vendors is the endpoint and the key, which is all
//! this file adds.
//!
//! Sharing rather than forking matters for a specific reason: those functions
//! encode fiddly asymmetries (Anthropic batches `tool_result` blocks where
//! OpenAI wants one `role:"tool"` message per call; images nest under `source`
//! one side and are `data:` URLs the other). A second copy would drift, and
//! the drift would show up as a silently dropped image or a lost tool result
//! on one provider only.

use serde_json::{json, Value};
use std::time::Duration;

use super::claude_client::{AgenticResponse, ClaudeError, ClaudeTool};
use super::workshop_model::WorkshopModel;
use super::xai_client::{anthropic_history_to_openai, claude_tools_to_openai, parse_openai_response};

const OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";

/// OpenAI API configuration.
#[derive(Debug, Clone, Default)]
pub struct OpenAiConfig {
    pub api_key: Option<String>,
}

/// Minimal OpenAI HTTP client.
pub struct OpenAiClient {
    config: OpenAiConfig,
}

impl OpenAiClient {
    pub fn new(config: OpenAiConfig) -> Self {
        Self { config }
    }

    /// Make a single OpenAI API call with tools. Mirrors
    /// `ClaudeClient::call_with_tools`'s contract: the caller owns the
    /// multi-turn loop (execute tools → send results → call again).
    pub fn call_with_tools(
        &self,
        messages: &[Value],
        tools: &[ClaudeTool],
        system_prompt: Option<&str>,
        model: &WorkshopModel,
    ) -> Result<AgenticResponse, ClaudeError> {
        let api_key = self.config.api_key.as_ref().ok_or(ClaudeError::NoApiKey)?;

        let request = json!({
            "model": model.api_id(),
            // OpenAI renamed this field for its reasoning-capable models and
            // rejects the old `max_tokens` on them. `max_completion_tokens` is
            // accepted across the current lineup, so it is the one to send.
            "max_completion_tokens": model.max_tokens(),
            "messages": anthropic_history_to_openai(messages, system_prompt),
            "tools": claude_tools_to_openai(tools),
        });

        let timeout = Duration::from_secs(model.timeout_secs());
        let response = ureq::post(OPENAI_ENDPOINT)
            .set("Authorization", &format!("Bearer {}", api_key))
            .set("content-type", "application/json")
            .timeout(timeout)
            .send_json(&request);

        match response {
            Ok(resp) => {
                let body: Value = resp
                    .into_json()
                    .map_err(|e| ClaudeError::InvalidResponse(e.to_string()))?;
                parse_openai_response(&body)
            }
            Err(ureq::Error::Status(429, _)) => Err(ClaudeError::RateLimited { retry_after: None }),
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(ClaudeError::ApiError {
                    error_type: format!("HTTP {}", code),
                    message: body,
                })
            }
            Err(ureq::Error::Transport(e)) => {
                if e.to_string().contains("timed out") {
                    Err(ClaudeError::Timeout)
                } else {
                    Err(ClaudeError::NetworkError(e.to_string()))
                }
            }
        }
    }
}
