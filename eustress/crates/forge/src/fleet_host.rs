//! Host agent: `eustress-forge host`, one per rented machine.
//!
//! Each second it reads the status file of every server it started and folds
//! in whether the process is still alive. A status phase of `exited-empty` or
//! `host-failed`, or a process that has exited, frees the slot. A start goes
//! through [`local_launch`] and [`local_launch::spawn`] with
//! `EUSTRESS_SERVER_HZ`, `EUSTRESS_SERVER_IDLE_HZ` and
//! `EUSTRESS_SERVER_EMPTY_EXIT_SECS` set; a stop writes `dedicated.stop`
//! beside the server's status file. The agent never kills a server; one that
//! outlives the agent leaves by itself after `EUSTRESS_SERVER_EMPTY_EXIT_SECS`
//! empty.
//!
//! ## Heartbeat mode
//!
//! With the machine's `ehost_` credential in `EUSTRESS_FLEET_HOST_TOKEN`, the
//! agent takes its orders from the control plane
//! (docs/architecture/HOSTED_FLEET.md, Route 1). Every `next_heartbeat_secs`
//! (5 by default) it POSTs a [`Heartbeat`] to
//! `<EUSTRESS_API_URL>/api/fleet/hosts/{machine_id}/heartbeat` with
//! `Authorization: Bearer ehost_...`, and applies the answer's commands:
//!
//! - `start`: checked with [`fleet::can_place`] against this machine. A start
//!   that would cross a line is listed in `rejected` with the line it would
//!   cross. Otherwise one server starts whose `EUSTRESS_SERVER_ID` is the
//!   command's `server_id` and whose `EUSTRESS_SERVER_TOKEN` is the command's
//!   `session_credential` (`eshs_`).
//! - `stop`: the server's stop file.
//!
//! Idempotency: a fresh `boot_id` (a UUID) per agent start, a `seq` that rises
//! by one on every heartbeat, and a [`Ledger`] of every `command_id` handled in
//! this boot. Each handled id is listed in `acked` (or `rejected`, with a
//! reason) until a `200` answer shows the control plane has it, and never
//! again; a command whose id was handled before is ignored. A freed slot is
//! listed in `freed` with `why` (`exited_empty`, `host_failed`, `stopped`,
//! `dead`) the same way.
//!
//! Failures ([`classify`], [`reply_step`]):
//!
//! - 401 or 403: nothing starts; running servers continue and leave by their
//!   empty exit. The heartbeat keeps trying on the backoff schedule.
//! - 400: logged with the field the control plane names. Nothing in the body is
//!   changed to make it pass; the next heartbeat goes at the usual interval.
//! - Unreachable, 5xx, 429, or any other answer: backoff of 5 s doubling to
//!   60 s ([`backoff_secs`]); nothing starts; the agent still writes the stop
//!   file of each of its own servers that has been empty past the hold
//!   ([`hold_stops`]).
//!
//! The `ehost_` credential is read from the environment only and goes into the
//! `Authorization` header and nowhere else. A started server never inherits it
//! ([`local_launch::HOST_ONLY_ENV`]). An `eshs_` goes only into its own
//! server's environment. Neither is in argv, a log line, an error, a status
//! file or any other file, and [`Secret`] prints neither under `Debug`. The API
//! URL must be `https://`, or `http://` to this computer.
//!
//! ## Inbox mode
//!
//! Without the credential the agent plans for its own machine alone, for local
//! testing: it describes the machine to [`crate::fleet::plan`] and applies the
//! actions meant for this machine. Join requests arrive as JSON files in an
//! inbox directory (`{"request_id","listing_id","max_players","region"}`), and
//! each answer is written to `<inbox>/answers/<request_id>.json`.
//!
//! Decisions ([`Hosted::observe`], [`Hosted::session`], [`machine`],
//! [`commands`], [`recent_from`], [`HostConfig::from_vars`], [`spawn_plan`],
//! [`heartbeat`], [`parse_answer`], [`Ledger`], [`classify`], [`reply_step`],
//! [`backoff_secs`], [`decide_start`], [`start_plan`], [`hold_stops`]) are pure
//! and tested with fake values; [`run`] and [`post_heartbeat`] hold the I/O.
//!
//! Environment (CONFIG DEFAULT in brackets): `EUSTRESS_FLEET_MACHINE_ID`
//! [hostname], `EUSTRESS_FLEET_REGION` [`EUSTRESS_HOST_REGION`, else `local`;
//! in heartbeat mode an unset region is sent as `null` and the control plane
//! uses the credential's region, and a region that is set must be the
//! credential's], `EUSTRESS_FLEET_THREADS` [detected], `EUSTRESS_FLEET_MEMORY_MB`
//! [/proc/meminfo; required elsewhere], `EUSTRESS_FLEET_GPU_VRAM_MB`
//! [the GPU's VRAM as [`crate::gpu_probe`] reads it; 0 offers no GPU],
//! `EUSTRESS_API_URL` [staging],
//! `EUSTRESS_FLEET_HOST_TOKEN` (environment only; heartbeat mode),
//! `EUSTRESS_HOST_PUBLIC`, `EUSTRESS_SERVER_TOKEN` (environment only; inbox
//! mode only, a heartbeat-mode server carries its own `eshs_`),
//! `EUSTRESS_SERVER_HZ` [60; under 60 is refused], `EUSTRESS_SERVER_EMPTY_EXIT_SECS`
//! [900, at least 60], `EUSTRESS_FLEET_INBOX` [~/.eustress_engine/fleet/inbox].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Child;

use serde::{Deserialize, Serialize};

use crate::fleet::{
    self, Capacity, Demand, FleetAction, FleetConfig, FleetState, JoinRequest, Machine, PlaceKind, RecentRequest,
    Session, SessionPhase,
};
use crate::gpu_probe::GpuReport;
use crate::local_launch::{self, LaunchRequest, SpawnPlan, STAGING_API};

/// The engine's phase while a session answers joins.
pub const PHASE_HOSTING: &str = "hosting";
/// `dedicated::EMPTY_EXIT_PHASE`: left after `EUSTRESS_SERVER_EMPTY_EXIT_SECS` empty.
pub const PHASE_EXITED_EMPTY: &str = "exited-empty";
/// `dedicated::HOST_FAILED_PHASE`: could not host; the process exits non-zero.
pub const PHASE_HOST_FAILED: &str = "host-failed";
/// CONFIG DEFAULT: a server left with nobody in it exits by itself after this.
/// Longer than the plan's hold, so the plan stops warm servers first.
pub const EMPTY_EXIT_SECS: u64 = 900;
/// The engine's floor for `EUSTRESS_SERVER_EMPTY_EXIT_SECS`.
pub const MIN_EMPTY_EXIT_SECS: u64 = 60;
/// CONFIG DEFAULT: a player sent to a session holds the seat this long.
pub const RESERVATION_SECS: u64 = 60;

/// The environment variable that holds this machine's `ehost_` credential.
pub const HOST_TOKEN_ENV: &str = "EUSTRESS_FLEET_HOST_TOKEN";
/// CONFIG DEFAULT: seconds between heartbeats when the answer names none.
pub const HEARTBEAT_SECS: u64 = 5;
/// The first wait after a failed heartbeat; each further failure doubles it.
pub const BACKOFF_FIRST_SECS: u64 = 5;
/// The longest wait between heartbeats, after failures or as asked.
pub const BACKOFF_MAX_SECS: u64 = 60;
/// The whole heartbeat request, connect to last byte.
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 10;
/// The control plane's limit on each list in a heartbeat (`MAX_HEARTBEAT_ITEMS`).
pub const MAX_BEAT_ITEMS: usize = 256;
/// The control plane's limit on a rejection's `why`.
pub const MAX_WHY_CHARS: usize = 200;
/// `agent_version` in every heartbeat.
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A credential. `Debug` prints `Secret(<redacted>)`; the value is reached
/// only through [`Secret::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// The fields of the engine's status file (`DedicatedStatus`) the agent reads.
/// Missing fields read as their defaults; unknown fields are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStatus {
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub listening: bool,
    #[serde(default)]
    pub join_link: String,
    #[serde(default)]
    pub players: u32,
    /// Seats taken: the players in the world and the ones still joining
    /// (the engine's `HostSession::seated_count`). 0 from an engine that
    /// does not write it.
    #[serde(default)]
    pub seated: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub sim_id: String,
    #[serde(default)]
    pub server_id: String,
}

impl ServerStatus {
    /// Seats taken, for placement: a player downloading the world has a
    /// seat long before the engine counts it in `players`.
    pub fn seats(&self) -> u32 {
        self.players.max(self.seated)
    }
}

/// Why a slot was freed. Serialized as the heartbeat's `freed[].why`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freed {
    ExitedEmpty,
    HostFailed,
    /// Exited after the agent wrote its stop file.
    Stopped,
    /// Exited with no stop asked and no exit phase written.
    Dead,
}

/// One server this agent started.
#[derive(Clone, Debug)]
pub struct Hosted {
    pub session_id: String,
    pub listing_id: String,
    pub port: u16,
    pub status_file: PathBuf,
    pub max_players: u32,
    pub started_at: u64,
    pub empty_since: Option<u64>,
    /// Expiry times of seats held for players on their way.
    pub reservations: Vec<u64>,
    pub stop_written_at: Option<u64>,
    pub status: Option<ServerStatus>,
}

impl Hosted {
    pub fn new(session_id: String, listing_id: String, port: u16, status_file: PathBuf, max_players: u32, now: u64) -> Self {
        Self {
            session_id,
            listing_id,
            port,
            status_file,
            max_players,
            started_at: now,
            empty_since: Some(now),
            reservations: Vec::new(),
            stop_written_at: None,
            status: None,
        }
    }

    /// Seats taken as the server last reported them ([`ServerStatus::seats`]).
    pub fn players(&self) -> u32 {
        self.status.as_ref().map_or(0, ServerStatus::seats)
    }

    pub fn reserve(&mut self, now: u64) {
        self.reservations.push(now + RESERVATION_SECS);
        self.empty_since = None;
    }

    /// Fold one status read (`None` when the file is missing or unreadable)
    /// and whether the process is alive. `Some` means the slot is free.
    pub fn observe(&mut self, status: Option<ServerStatus>, alive: bool, now: u64) -> Option<Freed> {
        self.reservations.retain(|&expiry| expiry > now);
        if let Some(status) = status {
            let joined = status.seats().saturating_sub(self.players()) as usize;
            let filled = joined.min(self.reservations.len());
            self.reservations.drain(..filled);
            if status.max_players > 0 {
                self.max_players = status.max_players;
            }
            self.status = Some(status);
        }
        if self.players() + self.reservations.len() as u32 > 0 {
            self.empty_since = None;
        } else if self.empty_since.is_none() {
            self.empty_since = Some(now);
        }
        match self.status.as_ref().map(|s| s.phase.as_str()) {
            Some(PHASE_EXITED_EMPTY) => return Some(Freed::ExitedEmpty),
            Some(PHASE_HOST_FAILED) => return Some(Freed::HostFailed),
            _ => {}
        }
        if !alive {
            return Some(if self.stop_written_at.is_some() { Freed::Stopped } else { Freed::Dead });
        }
        None
    }

