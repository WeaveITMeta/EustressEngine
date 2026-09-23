//! Simulation commands — the one place a run/pause/stop/set request becomes
//! engine state, and the ledger that makes every run addressable afterwards.
//!
//! Three transports feed [`apply`]:
//!
//! * Engine Bridge `sim.run` / `sim.pause` / `sim.stop` / `sim.set` —
//!   addressed by port, answered synchronously. The CLI's path.
//! * This instance's private queue,
//!   `<workspace>/.eustress/instances/<pid>/sim-commands.jsonl` — addressed by
//!   PID, fire-and-forget. The path for tools, including tools executing
//!   INSIDE this engine (which cannot use the bridge: `tools.call` runs on the
//!   main thread, so a round-trip to their own bridge would deadlock).
//! * The Universe's legacy queue, `<universe>/.eustress/sim-commands.jsonl` —
//!   drained only while this instance owns the Universe (see [`super::ipc`]).
//!
//! All three go through the same code, so a command means the same thing
//! however it arrives.
//!
//! ## Runs
//!
//! Every entry into Play from Edit is a RUN with an id; resuming from Pause
//! continues the same run. [`SimRunLedger`] records each run's configuration
//! and, when it ends, its tick count, final values, recording path, and why it
//! ended. The final values are captured in `on_play_stop` BEFORE the sim-value
//! store is cleared — the only moment they still exist. (Reading "final
//! values" from the first snapshot after stop, as the tools used to, returned
//! an already-cleared store.)
//!
//! A queued command may carry a ticket (`"id"`). The ledger records an
//! acknowledgement per ticket and links a `run` ticket to the run it started,
//! so a client that cannot hold a connection open can still wait for exactly
//! the run it asked for.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

use eustress_common::simulation::{SimulationClock, WatchPointRegistry};

use crate::play_mode::{PlayModeState, PlayModeType, StartPlayEvent, StopPlayEvent, TogglePauseEvent};

use super::ipc::SimIpc;
use super::plugin::{ScriptSimWrites, SimAutoStop, SimValuesResource};

/// Completed runs kept for `sim.state`.
const MAX_COMPLETED_RUNS: usize = 16;
/// Completed runs (with final values) published in the runtime snapshot.
/// Smaller than the in-memory history: the snapshot is rewritten at 4 Hz.
const SNAPSHOT_COMPLETED_RUNS: usize = 4;
/// Ticket acknowledgements kept.
const MAX_ACKS: usize = 64;
/// Acknowledgements published in the runtime snapshot.
const SNAPSHOT_ACKS: usize = 16;

// ─────────────────────────────────────────────────────────────────────────────
// Commands
// ─────────────────────────────────────────────────────────────────────────────

/// A simulation command, independent of how it arrived.
#[derive(Debug, Clone)]
pub enum SimCommand {
    /// Start a run from Edit, resume a paused one, or retune a running one.
    /// `None` fields keep the current value (a fresh run defaults to 1×,
    /// indefinite).
    Run { time_scale: Option<f64>, duration_s: Option<f64> },
    Pause,
    Stop,
    /// Write sim values — what a Rune `set_sim_value` would.
    Set { values: Vec<(String, f64)> },
}

impl SimCommand {
    pub fn op_name(&self) -> &'static str {
        match self {
            SimCommand::Run { .. } => "run",
            SimCommand::Pause => "pause",
            SimCommand::Stop => "stop",
            SimCommand::Set { .. } => "set",
        }
    }

    /// Parse one queue line: `{"op": "...", ...}`. Accepts the historical op
    /// names (`run_simulation`, `set_sim_value`, …) and their short forms.
    /// Returns the command and its ticket (`"id"`), if any.
    pub fn from_queue_line(line: &Value) -> Result<(SimCommand, Option<String>), String> {
        let ticket = line
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let op = line.get("op").and_then(|v| v.as_str()).unwrap_or("");
        let num = |key: &str| {
            line.get(key).and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
            })
        };
        let cmd = match op {
            "run_simulation" | "run" => SimCommand::Run {
                time_scale: num("time_scale"),
                duration_s: num("duration_s"),
            },
            "pause_simulation" | "pause" => SimCommand::Pause,
            "stop_simulation" | "stop" => SimCommand::Stop,
            "set_sim_value" | "set" => {
                let mut values = Vec::new();
                if let (Some(k), Some(v)) = (line.get("key").and_then(|v| v.as_str()), num("value")) {
                    values.push((k.to_owned(), v));
                }
                if let Some(map) = line.get("values").and_then(|v| v.as_object()) {
                    for (k, v) in map {
                        if let Some(n) = v.as_f64() {
                            values.push((k.clone(), n));
                        }
                    }
                }
                if values.is_empty() {
                    return Err(format!("'{op}' needs `key` + `value` or a `values` object"));
                }
                SimCommand::Set { values }
            }
            other => return Err(format!("unknown sim command op '{other}'")),
        };
        Ok((cmd, ticket))
    }
}

