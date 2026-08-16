// Eustress MCP server — stdio entry point.
//
// Hand-rolled JSON-RPC 2.0 shell. MCP is newline-delimited JSON on stdin /
// stdout with a fixed set of methods, so a dependency on a higher-level MCP
// SDK would double our binary size without adding capability. All language
// logic lives in tools.rs / resources.rs. This file wires the transport to
// the handlers, resolves the active Universe, and forwards file-watcher
// events up to subscribers.
//
// Replaces the TypeScript implementation that previously lived in
// infrastructure/mcp/server/. Same installer, same protocol surface, a
// fraction of the binary size.

mod bridge_tools;
mod resources;
mod shared_registry;
mod tools;
mod universe;
mod uri;
mod watcher;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::tools::ServerState;
use crate::universe::{discover_universes, find_universe_root, parse_search_roots};
use crate::watcher::SubscriptionManager;

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// In-flight requests that can be cancelled, keyed by their JSON-RPC id.
///
/// MCP clients cancel with `notifications/cancelled {requestId}`. Without a
/// registry there is nothing to route that at, so the notification was being
/// dropped and an `await_simulation` kept blocking a worker for its full
/// 300 s timeout after the caller had already walked away.
///
/// Ids are JSON values (string or number per JSON-RPC), so we key on their
/// canonical serialization to compare `1` and `"1"` the way the client meant.
#[derive(Default)]
struct CancellationRegistry {
    inflight: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl CancellationRegistry {
    fn key(id: &Value) -> String {
        id.to_string()
    }

    /// Register `id` as cancellable and hand back its flag. The guard is
    /// dropped by [`Self::finish`] once the request completes.
    fn begin(&self, id: &Value) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut map) = self.inflight.lock() {
            map.insert(Self::key(id), Arc::clone(&flag));
        }
        flag
    }

    fn finish(&self, id: &Value) {
        if let Ok(mut map) = self.inflight.lock() {
            map.remove(&Self::key(id));
        }
    }

    /// Flip the flag for `id`. Returns false when the id isn't in flight —
    /// which is normal and explicitly allowed by the spec (the response may
    /// already have been sent when the cancel was written).
    fn cancel(&self, id: &Value) -> bool {
        let Ok(map) = self.inflight.lock() else {
            return false;
        };
        match map.get(&Self::key(id)) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}

const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "eustress-mcp-server";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP revisions this server speaks, newest first. Every one of them uses the
/// same JSON-RPC methods we implement; the differences are in optional fields
/// clients may ignore.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Pick the version to answer `initialize` with.
///
/// The spec requires echoing the client's requested version when we support
/// it, and only otherwise proposing our own. Unconditionally answering with
/// our newest — the previous behaviour — tells a 2024-11-05 client to speak a
/// revision it has never heard of, which strict clients treat as a failed
/// handshake and abort on.
fn negotiate_protocol_version(params: &Value) -> &'static str {
    let requested = params.get("protocolVersion").and_then(|v| v.as_str());
    match requested {
        Some(req) => SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .find(|v| *v == req)
            .unwrap_or(PROTOCOL_VERSION),
        None => PROTOCOL_VERSION,
    }
}

fn resolve_initial_universe() -> Option<PathBuf> {
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--universe" {
            if let Some(val) = iter.next() {
                return Some(PathBuf::from(val));
            }
        }
    }
    if let Ok(val) = std::env::var("EUSTRESS_UNIVERSE") {
        if !val.is_empty() {
            return Some(PathBuf::from(val));
        }
    }
    find_universe_root(&std::env::current_dir().ok()?)
}

