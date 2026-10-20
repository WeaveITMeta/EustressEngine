//! Which engine a simulation tool talks to, and how.
//!
//! Several engines can have Spaces of one Universe open at once (the
//! `eustress open` fan-out: one engine per design variant, mission or
//! task). A simulation tool therefore has to name ONE of them, and send its
//! commands and read its answers through files that engine alone owns:
//!
//! * `<workspace>/.eustress/instances/<pid>/sim-commands.jsonl` — the
//!   engine's private command queue (drained by that engine only),
//! * `<workspace>/.eustress/instances/<pid>/snapshot.json` — its private
//!   runtime snapshot: play state, sim values, and its run ledger.
//!
//! Files, not the Engine Bridge, because these tools also run INSIDE an
//! engine (Workshop, bridge `tools.call`), where a TCP round-trip to their
//! own bridge would deadlock: `tools.call` executes on the main thread that
//! would have to answer it.
//!
//! [`resolve`] picks the engine:
//!
//! 1. `pid` in the tool input — that instance;
//! 2. `port` in the tool input — the instance with that bridge port;
//! 3. the calling process itself, when the tool runs inside an engine;
//! 4. the Universe's owner — the instance whose port is in
//!    `<universe>/.eustress/engine.port`, the engine `call_engine` reaches;
//! 5. the Universe's legacy single-slot files, when no instance record
//!    matches (an engine from before the registry existed). The owner
//!    serves those too, so this reaches the same engine as step 4.
//!
//! Paths come from `eustress-bridge-client`, the one definition the engine
//! writes against.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use eustress_bridge_client::{self as bc, InstanceRecord};
use serde_json::{json, Value};

use crate::ToolContext;

/// Where one tool invocation sends its commands and reads its answers.
#[derive(Debug, Clone)]
pub struct SimRoute {
    /// The target engine's process id. `None` only for the legacy route
    /// (step 5), where the owner is unknown.
    pub pid: Option<u32>,
    /// Its Engine Bridge port, when known.
    pub port: Option<u16>,
    /// Command queue to append to.
    pub queue: PathBuf,
    /// Runtime snapshot to read.
    pub snapshot: PathBuf,
    /// The Universe's shared telemetry log.
    pub telemetry: PathBuf,
    /// The Universe the target engine has open.
    pub universe: PathBuf,
    /// The Space the target engine has open, if its record says.
    pub space: Option<PathBuf>,
    /// How the target was chosen: `pid`, `port`, `self`, `owner`, `universe`.
    pub via: &'static str,
    /// Other registered engines with the same Universe open.
    pub others: Vec<InstanceRecord>,
}

impl SimRoute {
    /// A line telling the caller that its command went to the owner while
    /// other engines share the Universe — only when the target was chosen
    /// implicitly, since an explicit `pid`/`port` needs no explanation.
    pub fn note(&self) -> Option<String> {
        if !matches!(self.via, "owner" | "universe") || self.others.is_empty() {
            return None;
        }
        let pids: Vec<String> = self.others.iter().map(|r| r.pid.to_string()).collect();
        let target = match self.pid {
            Some(pid) => format!("the Universe's owner, pid {pid}"),
            None => "the Universe's owner".to_owned(),
        };
        Some(format!(
            "Note: {} other engine(s) have this Universe open (pid {}). This went to {target}; \
             pass `pid` to target a specific engine.",
            self.others.len(),
            pids.join(", ")
        ))
    }

    /// Structured description of the target, for `structured_data`.
    pub fn describe(&self) -> Value {
        json!({
            "pid": self.pid,
            "port": self.port,
            "via": self.via,
            "space": self.space.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "universe": self.universe.to_string_lossy(),
            "other_engines": self.others.iter().map(|r| json!({
                "pid": r.pid,
                "port": r.port,
                "space": r.space.as_ref().map(|p| p.to_string_lossy().into_owned()),
            })).collect::<Vec<_>>(),
        })
    }

    /// `pid 1234` / `the Universe's engine`, for messages.
    pub fn label(&self) -> String {
        match self.pid {
            Some(pid) => format!("engine pid {pid}"),
            None => "the Universe's engine".to_owned(),
        }
    }

