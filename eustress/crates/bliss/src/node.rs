//! Bliss Node — Light (default) or Full (opt-in) participation.
//!
//! Every EustressEngine instance runs a Light Node by default. The node
//! handles contribution tracking, co-sign requests to the witness Worker,
//! and BLS balance management. Users can upgrade to Full Node for +10%
//! BLS bonus and block production.

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::cosign::CosignClient;
use crate::error::BlissError;

// ---------------------------------------------------------------------------
// Node Mode
// ---------------------------------------------------------------------------

/// Active node mode — determines BLS earning multiplier and capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeMode {
    /// Light node: validates own transactions, relays events.
    /// ~200MB RAM, no bonus multiplier (1.0x). Runs by default.
    Light,
    /// Full node: stores blockchain, produces blocks, serves peers.
    /// ~2GB RAM, +10% bonus (1.1x). Requires opt-in and stable internet.
    Full,
}

impl NodeMode {
    /// BLS contribution bonus multiplier.
    pub fn bonus_multiplier(&self) -> f64 {
        match self {
            NodeMode::Light => 1.0,
            NodeMode::Full => 1.1,
        }
    }

    /// Display name for UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            NodeMode::Light => "Light Node",
            NodeMode::Full => "Full Node",
        }
    }

    /// Short description for settings panel.
    pub fn description(&self) -> &'static str {
        match self {
            NodeMode::Light => "Earn by creating — zero setup. Validates your own transactions and relays events.",
            NodeMode::Full => "Strengthen the network, earn +10% more BLS. Stores blockchain data and produces blocks.",
        }
    }

    /// Resource disclosure text (shown to user before enabling).
    pub fn disclosure(&self) -> &'static str {
        match self {
            NodeMode::Light => "Your device will validate your own transactions and relay data to support the Bliss network. Uses ~200MB RAM and minimal bandwidth.",
            NodeMode::Full => "Your device will store the full blockchain and may produce blocks. Uses ~2GB RAM and requires good internet. You'll earn a 10% contribution bonus.",
        }
    }

    /// Estimated RAM usage in MB.
    pub fn ram_mb(&self) -> u32 {
        match self {
            NodeMode::Light => 200,
            NodeMode::Full => 2048,
        }
    }

    /// Whether this mode requires stable internet.
    pub fn requires_stable_internet(&self) -> bool {
        matches!(self, NodeMode::Full)
    }
}

impl Default for NodeMode {
    fn default() -> Self {
        NodeMode::Light
    }
}

impl std::fmt::Display for NodeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ---------------------------------------------------------------------------
// Node Config
// ---------------------------------------------------------------------------

/// Configuration for the Bliss node running inside EustressEngine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Active node mode (Light or Full).
    pub mode: NodeMode,
    /// Witness Worker URL for co-signing requests.
    pub witness_url: String,
    /// Fork ID this engine is participating in.
    pub fork_id: String,
    /// Path to identity.toml (optional — loaded at startup if present).
    pub identity_path: Option<String>,
    /// Whether the node is enabled (user can disable BLS entirely).
    pub enabled: bool,
    /// API server port (default: 7777).
    pub api_port: u16,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            mode: NodeMode::Light,
            witness_url: "https://eustress.dev".to_string(),
            fork_id: "eustress.dev".to_string(),
            identity_path: None,
            enabled: true,
            api_port: 7777,
        }
    }
}

// ---------------------------------------------------------------------------
// Node State
// ---------------------------------------------------------------------------

/// Runtime state of the Bliss node.
#[derive(Debug)]
struct NodeState {
    /// Current BLS balance (from last known distribution).
    balance: u64,
    /// Pending contribution score (accumulates until next daily payout).
    pending_score: f64,
    /// Number of co-signed contributions this session.
    session_cosigns: u32,
    /// Whether identity is loaded and ready.
    identity_loaded: bool,
    /// Public key (from identity.toml) if loaded.
    public_key: Option<String>,
}