/// Who sent a command — carried into the ledger so a run records its origin.
#[derive(Debug, Clone)]
pub struct Origin {
    /// `"bridge"`, `"instance-queue"`, `"universe-queue"`.
    pub source: &'static str,
    pub ticket: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Ledger
// ─────────────────────────────────────────────────────────────────────────────

/// One run: configuration at start, outcome once it ends.
#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub run_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// Transport that started it, or `"play"` for the Play button, `--play`,
    /// headless autoplay, or any other entry into Play not made by a command.
    pub source: String,
    pub started_at: String,
    pub time_scale: f64,
    pub duration_s: Option<f64>,
    pub ended_at: Option<String>,
    /// `"duration_reached"`, `"stop_command"`, `"stopped"`, …
    pub end_reason: Option<String>,
    pub ticks: Option<u64>,
    pub sim_seconds: Option<f64>,
    /// Absolute path of the exported recording, if one was written.
    pub recording: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_values: Option<BTreeMap<String, f64>>,
}

/// A queued command's outcome, keyed by its ticket.
#[derive(Debug, Clone, Serialize)]
pub struct CommandAck {
    pub ticket: String,
    pub op: String,
    pub ok: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    pub at: String,
}

/// A run that has been requested but not yet entered Play.
#[derive(Debug, Clone)]
struct PendingRun {
    run_id: u64,
    ticket: Option<String>,
    source: String,
    time_scale: f64,
    duration_s: Option<f64>,
    /// A `stop` arrived before the run began: end it the moment it starts.
    stop_after_start: bool,
    /// A `pause` arrived before the run began: pause it the moment it starts.
    pause_after_start: bool,
    /// When the run was requested, and how many drain passes it has waited
    /// through — see [`SimRunLedger::expire_stale_pending`].
    requested_at: std::time::Instant,
    frames_waited: u32,
}

/// A requested run that has not entered Play after this long, AND after
/// [`PENDING_START_FRAMES`] frames, is recorded as `never_started`. Both
/// limits must pass: one huge frame (snapshotting a very large scene before
/// Play) can exceed the time limit on its own while the start is still on
/// track.
const PENDING_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const PENDING_START_FRAMES: u32 = 30;

/// Every run this engine has made, in order.
#[derive(Resource, Default, Debug)]
pub struct SimRunLedger {
    next_id: u64,
    pending: Option<PendingRun>,
    current: Option<RunRecord>,
    completed: VecDeque<RunRecord>,
    acks: VecDeque<CommandAck>,
    /// Why the next stop happens, set by whoever triggers it.
    stop_reason: Option<String>,
}

impl SimRunLedger {
    fn alloc_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Id of the run in progress, or the one about to start.
    pub fn active_run_id(&self) -> Option<u64> {
        self.current
            .as_ref()
            .map(|r| r.run_id)
            .or(self.pending.as_ref().map(|p| p.run_id))
    }

    /// Record why the coming stop happens (first writer wins, so the specific
    /// cause beats a generic one set later in the same frame).
    pub fn set_stop_reason(&mut self, reason: impl Into<String>) {
        if self.stop_reason.is_none() {
            self.stop_reason = Some(reason.into());
        }
    }

