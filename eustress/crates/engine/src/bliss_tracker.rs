//! =============================================================================
//! Bliss Contribution Tracker — real work → witnessed, co-signed contributions
//! =============================================================================
//!
//! This is the engine half of the Bliss proof-of-contribution loop. It turns
//! actual Studio work into `ContributionType` buckets, submits them to the
//! witness Worker (`api.eustress.dev`) for co-signing, and keeps the ribbon's
//! top-right Bliss badge in sync with the *authoritative* ledger balance.
//!
//! ## Attribution model
//!
//! Every frame in which the window is focused AND the user gave input within
//! the last `INPUT_IDLE_SECS`, the frame's `dt` is attributed to exactly one
//! bucket, most-valuable signal first:
//!
//! | Signal (within `SIGNAL_WINDOW_SECS`)          | Bucket        | Weight |
//! |-----------------------------------------------|---------------|--------|
//! | Script editor content changed                 | `Development` | 3.0x   |
//! | Undoable scene edit (`UndoStack` push)        | `Creation`    | 2.5x   |
//! | Input only (navigating, inspecting, testing)  | `ActiveTime`  | 1.0x   |
//!
//! Weights are applied by the **witness**, not here — the client only reports
//! type + duration. The witness also applies the +10% Full-node bonus and
//! enforces auth, dedupe, and rate limits, so a modified client can't
//! self-award score.
//!
//! ## Flush cycle
//!
//! Buckets flush to the witness every `FLUSH_INTERVAL_SECS` once they hold at
//! least `MIN_SUBMIT_SECS` (the witness clamps a submission to a minimum of
//! one weighted minute, so submitting less would inflate score). Submission
//! requires a logged-in user (bearer JWT) — while logged out, buckets keep
//! accumulating and persist to disk, then flush after the next login.
//!
//! ## Persistence
//!
//! `~/.eustress_engine/bliss_tracker.toml` holds the stable node id, the
//! last-known server balance (so the badge isn't blank offline), and any
//! unsent buckets. Written every `PERSIST_INTERVAL_SECS` and on `AppExit`.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::auth::{AuthState, AuthStatus, BlissNodeState};
use crate::undo::UndoStack;

// ---------------------------------------------------------------------------
// Tuning constants
// ---------------------------------------------------------------------------

/// Seconds of input silence after which time stops counting as active.
const INPUT_IDLE_SECS: f64 = 60.0;
/// How long after an edit signal its bucket keeps claiming active time.
const SIGNAL_WINDOW_SECS: f64 = 120.0;
/// How often accumulated buckets are submitted to the witness.
const FLUSH_INTERVAL_SECS: f64 = 300.0;
/// Minimum bucket size worth submitting (witness clamps to ≥1 minute).
const MIN_SUBMIT_SECS: f64 = 60.0;
/// Maximum duration per single submission (witness clamps at 3600).
const MAX_SUBMIT_SECS: f64 = 3600.0;
/// Cap on submissions per flush — stays far under the witness's
/// 120/hour rate limit even when draining an offline backlog.
const MAX_JOBS_PER_FLUSH: usize = 6;
/// Disk persistence cadence for the tracker state file.
const PERSIST_INTERVAL_SECS: f64 = 30.0;
/// Heartbeat cadence — each beat returns the authoritative balance.
const HEARTBEAT_INTERVAL_SECS: f64 = 90.0;

/// Contribution buckets tracked by the engine. Indexes into
/// [`BlissTracker::buckets`]. Weights shown are applied witness-side.
const BUCKET_TYPES: [&str; 3] = ["ActiveTime", "Creation", "Development"];
const IDX_ACTIVE_TIME: usize = 0;
const IDX_CREATION: usize = 1;
const IDX_DEVELOPMENT: usize = 2;

/// Client-side weight mirror — used ONLY for the badge's local pending
/// estimate. The witness's weight table is authoritative.
const LOCAL_WEIGHTS: [f64; 3] = [1.0, 2.5, 3.0];

// ---------------------------------------------------------------------------
// Persistent state file
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct TrackerFile {
    /// Stable node identifier (uuid v4, minted once per machine).
    node_id: String,
    /// Last authoritative balance from the witness — display cache so
    /// the badge shows the real balance immediately on launch.
    cached_balance: f64,
    /// Last server-side pending score seen (today's weighted minutes).
    cached_pending: f64,
    /// Unsent bucket seconds, keyed by contribution type name.
    #[serde(default)]
    pending_buckets: HashMap<String, f64>,
}