fn resolve_search_roots() -> Vec<PathBuf> {
    parse_search_roots(std::env::var("EUSTRESS_UNIVERSES_PATH").ok().as_deref())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // All tracing goes to stderr so it never pollutes the stdio JSON-RPC
    // channel. Default to `info`; operators can override via RUST_LOG.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let state = Arc::new(Mutex::new(ServerState {
        current_universe: resolve_initial_universe(),
        search_roots: resolve_search_roots(),
    }));

    // Boot-time tool-surface summary — goes to stderr so the stdio
    // JSON-RPC channel stays clean. Helps operators confirm the
    // server binary was rebuilt after a tool addition without having
    // to call `tools/list` through the IDE.
    let shared_tools = crate::shared_registry::list_shared_tools();
    let local_tool_count = tools::all_tools().len();
    tracing::info!(
        "🔧 {} v{} ready — {} hand-rolled tools + {} shared-registry tools = {} total surface",
        SERVER_NAME, SERVER_VERSION,
        local_tool_count, shared_tools.len(),
        local_tool_count + shared_tools.len()
    );

    // Outgoing writes must be serialized — both tool responses and watcher
    // notifications write to the same stdout. A Mutex-guarded tokio stdout
    // prevents interleaved bytes.
    let stdout = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));

    // Fire-and-forget notification channel, shared by the file watcher and by
    // handlers that change what the client's cached view should contain.
    let stdout_for_notify = Arc::clone(&stdout);
    let notifier: Notifier = Arc::new(move |notice: Value| {
        let stdout = Arc::clone(&stdout_for_notify);
        tokio::spawn(async move {
            let mut guard = stdout.lock().await;
            let line = format!("{notice}\n");
            let _ = guard.write_all(line.as_bytes()).await;
            let _ = guard.flush().await;
        });
    });

    // Subscription manager emits `notifications/resources/updated` for every
    // watched file change that matches a subscribed URI.
    let notifier_for_subs = Arc::clone(&notifier);
    let subs = Arc::new(SubscriptionManager::new(move |uri: String| {
        notifier_for_subs(json!({
            "jsonrpc": "2.0",
            "method": "notifications/resources/updated",
            "params": { "uri": uri },
        }));
    }));

    let cancels = Arc::new(CancellationRegistry::default());

    // Log startup — to stderr — so operators see state without it ever
    // reaching the client.
    {
        let s = state.lock().unwrap();
        let universe = s
            .current_universe
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none; set via tool)".into());
        let roots = s
            .search_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            std::io::stderr(),
            "[eustress-mcp] v{SERVER_VERSION} (rust) ready — universe={universe}, tools={}, search_roots=[{roots}]",
            tools::all_tools().len(),
        )
        .ok();
    }

    // If we have a universe already, point the watcher at it pre-emptively so
    // the first subscription doesn't pay the startup cost.
    if let Some(u) = state.lock().unwrap().current_universe.clone() {
        subs.retarget_universe(Some(u));
    }

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line).await;
        match read {
            Ok(0) => break, // EOF — client disconnected
            Ok(_) => {}
            Err(e) => {
                tracing::error!("stdin read error: {e}");
                break;
            }
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("parse error: {e}") },
                });
                write_response(&stdout, resp).await?;
                continue;
            }
        };

        // Gap 7 — handle each request on its own task so a long-running tool
        // (e.g. `await_simulation`, which blocks polling for up to `timeout_s`)
        // can't wedge the stdin read loop. Other requests (ping,
        // get_simulation_state, inspect_scene, …) keep flowing WHILE a sim runs —
        // required for live optimize/telemetry loops. Outgoing writes stay
        // serialized by the shared tokio Mutex in `write_response`.
        let state = Arc::clone(&state);
        let subs = Arc::clone(&subs);
        let stdout = Arc::clone(&stdout);
        let notifier = Arc::clone(&notifier);
        let cancels = Arc::clone(&cancels);
        tokio::spawn(async move {
            if let Err(e) =
                handle_message(&req, &state, &subs, &stdout, &notifier, &cancels).await
            {
                tracing::error!("request handling failed: {e}");
            }
        });
    }

    subs.shutdown();
    Ok(())
}

/// Sends a JSON-RPC notification to the client without waiting for it.
type Notifier = Arc<dyn Fn(Value) + Send + Sync>;

