//! `tools.call`: run one registered tool for a bridge caller, on a worker
//! thread.
//!
//! A tool takes a `ToolContext` (paths, the user, the grant), never the World,
//! so once the context is built it needs nothing from the main thread. Run on
//! the main thread, a tool held the frame for its whole duration, and a tool
//! that calls the engine back over this bridge (the AI camera tools, a wait on
//! a simulation) waited on the very thread that answers it. The call is
//! prepared on the main thread, run on a thread of its own, and answered
//! through the request's responder, as `db.export_toml` is.

use bevy::prelude::*;
use serde_json::Value;

use super::protocol::{BridgeError, BridgeRequest, BridgeResponse};
use super::server;
use crate::workshop::tools::{ToolContext, ToolRegistry};

/// One tool call with everything it needs off the main thread.
pub(crate) struct PreparedCall {
    id: Value,
    name: String,
    use_id: String,
    input: Value,
    ctx: ToolContext,
    registry: ToolRegistry,
}

impl PreparedCall {
    /// Dispatch the tool and build the bridge's answer.
    pub(crate) fn run(self) -> BridgeResponse {
        let result = self.registry.dispatch(&self.name, &self.use_id, self.input, &self.ctx);
        BridgeResponse::ok(
            self.id,
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
}

/// Build a call on the main thread. Params mirror the Anthropic tool_use
/// frame, so Claude-compatible callers need no translation:
///
/// ```text
/// { "name": "create_entity", "input": { ... }, "tool_use_id": "toolu_abc" }
/// ```
///
/// `tool_use_id` is optional; without one a fresh id is made. The context is
/// built from `SpaceRoot` and `AuthState` as the Workshop builds it, so a
/// tool behaves the same whoever calls it.
pub(crate) fn prepare(world: &World, req: &BridgeRequest) -> Result<PreparedCall, BridgeResponse> {
    let refuse = |e: BridgeError| BridgeResponse::error(req.id.clone(), e);
    let Some(name) = req.params.get("name").and_then(|v| v.as_str()) else {
        return Err(refuse(BridgeError::invalid_params("missing 'name' (tool id)")));
    };
    let input = req.params.get("input").cloned().unwrap_or(Value::Null);
    // During Play a tool that changes the Space waits for Stop, as the
    // bridge's own edits do; a read-only tool and the tools that drive the
    // running session still run (`workshop::tool_play_refusal`). This is the
    // engine's dispatch only: the MCP server's own writes are not checked
    // here.
    let class = eustress_tools::capability::capability_of_call(name, &input);
    let play_state = world.get_resource::<State<crate::play_mode::PlayModeState>>();
    if let Some(why) = crate::workshop::tool_play_refusal(play_state, name, class) {
        return Err(refuse(BridgeError::invalid_params(why)));
    }
    let use_id = req
        .params
        .get("tool_use_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("bridge_{}", rand_id()));

    let Some(space_root) = world.get_resource::<crate::space::SpaceRoot>() else {
        return Err(refuse(BridgeError::internal("SpaceRoot not loaded: no Space is active")));
    };
    let space_root_path = space_root.0.clone();
    let universe_root = crate::space::universe_root_for_path(&space_root_path)
        .unwrap_or_else(|| space_root_path.clone());

    let (user_id, username) = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.user.as_ref().map(|u| (Some(u.id.clone()), Some(u.username.clone()))))
        .unwrap_or((None, None));

    let display_unit = world
        .get_resource::<eustress_common::units::DisplayUnit>()
        .map(|d| d.0.symbol().to_string());

    let ctx = ToolContext {
        space_root: space_root_path,
        universe_root,
        user_id,
        username,
        luau_executor: None,
        display_unit,
        // Nothing can reach a running bridge call to cancel it.
        cancelled: None,
        // CMMC AC.L1-3.1.2. The bridge listens on TCP and is reachable by
        // anything that can read the `engine.port` file; it performs no
        // peer authentication (IA.L1-3.5.2, still open). Until it does, it
        // gets the standard set: Read and Write, never Execute, Destructive
        // or Network. That is what stops an unauthenticated local peer from
        // calling `run_bash` or `delete_entity`.
        permissions: eustress_tools::Permissions::standard().for_principal("engine-bridge"),
    };

    let Some(registry) = world.get_resource::<ToolRegistry>() else {
        return Err(refuse(BridgeError::internal("ToolRegistry not available")));
    };

    Ok(PreparedCall {
        id: req.id.clone(),
        name: name.to_string(),
        use_id,
        input,
        ctx,
        // Its handlers are shared, so the copy is cheap.
        registry: registry.clone(),
    })
}

/// Answer `tools.call` from a worker thread.
pub(crate) fn begin_tools_call(world: &mut World, pending: server::Pending) {
    match prepare(world, &pending.request) {
        Err(response) => {
            let _ = pending.responder.send(response);
        }
        Ok(call) => {
            let responder = pending.responder;
            std::thread::spawn(move || {
                let response = call.run();
                if responder.send(response).is_err() {
                    debug!("EngineBridge: tools.call caller disconnected before the tool finished");
                }
            });
        }
    }
}

/// Cheap per-request id without pulling in uuid here. The bridge id space is
/// private to sibling-initiated calls, so a collision is astronomically
/// unlikely.
fn rand_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workshop::tools::{ToolDefinition, ToolHandler, ToolResult};
    use std::sync::{Arc, Mutex};

    /// Records the thread it ran on. Registered under a Read-classified name,
    /// so the bridge's grant lets it through.
    struct ThreadProbe(Arc<Mutex<Option<std::thread::ThreadId>>>);

    impl ToolHandler for ThreadProbe {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "list_directory",
                description: "records the thread it runs on",
                input_schema: serde_json::json!({ "type": "object" }),
                modes: &[eustress_tools::WorkshopMode::General],
                requires_approval: false,
                stream_topics: &[],
            }
        }

