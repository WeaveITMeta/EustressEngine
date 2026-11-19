//! Fleet placement for Eustress-hosted sessions.
//!
//! One hosted session is one `eustress-engine --server` process. This module
//! decides which rented machine a session runs on, when a warm (empty, 5 Hz)
//! server is started or stopped, and when to recommend adding or releasing a
//! machine. It is pure: no I/O, no clock reads (`now` is passed in, in
//! seconds), and the same input always gives the same actions.
//!
//! ## Numbers
//!
//! CPU is counted in millithreads (1000 = one logical thread).
//!
//! | Field | Default | Label |
//! |---|---|---|
//! | `occupied_millis`, CPU of a session with players at 60 Hz | 800 | MEASURED (0.75 to 0.85 thread, 2026-11-14, i9-10980XE, 2-player Pong, rounded up to the budget) |
//! | `empty_millis`, CPU of an empty server at the 5 Hz idle rate | 63 | TARGET (0.76 x 5 / 60; not measured) |
//! | `session_memory_mb` | 60 | MEASURED (57 to 60 MB private memory) |
//! | `utilisation_permille`, the committed-CPU line | 800 | CONFIG DEFAULT |
//! | `activation_permille`, the all-warm-servers-activated line | 900 | CONFIG DEFAULT (decision 2, 2026-11-19: room for the measured 0.85-thread upper bound) |
//! | `memory_permille` | 800 | CONFIG DEFAULT |
//! | `occupancy_permille`, sessions with players over sessions running | 800 | CONFIG DEFAULT |
//! | `occupied_hz` / `idle_hz` | 60 / 5 | CONFIG DEFAULT (60 is also the floor) |
//! | `empty_hold_secs`, how long an empty server is kept before a stop | 120 | CONFIG DEFAULT |
//! | `demand_window_secs`, how long a join request counts as recent | 600 | CONFIG DEFAULT |
//! | `start_lead_secs`, time from start to hosting | 60 | TARGET (Space load time is not measured) |
//! | `warm_floor_max` | 2 | CONFIG DEFAULT |
//!
//! ## Per-machine cap
//!
//! A session is occupied when `players + reserved > 0` (a reservation is a
//! player the plan has sent but who has not arrived yet). For a machine with
//! `T` threads and `M` MB:
//!
//! ```text
//! committed  = occupied * occupied_millis + empty * empty_millis
//! activation = (occupied + empty) * occupied_millis
//! place one more session only if
//!     committed + cost       <= T * utilisation_permille      (80% of the threads)
//!     activation + occupied_millis <= T * activation_permille (90% of the threads)
//!     (sessions + 1) * session_memory_mb <= M * memory_permille / 1000
//! ```
//!
//! `cost` is `occupied_millis` for a session started for a player and
//! `empty_millis` for a warm server. The second line is the wiggle room for
//! activation: every warm server on the machine can go to 60 Hz at the same
//! moment and the machine still stays at or under 90% of its threads. The
//! last 10% is the room for the measured spread: a session's budget is 0.80
//! thread and its measured upper bound 0.85, so `T` sessions at that bound on
//! `T` threads use 85%. It bounds the warm servers a machine may hold:
//!
//! ```text
//! warm room     = min((T*800 - committed) / empty_millis, (T*900 - activation) / occupied_millis, memory room)
//! occupied room = min((T*800 - committed) / occupied_millis, (T*900 - activation) / occupied_millis, memory room)
//! ```
//!
//! At the defaults a 4-thread machine holds 4 occupied sessions (3200 / 800)
//! and at most 4 sessions of any kind (3600 / 800 = 4.5, rounded down); an
//! 8-thread machine holds 8 occupied and at most 9 (7200 / 800). A player
//! joining a warm server moves it from the empty to the occupied budget; that
//! move is not a placement, so it may lift `committed` past the 80% line,
//! never past 90%.
//! A warm server that is still loading, or within 30 s of its last player,
//! runs at the full rate for that time; that is the same activation room.
//! The policy never lowers a frame rate to fit more sessions: there is no
//! action that does, and [`FleetConfig::validate`] refuses `occupied_hz`
//! under 60.
//!
//! ## GPU (docs/architecture/GPU_HOSTING_RULE.md)
//!
//! A session is CPU-only unless its listing says otherwise. A listing's `gpu`
//! is [`GpuMode::None`] (the default), [`GpuMode::Optional`] or
//! [`GpuMode::Required`] with `gpu_min_vram_mb`. `None` and `Optional`
//! place exactly as above, on any machine. `Required` places only on a
//! machine that reports `gpu_available` and has at least `gpu_min_vram_mb`
//! of VRAM not already held by its other GPU sessions:
//!
//! ```text
//! gpu fits = gpu_available && gpu_vram_mb - sum(session.gpu_vram_mb) >= gpu_min_vram_mb
//! ```
//!
//! and the CPU and memory lines above still apply. The new session holds
//! `gpu_min_vram_mb` (its `gpu_vram_mb`) for as long as it runs. A seat in a
//! running, warm or starting session of a `Required` listing is taken only
//! on a machine with a GPU of at least that size. When no machine fits, the
//! request is refused with [`RefuseReason::NoGpuCapacity`]; it is never
//! placed on a machine without one.
//!
//! ## Warm pool
//!
//! `O` is the occupied sessions across the fleet and `t` the occupancy target
//! (0.80). For each listing `L` with a join request in the last
//! `demand_window_secs`:
//!
//! ```text
//! floor_L   = 1 if no running occupied session of L has a free seat, else 0
//! wanted_L  = ceil(cold_L * start_lead_secs / demand_window_secs)
//! desired_L = max(floor_L, wanted_L)
//! cap       = floor(O * (1 - t) / t)            (floor(O / 4) at t = 0.80)
//! W         = min(sum desired_L, max(cap, min(sum floor_L, warm_floor_max)))
//! ```
//!
//! `cold_L` counts recent requests of L that needed a new session (a warm
//! server or a start). With `W <= cap`, occupancy `O / (O + W)` is at least
//! `t`; the floor term lets up to `warm_floor_max` warm servers exist while
//! `O` is small, so once `O >= 8` occupancy is at least 0.80 at the defaults.
//! `W` is shared out in order of each listing's latest request, floors first.
//! Empty servers beyond a listing's share are stopped once they have been
//! empty, and running, for `empty_hold_secs` (the hysteresis): emptiest
//! machines first, so a machine can drain and be released.
//!
//! ## Machines
//!
//! Renting is a person's decision. The plan only recommends, through
//! forge-orchestration's [`AutoscalerConfig`] thresholds over the fleet's
//! committed CPU as a share of its 80% lines: add at `upscale_threshold`
//! (0.8), or at once when a player was refused; release a machine with no
//! occupied session when the rest would sit at or under
//! `downscale_threshold` (0.3); never inside `hysteresis_secs` (300) of the
//! last machine change, except for a refusal.
//!
//! ## forge-orchestration
//!
//! Used: [`NodeResources`] and [`ResourceRequirements`] hold each machine's
//! two capacity views (the 80% line and the 90% activation line) and
//! `NodeResources::can_fit` decides placement; [`PlacementConstraint::Region`]
//! matches a request's region against the machine's `region` label;
//! [`AutoscalerConfig`] carries the machine thresholds and hysteresis and is
//! validated by its own `validate`; [`ScalingDecision`] is the machine verdict.
//!
//! Set aside, one line each:
//! - `Autoscaler` / `ThresholdPolicy`: async, and the hysteresis reads `Instant::now`; this module takes time as input.
//! - `MetricsSnapshot::new`: stamps `Instant::now()`.
//! - `Workload::new` (so `Scheduler`, `OptimizedScheduler`, `FFDBinPacker`, `WorkloadBatch`): stamps `chrono::Utc::now()`, and their single capacity cannot express the second, activation, line.
//! - `BinPackScheduler::score`: penalises a machine the session barely fits, so it would not pick the fullest machine that still fits.
//! - `OptimizedScheduler`: interior locks and timing counters; its best-fit score ranks by leftover share, not by how full the machine is.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use forge_orchestration::autoscaler::{AutoscalerConfig, ScalingDecision};
use forge_orchestration::scheduler::{NodeResources, PlacementConstraint, ResourceRequirements};
use forge_orchestration::types::NodeId;
use serde::{Deserialize, Serialize};

use crate::local_launch::{rank_servers, RankServer};

