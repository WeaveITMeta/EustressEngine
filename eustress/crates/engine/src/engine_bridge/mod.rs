//! # Engine Bridge — general-purpose RPC surface for sibling processes
//!
//! Mirrors the LSP's `.eustress/lsp.port` handoff pattern but hosts a
//! JSON-RPC 2.0 endpoint that exposes live ECS state, simulation values,
//! embedvec search, and mention resolution. External processes (MCP
//! server, LSP upgrades, future plugins) connect over localhost TCP and
//! drive the engine as if they were embedded in it.
//!
//! ## Why a bridge and not IPC via filesystem?
//!
//! `runtime-snapshot.json` is fine for periodic reads of `sim_values`
//! (the LSP uses exactly that today), but it can't serve:
//!
//! * live entity queries — the filesystem doesn't know what's in the
//!   Bevy `World` right now,
//! * raycasts / physics queries — those need Rapier state,
//! * embedvec semantic search — the in-memory index is fresher than
//!   the RocksDB on disk,
//! * mutations that must happen *now* (waiting for file-watcher
//!   pickup is too slow for interactive tool calls).
//!
//! A TCP JSON-RPC endpoint collapses all of those into one surface
//! that any sibling can speak.
//!
//! ## Lifecycle
//!
//! 1. [`EngineBridgePlugin`] binds `127.0.0.1:0` at startup (OS picks
//!    a free port).
//! 2. The bound port is written to `<universe>/.eustress/engine.port`
//!    for sibling discovery.
//! 3. A tokio task handles incoming connections, parses JSON-RPC
//!    frames, and pushes each request onto a shared pending-queue.
//! 4. A Bevy system drains the queue on the main thread each frame,
//!    dispatches to a handler with `&mut World` access, and writes the
//!    response back through a oneshot channel the TCP task is
//!    awaiting.
//! 5. On `AppExit`, the port file is deleted and the listener shut
//!    down.
//!
//! ## Concurrency model
//!
//! Handlers run synchronously on the Bevy main thread — no lock
//! shenanigans with `World`. The TCP task is async but only sees
//! opaque `serde_json::Value`s; no Bevy types cross the thread
//! boundary. This mirrors how EustressStream publishes events:
//! producers drop them into a queue, a frame-scoped system consumes.

use bevy::prelude::*;
use std::sync::Arc;

mod port_file;
mod protocol;
mod server;

pub use protocol::{BridgeRequest, BridgeResponse, BridgeError, MethodName};

use server::PendingRequests;
use port_file::PortFile;

