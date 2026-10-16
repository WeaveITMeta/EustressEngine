//! `eustress-stream-node` binary — standalone TCP/REST streaming node.
//!
//! The `party` subcommands manage `parties.toml` and, on a party's own
//! machine, log in and print a token.
//!
//! # Usage
//!
//! Every listener is TLS 1.3 with the operator's certificate, and every
//! connection logs in, so a node needs `--tls-cert`, `--tls-key` and
//! `--parties` (or `EUSTRESS_TLS_CERT`, `EUSTRESS_TLS_KEY`, `EUSTRESS_PARTIES`).
//!
//! ```
//! # Single node on default port 33000 with REST on 43000
//! eustress-stream-node --tls-cert node.crt --tls-key private/node.key --parties private/parties.toml
//!
//! # Custom ports
//! eustress-stream-node --port 34000 --rest-port 44000
//!
//! # Forge cluster of 10 nodes starting at port 33000
//! eustress-stream-node --cluster --cluster-size 10 --port 33000
//!
//! # Override via env vars
//! EUSTRESS_PORT=33000 EUSTRESS_CLUSTER_SIZE=5 eustress-stream-node --cluster
//!
//! # Enroll, revoke and list parties (parties.toml is written owner-only)
//! eustress-stream-node party add north-plant --public-key <base64> \
//!     --subscribe scene_deltas --publish log/output --parties private/parties.toml
//! eustress-stream-node party revoke north-plant --parties private/parties.toml
//! eustress-stream-node party list --parties private/parties.toml
//!
//! # On a party's own machine: log in over REST and print a token
//! eustress-stream-node party token north-plant --node 127.0.0.1:43000 \
//!     --pin <SPKI SHA-256 hex> --identity eustress-north-plant.toml --scope browser
//! ```

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use clap::Parser;
use clap::{Subcommand, ValueEnum};
use eustress_identity::private_file;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use eustress_stream::StreamConfig;
use eustress_stream_node::{ForgeCluster, NodeConfig, StreamNode};
use eustress_stream_node::auth::{self, PartiesFile, PartyEntry};
use eustress_stream_node::client::{self, TokenRequest};
use eustress_stream_node::{SpkiPin, TokenClass};

// ─────────────────────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "eustress-stream-node",
    version,
    about = "EustressStream standalone node — TCP pub/sub + REST/SSE API",
    long_about = None,
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Interface to bind to. Defaults to loopback.
    ///
    /// Every connection logs in over TLS 1.3 and is held to its party's
    /// grants. A non-loopback bind additionally requires at least one enabled
    /// party; with none, the node refuses to start.
    #[arg(long, env = "EUSTRESS_BIND", default_value = "127.0.0.1")]
    bind: IpAddr,

    /// TCP port to listen on (auto-increments if occupied).
    #[arg(long, env = "EUSTRESS_PORT", default_value_t = 33000)]
    port: u16,

    /// REST/SSE HTTP port. Defaults to `port + 10000`.
    #[arg(long, env = "EUSTRESS_REST_PORT")]
    rest_port: Option<u16>,

    /// PEM certificate every listener presents. Required; the node never
    /// generates one.
    #[arg(long, env = "EUSTRESS_TLS_CERT")]
    tls_cert: Option<PathBuf>,

    /// PEM private key for the certificate. Required, and it must be
    /// readable by its owner (and SYSTEM) only.
    #[arg(long, env = "EUSTRESS_TLS_KEY")]
    tls_key: Option<PathBuf>,

    /// `parties.toml`: the parties enrolled to log in, with their grants.
    /// Required; it and its folder must be readable by their owner (and
    /// SYSTEM) only.
    #[arg(long, env = "EUSTRESS_PARTIES")]
    parties: Option<PathBuf>,

    /// Log filter (RUST_LOG syntax, e.g. "info,eustress_stream_node=debug").
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    log_level: String,

    /// Ring buffer capacity per topic (rounded up to next power of two).
    #[arg(long, env = "EUSTRESS_RING_CAP", default_value_t = 65536)]
    ring_cap: usize,

    /// Launch a ForgeCluster of N nodes instead of a single node.
    #[arg(long)]
    cluster: bool,

    /// Number of nodes in the ForgeCluster (requires --cluster).
    #[arg(long, env = "EUSTRESS_CLUSTER_SIZE", default_value_t = 10)]
    cluster_size: usize,

    /// Maximum frame size in bytes (default: 16 MiB).
    #[arg(long, default_value_t = 16 * 1024 * 1024)]
    frame_max: usize,

    /// Per-connection outbound channel capacity (messages).
    #[arg(long, default_value_t = 8192)]
    conn_cap: usize,

    /// Disable the REST/SSE server.
    #[arg(long)]
    no_rest: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&args.log_level))
        .with_target(false)
        .compact()
        .init();

    if let Some(Command::Party { action }) = args.command {
        run_party(action).await;
        return;
    }

    if args.cluster {
        run_cluster(&args).await;
    } else {
        run_single(&args).await;
    }
}