// ---------------------------------------------------------------------------
// Network worker (owns the tokio runtime + CosignClient)
// ---------------------------------------------------------------------------

/// A job for the network worker thread.
enum NetJob {
    Cosign {
        token: String,
        user_id: String,
        contribution_type: &'static str,
        duration_secs: u64,
        hash: String,
        node_mode: String,
        /// Correlates the in-flight bucket chunk for ack/refund.
        chunk_id: u64,
    },
    Heartbeat {
        node_id: String,
        mode: String,
        user_id: Option<String>,
        uptime_secs: u64,
    },
}

/// An event coming back from the network worker thread.
enum NetEvent {
    CosignOk {
        chunk_id: u64,
        contribution_type: &'static str,
        score_added: f64,
    },
    CosignFail {
        chunk_id: u64,
        error: String,
    },
    Balance {
        bliss_balance: f64,
        pending_score: f64,
    },
}

/// Channel handles to the network worker thread.
/// (`crossbeam_channel::Sender` is `Sync`, which `Resource` requires —
/// `std::sync::mpsc::Sender` is not.)
#[derive(Resource)]
pub struct BlissNet {
    job_tx: crossbeam_channel::Sender<NetJob>,
    inbox: Arc<Mutex<Vec<NetEvent>>>,
}

impl BlissNet {
    /// Spawn the worker thread: a single-threaded tokio runtime driving
    /// the witness `CosignClient`. Jobs are processed sequentially —
    /// contribution traffic is a few requests per hour.
    fn spawn(witness_url: String, fork_id: String) -> Self {
        let (job_tx, job_rx) = crossbeam_channel::unbounded::<NetJob>();
        let inbox: Arc<Mutex<Vec<NetEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let inbox_thread = Arc::clone(&inbox);

        std::thread::Builder::new()
            .name("bliss-net".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        error!("BlissNet: tokio runtime failed: {e} — Bliss sync disabled");
                        return;
                    }
                };
                let mut client =
                    eustress_bliss::CosignClient::new(witness_url, fork_id);

                while let Ok(job) = job_rx.recv() {
                    match job {
                        NetJob::Cosign {
                            token,
                            user_id,
                            contribution_type,
                            duration_secs,
                            hash,
                            node_mode,
                            chunk_id,
                        } => {
                            client.set_auth_token(Some(token));
                            let result = rt.block_on(client.cosign(
                                &user_id,
                                &hash,
                                contribution_type,
                                duration_secs,
                                &node_mode,
                            ));
                            let event = match result {
                                Ok(res) => NetEvent::CosignOk {
                                    chunk_id,
                                    contribution_type,
                                    score_added: res.score_added,
                                },
                                Err(e) => NetEvent::CosignFail {
                                    chunk_id,
                                    error: e.to_string(),
                                },
                            };
                            if let Ok(mut inbox) = inbox_thread.lock() {
                                inbox.push(event);
                            }
                        }
                        NetJob::Heartbeat {
                            node_id,
                            mode,
                            user_id,
                            uptime_secs,
                        } => {
                            let result = rt.block_on(client.heartbeat(
                                &node_id,
                                &mode,
                                0,
                                uptime_secs,
                                user_id.as_deref(),
                            ));
                            // Balance only means something for a known user.
                            if let (Ok(reply), Some(_)) = (result, user_id) {
                                if let Ok(mut inbox) = inbox_thread.lock() {
                                    inbox.push(NetEvent::Balance {
                                        bliss_balance: reply.bliss_balance,
                                        pending_score: reply.pending_score,
                                    });
                                }
                            }
                        }
                    }
                }
            })
            .expect("failed to spawn bliss-net thread");

        Self { job_tx, inbox }
    }
}

// ---------------------------------------------------------------------------
// Tracker resource
// ---------------------------------------------------------------------------

