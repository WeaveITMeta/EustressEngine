//! # eustress — Headless CLI for Eustress Engine
//!
//! ## Table of Contents
//! - Cli / Commands       — CLAP top-level command tree
//! - cmd_open              — `eustress open`      — open a Space in a NEW engine window, return pid + port
//! - cmd_instances         — `eustress instances` — list running engines (prunes dead records)
//! - cmd_close             — `eustress close`     — graceful engine.shutdown by pid / space / --all
//! - cmd_bridge            — `eustress bridge`    — drive a live engine over TCP (--port/--pid to pick one)
//! - cmd_run               — `eustress run`       — one-shot: launch eustress-headless, wait, relay exit code
//! - cmd_server            — `eustress server`    — start headless dedicated server
//! - cmd_publish           — `eustress publish`   — publish Space to Cloudflare R2
//! - cmd_sim               — `eustress sim`       — simulation history (in-process ring-buffer replay)
//!
//! ## Drive surface (HEADLESS_RUNTIME.md §7)
//! `bridge` is a thin wrapper over `eustress-bridge-client` (the same TCP JSON-RPC
//! client the MCP server uses), so it drives EITHER a windowed `eustress-engine` or
//! a headless `eustress-headless` process identically. `run` launches
//! `eustress-headless` as a child and relays its exit code, for CI / batch use.
//!
//! ## Fan-out (many engines at once)
//! `open` / `instances` / `close` are the orchestration primitives: an agent calls
//! `open` once per Space (windowed or `--headless`), captures each instance's
//! `port` from the printed record, drives each with `bridge --port <N>`, and tears
//! them down with `close`. The per-Universe `engine.port` file is a single slot
//! naming the Universe's owner, so multi-instance work MUST address by
//! `--port`/`--pid`; the registry behind it is `<workspace>/.eustress/instances/`.
//! Simulation runs are per instance too: `bridge --pid <N> sim-run --duration 60
//! --wait` runs one engine's simulation and prints that run's final values, while
//! its siblings on the same Universe run their own.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use eustress_common::sim_record::{ArcEpisodeRecord, IterationRecord, RuneScriptRecord, SimRecord, WorkshopIterationRecord};
use eustress_common::sim_stream::{SimQuery, SimStreamConfig, SimStreamReader};


// ─────────────────────────────────────────────────────────────────────────────
// CLI definition
// ─────────────────────────────────────────────────────────────────────────────

/// Eustress Engine CLI — headless control and simulation history.
#[derive(Parser, Debug)]
#[command(name = "eustress")]
#[command(about = "Eustress Engine CLI — server control, publishing, and simulation history")]
#[command(version)]
#[command(propagate_version = true)]
struct Cli {
    /// Legacy URL flag — retained for script compatibility, not used.
    #[arg(long, global = true, env = "IGGY_URL", default_value = "", hide = true)]
    iggy_url: String,

    #[arg(long, short, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Open a Space in a NEW engine window (or headless process) and return its pid + port.
    Open(OpenArgs),

    /// List every running engine instance (editor windows + headless), pruning dead records.
    Instances(InstancesArgs),

    /// Shut down running engine instance(s) cleanly via engine.shutdown.
    Close(CloseArgs),

    /// Drive a LIVE engine (windowed or eustress-headless) over the TCP bridge.
    Bridge {
        /// Universe root (holds `.eustress/engine.port`). Defaults to the
        /// current directory. Ignored when --port or --pid targets an
        /// instance directly.
        #[arg(long)]
        universe: Option<PathBuf>,
        /// Target a specific instance by its bridge port (from `eustress
        /// instances` / `eustress open`). Bypasses port-file discovery —
        /// the only unambiguous way to reach ONE engine when several run.
        #[arg(long, conflicts_with = "pid")]
        port: Option<u16>,
        /// Target a specific instance by process id (looked up in the
        /// instance registry).
        #[arg(long)]
        pid: Option<u32>,
        /// Workspace root holding `.eustress/instances/` (for --pid lookup).
        /// Defaults to $EUSTRESS_WORKSPACE, else <Documents>/Eustress.
        #[arg(long)]
        workspace: Option<PathBuf>,
        #[command(subcommand)]
        action: BridgeCommands,
    },

    /// Run a Space's simulation headlessly (spawns eustress-headless, relays its exit code).
    Run(RunArgs),

    /// Manage headless dedicated server processes.
    Server {
        #[command(subcommand)]
        action: ServerCommands,
    },

    /// Publish a Space to Cloudflare R2 via Wrangler.
    Publish(PublishArgs),

    /// Simulation history: replay runs, best iteration, workshop convergence.
    Sim {
        #[command(subcommand)]
        action: SimCommands,
    },