async fn handle_message(
    msg: &Value,
    state: &Arc<Mutex<ServerState>>,
    subs: &Arc<SubscriptionManager>,
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    notifier: &Notifier,
    cancels: &Arc<CancellationRegistry>,
) -> anyhow::Result<()> {
    let id = msg.get("id").cloned();
    let method = match msg.get("method").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return Ok(()), // responses from client, ignored
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no id) don't get a response.
    let is_notification = id.is_none();

    // Register the request as cancellable for as long as it runs. Only real
    // requests get a flag — a notification has no id to cancel by.
    let cancel_flag = id.as_ref().map(|i| cancels.begin(i));

    let result = dispatch(&method, &params, state, subs, notifier, cancels, cancel_flag.clone()).await;

    if let Some(i) = id.as_ref() {
        cancels.finish(i);
    }

    if is_notification {
        return Ok(());
    }

    // Per MCP, a cancelled request SHOULD NOT be answered — the client has
    // already released the id and a late response is at best ignored, at
    // worst matched against a reused id.
    if cancel_flag.is_some_and(|f| f.load(Ordering::Relaxed)) {
        tracing::debug!("suppressing response for cancelled request {method}");
        return Ok(());
    }

    let response = match result {
        Ok(value) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": value,
        }),
        Err(err) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": err.code, "message": err.message },
        }),
    };
    write_response(stdout, response).await
}

struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
        }
    }
    fn invalid(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
        }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch(
    method: &str,
    params: &Value,
    state: &Arc<Mutex<ServerState>>,
    subs: &Arc<SubscriptionManager>,
    notifier: &Notifier,
    cancels: &Arc<CancellationRegistry>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": negotiate_protocol_version(params),
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            "capabilities": {
                "tools": {},
                "resources": {
                    "subscribe": true,
                    // The resource list is per-Universe, and `set_active_universe`
                    // swaps it wholesale mid-session. We now announce that with
                    // `notifications/resources/list_changed`, so clients can stop
                    // serving a list that belongs to a Universe we've left.
                    "listChanged": true,
                },
                "prompts": {},
            },
        })),
        "initialized" | "notifications/initialized" => Ok(Value::Null),

        "ping" => Ok(json!({})),

        // ── Cancellation ─────────────────────────────────────────────
        //
        // Best-effort by design: a cancel that arrives after the work
        // finished is a no-op, which the spec explicitly permits. What it
        // buys us is that long-polling tools stop waiting immediately
        // instead of holding a worker for their full timeout.
        "notifications/cancelled" | "$/cancelRequest" => {
            let Some(id) = params
                .get("requestId")
                .or_else(|| params.get("id"))
                .filter(|v| !v.is_null())
            else {
                return Err(RpcError::invalid(
                    "notifications/cancelled: missing `requestId`",
                ));
            };
            let hit = cancels.cancel(id);
            let reason = params.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            tracing::debug!(
                "cancel for request {id}{}{reason} — {}",
                if reason.is_empty() { "" } else { ": " },
                if hit { "signalled" } else { "already finished" },
            );
            Ok(Value::Null)
        }

        // ── Tools ────────────────────────────────────────────────────
        //
        // The MCP server exposes two tool sets:
        //   1. Hand-rolled `eustress_*` tools (MCP-native API: universe
        //      browsing, git, script CRUD — historically stable names
        //      external IDEs wired into their agent configs).
        //   2. Shared `eustress-tools` registry — the same handlers the
        //      engine's Workshop agent ships. Unifies entity, file,
        //      script, memory, simulation, physics, spatial tools so a
        //      tool defined once is available everywhere.
        //
        // `tools/list` returns both; `tools/call` dispatches to
        // whichever registry owns the requested name. On collision the
        // hand-rolled list wins (it came first + has stable names).
        "tools/list" => {
            let mut all = Vec::new();
            for t in tools::all_tools() {
                all.push(json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": (t.input_schema)(),
                }));
            }
            // Append shared-registry tools, skipping any name already
            // present in the hand-rolled list.
            let taken: std::collections::HashSet<&'static str> =
                tools::all_tools().iter().map(|t| t.name).collect();
            for t in shared_registry::list_shared_tools() {
                if let Some(name) = t.get("name").and_then(|v| v.as_str()) {
                    if !taken.contains(name) {
                        all.push(t);
                    }
                }
            }
            Ok(json!({ "tools": all }))
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::invalid("tools/call: missing `name`"))?;
            let args = params.get("arguments").cloned().unwrap_or(Value::Object(Default::default()));

            // Hand-rolled tools take priority for stable-name compat.
            let registry = tools::all_tools();
            if let Some(tool) = registry.iter().find(|t| t.name == name) {
                let prior_universe = state.lock().unwrap().current_universe.clone();
                let mut guard = state.lock().unwrap();
                let result = (tool.handler)(&args, &mut guard);
                let new_universe = guard.current_universe.clone();
                drop(guard);

                if new_universe != prior_universe {
                    subs.retarget_universe(new_universe);
                    // Different Universe, different Spaces/scripts/briefs —
                    // everything the client listed a moment ago is now for the
                    // wrong world.
                    notifier(json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/resources/list_changed",
                    }));
                }
                return Ok(result.to_json());
            }

            // Fall through to the shared registry. Resolve the Universe with
            // bridge-tool awareness: live-engine tools target the running
            // engine's Universe (via its port file), everything else uses the
            // explicit `universe` arg or the server default.
            let universe = resolve_shared_universe(name, &args, state);
            // Gap 7 — shared-registry tools are synchronous and some BLOCK for a
            // long time (`await_simulation` / `run_experiment` poll with
            // `std::thread::sleep` until the run ends or `timeout_s`). Run them on
            // tokio's blocking pool so they never stall the single-threaded
            // executor that drives the stdin read loop and the other in-flight
            // request tasks. `try_dispatch` returns an owned, Send Option, so the
            // offload is a clean move.
            let name_owned = name.to_string();
            let args_owned = args.clone();
            let cancel_for_tool = cancel_flag.clone();
            let dispatched = tokio::task::spawn_blocking(move || {
                shared_registry::try_dispatch(
                    &name_owned,
                    &args_owned,
                    universe.as_ref(),
                    cancel_for_tool,
                )
            })
            .await
            .map_err(|e| RpcError::internal(format!("tool dispatch task failed: {e}")))?;
            if let Some(result) = dispatched {
                return Ok(shared_registry::to_mcp_json(result));
            }

            Err(RpcError::invalid(format!("unknown tool: {name}")))
        }

        // ── Resources ────────────────────────────────────────────────
        "resources/list" => {
            let universe = ensure_universe(state, subs);
            match universe {
                Some(u) => {
                    // Paginated per MCP: the previous version hard-stopped at
                    // 200 entries with no cursor and no marker, so a Universe
                    // with more Spaces/scripts than that simply looked like it
                    // ended there. Now the tail is reachable.
                    let cursor = params
                        .get("cursor")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    let offset = match decode_cursor(cursor.as_deref()) {
                        Ok(o) => o,
                        Err(e) => return Err(RpcError::invalid(e)),
                    };
                    let all = resources::list_resources(&u);
                    let page: Vec<_> =
                        all.iter().skip(offset).take(RESOURCE_PAGE_SIZE).cloned().collect();
                    let next = offset + page.len();
                    let mut out = json!({ "resources": page });
                    if next < all.len() {
                        out["nextCursor"] = json!(encode_cursor(next));
                    }
                    Ok(out)
                }
                None => Ok(json!({
                    "resources": [{
                        "uri": "eustress://help/setup",
                        "name": "Getting started",
                        "description":
                            "No Universe found on disk. Call the `eustress_list_universes` tool to discover, or `eustress_set_default_universe` to point at one explicitly.",
                        "mimeType": "text/markdown",
                    }],
                })),
            }
        }
        "resources/templates/list" => Ok(json!({
            "resourceTemplates": uri::templates(),
        })),
        "resources/read" => {
            let raw = params
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::invalid("resources/read: missing `uri`"))?
                .to_string();

            if raw == "eustress://help/setup" {
                return Ok(json!({
                    "contents": [{
                        "uri": raw,
                        "mimeType": "text/markdown",
                        "text": help_text(),
                    }],
                }));
            }

            let universe = ensure_universe(state, subs).ok_or_else(|| {
                RpcError::internal(
                    "No Universe configured. Call `eustress_list_universes` / `eustress_set_default_universe` first.",
                )
            })?;
            let block = resources::read_resource(&universe, &raw)
                .map_err(|e| RpcError::invalid(e))?;
            Ok(json!({ "contents": [block] }))
        }

        // ── Subscriptions ────────────────────────────────────────────
        "resources/subscribe" => {
            let uri_str = params
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::invalid("resources/subscribe: missing `uri`"))?
                .to_string();
            let universe = state.lock().unwrap().current_universe.clone();
            subs.subscribe(uri_str, universe);
            Ok(json!({}))
        }
        "resources/unsubscribe" => {
            let uri_str = params
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::invalid("resources/unsubscribe: missing `uri`"))?;
            subs.unsubscribe(uri_str);
            Ok(json!({}))
        }

        // ── Prompts (empty, present to silence client probes) ────────
        "prompts/list" => Ok(json!({ "prompts": [] })),

        // ── Shutdown ────────────────────────────────────────────────
        "shutdown" => Ok(Value::Null),

        other => Err(RpcError::method_not_found(other)),
    }
}

