// ============================================================================
// Eustress Engine - Telemetry
// Opt-in error reporting via Sentry
// ============================================================================

use bevy::prelude::*;

/// Plugin for opt-in telemetry and error reporting
pub struct TelemetryPlugin;

impl Plugin for TelemetryPlugin {
    fn build(&self, _app: &mut App) {
        // Telemetry systems:
        // - Sentry error reporting (opt-in)
        // - Crash reporting
        // - Performance metrics
    }
}

/// Report Rune validation error
pub fn report_rune_validation_error(_script_name: &str, _error: &str) {
    // TODO: Send to Sentry if telemetry enabled
}

/// Report Claude generation error
pub fn report_claude_generation_error(_prompt: &str, _error: &str) {
    // TODO: Send to Sentry if telemetry enabled
}

/// Report Rune success
pub fn report_rune_success(_script_name: &str, _duration_ms: u64) {
    // TODO: Track success metrics
}

/// Telemetry settings
#[derive(Resource, Debug, Clone, Default)]
pub struct TelemetrySettings {
    pub enabled: bool,
    pub anonymous: bool,
}

/// Initialize telemetry
pub fn init_telemetry(_settings: &TelemetrySettings) {
    // TODO: Initialize Sentry
}

/// Shutdown telemetry
pub fn shutdown_telemetry() {
    // TODO: Shutdown Sentry
}

/// Check if telemetry is enabled
pub fn is_telemetry_enabled() -> bool {
    false
}