    /// This server as the plan sees it.
    pub fn session(&self) -> Session {
        let hosting = self.status.as_ref().is_some_and(|s| s.phase == PHASE_HOSTING && s.listening);
        let phase = if self.stop_written_at.is_some() {
            SessionPhase::Stopping
        } else if hosting {
            SessionPhase::Hosting
        } else {
            SessionPhase::Starting
        };
        Session {
            id: self.session_id.clone(),
            listing_id: self.listing_id.clone(),
            phase,
            players: self.players(),
            reserved: self.reservations.len() as u32,
            max_players: self.max_players,
            started_at: self.started_at,
            empty_since: self.empty_since,
            // The control plane holds each session's VRAM from its own record
            // of the start; the host does not claim it.
            gpu_vram_mb: 0,
        }
    }

    pub fn stop_file(&self) -> PathBuf {
        stop_path(&self.status_file)
    }
}

/// The engine's stop file for a status file (`dedicated::stop_path`).
pub fn stop_path(status_file: &Path) -> PathBuf {
    status_file.with_file_name("dedicated.stop")
}

/// The directory a server this agent started downloads its listing into:
/// `world`, beside its status file (the engine's
/// `dedicated::server_download_dir`). Each server has its own, so a start,
/// which clears that directory first, never reaches a running server.
pub fn world_dir_of(status_file: &Path) -> PathBuf {
    status_file.with_file_name("world")
}

/// Remove a finished server's copy of its listing. Every server downloads
/// its own, so a machine that kept them would fill its disk one session at
/// a time. Only a directory named `world` is ever removed; the status file
/// and the log beside it stay.
fn remove_world(world: &Path) {
    if world.file_name().and_then(|name| name.to_str()) != Some("world") || !world.is_dir() {
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(world) {
        eprintln!("could not remove {}: {error}", world.display());
    }
}

/// The agent's settings. No `Debug`: it holds the server credential and the
/// host credential.
#[derive(Clone)]
pub struct HostConfig {
    pub machine_id: String,
    pub region: String,
    /// The region came from `EUSTRESS_FLEET_REGION` or `EUSTRESS_HOST_REGION`,
    /// not the `local` default.
    pub region_named: bool,
    pub threads: u32,
    pub memory_mb: u64,
    pub api: String,
    pub public_host: Option<String>,
    pub server_token: Option<String>,
    /// The machine's `ehost_` credential. `Some` runs the agent in heartbeat mode.
    pub host_token: Option<Secret>,
    pub empty_exit_secs: u64,
    pub inbox: PathBuf,
    pub fleet: FleetConfig,
    /// This machine's GPU as the heartbeat reports it: empty until
    /// [`HostConfig::with_gpu`] sets it from [`crate::gpu_probe`].
    pub gpu: GpuReport,
    /// `EUSTRESS_FLEET_GPU_VRAM_MB`: the VRAM to report in place of the
    /// probe's. 0 offers no GPU to the fleet.
    pub gpu_vram_override_mb: Option<u64>,
}

impl HostConfig {
    /// Read the settings through `var`. `threads`, `memory_mb` and `hostname`
    /// are what this computer reports, used when the variable is unset.
    pub fn from_vars(
        var: impl Fn(&str) -> Option<String>,
        threads: u32,
        memory_mb: Option<u64>,
        hostname: &str,
        home: &Path,
    ) -> Result<Self, String> {
        let get = |key: &str| var(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let mut fleet = FleetConfig::default();
        if let Some(raw) = get("EUSTRESS_SERVER_HZ") {
            let hz: u32 = raw.parse().map_err(|_| format!("EUSTRESS_SERVER_HZ={raw} is not a whole number"))?;
            fleet.occupied_hz = hz;
        }
        fleet.validate()?;
        let threads = match get("EUSTRESS_FLEET_THREADS") {
            Some(raw) => raw.parse().map_err(|_| format!("EUSTRESS_FLEET_THREADS={raw} is not a whole number"))?,
            None => threads,
        };
        if threads == 0 {
            return Err("this machine reports 0 threads".into());
        }
        let memory_mb = match get("EUSTRESS_FLEET_MEMORY_MB") {
            Some(raw) => raw.parse().map_err(|_| format!("EUSTRESS_FLEET_MEMORY_MB={raw} is not a whole number"))?,
            None => memory_mb.ok_or("set EUSTRESS_FLEET_MEMORY_MB: this computer's memory could not be read")?,
        };
        if memory_mb == 0 {
            return Err("EUSTRESS_FLEET_MEMORY_MB must be more than 0".into());
        }
        let empty_exit_secs = match get("EUSTRESS_SERVER_EMPTY_EXIT_SECS") {
            Some(raw) => raw
                .parse::<u64>()
                .map_err(|_| format!("EUSTRESS_SERVER_EMPTY_EXIT_SECS={raw} is not a whole number"))?
                .max(MIN_EMPTY_EXIT_SECS),
            None => EMPTY_EXIT_SECS,
        };
        let machine_id = get("EUSTRESS_FLEET_MACHINE_ID").unwrap_or_else(|| hostname.to_string());
        if !is_token(&machine_id) {
            return Err(format!("machine id {machine_id:?} must be 1 to 64 letters, digits, _ or -"));
        }
        let api = get("EUSTRESS_API_URL").unwrap_or_else(|| STAGING_API.to_string());
        let host_token = match get(HOST_TOKEN_ENV) {
            // The value is never part of an error.
            Some(value) if !is_host_credential(&value) => {
                return Err(format!("{HOST_TOKEN_ENV} is not a host credential (ehost_ and 64 lowercase hex digits)"));
            }
            Some(value) => {
                heartbeat_url(&api, &machine_id)?;
                Some(Secret::new(value))
            }
            None => None,
        };
        let gpu_vram_override_mb = match get("EUSTRESS_FLEET_GPU_VRAM_MB") {
            Some(raw) => Some(raw.parse().map_err(|_| format!("EUSTRESS_FLEET_GPU_VRAM_MB={raw} is not a whole number"))?),
            None => None,
        };
        let named_region = get("EUSTRESS_FLEET_REGION").or_else(|| get("EUSTRESS_HOST_REGION"));
        Ok(Self {
            machine_id,
            region_named: named_region.is_some(),
            region: named_region.unwrap_or_else(|| "local".into()),
            threads,
            memory_mb,
            api,
            public_host: get("EUSTRESS_HOST_PUBLIC"),
            server_token: get("EUSTRESS_SERVER_TOKEN"),
            host_token,
            empty_exit_secs,
            inbox: get("EUSTRESS_FLEET_INBOX")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".eustress_engine").join("fleet").join("inbox")),
            fleet,
            gpu: GpuReport::default(),
            gpu_vram_override_mb,
        })
    }

    /// The GPU the heartbeat reports: what the probe found, with
    /// `EUSTRESS_FLEET_GPU_VRAM_MB` in place of its VRAM when set. An
    /// override of 0 reports no GPU, so no listing that requires one is
    /// placed here.
    pub fn with_gpu(mut self, probed: GpuReport) -> Self {
        self.gpu = probed;
        if let Some(vram_mb) = self.gpu_vram_override_mb {
            self.gpu.vram_mb = vram_mb;
            self.gpu.available = vram_mb > 0;
        }
        self
    }
}