    /// Called on entering Play. A resume keeps the current run; a fresh entry
    /// adopts the pending run (from a `run` command) or opens a new one.
    /// Returns what the pending run asked to happen right after it started.
    pub fn begin_run(&mut self, clock: &SimulationClock) -> (bool, bool) {
        if self.current.is_some() {
            return (false, false);
        }
        let (record, stop, pause) = match self.pending.take() {
            Some(p) => (
                RunRecord {
                    run_id: p.run_id,
                    ticket: p.ticket,
                    source: p.source,
                    started_at: now(),
                    time_scale: p.time_scale,
                    duration_s: p.duration_s,
                    ended_at: None,
                    end_reason: None,
                    ticks: None,
                    sim_seconds: None,
                    recording: None,
                    final_values: None,
                },
                p.stop_after_start,
                p.pause_after_start,
            ),
            None => {
                let run_id = self.alloc_id();
                (
                    RunRecord {
                        run_id,
                        ticket: None,
                        source: "play".to_owned(),
                        started_at: now(),
                        time_scale: clock.time_scale,
                        duration_s: None,
                        ended_at: None,
                        end_reason: None,
                        ticks: None,
                        sim_seconds: None,
                        recording: None,
                        final_values: None,
                    },
                    false,
                    false,
                )
            }
        };
        info!("🏁 Sim run #{} started ({})", record.run_id, record.source);
        self.current = Some(record);
        self.stop_reason = None;
        (stop, pause)
    }

    /// Record a run that was requested but never entered Play, so a client
    /// waiting on its id sees it end instead of waiting out its own timeout.
    /// (Play refuses a start only if play mode is already mid-session while
    /// the state says Edit; nothing here can repair that, but it must not
    /// strand every later `run` behind a start that will never happen.)
    fn expire_stale_pending(&mut self, still_editing: bool) {
        let Some(p) = self.pending.as_mut() else { return };
        if !still_editing {
            return;
        }
        p.frames_waited = p.frames_waited.saturating_add(1);
        if p.frames_waited < PENDING_START_FRAMES || p.requested_at.elapsed() < PENDING_START_TIMEOUT {
            return;
        }
        let Some(p) = self.pending.take() else { return };
        warn!(
            "sim run #{} was requested {:.1}s ago but Play never started; recording it as never_started",
            p.run_id,
            p.requested_at.elapsed().as_secs_f64()
        );
        let at = now();
        self.completed.push_back(RunRecord {
            run_id: p.run_id,
            ticket: p.ticket,
            source: p.source,
            started_at: at.clone(),
            time_scale: p.time_scale,
            duration_s: p.duration_s,
            ended_at: Some(at),
            end_reason: Some("never_started".to_owned()),
            ticks: Some(0),
            sim_seconds: Some(0.0),
            recording: None,
            final_values: None,
        });
        while self.completed.len() > MAX_COMPLETED_RUNS {
            self.completed.pop_front();
        }
    }

    /// Called on returning to Edit, with the outcome captured before reset.
    pub fn complete_run(
        &mut self,
        ticks: u64,
        sim_seconds: f64,
        recording: Option<PathBuf>,
        final_values: BTreeMap<String, f64>,
    ) {
        let Some(mut run) = self.current.take() else { return };
        run.ended_at = Some(now());
        run.end_reason = Some(self.stop_reason.take().unwrap_or_else(|| "stopped".to_owned()));
        run.ticks = Some(ticks);
        run.sim_seconds = Some(sim_seconds);
        run.recording = recording.map(|p| p.to_string_lossy().into_owned());
        run.final_values = Some(final_values);
        info!(
            "🏁 Sim run #{} ended: {} after {} ticks ({:.2}s sim)",
            run.run_id,
            run.end_reason.as_deref().unwrap_or("?"),
            ticks,
            sim_seconds
        );
        self.completed.push_back(run);
        while self.completed.len() > MAX_COMPLETED_RUNS {
            self.completed.pop_front();
        }
    }

