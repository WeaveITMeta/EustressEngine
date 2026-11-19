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
//! | Input only (navigating, inspecting, testing)  | `ActiveTime`  | 0.02x  |
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
/// Consecutive non-auth cosign failures before the badge says "not syncing".
const SYNC_FAILURE_VISIBLE_AFTER: u32 = 3;
/// Least time between two sign-ins Studio starts on its own after Eustress
/// refuses the session's token, so an identity Eustress keeps refusing is not
/// retried every frame.
const RENEW_GAP_SECS: f64 = 600.0;

/// Contribution buckets tracked by the engine. Indexes into
/// [`BlissTracker::buckets`]. Weights shown are applied witness-side.
const BUCKET_TYPES: [&str; 3] = ["ActiveTime", "Creation", "Development"];
const IDX_ACTIVE_TIME: usize = 0;
const IDX_CREATION: usize = 1;
const IDX_DEVELOPMENT: usize = 2;

/// Client-side weight mirror — used ONLY for the badge's local pending
/// estimate. The witness's weight table is authoritative.
const LOCAL_WEIGHTS: [f64; 3] = [0.1, 2.5, 3.0];

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
    /// Next contribution chunk id. MUST persist: the id is mixed into the
    /// contribution hash, and the witness dedupes on that hash. Restarting
    /// at 1 regenerated already-used hashes, so re-submitted work after a
    /// crash was rejected as a duplicate and silently lost (or, for a
    /// different day, sailed through as a fresh claim).
    #[serde(default)]
    next_chunk_id: u64,
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
        /// Bearer JWT. Without it the witness treats the beat as anonymous:
        /// no presence accrues (so ActiveTime can never be credited) and no
        /// balance is returned.
        token: Option<String>,
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
    /// A heartbeat that carried a token was refused (401/403): the token
    /// expired or was ended. Without this, an expired token went unnoticed
    /// by every beat and only a cosign could latch it.
    HeartbeatRefused,
}

/// Whether a witness error means the session's token is no longer accepted
/// (as opposed to a network fault or a refused claim).
fn is_auth_refusal(error: &str) -> bool {
    error.contains("401") || error.contains("403") || error.to_ascii_lowercase().contains("unauthorized")
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
                            token,
                        } => {
                            let had_token = token.is_some();
                            client.set_auth_token(token);
                            let result = rt.block_on(client.heartbeat(
                                &node_id,
                                &mode,
                                0,
                                uptime_secs,
                                user_id.as_deref(),
                            ));
                            if let Err(e) = &result {
                                if had_token && is_auth_refusal(&e.to_string()) {
                                    if let Ok(mut inbox) = inbox_thread.lock() {
                                        inbox.push(NetEvent::HeartbeatRefused);
                                    }
                                }
                            }
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

/// Why the badge shows what it shows. Earning silently doing nothing was
/// indistinguishable from earning normally, which hid a real auth bug for
/// an entire release — the state is now explicit and rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarnState {
    /// Co-signing normally.
    Earning,
    /// No user session at all.
    SignedOut,
    /// Signed in locally, but the witness never issued a JWT (no private
    /// key in identity.toml, offline, unregistered identity, or a 401).
    NotAuthenticated,
    /// Authenticated, but submissions keep failing for a non-auth reason
    /// (network, 404, 5xx). Work is banked locally and will retry.
    NotSyncing,
    /// User turned Bliss off in the badge dropdown.
    Disabled,
}