    /// True when the tool is executing inside the engine it targets.
    pub fn is_self(&self) -> bool {
        self.via == "self"
    }

    /// Append one command and return its ticket. The engine records an
    /// acknowledgement for the ticket in its run ledger, and a `run`
    /// ticket is linked to the run it started — see [`await_run`].
    pub fn queue(&self, mut cmd: Value) -> Result<String, String> {
        let ticket = new_ticket();
        if let Value::Object(ref mut m) = cmd {
            m.insert("id".into(), Value::from(ticket.clone()));
            m.insert("queued_at".into(), Value::from(chrono::Utc::now().to_rfc3339()));
        }
        bc::append_json_line(&self.queue, &cmd)
            .map_err(|e| format!("queue {}: {e}", self.queue.display()))?;
        if matches!(cmd.get("op").and_then(|v| v.as_str()), Some("run" | "run_simulation")) {
            if let Ok(mut last) = LAST_RUN_TICKET.lock() {
                last.insert(self.queue.clone(), ticket.clone());
            }
        }
        Ok(ticket)
    }

    /// The ticket of the last `run` this process queued on this route.
    pub fn last_run_ticket(&self) -> Option<String> {
        LAST_RUN_TICKET.lock().ok()?.get(&self.queue).cloned()
    }
}

/// Last `run` ticket this process queued, per queue file — lets
/// `await_simulation` with no arguments wait for the run the caller just
/// started even before the engine has drained the command.
static LAST_RUN_TICKET: std::sync::LazyLock<Mutex<HashMap<PathBuf, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn new_ticket() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = chrono::Utc::now().timestamp_millis();
    format!("t{}-{ms}-{n}", std::process::id())
}

/// The `pid` / `port` targeting properties every simulation tool accepts,
/// merged into a tool's input schema.
pub fn with_target_props(mut schema: Value) -> Value {
    if let Some(props) = schema.get_mut("properties").and_then(|p| p.as_object_mut()) {
        props.insert(
            "pid".into(),
            json!({
                "type": "integer",
                "description": "Target one engine by process id when several have this Universe open (ids from `eustress instances`, or `other_engines` in any sim tool result). Default: the engine this tool runs in, else the Universe's owner."
            }),
        );
        props.insert(
            "port".into(),
            json!({
                "type": "integer",
                "description": "Target one engine by its Engine Bridge port instead of its pid."
            }),
        );
    }
    schema
}

// ─────────────────────────────────────────────────────────────────────────────
// Resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Pick the engine this invocation targets. See the module docs for the
/// order. `Err` when an explicit `pid`/`port` names no live engine.
pub fn resolve(input: &Value, ctx: &ToolContext) -> Result<SimRoute, String> {
    let workspaces = candidate_workspaces(&ctx.universe_root);

    if let Some(pid) = int_arg(input, "pid") {
        let pid = u32::try_from(pid).map_err(|_| format!("pid {pid} is out of range"))?;
        let (ws, rec) = find_record(&workspaces, |r| r.pid == pid).ok_or_else(|| {
            format!(
                "no engine with pid {pid} is registered under {}. `eustress instances` lists the running ones.",
                list_workspaces(&workspaces)
            )
        })?;
        require_live(&rec)?;
        let route = instance_route(&ws, rec, "pid", &ctx.universe_root);
        require_instance_ipc(&route)?;
        return Ok(route);
    }

    if let Some(port) = int_arg(input, "port") {
        let port = u16::try_from(port).map_err(|_| format!("port {port} is out of range"))?;
        let (ws, rec) = find_record(&workspaces, |r| r.port == port).ok_or_else(|| {
            format!(
                "no engine with bridge port {port} is registered under {}. `eustress instances` lists the running ones.",
                list_workspaces(&workspaces)
            )
        })?;
        require_live(&rec)?;
        let route = instance_route(&ws, rec, "port", &ctx.universe_root);
        require_instance_ipc(&route)?;
        return Ok(route);
    }

    // Running inside an engine: target that engine. The port check rules
    // out a stale record left by a dead engine that happened to have this
    // process's pid; an engine's own listener always accepts.
    let own = std::process::id();
    if let Some((ws, rec)) = find_record(&workspaces, |r| r.pid == own) {
        if bc::port_is_live(rec.port) {
            return Ok(instance_route(&ws, rec, "self", &ctx.universe_root));
        }
    }

    let universe = ctx.universe_root.clone();
    if let Some(port) = bc::read_port_file(&universe.join(".eustress").join("engine.port")) {
        if let Some((ws, rec)) = find_record(&workspaces, |r| r.port == port) {
            let route = instance_route(&ws, rec, "owner", &universe);
            // Only an engine that publishes a per-instance snapshot drains a
            // per-instance queue. An older owner (or one whose port is dead
            // and about to be re-claimed) is reached through the Universe's
            // files, which every owner serves.
            if route.snapshot.is_file() && bc::port_is_live(port) {
                return Ok(route);
            }
            return Ok(universe_route(universe, route.others));
        }
    }

    let others = workspaces
        .iter()
        .flat_map(|ws| bc::list_instances_unchecked(ws))
        .filter(|r| r.universe.as_deref() == Some(universe.as_path()))
        .collect();
    Ok(universe_route(universe, others))
}