        fn execute(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
            *self.0.lock().unwrap() = Some(std::thread::current().id());
            ToolResult {
                tool_name: "list_directory".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: "ran".to_string(),
                structured_data: None,
                stream_topic: None,
            }
        }
    }

    fn request(params: Value) -> BridgeRequest {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools.call", "params": params,
        }))
        .unwrap()
    }

    /// The tool runs on a thread of its own, and its result still reaches the
    /// caller under the request's id.
    #[test]
    fn a_tool_runs_off_the_main_thread_and_is_answered() {
        let ran_on = Arc::new(Mutex::new(None));
        let mut registry = ToolRegistry::default();
        registry.register(ThreadProbe(ran_on.clone()));
        let mut world = World::new();
        world.insert_resource(registry);
        world.insert_resource(crate::space::SpaceRoot(std::env::temp_dir()));

        let (tx, rx) = tokio::sync::oneshot::channel();
        begin_tools_call(
            &mut world,
            server::Pending { request: request(serde_json::json!({ "name": "list_directory", "input": {} })), responder: tx },
        );
        let answer = serde_json::to_value(rx.blocking_recv().expect("answered")).unwrap();
        assert_eq!(answer["id"], 7, "{answer}");
        assert_eq!(answer["result"]["success"], true, "{answer}");
        assert_eq!(answer["result"]["content"], "ran", "{answer}");
        let ran = ran_on.lock().unwrap().expect("the tool ran");
        assert_ne!(ran, std::thread::current().id(), "the tool ran on the calling thread");
    }

    /// Without an open Space the call is refused at once, on the calling
    /// thread, before any tool runs.
    #[test]
    fn without_a_space_the_call_is_refused_at_once() {
        let ran_on = Arc::new(Mutex::new(None));
        let mut registry = ToolRegistry::default();
        registry.register(ThreadProbe(ran_on.clone()));
        let mut world = World::new();
        world.insert_resource(registry);

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        begin_tools_call(
            &mut world,
            server::Pending { request: request(serde_json::json!({ "name": "list_directory" })), responder: tx },
        );
        let answer = serde_json::to_value(rx.try_recv().expect("answered before returning")).unwrap();
        assert!(answer["error"].is_object(), "{answer}");
        assert!(ran_on.lock().unwrap().is_none(), "the tool ran without a Space");
    }

    /// During Play a tool that changes the Space is refused before it is
    /// prepared, and a read-only tool still runs.
    #[test]
    fn play_refuses_a_tool_that_changes_the_space() {
        use crate::play_mode::PlayModeState;
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            let mut world = World::new();
            world.insert_resource(ToolRegistry::default());
            world.insert_resource(crate::space::SpaceRoot(std::env::temp_dir()));
            world.insert_resource(State::new(state));
            let refused = prepare(&world, &request(serde_json::json!({ "name": "create_entity", "input": {} })));
            let answer = serde_json::to_value(refused.err().expect("refused during Play")).unwrap();
            let message = answer["error"]["message"].as_str().unwrap_or_default().to_string();
            assert!(message.starts_with("Stop Play to run create_entity"), "{state:?}: {answer}");
            assert!(prepare(&world, &request(serde_json::json!({ "name": "list_directory", "input": {} }))).is_ok(), "{state:?}");
        }
        let mut world = World::new();
        world.insert_resource(ToolRegistry::default());
        world.insert_resource(crate::space::SpaceRoot(std::env::temp_dir()));
        world.insert_resource(State::new(PlayModeState::Editing));
        assert!(prepare(&world, &request(serde_json::json!({ "name": "create_entity", "input": {} }))).is_ok());
    }

    /// The tools that drive the running session pass tools.call during Play,
    /// the simulation's run, pause and stop included, though each is a Write
    /// tool; a Write tool that is not one of them is refused.
    #[test]
    fn the_session_tools_run_in_play_through_tools_call() {
        use crate::play_mode::PlayModeState;
        let session = [
            "play_input",
            "set_sim_value",
            "sim_step",
            "control_simulation",
            "set_breakpoint",
            "run_simulation",
            "pause_simulation",
            "stop_simulation",
        ];
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            let mut world = World::new();
            world.insert_resource(ToolRegistry::default());
            world.insert_resource(crate::space::SpaceRoot(std::env::temp_dir()));
            world.insert_resource(State::new(state));
            for tool in session {
                let class = eustress_tools::capability::capability_of_call(tool, &serde_json::json!({}));
                assert_eq!(class, Some(eustress_tools::Capability::Write), "{tool}");
                assert!(prepare(&world, &request(serde_json::json!({ "name": tool, "input": {} }))).is_ok(), "{state:?} refused {tool}");
            }
            let class = eustress_tools::capability::capability_of_call("run_experiment", &serde_json::json!({}));
            assert_eq!(class, Some(eustress_tools::Capability::Write));
            let refused = prepare(&world, &request(serde_json::json!({ "name": "run_experiment", "input": {} })));
            let answer = serde_json::to_value(refused.err().expect("refused during Play")).unwrap();
            let message = answer["error"]["message"].as_str().unwrap_or_default().to_string();
            assert!(message.starts_with("Stop Play to run run_experiment"), "{state:?}: {answer}");
        }
    }
}