impl EarnState {
    /// Text for the badge dropdown's pending line.
    fn hint(&self) -> Option<&'static str> {
        match self {
            EarnState::Earning => None,
            EarnState::SignedOut => Some("Sign in to earn BLS"),
            EarnState::NotAuthenticated => Some("Identity not verified: cannot earn"),
            EarnState::NotSyncing => Some("Not syncing: saved locally, retrying"),
            EarnState::Disabled => Some("Bliss disabled"),
        }
    }
}

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
    /// Why earning is (or isn't) happening — rendered on the badge.
    earn_state: EarnState,
    /// Set when the witness returns 401. Stops the flush/refund/retry loop
    /// until the user re-authenticates: cleared by a new token (any sign-in)
    /// or by a successful AUTHENTICATED heartbeat balance read, which proves
    /// credentials work again.
    auth_rejected: bool,
    /// The token the witness refused. A session holding any other token
    /// signed in again since, so the refusal no longer applies to it.
    rejected_token: Option<String>,
    /// `now` when Studio last signed in again on its own (see
    /// [`RENEW_GAP_SECS`]).
    last_renewal: f64,
    /// Consecutive non-auth cosign failures; drives the NotSyncing hint.
    consecutive_failures: u32,
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
            next_chunk_id: file.next_chunk_id.max(1),
            server_balance: file.cached_balance,
            server_pending: file.cached_pending,
            node_id,
            since_flush: 0.0,
            since_persist: 0.0,
            // First heartbeat fires quickly so the badge syncs on launch.
            since_heartbeat: HEARTBEAT_INTERVAL_SECS - 5.0,
            persist_path,
            display_dirty: true,
            earn_state: EarnState::SignedOut,
            auth_rejected: false,
            rejected_token: None,
            last_renewal: -RENEW_GAP_SECS,
            consecutive_failures: 0,
        }
    }
}

impl BlissTracker {
    /// Update the earn state, repainting the badge only on a real change.
    fn set_earn_state(&mut self, state: EarnState) {
        if self.earn_state != state {
            self.earn_state = state;
            self.display_dirty = true;
        }
    }

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
            next_chunk_id: self.next_chunk_id,
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