/// The Universe's legacy files, served by whichever engine owns it.
fn universe_route(universe: PathBuf, others: Vec<InstanceRecord>) -> SimRoute {
    SimRoute {
        pid: None,
        port: None,
        queue: bc::universe_sim_commands_path(&universe),
        snapshot: bc::universe_snapshot_path(&universe),
        telemetry: bc::universe_telemetry_path(&universe),
        universe,
        space: None,
        via: "universe",
        others,
    }
}

/// An explicitly named engine must drain its per-instance queue, or the
/// commands would be accepted into a file nobody reads. It proves it does
/// by publishing its per-instance snapshot (within a frame of its Space
/// loading), so give a freshly opened engine a moment before refusing.
fn require_instance_ipc(route: &SimRoute) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !route.snapshot.is_file() {
        if Instant::now() >= deadline {
            return Err(format!(
                "{} has not published its per-instance snapshot ({}). Either its Space is \
                 still loading (retry in a moment), or it runs a build without per-instance \
                 sim commands: restart it on a current build, or omit pid/port to reach \
                 the Universe's owner.",
                route.label(),
                route.snapshot.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn instance_route(ws: &Path, rec: InstanceRecord, via: &'static str, fallback_universe: &Path) -> SimRoute {
    let universe = rec
        .universe
        .clone()
        .unwrap_or_else(|| fallback_universe.to_path_buf());
    let others = bc::list_instances_unchecked(ws)
        .into_iter()
        .filter(|r| r.pid != rec.pid && r.universe.as_deref() == Some(universe.as_path()))
        .collect();
    SimRoute {
        pid: Some(rec.pid),
        port: Some(rec.port),
        queue: bc::instance_sim_commands_path(ws, rec.pid),
        snapshot: bc::instance_snapshot_path(ws, rec.pid),
        telemetry: bc::universe_telemetry_path(&universe),
        universe,
        space: rec.space.clone(),
        via,
        others,
    }
}

/// The registry lives beside the Universes: the context Universe's parent
/// first, then the default workspace (a tool whose context names a
/// Universe outside it can still reach engines registered there).
fn candidate_workspaces(universe: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(ws) = universe.parent() {
        out.push(ws.to_path_buf());
    }
    let default = bc::default_workspace_root();
    if !out.contains(&default) {
        out.push(default);
    }
    out
}

fn find_record(
    workspaces: &[PathBuf],
    pred: impl Fn(&InstanceRecord) -> bool,
) -> Option<(PathBuf, InstanceRecord)> {
    workspaces.iter().find_map(|ws| {
        bc::list_instances_unchecked(ws)
            .into_iter()
            .find(|r| pred(r))
            .map(|r| (ws.clone(), r))
    })
}

/// A record whose engine is gone is a queue nobody drains: refuse it
/// rather than accept commands into it.
fn require_live(rec: &InstanceRecord) -> Result<(), String> {
    if rec.pid == std::process::id() || bc::port_is_live(rec.port) {
        Ok(())
    } else {
        Err(format!(
            "engine pid {} (port {}) is registered but not running: it exited without \
             cleaning up. `eustress instances` prunes stale records.",
            rec.pid, rec.port
        ))
    }
}

fn list_workspaces(workspaces: &[PathBuf]) -> String {
    workspaces
        .iter()
        .map(|w| w.display().to_string())
        .collect::<Vec<_>>()
        .join(" or ")
}

fn int_arg(input: &Value, key: &str) -> Option<i64> {
    let v = input.get(key)?;
    v.as_i64()
        .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot
// ─────────────────────────────────────────────────────────────────────────────

/// One read of a target engine's runtime snapshot.
///
/// Parsed loosely as JSON to avoid a structural dependency on the engine's
/// `RuntimeSnapshot` type. Fields beyond `play_state` / `sim_values` are
/// absent from snapshots written by engines that predate them.
#[derive(Debug, Clone)]
pub struct SnapshotReading {
    pub sim_values: BTreeMap<String, f64>,
    pub play_state: String,
    pub age_ms: u128,
    pub pid: Option<u32>,
    pub space: Option<String>,
    pub tick: Option<u64>,
    pub sim_time_s: Option<f64>,
    /// The engine's run ledger (`pending`, `current`, `last`, `completed`,
    /// `acks`).
    pub runs: Option<Value>,
}

impl SnapshotReading {
    /// The run in progress (or about to start), if any.
    pub fn active_run_id(&self) -> Option<u64> {
        let runs = self.runs.as_ref()?;
        runs.get("current")
            .and_then(|c| c.get("run_id"))
            .or_else(|| runs.get("pending").and_then(|p| p.get("run_id")))
            .and_then(|v| v.as_u64())
    }
}

pub fn read_snapshot(route: &SimRoute) -> Result<SnapshotReading, String> {
    let path = &route.snapshot;
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(match route.pid {
                Some(pid) => format!(
                    "engine pid {pid} has not written its runtime snapshot yet ({} is missing). \
                     It is written ~4 Hz once a Space is open, in Edit or Play; retry in a moment.",
                    path.display()
                ),
                None => {
                    // Distinguish "engine not running" from "running, no
                    // snapshot yet": `engine.port` is written at bridge
                    // Startup and removed on shutdown.
                    let port_file = route.universe.join(".eustress").join("engine.port");
                    if port_file.exists() {
                        format!(
                            "engine is running but no runtime snapshot has been written yet \
                             (missing {}). It is written ~4 Hz once a Space is open in either \
                             Edit or Play mode; retry in a moment.",
                            path.display()
                        )
                    } else {
                        format!(
                            "engine does not appear to be running — no {} and no live \
                             snapshot at {}. Launch the engine on this Universe first.",
                            port_file.display(),
                            path.display()
                        )
                    }
                }
            });
        }
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    let val: Value = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "runtime snapshot at {} is unparseable (mid-write or corrupt): {e}",
            path.display()
        )
    })?;

    let sim_values = val
        .get("sim_values")
        .and_then(|v| v.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default();
    let play_state = val
        .get("play_state")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    // `generated_at` is RFC-3339. Report 0 when missing or unparseable —
    // better than failing the read.
    let age_ms = val
        .get("generated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            let diff = chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc));
            diff.num_milliseconds().max(0) as u128
        })
        .unwrap_or(0);

    Ok(SnapshotReading {
        sim_values,
        play_state,
        age_ms,
        pid: val.get("pid").and_then(|v| v.as_u64()).and_then(|p| u32::try_from(p).ok()),
        space: val.get("space").and_then(|v| v.as_str()).map(str::to_owned),
        tick: val.get("tick").and_then(|v| v.as_u64()),
        sim_time_s: val.get("sim_time_s").and_then(|v| v.as_f64()),
        runs: val.get("runs").cloned().filter(|r| !r.is_null()),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Waiting on a run
// ─────────────────────────────────────────────────────────────────────────────

/// Which run to wait for.
#[derive(Debug, Clone)]
pub enum RunTarget {
    /// The run a queued `run` command with this ticket started (or joined).
    Ticket(String),
    /// A run id from a previous result.
    RunId(u64),
    /// Whatever is running now; else the last `run` this process queued on
    /// the route; else nothing — the last completed run is returned.
    Active,
}

/// A finished wait.
#[derive(Debug, Clone)]
pub enum Awaited {
    /// The engine's ledger record for the run: `run_id`, `end_reason`,
    /// `ticks`, `sim_seconds`, `recording`, `final_values`, ….
    Run(Value),
    /// An engine without a run ledger stopped playing. Only its live values
    /// are available — and after a stop those have been reset.
    LegacyStopped(SnapshotReading),
}

impl Awaited {
    pub fn run_id(&self) -> Option<u64> {
        match self {
            Awaited::Run(r) => r.get("run_id").and_then(|v| v.as_u64()),
            Awaited::LegacyStopped(_) => None,
        }
    }

    /// Final values: the ledger's capture from the moment the run ended.
    pub fn final_values(&self) -> BTreeMap<String, f64> {
        match self {
            Awaited::Run(r) => r
                .get("final_values")
                .and_then(|v| v.as_object())
                .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n))).collect())
                .unwrap_or_default(),
            Awaited::LegacyStopped(s) => s.sim_values.clone(),
        }
    }

    pub fn end_reason(&self) -> String {
        match self {
            Awaited::Run(r) => r
                .get("end_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("stopped")
                .to_owned(),
            Awaited::LegacyStopped(s) => format!("left Play ({})", s.play_state),
        }
    }
}

/// Wait for a run on `route` to end. Polls the target's snapshot, which the
/// engine rewrites at 4 Hz.
pub fn await_run(
    route: &SimRoute,
    target: RunTarget,
    timeout: Duration,
    ctx: &ToolContext,
) -> Result<Awaited, String> {
    // A tool executing on the engine's own main thread (bridge `tools.call`)
    // would sleep on the very thread that advances the simulation and
    // writes the snapshot: the run could never end. Refuse instead of
    // freezing the engine for the whole timeout.
    if route.is_self() && std::thread::current().name() == Some("main") {
        return Err(
            "cannot wait for a run on the engine's own main thread, because the \
             simulation cannot advance while this call blocks it. Wait from outside the \
             engine (the MCP server, or `eustress bridge sim-await`), or poll \
             get_simulation_state."
                .to_owned(),
        );
    }

    let poll = Duration::from_millis(250);
    let start = Instant::now();
    let mut target = target;
    let mut run_id: Option<u64> = None;
    let mut first_poll = true;

    loop {
        if ctx.is_cancelled() {
            return Err(format!(
                "Cancelled after {:.1}s of waiting. The simulation was NOT stopped — call \
                 stop_simulation if you want it to end.",
                start.elapsed().as_secs_f64()
            ));
        }
        if start.elapsed() >= timeout {
            let which = run_id.map(|id| format!("run #{id}")).unwrap_or_else(|| "the run".into());
            return Err(format!(
                "Timeout after {:.1}s: {which} on {} has not finished.",
                timeout.as_secs_f64(),
                route.label()
            ));
        }

        let snap = match read_snapshot(route) {
            Ok(s) => s,
            Err(_) => {
                std::thread::sleep(poll);
                continue;
            }
        };

        let Some(runs) = snap.runs.as_ref() else {
            // An engine without a run ledger: the best signal is leaving
            // Play. Give a just-queued command a moment to be picked up
            // before trusting a "not playing" reading.
            if snap.play_state != "Playing" && start.elapsed() >= Duration::from_secs(1) {
                return Ok(Awaited::LegacyStopped(snap));
            }
            std::thread::sleep(poll);
            continue;
        };

        if first_poll {
            first_poll = false;
            if matches!(target, RunTarget::Active) {
                if let Some(id) = snap.active_run_id() {
                    target = RunTarget::RunId(id);
                } else if let Some(t) = route.last_run_ticket() {
                    target = RunTarget::Ticket(t);
                } else if let Some(last) = runs.get("last").filter(|l| !l.is_null()) {
                    return Ok(Awaited::Run(last.clone()));
                } else {
                    return Err(format!("{} has no run in progress and none completed.", route.label()));
                }
            }
        }

        if run_id.is_none() {
            run_id = match &target {
                RunTarget::RunId(id) => Some(*id),
                RunTarget::Ticket(t) => resolve_ticket(runs, t)?,
                RunTarget::Active => None,
            };
        }

        if let Some(id) = run_id {
            if let Some(done) = find_completed(runs, id) {
                return Ok(Awaited::Run(done));
            }
            let active = snap.active_run_id();
            let oldest_kept = runs
                .get("completed")
                .and_then(|c| c.as_array())
                .and_then(|c| c.first())
                .and_then(|r| r.get("run_id"))
                .and_then(|v| v.as_u64());
            if active != Some(id) && oldest_kept.is_some_and(|o| id < o) {
                return Err(format!(
                    "run #{id} on {} has ended, but its record has already rotated out of the \
                     snapshot's recent-runs window.",
                    route.label()
                ));
            }
        }

        std::thread::sleep(poll);
    }
}

/// The run a ticket started, once the engine has processed the ticket.
/// `Ok(None)` while it is still queued.
fn resolve_ticket(runs: &Value, ticket: &str) -> Result<Option<u64>, String> {
    if let Some(ack) = runs
        .get("acks")
        .and_then(|a| a.as_array())
        .and_then(|a| a.iter().rev().find(|a| a.get("ticket").and_then(|t| t.as_str()) == Some(ticket)))
    {
        if ack.get("ok").and_then(|v| v.as_bool()) == Some(false) {
            let detail = ack.get("detail").and_then(|v| v.as_str()).unwrap_or("rejected");
            return Err(format!("the engine rejected the command: {detail}"));
        }
        return match ack.get("run_id").and_then(|v| v.as_u64()) {
            Some(id) => Ok(Some(id)),
            None => Err(format!(
                "the engine processed the command but it started no run ({})",
                ack.get("detail").and_then(|v| v.as_str()).unwrap_or("no run")
            )),
        };
    }
    // Acks rotate; a run record also carries the ticket that started it.
    let from_record = ["pending", "current", "last"]
        .iter()
        .filter_map(|k| runs.get(*k))
        .chain(runs.get("completed").and_then(|c| c.as_array()).into_iter().flatten())
        .find(|r| r.get("ticket").and_then(|t| t.as_str()) == Some(ticket))
        .and_then(|r| r.get("run_id"))
        .and_then(|v| v.as_u64());
    Ok(from_record)
}

fn find_completed(runs: &Value, id: u64) -> Option<Value> {
    runs.get("completed")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
        .chain(runs.get("last"))
        .find(|r| r.get("run_id").and_then(|v| v.as_u64()) == Some(id))
        .cloned()
}

// ─────────────────────────────────────────────────────────────────────────────
// Telemetry
// ─────────────────────────────────────────────────────────────────────────────

/// Which telemetry lines to keep. Lines written by engines that predate
/// tagging carry no `pid`/`run_id`; they pass a filter only when it does
/// not constrain that field.
#[derive(Debug, Clone, Default)]
pub struct TelemetryFilter {
    pub pid: Option<u32>,
    pub run_id: Option<u64>,
    pub since: Option<chrono::DateTime<chrono::Utc>>,
}

impl TelemetryFilter {
    pub fn matches(&self, entry: &Value) -> bool {
        if let Some(pid) = self.pid {
            if entry.get("pid").and_then(|v| v.as_u64()) != Some(u64::from(pid)) {
                return false;
            }
        }
        if let Some(run_id) = self.run_id {
            if entry.get("run_id").and_then(|v| v.as_u64()) != Some(run_id) {
                return false;
            }
        }
        if let Some(since) = self.since {
            let fresh = entry
                .get("t")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_some_and(|t| t.with_timezone(&chrono::Utc) >= since);
            if !fresh {
                return false;
            }
        }
        true
    }
}

/// Every telemetry line in `path` that passes `filter`, oldest first.
pub fn read_telemetry(path: &Path, filter: &TelemetryFilter) -> Vec<Value> {
    let Ok(raw) = std::fs::read_to_string(path) else { return Vec::new() };
    raw.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| filter.matches(entry))
        .collect()
}
