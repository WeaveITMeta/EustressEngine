//! # API Extensions (Studio Plugins)
//!
//! Stub module for API extensions in studio plugins.

use bevy::prelude::*;

/// API extensions plugin placeholder
pub struct ApiExtensionsPlugin;

impl Plugin for ApiExtensionsPlugin {
    fn build(&self, _app: &mut App) {
        // TODO: Implement API extensions
    }
}

/// Spatial Vortex URL
pub const SPATIAL_VORTEX_URL: &str = "https://vortex.eustress.io";

/// List all available spaces
pub fn list_all_spaces() -> Vec<String> {
    Vec::new()
}

/// Get space count
pub fn get_space_count() -> usize {
    0
}

/// Check if space is available
pub fn is_space_available(_name: &str) -> bool {
    false
}

/// Register a space
pub fn register_space(_name: &str) -> Result<(), String> {
    Ok(())
}

/// Unregister a space
pub fn unregister_space(_name: &str) -> Result<(), String> {
    Ok(())
}

/// Get Vortex API URL
pub fn get_vortex_api_url() -> String {
    SPATIAL_VORTEX_URL.to_string()
}

/// Get space API URL
pub fn get_space_api_url(space_name: &str) -> String {
    format!("{}/spaces/{}", SPATIAL_VORTEX_URL, space_name)
}

/// Transport status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// WebTransport status tracker
#[derive(Resource, Debug, Default)]
pub struct WebTransportStatusTracker {
    pub status: TransportStatus,
    pub last_error: Option<String>,
}

/// Notify transport status change
pub fn notify_transport_status(_status: TransportStatus) {
    // TODO: Implement status notification
}

/// Get transport status as string
pub fn get_transport_status_string(status: TransportStatus) -> &'static str {
    match status {
        TransportStatus::Disconnected => "Disconnected",
        TransportStatus::Connecting => "Connecting",
        TransportStatus::Connected => "Connected",
        TransportStatus::Error => "Error",
    }
}

/// Check if transport is connected
pub fn is_transport_connected(status: TransportStatus) -> bool {
    status == TransportStatus::Connected
}
