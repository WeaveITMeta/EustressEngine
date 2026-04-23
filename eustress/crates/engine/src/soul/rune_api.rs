//! # Rune Script API — Engine integration
//!
//! Wraps the common `RuneRuntimeState` with engine-specific concerns:
//! - Queries `SoulScriptData` components to gather scripts for compilation
//! - Installs the engine's full ECS module (with spatial queries, camera, etc.)
//! - Wires into `PlayModeState` transitions

use bevy::prelude::*;
use std::collections::HashMap;

// Re-export the common runtime for external use
pub use eustress_common::soul::rune_runtime::{
    RuneRuntimeState, RuneModuleRegistry, ScriptSource,
    run_script_init, run_script_ready, run_script_update,
    run_script_exit, cleanup_scripts as cleanup_scripts_on_stop,
};

/// Rune script execution engine (legacy compat)
#[derive(Debug, Default)]
pub struct RuneScriptEngine {
    pub modules: HashMap<String, String>,
}

impl RuneScriptEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Script command for execution
#[derive(Debug, Clone)]
pub enum ScriptCommand {
    Spawn { class: String, name: String },
    Destroy { entity: Entity },
    SetProperty { entity: Entity, property: String, value: String },
    PlaySound { path: String },
    Log { message: String },
}

/// Physics spawn configuration
#[derive(Debug, Clone, Default)]
pub struct SpawnPhysics {
    pub enabled: bool,
    pub mass: f32,
    pub friction: f32,
}

