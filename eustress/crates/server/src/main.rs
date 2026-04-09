//! # Eustress Dedicated Server
//!
//! Headless server binary for hosting multiplayer games.
//!
//! ## Usage
//!
//! ```bash
//! # Start server with default settings
//! eustress-server
//!
//! # Start with custom port and scene
//! eustress-server --port 7777 --scene my_game.ron
//!
//! # Start with config file
//! eustress-server --config server.toml
//!
//! # Start with max players
//! eustress-server --max-players 100
//! ```
//!
//! ## Configuration (server.toml)
//!
//! ```toml
//! [server]
//! port = 7777
//! max_players = 100
//! tick_rate = 120
//! scene = "default.ron"
//!
//! [network]
//! timeout_ms = 30000
//! heartbeat_ms = 1000
//!
//! [physics]
//! gravity = [0.0, -35.0, 0.0]
//! max_entity_speed = 100.0
//! ```

use bevy::prelude::*;
use clap::Parser;
use std::io::Read;
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use eustress_common::services::{
    Workspace, PlayerService, DataStoreService, TeleportService, MarketplaceService,
};

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "eustress-server")]
#[command(about = "Eustress Engine Dedicated Server")]
#[command(version)]
struct Args {
    /// Server port
    #[arg(short, long, default_value = "7777")]
    port: u16,
    
    /// Maximum players
    #[arg(short, long, default_value = "100")]
    max_players: u32,
    
    /// Local scene/Universe folder to load
    #[arg(short, long)]
    scene: Option<PathBuf>,

    /// Simulation ID — downloads .pak from R2 on startup
    #[arg(long)]
    sim_id: Option<String>,

    /// API base URL for .pak download
    #[arg(long, default_value = "https://api.eustress.dev")]
    api_url: String,
    
    /// Configuration file
    #[arg(short, long)]
    config: Option<PathBuf>,
    
    /// Tick rate (Hz)
    #[arg(short, long, default_value = "120")]
    tick_rate: u32,
    
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
    
    /// Server region (for matchmaking)
    #[arg(long, default_value = "local")]
    region: String,
    
    /// Place ID (for teleport service)
    #[arg(long, default_value = "0")]
    place_id: u64,
}

// ============================================================================
// Server Configuration
// ============================================================================

#[derive(Debug, Clone, serde::Deserialize)]
struct ServerConfig {
    server: ServerSettings,
    network: NetworkSettings,
    physics: PhysicsSettings,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ServerSettings {
    port: u16,
    max_players: u32,
    tick_rate: u32,
    scene: Option<String>,
    region: String,
    place_id: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct NetworkSettings {
    timeout_ms: u64,
    heartbeat_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PhysicsSettings {
    gravity: [f32; 3],
    max_entity_speed: f32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSettings {
                port: 7777,
                max_players: 100,
                tick_rate: 120,
                scene: None,
                region: "local".to_string(),
                place_id: 0,
            },
            network: NetworkSettings {
                timeout_ms: 30000,
                heartbeat_ms: 1000,
            },
            physics: PhysicsSettings {
                gravity: [0.0, -35.0, 0.0],
                max_entity_speed: 100.0,
            },
        }
    }
}

// ============================================================================
// Server State
// ============================================================================

#[derive(Resource, Debug)]
struct ServerState {
    port: u16,
    max_players: u32,
    tick_rate: u32,
    region: String,
    place_id: u64,
    start_time: std::time::Instant,
    connected_players: u32,
}

impl ServerState {
    fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Parse CLI arguments
    let args = Args::parse();
    
    // Setup logging
    let filter = if args.verbose {
        EnvFilter::new("debug,wgpu=warn,naga=warn")
    } else {
        EnvFilter::new("info,wgpu=warn,naga=warn")
    };
    
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    info!("╔════════════════════════════════════════════════════════════╗");
    info!("║           Eustress Engine Dedicated Server                 ║");
    info!("╚════════════════════════════════════════════════════════════╝");
    
