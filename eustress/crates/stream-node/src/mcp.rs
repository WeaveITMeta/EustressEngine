//! MCP tool wrappers for EustressStream publish/subscribe.
//!
//! Exposes two tools:
//!   - `stream_publish`   — publish a base64-encoded payload to a topic
//!   - `stream_subscribe` — drain up to N recent messages from a topic

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use eustress_stream::EustressStream;

// ─────────────────────────────────────────────────────────────────────────────
// Tool descriptors (JSON Schema)
// ─────────────────────────────────────────────────────────────────────────────

/// MCP tool descriptor for `stream_publish`.
pub fn publish_tool_descriptor() -> Value {
    json!({
        "name": "stream_publish",
        "description": "Publish a message to an EustressStream topic.",
        "inputSchema": {
            "type": "object",
            "required": ["topic", "payload_b64"],
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Topic name to publish to."
                },
                "payload_b64": {
                    "type": "string",
                    "description": "Base64-encoded message payload."
                }
            }
        }
    })
}

/// MCP tool descriptor for `stream_subscribe`.
pub fn subscribe_tool_descriptor() -> Value {
    json!({
        "name": "stream_subscribe",
        "description": "Read recent messages from an EustressStream topic (ring buffer replay).",
        "inputSchema": {
            "type": "object",
            "required": ["topic"],
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Topic name to read from."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of messages to return (default 10).",
                    "default": 10
                },
                "from_offset": {
                    "type": "integer",
                    "description": "Start reading from this offset (default 0).",
                    "default": 0
                }
            }
        }
    })
}

/// MCP tool descriptor for `stream_topics`.
pub fn topics_tool_descriptor() -> Value {
    json!({
        "name": "stream_topics",
        "description": "List all active EustressStream topics with message counts.",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool invocations
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PublishInput {
    topic: String,
    payload_b64: String,
}

/// Handle a `stream_publish` MCP tool call.
pub fn handle_publish(stream: &EustressStream, input: Value) -> Value {
    let input: PublishInput = match serde_json::from_value(input) {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("invalid input: {e}") }),
    };

    let payload = match base64::engine::general_purpose::STANDARD.decode(&input.payload_b64) {
        Ok(b) => b,
        Err(e) => return json!({ "error": format!("base64 decode: {e}") }),
    };

    let offset = stream.producer(&input.topic).send_bytes(bytes::Bytes::from(payload));
    json!({ "offset": offset, "topic": input.topic })
}

#[derive(Deserialize)]
struct SubscribeInput {
    topic: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    from_offset: u64,
}

fn default_limit() -> usize { 10 }

#[derive(Serialize)]
struct MessageEntry {
    offset: u64,
    timestamp: u64,
    payload_b64: String,
}

/// Handle a `stream_subscribe` MCP tool call (returns recent messages synchronously).
pub fn handle_subscribe(stream: &EustressStream, input: Value) -> Value {
    let input: SubscribeInput = match serde_json::from_value(input) {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("invalid input: {e}") }),
    };

    let mut messages: Vec<MessageEntry> = Vec::new();
    stream.replay_ring(&input.topic, input.from_offset, |view| {
        if messages.len() < input.limit {
            messages.push(MessageEntry {
                offset: view.offset,
                timestamp: view.timestamp,
                payload_b64: base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    view.data,
                ),
            });
        }
    });

    json!({
        "topic": input.topic,
        "count": messages.len(),
        "messages": messages,
    })
}

/// Handle a `stream_topics` MCP tool call.
pub fn handle_topics(stream: &EustressStream) -> Value {
    let topics: Vec<Value> = stream.topics()
        .into_iter()
        .map(|name| json!({
            "name": name,
            "head": stream.head(&name),
            "subscribers": stream.subscriber_count(&name),
        }))
        .collect();

    json!({ "topics": topics })
}