    // Earn state is derived in `drain_net_events`; these are pure guards.
    let Some(auth) = auth else { return };
    if auth.status != AuthStatus::LoggedIn {
        return;
    }
    // A local-only identity (no private key / offline / unregistered) leaves
    // `token` set to the public key, which the witness rejects. Attempting
    // anyway burned the rate limit and looked identical to "earning" from
    // the UI, so refuse to send and let the badge explain why.
    if !auth.jwt_valid {
        return;
    }
    // A 401 already told us this session's credentials are dead; retrying
    // every 5 min just refunds and re-submits forever.
    if tracker.auth_rejected {
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

    let authed = auth
        .as_ref()
        .filter(|a| a.status == AuthStatus::LoggedIn && a.jwt_valid);
    let user_id = authed.and_then(|a| a.user.as_ref()).map(|u| u.id.clone());
    let token = authed.and_then(|a| a.token.clone());
    let mode = bliss_state
        .map(|b| b.mode.clone())
        .unwrap_or_else(|| "Light".to_string());

    let _ = net.job_tx.send(NetJob::Heartbeat {
        node_id: tracker.node_id.clone(),
        mode,
        user_id,
        uptime_secs: time.elapsed_secs_f64() as u64,
        token,
    });
}

/// Apply network events and refresh the badge display strings.
fn drain_net_events(
    mut tracker: ResMut<BlissTracker>,
    net: Res<BlissNet>,
    mut display: ResMut<BlissNodeState>,
    mut auth: Option<ResMut<AuthState>>,
    output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
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
                let chunk = tracker.inflight.remove(&chunk_id);
                tracker.consecutive_failures = 0;
                if score_added > 0.0 {
                    tracker.server_pending += score_added;
                    if let Some(ref mut out) = output {
                        out.info(format!(
                            "Bliss: +{score_added:.1} pts co-signed ({contribution_type})"
                        ));
                    }
                } else if let Some((idx, secs)) = chunk {
                    // A 200 that credited nothing means the claim was clamped
                    // (daily cap reached, or ActiveTime beyond observed
                    // presence). Dropping the chunk here destroyed real work —
                    // an offline backlog could never be paid because every
                    // flush silently consumed it. Put it back and let it
                    // settle once headroom exists.
                    tracker.buckets[idx] += secs;
                }
                tracker.display_dirty = true;
            }
            NetEvent::CosignFail { chunk_id, error } => {
                // Refund the chunk — it re-flushes next cycle.
                if let Some((idx, secs)) = tracker.inflight.remove(&chunk_id) {
                    tracker.buckets[idx] += secs;
                }
                // A 401/403 is not transient: the credentials are wrong and
                // every retry burns rate limit while looking like progress.
                // Latch it, surface it, and stop until re-auth.
                if is_auth_refusal(&error) {
                    let token = auth.as_ref().and_then(|a| a.get_token().map(str::to_string));
                    latch_refusal(&mut tracker, token, &mut output);
                } else {
                    // Non-auth failures were invisible: the badge kept saying
                    // "Earning" while every submission failed forever (e.g. a
                    // 404 from a missing user record). Surface it after a few
                    // consecutive failures rather than only on 401/403.
                    tracker.consecutive_failures += 1;
                    if tracker.consecutive_failures == SYNC_FAILURE_VISIBLE_AFTER {
                        tracker.set_earn_state(EarnState::NotSyncing);
                        if let Some(ref mut out) = output {
                            out.warn(format!(
                                "Bliss: {} consecutive sync failures — work is saved locally \
                                 but not being credited. Last error: {error}",
                                tracker.consecutive_failures
                            ));
                        }
                    }
                    debug!("Bliss cosign failed (kept locally, will retry): {error}");
                }
            }
            NetEvent::HeartbeatRefused => {
                let token = auth.as_ref().and_then(|a| a.get_token().map(str::to_string));
                latch_refusal(&mut tracker, token, &mut output);
            }
            NetEvent::Balance {
                bliss_balance,
                pending_score,
            } => {
                tracker.server_balance = bliss_balance;
                tracker.server_pending = pending_score;
                tracker.display_dirty = true;
                // The witness answered for this account, so whatever caused
                // an earlier 401 is resolved — allow flushing again.
                if tracker.auth_rejected {
                    tracker.auth_rejected = false;
                    tracker.rejected_token = None;
                }
                if let Some(ref mut auth) = auth {
                    if let Some(ref mut user) = auth.user {
                        user.bliss_balance = bliss_balance;
                    }
                }
            }
        }
    }

    // -- Refused token ------------------------------------------------------
    // A sign-in since the refusal holds a new token, which the refusal says
    // nothing about: clear the latch now rather than wait for a heartbeat.
    // While the refused token is still the session's, sign in again with the
    // identity file, so a token that expired after its 72 hours renews itself
    // instead of leaving Studio unable to earn until someone notices.
    if tracker.auth_rejected {
        let current = auth.as_ref().and_then(|a| a.get_token().map(str::to_string));
        if latch_outlived(tracker.rejected_token.as_deref(), current.as_deref()) {
            tracker.auth_rejected = false;
            tracker.rejected_token = None;
        } else if current.is_some() && tracker.now - tracker.last_renewal >= RENEW_GAP_SECS {
            let identity = auth.as_ref().and_then(|a| a.identity.clone()).or_else(|| {
                settings
                    .as_ref()
                    .and_then(|s| s.saved_identities.first())
                    .map(|i| std::path::PathBuf::from(&i.path))
            });
            if let (Some(path), Some(auth)) = (identity, auth.as_mut()) {
                tracker.last_renewal = tracker.now;
                auth.sign_in_with_identity(path, crate::auth::SignInSource::Renew);
                if let Some(ref mut out) = output {
                    out.info("Bliss: signing in again with your identity file".to_string());
                }
            }
        }
    }

    // -- Earn state ---------------------------------------------------------
    // Derived here (every frame) rather than in `flush_contributions` (every
    // 5 min) so toggling Bliss or signing in updates the badge immediately.
    // Single authority — do not also set this from the flush path.
    let rejected = tracker.auth_rejected;
    let failing = tracker.consecutive_failures >= SYNC_FAILURE_VISIBLE_AFTER;
    let derived = if !display.enabled {
        EarnState::Disabled
    } else {
        match auth.as_ref() {
            Some(a) if a.status == AuthStatus::LoggedIn => {
                if !a.jwt_valid || rejected {
                    EarnState::NotAuthenticated
                } else if failing {
                    EarnState::NotSyncing
                } else {
                    EarnState::Earning
                }
            }
            _ => EarnState::SignedOut,
        }
    };
    tracker.set_earn_state(derived);

    // -- Badge display strings ---------------------------------------------
    // Balance is the ledger truth; pending = server score + local unsent
    // estimate so the badge visibly responds to work within seconds. When
    // earning is blocked, the pending line says WHY instead of showing a
    // climbing number that will never be credited.
    if tracker.display_dirty {
        tracker.display_dirty = false;
        // 2dp everywhere — the ledger stores integer minor units
        // (1 BLS = 100), so more digits would be inventing precision
        // that does not exist and rendering float noise.
        display.balance = format!("{:.2}", tracker.server_balance);
        display.balance_short = format!("{:.2}", tracker.server_balance);
        display.pending = match tracker.earn_state.hint() {
            Some(hint) => hint.to_string(),
            None => {
                let pending_total =
                    tracker.server_pending + tracker.local_pending_estimate();
                format!("+{pending_total:.1} pts today")
            }
        };
    }
}