    /// Fork management: register with the trust registry, validate queue.
    Fork {
        #[command(subcommand)]
        action: ForkCommands,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Subcommand args
// ─────────────────────────────────────────────────────────────────────────────

/// Subcommands for `eustress bridge` — one call = one JSON-RPC round-trip to
/// `<universe>/.eustress/engine.port` (windowed editor or eustress-headless,
/// whichever is live). Mirrors `eustress_engine::engine_bridge::protocol::MethodName`;
/// see `bridge_tools.rs` in the MCP server for the same surface as AI tool calls.
#[derive(Subcommand, Debug)]
enum BridgeCommands {
    /// Health check — verify a live engine is reachable.
    Ping,
    /// Raw JSON-RPC passthrough — call any bridge method by name. Use this
    /// for methods without a dedicated subcommand (tool.equip, selection.set,
    /// action.invoke, viewport.capture, ai_camera.*, scene.overview, ...).
    Call {
        /// Bridge method name, e.g. "ecs.inspect" or "tool.equip".
        method: String,
        /// JSON object of params, e.g. '{"limit": 10}'.
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// List entities (id/name/class), paginated. Cheap; use `inspect` for detail.
    EcsQuery {
        #[arg(long)]
        class: Option<String>,
        #[arg(long, default_value = "0")]
        offset: u64,
        #[arg(long, default_value = "100")]
        limit: u64,
    },
    /// Rich live scene inspection: mesh/material/transform/physics flags + FPS.
    Inspect {
        #[arg(long)]
        class: Option<String>,
        #[arg(long)]
        name_contains: Option<String>,
        #[arg(long, default_value = "50")]
        limit: u64,
    },
    /// Read live simulation watchpoint values (empty = all).
    SimRead {
        #[arg(long, value_delimiter = ',')]
        keys: Vec<String>,
    },
    /// Deterministically advance physics by N fixed (1/60s) ticks — the
    /// POMDP control primitive. Leaves physics paused between calls.
    SimStep {
        #[arg(long, default_value = "1")]
        ticks: u64,
    },
    /// Start a simulation run (from Edit), resume a paused one, or retune a
    /// running one. Prints the run id; `--wait` blocks until the run ends
    /// and prints its final values.
    SimRun {
        /// Time compression (1.0 = realtime). A new run defaults to 1.0; a
        /// running or paused one keeps its scale unless this is given.
        #[arg(long)]
        time_scale: Option<f64>,
        /// Auto-stop after this many simulated seconds.
        #[arg(long)]
        duration: Option<f64>,
        /// Wait for the run to end and print its outcome.
        #[arg(long)]
        wait: bool,
        /// With --wait: give up after this many wall-clock seconds.
        #[arg(long, default_value = "300")]
        timeout: f64,
    },
    /// Pause the current run.
    SimPause,
    /// Stop the current run (restores the scene, like the Stop button).
    SimStop,
    /// Write sim values: `sim-set battery.current=0 cell.temp_c=25`.
    SimSet {
        /// `key=value` pairs.
        #[arg(required = true, value_name = "KEY=VALUE")]
        values: Vec<String>,
    },
    /// Play state, sim clock, and the run ledger (current run, recent runs
    /// with their final values).
    SimState,
    /// Wait for a run to end and print its outcome (final values, end
    /// reason, recording path).
    SimAwait {
        /// The run to wait for. Default: the run in progress, else the most
        /// recent one.
        #[arg(long)]
        run_id: Option<u64>,
        /// Give up after this many wall-clock seconds.
        #[arg(long, default_value = "300")]
        timeout: f64,
    },
    /// Cast a ray against live Avian colliders — the POMDP "sense" primitive.
    Raycast {
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
        origin: Option<Vec<f32>>,
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
        direction: Option<Vec<f32>>,
        #[arg(long)]
        max_distance: Option<f32>,
        #[arg(long)]
        max_hits: Option<u64>,
    },
    /// Tail the causal op-log — recent entity mutations, oldest-first.
    Oplog {
        #[arg(long, default_value = "50")]
        limit: u64,
    },
    /// Binary-ECS entity CRUD (create/read/update/delete/find).
    Entity {
        #[command(subcommand)]
        action: EntityCommands,
    },
}

#[derive(Subcommand, Debug)]
enum EntityCommands {
    Create {
        #[arg(long, default_value = "Part")]
        class: String,
        #[arg(long, default_value = "block")]
        shape: String,
        #[arg(long, default_value = "Part")]
        name: String,
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
        position: Option<Vec<f32>>,
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
        size: Option<Vec<f32>>,
        #[arg(long, num_args = 3, value_names = ["R", "G", "B"])]
        color: Option<Vec<f32>>,
        #[arg(long)]
        material: Option<String>,
        #[arg(long)]
        anchored: Option<bool>,
        #[arg(long)]
        can_collide: Option<bool>,
    },
    Read {
        #[arg(long)]
        uuid: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
    Update {
        #[arg(long)]
        uuid: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
        position: Option<Vec<f32>>,
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
        size: Option<Vec<f32>>,
        #[arg(long, num_args = 3, value_names = ["R", "G", "B"])]
        color: Option<Vec<f32>>,
        #[arg(long)]
        material: Option<String>,
        #[arg(long)]
        anchored: Option<bool>,
        #[arg(long)]
        can_collide: Option<bool>,
    },
    Delete {
        #[arg(long)]
        uuid: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
    Find {
        #[arg(long)]
        uuid: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        class: Option<String>,
        #[arg(long, default_value = "50")]
        limit: u64,
    },
}

/// `eustress open <space>` — spawn a NEW engine process on a Space, detached,
/// and print its instance record (pid, port) once the bridge is up. This is
/// the primitive an orchestrator uses to fan out: call it N times for N
/// Spaces, capture each `port`, then drive each with `eustress bridge --port`.
/// Several instances on Spaces of the SAME Universe are fine — each is
/// registered by pid, not by the single-slot per-Universe port file.
#[derive(Args, Debug)]
struct OpenArgs {
    /// `.eustress` Space directory to open.
    space: PathBuf,
    /// Start straight into Play mode. Without it, the instance opens in Edit
    /// and waits for a run (`eustress bridge --pid <N> sim-run`, or the
    /// run_simulation tool), so per-instance values can be set first.
    #[arg(long)]
    play: bool,
    /// Open in a windowless eustress-headless process instead of an editor window.
    #[arg(long)]
    headless: bool,
    /// Seconds to wait for the new instance's bridge to come up before
    /// giving up on reporting its port (the process keeps running either way).
    #[arg(long, default_value = "45")]
    wait_secs: u64,
    /// Workspace root holding `.eustress/instances/`. Defaults to
    /// $EUSTRESS_WORKSPACE, else <Documents>/Eustress.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Print only the JSON record (no status line) — for scripts/agents.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct InstancesArgs {
    /// Workspace root holding `.eustress/instances/`. Defaults to
    /// $EUSTRESS_WORKSPACE, else <Documents>/Eustress.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Print only the JSON array — for scripts/agents.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct CloseArgs {
    /// Close the instance with this pid.
    #[arg(long, conflicts_with_all = ["space", "all"])]
    pid: Option<u32>,
    /// Close every instance that has this Space open.
    #[arg(long, conflicts_with = "all")]
    space: Option<PathBuf>,
    /// Close every running instance.
    #[arg(long)]
    all: bool,
    /// If the graceful engine.shutdown is refused or times out, kill the process.
    #[arg(long)]
    force: bool,
    /// Workspace root holding `.eustress/instances/`. Defaults to
    /// $EUSTRESS_WORKSPACE, else <Documents>/Eustress.
    #[arg(long)]
    workspace: Option<PathBuf>,
}

/// `eustress run <space>` — launch eustress-headless as a child process,
/// wait for it, relay its exit code. See HEADLESS_RUNTIME.md §8: with
/// `--ticks`, a Space becomes a pure function `(space, ticks) -> recording.json + exit code`.
#[derive(Args, Debug)]
struct RunArgs {
    /// `.eustress` Space directory to simulate.
    space: PathBuf,
    /// Stop after N sim ticks (60 Hz fixed), export the recording, exit.
    /// Omit to run until killed or stopped via the bridge.
    #[arg(long)]
    ticks: Option<u64>,
    /// Main-loop rate in Hz (sim fixed-step always stays 60 Hz).
    #[arg(long, default_value = "60")]
    tick_rate: f64,
    /// Boot into Edit state instead of auto-entering Play.
    #[arg(long)]
    no_autoplay: bool,
}

#[derive(Subcommand, Debug)]
enum ServerCommands {
    Start {
        #[arg(long, default_value = "7777")]
        port: u16,
        #[arg(long, default_value = "100")]
        max_players: u32,
        #[arg(long)]
        scene: Option<PathBuf>,
        #[arg(long, default_value = "120")]
        tick_rate: u32,
    },
    Watch,
}

#[derive(Args, Debug)]
struct PublishArgs {
    #[arg(default_value = ".")]
    space_path: PathBuf,
    #[arg(long, default_value = "production")]
    env: String,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand, Debug)]
enum SimCommands {
    Replay {
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    Best {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Convergence {
        #[arg(long)]
        product: Option<String>,
        #[arg(long, default_value = "50")]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    Scripts {
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    Arc {
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    ArcBest {
        #[arg(long)]
        task: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ForkCommands {
    /// Register this fork with the Online Trust Registry via Cloudflare Tunnel.
    ///
    /// Steps: 1) Generates fork keypair if needed  2) Creates cloudflared tunnel
    /// 3) Registers well-known endpoints  4) Submits to the OTR at eustress.dev
    Register {
        /// Fork ID (domain-style, e.g. "neovia.fork" or "studio.example.com")
        #[arg(long)]
        fork_id: String,
        /// Chain ID (must be unique, not 1=mainnet or 2=testnet)
        #[arg(long)]
        chain_id: u32,
        /// Contact email or URL
        #[arg(long)]
        contact: String,
        /// Local port your fork server listens on (default: 8080)
        #[arg(long, default_value = "8080")]
        port: u16,
        /// Path to existing fork keypair (will be generated if not provided)
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Skip tunnel setup (use if you already have a tunnel configured)
        #[arg(long)]
        skip_tunnel: bool,
    },

    /// Run the validation loop — checks the registration queue every 30 minutes.
    ///
    /// Fetches pending fork registrations, validates their well-known endpoints,
    /// verifies tunnel connectivity, and processes approvals/rejections.
    Loop {
        /// Registry API endpoint (default: https://eustress.dev)
        #[arg(long, default_value = "https://eustress.dev")]
        registry_url: String,
        /// Check interval in minutes (default: 30)
        #[arg(long, default_value = "30")]
        interval_minutes: u64,
        /// Run once and exit (don't loop)
        #[arg(long)]
        once: bool,
    },

    /// Show the status of this fork's registration.
    Status {
        /// Fork ID to check
        #[arg(long)]
        fork_id: String,
        /// Registry API endpoint
        #[arg(long, default_value = "https://eustress.dev")]
        registry_url: String,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("warn")
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    match cli.command {
        Commands::Open(args) => cmd_open(args),
        Commands::Instances(args) => cmd_instances(args),
        Commands::Close(args) => cmd_close(args),
        Commands::Bridge { universe, port, pid, workspace, action } => {
            cmd_bridge(universe, port, pid, workspace, action)
        }
        Commands::Run(args) => cmd_run(args).await,
        Commands::Server { action } => cmd_server(action).await,
        Commands::Publish(args) => cmd_publish(args).await,
        Commands::Sim { action } => cmd_sim(&cli.iggy_url, action).await,
        Commands::Fork { action } => cmd_fork(action).await,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_bridge — drive a live engine (windowed or eustress-headless) over TCP
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the Universe root a `bridge`/`run` call should use: the explicit
/// `--universe` flag, else the current directory. `call_engine` itself also
/// falls back to the parent (workspace-root) port file, so this only needs
/// to get the common case right.
fn resolve_universe(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let dir = explicit.unwrap_or_else(|| PathBuf::from("."));
    dir.canonicalize()
        .with_context(|| format!("universe path not found: {}", dir.display()))
}

/// The workspace root holding `.eustress/instances/`: `--workspace`, else
/// `$EUSTRESS_WORKSPACE`, else `<Documents>/Eustress` (OneDrive-safe — see
/// `eustress_bridge_client::default_workspace_root`).
fn resolve_workspace(explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(eustress_bridge_client::default_workspace_root)
}

/// An existing Space directory as a PLAIN absolute path.
///
/// Deliberately not `canonicalize()`: on Windows that yields the
/// `\\?\C:\...` verbatim form, which the engine passes straight through
/// into its `SpaceRoot` — and its Universe resolver doesn't understand the
/// prefix, so the instance record reported the wrong Universe. It also
/// means the path the engine records and the path a later `close --space`
/// passes would differ in form and never match. `std::path::absolute`
/// keeps the ordinary `C:\...` spelling everywhere.
fn absolute_space_dir(p: &std::path::Path) -> Result<PathBuf> {
    if !p.is_dir() {
        anyhow::bail!("Space directory not found: {}", p.display());
    }
    std::path::absolute(p).with_context(|| format!("cannot resolve {}", p.display()))
}

/// Where a bridge call goes. `Universe` is the classic single-instance
/// discovery through `engine.port`; `Port` is direct, and the only
/// unambiguous choice when several engines are running.
enum Target {
    Universe(PathBuf),
    Port(u16),
}

impl Target {
    fn call(&self, method: &str, params: serde_json::Value) -> std::result::Result<serde_json::Value, String> {
        self.call_with_timeout(method, params, eustress_bridge_client::DEFAULT_REPLY_TIMEOUT)
    }

    /// [`Target::call`] with a reply deadline for methods that work before
    /// answering (`sim.step` runs every requested tick first).
    fn call_with_timeout(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: std::time::Duration,
    ) -> std::result::Result<serde_json::Value, String> {
        match self {
            Target::Universe(u) => eustress_bridge_client::call_engine_with_timeout(u, method, params, timeout),
            Target::Port(p) => eustress_bridge_client::call_port_with_timeout(*p, method, params, timeout),
        }
    }
}

/// Poll `sim.state` until run `run_id` (or, when `None`, the run in progress
/// at the first poll, else the most recent one) appears among the finished
/// runs; returns its ledger record.
fn await_run_over_bridge(
    target: &Target,
    run_id: Option<u64>,
    timeout_secs: f64,
) -> std::result::Result<serde_json::Value, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout_secs.max(0.0));
    let mut want = run_id;
    loop {
        let state = target.call("sim.state", serde_json::json!({}))?;
        let runs = state.get("runs").cloned().unwrap_or_default();
        if want.is_none() {
            let active = runs
                .get("current")
                .and_then(|c| c.get("run_id"))
                .or_else(|| runs.get("pending").and_then(|p| p.get("run_id")))
                .and_then(|v| v.as_u64());
            match active {
                Some(id) => want = Some(id),
                None => {
                    return runs
                        .get("last")
                        .filter(|l| !l.is_null())
                        .cloned()
                        .ok_or_else(|| "no run in progress and none completed".to_string());
                }
            }
        }
        let id = want.unwrap_or_default();
        let done = runs
            .get("completed")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
            .find(|r| r.get("run_id").and_then(|v| v.as_u64()) == Some(id))
            .cloned();
        if let Some(record) = done {
            return Ok(record);
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("timed out after {timeout_secs:.0}s waiting for run #{id}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// One-line summary of a finished run's ledger record.
fn run_summary(record: &serde_json::Value) -> String {
    let id = record.get("run_id").and_then(|v| v.as_u64()).unwrap_or(0);
    let reason = record.get("end_reason").and_then(|v| v.as_str()).unwrap_or("stopped");
    let ticks = record.get("ticks").and_then(|v| v.as_u64()).unwrap_or(0);
    let secs = record.get("sim_seconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let values = record
        .get("final_values")
        .and_then(|v| v.as_object())
        .map(|m| m.len())
        .unwrap_or(0);
    format!("run #{id} {reason} after {ticks} ticks ({secs:.2}s simulated), {values} final value(s)")
}

/// Find a sibling binary: prefer the one next to this executable (a dev
/// build puts every bin in the same `target/<profile>/`), else fall back to
/// the bare name on PATH. Shared by `open`, `run`, and `server`.
fn sibling_binary(name: &str) -> PathBuf {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(&file)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Print a bridge result: a colored one-line summary, then the full JSON
/// pretty-printed (so `eustress bridge ... | jq` composes cleanly).
fn print_bridge_result(summary: &str, result: &serde_json::Value) {
    println!("{} {summary}", "✓".green());
    println!("{}", serde_json::to_string_pretty(result).unwrap_or_default());
}

fn print_bridge_error(method: &str, err: &str) -> Result<()> {
    eprintln!("{} {method}: {err}", "✗".red());
    anyhow::bail!("bridge call failed");
}

fn cmd_bridge(
    universe: Option<PathBuf>,
    port: Option<u16>,
    pid: Option<u32>,
    workspace: Option<PathBuf>,
    action: BridgeCommands,
) -> Result<()> {
    let target = if let Some(p) = port {
        Target::Port(p)
    } else if let Some(pid) = pid {
        let ws = resolve_workspace(workspace);
        let rec = eustress_bridge_client::list_instances_unchecked(&ws)
            .into_iter()
            .find(|r| r.pid == pid)
            .with_context(|| format!(
                "no instance with pid {pid} in {} — run `eustress instances`",
                eustress_bridge_client::instances_dir(&ws).display()
            ))?;
        Target::Port(rec.port)
    } else {
        Target::Universe(resolve_universe(universe)?)
    };

    match action {
        BridgeCommands::Ping => match target.call("ping", serde_json::json!({})) {
            Ok(r) => {
                print_bridge_result("engine bridge is alive", &r);
                Ok(())
            }
            Err(e) => print_bridge_error("ping", &e),
        },

        BridgeCommands::Call { method, params } => {
            let params: serde_json::Value = serde_json::from_str(&params)
                .with_context(|| format!("--params is not valid JSON: {params}"))?;
            match target.call(&method, params) {
                Ok(r) => {
                    print_bridge_result(&format!("{method} ok"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error(&method, &e),
            }
        }

        BridgeCommands::EcsQuery { class, offset, limit } => {
            let mut params = serde_json::Map::new();
            if let Some(c) = class { params.insert("class".into(), c.into()); }
            params.insert("offset".into(), offset.into());
            params.insert("limit".into(), limit.into());
            match target.call("ecs.query", serde_json::Value::Object(params)) {
                Ok(r) => {
                    let total = r.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                    let returned = r.get("returned").and_then(|v| v.as_u64()).unwrap_or(0);
                    print_bridge_result(&format!("{returned} of {total} entities"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("ecs.query", &e),
            }
        }

        BridgeCommands::Inspect { class, name_contains, limit } => {
            let mut params = serde_json::Map::new();
            if let Some(c) = class { params.insert("class".into(), c.into()); }
            if let Some(n) = name_contains { params.insert("name_contains".into(), n.into()); }
            params.insert("limit".into(), limit.into());
            match target.call("ecs.inspect", serde_json::Value::Object(params)) {
                Ok(r) => {
                    let total = r.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                    let fps = r.get("fps").and_then(|v| v.as_f64());
                    let fps_note = fps.map(|f| format!(", fps={f:.1}")).unwrap_or_default();
                    print_bridge_result(&format!("{total} entities{fps_note}"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("ecs.inspect", &e),
            }
        }

        BridgeCommands::SimRead { keys } => {
            let params = if keys.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::json!({ "keys": keys })
            };
            match target.call("sim.read", params) {
                Ok(r) => {
                    let count = r.as_object().map(|m| m.len()).unwrap_or(0);
                    print_bridge_result(&format!("{count} sim value(s)"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("sim.read", &e),
            }
        }

        BridgeCommands::SimStep { ticks } => {
            // The engine runs every tick before it replies, so the deadline
            // scales with the work (the MCP `sim_step` tool uses the same).
            let deadline = std::time::Duration::from_millis(5_000 + ticks.min(10_000) * 10);
            match target.call_with_timeout("sim.step", serde_json::json!({ "ticks": ticks }), deadline) {
                Ok(r) => {
                    let stepped = r.get("stepped").and_then(|v| v.as_u64()).unwrap_or(0);
                    let secs = r.get("sim_seconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    print_bridge_result(&format!("stepped {stepped} tick(s) ({secs:.3}s sim time)"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("sim.step", &e),
            }
        }

        BridgeCommands::SimRun { time_scale, duration, wait, timeout } => {
            let mut params = serde_json::Map::new();
            if let Some(s) = time_scale { params.insert("time_scale".into(), s.into()); }
            if let Some(d) = duration { params.insert("duration_s".into(), d.into()); }
            let ack = match target.call("sim.run", serde_json::Value::Object(params)) {
                Ok(r) => r,
                Err(e) => return print_bridge_error("sim.run", &e),
            };
            let run_id = ack.get("run_id").and_then(|v| v.as_u64());
            let status = ack.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
            if !wait {
                let id = run_id.map(|i| format!(" run #{i}")).unwrap_or_default();
                print_bridge_result(&format!("sim.run {status}{id}"), &ack);
                return Ok(());
            }
            println!("{} sim.run {status}{}, waiting…", "·".dimmed(), run_id.map(|i| format!(" run #{i}")).unwrap_or_default());
            match await_run_over_bridge(&target, run_id, timeout) {
                Ok(record) => {
                    print_bridge_result(&run_summary(&record), &record);
                    Ok(())
                }
                Err(e) => print_bridge_error("sim.run --wait", &e),
            }
        }

        BridgeCommands::SimPause => match target.call("sim.pause", serde_json::json!({})) {
            Ok(r) => {
                let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
                print_bridge_result(&format!("sim.pause {status}"), &r);
                Ok(())
            }
            Err(e) => print_bridge_error("sim.pause", &e),
        },

        BridgeCommands::SimStop => match target.call("sim.stop", serde_json::json!({})) {
            Ok(r) => {
                let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
                print_bridge_result(&format!("sim.stop {status}"), &r);
                Ok(())
            }
            Err(e) => print_bridge_error("sim.stop", &e),
        },

        BridgeCommands::SimSet { values } => {
            let mut map = serde_json::Map::new();
            for pair in &values {
                let (key, value) = pair
                    .split_once('=')
                    .with_context(|| format!("expected KEY=VALUE, got {pair:?}"))?;
                let value: f64 = value
                    .trim()
                    .parse()
                    .with_context(|| format!("value for {key:?} is not a number: {value:?}"))?;
                map.insert(key.trim().to_string(), value.into());
            }
            match target.call("sim.set", serde_json::json!({ "values": map })) {
                Ok(r) => {
                    let n = r.get("applied").and_then(|v| v.as_u64()).unwrap_or(0);
                    print_bridge_result(&format!("set {n} sim value(s)"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("sim.set", &e),
            }
        }

        BridgeCommands::SimState => match target.call("sim.state", serde_json::json!({})) {
            Ok(r) => {
                let play = r.get("play_state").and_then(|v| v.as_str()).unwrap_or("?");
                let pid = r.get("pid").and_then(|v| v.as_u64()).map(|p| format!(" pid {p}")).unwrap_or_default();
                let owner = if r.get("owns_universe").and_then(|v| v.as_bool()) == Some(true) { ", Universe owner" } else { "" };
                let runs = r.get("runs");
                let run = runs
                    .and_then(|x| x.get("current"))
                    .and_then(|c| c.get("run_id"))
                    .and_then(|v| v.as_u64())
                    .map(|i| format!(", run #{i} in progress"))
                    .unwrap_or_default();
                print_bridge_result(&format!("{play}{pid}{owner}{run}"), &r);
                Ok(())
            }
            Err(e) => print_bridge_error("sim.state", &e),
        },

        BridgeCommands::SimAwait { run_id, timeout } => match await_run_over_bridge(&target, run_id, timeout) {
            Ok(record) => {
                print_bridge_result(&run_summary(&record), &record);
                Ok(())
            }
            Err(e) => print_bridge_error("sim.await", &e),
        },

        BridgeCommands::Raycast { origin, direction, max_distance, max_hits } => {
            let mut params = serde_json::Map::new();
            if let Some(o) = origin { params.insert("origin".into(), o.into()); }
            if let Some(d) = direction { params.insert("direction".into(), d.into()); }
            if let Some(m) = max_distance { params.insert("max_distance".into(), m.into()); }
            if let Some(m) = max_hits { params.insert("max_hits".into(), m.into()); }
            match target.call("scene.raycast", serde_json::Value::Object(params)) {
                Ok(r) => {
                    let n = r.get("hit_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    print_bridge_result(&format!("{n} hit(s)"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("scene.raycast", &e),
            }
        }

        BridgeCommands::Oplog { limit } => {
            match target.call("oplog.tail", serde_json::json!({ "limit": limit })) {
                Ok(r) => {
                    let count = r.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                    print_bridge_result(&format!("{count} mutation record(s)"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("oplog.tail", &e),
            }
        }

        BridgeCommands::Entity { action } => cmd_bridge_entity(&target, action),
    }
}

fn cmd_bridge_entity(target: &Target, action: EntityCommands) -> Result<()> {
    match action {
        EntityCommands::Create { class, shape, name, position, size, color, material, anchored, can_collide } => {
            let mut params = serde_json::Map::new();
            params.insert("class".into(), class.into());
            params.insert("shape".into(), shape.into());
            params.insert("name".into(), name.into());
            if let Some(v) = position { params.insert("position".into(), v.into()); }
            if let Some(v) = size { params.insert("size".into(), v.into()); }
            if let Some(v) = color { params.insert("color".into(), v.into()); }
            if let Some(v) = material { params.insert("material".into(), v.into()); }
            if let Some(v) = anchored { params.insert("anchored".into(), v.into()); }
            if let Some(v) = can_collide { params.insert("can_collide".into(), v.into()); }
            match target.call("entity.create", serde_json::Value::Object(params)) {
                Ok(r) => {
                    print_bridge_result("entity created", &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("entity.create", &e),
            }
        }
        EntityCommands::Read { uuid, name } => {
            let mut params = serde_json::Map::new();
            if let Some(v) = uuid { params.insert("uuid".into(), v.into()); }
            if let Some(v) = name { params.insert("name".into(), v.into()); }
            match target.call("entity.read", serde_json::Value::Object(params)) {
                Ok(r) => {
                    print_bridge_result("entity read", &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("entity.read", &e),
            }
        }
        EntityCommands::Update { uuid, name, position, size, color, material, anchored, can_collide } => {
            let mut params = serde_json::Map::new();
            if let Some(v) = uuid { params.insert("uuid".into(), v.into()); }
            if let Some(v) = name { params.insert("name".into(), v.into()); }
            if let Some(v) = position { params.insert("position".into(), v.into()); }
            if let Some(v) = size { params.insert("size".into(), v.into()); }
            if let Some(v) = color { params.insert("color".into(), v.into()); }
            if let Some(v) = material { params.insert("material".into(), v.into()); }
            if let Some(v) = anchored { params.insert("anchored".into(), v.into()); }
            if let Some(v) = can_collide { params.insert("can_collide".into(), v.into()); }
            match target.call("entity.update", serde_json::Value::Object(params)) {
                Ok(r) => {
                    print_bridge_result("entity updated", &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("entity.update", &e),
            }
        }
        EntityCommands::Delete { uuid, name } => {
            let mut params = serde_json::Map::new();
            if let Some(v) = uuid { params.insert("uuid".into(), v.into()); }
            if let Some(v) = name { params.insert("name".into(), v.into()); }
            match target.call("entity.delete", serde_json::Value::Object(params)) {
                Ok(r) => {
                    print_bridge_result("entity deleted", &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("entity.delete", &e),
            }
        }
        EntityCommands::Find { uuid, path, class, limit } => {
            let mut params = serde_json::Map::new();
            if let Some(v) = uuid { params.insert("uuid".into(), v.into()); }
            if let Some(v) = path { params.insert("path".into(), v.into()); }
            if let Some(v) = class { params.insert("class".into(), v.into()); }
            params.insert("limit".into(), limit.into());
            match target.call("entity.find", serde_json::Value::Object(params)) {
                Ok(r) => {
                    let total = r.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                    print_bridge_result(&format!("{total} entities found"), &r);
                    Ok(())
                }
                Err(e) => print_bridge_error("entity.find", &e),
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_open / cmd_instances / cmd_close — multi-instance lifecycle
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn a new engine process on `space`, detached from this CLI, and wait
/// for its `<workspace>/.eustress/instances/<pid>.json` record to appear so
/// we can hand the caller a port to drive it on.
///
/// Detached on purpose: the engine is a long-lived window, and this command
/// returns as soon as the bridge is up. The child's stdio is dropped so it
/// neither ties this terminal up nor dies with it.
fn cmd_open(args: OpenArgs) -> Result<()> {
    let space = absolute_space_dir(&args.space)?;
    let workspace = resolve_workspace(args.workspace);

    let (bin_name, kind) = if args.headless {
        ("eustress-headless", "headless")
    } else {
        ("eustress-engine", "editor")
    };
    let bin = sibling_binary(bin_name);

    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("--space").arg(&space);
    // `--play` means the same for both kinds. The editor boots into Edit
    // unless told `--play`; the headless runner boots into Play unless told
    // `--no-autoplay`, so translate. An orchestrator opening several
    // variants sets each one's values before starting its run, which an
    // instance that starts playing on its own would race.
    match (args.headless, args.play) {
        (false, true) => {
            cmd.arg("--play");
        }
        (true, false) => {
            cmd.arg("--no-autoplay");
        }
        _ => {}
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Own process group on Windows so a Ctrl+C in this terminal doesn't
    // propagate to the engine windows it spawned.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(windows)]
    stop_std_handle_inheritance();

    let child = cmd.spawn().with_context(|| {
        format!(
            "Failed to start {}. Build it with: cargo build -p eustress-engine --bin {bin_name}",
            bin.display()
        )
    })?;
    let pid = child.id();
    // Deliberately NOT waited on — the process outlives this command.
    drop(child);

    if !args.json {
        println!(
            "{} Launched {kind} pid {pid} on {} — waiting up to {}s for its bridge…",
            "▶".cyan().bold(),
            space.display().to_string().cyan(),
            args.wait_secs
        );
    }

    // Poll for THIS pid's record. The engine writes it only after the bridge
    // has bound, so its presence means the port is live.
    let record_path = eustress_bridge_client::instance_file_path(&workspace, pid);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(args.wait_secs);
    let record = loop {
        if let Ok(rec) = eustress_bridge_client::read_instance(&record_path) {
            break Some(rec);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    };

    match record {
        Some(rec) => {
            let json = serde_json::to_value(&rec).unwrap_or_default();
            if args.json {
                println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
            } else {
                print_bridge_result(
                    &format!("{kind} pid {} is up on port {}", rec.pid, rec.port),
                    &json,
                );
                println!(
                    "  drive it with: eustress bridge --port {} <command>   (or --pid {})",
                    rec.port, rec.pid
                );
            }
            Ok(())
        }
        None => {
            // Not a failure of the launch — the process is running; we just
            // couldn't confirm its bridge in time (slow first load, huge
            // Space). Report what we know and a non-zero exit so a script
            // notices it has no port yet.
            eprintln!(
                "{} {kind} pid {pid} launched but no instance record appeared at {} within {}s. \
                 The engine may still be loading — check `eustress instances` shortly.",
                "⚠".yellow(),
                record_path.display(),
                args.wait_secs
            );
            anyhow::bail!("bridge not up in time (pid {pid})");
        }
    }
}

fn cmd_instances(args: InstancesArgs) -> Result<()> {
    let workspace = resolve_workspace(args.workspace);
    // `list_instances` probes each record and prunes the dead ones, so what
    // comes back is what is actually drivable right now.
    let live = eustress_bridge_client::list_instances(&workspace);

    // The owner of a Universe is the instance whose port is in its
    // `engine.port`: the engine a Universe-addressed client (`bridge
    // --universe`, the MCP server by default) reaches, and the one that
    // serves the Universe's legacy sim-command queue.
    let owns_universe = |r: &eustress_bridge_client::InstanceRecord| {
        r.universe
            .as_ref()
            .and_then(|u| eustress_bridge_client::read_port_file(&u.join(".eustress").join("engine.port")))
            == Some(r.port)
    };

    if args.json {
        let rows: Vec<serde_json::Value> = live
            .iter()
            .map(|r| {
                let mut v = serde_json::to_value(r).unwrap_or_default();
                if let serde_json::Value::Object(ref mut m) = v {
                    m.insert("owns_universe".into(), owns_universe(r).into());
                }
                v
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return Ok(());
    }

    if live.is_empty() {
        println!(
            "{} no running engine instances (registry: {})",
            "·".dimmed(),
            eustress_bridge_client::instances_dir(&workspace).display()
        );
        return Ok(());
    }

    println!("{}", format!("{} running instance(s)", live.len()).bold());
    println!("{}", "─".repeat(78).dimmed());
    println!("  {:<8} {:<6} {:<9} {:<6} {}", "PID", "PORT", "KIND", "OWNER", "SPACE");
    for r in &live {
        let space = r
            .space
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".to_string());
        let owner = if owns_universe(r) { "*" } else { "" };
        println!(
            "  {:<8} {:<6} {:<9} {:<6} {}",
            r.pid.to_string().yellow(),
            r.port.to_string().cyan(),
            r.kind.as_str(),
            owner.green(),
            space.dimmed()
        );
    }
    if live.iter().any(|r| owns_universe(r)) {
        println!(
            "{}",
            "  * owns its Universe: reached by --universe and serves the Universe's sim queue".dimmed()
        );
    }
    Ok(())
}

fn cmd_close(args: CloseArgs) -> Result<()> {
    let workspace = resolve_workspace(args.workspace);
    let live = eustress_bridge_client::list_instances(&workspace);

    let want_space = match args.space {
        Some(s) => Some(absolute_space_dir(&s)?),
        None => None,
    };

    let targets: Vec<_> = live
        .into_iter()
        .filter(|r| {
            if args.all {
                true
            } else if let Some(pid) = args.pid {
                r.pid == pid
            } else if let Some(ws) = &want_space {
                r.space.as_deref().map(|p| p == ws.as_path()).unwrap_or(false)
            } else {
                false
            }
        })
        .collect();

    if targets.is_empty() {
        if !args.all && args.pid.is_none() && want_space.is_none() {
            anyhow::bail!("nothing selected — pass --pid <N>, --space <dir>, or --all");
        }
        println!("{} no matching running instances", "·".dimmed());
        return Ok(());
    }

    let mut failed = 0usize;
    for r in &targets {
        match eustress_bridge_client::call_port(r.port, "engine.shutdown", serde_json::json!({})) {
            Ok(_) => println!(
                "{} pid {} ({}) shutting down",
                "✓".green(),
                r.pid,
                r.kind.as_str()
            ),
            Err(e) if args.force => {
                eprintln!("{} pid {}: graceful shutdown failed ({e}); killing", "⚠".yellow(), r.pid);
                if kill_pid(r.pid) {
                    // The engine never got to remove its own record, or
                    // its private IPC directory.
                    let _ = std::fs::remove_file(eustress_bridge_client::instance_file_path(&workspace, r.pid));
                    let _ = std::fs::remove_dir_all(eustress_bridge_client::instance_dir(&workspace, r.pid));
                    println!("{} pid {} killed", "✓".green(), r.pid);
                } else {
                    eprintln!("{} pid {}: kill failed", "✗".red(), r.pid);
                    failed += 1;
                }
            }
            Err(e) => {
                eprintln!("{} pid {}: {e} (use --force to kill)", "✗".red(), r.pid);
                failed += 1;
            }
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} instance(s) could not be closed");
    }
    Ok(())
}

/// Keep this process's standard handles out of any child it spawns.
///
/// `open` returns while the engine it launched keeps running, and whoever
/// launched `open` (an agent capturing `--json`, a shell `$(...)`) reads its
/// output until EOF. EOF arrives only once every holder of the pipe's write
/// end has closed it. `std::process::Command` spawns with handle inheritance
/// on, so an inheritable handle this process holds, such as the stdout pipe
/// its parent gave it, reaches the engine whatever the engine's own `Stdio`
/// settings say, and the caller then waits for as long as the engine runs.
#[cfg(windows)]
fn stop_std_handle_inheritance() {
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;

    for handle in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        if !handle.is_null() {
            // SAFETY: a handle this process owns, or one the call rejects
            // harmlessly (a console pseudo-handle); only the inherit flag
            // changes.
            unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Best-effort hard kill, for `close --force` when the bridge won't answer.
fn kill_pid(pid: u32) -> bool {
    #[cfg(windows)]
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    #[cfg(not(windows))]
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    status.map(|s| s.success()).unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_run — one-shot headless batch runner (HEADLESS_RUNTIME.md §8)
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_run(args: RunArgs) -> Result<()> {
    let space = absolute_space_dir(&args.space)?;

    let mut cmd = tokio::process::Command::new(sibling_binary("eustress-headless"));
    cmd.arg("--space").arg(&space);
    cmd.arg("--tick-rate").arg(args.tick_rate.to_string());
    if let Some(n) = args.ticks {
        cmd.arg("--ticks").arg(n.to_string());
    }
    if args.no_autoplay {
        cmd.arg("--no-autoplay");
    }

    println!(
        "{} Running {} {}…",
        "▶".cyan().bold(),
        space.display().to_string().cyan(),
        match args.ticks {
            Some(n) => format!("for {n} ticks"),
            None => "(no tick limit — until killed or stopped via bridge)".to_string(),
        }
    );

    let mut child = cmd.spawn().context(
        "Failed to start eustress-headless. Build it with: \
         cargo build -p eustress-engine --bin eustress-headless"
    )?;
    let status = child.wait().await.context("eustress-headless error")?;

    if status.success() {
        println!("{} Run complete.", "✓".green());
        Ok(())
    } else {
        anyhow::bail!("eustress-headless exited with: {status}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_server
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_server(action: ServerCommands) -> Result<()> {
    match action {
        ServerCommands::Start { port, max_players, scene, tick_rate } => {
            let mut cmd = tokio::process::Command::new(sibling_binary("eustress-server"));
            cmd.arg("--port").arg(port.to_string())
               .arg("--max-players").arg(max_players.to_string())
               .arg("--tick-rate").arg(tick_rate.to_string());
            if let Some(s) = scene { cmd.arg("--scene").arg(s); }

            println!("{} Starting eustress-server on port {port}…", "●".green());

            let mut child = cmd.spawn().context(
                "Failed to start eustress-server. Build it with: cargo build -p eustress-server"
            )?;
            let status = child.wait().await.context("eustress-server error")?;
            if !status.success() {
                anyhow::bail!("eustress-server exited with: {status}");
            }
        }

        ServerCommands::Watch => {
            eprintln!(
                "{} 'server watch' needs a live network connection and isn't wired up yet. \
                 Use `eustress bridge ping` to check a running engine/eustress-headless instead.",
                "ℹ".cyan().bold()
            );
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_publish
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_publish(args: PublishArgs) -> Result<()> {
    let space_path = args.space_path
        .canonicalize()
        .with_context(|| format!("Space path not found: {}", args.space_path.display()))?;

    println!(
        "{} Publishing {} to Cloudflare R2 (env: {})…",
        "▲".cyan().bold(),
        space_path.display().to_string().cyan(),
        args.env
    );

    if args.dry_run {
        println!("{} Dry run — no files uploaded.", "ℹ".yellow());
        return Ok(());
    }

    let wrangler_toml = space_path
        .ancestors()
        .find_map(|p| {
            let c = p.join("infrastructure/cloudflare/wrangler.toml");
            if c.exists() { Some(c) } else { None }
        })
        .unwrap_or_else(|| PathBuf::from("wrangler.toml"));

    let status = tokio::process::Command::new("wrangler")
        .arg("r2").arg("object").arg("put")
        .arg("--config").arg(&wrangler_toml)
        .arg("--env").arg(&args.env)
        .current_dir(&space_path)
        .status()
        .await
        .context("Failed to invoke wrangler. Install with: npm install -g wrangler")?;

    if !status.success() {
        anyhow::bail!("wrangler failed with: {status}");
    }

    println!("{} Published successfully.", "✓".green());
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_sim — simulation history (in-process ring buffer replay)
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_sim(iggy_url: &str, action: SimCommands) -> Result<()> {
    let config = SimStreamConfig {
        url: iggy_url.to_string(),
        ..Default::default()
    };

    let reader = SimStreamReader::connect(&config)
        .await
        .map_err(|e| anyhow::anyhow!("SimStreamReader init failed: {e}"))?;

    match action {
        SimCommands::Replay { scenario, limit, json } => {
            let query = SimQuery { limit, ..Default::default() };
            let records = reader.replay_sim_results(&query).await;

            let records: Vec<&SimRecord> = records.iter()
                .filter(|r| {
                    scenario.as_deref().map_or(true, |f| {
                        r.scenario_name.to_lowercase().contains(&f.to_lowercase())
                    })
                })
                .collect();

            if json {
                println!("{}", serde_json::to_string_pretty(&records).unwrap_or_default());
                return Ok(());
            }

            println!("{}", format!("Simulation runs — {} found", records.len()).bold());
            println!("{}", "─".repeat(60).dimmed());
            for r in &records {
                let best = r.best_branch()
                    .map(|b| format!("{} ({:.1}%)", b.label, b.posterior * 100.0))
                    .unwrap_or_else(|| "—".to_string());
                println!(
                    "  {:>3}  {:<32}  samples: {:>7}  best: {}  {}ms",
                    format!("#{}", r.session_seq).dimmed(),
                    r.scenario_name.cyan(),
                    r.total_samples.to_string().yellow(),
                    best.green(),
                    r.duration_ms,
                );
            }
            if records.is_empty() {
                println!("  {}", "(no simulation runs recorded yet)".dimmed());
            }
        }

        SimCommands::Best { session, json } => {
            let query = SimQuery { limit: 0, ..Default::default() };
            let best = if let Some(ref sess_hex) = session {
                let sess_hex = sess_hex.to_lowercase();
                let all = reader.replay_iterations(&query).await;
                all.into_iter()
                    .filter(|r| {
                        let id_hex = format!("{:032x}", r.session_id);
                        id_hex.starts_with(&sess_hex)
                    })
                    .max_by(|a, b| a.similarity.partial_cmp(&b.similarity)
                        .unwrap_or(std::cmp::Ordering::Equal))
            } else {
                reader.best_iteration(&query).await
            };

            match best {
                Some(r) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                        return Ok(());
                    }
                    println!("{}", "Best iteration".bold());
                    println!("{}", "─".repeat(60).dimmed());
                    println!("  similarity : {}", format!("{:.1}%", r.similarity * 100.0).green().bold());
                    println!("  iteration  : {}", r.iteration);
                    println!("  feedback   : {}", r.verifier_feedback.dimmed());
                    println!("  duration   : {}ms", r.duration_ms);
                    println!("  code ({} chars):", r.generated_code.len());
                    for line in r.generated_code.lines().take(20) {
                        println!("    {line}");
                    }
                    if r.generated_code.lines().count() > 20 {
                        println!("    {}", "... (truncated)".dimmed());
                    }
                }
                None => println!("{}", "(no iterations recorded yet)".dimmed()),
            }
        }

        SimCommands::Convergence { product, limit, json } => {
            let query = SimQuery { limit, ..Default::default() };
            let records = reader.workshop_convergence(&query).await;

            let records: Vec<&WorkshopIterationRecord> = records.iter()
                .filter(|r| {
                    product.as_deref().map_or(true, |f| {
                        r.product_name.to_lowercase().contains(&f.to_lowercase())
                    })
                })
                .collect();

            if json {
                println!("{}", serde_json::to_string_pretty(&records).unwrap_or_default());
                return Ok(());
            }

            println!("{}", format!("Workshop convergence — {} generations", records.len()).bold());
            println!("{}", "─".repeat(70).dimmed());
            for r in &records {
                println!(
                    "  {:>4}  {:<28}  {:>8.3}  {}  {}",
                    r.generation,
                    r.product_name.cyan(),
                    r.fitness,
                    if r.is_best_generation { "★ best".green().to_string() } else { "".to_string() },
                    r.best_branch_label.dimmed(),
                );
            }
            if records.is_empty() {
                println!("  {}", "(no workshop iterations recorded yet)".dimmed());
            }
        }

        SimCommands::Scripts { scenario, limit, json } => {
            let query = SimQuery { limit, ..Default::default() };
            let records = reader.replay_rune_scripts(&query).await;

            let allowed_ids: Option<std::collections::HashSet<u128>> = if let Some(ref filter) = scenario {
                let filter_lc = filter.to_lowercase();
                let sim_query = SimQuery { limit: 0, ..Default::default() };
                let sim_records = reader.replay_sim_results(&sim_query).await;
                let ids: std::collections::HashSet<u128> = sim_records.iter()
                    .filter(|r| r.scenario_name.to_lowercase().contains(&filter_lc))
                    .map(|r| r.scenario_id)
                    .collect();
                Some(ids)
            } else {
                None
            };

            let records: Vec<&RuneScriptRecord> = records.iter()
                .filter(|r| {
                    match &allowed_ids {
                        Some(ids) => ids.contains(&r.scenario_id),
                        None => true,
                    }
                })
                .collect();

            if json {
                println!("{}", serde_json::to_string_pretty(&records).unwrap_or_default());
                return Ok(());
            }

            println!("{}", format!("Rune script audit — {} records", records.len()).bold());
            println!("{}", "─".repeat(60).dimmed());
            for r in &records {
                let status = if r.success { "OK".green() } else { "ERR".red() };
                println!(
                    "  [{}] seq:{:>4}  overrides:{:>2}  collapsed:{:>2}  new_branches:{:>2}  {}µs",
                    status,
                    r.session_seq,
                    r.probability_overrides.len(),
                    r.collapsed_branches.len(),
                    r.new_branches.len(),
                    r.execution_us,
                );
                if !r.error.is_empty() {
                    println!("       error: {}", r.error.red());
                }
                for msg in &r.log_messages {
                    println!("       log: {}", msg.dimmed());
                }
            }
            if records.is_empty() {
                println!("  {}", "(no Rune script records yet)".dimmed());
            }
        }

        SimCommands::Arc { task, limit, json } => {
            let query = SimQuery { limit, ..Default::default() };
            let records = reader.replay_arc_episodes(&query).await;

            let records: Vec<&ArcEpisodeRecord> = records.iter()
                .filter(|r| {
                    task.as_deref().map_or(true, |f| {
                        r.task_id.to_lowercase().contains(&f.to_lowercase())
                    })
                })
                .collect();

            if json {
                println!("{}", serde_json::to_string_pretty(&records).unwrap_or_default());
                return Ok(());
            }

            println!("{}", format!("ARC-AGI-3 episodes — {} found", records.len()).bold());
            println!("{}", "─".repeat(80).dimmed());
            for r in &records {
                println!(
                    "  {:<32}  {:<8}  {:>6}  {:>8.3}  {:>10}  {:>10}",
                    format!("{:032x}", r.episode_id).dimmed(),
                    r.task_id.cyan(),
                    r.total_steps.to_string().yellow(),
                    r.efficiency_ratio,
                    if r.goal_reached { "✓".green().to_string() } else { "✗".red().to_string() },
                    r.duration_ms,
                );
            }
            if records.is_empty() {
                println!("  {}", "(no ARC episode records yet)".dimmed());
            }
        }

        SimCommands::ArcBest { task, json } => {
            match reader.best_arc_episode(&task).await {
                Some(r) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                        return Ok(());
                    }
                    println!("{}", format!("Best ARC episode — task: {}", task).bold());
                    println!("{}", "─".repeat(60).dimmed());
                    println!("  episode_id     : {:032x}", r.episode_id);
                    println!("  task_id        : {}", r.task_id.cyan());
                    println!("  steps          : {}", r.total_steps.to_string().yellow());
                    println!("  efficiency     : {:.3}", r.efficiency_ratio);
                    println!("  goal_reached   : {}", if r.goal_reached { "yes".green().to_string() } else { "no".red().to_string() });
                    println!("  final_score    : {:.3}", r.final_score);
                    println!("  duration_ms    : {}", r.duration_ms);
                    println!("  actions ({}):", r.actions_taken.len());
                    for (i, a) in r.actions_taken.iter().enumerate().take(20) {
                        println!("    [{i:>3}] {a}");
                    }
                    if r.actions_taken.len() > 20 {
                        println!("    {}", "... (truncated)".dimmed());
                    }
                }
                None => println!("{}", format!("(no ARC episodes recorded for task '{task}')").dimmed()),
            }
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_fork — Fork registration and validation loop
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_fork(action: ForkCommands) -> Result<()> {
    match action {
        ForkCommands::Register {
            fork_id,
            chain_id,
            contact,
            port,
            key_file,
            skip_tunnel,
        } => {
            if chain_id == 1 || chain_id == 2 {
                anyhow::bail!("Chain ID 1 (mainnet) and 2 (testnet) are reserved");
            }

            println!("{}", "── Fork Registration ──".bold());
            println!("  Fork ID  : {}", fork_id.cyan());
            println!("  Chain ID : {}", chain_id.to_string().yellow());
            println!("  Contact  : {}", contact);
            println!();

            // Step 1: Generate or load fork keypair
            let key_path = key_file.unwrap_or_else(|| PathBuf::from("fork-key.json"));
            if key_path.exists() {
                println!("{} Using existing keypair: {}", "●".green(), key_path.display());
            } else {
                println!("{} Generating Ed25519 keypair → {}", "●".cyan(), key_path.display());
                // Use bliss-cli if available, otherwise generate inline
                let status = tokio::process::Command::new("bliss")
                    .args(["wallet", "create", "--name", &format!("fork-{}", fork_id)])
                    .status()
                    .await;
                match status {
                    Ok(s) if s.success() => {
                        println!("  {} Keypair generated via bliss-cli", "✓".green());
                    }
                    _ => {
                        // Fallback: generate a random keypair and save seed
                        use std::io::Write;
                        let seed: [u8; 32] = rand::random();
                        let mut f = std::fs::File::create(&key_path)?;
                        let hex_seed = hex::encode(seed);
                        writeln!(f, "{{")?;
                        writeln!(f, "  \"fork_id\": \"{fork_id}\",")?;
                        writeln!(f, "  \"chain_id\": {chain_id},")?;
                        writeln!(f, "  \"private_key_hex\": \"{hex_seed}\",")?;
                        writeln!(f, "  \"contact\": \"{contact}\"")?;
                        writeln!(f, "}}")?;
                        println!("  {} Keypair generated: {}", "✓".green(), key_path.display());
                        println!(
                            "  {} Keep this file secure — it signs balance attestations",
                            "⚠".yellow()
                        );
                    }
                }
            }

            // Step 2: Set up Cloudflare Tunnel (if not skipped)
            if !skip_tunnel {
                println!();
                println!("{} Setting up Cloudflare Tunnel…", "●".cyan());

                // Check cloudflared is installed
                let cf_check = tokio::process::Command::new("cloudflared")
                    .arg("--version")
                    .output()
                    .await;
                match cf_check {
                    Ok(output) if output.status.success() => {
                        let ver = String::from_utf8_lossy(&output.stdout);
                        println!("  {} cloudflared: {}", "✓".green(), ver.trim());
                    }
                    _ => {
                        println!(
                            "  {} cloudflared not found. Install from:",
                            "✗".red()
                        );
                        println!("    https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/");
                        println!();
                        println!("  After installing, run these commands:");
                        println!("    cloudflared tunnel login");
                        println!("    cloudflared tunnel create {fork_id}");
                        println!(
                            "    cloudflared tunnel route dns {fork_id} {fork_id}.eustress.dev"
                        );
                        println!(
                            "    cloudflared tunnel run --url http://localhost:{port} {fork_id}"
                        );
                        println!();
                        println!("  Then re-run: eustress fork register --fork-id {fork_id} --chain-id {chain_id} --contact \"{contact}\" --skip-tunnel");
                        return Ok(());
                    }
                }

                // Create tunnel
                println!("  Creating tunnel '{fork_id}'…");
                let create = tokio::process::Command::new("cloudflared")
                    .args(["tunnel", "create", &fork_id])
                    .status()
                    .await;
                match create {
                    Ok(s) if s.success() => {
                        println!("  {} Tunnel created", "✓".green());
                    }
                    _ => {
                        println!(
                            "  {} Tunnel may already exist (that's OK)",
                            "ℹ".yellow()
                        );
                    }
                }

                // Route DNS
                let subdomain = format!("{}.eustress.dev", fork_id.replace('.', "-"));
                println!("  Routing DNS: {subdomain} → tunnel…");
                let route = tokio::process::Command::new("cloudflared")
                    .args(["tunnel", "route", "dns", &fork_id, &subdomain])
                    .status()
                    .await;
                match route {
                    Ok(s) if s.success() => {
                        println!("  {} DNS routed: {subdomain}", "✓".green());
                    }
                    _ => {
                        println!(
                            "  {} DNS route may already exist (that's OK)",
                            "ℹ".yellow()
                        );
                    }
                }

                println!();
                println!("{} To start the tunnel, run:", "→".cyan());
                println!(
                    "    cloudflared tunnel run --url http://localhost:{port} {fork_id}"
                );
            }

            // Step 3: Submit registration to OTR
            println!();
            println!("{} Submitting to Online Trust Registry…", "●".cyan());
            println!(
                "  POST https://eustress.dev/api/fork-register"
            );
            println!("  {{");
            println!("    \"fork_id\": \"{fork_id}\",");
            println!("    \"chain_id\": {chain_id},");
            println!("    \"contact\": \"{contact}\",");
            println!("    \"endpoint\": \"https://{}.eustress.dev\"", fork_id.replace('.', "-"));
            println!("  }}");
            println!();

            // Attempt API call
            let endpoint = format!("https://{}.eustress.dev", fork_id.replace('.', "-"));
            let body = serde_json::json!({
                "fork_id": fork_id,
                "chain_id": chain_id,
                "contact": contact,
                "endpoint": endpoint,
            });

            let client = reqwest::Client::new();
            match client
                .post("https://eustress.dev/api/fork-register")
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    if status.is_success() {
                        println!("{} Registration submitted successfully!", "✓".green().bold());
                        println!("  {text}");
                    } else {
                        println!("{} Registration returned {status}: {text}", "⚠".yellow());
                        println!("  Your fork has been queued. Run 'eustress fork status --fork-id {fork_id}' to check.");
                    }
                }
                Err(e) => {
                    println!("{} Could not reach eustress.dev: {e}", "⚠".yellow());
                    println!("  Registration will be retried when the validation loop runs.");
                    println!("  Ensure your tunnel is running and well-known endpoints are live.");
                }
            }

            println!();
            println!("{}", "── Next Steps ──".bold());
            println!("  1. Start your fork server on port {port}");
            println!("  2. Serve /.well-known/eustress-fork with your fork info");
            println!("  3. Start the tunnel: cloudflared tunnel run --url http://localhost:{port} {fork_id}");
            println!("  4. Check status: eustress fork status --fork-id {fork_id}");

            Ok(())
        }

        ForkCommands::Loop {
            registry_url,
            interval_minutes,
            once,
        } => {
            println!("{}", "── Fork Validation Loop ──".bold());
            println!("  Registry : {}", registry_url.cyan());
            println!(
                "  Interval : {} minutes",
                interval_minutes.to_string().yellow()
            );
            if once {
                println!("  Mode     : single pass");
            } else {
                println!("  Mode     : continuous");
            }
            println!();

            let client = reqwest::Client::new();

            loop {
                println!(
                    "{} Checking registration queue… ({})",
                    "●".cyan(),
                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                );

                // Fetch pending registrations
                let pending_url = format!("{}/api/fork-registrations?status=pending", registry_url);
                match client.get(&pending_url).send().await {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            let text = resp.text().await.unwrap_or_default();
                            let registrations: Vec<serde_json::Value> =
                                serde_json::from_str(&text).unwrap_or_default();

                            if registrations.is_empty() {
                                println!("  {} No pending registrations", "·".dimmed());
                            } else {
                                println!(
                                    "  {} {} pending registrations",
                                    "→".yellow(),
                                    registrations.len()
                                );

                                for reg in &registrations {
                                    let fid = reg["fork_id"].as_str().unwrap_or("?");
                                    let ep = reg["endpoint"].as_str().unwrap_or("?");

                                    println!("  Validating {fid}…");

                                    // Step 1: Fetch /.well-known/eustress-fork
                                    let well_known_url =
                                        format!("{}/.well-known/eustress-fork", ep);
                                    match client.get(&well_known_url).send().await {
                                        Ok(wk_resp) if wk_resp.status().is_success() => {
                                            let wk_text =
                                                wk_resp.text().await.unwrap_or_default();
                                            match serde_json::from_str::<serde_json::Value>(
                                                &wk_text,
                                            ) {
                                                Ok(fork_info) => {
                                                    let claimed_id = fork_info["fork_id"]
                                                        .as_str()
                                                        .unwrap_or("");
                                                    if claimed_id == fid {
                                                        println!(
                                                            "    {} Well-known endpoint verified",
                                                            "✓".green()
                                                        );

                                                        // Step 2: Approve
                                                        let approve_url = format!(
                                                            "{}/api/fork-registrations/{}/approve",
                                                            registry_url, fid
                                                        );
                                                        let _ =
                                                            client.post(&approve_url).send().await;
                                                        println!(
                                                            "    {} Approved: {fid}",
                                                            "✓".green().bold()
                                                        );
                                                    } else {
                                                        println!(
                                                            "    {} fork_id mismatch: claimed '{}', expected '{fid}'",
                                                            "✗".red(),
                                                            claimed_id
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    println!(
                                                        "    {} Invalid JSON from well-known: {e}",
                                                        "✗".red()
                                                    );
                                                }
                                            }
                                        }
                                        Ok(wk_resp) => {
                                            println!(
                                                "    {} Well-known returned {}",
                                                "✗".red(),
                                                wk_resp.status()
                                            );
                                        }
                                        Err(e) => {
                                            println!(
                                                "    {} Cannot reach {ep}: {e}",
                                                "✗".red()
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            println!(
                                "  {} Registry returned {}",
                                "⚠".yellow(),
                                resp.status()
                            );
                        }
                    }
                    Err(e) => {
                        println!("  {} Cannot reach registry: {e}", "✗".red());
                    }
                }

                if once {
                    break;
                }

                println!(
                    "  {} Next check in {interval_minutes} minutes",
                    "⏳".dimmed()
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(
                    interval_minutes * 60,
                ))
                .await;
            }

            Ok(())
        }

        ForkCommands::Status {
            fork_id,
            registry_url,
        } => {
            let client = reqwest::Client::new();
            let url = format!("{}/api/fork-registrations/{}", registry_url, fork_id);
            match client.get(&url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    if status.is_success() {
                        let info: serde_json::Value =
                            serde_json::from_str(&text).unwrap_or_default();
                        println!("{}", "── Fork Status ──".bold());
                        println!("  Fork ID   : {}", fork_id.cyan());
                        println!(
                            "  Status    : {}",
                            info["status"].as_str().unwrap_or("unknown")
                        );
                        println!(
                            "  Chain ID  : {}",
                            info["chain_id"].as_u64().unwrap_or(0)
                        );
                        println!(
                            "  Endpoint  : {}",
                            info["endpoint"].as_str().unwrap_or("?")
                        );
                        println!(
                            "  Registered: {}",
                            info["registered_at"].as_str().unwrap_or("?")
                        );
                    } else {
                        println!("{} Fork '{fork_id}' not found ({status})", "✗".red());
                    }
                }
                Err(e) => {
                    println!("{} Cannot reach registry: {e}", "✗".red());
                }
            }
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
