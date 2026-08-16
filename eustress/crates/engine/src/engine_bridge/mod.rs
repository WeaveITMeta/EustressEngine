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
mod self_test;
mod server;
#[cfg(unix)]
mod unix_socket_file;

pub use protocol::{BridgeRequest, BridgeResponse, BridgeError, MethodName};

use server::PendingRequests;
use port_file::PortFile;
#[cfg(unix)]
use unix_socket_file::UnixSocketFile;

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
            .init_resource::<SimStepJob>()
            .add_systems(Startup, setup_engine_bridge)
            // `pump_sim_step` runs after the drain so a step requested this
            // frame starts advancing this frame rather than next.
            .add_systems(
                Update,
                (
                    (drain_bridge_requests, pump_sim_step).chain(),
                    resync_port_file_to_space,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// sim.step — advanced across frames instead of in one blocking burst
// ---------------------------------------------------------------------------

/// Wall-clock a `sim.step` job may spend inside a single frame.
///
/// `sim.step` runs fixed-timestep schedules synchronously, so the whole
/// request used to be serviced in one drain call: 10 000 ticks meant the
/// engine stopped rendering, stopped answering other RPCs, and looked hung
/// for as long as it took. Budgeting per frame keeps the window alive and the
/// bridge responsive — you can `capture_viewport` mid-rollout — while still
/// running far faster than real time. The engine visibly slows during a long
/// step; that is the trade, and it beats appearing frozen.
///
/// Determinism is unaffected: the same total tick count runs in the same
/// order, just spread over more frames.
const SIM_STEP_FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(100);

/// A `sim.step` request being advanced across frames.
struct InflightSimStep {
    id: serde_json::Value,
    /// Ticks still to run.
    remaining: u64,
    /// Ticks originally requested, for the response.
    requested: u64,
    /// Frames this job has spanned, for the response.
    frames: u64,
    responder: tokio::sync::oneshot::Sender<BridgeResponse>,
}

/// The single in-flight `sim.step`, if any.
///
/// Deliberately one at a time: concurrent stepped rollouts would interleave
/// ticks and destroy the reproducibility that stepped mode exists to provide.
#[derive(Resource, Default)]
pub struct SimStepJob(Option<InflightSimStep>);

/// Install a `sim.step` request as a background job, or reject it when one is
/// already running.
fn begin_sim_step(world: &mut World, pending: server::Pending) {
    let ticks = protocol::handlers::sim_step_ticks(&pending.request);
    let mut job = world.resource_mut::<SimStepJob>();
    if job.0.is_some() {
        let _ = pending.responder.send(BridgeResponse::error(
            pending.request.id.clone(),
            BridgeError::invalid_params(
                "a sim.step is already in progress — wait for it to finish before starting another",
            ),
        ));
        return;
    }
    job.0 = Some(InflightSimStep {
        id: pending.request.id.clone(),
        remaining: ticks,
        requested: ticks,
        frames: 0,
        responder: pending.responder,
    });
}

// ---------------------------------------------------------------------------
// db.export_toml — DB scan on the main thread, decode + write off it
// ---------------------------------------------------------------------------

/// Default cap on instances per export. Generous enough for a real world,
/// small enough that a fat-fingered call doesn't produce a million folders.
const EXPORT_DEFAULT_LIMIT: usize = 5_000;
/// Hard ceiling regardless of what the caller asks for.
const EXPORT_MAX_LIMIT: usize = 200_000;

/// Start a `db.export_toml` request.
///
/// Split deliberately: reading cores out of Fjall needs the main thread (the
/// active-DB handle is owned there), but decoding rkyv and writing thousands
/// of small files does not. Doing the whole thing inline would freeze the
/// engine for the length of the export — the same failure `sim.step` used to
/// have. So the scan happens here and everything after it on a worker, which
/// answers the client when the files are actually on disk.
fn begin_db_export(world: &mut World, pending: server::Pending) {
    #[cfg(not(feature = "world-db"))]
    {
        let _ = world;
        let _ = pending.responder.send(BridgeResponse::error(
            pending.request.id.clone(),
            BridgeError::internal(
                "db.export_toml: this build has the world-db feature disabled, so there is no \
                 database to export",
            ),
        ));
    }
    #[cfg(feature = "world-db")]
    {
        use crate::space::db_export::{
            dir_has_content, resolve_output_dir, ExportLayout, ExportPlan,
        };

        let req = &pending.request;
        let id = req.id.clone();

        let prepared = (|| -> Result<(Vec<(u64, Vec<u8>)>, ExportPlan, usize, bool), String> {
            if !crate::space::active_db::is_active() {
                return Err("no active world DB — export needs a converted (.eustress) Space".into());
            }
            let space_root = world
                .get_resource::<crate::space::SpaceRoot>()
                .ok_or("no SpaceRoot resource — no Space is open")?
                .0
                .clone();

            let layout = match req.params.get("layout").and_then(|v| v.as_str()) {
                Some(s) => ExportLayout::parse(s)?,
                None => ExportLayout::Folders,
            };
            let output_dir = resolve_output_dir(
                &space_root,
                req.params.get("output_dir").and_then(|v| v.as_str()),
            )?;
            let overwrite = req
                .params
                .get("overwrite")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !overwrite && dir_has_content(&output_dir) {
                return Err(format!(
                    "{} already has content — pass overwrite:true to export on top of it",
                    output_dir.display()
                ));
            }
            let class_filter = req
                .params
                .get("class")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or(EXPORT_DEFAULT_LIMIT)
                .clamp(1, EXPORT_MAX_LIMIT);

            // Region-scoped scans read only the relevant Morton cells; an
            // unscoped one walks the whole partition, which is the part of an
            // export the main thread genuinely has to pay for.
            let mut cores = match parse_region_cells(&req.params) {
                Some((cx, cy, cz)) => {
                    crate::space::active_db::iter_instance_cores_in_region(cx, cy, cz)
                }
                None => crate::space::active_db::iter_instance_cores(),
            };
            let scanned = cores.len();
            let truncated = scanned > limit;
            cores.truncate(limit);

            Ok((
                cores,
                ExportPlan { output_dir, layout, class_filter },
                scanned,
                truncated,
            ))
        })();

        let (cores, plan, scanned, truncated) = match prepared {
            Ok(v) => v,
            Err(e) => {
                let _ = pending
                    .responder
                    .send(BridgeResponse::error(id, BridgeError::invalid_params(e)));
                return;
            }
        };

        let layout_name = plan.layout.as_str();
        let responder = pending.responder;
        std::thread::spawn(move || {
            let response = match crate::space::db_export::write_export(&cores, &plan) {
                Ok(report) => BridgeResponse::ok(
                    id,
                    serde_json::json!({
                        "exported":     report.written,
                        "scanned":      scanned,
                        "filtered_out": report.filtered_out,
                        "undecodable":  report.undecodable,
                        // True when the limit cut the scan short: some
                        // instances are NOT in the output. Narrow with
                        // `region`/`class` or raise `limit` for full coverage.
                        "truncated":    truncated,
                        "output_dir":   report.output_dir.to_string_lossy(),
                        "layout":       layout_name,
                    }),
                ),
                Err(e) => BridgeResponse::error(id, BridgeError::internal(e)),
            };
            if responder.send(response).is_err() {
                debug!("EngineBridge: db.export_toml caller disconnected before it finished");
            }
        });
    }
}

/// Convert an optional `region: {min:[x,y,z], max:[x,y,z]}` world-space AABB
/// into the inclusive 21-bit Morton cell box the DB region scan expects.
/// Returns `None` when no usable region was supplied, meaning "scan it all".
#[cfg(feature = "world-db")]
fn parse_region_cells(
    params: &serde_json::Value,
) -> Option<((u32, u32), (u32, u32), (u32, u32))> {
    use eustress_worlddb::keys::world_to_cell;
    const CHUNK: f32 = 256.0;

    let region = params.get("region")?;
    let axis = |key: &str| -> Option<[f32; 3]> {
        let a = region.get(key)?.as_array()?;
        if a.len() < 3 {
            return None;
        }
        Some([
            a[0].as_f64()? as f32,
            a[1].as_f64()? as f32,
            a[2].as_f64()? as f32,
        ])
    };
    let min = axis("min")?;
    let max = axis("max")?;
    // Tolerate a caller that passes the corners the other way round.
    let span = |i: usize| {
        let (lo, hi) = if min[i] <= max[i] { (min[i], max[i]) } else { (max[i], min[i]) };
        (world_to_cell(lo, CHUNK), world_to_cell(hi, CHUNK))
    };
    Some((span(0), span(1), span(2)))
}

/// Advance the in-flight `sim.step` by one frame's budget, answering the
/// client once every requested tick has run.
fn pump_sim_step(world: &mut World) {
    // Taken out of the resource so `world` is free for `run_schedule`, then
    // put back if there's more to do.
    let Some(mut job) = world.resource_mut::<SimStepJob>().0.take() else {
        return;
    };

    let ran = protocol::handlers::pump_fixed_ticks(world, job.remaining, SIM_STEP_FRAME_BUDGET);
    job.remaining = job.remaining.saturating_sub(ran);
    job.frames += 1;

    if job.remaining > 0 {
        world.resource_mut::<SimStepJob>().0 = Some(job);
        return;
    }

    let response = BridgeResponse::ok(
        job.id.clone(),
        serde_json::json!({
            "stepped": job.requested,
            "sim_seconds": job.requested as f64 / 60.0,
            // How many frames the step spanned — 1 means it fit in a single
            // budget, higher means the engine stayed live while it worked.
            "frames": job.frames,
        }),
    );
    if job.responder.send(response).is_err() {
        debug!("EngineBridge: sim.step caller disconnected before it finished");
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
    /// The Unix socket transport's pointer file + real (short, temp-dir)
    /// bind path (unix platforms only) — `None` on other platforms or if
    /// the bind failed. See `unix_socket_file` module docs for why this
    /// isn't just "the socket path under the Universe".
    #[cfg(unix)]
    pub unix_socket: Option<std::sync::Arc<UnixSocketFile>>,
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
fn setup_engine_bridge(
    mut commands: Commands,
    queue: Res<BridgePendingQueue>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
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

    // Bind SYNCHRONOUSLY on the runtime via `block_on`. Bind is a near-instant
    // syscall, so this returns the real OS-assigned port deterministically —
    // unlike the old `spawn` + 500 ms `recv_timeout` handshake, which on 0.19's
    // heavier startup routinely timed out before a freshly created worker thread
    // ran the bind. On timeout the old code early-returned and DROPPED the local
    // `runtime`, shutting tokio down and aborting the (possibly-just-bound)
    // listener — leaving the bridge unbound, no port file, and siblings hitting
    // connect timeouts. Binding here removes the race entirely.
    let (listener, port) = match handle.block_on(server::bind_listener()) {
        Ok(bound) => bound,
        Err(e) => {
            error!("EngineBridge: listener failed to bind 127.0.0.1:0: {} — bridge disabled", e);
            commands.insert_resource(EngineBridgeHandle::default());
            return;
        }
    };

    // Spawn the long-lived accept loop on the runtime. The runtime is moved into
    // `BridgeRuntime` (below) and lives as long as the engine, so this task is
    // never aborted by an early drop.
    handle.spawn(server::run_accept_loop(listener, queue.clone()));

    // Write the port to `.eustress/engine.port` under whichever Universe
    // is current at startup. If `SpaceRoot` isn't set yet (bare engine
    // boot before a Universe is opened), we fall back to the Eustress
    // default location so siblings can still find us.
    // Resolve the Universe from the actually-loaded Space when we have
    // one (covers `--universe`, runtime Space switches, non-default
    // Universe names); otherwise fall back to a disk scan. Either path is
    // OneDrive-avoiding, unlike the old hardcoded `dirs::document_dir()`
    // guess that wrote the port file where no sibling could find it.
    let universe = port_file::resolve_universe_root(space_root.as_ref().map(|s| s.0.as_path()));
    let port_file = PortFile::write_for_universe(&universe, port).unwrap_or_else(|e| {
        warn!("EngineBridge: failed to write port file: {} — port {} still reachable via env var", e, port);
        PortFile::placeholder()
    });

    info!(
        "🔗 Engine Bridge listening on 127.0.0.1:{} (port file: {})",
        port,
        port_file.display_path()
    );

    // Also bind a Unix domain socket, advertised via a pointer file at
    // `<universe>/.eustress/engine.sock` (see `unix_socket_file` docs for
    // why the socket itself lives elsewhere, under the system temp dir).
    // Some MCP-connector sandboxes refuse AF_INET loopback outright but
    // permit filesystem-domain sockets under a writable root — see the
    // module docs on `server::handle_connection`. Best-effort: failure
    // just means TCP-only for this run, same "bridge disabled, engine
    // still runs" ethos as a failed TCP bind above.
    #[cfg(unix)]
    let unix_socket = {
        let bind_path = unix_socket_file::bind_path_for(&universe);
        match handle.block_on(server::bind_unix_listener(&bind_path)) {
            Ok(unix_listener) => {
                handle.spawn(server::run_unix_accept_loop(unix_listener, queue.clone()));
                match UnixSocketFile::write_for_universe(&universe, &bind_path) {
                    Ok(usf) => {
                        info!(
                            "🔗 Engine Bridge also listening on Unix socket: {} (alternate transport for sandboxes that block loopback TCP)",
                            usf.display_path()
                        );
                        Some(usf)
                    }
                    Err(e) => {
                        warn!(
                            "EngineBridge: unix socket bound at {} but pointer file write failed: {} — unreachable via .eustress/engine.sock this run",
                            bind_path.display(),
                            e
                        );
                        None
                    }
                }
            }
            Err(e) => {
                warn!(
                    "EngineBridge: unix socket bind failed at {}: {} — TCP-only this run",
                    bind_path.display(),
                    e
                );
                None
            }
        }
    };

    commands.insert_resource(EngineBridgeHandle {
        port: Some(port),
        port_file: Some(Arc::new(port_file)),
        #[cfg(unix)]
        unix_socket: unix_socket.map(Arc::new),
    });

    // STARTUP SELF-TEST — connect to our own port and do one `ping` round-trip
    // so a silent regression (bound-but-not-accepting, or the drain not running)
    // screams in the log instead of surfacing hours later as a dead MCP loop.
    // Spawned (not blocking): the response is serviced by `drain_bridge_requests`
    // on a later `Update` frame, so blocking Startup on it would deadlock.
    handle.spawn(self_test::run(port));

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
        // `sim.step` is the one method that does unbounded work, so it is
        // deferred to `pump_sim_step` and answered from there once complete.
        // Running it inline would block the frame for its whole duration.
        if matches!(pending.request.method, MethodName::SimStep) {
            begin_sim_step(world, pending);
            continue;
        }
        // Likewise deferred: the export writes thousands of files, which has
        // no business happening inside a frame.
        if matches!(pending.request.method, MethodName::DbExportToml) {
            begin_db_export(world, pending);
            continue;
        }

        let response = match pending.request.method {
            MethodName::Ping => protocol::handlers::ping(&pending.request),
            MethodName::SimRead => protocol::handlers::sim_read(world, &pending.request),
            MethodName::EcsQuery => protocol::handlers::ecs_query(world, &pending.request),
            MethodName::EcsInspect => protocol::handlers::ecs_inspect(world, &pending.request),
            MethodName::ToolEquip => protocol::handlers::tool_equip(world, &pending.request),
            MethodName::SelectionSet => protocol::handlers::selection_set(world, &pending.request),
            MethodName::StateGet => protocol::handlers::state_get(world, &pending.request),
            MethodName::ActionInvoke => protocol::handlers::action_invoke(world, &pending.request),
            MethodName::ViewportCapture => protocol::handlers::viewport_capture(world, &pending.request),
            MethodName::AiCameraSetPose => protocol::handlers::ai_camera_set_pose(world, &pending.request),
            MethodName::AiCameraOrbit => protocol::handlers::ai_camera_orbit(world, &pending.request),
            MethodName::AiCameraFrame => protocol::handlers::ai_camera_frame(world, &pending.request),
            MethodName::AiCameraCapture => protocol::handlers::ai_camera_capture(world, &pending.request),
            MethodName::ToolsList => protocol::handlers::tools_list(world, &pending.request),
            MethodName::ToolsCall => protocol::handlers::tools_call(world, &pending.request),
            MethodName::EntityCreate => protocol::handlers::entity_create(world, &pending.request),
            MethodName::EntityRead => protocol::handlers::entity_read(world, &pending.request),
            MethodName::EntityUpdate => protocol::handlers::entity_update(world, &pending.request),
            MethodName::EntityDelete => protocol::handlers::entity_delete(world, &pending.request),
            MethodName::EntityFind => protocol::handlers::entity_find(world, &pending.request),
            MethodName::EntityAddTag => protocol::handlers::entity_add_tag(world, &pending.request),
            MethodName::EntityRemoveTag => protocol::handlers::entity_remove_tag(world, &pending.request),
            MethodName::EntityPromote => protocol::handlers::entity_promote(world, &pending.request),
            MethodName::EntityDemote => protocol::handlers::entity_demote(world, &pending.request),
            MethodName::OplogTail => protocol::handlers::oplog_tail(&pending.request),
            // Deferred above to `pump_sim_step`, so this arm is dead today.
            // It reports an error rather than `unreachable!` because a panic
            // here would take the whole engine down with it — and the only
            // way to reach it is someone narrowing the guard above, which is
            // exactly when you want a message instead of a crash.
            MethodName::SimStep => BridgeResponse::error(
                pending.request.id.clone(),
                BridgeError::internal("sim.step reached the synchronous dispatch path"),
            ),
            MethodName::DbExportToml => BridgeResponse::error(
                pending.request.id.clone(),
                BridgeError::internal("db.export_toml reached the synchronous dispatch path"),
            ),
            MethodName::Raycast => protocol::handlers::raycast(world, &pending.request),
            MethodName::SceneOverview => protocol::handlers::scene_overview(world, &pending.request),
            MethodName::SimBindings => protocol::handlers::sim_bindings(world, &pending.request),
            MethodName::DataBind => protocol::handlers::data_bind(world, &pending.request),
            MethodName::DataBindings => protocol::handlers::data_bindings(world, &pending.request),
            MethodName::DataUnbind => protocol::handlers::data_unbind(world, &pending.request),
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

// ---------------------------------------------------------------------------
// Re-point the port file once the Space (hence its Universe) is known
// ---------------------------------------------------------------------------

/// The bridge binds and writes `engine.port` at `Startup`, before the Space
/// finishes loading — so the initial write uses a best-effort Universe
/// guess (see [`port_file::resolve_universe_root`]). Once `SpaceRoot` is
/// inserted (default boot) or changes (runtime Space switch), re-point the
/// port file at the loaded Space's *actual* Universe and drop the stale one,
/// so siblings (MCP server, plugins) always read the port from the right
/// place.
///
/// Cheap when idle: gated on `SpaceRoot` having changed, and a no-op when
/// the file is already at the correct Universe.
fn resync_port_file_to_space(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    handle: Option<ResMut<EngineBridgeHandle>>,
    bridge_runtime: Option<Res<BridgeRuntime>>,
    queue: Res<BridgePendingQueue>,
) {
    let Some(space_root) = space_root else { return };
    if !space_root.is_changed() {
        return;
    }
    let Some(mut handle) = handle else { return };
    let Some(port) = handle.port else { return };

    let Some(universe) = crate::space::universe_root_for_path(&space_root.0) else {
        return;
    };
    let target = universe.join(".eustress").join("engine.port");

    // Already pointing at the loaded Space's Universe — nothing to do.
    let port_file_current = handle
        .port_file
        .as_ref()
        .map(|pf| pf.path() == target.as_path())
        .unwrap_or(false);

    if !port_file_current {
        match PortFile::write_for_universe(&universe, port) {
            Ok(new_pf) => {
                info!(
                    "🔗 Engine Bridge port file re-pointed to loaded Space's Universe: {}",
                    new_pf.display_path()
                );
                // Replacing the handle drops the previous `PortFile`, whose
                // `Drop` removes the stale (wrong-Universe) port file.
                handle.port_file = Some(Arc::new(new_pf));
            }
            Err(e) => warn!(
                "EngineBridge: failed to re-point port file to {}: {}",
                universe.display(),
                e
            ),
        }
    }

    // Same re-pointing for the Unix socket transport (see `unix_socket_file`
    // module docs). The pointer file's *path* is the discovery mechanism,
    // so a stale one left pointing at the old Universe would silently
    // strand siblings there — this mirrors the port-file repoint above
    // rather than skipping it just because it's more work.
    #[cfg(unix)]
    {
        let new_pointer_path = universe.join(".eustress").join("engine.sock");
        let sock_current = handle
            .unix_socket
            .as_ref()
            .map(|u| u.pointer_path() == new_pointer_path.as_path())
            .unwrap_or(false);
        if !sock_current {
            if let Some(bridge_runtime) = bridge_runtime {
                let new_bind_path = unix_socket_file::bind_path_for(&universe);
                match bridge_runtime
                    .handle
                    .block_on(server::bind_unix_listener(&new_bind_path))
                {
                    Ok(unix_listener) => {
                        bridge_runtime
                            .handle
                            .spawn(server::run_unix_accept_loop(unix_listener, queue.0.clone()));
                        match UnixSocketFile::write_for_universe(&universe, &new_bind_path) {
                            Ok(new_usf) => {
                                info!(
                                    "🔗 Engine Bridge unix socket re-pointed to loaded Space's Universe: {}",
                                    new_usf.display_path()
                                );
                                // Replacing the handle drops the previous
                                // `UnixSocketFile`, whose `Drop` removes the
                                // stale pointer file(s) AND the stale bind
                                // path. The old accept-loop task itself keeps
                                // running on its now-orphaned fd until
                                // AppExit — harmless (no sibling can discover
                                // a deleted pointer), matching the "don't
                                // hard-fail on transport churn" ethos used
                                // elsewhere in this module.
                                handle.unix_socket = Some(Arc::new(new_usf));
                            }
                            Err(e) => warn!(
                                "EngineBridge: unix socket bound at {} but pointer file re-point failed: {}",
                                new_bind_path.display(),
                                e
                            ),
                        }
                    }
                    Err(e) => warn!(
                        "EngineBridge: failed to re-bind unix socket at {}: {}",
                        new_bind_path.display(),
                        e
                    ),
                }
            }
        }
    }
}