/// 1 to 64 of `A-Z a-z 0-9 _ -`. Listing, request and server ids become path parts.
pub fn is_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_lower_hex(value: &str) -> bool {
    value.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// `ehost_` and 64 lowercase hex digits, the control plane's grammar.
pub fn is_host_credential(value: &str) -> bool {
    value.strip_prefix("ehost_").is_some_and(|hex| hex.len() == 64 && is_lower_hex(hex))
}

/// `eshs_` and 1 to 256 ASCII letters or digits: safe to hand to a child's environment.
pub fn is_session_credential(value: &str) -> bool {
    value
        .strip_prefix("eshs_")
        .is_some_and(|rest| !rest.is_empty() && rest.len() <= 256 && rest.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// `cmd_` and 1 to 60 ASCII letters or digits, the control plane's grammar for
/// `acked` and `rejected`. A command with any other id is never listed.
pub fn is_command_id(value: &str) -> bool {
    value
        .strip_prefix("cmd_")
        .is_some_and(|rest| !rest.is_empty() && rest.len() <= 60 && rest.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Whether `rest` (what follows `http://`) names this computer.
fn is_loopback_authority(rest: &str) -> bool {
    let authority = rest.split(|c: char| c == '/' || c == '?' || c == '#').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Route 1's address for this machine. The host credential travels only over
/// `https://`, or `http://` to this computer.
pub fn heartbeat_url(api: &str, machine_id: &str) -> Result<String, String> {
    let base = api.trim().trim_end_matches('/');
    let secure = base.starts_with("https://") || base.strip_prefix("http://").is_some_and(is_loopback_authority);
    if !secure {
        return Err(format!(
            "EUSTRESS_API_URL {base} must be https:// (http:// only to this computer) to carry the host credential"
        ));
    }
    if !is_token(machine_id) {
        return Err(format!("machine id {machine_id:?} must be 1 to 64 letters, digits, _ or -"));
    }
    Ok(format!("{base}/api/fleet/hosts/{machine_id}/heartbeat"))
}

/// The frame-rate and empty-exit environment every server gets.
pub fn server_env(cfg: &HostConfig) -> Vec<(String, String)> {
    vec![
        ("EUSTRESS_SERVER_HZ".into(), cfg.fleet.occupied_hz.to_string()),
        ("EUSTRESS_SERVER_IDLE_HZ".into(), cfg.fleet.idle_hz.to_string()),
        ("EUSTRESS_SERVER_EMPTY_EXIT_SECS".into(), cfg.empty_exit_secs.to_string()),
        // A warm server waits with its simulation paused: it costs its idle
        // frames and nothing else until a player arrives.
        ("EUSTRESS_SERVER_IDLE_PAUSE".into(), "1".into()),
    ]
}

fn launch_request(
    cfg: &HostConfig,
    listing_id: &str,
    max_players: u32,
    ports_in_use: Vec<u16>,
    token: Option<String>,
) -> LaunchRequest {
    LaunchRequest {
        deployment_id: format!("fleet-{}", cfg.machine_id),
        experience_id: listing_id.to_string(),
        version: "latest".into(),
        region_variant: String::new(),
        region_name: cfg.region.clone(),
        count: 1,
        max_players,
        public_host: cfg.public_host.clone(),
        api: cfg.api.clone(),
        author_token: token,
        ports_in_use,
    }
}

fn with_server_env(cfg: &HostConfig, mut plan: SpawnPlan) -> SpawnPlan {
    for (key, value) in server_env(cfg) {
        plan.env.retain(|(k, _)| k != &key);
        plan.env.push((key, value));
    }
    plan
}

/// One server for `listing_id`, through `local_launch::plan`, with
/// [`server_env`] added. `None` for a listing id that is not a token.
pub fn spawn_plan(cfg: &HostConfig, listing_id: &str, max_players: u32, ports_in_use: Vec<u16>) -> Option<SpawnPlan> {
    if !is_token(listing_id) {
        return None;
    }
    let request = launch_request(cfg, listing_id, max_players, ports_in_use, cfg.server_token.clone());
    let plan = local_launch::plan(&request).into_iter().next()?;
    Some(with_server_env(cfg, plan))
}

/// The `--status-file` argument of a plan.
pub fn status_file_of(plan: &SpawnPlan) -> Option<PathBuf> {
    plan.args.windows(2).find(|w| w[0] == "--status-file").map(|w| PathBuf::from(&w[1]))
}

/// This machine for the plan.
pub fn machine<'a>(cfg: &HostConfig, hosted: impl Iterator<Item = &'a Hosted>) -> Machine {
    Machine {
        id: cfg.machine_id.clone(),
        threads: cfg.threads,
        memory_mb: cfg.memory_mb,
        region: cfg.region.clone(),
        draining: false,
        gpu_available: cfg.gpu.available,
        gpu_vram_mb: cfg.gpu.vram_mb,
        sessions: hosted.map(Hosted::session).collect(),
    }
}

/// What the edge loop does, from the plan's actions for this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCommand {
    /// Start a server. `planned` is the plan's placeholder id; `reserve_for`
    /// is the request whose player the seat is held for.
    Spawn { planned: String, listing_id: String, max_players: u32, reserve_for: Option<String> },
    Reserve { session_id: String },
    Stop { session_id: String },
    Answer { request_id: String, action: FleetAction },
    Note(String),
}

pub fn commands(machine_id: &str, actions: &[FleetAction]) -> Vec<HostCommand> {
    let mut out = Vec::new();
    for action in actions {
        match action {
            FleetAction::Route { request_id, machine_id: m, session_id, .. }
            | FleetAction::Wait { request_id, machine_id: m, session_id, .. } => {
                if m == machine_id {
                    out.push(HostCommand::Reserve { session_id: session_id.clone() });
                    out.push(HostCommand::Answer { request_id: request_id.clone(), action: action.clone() });
                }
            }
            FleetAction::StartForPlayer { request_id, listing_id, machine_id: m, session_id, max_players, .. } => {
                if m == machine_id {
                    out.push(HostCommand::Spawn {
                        planned: session_id.clone(),
                        listing_id: listing_id.clone(),
                        max_players: *max_players,
                        reserve_for: Some(request_id.clone()),
                    });
                    out.push(HostCommand::Answer { request_id: request_id.clone(), action: action.clone() });
                }
            }
            FleetAction::StartWarm { listing_id, machine_id: m, session_id, max_players, .. } => {
                if m == machine_id {
                    out.push(HostCommand::Spawn {
                        planned: session_id.clone(),
                        listing_id: listing_id.clone(),
                        max_players: *max_players,
                        reserve_for: None,
                    });
                }
            }
            FleetAction::Stop { machine_id: m, session_id, .. } => {
                if m == machine_id {
                    out.push(HostCommand::Stop { session_id: session_id.clone() });
                }
            }
            FleetAction::Refuse { request_id, .. } => {
                out.push(HostCommand::Answer { request_id: request_id.clone(), action: action.clone() });
            }
            FleetAction::RecommendAddMachine { region, reason, .. } => {
                out.push(HostCommand::Note(format!("recommend adding a machine in {region:?}: {reason}")));
            }
            FleetAction::RecommendReleaseMachine { machine_id: m, reason } => {
                out.push(HostCommand::Note(format!("recommend releasing {m}: {reason}")));
            }
        }
    }
    out
}

/// The pending requests as recent ones, marked by whether this plan needed
/// a new session for each (anything but a seat in a session with players).
pub fn recent_from(pending: &[JoinRequest], actions: &[FleetAction]) -> Vec<RecentRequest> {
    pending
        .iter()
        .filter_map(|request| {
            let action = actions.iter().find(|a| match a {
                FleetAction::Route { request_id, .. }
                | FleetAction::Wait { request_id, .. }
                | FleetAction::StartForPlayer { request_id, .. }
                | FleetAction::Refuse { request_id, .. } => request_id == &request.request_id,
                _ => false,
            })?;
            if matches!(action, FleetAction::Refuse { reason: fleet::RefuseReason::InvalidRequest, .. }) {
                return None;
            }
            Some(RecentRequest {
                listing_id: request.listing_id.clone(),
                at: request.at,
                max_players: request.max_players,
                region: request.region.clone(),
                needed_new_session: !matches!(action, FleetAction::Route { warm: false, .. }),
                gpu: request.gpu,
                gpu_min_vram_mb: request.gpu_min_vram_mb,
            })
        })
        .collect()
}

// -----------------------------------------------------------------------------
// Heartbeat mode: the request
// -----------------------------------------------------------------------------

/// Route 1's request body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Heartbeat {
    pub boot_id: String,
    pub seq: u64,
    pub machine: BeatMachine,
    pub capacity: Capacity,
    pub sessions: Vec<BeatSession>,
    pub freed: Vec<FreedEntry>,
    pub acked: Vec<String>,
    pub rejected: Vec<Rejected>,
    pub agent_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BeatMachine {
    pub threads: u32,
    pub memory_mb: u64,
    /// `None` (sent as `null`) when no region was named: the control plane
    /// uses the region the credential was minted for.
    pub region: Option<String>,
    pub draining: bool,
    /// The GPU adapter's name ([`crate::gpu_probe`]); empty when none was found.
    pub gpu: String,
    /// Its VRAM in MB; 0 where the platform does not expose it.
    pub gpu_vram_mb: u64,
    /// A hardware GPU: not a software or basic display adapter.
    pub gpu_available: bool,
}

/// A [`fleet::Session`] and the server's UDP port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BeatSession {
    #[serde(flatten)]
    pub session: Session,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FreedEntry {
    pub id: String,
    pub why: Freed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rejected {
    pub command_id: String,
    pub why: String,
}

/// One boot of the agent: its `boot_id` and the last `seq` sent.
#[derive(Clone, Debug)]
pub struct Boot {
    pub boot_id: String,
    seq: u64,
}

impl Boot {
    /// A fresh `boot_id` (a UUID: 36 of `0-9 a-f -`) and no heartbeat sent.
    pub fn start() -> Self {
        Self { boot_id: uuid::Uuid::new_v4().to_string(), seq: 0 }
    }

    /// The `seq` of the next heartbeat: one more than the last, every time.
    pub fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }
}

/// The command ids handled in this boot, and what the next heartbeat lists.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    handled: BTreeSet<String>,
    acked: Vec<String>,
    rejected: Vec<Rejected>,
    freed: Vec<FreedEntry>,
}

impl Ledger {
    /// True the first time `command_id` is seen in this boot. A command seen
    /// before is not executed again.
    pub fn first_time(&mut self, command_id: &str) -> bool {
        self.handled.insert(command_id.to_string())
    }

    pub fn ack(&mut self, command_id: &str) {
        self.acked.push(command_id.to_string());
    }

    pub fn reject(&mut self, command_id: &str, why: &str) {
        self.rejected.push(Rejected { command_id: command_id.to_string(), why: why.chars().take(MAX_WHY_CHARS).collect() });
    }

    pub fn freed(&mut self, session_id: &str, why: Freed) {
        self.freed.push(FreedEntry { id: session_id.to_string(), why });
    }

    /// A `200` answer to `sent`: what it listed reached the control plane and
    /// is never listed again. Anything added since stays.
    pub fn delivered(&mut self, sent: &Heartbeat) {
        self.acked.retain(|id| !sent.acked.contains(id));
        self.rejected.retain(|r| !sent.rejected.iter().any(|s| s.command_id == r.command_id));
        self.freed.retain(|f| !sent.freed.contains(f));
    }
}

/// The heartbeat for this machine now: its servers as `fleet::Session`
/// values with their ports, `fleet::capacity`, and up to [`MAX_BEAT_ITEMS`]
/// of each list the ledger holds (the rest go in the next one).
pub fn heartbeat(cfg: &HostConfig, boot_id: &str, seq: u64, servers: &[&Hosted], ledger: &Ledger) -> Heartbeat {
    let here = machine(cfg, servers.iter().copied());
    Heartbeat {
        boot_id: boot_id.to_string(),
        seq,
        machine: BeatMachine {
            threads: cfg.threads,
            memory_mb: cfg.memory_mb,
            region: cfg.region_named.then(|| cfg.region.clone()),
            draining: false,
            gpu: cfg.gpu.name.clone(),
            gpu_vram_mb: cfg.gpu.vram_mb,
            gpu_available: cfg.gpu.available,
        },
        capacity: fleet::capacity(&here, &cfg.fleet),
        sessions: servers
            .iter()
            .map(|h| BeatSession { session: h.session(), port: Some(h.port).filter(|&p| p > 0) })
            .collect(),
        freed: ledger.freed.iter().take(MAX_BEAT_ITEMS).cloned().collect(),
        acked: ledger.acked.iter().take(MAX_BEAT_ITEMS).cloned().collect(),
        rejected: ledger.rejected.iter().take(MAX_BEAT_ITEMS).cloned().collect(),
        agent_version: AGENT_VERSION.to_string(),
    }
}

// -----------------------------------------------------------------------------
// Heartbeat mode: the answer
// -----------------------------------------------------------------------------

/// A `start` command. `Debug` redacts the credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartCommand {
    pub command_id: String,
    pub listing_id: String,
    pub max_players: u32,
    pub server_id: String,
    pub for_request: Option<String>,
    pub session_credential: Secret,
}

/// One command of an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Start(StartCommand),
    Stop { command_id: String, server_id: String },
    /// A command with a usable id the agent cannot carry out; it is rejected.
    Unreadable { command_id: String },
}

impl Command {
    pub fn command_id(&self) -> &str {
        match self {
            Command::Start(start) => &start.command_id,
            Command::Stop { command_id, .. } | Command::Unreadable { command_id } => command_id,
        }
    }
}

/// A `200` answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub seq: Option<u64>,
    pub commands: Vec<Command>,
    pub next_heartbeat_secs: Option<u64>,
}

/// Read an answer body. `None` when it is not a JSON object or `commands` is
/// not a list. A command without an id the control plane accepts in `acked`
/// is dropped, since it could be neither acked nor rejected.
pub fn parse_answer(text: &str) -> Option<Answer> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    let commands = match object.get("commands") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(list)) => list.iter().filter_map(command_from).collect(),
        Some(_) => return None,
    };
    Some(Answer {
        seq: object.get("seq").and_then(|v| v.as_u64()),
        commands,
        next_heartbeat_secs: object.get("next_heartbeat_secs").and_then(|v| v.as_u64()),
    })
}

