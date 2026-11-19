//! The six outcome classes and the mechanical rules that assign them.
//!
//! Every invocation lands in exactly one class. There is no seventh class and
//! no "unknown": `classify` is total over every transport result.
//!
//! * `ok`: a success-shaped reply with an observable effect (a changed world
//!   digest for a tool that declares itself mutating) or a non-empty payload
//!   (for a tool that declares itself read-only).
//! * `structured_error`: a well-formed failure: a JSON-RPC error, or a tool
//!   reply flagged as an error.
//! * `silent_noop`: a success-shaped reply with no observable effect.
//! * `panic`: a "panicked at" line appeared in a watched stderr log during the
//!   call, or the connection dropped (the MCP server or the engine went away).
//! * `timeout`: no reply inside the deadline.
//! * `unreachable`: the tool could not be dispatched at all: the route reported
//!   it unknown, the host had no registry or Space, or the reply names a
//!   headless limitation from the fixed marker list below.

use serde_json::Value;

use crate::util::excerpt;

/// The six classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Outcome {
    Ok,
    StructuredError,
    SilentNoop,
    Panic,
    Timeout,
    Unreachable,
}

impl Outcome {
    pub const ALL: [Outcome; 6] = [
        Outcome::Ok,
        Outcome::StructuredError,
        Outcome::SilentNoop,
        Outcome::Panic,
        Outcome::Timeout,
        Outcome::Unreachable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::StructuredError => "structured_error",
            Outcome::SilentNoop => "silent_noop",
            Outcome::Panic => "panic",
            Outcome::Timeout => "timeout",
            Outcome::Unreachable => "unreachable",
        }
    }
}

/// Which surface carried the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `tools/call` on the MCP server over stdio.
    Mcp,
    /// `tools.call` on the Engine Bridge (the Workshop registry).
    EngineBridge,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Mcp => "mcp.stdio.tools/call",
            Route::EngineBridge => "engine_bridge.tools.call",
        }
    }
}

/// What came back over the wire, before any interpretation.
#[derive(Debug, Clone)]
pub enum Transport {
    /// A JSON-RPC `result`.
    Reply(Value),
    /// A JSON-RPC `error`.
    RpcError { code: i64, message: String },
    /// No reply inside the deadline.
    Timeout,
    /// The peer went away (closed stdout, dropped the socket, stopped
    /// listening).
    Dropped(String),
    /// The census could not dispatch the call at all.
    NotDispatched(String),
}

impl Transport {
    pub fn kind(&self) -> &'static str {
        match self {
            Transport::Reply(_) => "reply",
            Transport::RpcError { .. } => "rpc_error",
            Transport::Timeout => "timeout",
            Transport::Dropped(_) => "dropped",
            Transport::NotDispatched(_) => "not_dispatched",
        }
    }
}

/// A tool reply reduced to what both routes share.
#[derive(Debug, Clone, Default)]
pub struct ToolReply {
    /// The reply had the route's result shape.
    pub well_formed: bool,
    pub is_error: bool,
    /// Every text block, joined with newlines.
    pub text: String,
    /// `structuredContent` (MCP), `structured_data` (bridge), or the text
    /// itself when it is one JSON object.
    pub structured: Option<Value>,
    /// The reply carried an image block.
    pub has_image: bool,
}

impl ToolReply {
    /// A non-empty payload: text other than the MCP server's "(no output)"
    /// filler, a non-empty structured value, or an image.
    pub fn payload_nonempty(&self) -> bool {
        let t = self.text.trim();
        let text = !t.is_empty() && t != "(no output)";
        let structured = match &self.structured {
            None | Some(Value::Null) => false,
            Some(Value::Object(m)) => !m.is_empty(),
            Some(Value::Array(a)) => !a.is_empty(),
            Some(Value::String(s)) => !s.trim().is_empty(),
            Some(_) => true,
        };
        text || structured || self.has_image
    }
}

/// Reduce a route's `result` to a [`ToolReply`].
pub fn parse_reply(route: Route, result: &Value) -> ToolReply {
    match route {
        Route::Mcp => {
            let Some(blocks) = result.get("content").and_then(|c| c.as_array()) else {
                return ToolReply::default();
            };
            let mut texts = Vec::new();
            let mut has_image = false;
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                            texts.push(t.to_string());
                        }
                    }
                    Some("image") => has_image = true,
                    _ => {}
                }
            }
            let text = texts.join("\n");
            let structured = result
                .get("structuredContent")
                .cloned()
                .filter(|v| !v.is_null())
                .or_else(|| json_object_in(&text));
            ToolReply {
                well_formed: true,
                is_error: result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false),
                text,
                structured,
                has_image,
            }
        }
        Route::EngineBridge => {
            let Some(success) = result.get("success").and_then(|v| v.as_bool()) else {
                return ToolReply::default();
            };
            let text = result.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let structured = result
                .get("structured_data")
                .cloned()
                .filter(|v| !v.is_null())
                .or_else(|| json_object_in(&text));
            ToolReply {
                well_formed: true,
                is_error: !success,
                text,
                structured,
                has_image: false,
            }
        }
    }
}

