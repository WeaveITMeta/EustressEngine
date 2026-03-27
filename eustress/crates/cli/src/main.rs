//! # eustress — Headless CLI for Eustress Engine
//!
//! ## Table of Contents
//! - Cli / Commands       — CLAP top-level command tree
//! - IggySession          — thin wrapper: one IggyClient connection per command
//! - cmd_stream           — `eustress stream`   — subscribe to live scene delta feed
//! - cmd_agent            — `eustress agent`    — agent-in-the-loop: send commands, receive observations
//! - cmd_scene            — `eustress scene`    — snapshot / replay / diff
//! - cmd_server           — `eustress server`   — start/watch headless dedicated server
//! - cmd_publish          — `eustress publish`  — publish Space to Cloudflare R2 via Wrangler
//! - cmd_stats            — `eustress stats`    — Iggy stream/topic statistics
//!
//! ## Quick start
//! ```sh
//! # Watch live mutations from a running Studio session
//! eustress stream
//!
//! # Put an agent in the loop with a Rune script
//! eustress agent --script "workspace.spawn(\"Part\")"
//!
//! # Snapshot the current scene
//! eustress scene snapshot --out snapshot.toml
//!
//! # Replay deltas 0–500
//! eustress scene replay --from 0 --to 500 --out replay.toml
//! ```

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use bytes::Bytes;
use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::time::timeout;
use tracing::warn;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use uuid::Uuid;

use iggy::clients::client::IggyClient;
use iggy::prelude::{
    Consumer, Identifier, IggyMessage, MessageClient, Partitioning,
    PollingStrategy, TopicClient,
};
use eustress_common::sim_record::{ArcEpisodeRecord, IterationRecord, RuneScriptRecord, SimRecord, WorkshopIterationRecord};
use eustress_common::sim_stream::{SimQuery, SimStreamConfig, SimStreamReader};

use eustress_common::iggy_delta::{
    AgentAction, AgentCommand, AgentObservation, DeltaKind, ObservationPayload, SceneDelta,
    IGGY_DEFAULT_URL, IGGY_STREAM_NAME,
    IGGY_TOPIC_AGENT_COMMANDS, IGGY_TOPIC_AGENT_OBSERVATIONS, IGGY_TOPIC_SCENE_DELTAS,
};

// ─────────────────────────────────────────────────────────────────────────────
// CLI definition
// ─────────────────────────────────────────────────────────────────────────────

/// Eustress Engine CLI — headless control, streaming, and agent-in-the-loop via Apache Iggy.
#[derive(Parser, Debug)]
#[command(name = "eustress")]
#[command(about = "Eustress Engine CLI — headless control + Iggy datastreams")]
#[command(version)]
#[command(propagate_version = true)]
struct Cli {
    /// Iggy server connection URL.
    #[arg(long, global = true, env = "IGGY_URL", default_value = IGGY_DEFAULT_URL)]
    iggy_url: String,

    /// Enable verbose/debug logging.
    #[arg(long, short, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Subscribe to the live scene delta feed from a running Studio or Server session.
    Stream(StreamArgs),

    /// Agent-in-the-loop: send commands into a session and receive observations.
    Agent(AgentArgs),

    /// Scene utilities: snapshot, replay, diff, and export.
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

    /// Show Iggy stream/topic statistics.
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
    /// Only show these delta kinds (comma-separated, e.g. TransformChanged,PartAdded).
    #[arg(long, value_delimiter = ',')]
    filter: Vec<String>,

    /// Stop after N deltas (0 = stream forever).
    #[arg(long, default_value = "0")]
    limit: u64,

    /// Output raw JSON instead of pretty terminal output.
    #[arg(long)]
    json: bool,

    /// Replay from this sequence number (default: next available).
    #[arg(long)]
    from_seq: Option<u64>,
}

#[derive(Args, Debug)]
struct AgentArgs {
    /// Rune script source to execute in the session.
    #[arg(long, short)]
    script: Option<String>,