fn command_from(value: &serde_json::Value) -> Option<Command> {
    let command_id = value.get("command_id")?.as_str()?;
    if !is_command_id(command_id) {
        return None;
    }
    let text = |key: &str| value.get(key).and_then(|v| v.as_str());
    let unreadable = || Command::Unreadable { command_id: command_id.to_string() };
    let command = match text("do") {
        Some("stop") => match text("server_id").filter(|s| is_token(s)) {
            Some(server_id) => Command::Stop { command_id: command_id.to_string(), server_id: server_id.to_string() },
            None => unreadable(),
        },
        Some("start") => {
            let listing_id = text("listing_id").filter(|s| is_token(s));
            let server_id = text("server_id").filter(|s| is_token(s));
            let max_players = value
                .get("max_players")
                .and_then(|v| v.as_u64())
                .filter(|&n| n >= 1)
                .and_then(|n| u32::try_from(n).ok());
            let for_request = match value.get("for_request") {
                None | Some(serde_json::Value::Null) => Some(None),
                Some(serde_json::Value::String(s)) => Some(Some(s.clone())),
                Some(_) => None,
            };
            let credential = text("session_credential").filter(|s| is_session_credential(s));
            match (listing_id, server_id, max_players, for_request, credential) {
                (Some(listing_id), Some(server_id), Some(max_players), Some(for_request), Some(credential)) => {
                    Command::Start(StartCommand {
                        command_id: command_id.to_string(),
                        listing_id: listing_id.to_string(),
                        max_players,
                        server_id: server_id.to_string(),
                        for_request,
                        session_credential: Secret::new(credential.to_string()),
                    })
                }
                _ => unreadable(),
            }
        }
        _ => unreadable(),
    };
    Some(command)
}

/// An HTTP outcome, as the failure table sorts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// `200`.
    Answered,
    /// `401` (`bad_host_credential`) or `403` (`wrong_machine`).
    CredentialRefused,
    /// `400`: a field out of range.
    Invalid,
    /// No answer, 5xx, 429, or anything else.
    Unreachable,
}

pub fn classify(status: Option<u16>) -> Class {
    match status {
        Some(200) => Class::Answered,
        Some(401) | Some(403) => Class::CredentialRefused,
        Some(400) => Class::Invalid,
        _ => Class::Unreachable,
    }
}

/// The wait after `failures` heartbeats in a row failed: 5 s, doubling, at most 60 s.
pub fn backoff_secs(failures: u32) -> u64 {
    let doublings = failures.saturating_sub(1).min(16);
    (BACKOFF_FIRST_SECS << doublings).min(BACKOFF_MAX_SECS)
}

/// What the agent does with one heartbeat's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A `200` for the heartbeat just sent: carry out these commands.
    Apply(Vec<Command>),
    /// `401` or `403`: start nothing.
    CredentialRefused(u16),
    /// `400`, with the field the control plane named.
    Invalid(String),
    /// Back off, start nothing, stop own servers empty past the hold.
    Unreachable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub action: Action,
    /// Heartbeats failed in a row, this one included.
    pub failures: u32,
    /// Seconds until the next heartbeat.
    pub wait_secs: u64,
}

/// The outcome of the heartbeat numbered `sent_seq`, after `failures` failed
/// ones in a row. `status` is `None` when no answer came.
pub fn reply_step(status: Option<u16>, body: &str, sent_seq: u64, failures: u32) -> Step {
    let backoff = |action: Action| {
        let failures = failures.saturating_add(1);
        Step { action, failures, wait_secs: backoff_secs(failures) }
    };
    match classify(status) {
        Class::Answered => match parse_answer(body) {
            Some(answer) if answer.seq.map_or(true, |seq| seq == sent_seq) => Step {
                action: Action::Apply(answer.commands),
                failures: 0,
                wait_secs: answer.next_heartbeat_secs.unwrap_or(HEARTBEAT_SECS).clamp(1, BACKOFF_MAX_SECS),
            },
            Some(_) => backoff(Action::Unreachable("answered for another heartbeat".into())),
            None => backoff(Action::Unreachable("answered 200 without a heartbeat answer".into())),
        },
        Class::CredentialRefused => backoff(Action::CredentialRefused(status.unwrap_or(0))),
        Class::Invalid => Step { action: Action::Invalid(field_of(body)), failures: 0, wait_secs: HEARTBEAT_SECS },
        Class::Unreachable => backoff(Action::Unreachable(match status {
            Some(code) => format!("answered {code}"),
            None => "had no answer".into(),
        })),
    }
}

/// The `field` of a `400` body, printable characters only, at most 100.
fn field_of(body: &str) -> String {
    let field = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("field").and_then(|f| f.as_str()).map(str::to_string))
        .unwrap_or_default();
    let clean: String = field.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(100).collect();
    if clean.is_empty() {
        "an unnamed field".into()
    } else {
        clean
    }
}

// -----------------------------------------------------------------------------
// Heartbeat mode: carrying out commands
// -----------------------------------------------------------------------------

/// A start for a waiting player places an occupied session; any other a warm one.
pub fn start_kind(start: &StartCommand) -> PlaceKind {
    if start.for_request.is_some() {
        PlaceKind::Occupied
    } else {
        PlaceKind::Warm
    }
}

/// Which line one more session of `kind` would cross on `here`, for `rejected`.
pub fn line_crossed(here: &Machine, kind: PlaceKind, cfg: &FleetConfig) -> &'static str {
    let cost = match kind {
        PlaceKind::Occupied => cfg.occupied_millis,
        PlaceKind::Warm => cfg.empty_millis,
    };
    if here.draining {
        "this machine is draining"
    } else if here.committed_millis(cfg) + cost > here.committed_cap_millis(cfg) {
        "would pass the committed-CPU line"
    } else if here.activation_millis(cfg) + cfg.occupied_millis > here.activation_cap_millis(cfg) {
        "would pass the activation line"
    } else if here.memory_used_mb(cfg) + cfg.session_memory_mb > here.memory_cap_mb(cfg) {
        "would pass the memory line"
    } else {
        "would cross a line on this machine"
    }
}

/// One server for a `start` command: the control plane's `server_id` as its
/// `EUSTRESS_SERVER_ID` (and status directory), the command's `eshs_` as its
/// `EUSTRESS_SERVER_TOKEN`, and [`server_env`]. The agent's own
/// `EUSTRESS_SERVER_TOKEN` is not used. `None` for a malformed command.
pub fn start_plan(cfg: &HostConfig, start: &StartCommand, ports_in_use: Vec<u16>) -> Option<SpawnPlan> {
    let credential = start.session_credential.expose();
    if !is_token(&start.listing_id)
        || !is_token(&start.server_id)
        || start.max_players == 0
        || !is_session_credential(credential)
    {
        return None;
    }
    let request = launch_request(cfg, &start.listing_id, start.max_players, ports_in_use, Some(credential.to_string()));
    Some(with_server_env(cfg, local_launch::plan_named(&request, &start.server_id)))
}

/// What a `start` command comes to on this machine.
#[derive(Debug)]
pub enum StartDecision {
    Spawn { plan: SpawnPlan, status_file: PathBuf },
    /// A server with this id already runs here: nothing to start, acked.
    AlreadyRunning,
    Reject(String),
}

/// Decide a `start` against `here` (this machine with every server it runs).
/// [`fleet::can_place`] has the last word, so a control-plane bug cannot
/// overload the machine.
pub fn decide_start(cfg: &HostConfig, start: &StartCommand, here: &Machine, ports_in_use: Vec<u16>) -> StartDecision {
    if here.sessions.iter().any(|s| s.id == start.server_id) {
        return StartDecision::AlreadyRunning;
    }
    let kind = start_kind(start);
    if !fleet::can_place(here, kind, None, &cfg.fleet) {
        return StartDecision::Reject(line_crossed(here, kind, &cfg.fleet).to_string());
    }
    let Some(plan) = start_plan(cfg, start, ports_in_use) else {
        return StartDecision::Reject("malformed start command".into());
    };
    match status_file_of(&plan) {
        Some(status_file) => StartDecision::Spawn { plan, status_file },
        None => StartDecision::Reject("no status file in the plan".into()),
    }
}

/// The servers to stop while the control plane cannot be reached: hosting,
/// nobody in them or on the way, empty and running for at least `hold_secs`.
pub fn hold_stops<'a>(servers: impl Iterator<Item = &'a Hosted>, now: u64, hold_secs: u64) -> Vec<String> {
    servers
        .filter(|h| {
            let s = h.session();
            s.phase == SessionPhase::Hosting
                && !s.occupied()
                && s.empty_for(now) >= hold_secs
                && now.saturating_sub(s.started_at) >= hold_secs
        })
        .map(|h| h.session_id.clone())
        .collect()
}

// -----------------------------------------------------------------------------
// I/O
// -----------------------------------------------------------------------------

/// An inbox file.
#[derive(Clone, Debug, Deserialize)]
struct InboxRequest {
    request_id: String,
    listing_id: String,
    #[serde(default)]
    max_players: u32,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    gpu: fleet::GpuMode,
    #[serde(default)]
    gpu_min_vram_mb: u64,
}

/// The answer written for one request.
#[derive(Clone, Debug, Serialize)]
struct InboxAnswer<'a> {
    action: &'a FleetAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    join_link: Option<String>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_status(path: &Path) -> Option<ServerStatus> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Requests in the inbox, each file removed once read. Ids that are not
/// tokens are dropped (they would become paths).
fn read_inbox(inbox: &Path, now: u64) -> Vec<JoinRequest> {
    let Ok(entries) = std::fs::read_dir(inbox) else { return Vec::new() };
    let mut requests = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") || !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path).ok();
        let _ = std::fs::remove_file(&path);
        let Some(parsed) = text.and_then(|t| serde_json::from_str::<InboxRequest>(&t).ok()) else { continue };
        if !is_token(&parsed.request_id) || !is_token(&parsed.listing_id) {
            continue;
        }
        requests.push(JoinRequest {
            request_id: parsed.request_id,
            listing_id: parsed.listing_id,
            max_players: parsed.max_players,
            region: parsed.region,
            at: now,
            gpu: parsed.gpu,
            gpu_min_vram_mb: parsed.gpu_min_vram_mb,
        });
    }
    requests
}

fn memory_mb_from_proc() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Read every server's status and liveness; drop the freed ones (a freed
/// process still alive is kept in `exiting` until it is reaped). Returns the
/// freed sessions.
fn observe_all(hosted: &mut Vec<(Hosted, Child)>, exiting: &mut Vec<(Child, PathBuf)>, now: u64) -> Vec<(String, Freed)> {
    let mut freed_now = Vec::new();
    let mut keep = Vec::with_capacity(hosted.len());
    for (mut server, mut child) in hosted.drain(..) {
        let alive = matches!(child.try_wait(), Ok(None));
        let status = read_status(&server.status_file);
        match server.observe(status, alive, now) {
            Some(freed) => {
                eprintln!("session {} ({}) freed: {freed:?}", server.session_id, server.listing_id);
                freed_now.push((server.session_id.clone(), freed));
                // Its copy of the listing goes once the process has: at
                // once, or when the exit is reaped below.
                let world = world_dir_of(&server.status_file);
                if alive {
                    exiting.push((child, world));
                } else {
                    remove_world(&world);
                }
            }
            None => keep.push((server, child)),
        }
    }
    *hosted = keep;
    // Reap exits so no process is left a zombie.
    exiting.retain_mut(|(child, world)| {
        let alive = matches!(child.try_wait(), Ok(None));
        if !alive {
            remove_world(world);
        }
        alive
    });
    freed_now
}