fn json_object_in(text: &str) -> Option<Value> {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(v @ Value::Object(_)) => Some(v),
        _ => None,
    }
}

/// How a success-shaped reply proves its effect.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectRule {
    /// Read-only tools: a non-empty payload is the effect.
    Payload,
    /// Mutating tools: the world digest taken before and after must differ.
    Digest,
    /// A curated rule for a tool whose effect is invisible to the digest
    /// (MCP session state): the value at a JSON pointer in the reply must
    /// equal `value`.
    PayloadPointerEquals { pointer: String, value: Value },
}

impl EffectRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            EffectRule::Payload => "payload",
            EffectRule::Digest => "digest",
            EffectRule::PayloadPointerEquals { .. } => "payload_pointer_equals",
        }
    }
}

/// Evidence gathered around the call.
#[derive(Debug, Clone, Default)]
pub struct Evidence {
    /// A new "panicked at" line in a watched log during the call window.
    pub panic_logged: bool,
    /// `Some(changed)` when both digests were taken.
    pub digest_changed: Option<bool>,
}

/// The verdict for one invocation.
#[derive(Debug, Clone)]
pub struct Classified {
    pub outcome: Outcome,
    /// The error excerpt, the reason a call was unreachable, or what the
    /// verdict rests on.
    pub detail: Option<String>,
    pub effect_observed: Option<bool>,
    pub payload_nonempty: bool,
    /// The rule that decided: "panic_log", "transport", "rpc_error",
    /// "tool_error", "payload", "digest", "payload_fallback",
    /// "payload_pointer_equals", "malformed_result".
    pub basis: &'static str,
}

/// Text that marks a call as not dispatchable at all.
pub const UNDISPATCHABLE_MARKERS: &[&str] = &[
    "unknown tool",
    "method not found",
    "toolregistry not available",
    "spaceroot not loaded",
];

/// Text in a tool's error that names a headless limitation. Fixed and
/// published in the artifact; matched case-insensitively.
pub const HEADLESS_MARKERS: &[&str] = &[
    "requires a window",
    "requires the windowed",
    "windowed editor",
    "no window",
    "requires a gpu",
    "needs a gpu",
    "no gpu",
    "no render device",
    "not available in headless",
    "unavailable in headless",
    "not supported in headless",
    "in a headless build",
];

fn matches_any(text: &str, markers: &[&str]) -> bool {
    let lower = text.to_lowercase();
    markers.iter().any(|m| lower.contains(m))
}

const EXCERPT_CHARS: usize = 240;

