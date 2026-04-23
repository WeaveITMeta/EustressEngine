// ============================================================================
// Eustress Engine - Telemetry
// Opt-in error reporting via Sentry
// ============================================================================

use bevy::prelude::*;

/// Plugin for opt-in telemetry and error reporting
pub struct TelemetryPlugin;

impl Plugin for TelemetryPlugin {
    fn build(&self, app: &mut App) {
        // UX-polish counters per TOOLSET_UX.md §9 — per-tool
        // activation / commit / cancel counts, `⋯` popover opens,
        // keyboard vs mouse ratio. All gated on
        // `TelemetrySettings.enabled`; no I/O until explicit flush.
        app.init_resource::<TelemetrySettings>()
            .init_resource::<ToolUsageCounters>()
            .add_systems(Update, (
                count_tool_activations,
                count_tool_commits,
                count_tool_cancels,
            ));
    }
}

/// Per-tool usage counters. Accumulates in-memory; persisted to
/// `<universe>/.eustress/telemetry.json` on explicit `flush_to_disk`.
#[derive(Resource, Debug, Default, Clone)]
pub struct ToolUsageCounters {
    pub activations:   std::collections::HashMap<String, u64>,
    pub commits:       std::collections::HashMap<String, u64>,
    pub cancels:       std::collections::HashMap<String, u64>,
    pub advanced_opens: u64,
}

fn count_tool_activations(
    mut events: MessageReader<crate::modal_tool::ActivateModalToolEvent>,
    settings: Res<TelemetrySettings>,
    mut counters: ResMut<ToolUsageCounters>,
) {
    if !settings.enabled { events.clear(); return; }
    for event in events.read() {
        *counters.activations.entry(event.tool_id.clone()).or_default() += 1;
    }
}

fn count_tool_commits(
    mut events: MessageReader<crate::modal_tool::ModalToolCommittedEvent>,
    settings: Res<TelemetrySettings>,
    mut counters: ResMut<ToolUsageCounters>,
) {
    if !settings.enabled { events.clear(); return; }
    for event in events.read() {
        *counters.commits.entry(event.tool_id.clone()).or_default() += 1;
    }
}

fn count_tool_cancels(
    mut events: MessageReader<crate::modal_tool::CancelModalToolEvent>,
    settings: Res<TelemetrySettings>,
    mut counters: ResMut<ToolUsageCounters>,
    active: Res<crate::modal_tool::ActiveModalTool>,
) {
    if !settings.enabled { events.clear(); return; }
    for _ in events.read() {
        if let Some(id) = active.id() {
            *counters.cancels.entry(id.to_string()).or_default() += 1;
        }
    }
}

/// Write counters to `<space_root>/.eustress/telemetry.json`. Called
/// on demand from Settings UI — never automatic.
pub fn flush_counters_to_disk(
    space_root: &std::path::Path,
    settings: &TelemetrySettings,
    counters: &ToolUsageCounters,
) -> Result<std::path::PathBuf, String> {
    if !settings.enabled {
        return Err("Telemetry is opted out — no flush".to_string());
    }
    let path = space_root.join(".eustress").join("telemetry.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let payload = serde_json::json!({
        "anonymous":     settings.anonymous,
        "activations":   counters.activations,
        "commits":       counters.commits,
        "cancels":       counters.cancels,
        "advanced_opens": counters.advanced_opens,
    });
    std::fs::write(&path, payload.to_string()).map_err(|e| e.to_string())?;
    Ok(path)
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