impl Default for NodeState {
    fn default() -> Self {
        Self {
            balance: 0,
            pending_score: 0.0,
            session_cosigns: 0,
            identity_loaded: false,
            public_key: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Bliss Node
// ---------------------------------------------------------------------------

/// The Bliss node that runs inside every EustressEngine instance.
///
/// Light mode (default): contribution tracking + co-sign requests.
/// Full mode (opt-in): adds block production + peer serving for +10% bonus.
pub struct BlissNode {
    config: NodeConfig,
    state: Arc<RwLock<NodeState>>,
    cosign_client: CosignClient,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl BlissNode {
    /// Create a new Bliss node with the given configuration.
    pub fn new(config: NodeConfig) -> Self {
        let cosign_client = CosignClient::new(
            config.witness_url.clone(),
            config.fork_id.clone(),
        );

        Self {
            config,
            state: Arc::new(RwLock::new(NodeState::default())),
            cosign_client,
            server_handle: None,
        }
    }

    /// Create with default configuration (Light node on eustress.dev).
    pub fn light() -> Self {
        Self::new(NodeConfig::default())
    }

    /// Current node mode.
    pub fn mode(&self) -> NodeMode {
        self.config.mode
    }

    /// Switch node mode. Returns the disclosure text for the new mode
    /// (should be shown to the user for consent).
    pub fn set_mode(&mut self, mode: NodeMode) -> &'static str {
        self.config.mode = mode;
        mode.disclosure()
    }

    /// Whether the node is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Enable or disable the node.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    /// Get the current configuration.
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Load identity from a TOML file. Extracts the public key for
    /// co-sign requests.
    pub async fn load_identity(&self, toml_content: &str) -> Result<String, BlissError> {
        let mut public_key = String::new();

        for line in toml_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("public_key") {
                if let Some(val) = extract_toml_value(trimmed) {
                    public_key = val;
                }
            }
        }

        if public_key.is_empty() {
            return Err(BlissError::NoIdentity);
        }

        let mut state = self.state.write().await;
        state.identity_loaded = true;
        state.public_key = Some(public_key.clone());

        Ok(public_key)
    }

    /// Submit a contribution for co-signing. Returns the contribution hash
    /// and server co-signature on success.
    ///
    /// The contribution score is weighted by the node's bonus multiplier:
    /// Light = 1.0x, Full = 1.1x.
    pub async fn submit_contribution(
        &self,
        user_id: &str,
        contribution_hash: &str,
        contribution_type: &str,
        duration_secs: u64,
    ) -> Result<CosignResult, BlissError> {
        if !self.config.enabled {
            return Err(BlissError::Node("Node is disabled".to_string()));
        }

        let state = self.state.read().await;
        if !state.identity_loaded {
            return Err(BlissError::NoIdentity);
        }
        drop(state);

        // Request co-signature from witness Worker
        let result = self
            .cosign_client
            .cosign(user_id, contribution_hash, contribution_type, duration_secs)
            .await?;

        // Update local state
        let mut state = self.state.write().await;
        state.session_cosigns += 1;
        let weight = match contribution_type {
            "Development" => 3.0, "Creation" => 2.5, "Education" => 2.2,
            "Collaboration" => 2.0, "Optimization" => 2.0, "QualityAssurance" => 1.8,
            "Moderation" => 1.5, "Documentation" => 1.5, _ => 1.0,
        };
        state.pending_score += weight * self.config.mode.bonus_multiplier();

        Ok(result)
    }

    /// Get current session statistics.
    pub async fn session_stats(&self) -> SessionStats {
        let state = self.state.read().await;
        SessionStats {
            mode: self.config.mode,
            enabled: self.config.enabled,
            balance: state.balance,
            pending_score: state.pending_score,
            session_cosigns: state.session_cosigns,
            identity_loaded: state.identity_loaded,
            bonus_multiplier: self.config.mode.bonus_multiplier(),
        }
    }

    /// Update balance (called after daily distribution payout).
    pub async fn set_balance(&self, balance: u64) {
        let mut state = self.state.write().await;
        state.balance = balance;
    }

    /// Start the node API server for co-signing and identity verification.
    ///
    /// Registration happens on Cloudflare (api.eustress.dev). This node
    /// verifies identity.toml signatures locally and co-signs contributions.
    pub async fn start(&mut self) -> Result<u16, BlissError> {
        if !self.config.enabled {
            return Err(BlissError::Node("Node is disabled".to_string()));
        }

        let port = self.config.api_port;
        let mode = self.config.mode;
        let fork_id = self.config.fork_id.clone();
        let witness_url = self.config.witness_url.clone();

        let handle = crate::api::start_server(port, mode, fork_id, witness_url).await?;
        self.server_handle = Some(handle);

        tracing::info!("Bliss {} started on port {}", mode, port);
        Ok(port)
    }

    /// Whether the API server is running.
    pub fn is_running(&self) -> bool {
        self.server_handle
            .as_ref()
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }
}

/// Result of a successful co-sign request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CosignResult {
    pub server_signature: String,
    pub co_signed_at: String,
}

/// Snapshot of current session statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub mode: NodeMode,
    pub enabled: bool,
    pub balance: u64,
    pub pending_score: f64,
    pub session_cosigns: u32,
    pub identity_loaded: bool,
    pub bonus_multiplier: f64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a value from a TOML line like `key = "value"`.
fn extract_toml_value(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() == 2 {
        let val = parts[1].trim().trim_matches('"').trim_matches('\'');
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }
    None
}