    fn ack(&mut self, ticket: &Option<String>, op: &str, ok: bool, detail: String, run_id: Option<u64>) {
        let Some(ticket) = ticket.clone() else { return };
        self.acks.push_back(CommandAck {
            ticket,
            op: op.to_owned(),
            ok,
            detail,
            run_id,
            at: now(),
        });
        while self.acks.len() > MAX_ACKS {
            self.acks.pop_front();
        }
    }

    /// The ledger as JSON. `completed_limit` bounds the history (newest kept).
    pub fn to_json(&self, completed_limit: usize, ack_limit: usize) -> Value {
        let skip = self.completed.len().saturating_sub(completed_limit);
        let completed: Vec<&RunRecord> = self.completed.iter().skip(skip).collect();
        let ack_skip = self.acks.len().saturating_sub(ack_limit);
        let acks: Vec<&CommandAck> = self.acks.iter().skip(ack_skip).collect();
        json!({
            "pending": self.pending.as_ref().map(|p| json!({
                "run_id": p.run_id,
                "ticket": p.ticket,
                "time_scale": p.time_scale,
                "duration_s": p.duration_s,
            })),
            "current": self.current,
            "last": self.completed.back(),
            "completed": completed,
            "acks": acks,
        })
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ─────────────────────────────────────────────────────────────────────────────
// Apply
// ─────────────────────────────────────────────────────────────────────────────

/// Apply one command to the world. Returns the acknowledgement payload
/// (`status`, and the `run_id` for run/pause/stop).
///
/// Play-state transitions go through the same messages the Play / Pause /
/// Stop buttons send, so a command gets the full lifecycle — snapshot on
/// start, restore and despawn on stop — never a bare state flip.
pub fn apply(world: &mut World, cmd: SimCommand, origin: &Origin) -> Result<Value, String> {
    let state = *world.resource::<State<PlayModeState>>().get();
    let op = cmd.op_name();

    let result: Result<Value, String> = match cmd {
        SimCommand::Run { time_scale, duration_s } => Ok(apply_run(world, state, time_scale, duration_s, origin)),

        SimCommand::Pause => Ok(match state {
            PlayModeState::Playing => {
                world.write_message(TogglePauseEvent);
                let run_id = world.resource::<SimRunLedger>().active_run_id();
                json!({ "status": "pausing", "run_id": run_id })
            }
            PlayModeState::Paused => {
                let run_id = world.resource::<SimRunLedger>().active_run_id();
                json!({ "status": "already_paused", "run_id": run_id })
            }
            PlayModeState::Editing => {
                // A run requested this frame hasn't entered Play yet: pause it
                // the moment it does, rather than dropping the request.
                let mut ledger = world.resource_mut::<SimRunLedger>();
                match ledger.pending.as_mut() {
                    Some(p) => {
                        p.pause_after_start = true;
                        json!({ "status": "pausing_after_start", "run_id": p.run_id })
                    }
                    None => json!({ "status": "not_running" }),
                }
            }
        }),

        SimCommand::Stop => Ok(match state {
            PlayModeState::Playing | PlayModeState::Paused => {
                world.resource_mut::<SimAutoStop>().stop_at_sim_s = None;
                world
                    .resource_mut::<SimRunLedger>()
                    .set_stop_reason("stop_command");
                world.write_message(StopPlayEvent);
                let run_id = world.resource::<SimRunLedger>().active_run_id();
                json!({ "status": "stopping", "run_id": run_id })
            }
            PlayModeState::Editing => {
                // Same as pause: a `stop` right behind a `run` in one batch
                // must end that run, not be discarded because Play hasn't
                // been entered yet.
                let mut ledger = world.resource_mut::<SimRunLedger>();
                match ledger.pending.as_mut() {
                    Some(p) => {
                        p.stop_after_start = true;
                        json!({ "status": "stopping_after_start", "run_id": p.run_id })
                    }
                    None => json!({ "status": "not_running" }),
                }
            }
        }),

        SimCommand::Set { values } => {
            let keys: Vec<String> = values.iter().map(|(k, _)| k.clone()).collect();
            for (key, value) in &values {
                world
                    .resource_mut::<SimValuesResource>()
                    .0
                    .insert(key.clone(), *value);
                // Count as an explicit write so `apply_sim_values_to_ecs`
                // honours it over the default mode behaviour — a
                // `set battery.current 0` should stop the cell, not be
                // overwritten on the same frame.
                world
                    .resource_mut::<ScriptSimWrites>()
                    .0
                    .insert(key.clone(), *value);
                // And the thread-local Rune scripts read.
                crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
                    sv.borrow_mut().insert(key.clone(), *value);
                });
            }
            info!("sim command ({}): set {:?}", origin.source, keys);
            Ok(json!({ "status": "set", "applied": keys.len(), "keys": keys }))
        }
    };