/// MEASURED: 0.75 to 0.85 thread per 2-player session at 60 Hz, rounded up.
pub const OCCUPIED_MILLIS: u64 = 800;
/// TARGET: an empty server at 5 Hz, 0.76 x 5 / 60. Not measured.
pub const EMPTY_MILLIS: u64 = 63;
/// MEASURED: 57 to 60 MB private memory per session.
pub const SESSION_MEMORY_MB: u64 = 60;
/// CONFIG DEFAULT: the policy commits at most 80% of a machine's threads.
pub const UTILISATION_PERMILLE: u64 = 800;
/// CONFIG DEFAULT: every session at 60 Hz at once stays within 90% of the
/// threads (decision 2). `infrastructure/cloudflare/api/src/fleet_policy.mjs`
/// holds the same number; `the_worker_port_holds_the_same_numbers` pins them.
pub const ACTIVATION_PERMILLE: u64 = 900;
/// CONFIG DEFAULT: sessions use at most 80% of a machine's memory.
pub const MEMORY_PERMILLE: u64 = 800;
/// CONFIG DEFAULT: at least 80% of running sessions have players.
pub const OCCUPANCY_PERMILLE: u64 = 800;
/// CONFIG DEFAULT and floor: an occupied session runs at 60 Hz or faster.
pub const OCCUPIED_HZ: u32 = 60;
/// The lowest `occupied_hz` the policy accepts.
pub const MIN_OCCUPIED_HZ: u32 = 60;
/// The engine's ceiling for `EUSTRESS_SERVER_HZ`.
pub const MAX_OCCUPIED_HZ: u32 = 120;
/// CONFIG DEFAULT: the engine's idle rate with nobody in the session.
pub const IDLE_HZ: u32 = 5;
/// CONFIG DEFAULT: an empty server is kept this long before a stop.
pub const EMPTY_HOLD_SECS: u64 = 120;
/// CONFIG DEFAULT: a join request counts as recent this long.
pub const DEMAND_WINDOW_SECS: u64 = 600;
/// TARGET: seconds from start to hosting. Not measured.
pub const START_LEAD_SECS: u64 = 60;
/// CONFIG DEFAULT: warm servers the floor may keep while few sessions run.
pub const WARM_FLOOR_MAX: u32 = 2;
/// CONFIG DEFAULT: a player waiting on a start asks again after this long.
pub const RETRY_AFTER_SECS: u64 = 5;
/// The least VRAM a listing that requires a GPU may ask for.
pub const MIN_GPU_VRAM_MB: u64 = 256;
/// The most VRAM a listing that requires a GPU may ask for (256 GB).
pub const MAX_GPU_VRAM_MB: u64 = 262_144;

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// What a listing asks of its host's GPU (the listing's `gpu`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuMode {
    /// CPU only. The default.
    #[default]
    None,
    /// May use a GPU for anything that is not authoritative; places as `None`.
    Optional,
    /// Placed only on a machine with a GPU and `gpu_min_vram_mb` of free VRAM.
    Required,
}

impl GpuMode {
    pub fn is_none(&self) -> bool {
        *self == GpuMode::None
    }
}

/// The VRAM one session of a listing holds on its machine's GPU:
/// `gpu_min_vram_mb` when the listing requires a GPU, else 0.
pub fn gpu_hold_mb(mode: GpuMode, min_vram_mb: u64) -> u64 {
    match mode {
        GpuMode::Required => min_vram_mb,
        GpuMode::None | GpuMode::Optional => 0,
    }
}

/// The placement policy's settings. Defaults are the constants above.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FleetConfig {
    pub occupied_millis: u64,
    pub empty_millis: u64,
    pub session_memory_mb: u64,
    pub utilisation_permille: u64,
    pub activation_permille: u64,
    pub memory_permille: u64,
    pub occupancy_permille: u64,
    pub occupied_hz: u32,
    pub idle_hz: u32,
    pub empty_hold_secs: u64,
    pub demand_window_secs: u64,
    pub start_lead_secs: u64,
    pub warm_floor_max: u32,
    pub retry_after_secs: u64,
    /// Machine add and release thresholds over the fleet's committed share of
    /// its 80% lines, and the hysteresis between machine changes.
    pub machines: AutoscalerConfig,
}

impl Default for FleetConfig {
    fn default() -> Self {
        Self {
            occupied_millis: OCCUPIED_MILLIS,
            empty_millis: EMPTY_MILLIS,
            session_memory_mb: SESSION_MEMORY_MB,
            utilisation_permille: UTILISATION_PERMILLE,
            activation_permille: ACTIVATION_PERMILLE,
            memory_permille: MEMORY_PERMILLE,
            occupancy_permille: OCCUPANCY_PERMILLE,
            occupied_hz: OCCUPIED_HZ,
            idle_hz: IDLE_HZ,
            empty_hold_secs: EMPTY_HOLD_SECS,
            demand_window_secs: DEMAND_WINDOW_SECS,
            start_lead_secs: START_LEAD_SECS,
            warm_floor_max: WARM_FLOOR_MAX,
            retry_after_secs: RETRY_AFTER_SECS,
            machines: AutoscalerConfig::default(),
        }
    }
}

impl FleetConfig {
    /// Refuses a configuration that would break a target: an occupied rate
    /// under 60 Hz, a committed line above the activation line, an
    /// activation line above 100%, or an occupancy target of 0 or 100%.
    pub fn validate(&self) -> Result<(), String> {
        if self.occupied_hz < MIN_OCCUPIED_HZ || self.occupied_hz > MAX_OCCUPIED_HZ {
            return Err(format!(
                "occupied_hz is {}; an occupied session runs at {MIN_OCCUPIED_HZ} to {MAX_OCCUPIED_HZ} Hz",
                self.occupied_hz
            ));
        }
        if self.idle_hz == 0 || self.idle_hz >= self.occupied_hz {
            return Err(format!("idle_hz is {}; it must be at least 1 and under occupied_hz", self.idle_hz));
        }
        if self.occupied_millis == 0 || self.empty_millis == 0 || self.empty_millis > self.occupied_millis {
            return Err("empty_millis must be at least 1 and at most occupied_millis".into());
        }
        if self.session_memory_mb == 0 {
            return Err("session_memory_mb must be at least 1".into());
        }
        if self.utilisation_permille == 0
            || self.utilisation_permille > self.activation_permille
            || self.activation_permille > 1000
        {
            return Err("0 < utilisation_permille <= activation_permille <= 1000".into());
        }
        if self.memory_permille == 0 || self.memory_permille > 1000 {
            return Err("memory_permille must be 1 to 1000".into());
        }
        if self.occupancy_permille == 0 || self.occupancy_permille >= 1000 {
            return Err("occupancy_permille must be 1 to 999".into());
        }
        if self.demand_window_secs == 0 {
            return Err("demand_window_secs must be at least 1".into());
        }
        self.machines.validate().map_err(|e| e.to_string())
    }
}

/// Where a session is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    /// Started; the Space is loading and the server is not answering yet.
    Starting,
    /// Answering joins.
    Hosting,
    /// Asked to stop. Still running, so still counted for CPU and memory.
    Stopping,
}

/// One simulation server on one machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub listing_id: String,
    pub phase: SessionPhase,
    pub players: u32,
    /// Players sent here who have not arrived yet.
    pub reserved: u32,
    pub max_players: u32,
    pub started_at: u64,
    /// When `players + reserved` last fell to 0. `None` while occupied.
    pub empty_since: Option<u64>,
    /// VRAM this session holds on its machine's GPU: its listing's
    /// `gpu_min_vram_mb` when the listing requires a GPU, else 0. The control
    /// plane sets it from its own record of the start, never from a host.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gpu_vram_mb: u64,
}

impl Session {
    pub fn seated(&self) -> u32 {
        self.players.saturating_add(self.reserved)
    }

    pub fn occupied(&self) -> bool {
        self.seated() > 0
    }

    pub fn has_room(&self) -> bool {
        self.max_players > 0 && self.seated() < self.max_players
    }

    /// Empty and not on its way out: part of the warm pool.
    pub fn is_warm(&self) -> bool {
        !self.occupied() && self.phase != SessionPhase::Stopping
    }

    pub fn empty_for(&self, now: u64) -> u64 {
        self.empty_since.map_or(0, |since| now.saturating_sub(since))
    }

    /// Committed CPU: the occupied budget with players, the empty budget without.
    pub fn cost_millis(&self, cfg: &FleetConfig) -> u64 {
        if self.occupied() {
            cfg.occupied_millis
        } else {
            cfg.empty_millis
        }
    }
}

/// One rented machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    pub threads: u32,
    pub memory_mb: u64,
    pub region: String,
    /// A person marked it for release: nothing new is placed on it.
    pub draining: bool,
    /// The host reports a hardware GPU (`gpu_probe`): not a software or
    /// basic display adapter. Only a listing that requires a GPU reads it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub gpu_available: bool,
    /// That GPU's VRAM in MB; 0 where the platform does not expose it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gpu_vram_mb: u64,
    pub sessions: Vec<Session>,
}

impl Machine {
    /// VRAM held by this machine's GPU sessions.
    pub fn gpu_vram_used_mb(&self) -> u64 {
        self.sessions.iter().map(|s| s.gpu_vram_mb).sum()
    }

    /// A GPU host with at least `need_mb` of VRAM in all. A seat in a session
    /// already running here asks this; the session holds its VRAM already.
    pub fn gpu_class_fits(&self, need_mb: u64) -> bool {
        self.gpu_available && self.gpu_vram_mb >= need_mb
    }

    /// One more session holding `need_mb` fits this machine's GPU.
    pub fn gpu_room_fits(&self, need_mb: u64) -> bool {
        self.gpu_available && self.gpu_vram_mb.saturating_sub(self.gpu_vram_used_mb()) >= need_mb
    }

    pub fn committed_cap_millis(&self, cfg: &FleetConfig) -> u64 {
        u64::from(self.threads) * cfg.utilisation_permille
    }

    pub fn activation_cap_millis(&self, cfg: &FleetConfig) -> u64 {
        u64::from(self.threads) * cfg.activation_permille
    }

    pub fn memory_cap_mb(&self, cfg: &FleetConfig) -> u64 {
        self.memory_mb.saturating_mul(cfg.memory_permille) / 1000
    }

    pub fn committed_millis(&self, cfg: &FleetConfig) -> u64 {
        self.sessions.iter().map(|s| s.cost_millis(cfg)).sum()
    }

