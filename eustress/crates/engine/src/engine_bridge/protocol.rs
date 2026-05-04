//! JSON-RPC 2.0 protocol types + per-method handler dispatch.
//!
//! The wire format is plain JSON-RPC over newline-delimited JSON on a
//! TCP connection. Requests look like:
//!
//! ```json
//! {"jsonrpc":"2.0","id":1,"method":"sim.read","params":{"keys":["battery.voltage"]}}
//! ```
//!
//! Responses mirror the JSON-RPC spec:
//!
//! ```json
//! {"jsonrpc":"2.0","id":1,"result":{"values":{"battery.voltage":3.72}}}
//! ```
//!
//! Methods are modelled as a [`MethodName`] enum rather than strings
//! so the dispatcher is a single `match` on an enum variant — less
//! chance of typos and the compiler tracks exhaustiveness when new
//! methods are added.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Request / response frames
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct BridgeRequest {
    #[serde(default)]
    #[allow(dead_code)]
    pub jsonrpc: String,
    /// Request id — echoed back in the response. Can be a number, string,
    /// or null per JSON-RPC 2.0.
    pub id: Value,
    /// Method name, parsed into a [`MethodName`] at dispatch time.
    #[serde(deserialize_with = "deserialize_method")]
    pub method: MethodName,
    /// Method-specific parameters.
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
}

impl BridgeResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn error(id: Value, error: BridgeError) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(error) }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl BridgeError {
    pub fn method_not_found(name: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {}", name),
            data: None,
        }
    }

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self { code: -32602, message: msg.into(), data: None }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self { code: -32603, message: msg.into(), data: None }
    }
}

// ---------------------------------------------------------------------------
// Method names
// ---------------------------------------------------------------------------

/// Enumerates every method the bridge exposes. Grow this as new live
/// surfaces land (ecs.raycast, embedvec.search, mention.resolve, …).
///
/// `Unknown(String)` captures unrecognised method names so the
/// dispatcher can return a proper "method not found" error instead of
/// failing to deserialise the request entirely.
#[derive(Debug, Clone)]
pub enum MethodName {
    Ping,
    SimRead,
    EcsQuery,
    /// List every registered Workshop tool — MCP's `tools/list` proxies
    /// to this so external IDEs see the same 52+ tool surface Workshop has.
    ToolsList,
    /// Dispatch a tool by name — MCP's `tools/call` proxies to this, so
    /// external IDEs execute tools in-process inside the engine with
    /// full ECS access rather than re-implementing them out-of-process.
    ToolsCall,
    Unknown(String),
}

fn deserialize_method<'de, D>(de: D) -> Result<MethodName, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(de)?;
    Ok(match s.as_str() {
        "ping" => MethodName::Ping,
        "sim.read" => MethodName::SimRead,
        "ecs.query" => MethodName::EcsQuery,
        "tools.list" => MethodName::ToolsList,
        "tools.call" => MethodName::ToolsCall,
        _ => MethodName::Unknown(s),
    })
}

// ---------------------------------------------------------------------------
// Handlers — Bevy-main-thread execution of each method
// ---------------------------------------------------------------------------

pub mod handlers {
    use super::*;
    use bevy::prelude::*;