/// Entries per `resources/list` page.
const RESOURCE_PAGE_SIZE: usize = 100;

/// Cursors are opaque to the client per MCP, so the prefix exists purely to
/// make a hand-crafted or stale cursor fail loudly instead of being read as
/// an offset into a different Universe's list.
const CURSOR_PREFIX: &str = "eustress:offset:";

fn encode_cursor(offset: usize) -> String {
    format!("{CURSOR_PREFIX}{offset}")
}

fn decode_cursor(cursor: Option<&str>) -> Result<usize, String> {
    match cursor {
        None => Ok(0),
        Some(c) => c
            .strip_prefix(CURSOR_PREFIX)
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| format!("invalid cursor: {c}")),
    }
}

/// Auto-resolve a Universe if none is set. Clients that call `resources/list`
/// or `resources/read` before `eustress_set_default_universe` shouldn't be
/// punished with an empty list. Strategy: cwd first (most specific), then
/// sweep the search roots (broadest). First hit wins, logged to stderr.
fn ensure_universe(
    state: &Arc<Mutex<ServerState>>,
    subs: &Arc<SubscriptionManager>,
) -> Option<PathBuf> {
    {
        let s = state.lock().unwrap();
        if let Some(u) = &s.current_universe {
            return Some(u.clone());
        }
    }
    // Walk cwd
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = find_universe_root(&cwd) {
            let mut s = state.lock().unwrap();
            s.current_universe = Some(found.clone());
            writeln!(
                std::io::stderr(),
                "[eustress-mcp] auto-resolved Universe from cwd → {}",
                found.display()
            )
            .ok();
            drop(s);
            subs.retarget_universe(Some(found.clone()));
            return Some(found);
        }
    }
    // Sweep roots
    let roots = state.lock().unwrap().search_roots.clone();
    let found = discover_universes(&roots);
    if let Some(first) = found.first() {
        let mut s = state.lock().unwrap();
        s.current_universe = Some(first.clone());
        writeln!(
            std::io::stderr(),
            "[eustress-mcp] auto-resolved Universe from search roots → {} ({} total)",
            first.display(),
            found.len(),
        )
        .ok();
        drop(s);
        subs.retarget_universe(Some(first.clone()));
        return Some(first.clone());
    }
    None
}

/// Resolve the Universe for a shared-registry tool call.
///
/// Precedence:
///   1. An explicit `universe` arg (absolute path) — caller knows best.
///   2. For live-engine **bridge** tools: the Universe with a live
///      `engine.port` (the running engine). This is what makes
///      `inspect_scene` / `capture_viewport` / … "just work" without the
///      client first calling `set_active_universe` — they follow the port
///      file to wherever the engine is actually running.
///   3. The server's current default Universe.
///   4. A walk up from the current working directory.
fn resolve_shared_universe(
    name: &str,
    args: &Value,
    state: &Arc<Mutex<ServerState>>,
) -> Option<PathBuf> {
    if let Some(u) = args.get("universe").and_then(|v| v.as_str()) {
        let t = u.trim();
        if !t.is_empty() {
            return Some(PathBuf::from(t));
        }
    }
    if shared_registry::is_bridge_tool(name) {
        let roots = state.lock().unwrap().search_roots.clone();
        if let Some(live) = crate::universe::find_live_engine_universe(&roots) {
            return Some(live);
        }
    }
    if let Some(u) = state.lock().unwrap().current_universe.clone() {
        return Some(u);
    }
    find_universe_root(&std::env::current_dir().ok()?)
}