    /// Load Rune script from a file.
    #[arg(long)]
    script_file: Option<PathBuf>,

    /// Spawn a Part at X Y Z.
    #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"])]
    spawn_part: Option<Vec<f32>>,

    /// Class name to spawn (used with --spawn-part).
    #[arg(long, default_value = "Part")]
    class_name: String,

    /// Set transform: entity_index X Y Z.
    #[arg(long, num_args = 4, value_names = ["ENTITY", "X", "Y", "Z"])]
    set_transform: Option<Vec<f32>>,

    /// Request a full scene snapshot.
    #[arg(long)]
    snapshot: bool,

    /// Simulate forward N ticks then pause.
    #[arg(long)]
    simulate_ticks: Option<u32>,

    /// Timeout in seconds waiting for the observation reply.
    #[arg(long, default_value = "10")]
    timeout_secs: u64,

    /// Print the raw observation payload as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum SceneCommands {
    /// Request a full scene snapshot from a running session.
    Snapshot {
        #[arg(long, short)]
        out: Option<PathBuf>,
    },

    /// Replay deltas from the Iggy log between two sequence numbers.
    Replay {
        #[arg(long, default_value = "0")]
        from: u64,
        /// End sequence (0 = all available).
        #[arg(long, default_value = "0")]
        to: u64,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Show a summary diff between two sequence snapshots.
    Diff {
        #[arg(long)]
        seq_a: u64,
        #[arg(long)]
        seq_b: u64,
    },
}

#[derive(Subcommand, Debug)]
enum ServerCommands {
    /// Start a headless dedicated server (delegates to eustress-server binary).
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

    /// Watch the observation stream from a running server.
    Watch,
}

#[derive(Args, Debug)]
struct PublishArgs {
    /// Path to the Space directory to publish.
    #[arg(default_value = ".")]
    space_path: PathBuf,

    /// Target Cloudflare Wrangler environment.
    #[arg(long, default_value = "production")]
    env: String,

    /// Dry run — show what would be uploaded without uploading.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args, Debug)]
struct StatsArgs {
    /// Refresh interval in seconds (0 = print once).
    #[arg(long, default_value = "0")]
    watch: u64,
}

#[derive(Subcommand, Debug)]
enum SimCommands {
    /// Replay all SimRecord runs from Iggy history.
    /// Replaces the removed bincode+zstd file cache.
    Replay {
        /// Filter by scenario name (substring match).
        #[arg(long)]
        scenario: Option<String>,
        /// Maximum records to show (0 = all).
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Output raw JSON.
        #[arg(long)]
        json: bool,
    },

    /// Show the best iteration (highest similarity) from VIGA/workshop history.
    Best {
        /// Filter by session ID (hex string).
        #[arg(long)]
        session: Option<String>,
        /// Output raw JSON.
        #[arg(long)]
        json: bool,
    },

    /// Show workshop convergence curve for a product.
    Convergence {
        /// Filter by product name (substring match).
        #[arg(long)]
        product: Option<String>,
        /// Maximum generations to show (0 = all).
        #[arg(long, default_value = "50")]
        limit: u32,
        /// Output raw JSON.
        #[arg(long)]
        json: bool,
    },

    /// Show Rune script execution audit trail for a scenario.
    Scripts {
        /// Filter by scenario name (substring match).
        #[arg(long)]
        scenario: Option<String>,
        /// Maximum records to show (0 = all).
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Output raw JSON.
        #[arg(long)]
        json: bool,
    },

    /// Replay ARC-AGI-3 episode records from Iggy history.
    Arc {
        /// Filter by ARC task ID (exact or substring match).
        #[arg(long)]
        task: Option<String>,
        /// Maximum records to show (0 = all).
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Output raw JSON.
        #[arg(long)]
        json: bool,
    },