    /// Trivial health check — lets siblings verify the bridge is alive
    /// without touching any engine state.
    pub fn ping(req: &BridgeRequest) -> BridgeResponse {
        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({ "pong": true, "engine": "eustress" }),
        )
    }

    /// Read one or more simulation watchpoint values. Mirrors the
    /// `runtime-snapshot.json` payload but served live from the
    /// in-memory resource — no 250 ms staleness window.
    ///
    /// Params: `{ "keys": ["battery.voltage", ...] }` — omit `keys`
    /// to get every watchpoint.
    pub fn sim_read(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        // `SimValuesResource` (HashMap<String, f64>) is the authoritative
        // in-memory store the Rune runtime writes to on each sim tick.
        let Some(sim) = world.get_resource::<crate::simulation::plugin::SimValuesResource>() else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("SimValuesResource not available"),
            );
        };

        let filter: Option<Vec<String>> = req
            .params
            .get("keys")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let values: serde_json::Map<String, Value> = match filter {
            Some(keys) => keys
                .into_iter()
                .filter_map(|k| sim.0.get(&k).map(|v| (k, Value::from(*v))))
                .collect(),
            None => sim
                .0
                .iter()
                .map(|(k, v)| (k.clone(), Value::from(*v)))
                .collect(),
        };

        BridgeResponse::ok(req.id.clone(), Value::Object(values))
    }

    /// Return a shallow summary of entities matching a selector.
    ///
    /// Params:
    /// ```text
    /// {
    ///   "class":    "Part",      // optional — filter by class name
    ///   "offset":   0,           // optional — paginate, default 0
    ///   "limit":    10_000,      // optional — cap per-call, default 10_000, max 1_000_000
    ///   "encoding": "json"       // "json" | "bincode-base64" — default json
    /// }
    /// ```
    ///
    /// Clients that want the whole universe walk pages of `limit`
    /// until `offset + returned_count >= total`. Bincode over base64
    /// is ~5-10x smaller + faster than the JSON variant; use it for
    /// pages > 10k.
    ///
    /// Result:
    /// ```text
    /// {
    ///   "entities": [{ "id": "123v0", "name": "...", "class": "Part" }],
    ///   "offset": 0,
    ///   "total": 5_000_000,        // full matching count before pagination
    ///   "returned": 10_000,
    ///   "has_more": true,
    ///   "encoding": "json" | "bincode-base64"
    /// }
    /// ```
    pub fn ecs_query(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        // ── Parse params ─────────────────────────────────────────
        let class_filter: Option<String> = req
            .params
            .get("class")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let offset = req.params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        // Hard ceiling at 1M to prevent a single call from eating memory.
        // Page if you need more; the old 1k cap is gone.
        let limit = req
            .params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|l| l as usize)
            .unwrap_or(10_000)
            .min(1_000_000);

        let encoding = req
            .params
            .get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("json");

        // ── Collect matching entities in stable entity-id order ──
        //
        // We iterate once to count `total` (so clients know how many
        // pages exist) and also to slice the requested window.  Bevy's
        // query iterator is single-pass, so we materialize ids first.
        #[derive(serde::Serialize, Clone)]
        struct EntitySummary {
            id: String,
            name: String,
            class: Option<String>,
        }

        let mut q = world.query::<(Entity, &Name, Option<&eustress_common::classes::Instance>)>();
        let matches: Vec<EntitySummary> = q
            .iter(world)
            .filter_map(|(entity, name, instance)| {
                let class = instance.map(|i| i.class_name.as_str().to_string());
                if let Some(ref want) = class_filter {
                    if class.as_deref() != Some(want.as_str()) {
                        return None;
                    }
                }
                Some(EntitySummary {
                    id: format!("{}v{}", entity.index(), entity.generation()),
                    name: name.as_str().to_string(),
                    class,
                })
            })
            .collect();

        let total = matches.len();
        let window: Vec<&EntitySummary> = matches
            .iter()
            .skip(offset)
            .take(limit)
            .collect();
        let returned = window.len();
        let has_more = offset + returned < total;

        // ── Encode the window ────────────────────────────────────
        //
        // JSON is default + debuggable. `bincode-base64` is the bulk
        // path — ~6x smaller on localhost, and the base64 round-trip
        // over JSON-RPC keeps us in one transport. A dedicated binary
        // frame would shave the base64 overhead later; not worth it
        // until we profile pagination under real load.
        let entities_value = match encoding {
            "bincode-base64" => {
                let owned: Vec<EntitySummary> =
                    window.iter().map(|e| (*e).clone()).collect();
                // bincode 1.x API — `serialize` returns `Vec<u8>`.
                match bincode::serialize(&owned) {
                    Ok(bytes) => {
                        use base64::Engine as _;
                        Value::String(base64::engine::general_purpose::STANDARD.encode(bytes))
                    }
                    Err(e) => {
                        return BridgeResponse::error(
                            req.id.clone(),
                            BridgeError::internal(format!("bincode encode failed: {}", e)),
                        );
                    }
                }
            }
            _ => {
                // JSON default.
                match serde_json::to_value(&window) {
                    Ok(v) => v,
                    Err(e) => {
                        return BridgeResponse::error(
                            req.id.clone(),
                            BridgeError::internal(format!("json encode failed: {}", e)),
                        );
                    }
                }
            }
        };

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "entities": entities_value,
                "offset":   offset,
                "total":    total,
                "returned": returned,
                "has_more": has_more,
                "encoding": encoding,
            }),
        )
    }

    // ── tools.list / tools.call — proxy ToolRegistry over the bridge ──
    //
    // These two methods collapse the Workshop/MCP split: external IDEs
    // connected via MCP no longer need their own tool implementations,
    // they call `tools/list` + `tools/call` which forward to the
    // engine's in-process `ToolRegistry`. Tool handlers run on the
    // Bevy main thread with full live ECS access, then the result
    // travels back over TCP.

    /// Return every registered tool's public metadata (name,
    /// description, JSON Schema). Sibling processes use this to
    /// advertise tools to their own clients (MCP `tools/list`, Claude
    /// Desktop's tool picker, etc.) without duplicating the schema.
    ///
    /// Result:
    /// ```text
    /// {
    ///   "tools": [
    ///     { "name": "create_entity", "description": "...", "input_schema": {...}, "requires_approval": false },
    ///     ...
    ///   ]
    /// }
    /// ```
    pub fn tools_list(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let Some(registry) =
            world.get_resource::<crate::workshop::tools::ToolRegistry>()
        else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("ToolRegistry not available"),
            );
        };

        // `tools_for_modes` with an empty slice returns every General
        // tool; we want the whole catalogue, so pass every known mode.
        let all_modes: Vec<crate::workshop::modes::WorkshopMode> =
            crate::workshop::modes::WorkshopMode::ALL.iter().copied().collect();

        let defs = registry.tools_for_modes(&all_modes);
        let mut seen = std::collections::HashSet::new();
        let tools: Vec<Value> = defs
            .into_iter()
            .filter(|d| seen.insert(d.name)) // dedupe — same tool may appear in multiple modes
            .map(|d| {
                serde_json::json!({
                    "name": d.name,
                    "description": d.description,
                    "input_schema": d.input_schema,
                    "requires_approval": d.requires_approval,
                })
            })
            .collect();

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({ "tools": tools }),
        )
    }

    /// Dispatch a tool by name. Params mirror the Anthropic tool_use
    /// frame shape so Claude-compatible callers need zero translation:
    ///
    /// ```text
    /// { "name": "create_entity", "input": { ... }, "tool_use_id": "toolu_abc" }
    /// ```
    ///
    /// `tool_use_id` is optional — when absent we synthesize one so
    /// `ToolResult.tool_use_id` is always populated. `ToolContext` is
    /// built from `SpaceRoot` + `AuthState` just like the Workshop
    /// dispatcher, so the tool behaves identically whether it was
    /// invoked in-process by Claude or remotely by an external IDE.
    pub fn tools_call(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let Some(name) = req.params.get("name").and_then(|v| v.as_str()) else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::invalid_params("missing 'name' (tool id)"),
            );
        };
        let input = req.params.get("input").cloned().unwrap_or(Value::Null);
        let use_id = req
            .params
            .get("tool_use_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("bridge_{}", rand_id()));

        // Build a ToolContext from current resources. We can't take
        // `Res` here because we're already holding `&mut World` — go
        // through `world.get_resource` which returns `Option<&T>`.
        let Some(space_root) = world.get_resource::<crate::space::SpaceRoot>() else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("SpaceRoot not loaded — no Space is active"),
            );
        };
        let space_root_path = space_root.0.clone();
        let universe_root = crate::space::universe_root_for_path(&space_root_path)
            .unwrap_or_else(|| space_root_path.clone());

        let (user_id, username) = world
            .get_resource::<crate::auth::AuthState>()
            .and_then(|a| a.user.as_ref().map(|u| (Some(u.id.clone()), Some(u.username.clone()))))
            .unwrap_or((None, None));

        let ctx = crate::workshop::tools::ToolContext {
            space_root: space_root_path,
            universe_root,
            user_id,
            username,
            luau_executor: None,
        };

        let Some(registry) =
            world.get_resource::<crate::workshop::tools::ToolRegistry>()
        else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("ToolRegistry not available"),
            );
        };

        let result = registry.dispatch(name, &use_id, input, &ctx);

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "tool_name":       result.tool_name,
                "tool_use_id":     result.tool_use_id,
                "success":         result.success,
                "content":         result.content,
                "structured_data": result.structured_data,
                "stream_topic":    result.stream_topic,
            }),
        )
    }

    /// Cheap per-request id without pulling in uuid here. The bridge
    /// ID space is private to sibling-initiated calls, so a random
    /// 64-bit int collision is astronomically unlikely.
    fn rand_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{:x}", nanos)
    }
}