    let mut ledger = world.resource_mut::<SimRunLedger>();
    match &result {
        Ok(v) => {
            let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("ok").to_owned();
            let run_id = v.get("run_id").and_then(|r| r.as_u64());
            ledger.ack(&origin.ticket, op, true, status, run_id);
        }
        Err(e) => ledger.ack(&origin.ticket, op, false, e.clone(), None),
    }
    result
}

fn apply_run(
    world: &mut World,
    state: PlayModeState,
    time_scale: Option<f64>,
    duration_s: Option<f64>,
    origin: &Origin,
) -> Value {
    match state {
        PlayModeState::Editing => {
            let scale = time_scale.unwrap_or(1.0);
            let sim_now = {
                let mut clock = world.resource_mut::<SimulationClock>();
                clock.set_time_scale(scale);
                clock.simulation_time_s
            };
            world.resource_mut::<SimAutoStop>().stop_at_sim_s = duration_s.map(|d| sim_now + d);

            let mut ledger = world.resource_mut::<SimRunLedger>();
            // A second `run` before the first has entered Play retunes the
            // pending run instead of queuing another one behind it.
            if let Some(p) = ledger.pending.as_mut() {
                p.time_scale = scale;
                p.duration_s = duration_s;
                let run_id = p.run_id;
                return json!({
                    "status": "starting",
                    "run_id": run_id,
                    "time_scale": scale,
                    "duration_s": duration_s,
                });
            }
            let run_id = ledger.alloc_id();
            ledger.pending = Some(PendingRun {
                run_id,
                ticket: origin.ticket.clone(),
                source: origin.source.to_owned(),
                time_scale: scale,
                duration_s,
                stop_after_start: false,
                pause_after_start: false,
                requested_at: std::time::Instant::now(),
                frames_waited: 0,
            });
            world.write_message(StartPlayEvent { play_type: PlayModeType::default() });
            info!(
                "sim command ({}): run #{} (time_scale={:.2}x, {})",
                origin.source,
                run_id,
                scale,
                duration_s.map(|d| format!("auto-stop after {d:.2}s")).unwrap_or_else(|| "indefinite".into())
            );
            json!({ "status": "starting", "run_id": run_id, "time_scale": scale, "duration_s": duration_s })
        }
        PlayModeState::Paused | PlayModeState::Playing => {
            let (scale, sim_now) = {
                let mut clock = world.resource_mut::<SimulationClock>();
                if let Some(s) = time_scale {
                    clock.set_time_scale(s);
                }
                (clock.time_scale, clock.simulation_time_s)
            };
            if let Some(d) = duration_s {
                world.resource_mut::<SimAutoStop>().stop_at_sim_s = Some(sim_now + d);
            }
            let status = if state == PlayModeState::Paused {
                // Resume with the same toggle the Pause button uses.
                // (Sending StartPlayEvent here would ALSO resume — Play on a
                // started session toggles pause — but on a RUNNING one it
                // pauses, which is why `run` twice used to stop the world.)
                world.write_message(TogglePauseEvent);
                "resuming"
            } else {
                "running"
            };
            let run_id = world.resource::<SimRunLedger>().active_run_id();
            json!({ "status": status, "run_id": run_id, "time_scale": scale, "duration_s": duration_s })
        }
    }
}