/// Write the server's stop file. True when it was written.
fn write_stop(server: &mut Hosted, now: u64) -> bool {
    let stop = server.stop_file();
    if let Some(dir) = stop.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(&stop, b"stop\n") {
        Ok(()) => {
            server.stop_written_at = Some(now);
            true
        }
        Err(error) => {
            eprintln!("could not write {}: {error}", stop.display());
            false
        }
    }
}

/// `eustress-forge host`: read the environment, then [`run`]. The exit code.
pub fn main_from_env() -> i32 {
    let threads = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1);
    let hostname = hostname::get().ok().and_then(|h| h.into_string().ok()).unwrap_or_else(|| "host".into());
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let cfg = match HostConfig::from_vars(|k| std::env::var(k).ok(), threads, memory_mb_from_proc(), &hostname, &home) {
        // An OS read, not a GPU device: the agent never creates one.
        Ok(cfg) => cfg.with_gpu(crate::gpu_probe::cached().clone()),
        Err(error) => {
            eprintln!("eustress-forge host: {error}");
            return 1;
        }
    };
    run(cfg)
}

/// The loop: heartbeat mode with a host credential, inbox mode without.
/// Returns only when it cannot start.
pub fn run(cfg: HostConfig) -> i32 {
    match cfg.host_token.clone() {
        Some(token) => run_heartbeat(&cfg, &token),
        None => run_inbox(cfg),
    }
}

/// POST one heartbeat. `Ok((status, body))` for any HTTP answer, `Err` with
/// the transport error when none came. The credential goes in the
/// `Authorization` header only.
pub fn post_heartbeat(url: &str, token: &Secret, body: &str) -> Result<(u16, String), String> {
    let authorization = format!("Bearer {}", token.expose());
    let sent = ureq::post(url)
        .set("Authorization", &authorization)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(HEARTBEAT_TIMEOUT_SECS))
        .send_string(body);
    match sent {
        Ok(response) => {
            let status = response.status();
            Ok((status, response.into_string().unwrap_or_default()))
        }
        Err(ureq::Error::Status(status, response)) => Ok((status, response.into_string().unwrap_or_default())),
        Err(ureq::Error::Transport(transport)) => Err(transport.to_string()),
    }
}