/// Latch a refusal of `token` (the session's token when it was refused):
/// flushing stops, the badge says why, and the Output says so once.
fn latch_refusal(
    tracker: &mut BlissTracker,
    token: Option<String>,
    output: &mut Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
) {
    if tracker.auth_rejected {
        return;
    }
    tracker.auth_rejected = true;
    tracker.rejected_token = token;
    tracker.set_earn_state(EarnState::NotAuthenticated);
    if let Some(out) = output.as_mut() {
        out.warn(
            "Bliss: Eustress no longer accepts this sign-in (it expires after 72 hours). \
             Work is saved locally and is credited once Studio signs in again."
                .to_string(),
        );
    }
}

/// Whether a refusal latched on `rejected` no longer applies to the session,
/// which now holds `current`: it signed in again (a new token) or signed out.
fn latch_outlived(rejected: Option<&str>, current: Option<&str>) -> bool {
    rejected != current
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

pub struct BlissTrackerPlugin;

impl Plugin for BlissTrackerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlissTracker>()
            .insert_resource(BlissNet::spawn(
                // The witness: the API in use, whose Worker serves both
                // registration and co-signing (`eustress_common::api_base`).
                eustress_common::api_base::api_base().to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_token_is_told_from_other_failures() {
        assert!(is_auth_refusal("Unauthorized"));
        assert!(is_auth_refusal("heartbeat HTTP 401 Unauthorized"));
        assert!(is_auth_refusal("heartbeat HTTP 403 Forbidden"));
        assert!(!is_auth_refusal("Duplicate contribution hash"));
        assert!(!is_auth_refusal("Rate limit: 120 cosigns/hour"));
    }

    #[test]
    fn a_new_sign_in_outlives_the_refusal() {
        // The refused token is still the session's: the latch holds.
        assert!(!latch_outlived(Some("old"), Some("old")));
        // Signed in again: a new token the refusal says nothing about.
        assert!(latch_outlived(Some("old"), Some("new")));
        // Signed out: the badge says "Sign in to earn BLS" instead.
        assert!(latch_outlived(Some("old"), None));
    }

    #[test]
    fn the_badge_hints_carry_no_em_dash() {
        for state in [
            EarnState::SignedOut,
            EarnState::NotAuthenticated,
            EarnState::NotSyncing,
            EarnState::Disabled,
        ] {
            let hint = state.hint().unwrap();
            assert!(!hint.contains('\u{2014}'), "{hint}");
        }
        assert_eq!(EarnState::NotAuthenticated.hint(), Some("Identity not verified: cannot earn"));
    }
}
