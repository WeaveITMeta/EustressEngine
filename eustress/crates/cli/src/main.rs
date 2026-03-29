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

use eustress_common::iggy_delta::IGGY_DEFAULT_URL;

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
    /// Legacy Iggy URL flag — retained for script compatibility, not used.
    #[arg(long, global = true, env = "IGGY_URL", default_value = IGGY_DEFAULT_URL, hide = true)]
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
// Utilities
// ─────────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