/// `sim.state` payload: play state, clock, auto-stop target, and the ledger.
pub fn state_json(world: &World) -> Value {
    let play_state = world
        .get_resource::<State<PlayModeState>>()
        .map(|s| match s.get() {
            PlayModeState::Editing => "editing",
            PlayModeState::Playing => "playing",
            PlayModeState::Paused => "paused",
        })
        .unwrap_or("unknown");
    let clock = world.get_resource::<SimulationClock>();
    let ipc = world.get_resource::<SimIpc>();
    let bridge = world.get_resource::<crate::engine_bridge::EngineBridgeHandle>();
    json!({
        "pid": std::process::id(),
        "space": ipc.and_then(|i| i.space_name.clone()),
        "universe": ipc.and_then(|i| i.universe.as_ref().map(|u| u.to_string_lossy().into_owned())),
        "owns_universe": ipc.map(|i| i.owns_universe(bridge)).unwrap_or(false),
        "play_state": play_state,
        "clock": clock.map(|c| json!({
            "tick": c.tick_count,
            "sim_time_s": c.simulation_time_s,
            "time_scale": c.time_scale,
        })),
        "auto_stop_at_s": world.get_resource::<SimAutoStop>().and_then(|a| a.stop_at_sim_s),
        "runs": world
            .get_resource::<SimRunLedger>()
            .map(|l| l.to_json(MAX_COMPLETED_RUNS, MAX_ACKS))
            .unwrap_or(Value::Null),
    })
}

/// The ledger section of the runtime snapshot (bounded — rewritten at 4 Hz).
pub fn snapshot_runs_json(ledger: &SimRunLedger) -> Value {
    ledger.to_json(SNAPSHOT_COMPLETED_RUNS, SNAPSHOT_ACKS)
}

/// Every current sim value: `SimValuesResource` merged with the current value
/// of each enabled, finite watchpoint (`SimValuesResource` wins a collision).
/// Subsystems that own their physics (the ARC-1 nuclear model) record only
/// into the watchpoint registry, so both stores are needed for a full picture.
pub fn merged_sim_values(
    sim_values: Option<&SimValuesResource>,
    watchpoints: Option<&WatchPointRegistry>,
) -> BTreeMap<String, f64> {
    let mut merged: BTreeMap<String, f64> = sim_values
        .map(|r| r.0.iter().map(|(k, v)| (k.clone(), *v)).collect())
        .unwrap_or_default();
    if let Some(reg) = watchpoints {
        for (name, wp) in &reg.watchpoints {
            if wp.enabled && wp.current.is_finite() {
                merged.entry(name.clone()).or_insert(wp.current);
            }
        }
    }
    merged
}

// ─────────────────────────────────────────────────────────────────────────────
// Queue drain
// ─────────────────────────────────────────────────────────────────────────────

/// A queue file, claimed by renaming it aside.
///
/// Read-then-truncate — the old drain — is unsafe with more than one engine
/// or more than one writer: two engines can both read a line before either
/// truncates (the command runs twice), and a line appended between one
/// engine's read and its truncate is erased unread (the command never runs).
/// Renaming is atomic, so exactly one drainer claims a given batch, and a
/// writer that arrives after the rename simply starts a fresh queue file.
///
/// A writer that opened the file just before the rename can still land its
/// line in the claimed file after it was read, so a claim is held for one
/// more frame and re-read from where the first pass stopped before it is
/// deleted. Every writer opens, appends one line, and closes within
/// microseconds, so a frame is ample.
#[derive(Default, Debug)]
struct QueueClaim {
    held: Option<(PathBuf, usize)>,
}

impl QueueClaim {
    /// Finish last frame's claim, then — if `may_claim` — claim `queue` anew.
    /// Complete lines are appended to `out`.
    fn poll(&mut self, queue: &Path, claim: &Path, may_claim: bool, out: &mut Vec<String>) {
        if let Some((path, consumed)) = self.held.take() {
            if let Ok(bytes) = std::fs::read(&path) {
                if bytes.len() > consumed {
                    // Final pass: the file is about to be deleted, so take a
                    // trailing partial line too (none is expected — writers
                    // emit whole lines in one write).
                    push_lines(&bytes[consumed..], true, out);
                }
            }
            let _ = std::fs::remove_file(&path);
        }

        if !may_claim || !queue.is_file() {
            return;
        }
        // Fails harmlessly if another claimer won the race or the file is
        // briefly locked; the batch is picked up next frame.
        if std::fs::rename(queue, claim).is_err() {
            return;
        }
        let Ok(bytes) = std::fs::read(claim) else {
            // Leave it for the straggler pass to retry.
            self.held = Some((claim.to_path_buf(), 0));
            return;
        };
        let consumed = push_lines(&bytes, false, out);
        self.held = Some((claim.to_path_buf(), consumed));
    }
}