    /// CPU if every session on the machine ran at the occupied rate at once.
    pub fn activation_millis(&self, cfg: &FleetConfig) -> u64 {
        self.sessions.len() as u64 * cfg.occupied_millis
    }

    pub fn memory_used_mb(&self, cfg: &FleetConfig) -> u64 {
        self.sessions.len() as u64 * cfg.session_memory_mb
    }

    /// Committed CPU as millionths of the 80% line.
    pub fn fill_ppm(&self, cfg: &FleetConfig) -> u64 {
        self.committed_millis(cfg) * 1_000_000 / self.committed_cap_millis(cfg).max(1)
    }

    fn node_id() -> NodeId {
        // The views are throwaway; the machine's own `id` is its identity.
        NodeId::from_uuid(uuid::Uuid::nil())
    }

    /// The 80% line as a forge-orchestration node.
    fn committed_view(&self, cfg: &FleetConfig) -> NodeResources {
        let mut node = NodeResources::new(Self::node_id(), self.committed_cap_millis(cfg), self.memory_cap_mb(cfg))
            .with_label("region", self.region.clone());
        node.cpu_allocated = self.committed_millis(cfg);
        node.memory_allocated = self.memory_used_mb(cfg);
        node.schedulable = !self.draining;
        node
    }

    /// The 90% activation line as a forge-orchestration node.
    fn activation_view(&self, cfg: &FleetConfig) -> NodeResources {
        let mut node = NodeResources::new(Self::node_id(), self.activation_cap_millis(cfg), 0);
        node.cpu_allocated = self.activation_millis(cfg);
        node.schedulable = !self.draining;
        node
    }
}

/// What kind of session a placement adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// Started for a player who is waiting.
    Occupied,
    /// Started empty, to idle at 5 Hz until a player arrives.
    Warm,
}

/// A machine's lines, what is committed against them, and the room left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capacity {
    pub committed_cap_millis: u64,
    pub committed_millis: u64,
    pub activation_cap_millis: u64,
    pub activation_millis: u64,
    pub memory_cap_mb: u64,
    pub memory_used_mb: u64,
    pub sessions: u32,
    /// More sessions for waiting players that fit now.
    pub occupied_room: u64,
    /// More warm servers that fit now.
    pub warm_room: u64,
}

pub fn capacity(machine: &Machine, cfg: &FleetConfig) -> Capacity {
    let committed_cap = machine.committed_cap_millis(cfg);
    let committed = machine.committed_millis(cfg);
    let activation_cap = machine.activation_cap_millis(cfg);
    let activation = machine.activation_millis(cfg);
    let memory_cap = machine.memory_cap_mb(cfg);
    let memory_used = machine.memory_used_mb(cfg);
    let (occupied_room, warm_room) = if machine.draining {
        (0, 0)
    } else {
        let cpu_left = committed_cap.saturating_sub(committed);
        let activation_room = activation_cap.saturating_sub(activation) / cfg.occupied_millis.max(1);
        let memory_room = memory_cap.saturating_sub(memory_used) / cfg.session_memory_mb.max(1);
        (
            (cpu_left / cfg.occupied_millis.max(1)).min(activation_room).min(memory_room),
            (cpu_left / cfg.empty_millis.max(1)).min(activation_room).min(memory_room),
        )
    };
    Capacity {
        committed_cap_millis: committed_cap,
        committed_millis: committed,
        activation_cap_millis: activation_cap,
        activation_millis: activation,
        memory_cap_mb: memory_cap,
        memory_used_mb: memory_used,
        sessions: machine.sessions.len() as u32,
        occupied_room,
        warm_room,
    }
}

/// Whether one more session of `kind` fits on `machine` (and in `region`,
/// when one is named) without crossing the 80% line, the 90% activation
/// line, or the memory line.
pub fn can_place(machine: &Machine, kind: PlaceKind, region: Option<&str>, cfg: &FleetConfig) -> bool {
    let node = machine.committed_view(cfg);
    if let Some(region) = region.filter(|r| !r.is_empty()) {
        if !PlacementConstraint::Region(region.to_string()).matches(&node) {
            return false;
        }
    }
    let cost = match kind {
        PlaceKind::Occupied => cfg.occupied_millis,
        PlaceKind::Warm => cfg.empty_millis,
    };
    node.can_fit(&ResourceRequirements::new().cpu(cost).memory(cfg.session_memory_mb))
        && machine
            .activation_view(cfg)
            .can_fit(&ResourceRequirements::new().cpu(cfg.occupied_millis).memory(0))
}

/// Pack tightly: the fullest machine (committed share of its 80% line) that
/// still fits. Ties go to more sessions, then the lower id. An emptied
/// machine is left empty, so it can be released.
pub fn choose_machine(machines: &[Machine], kind: PlaceKind, region: Option<&str>, cfg: &FleetConfig) -> Option<usize> {
    choose_gpu_machine(machines, kind, region, None, cfg)
}

/// [`choose_machine`] for a listing that requires a GPU when `gpu_need_mb`
/// is `Some`: only machines whose GPU has that much VRAM free are candidates,
/// and among them the same packing applies. `None` is [`choose_machine`].
pub fn choose_gpu_machine(
    machines: &[Machine],
    kind: PlaceKind,
    region: Option<&str>,
    gpu_need_mb: Option<u64>,
    cfg: &FleetConfig,
) -> Option<usize> {
    let mut best: Option<(usize, u64, usize)> = None;
    for (i, machine) in machines.iter().enumerate() {
        if !can_place(machine, kind, region, cfg) {
            continue;
        }
        if gpu_need_mb.is_some_and(|need| !machine.gpu_room_fits(need)) {
            continue;
        }
        let fill = machine.fill_ppm(cfg);
        let sessions = machine.sessions.len();
        let better = match best {
            None => true,
            Some((bi, bfill, bsessions)) => {
                fill > bfill
                    || (fill == bfill && (sessions > bsessions || (sessions == bsessions && machine.id < machines[bi].id)))
            }
        };
        if better {
            best = Some((i, fill, sessions));
        }
    }
    best.map(|(i, _, _)| i)
}

/// The fleet as the plan sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetState {
    pub machines: Vec<Machine>,
    /// When a machine was last added or released, for the hysteresis.
    pub last_machine_change_at: Option<u64>,
}

/// A player's Play click, waiting for an answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRequest {
    /// The caller's id for this click; the answer carries it back.
    pub request_id: String,
    pub listing_id: String,
    pub max_players: u32,
    pub region: Option<String>,
    pub at: u64,
    /// The listing's `gpu`. Absent is `none`.
    #[serde(default, skip_serializing_if = "GpuMode::is_none")]
    pub gpu: GpuMode,
    /// The listing's `gpu_min_vram_mb`; read only when `gpu` is `required`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gpu_min_vram_mb: u64,
}

impl JoinRequest {
    /// `Some(VRAM)` when this listing requires a GPU.
    pub fn gpu_need_mb(&self) -> Option<u64> {
        (self.gpu == GpuMode::Required).then_some(self.gpu_min_vram_mb)
    }
}

/// A join request already answered, kept for the warm pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentRequest {
    pub listing_id: String,
    pub at: u64,
    pub max_players: u32,
    pub region: Option<String>,
    /// It needed a warm server or a start, not a seat in a running session.
    pub needed_new_session: bool,
    /// The listing's `gpu` when the request was made. Absent is `none`.
    #[serde(default, skip_serializing_if = "GpuMode::is_none")]
    pub gpu: GpuMode,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gpu_min_vram_mb: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Demand {
    pub pending: Vec<JoinRequest>,
    pub recent: Vec<RecentRequest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Empty past the hold, and beyond the warm pool.
    SurplusEmpty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefuseReason {
    /// No machine has room for another 60 Hz session.
    NoCapacity,
    /// The listing requires a GPU and no machine with a GPU of at least its
    /// `gpu_min_vram_mb` free has room. Never placed on a CPU-only machine.
    NoGpuCapacity,
    /// No listing id, or a listing with no seats.
    InvalidRequest,
}

/// What the plan asks for. Machines are recommended, never rented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FleetAction {
    /// Send the player to a hosting session with a free seat.
    Route { request_id: String, listing_id: String, machine_id: String, session_id: String, warm: bool },
    /// The session is starting; ask again after `retry_after_secs`.
    Wait { request_id: String, listing_id: String, machine_id: String, session_id: String, retry_after_secs: u64 },
    /// Start a session for a waiting player. `session_id` is a placeholder
    /// (`planned-N`) the host replaces with its own id. `gpu_vram_mb` is the
    /// VRAM the session holds (0 for a CPU-only listing).
    StartForPlayer {
        request_id: String,
        listing_id: String,
        machine_id: String,
        session_id: String,
        max_players: u32,
        retry_after_secs: u64,
        #[serde(default, skip_serializing_if = "is_zero")]
        gpu_vram_mb: u64,
    },
    /// Start an empty server that idles at 5 Hz until a player arrives.
    StartWarm {
        listing_id: String,
        machine_id: String,
        session_id: String,
        max_players: u32,
        #[serde(default, skip_serializing_if = "is_zero")]
        gpu_vram_mb: u64,
    },
    /// Stop an empty server (the stop file).
    Stop { machine_id: String, session_id: String, listing_id: String, reason: StopReason },
    /// A person should rent a machine in `region` (empty means any), with a
    /// GPU of at least `gpu_vram_mb` when that is not 0.
    RecommendAddMachine {
        region: String,
        reason: String,
        #[serde(default, skip_serializing_if = "is_zero")]
        gpu_vram_mb: u64,
    },
    /// A person may release this machine once it is drained.
    RecommendReleaseMachine { machine_id: String, reason: String },
    /// No session for this click.
    Refuse { request_id: String, listing_id: String, reason: RefuseReason },
}

