//! # xAI (Grok) API Client
//!
//! HTTP client for xAI's OpenAI-compatible chat-completions endpoint,
//! shaped to slot into the same call site as [`super::claude_client::ClaudeClient`]:
//! same `ClaudeTool`/`ToolUseBlock`/`AgenticResponse`/`ClaudeError` types,
//! same `ureq`-blocking-call-from-a-spawned-thread convention.
//!
//! The wire format genuinely differs from Anthropic's Messages API (OpenAI
//! chat-completions shape, `Authorization: Bearer` auth, `tool_calls` with
//! *string* `arguments`, one `role:"tool"` message per call instead of a
//! batched `tool_result` block) — so the translation lives in two pure,
//! unit-tested functions rather than being folded into `ClaudeClient`.

use serde_json::{json, Value};
use std::time::Duration;

use super::claude_client::{AgenticResponse, ClaudeError, ClaudeTool, ToolUseBlock};
use super::workshop_model::WorkshopModel;

const XAI_ENDPOINT: &str = "https://api.x.ai/v1/chat/completions";

/// xAI API configuration.
#[derive(Debug, Clone, Default)]
pub struct XaiConfig {
    pub api_key: Option<String>,
}

/// Minimal xAI (Grok) HTTP client.
pub struct XaiClient {
    config: XaiConfig,
}

impl XaiClient {
    pub fn new(config: XaiConfig) -> Self {
        Self { config }
    }

    /// Make a single Grok API call with tools. Mirrors
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
            "max_tokens": model.max_tokens(),
            "messages": anthropic_history_to_openai(messages, system_prompt),
            "tools": claude_tools_to_openai(tools),
        });

        let timeout = Duration::from_secs(model.timeout_secs());
        let response = ureq::post(XAI_ENDPOINT)
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

/// `ClaudeTool` (Anthropic `input_schema`) → OpenAI function-tool shape.
fn claude_tools_to_openai(tools: &[ClaudeTool]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })
        })
        .collect()
}

/// Translate an Anthropic-shaped message history (as built by
/// `workshop::claude_bridge::build_anthropic_messages`) into OpenAI
/// chat-completions messages. Pure and unit-tested — no HTTP.
///
/// Shape differences from Anthropic, handled here:
/// - Anthropic content blocks → OpenAI top-level `content` (string) +
///   `tool_calls` (array) on assistant messages, with `arguments`
///   JSON-*stringified* (not a nested object).
/// - Anthropic batches every `tool_result` for a turn into one user message;
///   OpenAI wants one `{role:"tool", tool_call_id, content}` message *per*
///   call, in order, with any leftover plain user text following.
///
/// Known v1 gap: image/document content blocks (from `@file:` mention
/// resolution) aren't translated — only `text` and `tool_result`/`tool_use`
/// blocks are. Grok 4.5 supports vision, but wiring that through is out of
/// scope for the model picker; a mention that resolves to an image is
/// silently dropped from the Grok-bound history rather than erroring.
pub fn anthropic_history_to_openai(messages: &[Value], system_prompt: Option<&str>) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    if let Some(sys) = system_prompt {
        if !sys.is_empty() {
            out.push(json!({ "role": "system", "content": sys }));
        }
    }

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let Some(content) = msg.get("content").and_then(|c| c.as_array()) else {
            continue;
        };

        match role {
            "assistant" => out.push(assistant_message_to_openai(content)),
            "user" => out.extend(user_message_to_openai(content)),
            _ => {}
        }
    }
    out
}

fn assistant_message_to_openai(content: &[Value]) -> Value {
    let mut text = String::new();
    let mut tool_calls = Vec::new();

    for block in content {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                }
            }
            Some("tool_use") => {
                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        // OpenAI wants a JSON *string* here, not a nested object.
                        "arguments": input.to_string(),
                    }
                }));
            }
            _ => {}
        }
    }

    let mut entry = serde_json::Map::new();
    entry.insert("role".into(), json!("assistant"));
    entry.insert(
        "content".into(),
        if text.is_empty() {
            Value::Null
        } else {
            json!(text)
        },
    );
    if !tool_calls.is_empty() {
        entry.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    Value::Object(entry)
}

/// Anthropic batches every `tool_result` for a turn into one user message.
/// OpenAI wants each as its own `role:"tool"` message. Emits the tool
/// messages first (matching the preceding assistant `tool_calls`), then one
/// trailing user message for any leftover plain text — never merges the two.
fn user_message_to_openai(content: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut leftover_text = String::new();

    for block in content {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("tool_result") => {
                let tool_call_id = block
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let result_content = block
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": result_content,
                }));
            }
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    if !leftover_text.is_empty() {
                        leftover_text.push('\n');
                    }
                    leftover_text.push_str(t);
                }
            }
            _ => {}
        }
    }

    if !leftover_text.is_empty() {
        out.push(json!({ "role": "user", "content": leftover_text }));
    }
    out
}