/// Entity data for scripts
#[derive(Debug, Clone, Default)]
pub struct EntityData {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// Input data for scripts
#[derive(Debug, Clone, Default)]
pub struct InputData {
    pub mouse_position: Vec2,
    pub keys_pressed: Vec<String>,
}

/// Physics data for scripts
#[derive(Debug, Clone, Default)]
pub struct PhysicsData {
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
}

// ============================================================================
// Engine-specific systems — query SoulScriptData + install ECS module
// ============================================================================

/// System: register the engine's ECS module into the RuneModuleRegistry.
/// Called once at startup so modules are ready when play mode starts.
pub fn register_engine_rune_modules(
    mut module_registry: ResMut<RuneModuleRegistry>,
) {
    #[cfg(feature = "realism-scripting")]
    {
        match super::rune_ecs_module::create_ecs_module() {
            Ok(module) => {
                module_registry.add_module(module);
                info!("✅ Registered engine ECS module for Rune runtime");
            }
            Err(e) => {
                error!("Failed to create engine ECS module: {}", e);
            }
        }
    }
}

/// System: recompile any Rune scripts whose `SoulScriptData.dirty` flag
/// was set by the file watcher. Runs every frame during Update; a
/// no-op when no scripts changed. Lets the user save a `.rune` file in
/// an external editor (or Studio) and see the change take effect on
/// the next frame without leaving Play mode.
///
/// Pure Rune only — SoulScript's AI-assisted build pipeline still runs
/// via `TriggerBuildEvent`. This path skips it entirely.
pub fn hot_recompile_dirty_rune_scripts(
    mut scripts: Query<(Entity, &Name, &mut super::SoulScriptData)>,
    mut runtime: ResMut<RuneRuntimeState>,
    module_registry: Res<RuneModuleRegistry>,
) {
    #[cfg(feature = "realism-scripting")]
    {
        for (entity, name, mut data) in scripts.iter_mut() {
            if !data.dirty {
                continue;
            }
            if data.run_context != super::SoulRunContext::Rune {
                continue;
            }
            if data.source.is_empty() {
                data.dirty = false; // nothing to compile; clear the flag
                continue;
            }
            let entity_index = entity.index().index();
            let name_str = name.as_str().to_string();
            match eustress_common::soul::rune_runtime::hot_recompile_one_script(
                &mut runtime,
                &module_registry,
                entity_index,
                &name_str,
                &data.source,
            ) {
                Ok(()) => {
                    data.dirty = false;
                    data.build_status = crate::soul::SoulBuildStatus::Built;
                    data.errors.clear();
                }
                Err(msg) => {
                    // Clear dirty so retries only happen on the next
                    // save — otherwise a broken file would try every
                    // frame and flood the log.
                    data.dirty = false;
                    data.build_status = crate::soul::SoulBuildStatus::Failed;
                    data.errors = vec![msg];
                }
            }
        }
    }
    #[allow(unused_variables)]
    {
        let _ = (&scripts, &runtime, &module_registry);
    }
}

/// System: drain Rune + Luau script errors into the Output panel so users
/// see every compile/runtime failure alongside their `log_info` lines.
///
/// Rune errors accumulate in [`RuneRuntimeState::last_errors`] as
/// `(script_name, message)` pairs — pushed by the initial compile
/// (`compile_scripts`), the hot-recompile path, and `run_script_update`
/// when `on_update` throws. This system drains that vec each frame and
/// forwards each entry to [`crate::ui::OutputConsole`] tagged with the
/// `rune` source so the panel's filter chips work. After drain the
/// vec is cleared — same error doesn't re-push every frame.
///
/// Luau errors arrive via `LuauScriptErrorEvent` messages (emitted by
/// the common crate's Luau runtime). We drain those too.
///
/// Both paths include the script name and the full error text; the
/// Rune compiler and mlua both embed line:col info in their messages
/// already, so the user sees "foo.rune:12:5: expected identifier" etc.
/// without extra parsing on our end.
pub fn drain_script_errors_to_output(
    mut runtime: ResMut<RuneRuntimeState>,
    mut luau_errors: MessageReader<eustress_common::luau::runtime::LuauScriptErrorEvent>,
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
    analysis: Option<Res<crate::script_editor::ScriptAnalysis>>,
    mut last_analysis_gen: Local<u64>,
    queue: Option<Res<eustress_common::change_queue::ChangeQueue>>,
) {
    // LSP-grade compile diagnostics — same source of truth the Problems
    // panel and squiggle overlay use. Push exactly once per analyzer
    // generation so edits don't flood the Output while the user is
    // mid-typing; the 80 ms debounce in `ScriptAnalysisPlugin` already
    // throttles the upstream analyze() rate.
    if let (Some(ref mut out), Some(analysis)) = (&mut output, analysis.as_deref()) {
        if analysis.generation != *last_analysis_gen && !analysis.result.diagnostics.is_empty() {
            let path_label = analysis
                .active_path
                .as_deref()
                .unwrap_or("<unsaved>");
            let file_name = std::path::Path::new(path_label)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(path_label);
            for diag in &analysis.result.diagnostics {
                let level = match diag.severity {
                    crate::script_editor::analyzer::Severity::Error => crate::ui::slint_ui::LogLevel::Error,
                    crate::script_editor::analyzer::Severity::Warning => crate::ui::slint_ui::LogLevel::Warn,
                    crate::script_editor::analyzer::Severity::Info
                    | crate::script_editor::analyzer::Severity::Hint => crate::ui::slint_ui::LogLevel::Info,
                };
                // Format: [file.rune:12:5] message — matches the LSP
                // client's hover output and compiler convention.
                out.push_with_source(
                    level,
                    format!(
                        "[{}:{}:{}] {}",
                        file_name,
                        diag.range.start_line,
                        diag.range.start_column,
                        diag.message,
                    ),
                    diag.source, // "rune" or "rune-warning" or "rune-link"
                );
            }
            *last_analysis_gen = analysis.generation;
        }
    }

    // Rune: drain `last_errors`. Use `std::mem::take` so we empty the
    // vec even if `output` isn't available — avoids re-logging next
    // frame once the panel resource shows up.
    //
    // ALSO tee each compile error to the `rune.compile.error` stream
    // topic so MCP / CLI subscribers (including the Workshop AI via
    // `query_stream_events`) can detect failures after calling
    // `execute_rune` and iterate the script until it compiles clean —
    // the feedback loop the user asked for in 2026-04-22.
    let rune_batch = std::mem::take(&mut runtime.last_errors);
    for (script_name, err) in &rune_batch {
        // Each line of `err` is one pre-formatted
        // `script:line:col: error: msg` diagnostic from
        // `format_compile_diagnostics`. Publish them line-by-line
        // so subscribers see one structured event per diagnostic.
        if let Some(ref queue) = queue {
            for line in err.lines() {
                if line.trim().is_empty() { continue; }
                let payload = serde_json::json!({
                    "script": script_name,
                    "line": line.trim(),
                });
                if let Ok(bytes) = serde_json::to_vec(&payload) {
                    queue.stream
                        .producer("rune.compile.error")
                        .send_bytes(bytes::Bytes::from(bytes));
                }
            }
        }
    }
    if let Some(ref mut out) = output {
        for (script_name, err) in rune_batch {
            // Split multi-line errors so each line is one OutputConsole
            // entry — otherwise the Slint TextInput shows `\n`-joined
            // text on a single row and the timestamp/level badges only
            // annotate the first line.
            for line in err.lines() {
                if line.trim().is_empty() { continue; }
                out.push_with_source(
                    crate::ui::slint_ui::LogLevel::Error,
                    format!("[{}] {}", script_name, line),
                    "rune",
                );
            }
        }
    }

    // Luau: message-driven, one event per error.
    for event in luau_errors.read() {
        if let Some(ref mut out) = output {
            let line_suffix = event
                .line
                .map(|l| format!(":{}", l))
                .unwrap_or_default();
            out.push_with_source(
                crate::ui::slint_ui::LogLevel::Error,
                format!("[{}{}] {}", event.script_name, line_suffix, event.error),
                "luau",
            );
        }
    }
}

/// System: hot-reload dirty Luau scripts during Play mode. Luau uses a
/// single persistent `Lua` state — re-running `execute_chunk` on the
/// updated source redefines any globals / functions the script sets
/// up, which is the Luau equivalent of "recompile" for Rune.
///
/// Parallel to `hot_recompile_dirty_rune_scripts` so external-editor
/// saves propagate live for both languages.
pub fn hot_reload_dirty_luau_scripts(
    mut scripts: Query<(&Name, &mut super::SoulScriptData)>,
    mut luau_state: Option<ResMut<eustress_common::luau::runtime::LuauRuntimeState>>,
) {
    #[cfg(feature = "luau")]
    {
        let Some(luau_state) = luau_state.as_deref_mut() else { return };
        let Some(runtime) = luau_state.runtime.as_mut() else {
            // LuauRuntimeState is registered but hasn't lazy-initialised
            // its runtime yet — clear dirty flags so we don't retry the
            // whole list every frame until the first execution lands.
            for (_, mut data) in scripts.iter_mut() {
                if data.dirty && data.run_context == super::SoulRunContext::Luau {
                    data.dirty = false;
                }
            }
            return;
        };

        for (name, mut data) in scripts.iter_mut() {
            if !data.dirty {
                continue;
            }
            if data.run_context != super::SoulRunContext::Luau {
                continue;
            }
            if data.source.is_empty() {
                data.dirty = false;
                continue;
            }
            let chunk_name = format!("hot-reload:{}", name.as_str());
            match runtime.execute_chunk(&data.source, &chunk_name) {
                Ok(()) => {
                    info!("🔥 Hot-reloaded Luau script '{}'", name.as_str());
                    data.dirty = false;
                    data.build_status = crate::soul::SoulBuildStatus::Built;
                    data.errors.clear();
                }
                Err(msg) => {
                    warn!("⚠ Luau hot-reload error in '{}': {}", name.as_str(), msg);
                    data.dirty = false;
                    data.build_status = crate::soul::SoulBuildStatus::Failed;
                    data.errors = vec![msg];
                }
            }
        }
    }
    #[cfg(not(feature = "luau"))]
    {
        let _ = (&scripts, &luau_state);
    }
}

/// System: compile all SoulScriptData entities when entering Playing state.
/// Gathers script sources from ECS and delegates to common runtime.
pub fn compile_scripts_on_play(
    scripts: Query<(Entity, &Name, &super::SoulScriptData, Option<&crate::space::LoadedFromFile>)>,
    mut runtime: ResMut<RuneRuntimeState>,
    module_registry: Res<RuneModuleRegistry>,
) {
    let total_scripts = scripts.iter().count();
    let sources: Vec<ScriptSource> = scripts.iter()
        .filter(|(_, _name, data, loaded)| {
            if data.source.is_empty() { return false; }
            if data.run_context != super::SoulRunContext::Rune { return false; }
            // Only compile .rune and .soul files — skip .md, .txt, and anything else
            if let Some(loaded) = loaded {
                let ext = loaded.path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                matches!(ext, "rune" | "soul")
            } else {
                // No LoadedFromFile — in-memory script, compile it
                true
            }
        })
        .map(|(entity, name, data, _loaded)| ScriptSource {
            entity_index: entity.index().index(),
            name: name.as_str().to_string(),
            source: data.source.clone(),
        })
        .collect();

    info!("🎮 compile_scripts_on_play: {} total SoulScriptData entities, {} Rune scripts to compile",
        total_scripts, sources.len());
    for s in &sources {
        info!("  📜 Script '{}' ({} bytes)", s.name, s.source.len());
    }

    #[cfg(feature = "realism-scripting")]
    {
        eustress_common::soul::rune_runtime::compile_scripts(
            &mut runtime,
            &module_registry,
            &sources,
        );

        if !runtime.last_errors.is_empty() {
            for (name, err) in &runtime.last_errors {
                error!("❌ Script '{}' compile error: {}", name, err);
            }
        }
    }

    let _ = sources;
}

// ============================================================================
// Engine wrapper systems — populate thread-locals before script execution
// ============================================================================

/// System: populate ECS bindings + SIM_VALUES thread-locals before Rune scripts run.
/// Must run BEFORE run_script_init / run_script_update each frame.
/// Frame counter for periodic debug logging (every 60 frames = ~1s)
static SCRIPT_LOG_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn prepare_script_bindings(
    ecs_bindings: Option<Res<crate::ui::rune_ecs_bindings::ECSBindings>>,
    sim_values_res: Option<Res<crate::simulation::plugin::SimValuesResource>>,
) {
    let frame = SCRIPT_LOG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    #[cfg(feature = "realism-scripting")]
    {
        if let Some(bindings) = ecs_bindings {
            super::rune_ecs_module::set_ecs_bindings(bindings.clone());

            if let Ok(sim) = bindings.simulation.read() {
                let count = sim.len();
                super::rune_ecs_module::SIM_VALUES.with(|sv| {
                    let mut sv = sv.borrow_mut();
                    for (k, v) in sim.iter() {
                        sv.insert(k.clone(), *v);
                    }
                });

                if frame % 60 == 0 && count > 0 {
                    info!("🔗 Script bindings: {} sim values (frame {})", count, frame);
                }
            }
        } else if frame % 300 == 0 {
            warn!("⚠ ECSBindings resource not found — scripts will read 0.0 for sim values");
        }

        // Also copy from SimValuesResource (populated by publish_echem_to_sim_values)
        // into the thread-local so Rune scripts can read electrochemistry data
        // regardless of which thread published it.
        if let Some(ref svr) = sim_values_res {
            if !svr.0.is_empty() {
                super::rune_ecs_module::SIM_VALUES.with(|sv| {
                    let mut sv = sv.borrow_mut();
                    for (k, v) in svr.0.iter() {
                        sv.insert(k.clone(), *v);
                    }
                });
            }
        }
    }

    let _ = frame;
}

/// System: clear thread-local bindings after Rune scripts have run.
/// Must run AFTER run_script_update each frame.
pub fn cleanup_script_bindings() {
    #[cfg(feature = "realism-scripting")]
    {
        super::rune_ecs_module::clear_ecs_bindings();
    }
}

/// System: drain script log buffer into OutputConsole (runs every frame during play).
pub fn drain_script_logs_to_output(
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
) {
    let logs = eustress_common::gui::drain_script_logs();
    if logs.is_empty() { return; }
    let Some(ref mut out) = output else { return; };
    for entry in logs {
        match entry.level {
            eustress_common::gui::ScriptLogLevel::Info => out.info(entry.message),
            eustress_common::gui::ScriptLogLevel::Warn => out.warn(entry.message),
            eustress_common::gui::ScriptLogLevel::Error => out.error(entry.message),
        }
    }
}

// Legacy stubs for compatibility
pub fn execute_rune_script(_source: &str, _context: &mut super::soul_context::SoulContext) -> Result<(), String> {
    Ok(())
}

pub fn validate_rune_script(_source: &str) -> Result<(), Vec<String>> {
    Ok(())
}

pub fn update_world_state(_world: &World) {}
pub fn update_input_state(_input: &ButtonInput<KeyCode>) {}
pub fn update_mouse_raycast(_ray: Option<Ray3d>) {}
