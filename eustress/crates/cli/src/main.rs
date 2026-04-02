//! # eustress — Headless CLI for Eustress Engine
//!
//! ## Table of Contents
//! - Cli / Commands       — CLAP top-level command tree
//! - cmd_server           — `eustress server`  — start headless dedicated server
//! - cmd_publish          — `eustress publish` — publish Space to Cloudflare R2
//! - cmd_sim              — `eustress sim`     — simulation history (in-process only)
//! - cmd_stream/agent     — stubs (EustressStream is in-process; no network transport)
//!
//! ## Note on streaming commands
//! Apache Iggy has been replaced with `eustress-stream`, an **in-process** streaming
//! library. The `stream`, `agent`, `scene`, `server watch`, and `stats` commands
//! required a live network connection to an Iggy server and are now stubs that
//! print an informative message. The `publish`, `server start`, and `sim` commands
//! continue to work as before.

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
    /// [stub] Subscribe to the live scene delta feed (requires in-process access).
    Stream(StreamArgs),

    /// [stub] Agent-in-the-loop commands (requires in-process access).
    Agent(AgentArgs),

    /// [stub] Scene utilities: snapshot, replay, diff (requires in-process access).
    Scene {
        #[command(subcommand)]
        action: SceneCommands,
    },

    /// Manage headless dedicated server processes.
    Server {
        #[command(subcommand)]
        action: ServerCommands,
    },

    /// Publish a Space to Cloudflare R2 via Wrangler.
    Publish(PublishArgs),

    /// [stub] Show stream statistics (requires in-process access).
    Stats(StatsArgs),

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

#[derive(Args, Debug)]
struct StreamArgs {
    #[arg(long, value_delimiter = ',')]
    filter: Vec<String>,
    #[arg(long, default_value = "0")]
    limit: u64,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    from_seq: Option<u64>,
}

#[derive(Args, Debug)]
struct AgentArgs {
    #[arg(long, short)]
    script: Option<String>,
    #[arg(long)]
    script_file: Option<PathBuf>,
    #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
    spawn_part: Option<Vec<f32>>,
    #[arg(long, default_value = "Part")]
    class_name: String,
    #[arg(long, num_args = 4, value_names = ["ENTITY", "X", "Y", "Z"])]
    set_transform: Option<Vec<f32>>,
    #[arg(long)]
    snapshot: bool,
    #[arg(long)]
    simulate_ticks: Option<u32>,
    #[arg(long, default_value = "10")]
    timeout_secs: u64,
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum SceneCommands {
    Snapshot {
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    Replay {
        #[arg(long, default_value = "0")]
        from: u64,
        #[arg(long, default_value = "0")]
        to: u64,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Diff {
        #[arg(long)]
        seq_a: u64,
        #[arg(long)]
        seq_b: u64,
    },
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

#[derive(Args, Debug)]
struct StatsArgs {
    #[arg(long, default_value = "0")]
    watch: u64,
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
        Commands::Stream(args) => cmd_streaming_stub("stream", args.json),
        Commands::Agent(args) => cmd_streaming_stub("agent", args.json),
        Commands::Scene { action: _ } => cmd_streaming_stub("scene", false),
        Commands::Server { action } => cmd_server(action).await,
        Commands::Publish(args) => cmd_publish(args).await,
        Commands::Stats(_) => cmd_streaming_stub("stats", false),
        Commands::Sim { action } => cmd_sim(&cli.iggy_url, action).await,
        Commands::Fork { action } => cmd_fork(action).await,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Streaming commands — stubs
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_streaming_stub(cmd: &str, _json: bool) -> Result<()> {
    eprintln!(
        "{} The '{}' command is not available in this build.\n\
         \n\
         EustressStream is an in-process streaming library — live streaming\n\
         commands require access to the running engine process directly.\n\
         \n\
         For simulation history use:  eustress sim replay / best / convergence\n\
         For server management use:   eustress server start\n\
         For publishing use:          eustress publish",
        "ℹ".cyan().bold(),
        cmd
    );
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_server
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_server(action: ServerCommands) -> Result<()> {
    match action {
        ServerCommands::Start { port, max_players, scene, tick_rate } => {
            let mut cmd = tokio::process::Command::new("eustress-server");
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
            return cmd_streaming_stub("server watch", false);
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
