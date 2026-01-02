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
use std::path::PathBuf;
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
    
    /// Scene file to load
    #[arg(short, long)]
    scene: Option<PathBuf>,
    
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
    
    info!("Server configuration:");
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
        
        // Runtime (character, ownership)
        // .add_plugins(eustress_runtime::EustressRuntimePlugin)
        
        // Networking (server mode)
        // .add_plugins(eustress_networking::ServerNetworkPlugin::new(port))
        
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
    mut exit: EventWriter<AppExit>,
) {
    // Note: In headless mode, we'd use signals instead
    // This is a placeholder for graceful shutdown
    
    // Check for Ctrl+C via tokio signal handler in production
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
