//! # Forge Integration
//!
//! Connects the Engine to Forge cloud orchestration for dedicated server management.
//!
//! ## Flow
//!
//! 1. User opens Forge Connect dialog → enters URL + API key → clicks Connect
//! 2. `connect_to_forge` authenticates with Forge API via ForgeClient
//! 3. `AllocateForgeServer` calls `deploy_experience()` to spin up a Nomad job
//! 4. Nomad downloads the .pak from R2, starts eustress-server, registers heartbeat
//! 5. Player connects via QUIC to the allocated server

use bevy::prelude::*;
use std::sync::Arc;
use parking_lot::Mutex;
use eustress_forge_sdk::{
    client::ForgeClient,
    deployment::{DeploymentSpec, DeploymentInfo, DeploymentStatus},
    types::Region,
};

/// What a background Forge call reports back to the main thread.
enum ForgeOutcome {
    Connected,
    ConnectFailed(String),
    Deployed(DeploymentInfo),
    DeployFailed(String),
}

/// Bevy Resource holding the Forge connection state.
#[derive(Resource)]
pub struct ForgeState {
    /// The authenticated Forge client (None if not connected)
    client: Arc<Mutex<Option<ForgeClient>>>,
    /// Results posted by the connect / deploy threads, applied to this
    /// state by `apply_forge_outcomes` on the main thread.
    outcomes: Arc<Mutex<Vec<ForgeOutcome>>>,
    /// Current connection status
    pub status: ForgeConnectionStatus,
    /// Last error message
    pub error: Option<String>,
    /// Active deployment info
    pub deployment: Option<DeploymentInfo>,
    /// Forge API URL
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForgeConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

impl Default for ForgeState {
    fn default() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            outcomes: Arc::new(Mutex::new(Vec::new())),
            status: ForgeConnectionStatus::Disconnected,
            error: None,
            deployment: None,
            url: "https://forge.eustress.dev".to_string(),
        }
    }
}

impl ForgeState {
    /// Drop the client and forget the deployment.
    pub fn disconnect(&mut self) {
        *self.client.lock() = None;
        self.status = ForgeConnectionStatus::Disconnected;
        self.error = None;
        self.deployment = None;
    }
}

/// Plugin that registers ForgeState and the connection/allocation systems.
pub struct ForgePlugin;

impl Plugin for ForgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ForgeState>()
            .add_systems(Update, apply_forge_outcomes);
    }
}

/// Apply what the background threads reported: connection status, errors,
/// and allocated deployments, each echoed to the Output console.
fn apply_forge_outcomes(
    mut forge: ResMut<ForgeState>,
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
) {
    let outcomes = std::mem::take(&mut *forge.outcomes.lock());
    for outcome in outcomes {
        match outcome {
            ForgeOutcome::Connected => {
                forge.status = ForgeConnectionStatus::Connected;
                forge.error = None;
                if let Some(out) = output.as_mut() {
                    out.info(format!("Connected to Forge at {}", forge.url));
                }
            }
            ForgeOutcome::ConnectFailed(e) => {
                forge.status = ForgeConnectionStatus::Failed;
                if let Some(out) = output.as_mut() {
                    out.error(format!("Forge connection failed: {e}"));
                }
                forge.error = Some(e);
            }
            ForgeOutcome::Deployed(info) => {
                if let Some(out) = output.as_mut() {
                    out.info(format!("Forge server allocated: deployment {} ({})", info.id, info.status));
                }
                forge.deployment = Some(info);
            }
            ForgeOutcome::DeployFailed(e) => {
                if let Some(out) = output.as_mut() {
                    out.error(format!("Forge server allocation failed: {e}"));
                }
                forge.error = Some(e);
            }
        }
    }
}

/// Connect to the Forge API. Called from the drain handler when user clicks Connect.
/// Runs on a background thread since ForgeClient::new() and authenticate() are async.
pub fn connect_to_forge(
    forge_state: &mut ForgeState,
    url: &str,
    api_key: &str,
) {
    let url = url.trim().to_string();
    forge_state.url = url.clone();
    forge_state.error = None;
    if url.is_empty() {
        forge_state.status = ForgeConnectionStatus::Failed;
        forge_state.error = Some("Enter the Forge server URL.".to_string());
        return;
    }
    forge_state.status = ForgeConnectionStatus::Connecting;

    let client_arc = forge_state.client.clone();
    let outcomes = forge_state.outcomes.clone();
    let api_key = api_key.to_string();

    std::thread::spawn(move || {
        let outcome = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt.block_on(async {
                match ForgeClient::new(&url).await {
                    Ok(mut client) => {
                        if !api_key.is_empty() {
                            if let Err(e) = client.authenticate(&api_key).await {
                                tracing::error!("Forge auth failed: {}", e);
                                return ForgeOutcome::ConnectFailed(format!("authentication failed: {e}"));
                            }
                        }
                        tracing::info!("Connected to Forge at {}", url);
                        *client_arc.lock() = Some(client);
                        ForgeOutcome::Connected
                    }
                    Err(e) => {
                        tracing::error!("Forge connection failed: {}", e);
                        ForgeOutcome::ConnectFailed(format!("could not reach {url}: {e}"))
                    }
                }
            }),
            Err(e) => {
                tracing::error!("Failed to create tokio runtime for Forge: {}", e);
                ForgeOutcome::ConnectFailed(format!("could not start the network runtime: {e}"))
            }
        };
        outcomes.lock().push(outcome);
    });
}

/// Allocate a dedicated server for a simulation via Forge.
/// Returns immediately — the deployment status is polled via ForgeState.
pub fn allocate_server(
    forge_state: &mut ForgeState,
    sim_id: &str,
    max_players: u32,
    region: Region,
) {
    let client_arc = forge_state.client.clone();
    let outcomes = forge_state.outcomes.clone();
    let sim_id = sim_id.to_string();

    let client_guard = client_arc.lock();
    if client_guard.is_none() {
        forge_state.error = Some("Not connected to Forge".to_string());
        tracing::warn!("Cannot allocate server — not connected to Forge");
        return;
    }
    drop(client_guard);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();

        match rt {
            Ok(rt) => {
                rt.block_on(async {
                    let guard = client_arc.lock();
                    let Some(ref client) = *guard else { return };

                    let spec = DeploymentSpec {
                        experience_id: sim_id.clone(),
                        version: "latest".to_string(),
                        regions: vec![region],
                        min_servers: 1,
                        max_servers: 5,
                        max_players_per_server: Some(max_players),
                        env: None,
                    };

                    match client.deploy_experience(spec).await {
                        Ok(info) => {
                            tracing::info!(
                                "Forge deployment created: {} (status: {})",
                                info.id, info.status
                            );
                            outcomes.lock().push(ForgeOutcome::Deployed(info));
                        }
                        Err(e) => {
                            tracing::error!("Forge deployment failed: {}", e);
                            outcomes.lock().push(ForgeOutcome::DeployFailed(e.to_string()));
                        }
                    }
                });
            }
            Err(e) => {
                tracing::error!("Failed to create tokio runtime for Forge: {}", e);
                outcomes.lock().push(ForgeOutcome::DeployFailed(format!("could not start the network runtime: {e}")));
            }
        }
    });
}