    // Load config file if provided
    let config = if let Some(config_path) = &args.config {
        match std::fs::read_to_string(config_path) {
            Ok(content) => {
                toml::from_str(&content).unwrap_or_else(|e| {
                    warn!("Failed to parse config: {}, using defaults", e);
                    ServerConfig::default()
                })
            }
            Err(e) => {
                warn!("Failed to read config file: {}, using defaults", e);
                ServerConfig::default()
            }
        }
    } else {
        ServerConfig::default()
    };
    
    // Merge CLI args with config (CLI takes precedence)
    let port = args.port;
    let max_players = args.max_players;
    let tick_rate = args.tick_rate;
    let region = args.region.clone();
    let place_id = args.place_id;
    
    // Resolve Universe folder — either local path or download .pak from R2
    let universe_root = if let Some(ref scene_path) = args.scene {
        info!("Loading local Universe from {:?}", scene_path);
        scene_path.clone()
    } else if let Some(ref sim_id) = args.sim_id {
        info!("Downloading Universe .pak for simulation {}...", sim_id);
        match download_and_extract_pak(sim_id, &args.api_url) {
            Ok(path) => {
                info!("Universe extracted to {:?}", path);
                path
            }
            Err(e) => {
                tracing::error!("Failed to download .pak: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        warn!("No scene or sim_id specified — running empty server");
        std::env::temp_dir().join("eustress-server-empty")
    };

    info!("Server configuration:");
    info!("  Universe: {:?}", universe_root);
    info!("  Port: {}", port);
    info!("  Max players: {}", max_players);
    info!("  Tick rate: {} Hz", tick_rate);
    info!("  Region: {}", region);
    info!("  Place ID: {}", place_id);

    // Create Bevy app (headless)
    App::new()
        // Minimal plugins (no rendering)
        .add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::scene::ScenePlugin)
        
        // Fixed timestep for physics
        .insert_resource(Time::<Fixed>::from_hz(tick_rate as f64))
        
        // Server state
        .insert_resource(ServerState {
            port,
            max_players,
            tick_rate,
            region: region.clone(),
            place_id,
            start_time: std::time::Instant::now(),
            connected_players: 0,
        })
        
        // Workspace with physics config
        .insert_resource(Workspace::default()
            .with_gravity(Vec3::from_array(config.physics.gravity))
            .with_speed_limits(config.physics.max_entity_speed, 200.0))
        
        // Player service
        .insert_resource(PlayerService::default())
        
        // Platform services
        .add_plugins(eustress_common::services::datastore::DataStorePlugin)
        .add_plugins(eustress_common::services::teleport::TeleportPlugin)
        .add_plugins(eustress_common::services::marketplace::MarketplacePlugin)
        
        // Networking (server mode — port configured via StartServer message)
        .add_plugins(eustress_networking::server::ServerNetworkPlugin)
        
        // Server systems
        .add_systems(Startup, setup_server)
        .add_systems(Update, (
            log_server_status,
            handle_shutdown,
        ))
        .add_systems(FixedUpdate, server_tick)
        
        .run();
}

// ============================================================================
// Systems
// ============================================================================

fn setup_server(
    state: Res<ServerState>,
    mut teleport_service: ResMut<TeleportService>,
) {
    info!("Server starting on port {}...", state.port);
    
    // Set current server info for teleport service
    teleport_service.current_server = Some(eustress_common::services::teleport::ServerInfo {
        server_id: format!("server-{}", uuid::Uuid::new_v4()),
        place_id: state.place_id,
        player_count: 0,
        max_players: state.max_players,
        region: state.region.clone(),
        tags: vec![],
        is_reserved: false,
        age_seconds: 0,
        ping_ms: None,
    });
    
    info!("Server ready! Listening for connections...");
}

fn log_server_status(
    state: Res<ServerState>,
    time: Res<Time>,
) {
    // Log status every 60 seconds
    static LAST_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    
    let now = state.uptime_secs();
    let last = LAST_LOG.load(std::sync::atomic::Ordering::Relaxed);
    
    if now >= last + 60 {
        LAST_LOG.store(now, std::sync::atomic::Ordering::Relaxed);
        
        info!(
            "Server status: {} players, uptime {}s, {:.1} TPS",
            state.connected_players,
            now,
            1.0 / time.delta_secs()
        );
    }
}

fn server_tick(
    // Add game logic systems here
) {
    // Physics, replication, etc. run in FixedUpdate
}

fn handle_shutdown(
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut exit: MessageWriter<AppExit>,
) {
    // Note: In headless mode, we'd use signals instead
    // This is a placeholder for graceful shutdown
    
    // Check for Ctrl+C via tokio signal handler in production
}

// ============================================================================
// .pak Download & Extraction
// ============================================================================

/// Download a Universe .pak from R2 via the API and extract it to a temp directory.
/// Returns the path to the extracted Universe folder.
fn download_and_extract_pak(sim_id: &str, api_url: &str) -> Result<PathBuf, String> {
    // Step 1: Get simulation metadata to find the R2 key
    let meta_resp = ureq::get(&format!("{}/api/simulations/{}", api_url, sim_id))
        .call()
        .map_err(|e| format!("Fetch simulation metadata: {}", e))?;
    let meta: serde_json::Value = meta_resp.into_json()
        .map_err(|e| format!("Parse metadata: {}", e))?;

    let r2_key = meta["r2_key"].as_str()
        .ok_or("Simulation has no published .pak (r2_key missing)")?;

    // Step 2: Download the .pak via the API download endpoint (handles auth for private sims)
    let pak_url = format!("{}/api/simulations/{}/download", api_url, sim_id);
    info!("Downloading .pak from {}...", pak_url);

    let pak_resp = ureq::get(&pak_url)
        .call()
        .map_err(|e| format!("Download .pak: {}", e))?;

    let mut pak_bytes = Vec::new();
    pak_resp.into_reader().read_to_end(&mut pak_bytes)
        .map_err(|e| format!("Read .pak bytes: {}", e))?;

    info!("Downloaded {:.1} MB .pak", pak_bytes.len() as f64 / 1_048_576.0);

    // Step 3: Decompress zstd
    let tar_bytes = zstd::decode_all(std::io::Cursor::new(&pak_bytes))
        .map_err(|e| format!("Zstd decompress: {}", e))?;

    // Step 4: Extract tar to temp directory
    let extract_dir = std::env::temp_dir()
        .join("eustress-server")
        .join(sim_id);

    // Clean previous extraction if exists
    if extract_dir.exists() {
        let _ = std::fs::remove_dir_all(&extract_dir);
    }
    std::fs::create_dir_all(&extract_dir)
        .map_err(|e| format!("Create extract dir: {}", e))?;

    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    archive.unpack(&extract_dir)
        .map_err(|e| format!("Extract tar: {}", e))?;

    // The Universe folder structure is preserved inside the tar
    // Return the extract directory as the Universe root
    Ok(extract_dir)
}

// ============================================================================
// Metrics (optional)
// ============================================================================

#[cfg(feature = "metrics")]
mod metrics {
    use prometheus::{Counter, Gauge, Registry};
    
    lazy_static::lazy_static! {
        pub static ref REGISTRY: Registry = Registry::new();
        pub static ref PLAYERS_CONNECTED: Gauge = Gauge::new(
            "eustress_players_connected", "Number of connected players"
        ).unwrap();
        pub static ref MESSAGES_RECEIVED: Counter = Counter::new(
            "eustress_messages_received", "Total network messages received"
        ).unwrap();
        pub static ref MESSAGES_SENT: Counter = Counter::new(
            "eustress_messages_sent", "Total network messages sent"
        ).unwrap();
    }
    
    pub fn init() {
        REGISTRY.register(Box::new(PLAYERS_CONNECTED.clone())).unwrap();
        REGISTRY.register(Box::new(MESSAGES_RECEIVED.clone())).unwrap();
        REGISTRY.register(Box::new(MESSAGES_SENT.clone())).unwrap();
    }
}