/// Engine-side contribution state. The drain in `slint_ui.rs` marks
/// script activity on this resource; everything else is fed by
/// [`track_activity`].
#[derive(Resource)]
pub struct BlissTracker {
    /// `Time::elapsed_secs_f64` mirror so non-`Time` systems (the Slint
    /// drain) can stamp signals without their own clock access.
    pub now: f64,
    /// Last frame at which the user gave any input.
    last_input: f64,
    /// Last undoable scene edit (UndoStack push).
    last_scene_edit: f64,
    /// Last script editor content change. Written by the Slint drain on
    /// `SlintAction::ScriptContentChanged`.
    pub last_script_edit: f64,
    /// UndoStack sequence at the previous frame.
    last_undo_sequence: u64,
    /// Accumulated unsent seconds per bucket (see `BUCKET_TYPES`).
    buckets: [f64; 3],
    /// Chunks submitted to the witness but not yet acked:
    /// `chunk_id → (bucket index, seconds)`. Refunded on failure.
    inflight: HashMap<u64, (usize, f64)>,
    next_chunk_id: u64,
    /// Authoritative balance from the witness (whole BLS).
    server_balance: f64,
    /// Today's server-side pending score (weighted minutes).
    server_pending: f64,
    /// Stable node id (persisted; survives restarts).
    node_id: String,
    since_flush: f64,
    since_persist: f64,
    since_heartbeat: f64,
    /// Where tracker state persists; `None` if no home dir (accrual
    /// then lives only in memory for the session).
    persist_path: Option<std::path::PathBuf>,
    /// Signals negative (idle-out) edges so the display refresh knows
    /// a repaint is needed even without new input.
    display_dirty: bool,
}

impl Default for BlissTracker {
    fn default() -> Self {
        let persist_path = dirs::home_dir()
            .map(|h| h.join(".eustress_engine").join("bliss_tracker.toml"));

        let file: TrackerFile = persist_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();

        let node_id = if file.node_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            file.node_id
        };

        let mut buckets = [0.0; 3];
        for (i, name) in BUCKET_TYPES.iter().enumerate() {
            buckets[i] = file.pending_buckets.get(*name).copied().unwrap_or(0.0);
        }

        Self {
            now: 0.0,
            // Idle until proven otherwise — no free ActiveTime at launch.
            last_input: -INPUT_IDLE_SECS,
            last_scene_edit: -SIGNAL_WINDOW_SECS,
            last_script_edit: -SIGNAL_WINDOW_SECS,
            last_undo_sequence: 0,
            buckets,
            inflight: HashMap::new(),
            next_chunk_id: 1,
            server_balance: file.cached_balance,
            server_pending: file.cached_pending,
            node_id,
            since_flush: 0.0,
            since_persist: 0.0,
            // First heartbeat fires quickly so the badge syncs on launch.
            since_heartbeat: HEARTBEAT_INTERVAL_SECS - 5.0,
            persist_path,
            display_dirty: true,
        }
    }
}

impl BlissTracker {
    /// Local pending-score estimate (weighted minutes) for unsent +
    /// in-flight seconds. Display only — the witness is authoritative.
    fn local_pending_estimate(&self) -> f64 {
        let mut est = 0.0;
        for (i, secs) in self.buckets.iter().enumerate() {
            est += LOCAL_WEIGHTS[i] * secs / 60.0;
        }
        for (idx, secs) in self.inflight.values() {
            est += LOCAL_WEIGHTS[*idx] * secs / 60.0;
        }
        est
    }

