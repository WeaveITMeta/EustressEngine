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

use super::analyzer::{self, AnalysisResult, Diagnostic};
use super::language::{Analysis, LanguageRegistry, LanguageService};
use std::sync::Arc;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures_lite::future};
use std::collections::HashMap;
use std::path::PathBuf;
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
    /// The service's full output that `result` is derived from, including
    /// its own index for hover and go to definition.
    pub analysis: Option<Analysis>,
    /// The service analyzing the active file.
    pub service: Option<Arc<dyn LanguageService>>,
    /// Monotonic counter, incremented on every new result. UI consumers
    /// compare against their last seen generation to decide whether to
    /// rebuild models.
    pub generation: u64,
    /// `StudioState::script_content_revision` last acted on.
    seen_revision: u64,

    // ── debounce state ────────────────────────────────────────────────
    /// Wall-clock time of the most recent source change.
    pending_change: Option<Instant>,
    /// Source text queued for the next task. Overwritten on every edit so
    /// only the latest content is analyzed.
    pending_source: Option<String>,
    /// The currently-running analyzer task, if any. We poll this each tick.
    in_flight: Option<Task<Analysis>>,
}

impl ScriptAnalysis {
    /// Submit a new source text for analysis. Called from the editor sync
    /// layer whenever `script_editor_content` changes. Debounce is handled
    /// internally — the actual task kickoff happens after `DEBOUNCE_MS`
    /// milliseconds with no further edits.
    pub fn submit(&mut self, source: String, service: Arc<dyn LanguageService>) {
        self.pending_source = Some(source);
        self.pending_change = Some(Instant::now());
        self.service = Some(service);
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

/// Space-wide diagnostics collected by scanning all scripts on load.
/// Keyed by absolute file path string. The active editor's diagnostics
/// (from `ScriptAnalysis`) override the cached entry for that file.
#[derive(Resource, Default)]
pub struct SpaceDiagnostics {
    /// file_path → diagnostics from the last completed scan of that file.
    pub by_file: HashMap<String, Vec<Diagnostic>>,
    /// Bumped each time any entry changes, so `sync_analyzer_to_slint`
    /// can skip no-op frames.
    pub generation: u64,
    /// In-flight batch scan task. Produces a full map replacement.
    in_flight: Option<Task<HashMap<String, Vec<Diagnostic>>>>,
    /// Space root at the time the scan was launched — used to detect
    /// whether we need to re-scan when the Space changes.
    scanned_root: Option<PathBuf>,
}

impl SpaceDiagnostics {
    pub fn error_count(&self) -> usize {
        self.by_file.values()
            .flat_map(|v| v.iter())
            .filter(|d| d.severity == analyzer::Severity::Error)
            .count()
    }
    pub fn warning_count(&self) -> usize {
        self.by_file.values()
            .flat_map(|v| v.iter())
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

    let Some(service) = analysis.service.clone() else { return };
    let source = analysis.pending_source.take().unwrap_or_default();
    analysis.pending_change = None;
    analysis.source = source.clone();

    let task = AsyncComputeTaskPool::get().spawn(async move {
        service.analyze(&source)
    });
    analysis.in_flight = Some(task);
}

/// Poll the in-flight task; when it finishes, store the result and bump
/// the generation counter so UI systems redraw. Also updates the
/// Space-wide cache so the Problems panel shows live edits.
fn poll_analysis(
    mut analysis: ResMut<ScriptAnalysis>,
    mut space_diag: ResMut<SpaceDiagnostics>,
) {
    let Some(task) = analysis.in_flight.as_mut() else { return };
    let Some(result) = future::block_on(future::poll_once(task)) else {
        return;
    };
    // Mirror into SpaceDiagnostics for the merged Problems panel view.
    if let Some(path) = &analysis.active_path.clone() {
        update_space_diag_from_editor(&mut space_diag, path, result.diagnostics.clone());
    }
    analysis.result = AnalysisResult {
        diagnostics: result.diagnostics.clone(),
        symbols: index_symbols(&result.symbols),
    };
    analysis.analysis = Some(result);
    analysis.generation = analysis.generation.wrapping_add(1);
    analysis.in_flight = None;
}

/// Kick off a background scan of all `.rune` / `.luau` files under the
/// current Space's `SoulService/` folder when the Space root changes.
fn kick_off_space_scan(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    languages: Option<Res<ScriptLanguages>>,
    mut space_diag: ResMut<SpaceDiagnostics>,
) {
    let Some(space_root) = space_root else { return };
    // Only re-scan when the root has actually changed.
    let current_root = space_root.0.clone();
    if space_diag.scanned_root.as_ref().map(|p| p.as_path()) == Some(current_root.as_path()) {
        return;
    }
    // Don't queue a new scan while one is in-flight for the same root.
    if space_diag.in_flight.is_some() {
        return;
    }
    space_diag.scanned_root = Some(current_root.clone());

    let registry = languages.map(|l| l.0.clone()).unwrap_or_default();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        scan_space_scripts(&current_root, &registry)
    });
    space_diag.in_flight = Some(task);
}

/// Poll the background scan task. When done, replace `by_file` and bump
/// the generation so the Problems panel redraws.
fn poll_space_scan(mut space_diag: ResMut<SpaceDiagnostics>) {
    let Some(task) = space_diag.in_flight.as_mut() else { return };
    let Some(result) = future::block_on(future::poll_once(task)) else {
        return;
    };
    space_diag.by_file = result;
    space_diag.generation = space_diag.generation.wrapping_add(1);
    space_diag.in_flight = None;
}

/// Called from `submit_active_script` / `poll_analysis` to keep the
/// Space-wide cache in sync with live edits in the active tab.
pub fn update_space_diag_from_editor(
    space_diag: &mut SpaceDiagnostics,
    path: &str,
    diagnostics: Vec<Diagnostic>,
) {
    space_diag.by_file.insert(path.to_string(), diagnostics);
    space_diag.generation = space_diag.generation.wrapping_add(1);
}

/// Walk `<space_root>/SoulService/` (and fall back to the whole Space root)
/// and run each file's language service over it: Rune files through the
/// Rune service, Luau files through the Luau service. A file no service
/// handles is skipped.
fn scan_space_scripts(
    space_root: &std::path::Path,
    registry: &LanguageRegistry,
) -> HashMap<String, Vec<Diagnostic>> {
    let soul_dir = space_root.join("SoulService");
    let scan_root = if soul_dir.is_dir() { soul_dir } else { space_root.to_path_buf() };

    let mut results = HashMap::new();
    walk_for_scripts(&scan_root, 0, registry, &mut results);
    results
}

fn walk_for_scripts(
    dir: &std::path::Path,
    depth: usize,
    registry: &LanguageRegistry,
    out: &mut HashMap<String, Vec<Diagnostic>>,
) {
    if depth > 12 { return; }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let ft = match entry.file_type() { Ok(f) => f, Err(_) => continue };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "target" || name_str == "node_modules" {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            walk_for_scripts(&path, depth + 1, registry, out);
        } else if ft.is_file() {
            let Some(service) = registry.for_path(&path) else { continue };
            // A file over 1 MB is data, not a script.
            if entry.metadata().map_or(true, |m| m.len() > 1_000_000) {
                continue;
            }
            if let Ok(source) = std::fs::read_to_string(&path) {
                let diagnostics = service.analyze(&source).diagnostics;
                if !diagnostics.is_empty() {
                    out.insert(path.display().to_string(), diagnostics);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────

pub struct ScriptAnalysisPlugin;

/// The language services the editor shell hands source files to, one per
/// language. The shell asks `for_path(source)` and never branches on the
/// language itself.
#[derive(Resource, Clone)]
pub struct ScriptLanguages(pub super::language::LanguageRegistry);

impl Default for ScriptLanguages {
    fn default() -> Self {
        let mut registry = super::language::LanguageRegistry::default();
        registry.register(std::sync::Arc::new(super::rune_service::RuneService));
        registry.register(std::sync::Arc::new(super::luau_service::LuauService));
        Self(registry)
    }
}

impl Plugin for ScriptAnalysisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptAnalysis>()
           .init_resource::<ScriptLanguages>()
           .init_resource::<SpaceDiagnostics>()
           .add_systems(Update, (
               submit_active_script,
               kick_off_analysis,
               poll_analysis,
               kick_off_space_scan,
               poll_space_scan,
           ).chain());
    }
}

/// Hand the active Code view's text to the service for its source file.
/// Runs once per applied edit (`script_content_revision`); the highlighter
/// owns `script_content_dirty`. `script_source_path` is set only for a Code
/// view, so Summary and Markdown views, and files no service handles, are
/// not submitted.
fn submit_active_script(
    state: Option<Res<crate::ui::StudioState>>,
    tab_manager: Option<Res<crate::ui::center_tabs::CenterTabManager>>,
    languages: Option<Res<ScriptLanguages>>,
    mut analysis: ResMut<ScriptAnalysis>,
) {
    let Some(state) = state else { return };
    if state.script_content_revision == analysis.seen_revision { return }
    analysis.seen_revision = state.script_content_revision;

    // The tab's own path keys the Problems panel's jump routing.
    let path = tab_manager.as_deref()
        .and_then(|mgr| mgr.tabs.get(mgr.active_tab))
        .and_then(|t| t.file_path.as_ref())
        .map(|p| p.display().to_string());
    let service = state
        .script_source_path
        .as_deref()
        .and_then(|source| languages.as_ref().and_then(|l| l.0.for_path(source)));

    if let Some(service) = service {
        analysis.submit(state.script_editor_content.clone(), service);
        // Track which file the analyzer is operating on — Problems panel
        // uses this for "jump to line" routing.
        analysis.active_path = path;
    } else if !analysis.result.diagnostics.is_empty()
        || analysis.pending_source.is_some()
        || analysis.in_flight.is_some()
    {
        // No service for this view: drop the previous file's results so
        // they never draw on this text.
        analysis.pending_source = None;
        analysis.pending_change = None;
        analysis.in_flight = None;
        analysis.result = analyzer::AnalysisResult::default();
        analysis.analysis = None;
        analysis.service = None;
        analysis.generation = analysis.generation.wrapping_add(1);
        analysis.active_path = path;
    }
}

/// A symbol index over a service's symbols, for the consumers that look
/// definitions up by name.
fn index_symbols(symbols: &[analyzer::Symbol]) -> analyzer::SymbolIndex {
    let mut index = analyzer::SymbolIndex::default();
    for symbol in symbols {
        index.by_name.entry(symbol.name.clone()).or_default().push(symbol.clone());
    }
    index
}