/// Occupied sessions and running sessions (`Stopping` excluded from both).
pub fn occupancy(state: &FleetState) -> (u64, u64) {
    let mut occupied = 0;
    let mut running = 0;
    for session in state.machines.iter().flat_map(|m| m.sessions.iter()) {
        if session.phase == SessionPhase::Stopping {
            continue;
        }
        running += 1;
        if session.occupied() {
            occupied += 1;
        }
    }
    (occupied, running)
}

/// The warm-pool size `W` for `occupied` sessions, given the summed floors
/// and desires (see the module doc).
pub fn warm_pool_size(occupied: u64, floor_total: u64, desired_total: u64, cfg: &FleetConfig) -> u64 {
    let permille = cfg.occupancy_permille.clamp(1, 999);
    let cap = occupied * (1000 - permille) / permille;
    desired_total.min(cap.max(floor_total.min(u64::from(cfg.warm_floor_max))))
}

/// The actions for this moment.
pub fn plan(now: u64, state: &FleetState, demand: &Demand, cfg: &FleetConfig) -> Vec<FleetAction> {
    project(now, state, demand, cfg).0
}

/// The actions, and the fleet as it will be once they are applied (started
/// sessions present as `Starting`, stopped ones as `Stopping`, routed
/// players as reservations).
pub fn project(now: u64, state: &FleetState, demand: &Demand, cfg: &FleetConfig) -> (Vec<FleetAction>, FleetState) {
    let mut fleet = state.clone();
    fleet.machines.sort_by(|a, b| a.id.cmp(&b.id));
    for machine in &mut fleet.machines {
        machine.sessions.sort_by(|a, b| a.id.cmp(&b.id));
    }
    let mut actions = Vec::new();
    let mut seq = 0u32;
    let mut recent: Vec<RecentRequest> =
        demand.recent.iter().filter(|r| in_window(now, r.at, cfg)).cloned().collect();
    let mut refused = Refused::default();

    let mut pending: Vec<&JoinRequest> = demand.pending.iter().collect();
    pending.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.request_id.cmp(&b.request_id)));
    for request in pending {
        let (action, needed_new) = place_request(now, &mut fleet, request, cfg, &mut seq);
        let invalid = matches!(action, FleetAction::Refuse { reason: RefuseReason::InvalidRequest, .. });
        let region = request.region.clone().unwrap_or_default();
        match &action {
            FleetAction::Refuse { reason: RefuseReason::NoCapacity, .. } => {
                refused.cpu.insert(region);
            }
            FleetAction::Refuse { reason: RefuseReason::NoGpuCapacity, .. } => {
                let need = refused.gpu.entry(region).or_insert(0);
                *need = (*need).max(request.gpu_min_vram_mb);
            }
            _ => {}
        }
        if !invalid {
            recent.push(RecentRequest {
                listing_id: request.listing_id.clone(),
                at: request.at.min(now),
                max_players: request.max_players,
                region: request.region.clone(),
                needed_new_session: needed_new,
                gpu: request.gpu,
                gpu_min_vram_mb: request.gpu_min_vram_mb,
            });
        }
        actions.push(action);
    }

    warm_pool(now, &mut fleet, &recent, cfg, &mut seq, &mut actions);
    machine_actions(now, &fleet, &refused, cfg, &mut actions);
    (actions, fleet)
}

/// The regions a click was refused in this plan: for CPU room, and for GPU
/// room with the largest VRAM asked.
#[derive(Default)]
struct Refused {
    cpu: BTreeSet<String>,
    gpu: BTreeMap<String, u64>,
}

impl Refused {
    fn any(&self) -> bool {
        !self.cpu.is_empty() || !self.gpu.is_empty()
    }
}

fn in_window(now: u64, at: u64, cfg: &FleetConfig) -> bool {
    at <= now && now - at <= cfg.demand_window_secs
}

fn ceil_div(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        a / b + u64::from(a % b != 0)
    }
}

fn next_planned(seq: &mut u32) -> String {
    let id = format!("planned-{}", *seq);
    *seq += 1;
    id
}

/// Seat the requester in session `(mi, si)`: a reservation, so the next
/// request sees one seat fewer.
fn seat(fleet: &mut FleetState, (mi, si): (usize, usize), request: &JoinRequest, cfg: &FleetConfig) -> FleetAction {
    let machine_id = fleet.machines[mi].id.clone();
    let session = &mut fleet.machines[mi].sessions[si];
    let warm = !session.occupied();
    session.reserved += 1;
    session.empty_since = None;
    if session.phase == SessionPhase::Hosting {
        FleetAction::Route {
            request_id: request.request_id.clone(),
            listing_id: request.listing_id.clone(),
            machine_id,
            session_id: session.id.clone(),
            warm,
        }
    } else {
        FleetAction::Wait {
            request_id: request.request_id.clone(),
            listing_id: request.listing_id.clone(),
            machine_id,
            session_id: session.id.clone(),
            retry_after_secs: cfg.retry_after_secs,
        }
    }
}

/// One click, in order of preference: a running session with players and a
/// free seat (busiest first, the Quick Join rank); a warm server (on the
/// machine furthest under its 80% line); a session already starting; a new
/// session on the fullest machine that fits; a refusal.
fn place_request(
    now: u64,
    fleet: &mut FleetState,
    request: &JoinRequest,
    cfg: &FleetConfig,
    seq: &mut u32,
) -> (FleetAction, bool) {
    if request.listing_id.is_empty() || request.max_players == 0 {
        let refuse = FleetAction::Refuse {
            request_id: request.request_id.clone(),
            listing_id: request.listing_id.clone(),
            reason: RefuseReason::InvalidRequest,
        };
        return (refuse, false);
    }
    let region = request.region.as_deref().filter(|r| !r.is_empty());
    let gpu_need = request.gpu_need_mb();
    // A listing that requires a GPU takes a seat only on a GPU host of its size.
    let in_region = |machine: &Machine| {
        region.map_or(true, |r| machine.region == r) && gpu_need.map_or(true, |need| machine.gpu_class_fits(need))
    };

    let mut rooms: Vec<(usize, usize)> = Vec::new();
    let mut warm: Vec<(u64, usize, usize)> = Vec::new();
    let mut starting: Vec<(bool, usize, usize)> = Vec::new();
    for (mi, machine) in fleet.machines.iter().enumerate() {
        if !in_region(machine) {
            continue;
        }
        let fill = machine.fill_ppm(cfg);
        for (si, session) in machine.sessions.iter().enumerate() {
            if session.listing_id != request.listing_id || !session.has_room() {
                continue;
            }
            match session.phase {
                SessionPhase::Hosting if session.occupied() => rooms.push((mi, si)),
                SessionPhase::Hosting => warm.push((fill, mi, si)),
                SessionPhase::Starting => starting.push((!session.occupied(), mi, si)),
                SessionPhase::Stopping => {}
            }
        }
    }

    let ranked = rank_servers(
        rooms
            .iter()
            .enumerate()
            .map(|(k, &(mi, si))| {
                let session = &fleet.machines[mi].sessions[si];
                RankServer {
                    id: k.to_string(),
                    players: session.seated(),
                    max_players: session.max_players,
                    tier: 0,
                    latency_ms: None,
                }
            })
            .collect(),
    );
    if let Some(best) = ranked.first().and_then(|r| r.id.parse::<usize>().ok()).and_then(|k| rooms.get(k).copied()) {
        return (seat(fleet, best, request, cfg), false);
    }

    warm.sort();
    if let Some(&(_, mi, si)) = warm.first() {
        return (seat(fleet, (mi, si), request, cfg), true);
    }

    starting.sort();
    if let Some(&(_, mi, si)) = starting.first() {
        return (seat(fleet, (mi, si), request, cfg), true);
    }

    if let Some(mi) = choose_gpu_machine(&fleet.machines, PlaceKind::Occupied, region, gpu_need, cfg) {
        let session_id = next_planned(seq);
        let machine_id = fleet.machines[mi].id.clone();
        let gpu_vram_mb = gpu_need.unwrap_or(0);
        fleet.machines[mi].sessions.push(Session {
            id: session_id.clone(),
            listing_id: request.listing_id.clone(),
            phase: SessionPhase::Starting,
            players: 0,
            reserved: 1,
            max_players: request.max_players,
            started_at: now,
            empty_since: None,
            gpu_vram_mb,
        });
        let start = FleetAction::StartForPlayer {
            request_id: request.request_id.clone(),
            listing_id: request.listing_id.clone(),
            machine_id,
            session_id,
            max_players: request.max_players,
            retry_after_secs: cfg.retry_after_secs,
            gpu_vram_mb,
        };
        return (start, true);
    }

    let refuse = FleetAction::Refuse {
        request_id: request.request_id.clone(),
        listing_id: request.listing_id.clone(),
        reason: if gpu_need.is_some() { RefuseReason::NoGpuCapacity } else { RefuseReason::NoCapacity },
    };
    (refuse, true)
}

struct ListingDemand {
    latest: u64,
    max_players: u32,
    region: Option<String>,
    cold: u64,
    gpu_need: Option<u64>,
}

struct WarmRow {
    listing_id: String,
    latest: u64,
    max_players: u32,
    region: Option<String>,
    floor: u64,
    desired: u64,
    gpu_need: Option<u64>,
}