/// Split `bytes` into lines, appending each non-empty one to `out`. Returns
/// how many bytes were consumed; an unterminated tail is left unconsumed
/// unless `take_partial`.
fn push_lines(bytes: &[u8], take_partial: bool, out: &mut Vec<String>) -> usize {
    let mut consumed = 0;
    for chunk in bytes.split_inclusive(|b| *b == b'\n') {
        let complete = chunk.last() == Some(&b'\n');
        if !complete && !take_partial {
            break;
        }
        consumed += chunk.len();
        let line = String::from_utf8_lossy(chunk);
        let line = line.trim();
        if !line.is_empty() {
            out.push(line.to_owned());
        }
    }
    consumed
}

/// Claim state for both of this instance's queues.
#[derive(Resource, Default, Debug)]
pub struct SimQueueDrains {
    instance: QueueClaim,
    universe: QueueClaim,
}

/// Drain this instance's private queue, and the Universe's legacy queue if
/// this instance owns the Universe, applying each command in order.
pub fn drain_sim_command_queues(world: &mut World) {
    let still_editing = world
        .get_resource::<State<PlayModeState>>()
        .is_some_and(|s| *s.get() == PlayModeState::Editing);
    if let Some(mut ledger) = world.get_resource_mut::<SimRunLedger>() {
        ledger.expire_stale_pending(still_editing);
    }

    let Some(ipc) = world.get_resource::<SimIpc>().cloned() else { return };
    let mut drains = world.remove_resource::<SimQueueDrains>().unwrap_or_default();

    let mut instance_lines = Vec::new();
    if let (Some(q), Some(c)) = (ipc.instance_queue(), ipc.instance_queue_claim()) {
        drains.instance.poll(&q, &c, true, &mut instance_lines);
    }

    let mut universe_lines = Vec::new();
    if let (Some(q), Some(c)) = (ipc.legacy_queue(), ipc.legacy_queue_claim()) {
        // Ownership is read only when there is a legacy batch to claim; an
        // in-flight claim is always finished, even if ownership has moved.
        let may_claim = q.is_file()
            && ipc.owns_universe(world.get_resource::<crate::engine_bridge::EngineBridgeHandle>());
        drains.universe.poll(&q, &c, may_claim, &mut universe_lines);
    }

    world.insert_resource(drains);

    for (source, lines) in [("instance-queue", instance_lines), ("universe-queue", universe_lines)] {
        for line in lines {
            let parsed = serde_json::from_str::<Value>(&line)
                .map_err(|e| format!("not JSON: {e}"))
                .and_then(|v| SimCommand::from_queue_line(&v));
            match parsed {
                Ok((cmd, ticket)) => {
                    let origin = Origin { source, ticket };
                    if let Err(e) = apply(world, cmd, &origin) {
                        warn!("sim command ({source}) failed: {e}");
                    }
                }
                Err(e) => {
                    // Never silently: a dropped line is a command someone is
                    // waiting on. Acknowledge the failure if it has a ticket.
                    let short: String = line.chars().take(200).collect();
                    warn!("sim command ({source}) rejected: {e}; line: {short}");
                    let ticket = serde_json::from_str::<Value>(&line)
                        .ok()
                        .and_then(|v| v.get("id").and_then(|t| t.as_str()).map(str::to_owned));
                    let op = serde_json::from_str::<Value>(&line)
                        .ok()
                        .and_then(|v| v.get("op").and_then(|t| t.as_str()).map(str::to_owned))
                        .unwrap_or_default();
                    world
                        .resource_mut::<SimRunLedger>()
                        .ack(&ticket, &op, false, e, None);
                }
            }
        }
    }
}