/// Parse an OpenAI-shaped chat-completions response into the same
/// provider-neutral `AgenticResponse` Workshop already uses for Anthropic.
pub fn parse_openai_response(body: &Value) -> Result<AgenticResponse, ClaudeError> {
    let choice = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| ClaudeError::InvalidResponse("Missing choices[0]".to_string()))?;

    let message = choice
        .get("message")
        .ok_or_else(|| ClaudeError::InvalidResponse("Missing choices[0].message".to_string()))?;

    let text = message
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();

    let mut tool_uses = Vec::new();
    if let Some(calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        for call in calls {
            let id = call
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let function = call.get("function");
            let name = function
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            // CRITICAL: `arguments` is a JSON *string* on the wire, not an
            // object — must be re-parsed. Silent-bug magnet if skipped.
            let arguments_str = function
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(arguments_str).map_err(|e| {
                ClaudeError::InvalidResponse(format!(
                    "tool_calls[].function.arguments not valid JSON: {}",
                    e
                ))
            })?;
            tool_uses.push(ToolUseBlock { id, name, input });
        }
    }

    // Light normalization to Anthropic's vocabulary for consistent logs —
    // nothing in poll_agentic_responses branches on the exact string today.
    let stop_reason = match choice.get("finish_reason").and_then(|s| s.as_str()) {
        Some("tool_calls") => "tool_use",
        Some("length") => "max_tokens",
        Some(other) => other,
        None => "end_turn",
    }
    .to_string();

    let usage = body.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    Ok(AgenticResponse {
        text,
        tool_uses,
        stop_reason,
        input_tokens,
        output_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_only_response() {
        let body = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "Hello there." },
                "finish_reason": "stop",
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 4 },
        });
        let parsed = parse_openai_response(&body).expect("should parse");
        assert_eq!(parsed.text, "Hello there.");
        assert!(parsed.tool_uses.is_empty());
        assert_eq!(parsed.stop_reason, "end_turn");
        assert_eq!(parsed.input_tokens, 12);
        assert_eq!(parsed.output_tokens, 4);
    }

    #[test]
    fn parses_single_tool_call_with_stringified_arguments() {
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "inspect_scene",
                            "arguments": "{\"class\":\"Part\",\"limit\":10}",
                        }
                    }]
                },
                "finish_reason": "tool_calls",
            }],
            "usage": { "prompt_tokens": 50, "completion_tokens": 20 },
        });
        let parsed = parse_openai_response(&body).expect("should parse");
        assert_eq!(parsed.text, "");
        assert_eq!(parsed.stop_reason, "tool_use");
        assert_eq!(parsed.tool_uses.len(), 1);
        let call = &parsed.tool_uses[0];
        assert_eq!(call.id, "call_abc123");
        assert_eq!(call.name, "inspect_scene");
        assert_eq!(call.input, json!({"class": "Part", "limit": 10}));
    }

    #[test]
    fn parses_multiple_tool_calls() {
        let body = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [
                        { "id": "call_1", "type": "function", "function": { "name": "a", "arguments": "{}" } },
                        { "id": "call_2", "type": "function", "function": { "name": "b", "arguments": "{\"x\":1}" } },
                    ]
                },
                "finish_reason": "tool_calls",
            }],
            "usage": { "prompt_tokens": 30, "completion_tokens": 15 },
        });
        let parsed = parse_openai_response(&body).expect("should parse");
        assert_eq!(parsed.tool_uses.len(), 2);
        assert_eq!(parsed.tool_uses[0].name, "a");
        assert_eq!(parsed.tool_uses[1].name, "b");
        assert_eq!(parsed.tool_uses[1].input, json!({"x": 1}));
    }

    #[test]
    fn translates_tool_use_and_batched_tool_result_into_per_call_messages() {
        // Anthropic-shaped history: user text, assistant tool_use (two
        // calls), then ONE user message batching both tool_results plus a
        // trailing plain-text follow-up.
        let history = vec![
            json!({
                "role": "user",
                "content": [{ "type": "text", "text": "Create two parts." }]
            }),
            json!({
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "On it." },
                    { "type": "tool_use", "id": "toolu_1", "name": "create_entity", "input": {"name": "A"} },
                    { "type": "tool_use", "id": "toolu_2", "name": "create_entity", "input": {"name": "B"} },
                ]
            }),
            json!({
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "content": "created A" },
                    { "type": "tool_result", "tool_use_id": "toolu_2", "content": "created B" },
                    { "type": "text", "text": "Now also rename them." },
                ]
            }),
        ];

        let openai = anthropic_history_to_openai(&history, Some("system prompt"));

        // system, user, assistant(+tool_calls), tool, tool, user(leftover)
        assert_eq!(openai.len(), 6);
        assert_eq!(openai[0]["role"], "system");
        assert_eq!(openai[1]["role"], "user");
        assert_eq!(openai[1]["content"], "Create two parts.");

        assert_eq!(openai[2]["role"], "assistant");
        assert_eq!(openai[2]["content"], "On it.");
        let calls = openai[2]["tool_calls"]
            .as_array()
            .expect("tool_calls array");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["id"], "toolu_1");
        // arguments must be a JSON STRING, not a nested object.
        assert!(calls[0]["function"]["arguments"].is_string());
        assert_eq!(calls[0]["function"]["arguments"], "{\"name\":\"A\"}");

        // Two tool_results become TWO separate tool-role messages, not one.
        assert_eq!(openai[3]["role"], "tool");
        assert_eq!(openai[3]["tool_call_id"], "toolu_1");
        assert_eq!(openai[3]["content"], "created A");
        assert_eq!(openai[4]["role"], "tool");
        assert_eq!(openai[4]["tool_call_id"], "toolu_2");
        assert_eq!(openai[4]["content"], "created B");

        // Leftover plain text after the tool_results lands in its own,
        // trailing user message.
        assert_eq!(openai[5]["role"], "user");
        assert_eq!(openai[5]["content"], "Now also rename them.");
    }
}