async fn run_single(args: &Args) {
    let stream_config = StreamConfig::default()
        .in_memory()
        .with_ring_capacity(args.ring_cap);

    let config = NodeConfig {
        bind_addr: args.bind,
        port: args.port,
        rest_port: args.rest_port,
        frame_max_bytes: args.frame_max,
        connection_channel_capacity: args.conn_cap,
        stream_config,
        tls_cert: args.tls_cert.clone(),
        tls_key: args.tls_key.clone(),
        parties_path: args.parties.clone(),
        ..NodeConfig::default()
    };

    let node = match StreamNode::start(config).await {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to start node: {e}");
            std::process::exit(1);
        }
    };

    info!("EustressStream node online — TCP {}", node.listen_addr());
    info!("TLS 1.3 on every listener. Clients pin SPKI SHA-256 {}", node.spki_pin());

    if !args.no_rest {
        node.start_rest();
        let rest_port = node.listen_addr().port() + 10000;
        let effective_rest = args.rest_port.unwrap_or(rest_port);
        info!("REST/SSE API on https://{}:{effective_rest} (bearer tokens)", args.bind);
        info!("  POST /v1/login/challenge, POST /v1/login   (no token)");
        info!("  POST /topics/{{name}}/publish");
        info!("  GET  /topics/{{name}}/stream   (SSE live feed)");
        info!("  GET  /topics/{{name}}/replay   (ring buffer replay)");
        info!("  GET  /topics");
        info!("  GET  /health");
    }

    wait_for_shutdown().await;
    node.shutdown();
    info!("Node shut down cleanly.");
}

async fn run_cluster(args: &Args) {
    let stream_config = StreamConfig::default()
        .in_memory()
        .with_ring_capacity(args.ring_cap);

    let base_config = NodeConfig {
        bind_addr: args.bind,
        rest_port: args.rest_port,
        frame_max_bytes: args.frame_max,
        connection_channel_capacity: args.conn_cap,
        stream_config,
        tls_cert: args.tls_cert.clone(),
        tls_key: args.tls_key.clone(),
        parties_path: args.parties.clone(),
        ..NodeConfig::default()
    };

    let cluster = match ForgeCluster::start(args.port, args.cluster_size, base_config).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to start cluster: {e}");
            std::process::exit(1);
        }
    };

    let addrs = cluster.node_addrs();
    info!(
        "ForgeCluster online — {} nodes on ports {}–{}",
        cluster.node_count(),
        addrs.first().map(|a| a.port()).unwrap_or(0),
        addrs.last().map(|a| a.port()).unwrap_or(0),
    );
    for addr in &addrs {
        info!("  node TCP {addr}");
    }

    let stats = cluster.aggregate_stats();
    info!(
        "Cluster ready: {} nodes, {} topics, {} subscribers",
        stats.node_count, stats.total_topics, stats.total_subscribers
    );

    wait_for_shutdown().await;
    cluster.shutdown();
    info!("Cluster shut down cleanly.");
}

// ─────────────────────────────────────────────────────────────────────────────
// party subcommands
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
enum Command {
    /// Enroll, revoke and list parties, or log in as a party and print a token.
    Party {
        #[command(subcommand)]
        action: PartyCommand,
    },
}

