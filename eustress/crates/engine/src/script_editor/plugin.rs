//! # ScriptAnalysisPlugin — bridge between `analyzer` and Bevy
//!
//! Owns a single Bevy `Resource` ([`ScriptAnalysis`]) that mirrors the
//! current editor's analyzer output. Consumers (squiggle renderer, Problems
//! panel, go-to-def handler, etc.) read this resource; they never call the
//! analyzer directly.
//!
//! ## Scheduling
//!
//! Analysis runs on `AsyncComputeTaskPool` with an 80 ms debounce. That
//! matches the human typing rate — anything faster wastes CPU on every
//! keystroke, anything slower feels laggy. Debouncing + cancellation are
//! handled by the `pending_change` timestamp + `in_flight` task slot; if a
//! new edit arrives while a task is still running, we simply let the old
//! task finish and throw its result away (its source text is already stale
//! — the fresh task's output supersedes it).

use super::analyzer::{self, AnalysisResult};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures_lite::future};
use std::time::{Duration, Instant};

/// Debounce delay between the last edit and the analysis kickoff. Tuned to
/// roughly the pause-between-keystrokes for fluent typists.
const DEBOUNCE_MS: u64 = 80;

/// Latest analyzer output plus bookkeeping for the debounce/cancel logic.
/// The `generation` counter bumps on every new result — UI consumers track
/// it to skip redundant redraws.
#[derive(Resource, Default)]
pub struct ScriptAnalysis {
    /// The file we're analyzing, if any. `None` when no script tab is open.
    /// Populated by the sync layer when the active tab changes.
    pub active_path: Option<String>,
    /// Source text last submitted for analysis. Used both to detect whether
    /// we need to re-run and to map byte offsets back to pixel positions.
    pub source: String,
    /// Most recent completed analysis. Starts empty; filled on first tick
    /// after the editor receives any content.
    pub result: AnalysisResult,
    /// Monotonic counter, incremented on every new result. UI consumers
    /// compare against their last seen generation to decide whether to
    /// rebuild models.
    pub generation: u64,

    // ── debounce state ────────────────────────────────────────────────
    /// Wall-clock time of the most recent source change.
    pending_change: Option<Instant>,
    /// Source text queued for the next task. Overwritten on every edit so
    /// only the latest content is analyzed.
    pending_source: Option<String>,
    /// The currently-running analyzer task, if any. We poll this each tick.
    in_flight: Option<Task<AnalysisResult>>,
}

impl ScriptAnalysis {
    /// Submit a new source text for analysis. Called from the editor sync
    /// layer whenever `script_editor_content` changes. Debounce is handled
    /// internally — the actual task kickoff happens after `DEBOUNCE_MS`
    /// milliseconds with no further edits.
    pub fn submit(&mut self, source: String) {
        self.pending_source = Some(source);
        self.pending_change = Some(Instant::now());
    }

    /// Current diagnostic count, for status-bar / badge rendering.
    pub fn error_count(&self) -> usize {
        self.result.diagnostics.iter()
            .filter(|d| d.severity == analyzer::Severity::Error)
            .count()
    }
    pub fn warning_count(&self) -> usize {
        self.result.diagnostics.iter()
            .filter(|d| d.severity == analyzer::Severity::Warning)
            .count()
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────

/// Kick off an analyzer task when the debounce window has elapsed and no
/// task is currently in-flight.
fn kick_off_analysis(mut analysis: ResMut<ScriptAnalysis>) {
    // Guard: nothing queued OR task already running.
    if analysis.pending_source.is_none() || analysis.in_flight.is_some() {
        return;
    }
    // Guard: debounce window hasn't elapsed.
    let Some(queued_at) = analysis.pending_change else { return };
    if queued_at.elapsed() < Duration::from_millis(DEBOUNCE_MS) {
        return;
    }

    let source = analysis.pending_source.take().unwrap_or_default();
    analysis.pending_change = None;
    analysis.source = source.clone();

    let task = AsyncComputeTaskPool::get().spawn(async move {
        analyzer::analyze(&source)
    });
    analysis.in_flight = Some(task);
}

/// Poll the in-flight task; when it finishes, store the result and bump
/// the generation counter so UI systems redraw.
fn poll_analysis(mut analysis: ResMut<ScriptAnalysis>) {
    let Some(task) = analysis.in_flight.as_mut() else { return };
    let Some(result) = future::block_on(future::poll_once(task)) else {
        return; // Still running — poll again next tick.
    };
    analysis.result = result;
    analysis.generation = analysis.generation.wrapping_add(1);
    analysis.in_flight = None;
}

// ─────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────

pub struct ScriptAnalysisPlugin;

impl Plugin for ScriptAnalysisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptAnalysis>()
           .add_systems(Update, (
               submit_active_script,
               kick_off_analysis,
               poll_analysis,
           ).chain());
    }
}

/// Read the active editor tab's code content and hand it to the analyzer.
/// Only fires on `script_content_dirty` to avoid re-submitting the same
/// source every frame. Summary (markdown) tabs are ignored — there's no
/// Rune to parse.
fn submit_active_script(
    state: Option<ResMut<crate::ui::StudioState>>,
    tab_manager: Option<Res<crate::ui::center_tabs::CenterTabManager>>,
    mut analysis: ResMut<ScriptAnalysis>,
) {
    let Some(mut state) = state else { return };
    if !state.script_content_dirty { return }

    // Determine whether the active tab is a Rune code view. Summary markdown
    // is ignored; the analyzer would emit noise on markdown prose.
    let is_code = tab_manager
        .as_deref()
        .and_then(|mgr| mgr.tabs.get(mgr.active_tab))
        .map(|tab| matches!(
            tab.tab_type,
            crate::ui::center_tabs::CenterTabType::SoulScript {
                mode: crate::ui::center_tabs::SoulScriptMode::Code
            } | crate::ui::center_tabs::CenterTabType::CodeEditor { .. }
        ))
        .unwrap_or(false);

    if is_code {
        analysis.submit(state.script_editor_content.clone());
        // Track which file the analyzer is operating on — Problems panel
        // uses this for "jump to line" routing.
        analysis.active_path = tab_manager.as_deref()
            .and_then(|mgr| mgr.tabs.get(mgr.active_tab))
            .and_then(|t| t.file_path.as_ref())
            .map(|p| p.display().to_string());
    }
    state.script_content_dirty = false;
}