async fn write_response(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    response: Value,
) -> anyhow::Result<()> {
    let serialized = serde_json::to_string(&response)?;
    let mut guard = stdout.lock().await;
    guard.write_all(serialized.as_bytes()).await?;
    guard.write_all(b"\n").await?;
    guard.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoes_a_supported_client_version() {
        // Claude Desktop still initializes with 2024-11-05; answering
        // 2025-06-18 there is a handshake failure, not an upgrade.
        let params = json!({ "protocolVersion": "2024-11-05" });
        assert_eq!(negotiate_protocol_version(&params), "2024-11-05");
    }

    #[test]
    fn proposes_our_version_for_unknown_or_absent() {
        assert_eq!(
            negotiate_protocol_version(&json!({ "protocolVersion": "1999-01-01" })),
            PROTOCOL_VERSION
        );
        assert_eq!(negotiate_protocol_version(&json!({})), PROTOCOL_VERSION);
        assert_eq!(negotiate_protocol_version(&Value::Null), PROTOCOL_VERSION);
    }

    #[test]
    fn cancel_signals_the_inflight_request() {
        let reg = CancellationRegistry::default();
        let id = json!(7);
        let flag = reg.begin(&id);
        assert!(!flag.load(Ordering::Relaxed));
        assert!(reg.cancel(&id));
        assert!(flag.load(Ordering::Relaxed));
    }

    #[test]
    fn cancel_after_completion_is_a_no_op() {
        let reg = CancellationRegistry::default();
        let id = json!("abc");
        let flag = reg.begin(&id);
        reg.finish(&id);
        // Allowed by the spec: the response may already have been sent.
        assert!(!reg.cancel(&id));
        assert!(!flag.load(Ordering::Relaxed));
    }

    #[test]
    fn cancel_does_not_leak_across_ids() {
        let reg = CancellationRegistry::default();
        let a = reg.begin(&json!(1));
        let b = reg.begin(&json!(2));
        reg.cancel(&json!(2));
        assert!(!a.load(Ordering::Relaxed));
        assert!(b.load(Ordering::Relaxed));
        // A string id and a numeric id are distinct requests.
        assert!(!reg.cancel(&json!("2")));
    }

    #[test]
    fn cursor_round_trips_and_rejects_junk() {
        assert_eq!(decode_cursor(None).unwrap(), 0);
        assert_eq!(decode_cursor(Some(&encode_cursor(250))).unwrap(), 250);
        assert!(decode_cursor(Some("250")).is_err());
        assert!(decode_cursor(Some("eustress:offset:abc")).is_err());
    }
}

fn help_text() -> &'static str {
    "# Eustress MCP — Getting started\n\n\
     The server is running but has no Universe selected, so there are no\n\
     Spaces, Scripts, or entities to browse.\n\n\
     **Next steps** (pick one):\n\n\
     1. Call the `eustress_list_universes` tool — it scans the configured\n   \
        search roots (`EUSTRESS_UNIVERSES_PATH` env var; defaults to\n   \
        `~/Eustress`, `~/Documents/Eustress`, home) and any Universe enclosing\n   \
        the current working directory.\n\
     2. Call `eustress_set_default_universe` with an absolute path to a folder\n   \
        that contains `Spaces/`.\n\
     3. Restart the MCP server with `EUSTRESS_UNIVERSE=/path/to/Universe` or\n   \
        `--universe /path/to/Universe`.\n\n\
     Once a Universe is selected, `resources/list` will return the Spaces,\n\
     scripts, conversations, and briefs in that Universe."
}