#[derive(Subcommand, Debug)]
enum PartyCommand {
    /// Enroll a party by the Ed25519 public key in its identity file.
    Add {
        /// Party id: 1 to 64 characters of A-Z, a-z, 0-9, _ and -.
        id: String,
        /// The party's Ed25519 public key: base64, or 64 hex digits.
        #[arg(long)]
        public_key: String,
        /// A topic the party may subscribe to. Repeat for more.
        #[arg(long = "subscribe")]
        subscribe: Vec<String>,
        /// A topic the party may publish to. Repeat for more. The reserved
        /// topics scene_deltas and agent_commands also need --admin.
        #[arg(long = "publish")]
        publish: Vec<String>,
        /// Allow publishing to the reserved topics listed with --publish.
        #[arg(long)]
        admin: bool,
        /// Simultaneous subscriptions this party may hold on the node.
        #[arg(long, default_value_t = auth::DEFAULT_MAX_SUBSCRIPTIONS)]
        max_subscriptions: usize,
        /// Enroll the party disabled.
        #[arg(long)]
        disabled: bool,
        /// The parties.toml to edit. Created owner-only if it does not exist.
        #[arg(long, env = "EUSTRESS_PARTIES")]
        parties: PathBuf,
    },
    /// Revoke a party (enabled = false). A running node drops its live
    /// connections and tokens within a second.
    Revoke {
        /// The party id to revoke.
        id: String,
        /// The parties.toml to edit.
        #[arg(long, env = "EUSTRESS_PARTIES")]
        parties: PathBuf,
    },
    /// List the enrolled parties and their grants.
    List {
        /// The parties.toml to read.
        #[arg(long, env = "EUSTRESS_PARTIES")]
        parties: PathBuf,
    },
    /// On a party's own machine: log in to a node's REST port and print a
    /// token and its expiry.
    Token {
        /// The party to log in as.
        party_id: String,
        /// The node's REST address, host:port.
        #[arg(long)]
        node: String,
        /// SHA-256 of the node certificate's SubjectPublicKeyInfo, 64 hex
        /// digits. Any other certificate is refused.
        #[arg(long)]
        pin: String,
        /// The party's identity file. It must be readable by its owner only;
        /// a file that fails that check is not read.
        #[arg(long)]
        identity: PathBuf,
        /// browser (15 minutes) or session (one hour).
        #[arg(long, value_enum, default_value = "browser")]
        scope: Scope,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Scope {
    Browser,
    Session,
}

async fn run_party(action: PartyCommand) {
    let result = match action {
        PartyCommand::Add { id, public_key, subscribe, publish, admin, max_subscriptions, disabled, parties } => {
            party_add(&parties, id, &public_key, subscribe, publish, admin, max_subscriptions, !disabled)
        }
        PartyCommand::Revoke { id, parties } => party_revoke(&parties, &id),
        PartyCommand::List { parties } => party_list(&parties),
        PartyCommand::Token { party_id, node, pin, identity, scope } => {
            party_token(party_id, node, &pin, identity, scope).await
        }
    };
    if let Err(message) = result {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

/// Read `parties.toml` for editing. An existing file is opened with the
/// owner-only check (`verify_open`) and read through the handle that check
/// returned, so an edit never takes in entries another account wrote. A
/// missing file reads as empty when `create` is set.
fn read_parties_for_edit(path: &Path, create: bool) -> Result<PartiesFile, String> {
    use std::io::Read as _;
    let mut checked = match private_file::verify_open(path) {
        Ok(checked) => checked,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return if create { Ok(PartiesFile::default()) } else { Err(format!("cannot read {}: {e}", path.display())) };
        }
        Err(e) => return Err(format!("refusing to use {}: {}", path.display(), auth::owner_only_refusal(path, &e))),
    };
    let mut text = String::new();
    checked.read_to_string(&mut text).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    PartiesFile::parse(&text).map_err(|e| format!("{} does not parse: {e}", path.display()))
}

/// Write `parties.toml` owner-only, through a temporary file renamed into
/// place.
fn write_parties(path: &Path, file: &PartiesFile) -> Result<(), String> {
    let body = file.to_toml()?;
    let text = format!(
        "# Parties enrolled on this stream node. Edit with `eustress-stream-node party`.\n\
         # A party logs in only with `enabled = true`, and may publish and subscribe\n\
         # only to the topics listed for it. scene_deltas and agent_commands also\n\
         # need `admin = true` to publish.\n\n{body}"
    );
    private_file::write_private(path, text.as_bytes()).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// A public key given as base64 or 64 hex digits, as canonical base64.
fn normalize_public_key(text: &str) -> Result<String, String> {
    let text = text.trim();
    let bytes: Vec<u8> = if text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        (0..32)
            .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16))
            .collect::<Result<_, _>>()
            .map_err(|_| "the public key is not valid hex".to_string())?
    } else {
        STANDARD.decode(text.as_bytes()).map_err(|_| "the public key is neither base64 nor 64 hex digits".to_string())?
    };
    let b64 = STANDARD.encode(&bytes);
    eustress_identity::keypair::decode_public_key(&b64)
        .map_err(|_| "the public key is not an Ed25519 public key".to_string())?;
    Ok(b64)
}

#[allow(clippy::too_many_arguments)]
fn party_add(
    path: &Path,
    id: String,
    public_key: &str,
    subscribe: Vec<String>,
    publish: Vec<String>,
    admin: bool,
    max_subscriptions: usize,
    enabled: bool,
) -> Result<(), String> {
    if !auth::is_valid_party_id(&id) {
        return Err("a party id is 1 to 64 characters of A-Z, a-z, 0-9, _ and -".to_string());
    }
    let public_key = normalize_public_key(public_key)?;
    let mut file = read_parties_for_edit(path, true)?;
    if file.parties.iter().any(|p| p.id == id) {
        return Err(format!("party {id} is already enrolled in {}", path.display()));
    }
    for topic in publish.iter().filter(|t| auth::is_reserved_topic(t)) {
        if !admin {
            eprintln!("note: {topic} is reserved; publishing to it also needs --admin");
        }
    }
    file.parties.push(PartyEntry { id: id.clone(), public_key, enabled, admin, max_subscriptions, subscribe, publish });
    write_parties(path, &file)?;
    println!("enrolled {id} in {}{}", path.display(), if enabled { "" } else { " (disabled)" });
    Ok(())
}

fn party_revoke(path: &Path, id: &str) -> Result<(), String> {
    let mut file = read_parties_for_edit(path, false)?;
    let Some(entry) = file.find_mut(id) else {
        return Err(format!("party {id} is not enrolled in {}", path.display()));
    };
    entry.enabled = false;
    write_parties(path, &file)?;
    println!("revoked {id}: a running node drops its live connections and tokens within a second");
    Ok(())
}

fn party_list(path: &Path) -> Result<(), String> {
    let file = read_parties_for_edit(path, false)?;
    if file.parties.is_empty() {
        println!("no parties enrolled in {}", path.display());
        return Ok(());
    }
    for p in &file.parties {
        println!(
            "{}  enabled={}  admin={}  max_subscriptions={}  subscribe=[{}]  publish=[{}]",
            p.id,
            p.enabled,
            p.admin,
            p.max_subscriptions,
            p.subscribe.join(", "),
            p.publish.join(", "),
        );
    }
    Ok(())
}

/// Log in over REST as `party_id` and print a token. It follows the retry
/// rule: a 429 or 503 is retried, at most three attempts in all.
async fn party_token(party_id: String, node: String, pin: &str, identity: PathBuf, scope: Scope) -> Result<(), String> {
    let pin = SpkiPin::from_hex(pin).map_err(|e| e.to_string())?;
    let class = match scope {
        Scope::Browser => TokenClass::Browser,
        Scope::Session => TokenClass::Session,
    };
    let request = TokenRequest { party_id, node, pin, identity, class };
    let issued = client::request_token(&request).await.map_err(|e| e.to_string())?;
    let expires_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|now| (now + issued.expires_in).as_secs())
        .unwrap_or(0);
    println!("token: {}", issued.token);
    println!("token_class: {}", issued.class.as_str());
    println!("expires_in_seconds: {}", issued.expires_in.as_secs());
    println!("expires_at_unix: {expires_at_unix}");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Graceful shutdown
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
async fn wait_for_shutdown() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("Received SIGINT"),
        _ = sigterm.recv() => info!("Received SIGTERM"),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    info!("Received Ctrl+C");
}