    /// Show the best (lowest efficiency_ratio) episode for a given ARC task.
    ArcBest {
        /// ARC task ID to query (e.g. "ls20").
        #[arg(long)]
        task: String,
        /// Output raw JSON.
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
        EnvFilter::new("debug,iggy=info,h2=warn,hyper=warn")
    } else {
        EnvFilter::new("warn")
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    match cli.command {
        Commands::Stream(args) => cmd_stream(&cli.iggy_url, args).await,
        Commands::Agent(args) => cmd_agent(&cli.iggy_url, args).await,
        Commands::Scene { action } => cmd_scene(&cli.iggy_url, action).await,
        Commands::Server { action } => cmd_server(&cli.iggy_url, action).await,
        Commands::Publish(args) => cmd_publish(args).await,
        Commands::Stats(args) => cmd_stats(&cli.iggy_url, args).await,
        Commands::Sim { action } => cmd_sim(&cli.iggy_url, action).await,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IggySession — shared Iggy client helper
// ─────────────────────────────────────────────────────────────────────────────

struct IggySession {
    client: IggyClient,
    stream_id: Identifier,
}

impl IggySession {
    async fn connect(url: &str) -> Result<Self> {
        let client = IggyClient::from_connection_string(url)
            .with_context(|| format!("Invalid Iggy URL: {url}"))?;

        client.connect().await.with_context(|| {
            format!(
                "Cannot reach Iggy server at {url}.\n\
                 Start it with: iggy-server\n\
                 Or set --iggy-url / $IGGY_URL."
            )
        })?;

        let stream_id = Identifier::named(IGGY_STREAM_NAME)
            .context("Invalid stream name")?;

        Ok(Self { client, stream_id })
    }

    fn topic_id(name: &str) -> Result<Identifier> {
        Identifier::named(name).with_context(|| format!("Invalid topic name: {name}"))
    }

    /// Poll up to `count` messages from `topic`, returning raw payloads.
    async fn poll_raw(
        &self,
        topic: &str,
        strategy: &PollingStrategy,
        count: u32,
    ) -> Result<Vec<Vec<u8>>> {
        let topic_id = Self::topic_id(topic)?;
        let consumer = Consumer::default();

        let polled = self
            .client
            .poll_messages(&self.stream_id, &topic_id, Some(1), &consumer, strategy, count, true)
            .await
            .with_context(|| format!("poll_messages failed for topic '{topic}'"))?;

        Ok(polled.messages.iter().map(|m| m.payload.to_vec()).collect())
    }

    /// Send one raw payload to `topic`.
    async fn send_raw(&self, topic: &str, payload: Vec<u8>) -> Result<()> {
        let topic_id = Self::topic_id(topic)?;
        let partitioning = Partitioning::balanced();
        let mut messages = vec![
            IggyMessage::builder()
                .payload(Bytes::from(payload))
                .build()
                .context("Build IggyMessage")?,
        ];

        self.client
            .send_messages(&self.stream_id, &topic_id, &partitioning, &mut messages)
            .await
            .with_context(|| format!("send_messages failed for topic '{topic}'"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_stream
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_stream(iggy_url: &str, args: StreamArgs) -> Result<()> {
    let session = IggySession::connect(iggy_url).await?;

    let strategy = match args.from_seq {
        Some(seq) => PollingStrategy::offset(seq),
        None => PollingStrategy::next(),
    };

    println!(
        "{}  Subscribed to {}  {}",
        "●".green(),
        format!("{IGGY_STREAM_NAME}/{IGGY_TOPIC_SCENE_DELTAS}").cyan(),
        "(Ctrl+C to stop)".dimmed()
    );

    let mut count: u64 = 0;

    loop {
        let payloads = session
            .poll_raw(IGGY_TOPIC_SCENE_DELTAS, &strategy, 256)
            .await
            .unwrap_or_default();

        for payload in payloads {
            match SceneDelta::from_bytes(&payload) {
                Ok(delta) => {
                    if !args.filter.is_empty() {
                        let kind_str = format!("{:?}", delta.kind);
                        if !args.filter.iter().any(|f| kind_str.contains(f.as_str())) {
                            continue;
                        }
                    }

                    if args.json {
                        if let Ok(json) = serde_json::to_string(&delta) {
                            println!("{json}");
                        }
                    } else {
                        print_delta(&delta);
                    }

                    count += 1;
                    if args.limit > 0 && count >= args.limit {
                        println!("\n{} Reached limit of {} deltas.", "✓".green(), args.limit);
                        return Ok(());
                    }
                }
                Err(e) => warn!("Bad delta: {e}"),
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn print_delta(d: &SceneDelta) {
    let kind_str = match d.kind {
        DeltaKind::PartAdded => format!("{}", "PartAdded     ".green().bold()),
        DeltaKind::PartRemoved => format!("{}", "PartRemoved   ".red().bold()),
        DeltaKind::TransformChanged => format!("{}", "Transform     ".yellow()),
        DeltaKind::PartPropertiesChanged => format!("{}", "Properties    ".blue()),
        DeltaKind::Renamed => format!("{}", "Renamed       ".cyan()),
        DeltaKind::Reparented => format!("{}", "Reparented    ".magenta()),
        DeltaKind::ScriptChanged => format!("{}", "Script        ".white()),
        DeltaKind::LightChanged => format!("{}", "Light         ".yellow()),
        DeltaKind::CameraChanged => format!("{}", "Camera        ".cyan()),
        DeltaKind::TerrainChunkChanged => format!("{}", "Terrain       ".green()),
        DeltaKind::BatchMarker => format!("{}", "Batch         ".dimmed()),
    };

    let detail = match d.kind {
        DeltaKind::TransformChanged => d.transform.as_ref().map(|t| {
            format!("pos=[{:.2},{:.2},{:.2}]", t.position[0], t.position[1], t.position[2])
                .white()
                .to_string()
        }).unwrap_or_default(),
        DeltaKind::Renamed => d.name.as_ref()
            .map(|n| format!("\"{}\"", n.name).cyan().to_string())
            .unwrap_or_default(),
        _ => String::new(),
    };

    println!(
        "{kind_str}  {}  {}  {}  {detail}",
        format!("entity={}", d.entity).dimmed(),
        format!("seq={}", d.seq).dimmed(),
        format!("+{}ms", d.timestamp_ms).dimmed(),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_agent
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_agent(iggy_url: &str, args: AgentArgs) -> Result<()> {
    let session = IggySession::connect(iggy_url).await?;

    let action = build_agent_action(&args)?;
    let script = if let Some(path) = &args.script_file {
        Some(tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Read script file: {}", path.display()))?)
    } else {
        args.script.clone()
    };

    let command_id = Uuid::new_v4().as_u128();
    let cmd = AgentCommand {
        command_id,
        action,
        script,
        issued_at_ms: unix_ms(),
    };

    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&cmd)
        .map_err(|e| anyhow::anyhow!("Serialize AgentCommand: {e}"))?
        .to_vec();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message(format!(
        "Sending command {:x} → {}",
        command_id,
        format!("{IGGY_STREAM_NAME}/{IGGY_TOPIC_AGENT_COMMANDS}").cyan()
    ));
    spinner.enable_steady_tick(Duration::from_millis(80));

    session
        .send_raw(IGGY_TOPIC_AGENT_COMMANDS, payload)
        .await
        .context("Publish AgentCommand to Iggy")?;

    spinner.set_message(format!(
        "Waiting for observation (timeout {}s)…",
        args.timeout_secs
    ));

    let result = timeout(
        Duration::from_secs(args.timeout_secs),
        poll_for_observation(&session, command_id),
    )
    .await;

    spinner.finish_and_clear();

    match result {
        Ok(Ok(obs)) => {
            print_observation(&obs, args.json);
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => {
            eprintln!(
                "{} Timed out after {}s. Ensure a Studio/Server session is running.",
                "✗".red(),
                args.timeout_secs
            );
            std::process::exit(1);
        }
    }
}

fn build_agent_action(args: &AgentArgs) -> Result<AgentAction> {
    if args.snapshot {
        return Ok(AgentAction::RequestSnapshot);
    }
    if let Some(pos) = &args.spawn_part {
        return Ok(AgentAction::SpawnPart {
            position: [pos[0], pos[1], pos[2]],
            class_name: args.class_name.clone(),
        });
    }
    if let Some(vals) = &args.set_transform {
        return Ok(AgentAction::SetTransform {
            entity: vals[0] as u64,
            position: [vals[1], vals[2], vals[3]],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
    }
    if let Some(ticks) = args.simulate_ticks {
        return Ok(AgentAction::SimulateNTicks { ticks });
    }
    if args.script.is_some() || args.script_file.is_some() {
        return Ok(AgentAction::ExecuteScript);
    }
    anyhow::bail!(
        "No action specified. Use --snapshot, --spawn-part, --set-transform, \
         --simulate-ticks, or --script."
    )
}

async fn poll_for_observation(
    session: &IggySession,
    command_id: u128,
) -> Result<AgentObservation> {
    let strategy = PollingStrategy::next();
    loop {
        let payloads = session
            .poll_raw(IGGY_TOPIC_AGENT_OBSERVATIONS, &strategy, 64)
            .await
            .unwrap_or_default();

        for payload in payloads {
            if let Ok(obs) = rkyv::from_bytes::<AgentObservation, rkyv::rancor::Error>(&payload) {
                if obs.command_id == command_id {
                    return Ok(obs);
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn print_observation(obs: &AgentObservation, as_json: bool) {
    if as_json {
        if let Ok(json) = serde_json::to_string_pretty(obs) {
            println!("{json}");
        }
        return;
    }

    println!("{}", "─".repeat(60).dimmed());
    println!(
        "{} Observation  cmd={:x}  seq={}  +{}ms",
        "◆".green().bold(),
        obs.command_id, obs.seq, obs.timestamp_ms
    );

    match &obs.payload {
        ObservationPayload::ScriptResult { output } => {
            println!("{} {}", "Result:".cyan().bold(), output);
        }
        ObservationPayload::ScriptError { message } => {
            println!("{} {}", "Error:".red().bold(), message);
        }
        ObservationPayload::SceneSnapshot { entity_count, tick, scene_id } => {
            println!("{} {}", "Scene:".cyan().bold(), scene_id);
            println!("  entities : {}", entity_count.to_string().yellow());
            println!("  tick     : {}", tick.to_string().yellow());
        }
        ObservationPayload::EntitySpawned { entity, class_name } => {
            println!("{} entity={} class={}", "Spawned:".green().bold(), entity, class_name.cyan());
        }
        ObservationPayload::EntityDespawned { entity } => {
            println!("{} entity={}", "Despawned:".red().bold(), entity);
        }
        ObservationPayload::SimulationAdvanced { ticks, elapsed_ms } => {
            println!("{} {} ticks in {}ms", "Simulated:".yellow().bold(), ticks, elapsed_ms);
        }
        ObservationPayload::Ack { message } => {
            println!("{} {}", "Ack:".green(), message);
        }
        ObservationPayload::Error { message } => {
            println!("{} {}", "Error:".red().bold(), message);
        }
        ObservationPayload::EnvironmentState { json, step, terminated } => {
            println!("{} step={} terminated={}", "EnvState:".cyan().bold(), step, terminated);
            if json.len() <= 200 {
                println!("  {json}");
            } else {
                println!("  {}…", &json[..200]);
            }
        }
    }

    println!("{}", "─".repeat(60).dimmed());
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_scene
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_scene(iggy_url: &str, action: SceneCommands) -> Result<()> {
    let session = IggySession::connect(iggy_url).await?;

    match action {
        SceneCommands::Snapshot { out } => {
            let command_id = Uuid::new_v4().as_u128();
            let cmd = AgentCommand {
                command_id,
                action: AgentAction::RequestSnapshot,
                script: None,
                issued_at_ms: unix_ms(),
            };
            let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&cmd)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .to_vec();
            session.send_raw(IGGY_TOPIC_AGENT_COMMANDS, payload).await?;

            let obs = timeout(
                Duration::from_secs(10),
                poll_for_observation(&session, command_id),
            )
            .await
            .context("Timed out")?
            .context("Snapshot error")?;

            if let ObservationPayload::SceneSnapshot { entity_count, tick, scene_id } = &obs.payload {
                let content = format!(
                    "# Eustress scene snapshot\nscene_id = \"{scene_id}\"\n\
                     entity_count = {entity_count}\ntick = {tick}\nsnapshot_at_ms = {}\n",
                    obs.timestamp_ms
                );
                match out {
                    Some(path) => {
                        tokio::fs::write(&path, &content)
                            .await
                            .with_context(|| format!("Write to {}", path.display()))?;
                        println!("{} Snapshot → {}", "✓".green(), path.display());
                    }
                    None => print!("{content}"),
                }
            } else {
                print_observation(&obs, false);
            }
        }

        SceneCommands::Replay { from, to, out } => {
            use eustress_common::toml_materializer::SceneMirror;

            println!(
                "Replaying deltas {} → {}…",
                from,
                if to == 0 { "end".to_string() } else { to.to_string() }
            );

            let strategy = PollingStrategy::offset(from);
            let count = if to > from { (to - from + 1) as u32 } else { u32::MAX };

            let payloads = session.poll_raw(IGGY_TOPIC_SCENE_DELTAS, &strategy, count).await?;

            let mut mirror = SceneMirror::new("replay".to_string());
            let mut replayed: u64 = 0;

            for payload in payloads {
                if let Ok(delta) = SceneDelta::from_bytes(&payload) {
                    if to > 0 && delta.seq > to { break; }
                    mirror.apply(&delta);
                    replayed += 1;
                }
            }

            let toml_str = mirror.to_toml_string().map_err(|e| anyhow::anyhow!("{e}"))?;

            match out {
                Some(path) => {
                    tokio::fs::write(&path, &toml_str)
                        .await
                        .with_context(|| format!("Write to {}", path.display()))?;
                    println!(
                        "{} Replayed {replayed} deltas → {} entities → {}",
                        "✓".green(),
                        mirror.entities.len(),
                        path.display()
                    );
                }
                None => print!("{toml_str}"),
            }
        }

        SceneCommands::Diff { seq_a, seq_b } => {
            use eustress_common::toml_materializer::SceneMirror;

            let strategy = PollingStrategy::offset(0);
            let payloads = session
                .poll_raw(IGGY_TOPIC_SCENE_DELTAS, &strategy, (seq_b + 1) as u32)
                .await?;

            let mut mirror_a = SceneMirror::new("diff-a".to_string());
            let mut mirror_b = SceneMirror::new("diff-b".to_string());

            for payload in &payloads {
                if let Ok(delta) = SceneDelta::from_bytes(payload) {
                    if delta.seq <= seq_a { mirror_a.apply(&delta); }
                    if delta.seq <= seq_b { mirror_b.apply(&delta); }
                }
            }

            let added: Vec<u64> = mirror_b.entities.keys()
                .filter(|k| !mirror_a.entities.contains_key(k))
                .copied().collect();
            let removed: Vec<u64> = mirror_a.entities.keys()
                .filter(|k| !mirror_b.entities.contains_key(k))
                .copied().collect();
            let moved: Vec<u64> = mirror_a.entities.iter()
                .filter_map(|(k, a)| mirror_b.entities.get(k)
                    .and_then(|b| if b.position != a.position { Some(*k) } else { None }))
                .collect();

            println!("{}", format!("Diff seq {seq_a} → {seq_b}").bold());
            println!("  {} added   : {:?}", "+".green(), added);
            println!("  {} removed : {:?}", "−".red(), removed);
            println!("  {} moved   : {:?}", "~".yellow(), moved);
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_server
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_server(iggy_url: &str, action: ServerCommands) -> Result<()> {
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
            println!("{} Watching observations…", "◆".cyan());
            let session = IggySession::connect(iggy_url).await?;
            let strategy = PollingStrategy::next();
            loop {
                let payloads = session
                    .poll_raw(IGGY_TOPIC_AGENT_OBSERVATIONS, &strategy, 64)
                    .await
                    .unwrap_or_default();
                for payload in payloads {
                    if let Ok(obs) = rkyv::from_bytes::<AgentObservation, rkyv::rancor::Error>(&payload) {
                        print_observation(&obs, false);
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
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
// cmd_stats
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_stats(iggy_url: &str, args: StatsArgs) -> Result<()> {
    let session = IggySession::connect(iggy_url).await?;

    let topics = [
        IGGY_TOPIC_SCENE_DELTAS,
        IGGY_TOPIC_AGENT_COMMANDS,
        IGGY_TOPIC_AGENT_OBSERVATIONS,
    ];

    loop {
        println!("\n{}", format!("Iggy stats — {iggy_url}").bold());
        println!("{}", "─".repeat(50).dimmed());

        for topic in &topics {
            let topic_id = IggySession::topic_id(topic)?;
            match session.client.get_topic(&session.stream_id, &topic_id).await {
                Ok(Some(t)) => {
                    let msg_count: u64 = t.partitions.iter().map(|p| p.messages_count).sum();
                    println!(
                        "  {:<32}  {} messages",
                        topic.cyan(),
                        msg_count.to_string().yellow()
                    );
                }
                Ok(None) => {
                    println!("  {:<32}  {}", topic.dimmed(), "(not found)".yellow());
                }
                Err(e) => {
                    println!("  {:<32}  {}", topic.dimmed(), format!("error: {e}").red());
                }
            }
        }

        if args.watch == 0 { break; }
        tokio::time::sleep(Duration::from_secs(args.watch)).await;
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────────────────────

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ─────────────────────────────────────────────────────────────────────────────
// cmd_sim — simulation history: replay / best / convergence / scripts
// ─────────────────────────────────────────────────────────────────────────────

async fn cmd_sim(iggy_url: &str, action: SimCommands) -> Result<()> {
    let config = SimStreamConfig {
        url: iggy_url.to_string(),
        ..Default::default()
    };

    let reader = SimStreamReader::connect(&config)
        .await
        .with_context(|| format!("Cannot connect SimStreamReader to {iggy_url}"))?;

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
            // Filter all iterations by session_id hex prefix before picking best.
            let best = if let Some(ref sess_hex) = session {
                let sess_hex = sess_hex.to_lowercase();
                let all = reader.replay_iterations(&query).await;
                all.into_iter()
                    .filter(|r| {
                        // session_id is u128 — compare as zero-padded 32-char hex, prefix match.
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
            println!(
                "  {:>4}  {:<28}  {:>8}  {:>8}  {}",
                "gen".dimmed(), "product".dimmed(), "fitness".dimmed(), "best?".dimmed(), "branch".dimmed()
            );
            for r in &records {
                println!(
                    "  {:>4}  {:<28}  {:>8}  {:>8}  {}",
                    r.generation,
                    r.product_name.cyan(),
                    format!("{:.3}", r.fitness).yellow(),
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

            // Build a scenario_id allow-set from sim_results when --scenario is given.
            // RuneScriptRecord.scenario_id matches SimRecord.scenario_id — join on that.
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
            println!(
                "  {:<32}  {:<8}  {:>6}  {:>8}  {:>10}  {:>10}",
                "episode_id".dimmed(), "task_id".dimmed(), "steps".dimmed(),
                "eff_ratio".dimmed(), "goal".dimmed(), "duration_ms".dimmed()
            );
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