fn warm_pool(
    now: u64,
    fleet: &mut FleetState,
    recent: &[RecentRequest],
    cfg: &FleetConfig,
    seq: &mut u32,
    actions: &mut Vec<FleetAction>,
) {
    let occupied = occupancy(fleet).0;

    let mut demand: BTreeMap<String, ListingDemand> = BTreeMap::new();
    for request in recent {
        if !in_window(now, request.at, cfg) || request.listing_id.is_empty() {
            continue;
        }
        let entry = demand.entry(request.listing_id.clone()).or_insert(ListingDemand {
            latest: request.at,
            max_players: request.max_players,
            region: request.region.clone(),
            cold: 0,
            gpu_need: None,
        });
        if request.at >= entry.latest {
            entry.latest = request.at;
            if request.max_players > 0 {
                entry.max_players = request.max_players;
            }
            entry.region = request.region.clone();
            // The listing's latest setting decides where its warm servers go.
            entry.gpu_need = (request.gpu == GpuMode::Required).then_some(request.gpu_min_vram_mb);
        }
        if request.needed_new_session {
            entry.cold += 1;
        }
    }

    let mut rows: Vec<WarmRow> = demand
        .iter()
        .map(|(listing_id, d)| {
            let room = fleet
                .machines
                .iter()
                .filter(|m| d.gpu_need.map_or(true, |need| m.gpu_class_fits(need)))
                .flat_map(|m| m.sessions.iter())
                .any(|s| &s.listing_id == listing_id && s.phase != SessionPhase::Stopping && s.occupied() && s.has_room());
            let floor = u64::from(!room);
            let wanted = ceil_div(d.cold * cfg.start_lead_secs, cfg.demand_window_secs);
            WarmRow {
                listing_id: listing_id.clone(),
                latest: d.latest,
                max_players: d.max_players.max(1),
                region: d.region.clone(),
                floor,
                desired: floor.max(wanted),
                gpu_need: d.gpu_need,
            }
        })
        .collect();

    let floor_total: u64 = rows.iter().map(|r| r.floor).sum();
    let desired_total: u64 = rows.iter().map(|r| r.desired).sum();
    let pool = warm_pool_size(occupied, floor_total, desired_total, cfg);

    rows.sort_by(|a, b| b.latest.cmp(&a.latest).then_with(|| a.listing_id.cmp(&b.listing_id)));
    let mut allot: BTreeMap<String, u64> = BTreeMap::new();
    let mut left = pool;
    for row in &rows {
        let give = row.floor.min(left);
        left -= give;
        *allot.entry(row.listing_id.clone()).or_insert(0) += give;
    }
    for row in &rows {
        let give = (row.desired - row.floor).min(left);
        left -= give;
        *allot.entry(row.listing_id.clone()).or_insert(0) += give;
    }

    let mut have: BTreeMap<String, u64> = BTreeMap::new();
    for session in fleet.machines.iter().flat_map(|m| m.sessions.iter()) {
        if session.is_warm() {
            *have.entry(session.listing_id.clone()).or_insert(0) += 1;
        }
    }

    for row in &rows {
        let want = allot.get(&row.listing_id).copied().unwrap_or(0);
        let mut count = have.get(&row.listing_id).copied().unwrap_or(0);
        while count < want {
            let Some(mi) =
                choose_gpu_machine(&fleet.machines, PlaceKind::Warm, row.region.as_deref(), row.gpu_need, cfg)
            else {
                break;
            };
            let session_id = next_planned(seq);
            let gpu_vram_mb = row.gpu_need.unwrap_or(0);
            fleet.machines[mi].sessions.push(Session {
                id: session_id.clone(),
                listing_id: row.listing_id.clone(),
                phase: SessionPhase::Starting,
                players: 0,
                reserved: 0,
                max_players: row.max_players,
                started_at: now,
                empty_since: Some(now),
                gpu_vram_mb,
            });
            actions.push(FleetAction::StartWarm {
                listing_id: row.listing_id.clone(),
                machine_id: fleet.machines[mi].id.clone(),
                session_id,
                max_players: row.max_players,
                gpu_vram_mb,
            });
            count += 1;
        }
    }

    for (listing_id, &count) in &have {
        let want = allot.get(listing_id).copied().unwrap_or(0);
        if count <= want {
            continue;
        }
        // Draining machines first, then machines past their 80% line, then
        // the emptiest machine, then the longest empty.
        let mut candidates: Vec<(bool, bool, u64, Reverse<u64>, usize, usize)> = Vec::new();
        for (mi, machine) in fleet.machines.iter().enumerate() {
            let over = machine.committed_millis(cfg) > machine.committed_cap_millis(cfg);
            let fill = machine.fill_ppm(cfg);
            for (si, session) in machine.sessions.iter().enumerate() {
                if &session.listing_id != listing_id || !session.is_warm() || session.phase != SessionPhase::Hosting {
                    continue;
                }
                let held = session.empty_for(now) >= cfg.empty_hold_secs
                    && now.saturating_sub(session.started_at) >= cfg.empty_hold_secs;
                if !held {
                    continue;
                }
                candidates.push((!machine.draining, !over, fill, Reverse(session.empty_for(now)), mi, si));
            }
        }
        candidates.sort();
        for &(_, _, _, _, mi, si) in candidates.iter().take((count - want) as usize) {
            let machine_id = fleet.machines[mi].id.clone();
            let session = &mut fleet.machines[mi].sessions[si];
            session.phase = SessionPhase::Stopping;
            actions.push(FleetAction::Stop {
                machine_id,
                session_id: session.id.clone(),
                listing_id: session.listing_id.clone(),
                reason: StopReason::SurplusEmpty,
            });
        }
    }
}

/// The machine verdict: `ScaleUp(1)`, `ScaleDown(1)` with the machine to
/// release, or `NoChange`. A refusal asks for a machine even inside the
/// hysteresis; everything else waits for it.
pub fn machine_decision(now: u64, fleet: &FleetState, refused: bool, cfg: &FleetConfig) -> (ScalingDecision, Option<usize>) {
    let limits = &cfg.machines;
    let count = fleet.machines.len() as u32;
    if refused {
        return if count < limits.max_instances.max(1) {
            (ScalingDecision::ScaleUp(1), None)
        } else {
            (ScalingDecision::NoChange, None)
        };
    }
    let settled = fleet
        .last_machine_change_at
        .map_or(true, |at| now.saturating_sub(at) >= limits.hysteresis_secs);
    if !settled {
        return (ScalingDecision::NoChange, None);
    }
    let mut cap = 0u64;
    let mut committed = 0u64;
    for machine in fleet.machines.iter().filter(|m| !m.draining) {
        cap += machine.committed_cap_millis(cfg);
        committed += machine.committed_millis(cfg);
    }
    if cap > 0 && committed as f64 / cap as f64 >= limits.upscale_threshold && count < limits.max_instances {
        return (ScalingDecision::ScaleUp(1), None);
    }
    if count <= limits.min_instances {
        return (ScalingDecision::NoChange, None);
    }
    let mut candidates: Vec<(bool, usize, u64, &str, usize)> = fleet
        .machines
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.sessions.iter().any(|s| s.phase != SessionPhase::Stopping && s.occupied()))
        .map(|(i, m)| (!m.draining, m.sessions.len(), m.committed_millis(cfg), m.id.as_str(), i))
        .collect();
    candidates.sort();
    for (_, _, _, _, i) in candidates {
        let machine = &fleet.machines[i];
        let (own_cap, own_committed) = if machine.draining {
            (0, 0)
        } else {
            (machine.committed_cap_millis(cfg), machine.committed_millis(cfg))
        };
        let rest_cap = cap.saturating_sub(own_cap);
        let rest_committed = committed.saturating_sub(own_committed);
        if rest_cap == 0 {
            continue;
        }
        if rest_committed as f64 / rest_cap as f64 <= limits.downscale_threshold {
            return (ScalingDecision::ScaleDown(1), Some(i));
        }
    }
    (ScalingDecision::NoChange, None)
}