/// Assign exactly one class.
pub fn classify(route: Route, transport: &Transport, rule: &EffectRule, ev: &Evidence) -> Classified {
    let verdict = |outcome, detail: Option<String>, basis| Classified {
        outcome,
        detail,
        effect_observed: None,
        payload_nonempty: false,
        basis,
    };
    if ev.panic_logged {
        return verdict(
            Outcome::Panic,
            Some("a 'panicked at' line appeared in a watched stderr log during the call".into()),
            "panic_log",
        );
    }
    match transport {
        Transport::Dropped(d) => verdict(Outcome::Panic, Some(excerpt(d, EXCERPT_CHARS)), "transport"),
        Transport::Timeout => verdict(Outcome::Timeout, None, "transport"),
        Transport::NotDispatched(why) => verdict(Outcome::Unreachable, Some(excerpt(why, EXCERPT_CHARS)), "transport"),
        Transport::RpcError { code, message } => {
            let detail = Some(excerpt(&format!("{code}: {message}"), EXCERPT_CHARS));
            if matches_any(message, UNDISPATCHABLE_MARKERS) || matches_any(message, HEADLESS_MARKERS) {
                verdict(Outcome::Unreachable, detail, "rpc_error")
            } else {
                verdict(Outcome::StructuredError, detail, "rpc_error")
            }
        }
        Transport::Reply(result) => {
            let reply = parse_reply(route, result);
            let payload = reply.payload_nonempty();
            if !reply.well_formed {
                return Classified {
                    outcome: Outcome::SilentNoop,
                    detail: Some(format!("success-shaped reply without the {} result shape", route.as_str())),
                    effect_observed: Some(false),
                    payload_nonempty: false,
                    basis: "malformed_result",
                };
            }
            if reply.is_error {
                let text = if reply.text.trim().is_empty() {
                    reply.structured.as_ref().map(|s| s.to_string()).unwrap_or_default()
                } else {
                    reply.text.clone()
                };
                let outcome = if matches_any(&text, UNDISPATCHABLE_MARKERS) || matches_any(&text, HEADLESS_MARKERS) {
                    Outcome::Unreachable
                } else {
                    Outcome::StructuredError
                };
                return Classified {
                    outcome,
                    detail: Some(excerpt(&text, EXCERPT_CHARS)),
                    effect_observed: None,
                    payload_nonempty: payload,
                    basis: "tool_error",
                };
            }
            let (effect, basis) = match rule {
                EffectRule::Payload => (payload, "payload"),
                EffectRule::Digest => match ev.digest_changed {
                    Some(changed) => (changed, "digest"),
                    None => (payload, "payload_fallback"),
                },
                EffectRule::PayloadPointerEquals { pointer, value } => {
                    let got = reply.structured.as_ref().and_then(|s| s.pointer(pointer));
                    (got == Some(value), "payload_pointer_equals")
                }
            };
            Classified {
                outcome: if effect { Outcome::Ok } else { Outcome::SilentNoop },
                detail: if effect { None } else { Some(excerpt(&reply.text, EXCERPT_CHARS)) },
                effect_observed: Some(effect),
                payload_nonempty: payload,
                basis,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mcp_ok(text: &str) -> Transport {
        Transport::Reply(json!({ "content": [{ "type": "text", "text": text }], "isError": false }))
    }

    #[test]
    fn a_mutating_success_without_a_digest_change_is_a_silent_noop() {
        let ev = Evidence { panic_logged: false, digest_changed: Some(false) };
        let c = classify(Route::Mcp, &mcp_ok("Updated WelcomeCube"), &EffectRule::Digest, &ev);
        assert_eq!(c.outcome, Outcome::SilentNoop);
        assert!(c.payload_nonempty, "a chatty reply is still a no-op");
        let ev = Evidence { panic_logged: false, digest_changed: Some(true) };
        assert_eq!(classify(Route::Mcp, &mcp_ok("x"), &EffectRule::Digest, &ev).outcome, Outcome::Ok);
    }

    #[test]
    fn read_only_success_needs_a_payload() {
        let ev = Evidence::default();
        assert_eq!(classify(Route::Mcp, &mcp_ok("(no output)"), &EffectRule::Payload, &ev).outcome, Outcome::SilentNoop);
        assert_eq!(classify(Route::Mcp, &mcp_ok("3 entities"), &EffectRule::Payload, &ev).outcome, Outcome::Ok);
    }

    #[test]
    fn errors_are_structured_unless_undispatchable() {
        let ev = Evidence::default();
        let err = Transport::Reply(json!({ "content": [{ "type": "text", "text": "Missing 'name'" }], "isError": true }));
        assert_eq!(classify(Route::Mcp, &err, &EffectRule::Digest, &ev).outcome, Outcome::StructuredError);
        let unknown = Transport::RpcError { code: -32602, message: "unknown tool: x".into() };
        assert_eq!(classify(Route::Mcp, &unknown, &EffectRule::Digest, &ev).outcome, Outcome::Unreachable);
        let bridge = Transport::Reply(json!({ "success": false, "content": "requires the windowed editor" }));
        assert_eq!(classify(Route::EngineBridge, &bridge, &EffectRule::Payload, &ev).outcome, Outcome::Unreachable);
    }

    #[test]
    fn the_not_running_hint_is_not_a_headless_marker() {
        // The bridge client's message names eustress-headless; it must not
        // read as a headless limitation.
        let ev = Evidence::default();
        let t = Transport::Reply(json!({ "content": [{ "type": "text", "text":
            "Eustress engine is not running. Open the engine or run eustress-headless and retry." }], "isError": true }));
        assert_eq!(classify(Route::Mcp, &t, &EffectRule::Payload, &ev).outcome, Outcome::StructuredError);
    }

    #[test]
    fn panics_and_transport_failures_win() {
        let ev = Evidence { panic_logged: true, digest_changed: Some(true) };
        assert_eq!(classify(Route::Mcp, &mcp_ok("fine"), &EffectRule::Digest, &ev).outcome, Outcome::Panic);
        let ev = Evidence::default();
        assert_eq!(classify(Route::Mcp, &Transport::Timeout, &EffectRule::Digest, &ev).outcome, Outcome::Timeout);
        assert_eq!(classify(Route::Mcp, &Transport::Dropped("eof".into()), &EffectRule::Digest, &ev).outcome, Outcome::Panic);
    }

    #[test]
    fn malformed_results_are_not_errors() {
        let ev = Evidence::default();
        let t = Transport::Reply(json!({ "weird": 1 }));
        assert_eq!(classify(Route::Mcp, &t, &EffectRule::Payload, &ev).outcome, Outcome::SilentNoop);
    }

    #[test]
    fn pointer_rule_reads_json_text() {
        let ev = Evidence::default();
        let rule = EffectRule::PayloadPointerEquals { pointer: "/changed".into(), value: json!(true) };
        assert_eq!(classify(Route::Mcp, &mcp_ok("{\"changed\": true}"), &rule, &ev).outcome, Outcome::Ok);
        assert_eq!(classify(Route::Mcp, &mcp_ok("{\"changed\": false}"), &rule, &ev).outcome, Outcome::SilentNoop);
    }

    #[test]
    fn every_class_has_a_distinct_name() {
        let mut names: Vec<_> = Outcome::ALL.iter().map(|o| o.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 6);
    }
}
