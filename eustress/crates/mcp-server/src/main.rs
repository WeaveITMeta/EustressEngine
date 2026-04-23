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

mod resources;
mod shared_registry;
mod tools;
mod universe;
mod uri;
mod watcher;

use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::tools::ServerState;
use crate::universe::{discover_universes, find_universe_root, parse_search_roots};
use crate::watcher::SubscriptionManager;

const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "eustress-mcp-server";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

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

    // Subscription manager emits `notifications/resources/updated` for every
    // watched file change that matches a subscribed URI. We stash a cloned
    // writer so the callback can send without borrowing `state`.
    let stdout_for_notify = Arc::clone(&stdout);
    let subs = Arc::new(SubscriptionManager::new(move |uri: String| {
        let notice = json!({
            "jsonrpc": "2.0",
            "method": "notifications/resources/updated",
            "params": { "uri": uri },
        });
        let stdout = Arc::clone(&stdout_for_notify);
        tokio::spawn(async move {
            let mut guard = stdout.lock().await;
            let line = format!("{notice}\n");
            let _ = guard.write_all(line.as_bytes()).await;
            let _ = guard.flush().await;
        });
    }));

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

        handle_message(&req, &state, &subs, &stdout).await?;
    }

    subs.shutdown();
    Ok(())
}

async fn handle_message(
    msg: &Value,
    state: &Arc<Mutex<ServerState>>,
    subs: &Arc<SubscriptionManager>,
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
) -> anyhow::Result<()> {
    let id = msg.get("id").cloned();
    let method = match msg.get("method").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return Ok(()), // responses from client, ignored
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no id) don't get a response.
    let is_notification = id.is_none();

    let result = dispatch(&method, &params, state, subs).await;

    if is_notification {
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

async fn dispatch(
    method: &str,
    params: &Value,
    state: &Arc<Mutex<ServerState>>,
    subs: &Arc<SubscriptionManager>,
) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            "capabilities": {
                "tools": {},
                "resources": {
                    "subscribe": true,
                    "listChanged": false,
                },
                "prompts": {},
            },
        })),
        "initialized" | "notifications/initialized" => Ok(Value::Null),

        "ping" => Ok(json!({})),

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
                }
                return Ok(result.to_json());
            }

            // Fall through to the shared registry.
            let universe = state.lock().unwrap().current_universe.clone();
            if let Some(result) = shared_registry::try_dispatch(name, &args, universe.as_ref()) {
                return Ok(shared_registry::to_mcp_json(result));
            }

            Err(RpcError::invalid(format!("unknown tool: {name}")))
        }

        // ── Resources ────────────────────────────────────────────────
        "resources/list" => {
            let universe = ensure_universe(state, subs);
            match universe {
                Some(u) => {
                    let listed = resources::list_resources(&u);
                    Ok(json!({ "resources": listed }))
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