/// Shared tokio runtime used by the bridge's TCP listener. We own our
/// own multi-thread runtime because existing engine subsystems each
/// create their own ad-hoc `Runtime`s — sharing one would be nice but
/// is out of scope for this change. Two worker threads is plenty for
/// sibling RPC traffic (Claude API requests still spawn separate
/// threads).
#[derive(Resource)]
struct BridgeRuntime {
    handle: tokio::runtime::Handle,
    // Holds the runtime alive — dropping it shuts down the listener.
    _runtime: tokio::runtime::Runtime,
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Registers the Engine Bridge: spawns the TCP listener, advertises its
/// port via `.eustress/engine.port`, and installs the per-frame system
/// that drains pending requests.
pub struct EngineBridgePlugin;

impl Plugin for EngineBridgePlugin {
    fn build(&self, app: &mut App) {
        let pending = PendingRequests::default();
        app.insert_resource(BridgePendingQueue(pending.clone()))
            .add_systems(Startup, setup_engine_bridge)
            .add_systems(Update, drain_bridge_requests);
    }
}

/// Shared pending-request queue. Writers are tokio tasks in the TCP
/// listener; the single reader is [`drain_bridge_requests`] on the Bevy
/// main thread. `Arc<Mutex<_>>` under the hood so both sides share
/// ownership across the async/sync boundary.
#[derive(Resource, Clone)]
pub struct BridgePendingQueue(pub(crate) PendingRequests);

/// Stores the `.eustress/engine.port` file path so the shutdown system
/// can remove it. We could recompute from `SpaceRoot` but caching it
/// survives Universe switches cleanly — the launcher will rewrite to
/// the new path next startup.
#[derive(Resource, Default)]
pub struct EngineBridgeHandle {
    pub port: Option<u16>,
    pub port_file: Option<std::sync::Arc<PortFile>>,
}

// ---------------------------------------------------------------------------
// Startup: bind + advertise + spawn tokio task
// ---------------------------------------------------------------------------

/// Bind the TCP listener, write the port file, and spawn the async
/// accept loop. Runs once on `Startup`.
///
/// Failure modes are logged but don't abort the engine — if the bridge
/// can't start (port exhausted, filesystem read-only, etc.) the engine
/// still runs, siblings just can't connect. This matches the LSP
/// launcher's "missing binary = no child = no error" ethos.
fn setup_engine_bridge(mut commands: Commands, queue: Res<BridgePendingQueue>) {
    // Bring up our own multi-thread runtime. Two workers is enough —
    // the bridge's workload is a handful of short-lived request
    // tasks; anything compute-heavy runs on the Bevy main thread.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("engine-bridge")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            warn!("EngineBridge: tokio runtime build failed: {} — bridge disabled", e);
            return;
        }
    };
    let handle = runtime.handle().clone();

    let queue = queue.0.clone();

    // Spawn the listener on the shared tokio runtime. `Handle::block_on`
    // isn't what we want here — we want a fire-and-forget task. The
    // result goes back via `oneshot` so the Bevy thread can record the
    // bound port without blocking.
    let (port_tx, port_rx) = std::sync::mpsc::channel::<Option<u16>>();
    handle.spawn(async move {
        match server::start_listener(queue).await {
            Ok(port) => {
                let _ = port_tx.send(Some(port));
            }
            Err(e) => {
                warn!("EngineBridge: listener failed to start: {}", e);
                let _ = port_tx.send(None);
            }
        }
    });

    // Wait briefly for the bind to complete. Done with a bounded recv
    // so a stuck tokio doesn't deadlock startup — 500 ms is enough for
    // localhost bind, which takes microseconds.
    let port = match port_rx.recv_timeout(std::time::Duration::from_millis(500)) {
        Ok(Some(p)) => p,
        _ => {
            warn!("EngineBridge: listener did not report a port within 500ms");
            commands.insert_resource(EngineBridgeHandle::default());
            return;
        }
    };

    // Write the port to `.eustress/engine.port` under whichever Universe
    // is current at startup. If `SpaceRoot` isn't set yet (bare engine
    // boot before a Universe is opened), we fall back to the Eustress
    // default location so siblings can still find us.
    let port_file = PortFile::write_for_current_universe(port).unwrap_or_else(|e| {
        warn!("EngineBridge: failed to write port file: {} — port {} still reachable via env var", e, port);
        PortFile::placeholder()
    });

    info!(
        "🔗 Engine Bridge listening on 127.0.0.1:{} (port file: {})",
        port,
        port_file.display_path()
    );

    commands.insert_resource(EngineBridgeHandle {
        port: Some(port),
        port_file: Some(Arc::new(port_file)),
    });
    commands.insert_resource(BridgeRuntime {
        handle,
        _runtime: runtime,
    });
}

// ---------------------------------------------------------------------------
// Per-frame drain: execute pending RPC requests on the Bevy main thread
// ---------------------------------------------------------------------------

/// Each frame, pull every ready request out of the shared queue,
/// dispatch it, and ship the response back through its oneshot
/// channel. Runs on the main thread so handlers get full `&mut World`
/// access (injected via `SystemState` when needed).
///
/// Budget: up to `MAX_PER_FRAME` requests per tick so a misbehaving
/// client can't stall the frame. Extras stay queued for next frame.
const MAX_PER_FRAME: usize = 64;

fn drain_bridge_requests(world: &mut World) {
    let Some(queue) = world.get_resource::<BridgePendingQueue>().cloned() else {
        return;
    };

    let mut drained = queue.0.drain(MAX_PER_FRAME);
    if drained.is_empty() {
        return;
    }

    for pending in drained.drain(..) {
        let response = match pending.request.method {
            MethodName::Ping => protocol::handlers::ping(&pending.request),
            MethodName::SimRead => protocol::handlers::sim_read(world, &pending.request),
            MethodName::EcsQuery => protocol::handlers::ecs_query(world, &pending.request),
            MethodName::ToolsList => protocol::handlers::tools_list(world, &pending.request),
            MethodName::ToolsCall => protocol::handlers::tools_call(world, &pending.request),
            MethodName::Unknown(ref name) => {
                // Unknown method — return a JSON-RPC "method not found"
                // error rather than crashing the handler.
                BridgeResponse::error(
                    pending.request.id.clone(),
                    BridgeError::method_not_found(name),
                )
            }
        };

        // Deliver the response. `send` returns `Err` only when the
        // receiver is gone (client dropped the connection mid-flight),
        // which we log once per drop and otherwise ignore.
        if pending.responder.send(response).is_err() {
            debug!("EngineBridge: client dropped before response could be delivered");
        }
    }
}