fn machine_actions(
    now: u64,
    fleet: &FleetState,
    refused: &Refused,
    cfg: &FleetConfig,
    actions: &mut Vec<FleetAction>,
) {
    let place = |region: &str| if region.is_empty() { String::new() } else { format!(" in {region}") };
    match machine_decision(now, fleet, refused.any(), cfg) {
        (ScalingDecision::ScaleUp(_), _) if refused.any() => {
            for region in &refused.cpu {
                actions.push(FleetAction::RecommendAddMachine {
                    region: region.clone(),
                    reason: format!(
                        "a player was refused: no machine{} had room for another {} Hz session",
                        place(region.as_str()),
                        cfg.occupied_hz
                    ),
                    gpu_vram_mb: 0,
                });
            }
            for (region, &need) in &refused.gpu {
                actions.push(FleetAction::RecommendAddMachine {
                    region: region.clone(),
                    reason: format!(
                        "a player was refused: no machine{} with a GPU of at least {need} MB of VRAM free had room for another {} Hz session",
                        place(region.as_str()),
                        cfg.occupied_hz
                    ),
                    gpu_vram_mb: need.max(1),
                });
            }
        }
        (ScalingDecision::ScaleUp(_), _) => {
            let region = fleet
                .machines
                .iter()
                .filter(|m| !m.draining)
                .max_by(|a, b| a.fill_ppm(cfg).cmp(&b.fill_ppm(cfg)).then_with(|| b.id.cmp(&a.id)))
                .map(|m| m.region.clone())
                .unwrap_or_default();
            actions.push(FleetAction::RecommendAddMachine {
                region,
                reason: format!(
                    "committed CPU is at or past {:.0}% of the machines' {}% lines",
                    cfg.machines.upscale_threshold * 100.0,
                    cfg.utilisation_permille / 10
                ),
                gpu_vram_mb: 0,
            });
        }
        (ScalingDecision::ScaleDown(_), Some(i)) => {
            let machine = &fleet.machines[i];
            let warm = machine.sessions.iter().filter(|s| s.is_warm()).count();
            actions.push(FleetAction::RecommendReleaseMachine {
                machine_id: machine.id.clone(),
                reason: format!(
                    "no session with players; the rest of the fleet stays at or under {:.0}% of its lines without it; drain it first ({warm} warm server(s) stop with it)",
                    cfg.machines.downscale_threshold * 100.0
                ),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> FleetConfig {
        FleetConfig::default()
    }

    fn machine(id: &str, threads: u32) -> Machine {
        Machine {
            id: id.into(),
            threads,
            memory_mb: 16_384,
            region: "us-east-1".into(),
            draining: false,
            gpu_available: false,
            gpu_vram_mb: 0,
            sessions: Vec::new(),
        }
    }

    fn gpu_machine(id: &str, threads: u32, vram_mb: u64) -> Machine {
        Machine { gpu_available: true, gpu_vram_mb: vram_mb, ..machine(id, threads) }
    }

    fn occupied(id: &str, listing: &str, players: u32, max: u32) -> Session {
        Session {
            id: id.into(),
            listing_id: listing.into(),
            phase: SessionPhase::Hosting,
            players,
            reserved: 0,
            max_players: max,
            started_at: 0,
            empty_since: None,
            gpu_vram_mb: 0,
        }
    }

    fn warm(id: &str, listing: &str, since: u64) -> Session {
        Session {
            id: id.into(),
            listing_id: listing.into(),
            phase: SessionPhase::Hosting,
            players: 0,
            reserved: 0,
            max_players: 2,
            started_at: since,
            empty_since: Some(since),
            gpu_vram_mb: 0,
        }
    }

    fn request(id: &str, listing: &str, at: u64) -> JoinRequest {
        JoinRequest {
            request_id: id.into(),
            listing_id: listing.into(),
            max_players: 2,
            region: None,
            at,
            gpu: GpuMode::None,
            gpu_min_vram_mb: 0,
        }
    }

    fn gpu_request(id: &str, listing: &str, at: u64, vram_mb: u64) -> JoinRequest {
        JoinRequest { gpu: GpuMode::Required, gpu_min_vram_mb: vram_mb, ..request(id, listing, at) }
    }

    fn recent(listing: &str, at: u64) -> RecentRequest {
        RecentRequest {
            listing_id: listing.into(),
            at,
            max_players: 2,
            region: None,
            needed_new_session: true,
            gpu: GpuMode::None,
            gpu_min_vram_mb: 0,
        }
    }

    fn state(machines: Vec<Machine>) -> FleetState {
        FleetState { machines, last_machine_change_at: None }
    }

    fn starts_and_stops(actions: &[FleetAction]) -> (Vec<String>, Vec<String>) {
        let mut starts = Vec::new();
        let mut stops = Vec::new();
        for action in actions {
            match action {
                FleetAction::StartWarm { listing_id, .. } | FleetAction::StartForPlayer { listing_id, .. } => {
                    starts.push(listing_id.clone())
                }
                FleetAction::Stop { listing_id, .. } => stops.push(listing_id.clone()),
                _ => {}
            }
        }
        (starts, stops)
    }

    #[test]
    fn the_defaults_validate_and_occupied_never_runs_under_60_hz() {
        assert!(cfg().validate().is_ok());
        for hz in [0, 5, 20, 30, 59] {
            let low = FleetConfig { occupied_hz: hz, ..cfg() };
            assert!(low.validate().is_err(), "{hz} Hz must be refused");
        }
        assert!(FleetConfig { occupied_hz: 120, ..cfg() }.validate().is_ok());
        assert!(FleetConfig { occupied_hz: 121, ..cfg() }.validate().is_err());
        assert!(FleetConfig { idle_hz: 60, ..cfg() }.validate().is_err());
        assert!(FleetConfig { utilisation_permille: 1001, activation_permille: 1001, ..cfg() }.validate().is_err());
        assert!(FleetConfig { utilisation_permille: 900, activation_permille: 800, ..cfg() }.validate().is_err());
    }

    #[test]
    fn four_threads_hold_exactly_four_occupied_60_hz_sessions() {
        let cfg = cfg();
        let mut m = machine("m", 4);
        assert_eq!(capacity(&m, &cfg).occupied_room, 4);
        let mut placed = 0;
        for i in 0..10 {
            if !can_place(&m, PlaceKind::Occupied, None, &cfg) {
                break;
            }
            m.sessions.push(occupied(&format!("s{i}"), "pong", 2, 2));
            placed += 1;
        }
        assert_eq!(placed, 4, "3.2 threads / 0.8 thread");
        assert_eq!(m.committed_millis(&cfg), 3200);
        assert!(!can_place(&m, PlaceKind::Occupied, None, &cfg), "the fifth is refused");
        assert_eq!(capacity(&m, &cfg).occupied_room, 0);
    }

    #[test]
    fn activating_every_warm_server_at_once_never_passes_the_threads() {
        let cfg = cfg();
        for threads in 1..=64u32 {
            for occupied_first in 0..=threads * 2 {
                let mut m = machine("m", threads);
                for i in 0..occupied_first {
                    if can_place(&m, PlaceKind::Occupied, None, &cfg) {
                        m.sessions.push(occupied(&format!("o{i}"), "l", 1, 2));
                    }
                }
                let mut i = 0;
                while can_place(&m, PlaceKind::Warm, None, &cfg) {
                    m.sessions.push(warm(&format!("w{i}"), "l", 0));
                    i += 1;
                    assert!(i < 100_000);
                }
                let ninety_percent = u64::from(threads) * 900;
                assert!(m.committed_millis(&cfg) <= u64::from(threads) * 800, "committed past 80% on {threads} threads");
                let all_at_60_hz = m.sessions.len() as u64 * cfg.occupied_millis;
                assert!(
                    all_at_60_hz <= ninety_percent,
                    "{threads} threads, {} sessions: {all_at_60_hz} > {ninety_percent}",
                    m.sessions.len()
                );
            }
        }
        let mut empty = machine("m", 4);
        while can_place(&empty, PlaceKind::Warm, None, &cfg) {
            empty.sessions.push(warm(&format!("w{}", empty.sessions.len()), "l", 0));
        }
        assert_eq!(empty.sessions.len(), 4, "3600 / 800: the activation line bounds the warm servers");
    }

    #[test]
    fn the_activation_line_is_90_percent_and_leaves_room_for_the_measured_spread() {
        let cfg = cfg();
        assert_eq!(ACTIVATION_PERMILLE, 900);
        assert_eq!(cfg.activation_permille, 900);
        for (threads, most) in [(1u32, 1usize), (2, 2), (4, 4), (8, 9), (16, 18), (32, 36)] {
            let mut m = machine("m", threads);
            while can_place(&m, PlaceKind::Warm, None, &cfg) {
                m.sessions.push(warm(&format!("w{}", m.sessions.len()), "l", 0));
            }
            assert_eq!(m.sessions.len(), most, "{threads} threads: floor({threads} x 900 / 800)");
            // Every one of them occupied at the measured upper bound, 0.85 thread.
            assert!(m.sessions.len() as u64 * 850 <= u64::from(threads) * 1000, "{threads} threads at 0.85");
        }
    }

    /// The Worker's port (`infrastructure/cloudflare/api/src/fleet_policy.mjs`)
    /// must place by the same numbers. `tests/fleet_policy.test.mjs` reads this
    /// file the same way, so either side changing alone fails a test.
    #[test]
    fn the_worker_port_holds_the_same_numbers() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../infrastructure/cloudflare/api/src/fleet_policy.mjs");
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let js = |name: &str| -> u64 {
            let prefix = format!("export const {name} = ");
            let line = text
                .lines()
                .find(|l| l.starts_with(&prefix))
                .unwrap_or_else(|| panic!("{name} is not exported by fleet_policy.mjs"));
            line[prefix.len()..].trim_end_matches(';').replace('_', "").trim().parse().unwrap()
        };
        assert_eq!(js("ACTIVATION_PERMILLE"), ACTIVATION_PERMILLE);
        assert_eq!(js("UTILISATION_PERMILLE"), UTILISATION_PERMILLE);
        assert_eq!(js("OCCUPIED_MILLIS"), OCCUPIED_MILLIS);
        assert_eq!(js("EMPTY_MILLIS"), EMPTY_MILLIS);
        assert_eq!(js("SESSION_MEMORY_MB"), SESSION_MEMORY_MB);
        assert_eq!(js("MEMORY_PERMILLE"), MEMORY_PERMILLE);
        assert_eq!(js("OCCUPANCY_PERMILLE"), OCCUPANCY_PERMILLE);
        assert_eq!(js("MIN_GPU_VRAM_MB"), MIN_GPU_VRAM_MB);
        assert_eq!(js("MAX_GPU_VRAM_MB"), MAX_GPU_VRAM_MB);
    }

    #[test]
    fn a_gpu_listing_starts_only_on_a_gpu_host_with_the_vram_free() {
        let cfg = cfg();
        let cpu = machine("a-cpu", 16);
        let small = gpu_machine("b-small", 16, 4_096);
        let big = gpu_machine("c-big", 16, 12_288);
        let demand = Demand { pending: vec![gpu_request("r1", "city", 10, 8_192)], recent: Vec::new() };
        let (actions, projected) = project(10, &state(vec![cpu.clone(), small.clone(), big.clone()]), &demand, &cfg);
        assert!(
            matches!(&actions[0], FleetAction::StartForPlayer { machine_id, gpu_vram_mb: 8_192, .. } if machine_id == "c-big"),
            "{actions:?}"
        );
        let placed = projected.machines.iter().find(|m| m.id == "c-big").unwrap();
        assert_eq!(placed.gpu_vram_used_mb(), 8_192, "the session holds its VRAM");

        // 12 GB with 8 GB held: a second 8 GB session does not fit, and is refused rather than sent to a CPU machine.
        let mut held = big.clone();
        held.sessions.push(Session { gpu_vram_mb: 8_192, ..occupied("s1", "other", 2, 2) });
        let demand = Demand { pending: vec![gpu_request("r2", "city", 20, 8_192)], recent: Vec::new() };
        let actions = plan(20, &state(vec![cpu.clone(), small.clone(), held]), &demand, &cfg);
        assert!(
            matches!(&actions[0], FleetAction::Refuse { reason: RefuseReason::NoGpuCapacity, .. }),
            "{actions:?}"
        );
        assert!(
            actions.iter().any(|a| matches!(a, FleetAction::RecommendAddMachine { gpu_vram_mb: 8_192, .. })),
            "the recommendation names the GPU: {actions:?}"
        );
        assert!(starts_and_stops(&actions).0.is_empty(), "nothing starts on a CPU machine: {actions:?}");
    }

    #[test]
    fn a_gpu_listing_never_takes_a_seat_on_a_cpu_machine() {
        let cfg = cfg();
        let mut cpu = machine("a-cpu", 16);
        cpu.sessions.push(occupied("cpu-room", "city", 1, 4));
        cpu.sessions.push(warm("cpu-warm", "city", 0));
        let gpu = gpu_machine("b-gpu", 16, 16_384);
        let demand = Demand { pending: vec![gpu_request("r1", "city", 10, 8_192)], recent: Vec::new() };
        let actions = plan(10, &state(vec![cpu.clone(), gpu.clone()]), &demand, &cfg);
        assert!(
            matches!(&actions[0], FleetAction::StartForPlayer { machine_id, .. } if machine_id == "b-gpu"),
            "{actions:?}"
        );
        // The same listing as CPU-only takes the seat with players first, as always.
        let demand = Demand { pending: vec![request("r1", "city", 10)], recent: Vec::new() };
        let actions = plan(10, &state(vec![cpu, gpu]), &demand, &cfg);
        assert!(matches!(&actions[0], FleetAction::Route { session_id, .. } if session_id == "cpu-room"), "{actions:?}");
    }

    #[test]
    fn none_and_optional_place_exactly_as_a_listing_without_the_setting() {
        let cfg = cfg();
        let mut a = gpu_machine("a", 8, 8_192);
        a.sessions.push(occupied("a1", "pong", 1, 2));
        a.sessions.push(warm("a2", "tag", 0));
        let mut b = machine("b", 4);
        b.sessions.push(occupied("b1", "tag", 2, 2));
        let plain = Demand {
            pending: vec![request("r1", "pong", 10), request("r2", "tag", 11), request("r3", "new", 12)],
            recent: vec![recent("tag", 5)],
        };
        let base = plan(20, &state(vec![a.clone(), b.clone()]), &plain, &cfg);
        assert!(!base.is_empty());
        for mode in [GpuMode::None, GpuMode::Optional] {
            let mut demand = plain.clone();
            for r in &mut demand.pending {
                r.gpu = mode;
                r.gpu_min_vram_mb = 4_096;
            }
            for r in &mut demand.recent {
                r.gpu = mode;
                r.gpu_min_vram_mb = 4_096;
            }
            assert_eq!(plan(20, &state(vec![a.clone(), b.clone()]), &demand, &cfg), base, "{mode:?}");
        }
    }

    #[test]
    fn a_gpu_listing_keeps_its_warm_server_on_a_gpu_host() {
        let cfg = cfg();
        let cpu = machine("a-cpu", 16);
        let gpu = gpu_machine("b-gpu", 4, 8_192);
        let demand = Demand {
            pending: Vec::new(),
            recent: vec![RecentRequest { gpu: GpuMode::Required, gpu_min_vram_mb: 6_144, ..recent("city", 990) }],
        };
        let actions = plan(1000, &state(vec![cpu.clone(), gpu]), &demand, &cfg);
        assert_eq!(starts_and_stops(&actions).0, vec!["city".to_string()], "{actions:?}");
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, FleetAction::StartWarm { machine_id, gpu_vram_mb: 6_144, .. } if machine_id == "b-gpu")),
            "{actions:?}"
        );
        // No GPU host: no warm server at all.
        assert!(starts_and_stops(&plan(1000, &state(vec![cpu]), &demand, &cfg)).0.is_empty());
    }

    #[test]
    fn capacity_agrees_with_placing_one_at_a_time() {
        let cfg = cfg();
        for threads in 1..=32u32 {
            for kind in [PlaceKind::Occupied, PlaceKind::Warm] {
                let mut m = machine("m", threads);
                m.sessions.push(occupied("seed", "l", 1, 2));
                let room = match kind {
                    PlaceKind::Occupied => capacity(&m, &cfg).occupied_room,
                    PlaceKind::Warm => capacity(&m, &cfg).warm_room,
                };
                let mut placed = 0u64;
                while can_place(&m, kind, None, &cfg) {
                    let s = match kind {
                        PlaceKind::Occupied => occupied(&format!("s{placed}"), "l", 1, 2),
                        PlaceKind::Warm => warm(&format!("s{placed}"), "l", 0),
                    };
                    m.sessions.push(s);
                    placed += 1;
                }
                assert_eq!(placed, room, "{threads} threads, {kind:?}");
            }
        }
        let mut draining = machine("d", 8);
        draining.draining = true;
        assert!(!can_place(&draining, PlaceKind::Warm, None, &cfg));
        assert_eq!(capacity(&draining, &cfg).occupied_room, 0);
    }

    #[test]
    fn packing_prefers_the_fuller_machine_that_still_fits() {
        let cfg = cfg();
        let mut a = machine("a", 8);
        for i in 0..3 {
            a.sessions.push(occupied(&format!("a{i}"), "l", 1, 2));
        }
        let mut b = machine("b", 8);
        b.sessions.push(occupied("b0", "l", 1, 2));
        let machines = vec![b.clone(), a.clone()];
        assert_eq!(choose_machine(&machines, PlaceKind::Occupied, None, &cfg), Some(1), "a is fuller");

        let mut full = a.clone();
        while can_place(&full, PlaceKind::Occupied, None, &cfg) {
            full.sessions.push(occupied(&format!("f{}", full.sessions.len()), "l", 1, 2));
        }
        let machines = vec![full, b.clone()];
        assert_eq!(choose_machine(&machines, PlaceKind::Occupied, None, &cfg), Some(1), "a full machine is skipped");

        let twins = vec![machine("z", 8), machine("y", 8)];
        assert_eq!(choose_machine(&twins, PlaceKind::Occupied, None, &cfg), Some(1), "a tie goes to the lower id");

        let mut west = machine("w", 8);
        west.region = "us-west-2".into();
        let machines = vec![a.clone(), west];
        assert_eq!(choose_machine(&machines, PlaceKind::Occupied, Some("us-west-2"), &cfg), Some(1));
        assert_eq!(choose_machine(&machines, PlaceKind::Occupied, Some("eu-west-1"), &cfg), None);

        let demand = Demand { pending: vec![request("r1", "new", 10)], recent: Vec::new() };
        let actions = plan(10, &state(vec![b, a]), &demand, &cfg);
        assert!(
            actions.iter().any(|x| matches!(x, FleetAction::StartForPlayer { machine_id, .. } if machine_id == "a")),
            "{actions:?}"
        );
    }

    #[test]
    fn a_click_takes_a_seat_before_anything_starts() {
        let cfg = cfg();
        let mut m = machine("m", 8);
        m.sessions.push(occupied("busy", "pong", 3, 4));
        m.sessions.push(occupied("quiet", "pong", 1, 4));
        m.sessions.push(occupied("full", "pong", 4, 4));
        m.sessions.push(warm("spare", "pong", 0));
        let demand = Demand { pending: vec![request("r1", "pong", 50)], recent: Vec::new() };
        let actions = plan(50, &state(vec![m.clone()]), &demand, &cfg);
        assert_eq!(
            actions[0],
            FleetAction::Route {
                request_id: "r1".into(),
                listing_id: "pong".into(),
                machine_id: "m".into(),
                session_id: "busy".into(),
                warm: false
            }
        );
        assert!(starts_and_stops(&actions).0.is_empty());

        // No seat in a session with players: the warm server, then a start.
        m.sessions.retain(|s| s.id == "full" || s.id == "spare");
        let demand = Demand { pending: vec![request("r1", "pong", 50), request("r2", "pong", 51)], recent: Vec::new() };
        let actions = plan(50, &state(vec![m]), &demand, &cfg);
        assert!(matches!(&actions[0], FleetAction::Route { session_id, warm: true, .. } if session_id == "spare"));
        assert!(matches!(&actions[1], FleetAction::Route { session_id, warm: false, .. } if session_id == "spare"));
    }

    #[test]
    fn a_full_fleet_refuses_and_recommends_a_machine_instead_of_slowing_anyone() {
        let cfg = cfg();
        let mut m = machine("m", 4);
        for i in 0..4 {
            m.sessions.push(occupied(&format!("s{i}"), "other", 2, 2));
        }
        let demand = Demand { pending: vec![request("r1", "pong", 10)], recent: Vec::new() };
        let actions = plan(10, &state(vec![m]), &demand, &cfg);
        assert!(actions.iter().any(|a| matches!(a, FleetAction::Refuse { reason: RefuseReason::NoCapacity, .. })));
        assert!(actions.iter().any(|a| matches!(a, FleetAction::RecommendAddMachine { .. })));
        assert!(starts_and_stops(&actions).0.is_empty());

        let empty = plan(10, &FleetState::default(), &demand, &cfg);
        assert!(empty.iter().any(|a| matches!(a, FleetAction::RecommendAddMachine { .. })));
    }

    #[test]
    fn planned_occupancy_stays_at_or_above_80_percent_once_a_handful_run() {
        let cfg = cfg();
        for count in 0..=60u32 {
            let mut machines: Vec<Machine> = (0..8).map(|i| machine(&format!("m{i}"), 16)).collect();
            for i in 0..count {
                let target = (i % 8) as usize;
                machines[target].sessions.push(occupied(&format!("o{i}"), &format!("busy{i}"), 2, 2));
            }
            // Forty Spaces with a recent click and no free seat: every one wants a warm server.
            let mut demand = Demand::default();
            for k in 0..40 {
                demand.recent.push(recent(&format!("cold{k}"), 990));
            }
            for i in 0..count {
                demand.recent.push(recent(&format!("busy{i}"), 990));
            }
            let (actions, projected) = project(1000, &state(machines), &demand, &cfg);
            let (occupied_now, running) = occupancy(&projected);
            assert_eq!(occupied_now, u64::from(count));
            let warm_count = running - occupied_now;
            if count >= 8 {
                assert!(occupied_now * 1000 >= 800 * running, "{count} occupied, {running} running: {actions:?}");
            } else {
                assert!(warm_count <= 2, "{count} occupied, {warm_count} warm");
            }
            assert!(warm_count >= 1, "a Space clicked moments ago keeps a warm server");
        }
    }

    #[test]
    fn empty_servers_beyond_the_pool_stop_once_held() {
        let cfg = cfg();
        let mut m = machine("m", 32);
        for i in 0..8 {
            m.sessions.push(occupied(&format!("o{i}"), &format!("busy{i}"), 2, 2));
        }
        for i in 0..10 {
            m.sessions.push(warm(&format!("w{i}"), &format!("idle{i}"), 0));
        }
        let (actions, projected) = project(5000, &state(vec![m.clone()]), &Demand::default(), &cfg);
        assert_eq!(starts_and_stops(&actions).1.len(), 10, "no recent click: none is kept");
        assert_eq!(occupancy(&projected), (8, 8));

        let mut demand = Demand::default();
        demand.recent.push(recent("idle3", 4990));
        let (actions, projected) = project(5000, &state(vec![m]), &demand, &cfg);
        assert_eq!(starts_and_stops(&actions).1.len(), 9);
        assert!(!starts_and_stops(&actions).1.contains(&"idle3".to_string()), "a recent click keeps its warm server");
        assert_eq!(occupancy(&projected), (8, 9));
    }

    #[test]
    fn hysteresis_holds_across_a_join_and_a_leave() {
        let cfg = cfg();
        let mut m = machine("m", 8);
        m.sessions.push(occupied("s1", "pong", 2, 2));
        m.sessions.push(warm("w1", "pong", 0));
        let mut demand = Demand { pending: Vec::new(), recent: vec![recent("pong", 5)] };

        let steady = plan(10, &state(vec![m.clone()]), &demand, &cfg);
        assert!(steady.is_empty(), "the warm server is the floor: {steady:?}");

        demand.pending.push(request("r1", "pong", 20));
        let joined = plan(20, &state(vec![m.clone()]), &demand, &cfg);
        assert_eq!(joined.len(), 1, "{joined:?}");
        assert!(matches!(&joined[0], FleetAction::Route { session_id, warm: true, .. } if session_id == "w1"));
        demand.pending.clear();
        demand.recent.push(RecentRequest { needed_new_session: true, ..recent("pong", 20) });

        // The player leaves at 30: w1 is empty again, the click is recent, nothing moves.
        let w1 = m.sessions.iter_mut().find(|s| s.id == "w1").unwrap();
        w1.empty_since = Some(30);
        for now in [40, 100, 400, 620] {
            let actions = plan(now, &state(vec![m.clone()]), &demand, &cfg);
            assert!(actions.is_empty(), "at {now}: {actions:?}");
        }
        // Clicks aged out and w1 empty past the hold: stopped.
        let late = plan(1000, &state(vec![m.clone()]), &demand, &cfg);
        assert!(matches!(late.as_slice(), [FleetAction::Stop { session_id, .. }] if session_id == "w1"), "{late:?}");

        // A leave long after the last click still waits out the hold.
        let w1 = m.sessions.iter_mut().find(|s| s.id == "w1").unwrap();
        w1.empty_since = Some(1000);
        assert!(plan(1010, &state(vec![m.clone()]), &demand, &cfg).is_empty(), "inside the hold");
        assert!(plan(1119, &state(vec![m.clone()]), &demand, &cfg).is_empty(), "inside the hold");
        assert_eq!(starts_and_stops(&plan(1120, &state(vec![m]), &demand, &cfg)).1, vec!["pong".to_string()]);
    }

    #[test]
    fn no_plan_starts_and_stops_the_same_listing() {
        let cfg = cfg();
        for now in (0..2000).step_by(37) {
            let mut m = machine("m", 16);
            m.sessions.push(occupied("s1", "pong", 2, 2));
            m.sessions.push(warm("w1", "pong", 0));
            m.sessions.push(warm("w2", "pong", 0));
            m.sessions.push(warm("w3", "tag", 100));
            let demand = Demand {
                pending: vec![request(&format!("r{now}"), "tag", now)],
                recent: vec![recent("pong", now.saturating_sub(300)), recent("tag", now.saturating_sub(50))],
            };
            let (starts, stops) = starts_and_stops(&plan(now, &state(vec![m]), &demand, &cfg));
            for listing in &starts {
                assert!(!stops.contains(listing), "at {now}: {listing} started and stopped");
            }
        }
    }

    #[test]
    fn a_machine_with_no_players_is_recommended_for_release_outside_the_hysteresis() {
        let cfg = cfg();
        let mut a = machine("a", 8);
        a.sessions.push(occupied("o1", "pong", 2, 2));
        let idle = machine("b", 8);
        let mut fleet = state(vec![a, idle]);
        let actions = plan(10_000, &fleet, &Demand::default(), &cfg);
        assert!(
            actions.iter().any(|x| matches!(x, FleetAction::RecommendReleaseMachine { machine_id, .. } if machine_id == "b")),
            "{actions:?}"
        );
        fleet.last_machine_change_at = Some(9_900);
        let held = plan(10_000, &fleet, &Demand::default(), &cfg);
        assert!(!held.iter().any(|x| matches!(x, FleetAction::RecommendReleaseMachine { .. })), "{held:?}");
        let single = state(vec![machine("only", 8)]);
        assert!(plan(10_000, &single, &Demand::default(), &cfg).is_empty(), "the last machine is never released");
    }

    #[test]
    fn the_plan_is_deterministic_whatever_order_the_machines_arrive_in() {
        let cfg = cfg();
        let mut a = machine("a", 8);
        a.sessions.push(occupied("a1", "pong", 1, 2));
        a.sessions.push(warm("a2", "tag", 0));
        let mut b = machine("b", 4);
        b.sessions.push(occupied("b1", "tag", 2, 2));
        let demand = Demand {
            pending: vec![request("r2", "tag", 11), request("r1", "pong", 10), request("r3", "new", 12)],
            recent: vec![recent("tag", 5)],
        };
        let one = plan(20, &state(vec![a.clone(), b.clone()]), &demand, &cfg);
        let two = plan(20, &state(vec![b, a]), &demand, &cfg);
        assert_eq!(one, two);
        assert!(!one.is_empty());
    }

    #[test]
    fn the_warm_pool_formula() {
        let cfg = cfg();
        assert_eq!(warm_pool_size(0, 5, 9, &cfg), 2, "floor term while nothing runs");
        assert_eq!(warm_pool_size(4, 1, 9, &cfg), 1, "floor of one Space");
        assert_eq!(warm_pool_size(8, 9, 9, &cfg), 2, "8 occupied: cap 2");
        assert_eq!(warm_pool_size(40, 0, 30, &cfg), 10, "40 occupied: cap 10");
        assert_eq!(warm_pool_size(40, 0, 3, &cfg), 3, "never more than wanted");
        assert_eq!(warm_pool_size(40, 0, 0, &cfg), 0);
        for occupied in 8..500u64 {
            let w = warm_pool_size(occupied, 1000, 1000, &cfg);
            assert!(occupied * 1000 >= 800 * (occupied + w), "{occupied} occupied, {w} warm");
        }
    }
}