    /// Write state to disk. Failure is non-fatal (retried next interval).
    fn persist(&self) {
        let Some(path) = self.persist_path.as_ref() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut pending_buckets = HashMap::new();
        for (i, name) in BUCKET_TYPES.iter().enumerate() {
            // In-flight chunks are folded back in: if the process dies
            // before the ack, the work is re-submitted next session
            // (the witness dedupes by contribution hash).
            let inflight: f64 = self
                .inflight
                .values()
                .filter(|(idx, _)| *idx == i)
                .map(|(_, s)| s)
                .sum();
            let total = self.buckets[i] + inflight;
            if total > 0.0 {
                pending_buckets.insert(name.to_string(), total);
            }
        }
        let file = TrackerFile {
            node_id: self.node_id.clone(),
            cached_balance: self.server_balance,
            cached_pending: self.server_pending,
            pending_buckets,
        };
        match toml::to_string_pretty(&file) {
            Ok(body) => {
                if let Err(e) = std::fs::write(path, body) {
                    warn!("BlissTracker: persist failed: {e}");
                }
            }
            Err(e) => warn!("BlissTracker: serialize failed: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Attribute frame time to contribution buckets based on live signals.
fn track_activity(
    time: Res<Time>,
    mut tracker: ResMut<BlissTracker>,
    undo: Option<Res<UndoStack>>,
    mut key_events: MessageReader<KeyboardInput>,
    mut mouse_button_events: MessageReader<MouseButtonInput>,
    mut mouse_motion_events: MessageReader<MouseMotion>,
    mut mouse_wheel_events: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs_f64();
    tracker.now = now;

    // -- Input signal ------------------------------------------------------
    let had_input = key_events.read().next().is_some()
        || mouse_button_events.read().next().is_some()
        || mouse_motion_events.read().next().is_some()
        || mouse_wheel_events.read().next().is_some();
    if had_input {
        tracker.last_input = now;
    }

    // -- Scene edit signal (undoable work) ---------------------------------
    if let Some(undo) = undo {
        let seq = undo.sequence();
        if seq != tracker.last_undo_sequence {
            tracker.last_undo_sequence = seq;
            tracker.last_scene_edit = now;
        }
    }

    // -- Attribution -------------------------------------------------------
    let focused = windows.iter().next().map(|w| w.focused).unwrap_or(false);
    let active = focused && (now - tracker.last_input) <= INPUT_IDLE_SECS;
    if !active {
        return;
    }

    let bucket = if (now - tracker.last_script_edit) <= SIGNAL_WINDOW_SECS {
        IDX_DEVELOPMENT
    } else if (now - tracker.last_scene_edit) <= SIGNAL_WINDOW_SECS {
        IDX_CREATION
    } else {
        IDX_ACTIVE_TIME
    };
    tracker.buckets[bucket] += dt;
    tracker.display_dirty = true;
}

/// Submit matured buckets to the witness for co-signing. Requires a
/// logged-in user; otherwise buckets keep accumulating locally.
fn flush_contributions(
    time: Res<Time>,
    mut tracker: ResMut<BlissTracker>,
    net: Res<BlissNet>,
    auth: Option<Res<AuthState>>,
    bliss_state: Option<Res<BlissNodeState>>,
) {
    tracker.since_flush += time.delta_secs_f64();
    if tracker.since_flush < FLUSH_INTERVAL_SECS {
        return;
    }
    tracker.since_flush = 0.0;

    let Some(auth) = auth else { return };
    if auth.status != AuthStatus::LoggedIn {
        return;
    }
    let (Some(token), Some(user)) = (auth.token.clone(), auth.user.as_ref()) else {
        return;
    };
    let user_id = user.id.clone();

    let node_mode = bliss_state
        .as_ref()
        .map(|b| b.mode.clone())
        .unwrap_or_else(|| "Light".to_string());
    let enabled = bliss_state.map(|b| b.enabled).unwrap_or(true);
    if !enabled {
        return;
    }

    let mut jobs = 0usize;
    for i in 0..BUCKET_TYPES.len() {
        while tracker.buckets[i] >= MIN_SUBMIT_SECS && jobs < MAX_JOBS_PER_FLUSH {
            let chunk_secs = tracker.buckets[i].min(MAX_SUBMIT_SECS);
            tracker.buckets[i] -= chunk_secs;

            let chunk_id = tracker.next_chunk_id;
            tracker.next_chunk_id += 1;
            tracker.inflight.insert(chunk_id, (i, chunk_secs));

            // Content hash binds user, day, type, duration, and a
            // monotonic component — the witness rejects replays of the
            // same hash, and the same work can't be claimed twice.
            let day = chrono::Utc::now().format("%Y-%m-%d");
            let hash = blake3::hash(
                format!(
                    "{user_id}|{day}|{}|{}|{}|{}",
                    BUCKET_TYPES[i],
                    chunk_secs as u64,
                    tracker.node_id,
                    chunk_id,
                )
                .as_bytes(),
            )
            .to_hex()
            .to_string();

            let job = NetJob::Cosign {
                token: token.clone(),
                user_id: user_id.clone(),
                contribution_type: BUCKET_TYPES[i],
                duration_secs: chunk_secs.round() as u64,
                hash,
                node_mode: node_mode.clone(),
                chunk_id,
            };
            if net.job_tx.send(job).is_err() {
                // Worker thread gone — refund and stop trying.
                if let Some((idx, secs)) = tracker.inflight.remove(&chunk_id) {
                    tracker.buckets[idx] += secs;
                }
                return;
            }
            jobs += 1;
        }
    }
}

/// Periodic heartbeat — registers the node and pulls the authoritative
/// balance + today's pending score back for the badge.
fn send_heartbeat(
    time: Res<Time>,
    mut tracker: ResMut<BlissTracker>,
    net: Res<BlissNet>,
    auth: Option<Res<AuthState>>,
    bliss_state: Option<Res<BlissNodeState>>,
) {
    tracker.since_heartbeat += time.delta_secs_f64();
    if tracker.since_heartbeat < HEARTBEAT_INTERVAL_SECS {
        return;
    }
    tracker.since_heartbeat = 0.0;

    let user_id = auth
        .as_ref()
        .filter(|a| a.status == AuthStatus::LoggedIn)
        .and_then(|a| a.user.as_ref())
        .map(|u| u.id.clone());
    let mode = bliss_state
        .map(|b| b.mode.clone())
        .unwrap_or_else(|| "Light".to_string());

    let _ = net.job_tx.send(NetJob::Heartbeat {
        node_id: tracker.node_id.clone(),
        mode,
        user_id,
        uptime_secs: time.elapsed_secs_f64() as u64,
    });
}

/// Apply network events and refresh the badge display strings.
fn drain_net_events(
    mut tracker: ResMut<BlissTracker>,
    net: Res<BlissNet>,
    mut display: ResMut<BlissNodeState>,
    mut auth: Option<ResMut<AuthState>>,
    // NOTE: two `OutputConsole` types exist (ui/mod.rs legacy vs
    // ui/slint_ui.rs live) — the slint_ui one is the registered resource.
    output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
) {
    let events: Vec<NetEvent> = match net.inbox.lock() {
        Ok(mut inbox) => inbox.drain(..).collect(),
        Err(_) => Vec::new(),
    };

    let mut output = output;
    for event in events {
        match event {
            NetEvent::CosignOk {
                chunk_id,
                contribution_type,
                score_added,
            } => {
                tracker.inflight.remove(&chunk_id);
                tracker.server_pending += score_added;
                tracker.display_dirty = true;
                if let Some(ref mut out) = output {
                    out.info(format!(
                        "Bliss: +{score_added:.1} pts co-signed ({contribution_type})"
                    ));
                }
            }
            NetEvent::CosignFail { chunk_id, error } => {
                // Refund the chunk — it re-flushes next cycle.
                if let Some((idx, secs)) = tracker.inflight.remove(&chunk_id) {
                    tracker.buckets[idx] += secs;
                }
                debug!("Bliss cosign failed (kept locally, will retry): {error}");
            }
            NetEvent::Balance {
                bliss_balance,
                pending_score,
            } => {
                tracker.server_balance = bliss_balance;
                tracker.server_pending = pending_score;
                tracker.display_dirty = true;
                if let Some(ref mut auth) = auth {
                    if let Some(ref mut user) = auth.user {
                        user.bliss_balance = bliss_balance as i64;
                    }
                }
            }
        }
    }

    // -- Badge display strings ---------------------------------------------
    // Balance is the ledger truth; pending = server score + local unsent
    // estimate so the badge visibly responds to work within seconds.
    if tracker.display_dirty {
        tracker.display_dirty = false;
        let pending_total = tracker.server_pending + tracker.local_pending_estimate();
        display.balance = format!("{:.18}", tracker.server_balance);
        display.balance_short = format!("{:.2}", tracker.server_balance);
        display.pending = format!("+{pending_total:.1} pts today");
    }
}

/// Persist tracker state periodically and on exit.
fn persist_tracker(
    time: Res<Time>,
    mut tracker: ResMut<BlissTracker>,
    mut exit: MessageReader<bevy::app::AppExit>,
) {
    tracker.since_persist += time.delta_secs_f64();
    let exiting = exit.read().next().is_some();
    if tracker.since_persist >= PERSIST_INTERVAL_SECS || exiting {
        tracker.since_persist = 0.0;
        tracker.persist();
    }
}

/// Belt-and-braces exit persistence: the Slint window-close path returns
/// `AppExit` from the app runner without another `Update` frame, so the
/// system above can miss the final state. Resources drop with the
/// `World` on graceful teardown — persist here too (idempotent write).
impl Drop for BlissTracker {
    fn drop(&mut self) {
        self.persist();
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Witness base URL. The Worker serves both registration and co-signing.
const WITNESS_URL: &str = "https://api.eustress.dev";

pub struct BlissTrackerPlugin;

impl Plugin for BlissTrackerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlissTracker>()
            .insert_resource(BlissNet::spawn(
                WITNESS_URL.to_string(),
                "eustress.dev".to_string(),
            ))
            .add_systems(
                Update,
                (
                    track_activity,
                    flush_contributions,
                    send_heartbeat,
                    drain_net_events,
                    persist_tracker,
                )
                    .chain(),
            );
    }
}
