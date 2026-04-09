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

/// Bevy Resource holding the Forge connection state.
#[derive(Resource)]
pub struct ForgeState {
    /// The authenticated Forge client (None if not connected)
    client: Arc<Mutex<Option<ForgeClient>>>,
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
            status: ForgeConnectionStatus::Disconnected,
            error: None,
            deployment: None,
            url: "https://forge.eustress.dev".to_string(),
        }
    }
}

/// Plugin that registers ForgeState and the connection/allocation systems.
pub struct ForgePlugin;

impl Plugin for ForgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ForgeState>();
    }
}

/// Connect to the Forge API. Called from the drain handler when user clicks Connect.
/// Runs on a background thread since ForgeClient::new() and authenticate() are async.
pub fn connect_to_forge(
    forge_state: &mut ForgeState,
    url: &str,
    api_key: &str,
) {
    forge_state.status = ForgeConnectionStatus::Connecting;
    forge_state.error = None;
    forge_state.url = url.to_string();

    let client_arc = forge_state.client.clone();
    let url = url.to_string();
    let api_key = api_key.to_string();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();

        match rt {
            Ok(rt) => {
                rt.block_on(async {
                    match ForgeClient::new(&url).await {
                        Ok(mut client) => {
                            if !api_key.is_empty() {
                                if let Err(e) = client.authenticate(&api_key).await {
                                    tracing::error!("Forge auth failed: {}", e);
                                    return;
                                }
                            }
                            tracing::info!("Connected to Forge at {}", url);
                            *client_arc.lock() = Some(client);
                        }
                        Err(e) => {
                            tracing::error!("Forge connection failed: {}", e);
                        }
                    }
                });
            }
            Err(e) => tracing::error!("Failed to create tokio runtime for Forge: {}", e),
        }
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
                        }
                        Err(e) => {
                            tracing::error!("Forge deployment failed: {}", e);
                        }
                    }
                });
            }
            Err(e) => tracing::error!("Failed to create tokio runtime for Forge: {}", e),
        }
    });
}