fn run_heartbeat(cfg: &HostConfig, token: &Secret) -> i32 {
    let url = match heartbeat_url(&cfg.api, &cfg.machine_id) {
        Ok(url) => url,
        Err(error) => {
            eprintln!("eustress-forge host: {error}");
            return 1;
        }
    };
    let mut boot = Boot::start();
    eprintln!(
        "eustress-forge host {}: {} threads, {} MB, region {}, {} Hz occupied, {} Hz idle, heartbeat {url}, boot {}",
        cfg.machine_id,
        cfg.threads,
        cfg.memory_mb,
        if cfg.region_named { cfg.region.as_str() } else { "(the credential's)" },
        cfg.fleet.occupied_hz,
        cfg.fleet.idle_hz,
        boot.boot_id
    );
    if cfg.server_token.is_some() {
        eprintln!("EUSTRESS_SERVER_TOKEN is set and is not given to any server: each carries its own session credential");
    }
    let mut hosted: Vec<(Hosted, Child)> = Vec::new();
    let mut exiting: Vec<(Child, PathBuf)> = Vec::new();
    let mut ledger = Ledger::default();
    let mut failures: u32 = 0;
    let mut next_beat_at: u64 = 0;
    loop {
        let now = unix_now();
        for (session_id, why) in observe_all(&mut hosted, &mut exiting, now) {
            ledger.freed(&session_id, why);
        }
        if now >= next_beat_at {
            let seq = boot.next_seq();
            let sent = {
                let servers: Vec<&Hosted> = hosted.iter().map(|(h, _)| h).collect();
                heartbeat(cfg, &boot.boot_id, seq, &servers, &ledger)
            };
            let text = serde_json::to_string(&sent).unwrap_or_default();
            let step = match post_heartbeat(&url, token, &text) {
                Ok((status, answer)) => reply_step(Some(status), &answer, seq, failures),
                Err(transport) => {
                    eprintln!("heartbeat {seq}: {transport}");
                    reply_step(None, "", seq, failures)
                }
            };
            let was_failing = failures > 0;
            failures = step.failures;
            next_beat_at = unix_now().saturating_add(step.wait_secs);
            match step.action {
                Action::Apply(commands) => {
                    if was_failing {
                        eprintln!("heartbeat {seq}: answered again");
                    }
                    ledger.delivered(&sent);
                    apply_commands(cfg, &mut hosted, &mut ledger, commands, unix_now());
                }
                Action::CredentialRefused(code) => eprintln!(
                    "heartbeat {seq}: refused ({code}), the host credential is not valid for {}; starting nothing, running servers leave by their empty exit; next try in {} s",
                    cfg.machine_id, step.wait_secs
                ),
                Action::Invalid(field) => {
                    eprintln!("heartbeat {seq}: refused (400), {field} is missing or out of range; nothing is changed to make it pass")
                }
                Action::Unreachable(why) => {
                    eprintln!("heartbeat {seq}: the control plane {why}; starting nothing; next try in {} s", step.wait_secs);
                    let now = unix_now();
                    let due = hold_stops(hosted.iter().map(|(h, _)| h), now, cfg.fleet.empty_hold_secs);
                    for session_id in due {
                        if let Some((server, _)) = hosted.iter_mut().find(|(h, _)| h.session_id == session_id) {
                            if write_stop(server, now) {
                                eprintln!("stopped {session_id}: empty past the hold while the control plane is away");
                            }
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// Carry out an answer's commands, each id once per boot.
fn apply_commands(cfg: &HostConfig, hosted: &mut Vec<(Hosted, Child)>, ledger: &mut Ledger, commands: Vec<Command>, now: u64) {
    for command in commands {
        let id = command.command_id().to_string();
        if !ledger.first_time(&id) {
            continue;
        }
        match command {
            Command::Unreadable { .. } => {
                eprintln!("rejected {id}: unreadable command");
                ledger.reject(&id, "unreadable command");
            }
            Command::Stop { server_id, .. } => match hosted.iter_mut().find(|(h, _)| h.session_id == server_id) {
                Some((server, _)) if server.stop_written_at.is_none() => {
                    if write_stop(server, now) {
                        eprintln!("stopping {server_id}");
                        ledger.ack(&id);
                    } else {
                        ledger.reject(&id, "could not write the stop file");
                    }
                }
                // Already stopping, or not running here: nothing to do.
                _ => ledger.ack(&id),
            },
            Command::Start(start) => {
                let here = machine(cfg, hosted.iter().map(|(h, _)| h));
                let ports: Vec<u16> = hosted.iter().map(|(h, _)| h.port).collect();
                match decide_start(cfg, &start, &here, ports) {
                    StartDecision::AlreadyRunning => ledger.ack(&id),
                    StartDecision::Reject(why) => {
                        eprintln!("rejected start {id} for {}: {why}", start.listing_id);
                        ledger.reject(&id, &why);
                    }
                    StartDecision::Spawn { plan, status_file } => match local_launch::spawn(&plan) {
                        Ok(child) => {
                            let mut server = Hosted::new(
                                start.server_id.clone(),
                                start.listing_id.clone(),
                                plan.port,
                                status_file,
                                start.max_players,
                                now,
                            );
                            if start.for_request.is_some() {
                                server.reserve(now);
                            }
                            eprintln!("started {} for {} on UDP {}", start.server_id, start.listing_id, plan.port);
                            hosted.push((server, child));
                            ledger.ack(&id);
                        }
                        Err(error) => {
                            eprintln!("could not start a server for {}: {error}", start.listing_id);
                            ledger.reject(&id, "could not start the server process");
                        }
                    },
                }
            }
        }
    }
}

/// Inbox mode. Returns only when the inbox cannot be created.
fn run_inbox(cfg: HostConfig) -> i32 {
    let answers = cfg.inbox.join("answers");
    if let Err(error) = std::fs::create_dir_all(&answers) {
        eprintln!("eustress-forge host: inbox {}: {error}", answers.display());
        return 1;
    }
    eprintln!(
        "eustress-forge host {}: {} threads, {} MB, region {}, {} Hz occupied, {} Hz idle, inbox {}",
        cfg.machine_id,
        cfg.threads,
        cfg.memory_mb,
        cfg.region,
        cfg.fleet.occupied_hz,
        cfg.fleet.idle_hz,
        cfg.inbox.display()
    );
    let mut hosted: Vec<(Hosted, Child)> = Vec::new();
    let mut exiting: Vec<(Child, PathBuf)> = Vec::new();
    let mut recent: Vec<RecentRequest> = Vec::new();
    loop {
        let now = unix_now();

        observe_all(&mut hosted, &mut exiting, now);

        let pending = read_inbox(&cfg.inbox, now);
        recent.retain(|r| now.saturating_sub(r.at) <= cfg.fleet.demand_window_secs);
        let state = FleetState {
            machines: vec![machine(&cfg, hosted.iter().map(|(h, _)| h))],
            last_machine_change_at: None,
        };
        let demand = Demand { pending: pending.clone(), recent: recent.clone() };
        let actions = fleet::plan(now, &state, &demand, &cfg.fleet);
        recent.extend(recent_from(&pending, &actions));

        let mut planned: BTreeMap<String, String> = BTreeMap::new();
        for command in commands(&cfg.machine_id, &actions) {
            match command {
                HostCommand::Spawn { planned: placeholder, listing_id, max_players, reserve_for } => {
                    let ports: Vec<u16> = hosted.iter().map(|(h, _)| h.port).collect();
                    let Some(plan) = spawn_plan(&cfg, &listing_id, max_players, ports) else {
                        eprintln!("refused to start {listing_id:?}: not a listing id");
                        continue;
                    };
                    let Some(status_file) = status_file_of(&plan) else { continue };
                    match local_launch::spawn(&plan) {
                        Ok(child) => {
                            let mut server = Hosted::new(
                                plan.instance_id.clone(),
                                listing_id.clone(),
                                plan.port,
                                status_file,
                                max_players,
                                now,
                            );
                            if reserve_for.is_some() {
                                server.reserve(now);
                            }
                            eprintln!("started {} for {listing_id} on UDP {}", plan.instance_id, plan.port);
                            planned.insert(placeholder, plan.instance_id.clone());
                            hosted.push((server, child));
                        }
                        Err(error) => eprintln!("could not start a server for {listing_id}: {error}"),
                    }
                }
                HostCommand::Reserve { session_id } => {
                    let id = planned.get(&session_id).cloned().unwrap_or(session_id);
                    if let Some((server, _)) = hosted.iter_mut().find(|(h, _)| h.session_id == id) {
                        server.reserve(now);
                    }
                }
                HostCommand::Stop { session_id } => {
                    if let Some((server, _)) = hosted.iter_mut().find(|(h, _)| h.session_id == session_id) {
                        write_stop(server, now);
                    }
                }
                HostCommand::Answer { request_id, action } => {
                    let join_link = match &action {
                        FleetAction::Route { session_id, .. } => hosted
                            .iter()
                            .find(|(h, _)| &h.session_id == session_id)
                            .and_then(|(h, _)| h.status.as_ref())
                            .map(|s| s.join_link.clone())
                            .filter(|l| !l.is_empty()),
                        _ => None,
                    };
                    let answer = InboxAnswer { action: &action, join_link };
                    if let Ok(text) = serde_json::to_string_pretty(&answer) {
                        let _ = std::fs::write(answers.join(format!("{request_id}.json")), text);
                    }
                }
                HostCommand::Note(note) => eprintln!("{note}"),
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HostConfig {
        let vars = |k: &str| match k {
            "EUSTRESS_FLEET_MACHINE_ID" => Some("vps-1".to_string()),
            "EUSTRESS_FLEET_REGION" => Some("us-east-1".to_string()),
            _ => None,
        };
        HostConfig::from_vars(vars, 4, Some(8192), "ignored", Path::new("/home/fleet")).unwrap()
    }

    fn status(phase: &str, players: u32) -> ServerStatus {
        ServerStatus {
            phase: phase.into(),
            listening: phase == PHASE_HOSTING,
            players,
            max_players: 2,
            join_link: "eustress-player://join/203.0.113.7:7777?key=k&pin=p".into(),
            ..ServerStatus::default()
        }
    }

    fn hosted(id: &str, listing: &str) -> Hosted {
        Hosted::new(id.into(), listing.into(), 7777, PathBuf::from(format!("/tmp/{id}/status.json")), 2, 0)
    }

    /// Built at run time so no credential-shaped literal sits in the file.
    fn fake_host_token() -> String {
        format!("ehost_{}", "ab".repeat(32))
    }

    fn fake_session_token() -> String {
        format!("eshs_{}", "ef".repeat(32))
    }

    fn heartbeat_cfg() -> HostConfig {
        let host = fake_host_token();
        let server = format!("esrv_{}", "cd".repeat(32));
        let vars = move |k: &str| match k {
            "EUSTRESS_FLEET_MACHINE_ID" => Some("vps-1".to_string()),
            "EUSTRESS_FLEET_REGION" => Some("us-east-1".to_string()),
            "EUSTRESS_SERVER_TOKEN" => Some(server.clone()),
            HOST_TOKEN_ENV => Some(host.clone()),
            _ => None,
        };
        HostConfig::from_vars(vars, 4, Some(8192), "ignored", Path::new("/home/fleet")).unwrap()
    }

    fn start(command_id: &str, server_id: &str, for_request: Option<&str>) -> StartCommand {
        StartCommand {
            command_id: command_id.into(),
            listing_id: "07d0aa".into(),
            max_players: 2,
            server_id: server_id.into(),
            for_request: for_request.map(str::to_string),
            session_credential: Secret::new(fake_session_token()),
        }
    }

    #[test]
    fn exit_phases_and_a_dead_process_free_the_slot() {
        let mut h = hosted("a", "pong");
        assert_eq!(h.observe(Some(status(PHASE_HOSTING, 1)), true, 5), None);
        assert_eq!(h.observe(Some(status(PHASE_EXITED_EMPTY, 0)), true, 6), Some(Freed::ExitedEmpty));
        let mut h = hosted("b", "pong");
        assert_eq!(h.observe(Some(status(PHASE_HOST_FAILED, 0)), true, 6), Some(Freed::HostFailed));
        let mut h = hosted("c", "pong");
        assert_eq!(h.observe(Some(status(PHASE_HOSTING, 0)), false, 6), Some(Freed::Dead));
        let mut h = hosted("d", "pong");
        h.stop_written_at = Some(5);
        assert_eq!(h.observe(None, false, 6), Some(Freed::Stopped));
        let mut h = hosted("e", "pong");
        assert_eq!(h.observe(None, true, 1), None, "no status file yet: still starting");
        assert_eq!(h.session().phase, SessionPhase::Starting);
    }

    #[test]
    fn a_status_file_becomes_the_session_the_plan_reads() {
        let mut h = hosted("a", "pong");
        h.observe(Some(status("starting", 0)), true, 1);
        assert_eq!(h.session().phase, SessionPhase::Starting);
        h.observe(Some(status(PHASE_HOSTING, 2)), true, 2);
        let s = h.session();
        assert_eq!((s.phase, s.players, s.occupied(), s.empty_since), (SessionPhase::Hosting, 2, true, None));
        h.observe(Some(status(PHASE_HOSTING, 0)), true, 40);
        assert_eq!(h.session().empty_since, Some(40));
        h.stop_written_at = Some(41);
        assert_eq!(h.session().phase, SessionPhase::Stopping);
        // The engine's own JSON, extra fields and all, reads.
        let engine = r#"{"pid":7,"phase":"hosting","port":7777,"listening":true,"pin":"ab","join_link":"x","public_host":"","players":1,"max_players":2,"sim_id":"pong","server_scripts":true,"script_note":"","probe":"","error":"","server_id":"a","region":"us-east-1","gpu":"","cpu":"","hardware_tier":"low"}"#;
        let parsed: ServerStatus = serde_json::from_str(engine).unwrap();
        assert_eq!((parsed.players, parsed.max_players, parsed.listening), (1, 2, true));
    }

    /// A player who is still downloading the world has a seat. The engine
    /// counts it in `seated` at once and in `players` only when it enters
    /// the world, so placement reads the larger of the two.
    #[test]
    fn a_joining_player_holds_a_seat_before_it_enters_the_world() {
        let mut h = hosted("a", "pong");
        h.reserve(10);
        let joining = ServerStatus { seated: 1, ..status(PHASE_HOSTING, 0) };
        assert_eq!(h.observe(Some(joining), true, 12), None);
        let s = h.session();
        assert_eq!((s.players, s.reserved), (1, 0), "the joiner has the seat its reservation held");
        assert!(s.occupied());
        // The reservation's 60 s pass while the world still downloads: the seat stays taken.
        let joining = ServerStatus { seated: 1, ..status(PHASE_HOSTING, 0) };
        h.observe(Some(joining), true, 10 + RESERVATION_SECS + 5);
        let s = h.session();
        assert_eq!((s.players, s.reserved, s.empty_since), (1, 0, None), "a slow download gave the seat away");
        // In the world: counted once, not twice.
        let playing = ServerStatus { seated: 1, ..status(PHASE_HOSTING, 1) };
        h.observe(Some(playing), true, 200);
        assert_eq!(h.session().players, 1);
        // An engine that writes no `seated` reads as before.
        let old: ServerStatus = serde_json::from_str(r#"{"phase":"hosting","listening":true,"players":2,"max_players":2}"#).unwrap();
        assert_eq!((old.seated, old.seats()), (0, 2));
    }

    /// A server's copy of its listing sits beside its own status file, in a
    /// directory of its own: never the listing's shared directory, which
    /// holds every server's status and stop files.
    #[test]
    fn a_servers_world_directory_is_its_own() {
        let cfg = cfg();
        let a = status_file_of(&spawn_plan(&cfg, "pong", 2, vec![]).unwrap()).unwrap();
        let b = status_file_of(&spawn_plan(&cfg, "pong", 2, vec![7777]).unwrap()).unwrap();
        let (world_a, world_b) = (world_dir_of(&a), world_dir_of(&b));
        assert_ne!(world_a, world_b, "two servers of one listing share a world directory");
        assert_eq!(world_a.parent(), a.parent(), "beside the status file");
        assert_eq!(world_a.file_name().and_then(|n| n.to_str()), Some("world"));
        for kept in [&a, &b, &stop_path(&a), &stop_path(&b)] {
            assert!(!kept.starts_with(&world_a) && !kept.starts_with(&world_b), "{} is inside a world directory", kept.display());
        }
        // Removal touches a directory named `world` and nothing else.
        let dir = std::env::temp_dir().join(format!("eustress-fleet-host-test-{}", std::process::id()));
        let world = dir.join("world");
        std::fs::create_dir_all(world.join("Spaces")).unwrap();
        std::fs::write(dir.join("status.json"), b"{}").unwrap();
        remove_world(&dir);
        assert!(world.is_dir() && dir.join("status.json").is_file(), "a directory not named world was removed");
        remove_world(&world);
        assert!(!world.exists() && dir.join("status.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reservation_holds_the_seat_until_the_player_arrives_or_it_expires() {
        let mut h = hosted("a", "pong");
        h.observe(Some(status(PHASE_HOSTING, 0)), true, 10);
        h.reserve(10);
        assert!(h.session().occupied());
        h.observe(Some(status(PHASE_HOSTING, 1)), true, 12);
        assert_eq!((h.session().players, h.session().reserved), (1, 0), "the arrival fills the reservation");
        h.reserve(20);
        h.observe(Some(status(PHASE_HOSTING, 0)), true, 30);
        assert_eq!(h.session().reserved, 1);
        h.observe(Some(status(PHASE_HOSTING, 0)), true, 20 + RESERVATION_SECS);
        assert_eq!(h.session().reserved, 0);
        assert_eq!(h.session().empty_since, Some(20 + RESERVATION_SECS));
    }

    #[test]
    fn every_server_runs_at_60_hz_with_a_5_hz_idle_and_an_empty_exit() {
        let cfg = cfg();
        let env = server_env(&cfg);
        assert!(env.contains(&("EUSTRESS_SERVER_HZ".into(), "60".into())));
        assert!(env.contains(&("EUSTRESS_SERVER_IDLE_HZ".into(), "5".into())));
        assert!(env.contains(&("EUSTRESS_SERVER_EMPTY_EXIT_SECS".into(), "900".into())));
        let mut cfg = cfg;
        cfg.server_token = Some(format!("esrv_{}", "cd".repeat(32)));
        let plan = spawn_plan(&cfg, "pong", 2, vec![7777]).unwrap();
        assert_eq!(plan.port, 7778);
        assert!(plan.args.iter().any(|a| a == "--server"));
        for (k, v) in server_env(&cfg) {
            assert_eq!(plan.env.iter().filter(|(key, _)| key == &k).count(), 1);
            assert!(plan.env.contains(&(k, v)));
        }
        assert!(!local_launch::leaks_secret(&plan));
        let status = status_file_of(&plan).unwrap();
        assert!(status.parent().unwrap().ends_with(&plan.instance_id), "its own stop file");
        assert!(spawn_plan(&cfg, "../etc", 2, Vec::new()).is_none());
    }

    #[test]
    fn the_environment_cannot_lower_the_frame_rate() {
        let home = Path::new("/home/fleet");
        let with = |hz: &'static str| {
            move |k: &str| match k {
                "EUSTRESS_SERVER_HZ" => Some(hz.to_string()),
                _ => None,
            }
        };
        assert!(HostConfig::from_vars(with("30"), 4, Some(8192), "h", home).is_err());
        assert!(HostConfig::from_vars(with("59"), 4, Some(8192), "h", home).is_err());
        assert_eq!(HostConfig::from_vars(with("90"), 4, Some(8192), "h", home).unwrap().fleet.occupied_hz, 90);
        let plain = HostConfig::from_vars(|_| None, 4, Some(8192), "h", home).unwrap();
        assert_eq!(plain.fleet.occupied_hz, 60);
        assert_eq!(plain.machine_id, "h");
        assert!(HostConfig::from_vars(|_| None, 4, None, "h", home).is_err(), "memory must be known");
        let short = |k: &str| (k == "EUSTRESS_SERVER_EMPTY_EXIT_SECS").then(|| "5".to_string());
        assert_eq!(HostConfig::from_vars(short, 4, Some(8192), "h", home).unwrap().empty_exit_secs, 60);
    }

    #[test]
    fn the_loop_decision_from_fake_status_values() {
        let cfg = cfg();
        let mut busy = hosted("busy", "pong");
        busy.observe(Some(status(PHASE_HOSTING, 2)), true, 100);
        let mut spare = hosted("spare", "pong");
        spare.observe(Some(status(PHASE_HOSTING, 0)), true, 100);
        let servers = vec![busy, spare];
        let state = FleetState { machines: vec![machine(&cfg, servers.iter())], last_machine_change_at: None };
        let pending = vec![JoinRequest {
            request_id: "r1".into(),
            listing_id: "pong".into(),
            max_players: 2,
            region: None,
            at: 101,
            gpu: fleet::GpuMode::None,
            gpu_min_vram_mb: 0,
        }];
        let actions = fleet::plan(101, &state, &Demand { pending: pending.clone(), recent: Vec::new() }, &cfg.fleet);
        let cmds = commands(&cfg.machine_id, &actions);
        assert_eq!(cmds[0], HostCommand::Reserve { session_id: "spare".into() }, "{cmds:?}");
        assert!(recent_from(&pending, &actions)[0].needed_new_session);

        // The spare left empty: the slot is free, and the next click starts a server.
        let mut spare = servers[1].clone();
        assert_eq!(spare.observe(Some(status(PHASE_EXITED_EMPTY, 0)), true, 200), Some(Freed::ExitedEmpty));
        let state = FleetState { machines: vec![machine(&cfg, servers[..1].iter())], last_machine_change_at: None };
        let actions = fleet::plan(201, &state, &Demand { pending, recent: Vec::new() }, &cfg.fleet);
        let cmds = commands(&cfg.machine_id, &actions);
        assert!(
            matches!(&cmds[0], HostCommand::Spawn { listing_id, reserve_for: Some(r), .. } if listing_id == "pong" && r == "r1"),
            "{cmds:?}"
        );

        // Actions for another machine are not this agent's.
        let other = commands("vps-2", &actions);
        assert!(other.iter().all(|c| matches!(c, HostCommand::Answer { .. } | HostCommand::Note(_))), "{other:?}");
    }

    #[test]
    fn the_host_credential_turns_heartbeat_mode_on_and_never_appears_in_an_error() {
        let home = Path::new("/home/fleet");
        assert!(cfg().host_token.is_none(), "no credential: inbox mode");
        let cfg = heartbeat_cfg();
        assert_eq!(cfg.host_token.as_ref().map(|t| t.expose().to_string()), Some(fake_host_token()));
        assert!(cfg.region_named);

        let bad = format!("ehost_{}", "AB".repeat(32));
        let bad_value = bad.clone();
        let error = HostConfig::from_vars(
            move |k: &str| (k == HOST_TOKEN_ENV).then(|| bad.clone()),
            4,
            Some(8192),
            "h",
            home,
        )
        .err()
        .expect("upper-case hex is not the grammar");
        assert!(!error.contains(&bad_value), "{error}");
        assert!(error.contains(HOST_TOKEN_ENV));

        let token = fake_host_token();
        let plain_http = move |k: &str| match k {
            HOST_TOKEN_ENV => Some(token.clone()),
            "EUSTRESS_API_URL" => Some("http://api.example.com".to_string()),
            _ => None,
        };
        let error = HostConfig::from_vars(plain_http, 4, Some(8192), "h", home).err().expect("no plain http off this computer");
        assert!(!error.contains(&fake_host_token()), "{error}");

        let unnamed = HostConfig::from_vars(|_| None, 4, Some(8192), "h", home).unwrap();
        assert!(!unnamed.region_named);
        assert_eq!(unnamed.region, "local");
    }

    #[test]
    fn the_heartbeat_url_carries_the_credential_only_over_tls_or_to_this_computer() {
        assert_eq!(
            heartbeat_url("https://api-staging.eustress.dev/", "vps-1").unwrap(),
            "https://api-staging.eustress.dev/api/fleet/hosts/vps-1/heartbeat"
        );
        assert!(heartbeat_url("http://127.0.0.1:8787", "vps-1").is_ok());
        assert!(heartbeat_url("http://localhost:8787/", "vps-1").is_ok());
        assert!(heartbeat_url("http://[::1]:8787", "vps-1").is_ok());
        assert!(heartbeat_url("http://api.example.com", "vps-1").is_err());
        assert!(heartbeat_url("http://localhost.example.com", "vps-1").is_err());
        assert!(heartbeat_url("http://127.0.0.1@example.com", "vps-1").is_err());
        assert!(heartbeat_url("ftp://api.example.com", "vps-1").is_err());
        assert!(heartbeat_url("https://api.example.com", "../x").is_err());
    }

    #[test]
    fn the_heartbeat_body_is_route_1() {
        let cfg = cfg();
        let mut busy = hosted("8f0c", "07d0aa");
        busy.observe(Some(status(PHASE_HOSTING, 2)), true, 1000);
        let servers = vec![&busy];
        let mut ledger = Ledger::default();
        ledger.ack("cmd_01J");
        ledger.reject("cmd_01L", &"x".repeat(500));
        ledger.freed("11aa", Freed::ExitedEmpty);
        ledger.freed("11ab", Freed::HostFailed);
        let beat = heartbeat(&cfg, "boot-1", 1842, &servers, &ledger);
        let json = serde_json::to_value(&beat).unwrap();
        assert_eq!(json["boot_id"], "boot-1");
        assert_eq!(json["seq"], 1842);
        assert_eq!(
            json["machine"],
            serde_json::json!({
                "threads": 4, "memory_mb": 8192, "region": "us-east-1", "draining": false,
                "gpu": "", "gpu_vram_mb": 0, "gpu_available": false,
            })
        );
        assert_eq!(json["capacity"]["committed_cap_millis"], 3200);
        assert_eq!(json["capacity"]["committed_millis"], 800);
        assert_eq!(json["capacity"]["activation_cap_millis"], 3600, "90% of 4 threads");
        let session = &json["sessions"][0];
        assert_eq!(session["id"], "8f0c");
        assert_eq!(session["listing_id"], "07d0aa");
        assert_eq!(session["phase"], "hosting");
        assert_eq!(session["players"], 2);
        assert_eq!(session["reserved"], 0);
        assert_eq!(session["max_players"], 2);
        assert_eq!(session["started_at"], 0);
        assert!(session["empty_since"].is_null());
        assert_eq!(session["port"], 7777);
        assert_eq!(
            json["freed"],
            serde_json::json!([{ "id": "11aa", "why": "exited_empty" }, { "id": "11ab", "why": "host_failed" }])
        );
        assert_eq!(json["acked"], serde_json::json!(["cmd_01J"]));
        assert_eq!(json["rejected"][0]["command_id"], "cmd_01L");
        assert_eq!(json["rejected"][0]["why"].as_str().unwrap().len(), MAX_WHY_CHARS);
        assert_eq!(json["agent_version"], AGENT_VERSION);
        assert_eq!(serde_json::to_value(Freed::Stopped).unwrap(), "stopped");
        assert_eq!(serde_json::to_value(Freed::Dead).unwrap(), "dead");

        // No region named: null, and the control plane uses the credential's.
        let mut unnamed = cfg.clone();
        unnamed.region_named = false;
        assert!(serde_json::to_value(heartbeat(&unnamed, "b", 1, &[], &ledger)).unwrap()["machine"]["region"].is_null());
    }

    #[test]
    fn a_boot_has_a_fresh_id_and_a_seq_that_only_rises() {
        let mut one = Boot::start();
        let two = Boot::start();
        assert_ne!(one.boot_id, two.boot_id);
        assert!(is_token(&one.boot_id), "the control plane's id grammar: {}", one.boot_id);
        let seqs: Vec<u64> = (0..5).map(|_| one.next_seq()).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn each_command_id_runs_once_and_is_listed_until_delivered() {
        let cfg = cfg();
        let mut ledger = Ledger::default();
        assert!(ledger.first_time("cmd_A"));
        assert!(!ledger.first_time("cmd_A"), "a resent command is ignored");
        ledger.ack("cmd_A");
        assert!(ledger.first_time("cmd_B"));
        ledger.reject("cmd_B", "would pass the activation line");
        ledger.freed("s1", Freed::Dead);

        // The first heartbeat fails: everything is listed again in the next.
        let first = heartbeat(&cfg, "boot", 1, &[], &ledger);
        let second = heartbeat(&cfg, "boot", 2, &[], &ledger);
        assert_eq!((first.acked.clone(), first.freed.len(), first.rejected.len()), (vec!["cmd_A".to_string()], 1, 1));
        assert_eq!(second.acked, first.acked);

        // The second is answered: none of it is listed again. An ack made after
        // the heartbeat was built stays for the next one.
        ledger.ack("cmd_C");
        ledger.delivered(&second);
        let third = heartbeat(&cfg, "boot", 3, &[], &ledger);
        assert_eq!(third.acked, vec!["cmd_C".to_string()]);
        assert!(third.rejected.is_empty() && third.freed.is_empty());
        ledger.delivered(&third);
        assert!(heartbeat(&cfg, "boot", 4, &[], &ledger).acked.is_empty());
        assert!(!ledger.first_time("cmd_A"), "still remembered for the life of the boot");
    }

    #[test]
    fn the_answer_parses_starts_and_stops_and_drops_what_cannot_be_acked() {
        let token = fake_session_token();
        let body = serde_json::json!({
            "seq": 7,
            "commands": [
                { "command_id": "cmd_01M", "do": "start", "listing_id": "07d0aa", "max_players": 2,
                  "server_id": "8f0d", "for_request": "req_1", "session_credential": token },
                { "command_id": "cmd_01N", "do": "stop", "server_id": "11ab" },
                { "command_id": "cmd_01P", "do": "start", "listing_id": "07d0aa", "max_players": 2,
                  "server_id": "8f0e", "for_request": null, "session_credential": token },
                { "command_id": "cmd_01Q", "do": "start", "listing_id": "07d0aa", "max_players": 2, "server_id": "8f0f" },
                { "command_id": "cmd_01R", "do": "reboot" },
                { "command_id": "not-a-command-id", "do": "stop", "server_id": "11ab" },
                { "do": "stop", "server_id": "11ab" }
            ],
            "next_heartbeat_secs": 5
        })
        .to_string();
        let answer = parse_answer(&body).unwrap();
        assert_eq!(answer.seq, Some(7));
        assert_eq!(answer.next_heartbeat_secs, Some(5));
        assert_eq!(answer.commands.len(), 5, "{:?}", answer.commands);
        assert_eq!(answer.commands[0], Command::Start(start("cmd_01M", "8f0d", Some("req_1"))));
        assert_eq!(answer.commands[1], Command::Stop { command_id: "cmd_01N".into(), server_id: "11ab".into() });
        assert_eq!(answer.commands[2], Command::Start(start("cmd_01P", "8f0e", None)));
        assert_eq!(answer.commands[3], Command::Unreadable { command_id: "cmd_01Q".into() }, "no credential");
        assert_eq!(answer.commands[4], Command::Unreadable { command_id: "cmd_01R".into() });
        assert!(!format!("{answer:?}").contains(&token), "Debug redacts the session credential");
        assert!(parse_answer("[]").is_none());
        assert!(parse_answer("{\"commands\":{}}").is_none());
        assert!(parse_answer("<html>").is_none());
        assert_eq!(parse_answer("{\"seq\":1}").unwrap().commands, Vec::new());
    }

    #[test]
    fn http_outcomes_map_to_what_the_agent_does() {
        let ok = serde_json::json!({ "seq": 9, "commands": [], "next_heartbeat_secs": 7 }).to_string();
        assert_eq!(reply_step(Some(200), &ok, 9, 3), Step { action: Action::Apply(Vec::new()), failures: 0, wait_secs: 7 });
        let slow = serde_json::json!({ "seq": 9, "commands": [], "next_heartbeat_secs": 3600 }).to_string();
        assert_eq!(reply_step(Some(200), &slow, 9, 0).wait_secs, BACKOFF_MAX_SECS);
        assert!(matches!(reply_step(Some(200), &ok, 10, 0).action, Action::Unreachable(_)), "another heartbeat's answer");
        assert!(matches!(reply_step(Some(200), "oops", 9, 0).action, Action::Unreachable(_)));

        for code in [401, 403] {
            let step = reply_step(Some(code), "{\"code\":\"bad_host_credential\"}", 9, 0);
            assert_eq!(step, Step { action: Action::CredentialRefused(code), failures: 1, wait_secs: 5 });
        }
        let invalid = reply_step(Some(400), "{\"code\":\"bad_heartbeat\",\"field\":\"sessions[0].listing_id\"}", 9, 2);
        assert_eq!(invalid, Step { action: Action::Invalid("sessions[0].listing_id".into()), failures: 0, wait_secs: HEARTBEAT_SECS });
        for status in [None, Some(500), Some(502), Some(503), Some(429), Some(404)] {
            let step = reply_step(status, "", 9, 1);
            assert!(matches!(step.action, Action::Unreachable(_)), "{status:?}");
            assert_eq!((step.failures, step.wait_secs), (2, 10), "{status:?}");
        }
        assert_eq!(classify(Some(200)), Class::Answered);
        assert_eq!(classify(Some(201)), Class::Unreachable);
    }

    #[test]
    fn backoff_starts_at_5_seconds_and_doubles_to_60() {
        let waits: Vec<u64> = (1..=7).map(backoff_secs).collect();
        assert_eq!(waits, vec![5, 10, 20, 40, 60, 60, 60]);
        assert_eq!(backoff_secs(u32::MAX), 60);
    }

    #[test]
    fn a_start_runs_with_the_control_planes_server_id_and_its_own_session_credential() {
        let cfg = heartbeat_cfg();
        let command = start("cmd_01M", "8f0d0c1e", Some("req_1"));
        let session = fake_session_token();
        let plan = start_plan(&cfg, &command, vec![7777]).unwrap();
        let env = |key: &str| plan.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert_eq!(env("EUSTRESS_SERVER_ID").as_deref(), Some("8f0d0c1e"));
        assert_eq!(env("EUSTRESS_SERVER_TOKEN"), Some(session.clone()));
        assert_eq!(env("EUSTRESS_AUTHOR_TOKEN"), None);
        assert_eq!(plan.env.iter().filter(|(k, _)| k == "EUSTRESS_SERVER_TOKEN").count(), 1);
        assert_eq!(plan.instance_id, "8f0d0c1e");
        assert_eq!(plan.port, 7778);
        assert!(status_file_of(&plan).unwrap().parent().unwrap().ends_with("8f0d0c1e"));
        for (k, v) in server_env(&cfg) {
            assert!(plan.env.contains(&(k, v)));
        }

        // The session credential is in the child's environment only.
        assert!(!local_launch::leaks_secret(&plan));
        assert!(!plan.args.iter().any(|a| a.contains(&session)));
        assert!(!format!("{plan:?}").contains(&session));
        assert!(!format!("{command:?}").contains(&session));
        // Neither the agent's server credential nor the host credential reaches the child.
        let server = cfg.server_token.clone().unwrap();
        let host = fake_host_token();
        for (_, v) in &plan.env {
            assert!(!v.contains(&server) && !v.contains(&host));
        }
        assert!(!plan.args.iter().any(|a| a.contains(&host) || a.contains(&server)));
        assert!(!format!("{:?}", cfg.host_token).contains(&host));

        // A malformed start plans nothing.
        let mut bad = command.clone();
        bad.session_credential = Secret::new(format!("esrv_{}", "cd".repeat(32)));
        assert!(start_plan(&cfg, &bad, Vec::new()).is_none());
        let mut bad = command;
        bad.server_id = "../etc".into();
        assert!(start_plan(&cfg, &bad, Vec::new()).is_none());
    }

    #[test]
    fn the_heartbeat_reports_the_probed_gpu_and_an_override_replaces_its_vram() {
        let probed = GpuReport { name: "NVIDIA GeForce RTX 2080 Ti".into(), vram_mb: 11_264, available: true };
        let cfg = heartbeat_cfg().with_gpu(probed.clone());
        let json = serde_json::to_value(heartbeat(&cfg, "b", 1, &[], &Ledger::default())).unwrap();
        assert_eq!(json["machine"]["gpu"], "NVIDIA GeForce RTX 2080 Ti");
        assert_eq!(json["machine"]["gpu_vram_mb"], 11_264);
        assert_eq!(json["machine"]["gpu_available"], true);
        let here = machine(&cfg, std::iter::empty());
        assert!(here.gpu_available && here.gpu_vram_mb == 11_264, "the plan sees the same GPU");

        let home = Path::new("/home/fleet");
        let off = |k: &str| (k == "EUSTRESS_FLEET_GPU_VRAM_MB").then(|| "0".to_string());
        let cfg = HostConfig::from_vars(off, 4, Some(8192), "h", home).unwrap().with_gpu(probed.clone());
        assert!(!cfg.gpu.available, "an override of 0 offers no GPU");
        let set = |k: &str| (k == "EUSTRESS_FLEET_GPU_VRAM_MB").then(|| "24576".to_string());
        let cfg = HostConfig::from_vars(set, 4, Some(8192), "h", home).unwrap().with_gpu(GpuReport::default());
        assert!(cfg.gpu.available && cfg.gpu.vram_mb == 24_576);
        let bad = |k: &str| (k == "EUSTRESS_FLEET_GPU_VRAM_MB").then(|| "lots".to_string());
        assert!(HostConfig::from_vars(bad, 4, Some(8192), "h", home).is_err());
    }

    #[test]
    fn a_start_that_would_cross_a_line_on_this_machine_is_rejected() {
        let cfg = heartbeat_cfg();
        let mut full: Vec<Hosted> = (0..4).map(|i| hosted(&format!("o{i}"), "07d0aa")).collect();
        for h in &mut full {
            h.observe(Some(status(PHASE_HOSTING, 2)), true, 10);
        }
        let here = machine(&cfg, full.iter());
        match decide_start(&cfg, &start("cmd_1", "new1", Some("req_1")), &here, Vec::new()) {
            StartDecision::Reject(why) => assert_eq!(why, "would pass the committed-CPU line"),
            other => panic!("{other:?}"),
        }

        // Four warm servers: a fifth session would make 5 x 800 = 4000 > 3600, 90% of 4 threads.
        let mut warm: Vec<Hosted> = (0..4).map(|i| hosted(&format!("w{i}"), "07d0aa")).collect();
        for h in &mut warm {
            h.observe(Some(status(PHASE_HOSTING, 0)), true, 10);
        }
        let here = machine(&cfg, warm.iter());
        match decide_start(&cfg, &start("cmd_2", "new2", None), &here, Vec::new()) {
            StartDecision::Reject(why) => assert_eq!(why, "would pass the activation line"),
            other => panic!("{other:?}"),
        }

        let here = machine(&cfg, full[..1].iter());
        assert!(matches!(decide_start(&cfg, &start("cmd_3", "o0", None), &here, Vec::new()), StartDecision::AlreadyRunning));
        match decide_start(&cfg, &start("cmd_4", "new4", Some("req_4")), &here, vec![7777]) {
            StartDecision::Spawn { plan, status_file } => {
                assert_eq!(plan.instance_id, "new4");
                assert!(status_file.parent().unwrap().ends_with("new4"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(start_kind(&start("c", "s", Some("r"))), PlaceKind::Occupied);
        assert_eq!(start_kind(&start("c", "s", None)), PlaceKind::Warm);
    }

    #[test]
    fn without_the_control_plane_empty_servers_past_the_hold_still_stop() {
        let hold = 120;
        let mut warm = hosted("warm", "07d0aa");
        warm.observe(Some(status(PHASE_HOSTING, 0)), true, 0);
        let mut busy = hosted("busy", "07d0aa");
        busy.observe(Some(status(PHASE_HOSTING, 1)), true, 0);
        let mut stopping = hosted("stopping", "07d0aa");
        stopping.observe(Some(status(PHASE_HOSTING, 0)), true, 0);
        stopping.stop_written_at = Some(5);
        let mut loading = hosted("loading", "07d0aa");
        loading.observe(Some(status("starting", 0)), true, 0);
        let servers = [warm.clone(), busy, stopping, loading];
        assert!(hold_stops(servers.iter(), hold - 1, hold).is_empty(), "inside the hold");
        assert_eq!(hold_stops(servers.iter(), hold, hold), vec!["warm".to_string()]);

        // Empty for the hold, but started inside it: kept.
        let mut fresh = hosted("fresh", "07d0aa");
        fresh.started_at = 100;
        fresh.observe(Some(status(PHASE_HOSTING, 0)), true, 100);
        fresh.empty_since = Some(0);
        assert!(hold_stops([fresh].iter(), hold, hold).is_empty());
    }
}
