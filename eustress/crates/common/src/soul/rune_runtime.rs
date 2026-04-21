//! # Rune Script Runtime
//!
//! Core Rune VM runtime for compiling and executing scripts each frame.
//! Shared between engine (editor play mode) and client (game runtime).
//!
//! The caller provides Rune `Module`s to install (ECS bindings, GUI API, etc.)
//! so each context can customize what's available to scripts.

use bevy::prelude::*;
use tracing::{info, warn, error};
use std::collections::HashMap;

// Thread-local output buffer for capturing print/warn from Rune scripts.
thread_local! {
    pub static RUNE_OUTPUT: std::cell::RefCell<Vec<(String, bool)>> = std::cell::RefCell::new(Vec::new());
}

/// Drain all captured Rune output since the last call.
pub fn drain_rune_output() -> Vec<(String, bool)> {
    RUNE_OUTPUT.with(|buf| buf.borrow_mut().drain(..).collect())
}

// ============================================================================
// Runtime State — Bevy resource tracking compiled scripts
// ============================================================================

/// Resource tracking compiled Rune scripts ready for per-frame execution.
/// Insert this resource in your app, then use the systems below.
#[derive(Resource, Default)]
pub struct RuneRuntimeState {
    /// Compiled script units keyed by entity index
    #[cfg(feature = "realism-scripting")]
    pub compiled: HashMap<u32, CompiledScript>,
    /// Whether on_init has been called for each script
    pub initialized: HashMap<u32, bool>,
    /// Errors from last frame (for display in output panel)
    pub last_errors: Vec<(String, String)>,
}

/// A compiled Rune script ready for execution
#[cfg(feature = "realism-scripting")]
pub struct CompiledScript {
    pub unit: std::sync::Arc<rune::Unit>,
    pub context: std::sync::Arc<rune::runtime::RuntimeContext>,
    pub name: String,
}

// ============================================================================
// Compilation — builds Rune context from caller-provided modules
// ============================================================================

/// Configuration for building the Rune runtime context.
/// Callers push their modules here before compilation.
#[derive(Resource, Default)]
pub struct RuneModuleRegistry {
    #[cfg(feature = "realism-scripting")]
    modules: Vec<rune::Module>,
}

#[cfg(feature = "realism-scripting")]
impl RuneModuleRegistry {
    /// Register a Rune module to be installed when scripts are compiled
    pub fn add_module(&mut self, module: rune::Module) {
        self.modules.push(module);
    }

    /// Build a Rune Context with all registered modules installed
    pub fn build_context(&self) -> Result<rune::Context, String> {
        let mut ctx = rune::Context::with_default_modules()
            .map_err(|e| format!("Failed to create Rune context: {}", e))?;

        for module in &self.modules {
            ctx.install(module.clone())
                .map_err(|e| format!("Failed to install Rune module: {}", e))?;
        }

        Ok(ctx)
    }
}

// ============================================================================
// Script source input — what to compile
// ============================================================================

/// A script to be compiled and executed
#[derive(Debug, Clone)]
pub struct ScriptSource {
    /// Entity index (used as key)
    pub entity_index: u32,
    /// Display name
    pub name: String,
    /// Rune source code
    pub source: String,
}

// ============================================================================
// Compile function — called when entering play mode
// ============================================================================

/// Compile a batch of scripts using the registered modules.
/// Call this when entering play mode / starting simulation.
#[cfg(feature = "realism-scripting")]
pub fn compile_scripts(
    runtime: &mut RuneRuntimeState,
    module_registry: &RuneModuleRegistry,
    scripts: &[ScriptSource],
) {
    runtime.compiled.clear();
    runtime.initialized.clear();
    runtime.last_errors.clear();

    let rune_context = match module_registry.build_context() {
        Ok(ctx) => ctx,
        Err(e) => {
            error!("Failed to build Rune context: {}", e);
            runtime.last_errors.push(("runtime".to_string(), e));
            return;
        }
    };

    let runtime_ctx = match rune_context.runtime() {
        Ok(r) => std::sync::Arc::new(r),
        Err(e) => {
            error!("Failed to build runtime context: {}", e);
            runtime.last_errors.push(("runtime".to_string(), e.to_string()));
            return;
        }
    };

    for script in scripts {
        let mut sources = rune::Sources::new();
        let source = match rune::Source::memory(&script.source) {
            Ok(s) => s,
            Err(e) => {
                runtime.last_errors.push((script.name.clone(), format!("Source error: {}", e)));
                continue;
            }
        };
        if let Err(e) = sources.insert(source) {
            runtime.last_errors.push((script.name.clone(), format!("Insert error: {}", e)));
            continue;
        }

        let mut diagnostics = rune::Diagnostics::new();
        let result = rune::prepare(&mut sources)
            .with_context(&rune_context)
            .with_diagnostics(&mut diagnostics)
            .build();

        match result {
            Ok(unit) => {
                runtime.compiled.insert(script.entity_index, CompiledScript {
                    unit: std::sync::Arc::new(unit),
                    context: runtime_ctx.clone(),
                    name: script.name.clone(),
                });
                info!("✅ Compiled Rune script '{}'", script.name);
            }
            Err(e) => {
                let msg = format!("Compile error: {}", e);
                warn!("❌ Failed to compile '{}': {}", script.name, msg);
                runtime.last_errors.push((script.name.clone(), msg));
            }
        }
    }

    info!("🎮 Compiled {} Rune scripts", runtime.compiled.len());
}

// ============================================================================
// Single-script hot recompile — replace one compiled unit while Play runs
// ============================================================================

/// Recompile one Rune script in place and swap the result into
/// [`RuneRuntimeState::compiled`]. Used by the file watcher to deliver
/// live edits without dropping out of Play mode.
///
/// Returns `Ok(())` on success. On compile failure the old unit is left
/// untouched so the running VM keeps executing the last known-good
/// version; the error is pushed onto `runtime.last_errors` and also
/// returned so the caller can surface it in the UI.
///
/// Building a fresh `rune::Context` for each recompile is cheap
/// (microseconds on our module set) and keeps the behaviour identical
/// to [`compile_scripts`] — no lock-in on a half-built context that
/// could drift out of sync with the registry.
#[cfg(feature = "realism-scripting")]
pub fn hot_recompile_one_script(
    runtime: &mut RuneRuntimeState,
    module_registry: &RuneModuleRegistry,
    entity_index: u32,
    name: &str,
    source: &str,
) -> Result<(), String> {
    let rune_context = module_registry.build_context()?;
    let runtime_ctx = std::sync::Arc::new(
        rune_context
            .runtime()
            .map_err(|e| format!("runtime build: {}", e))?,
    );

    let mut sources = rune::Sources::new();
    let src = rune::Source::memory(source)
        .map_err(|e| format!("source init: {}", e))?;
    sources
        .insert(src)
        .map_err(|e| format!("source insert: {}", e))?;

    let mut diagnostics = rune::Diagnostics::new();
    match rune::prepare(&mut sources)
        .with_context(&rune_context)
        .with_diagnostics(&mut diagnostics)
        .build()
    {
        Ok(unit) => {
            runtime.compiled.insert(
                entity_index,
                CompiledScript {
                    unit: std::sync::Arc::new(unit),
                    context: runtime_ctx,
                    name: name.to_string(),
                },
            );
            // Reset the per-entity initialisation flag so `on_init` fires
            // again against the new unit. Without this, changes to
            // `on_init` would silently not take effect until restart.
            runtime.initialized.insert(entity_index, false);
            info!("🔥 Hot-recompiled Rune script '{}'", name);
            Ok(())
        }
        Err(e) => {
            let msg = format!("hot recompile error in '{}': {}", name, e);
            warn!("⚠ {}", msg);
            runtime
                .last_errors
                .push((name.to_string(), msg.clone()));
            Err(msg)
        }
    }
}

// ============================================================================
// One-shot execution — run a script immediately outside simulation mode
// ============================================================================

/// Execute a Rune script string immediately and return the result.
/// Accepts optional extra modules (e.g. the engine's ECS module with Instance API).
/// Captures print output to RUNE_OUTPUT thread-local.
#[cfg(feature = "realism-scripting")]
pub fn execute_oneshot(
    extra_modules: &[rune::Module],
    source_code: &str,
    _name: &str,
) -> Result<String, String> {
    let mut ctx = rune::Context::with_default_modules()
        .map_err(|e| format!("Failed to create Rune context: {}", e))?;

    // Install caller-provided modules (ECS module with Instance API, Vector3, etc.)
    for module in extra_modules {
        ctx.install(module.clone())
            .map_err(|e| format!("Failed to install module: {}", e))?;
    }

    // Install output capture — overrides std::io print functions
    let mut io_module = rune::Module::with_crate("std")
        .map_err(|e| format!("Failed to create io module: {}", e))?;

    io_module.function("println", |s: String| {
        tracing::info!("[Rune] {}", s);
        RUNE_OUTPUT.with(|buf| buf.borrow_mut().push((s, false)));
    }).build()
        .map_err(|e| format!("Failed to register println: {}", e))?;

    io_module.function("print", |s: String| {
        tracing::info!("[Rune] {}", s);
        RUNE_OUTPUT.with(|buf| buf.borrow_mut().push((s, false)));
    }).build()
        .map_err(|e| format!("Failed to register print: {}", e))?;

    ctx.install(io_module)
        .map_err(|e| format!("Failed to install io capture module: {}", e))?;

    let runtime_ctx = ctx.runtime()
        .map_err(|e| format!("Failed to build runtime context: {}", e))?;

    let mut sources = rune::Sources::new();
    let source = rune::Source::memory(source_code)
        .map_err(|e| format!("Source error: {}", e))?;
    sources.insert(source)
        .map_err(|e| format!("Insert error: {}", e))?;

    let mut diagnostics = rune::Diagnostics::new();
    let build_result = rune::prepare(&mut sources)
        .with_context(&ctx)
        .with_diagnostics(&mut diagnostics)
        .build();

    if diagnostics.has_error() || build_result.is_err() {
        // Collect ALL error info — every method, concatenated
        let mut error_lines = Vec::new();

        // Termcolor formatted output
        let mut buf = rune::termcolor::Buffer::no_color();
        let emit_result = diagnostics.emit(&mut buf, &sources);
        let rendered = String::from_utf8_lossy(buf.as_slice()).to_string();
        if !rendered.trim().is_empty() {
            error_lines.push(rendered.trim().to_string());
        } else if let Err(e) = emit_result {
            error_lines.push(format!("[emit failed: {}]", e));
        }

        // Raw diagnostics — always collect, not just as fallback
        let diag_count = diagnostics.diagnostics().len();
        for diag in diagnostics.diagnostics() {
            error_lines.push(format!("{:#?}", diag));
        }

        // Build error
        if let Err(ref e) = build_result {
            error_lines.push(format!("[build error: {}]", e));
        }

        // Log everything for console debugging
        tracing::error!("Rune compile failed: has_error={}, diag_count={}, build_ok={}, lines={}",
            diagnostics.has_error(), diag_count, build_result.is_ok(), error_lines.len());
        for line in &error_lines {
            tracing::error!("  {}", line);
        }

        if error_lines.is_empty() {
            error_lines.push("unknown compile error (no diagnostics produced)".to_string());
        }

        return Err(format!("Compile error:\n{}", error_lines.join("\n")));
    }

    let unit = build_result.unwrap();

    let mut vm = rune::Vm::new(
        std::sync::Arc::new(runtime_ctx),
        std::sync::Arc::new(unit),
    );

    // Try calling main() first, fall back to on_init()
    let result = vm.call(["main"], ())
        .or_else(|_| vm.call(["on_init"], ()))
        .map_err(|e| format!("Runtime error: {}", e))?;

    Ok(format!("{:?}", result))
}

/// Stub for when realism-scripting feature is disabled
#[cfg(not(feature = "realism-scripting"))]
pub fn execute_oneshot(
    _extra_modules: &[()],
    _source_code: &str,
    _name: &str,
) -> Result<String, String> {
    Err("Rune scripting is not enabled (realism-scripting feature required)".to_string())
}

// ============================================================================
// Execution systems — run during play mode
// ============================================================================

/// Call on_init() for any scripts that haven't been initialized yet.
/// Run this every frame during play mode — it tracks which scripts have been init'd.
pub fn run_script_init(
    mut runtime: ResMut<RuneRuntimeState>,
) {
    #[cfg(feature = "realism-scripting")]
    {
        let keys: Vec<u32> = runtime.compiled.keys().cloned().collect();
        for idx in keys {
            if runtime.initialized.get(&idx).copied().unwrap_or(false) {
                continue;
            }
            runtime.initialized.insert(idx, true);

            let compiled = &runtime.compiled[&idx];
            let mut vm = rune::Vm::new(compiled.context.clone(), compiled.unit.clone());

            match vm.call(["on_init"], ()) {
                Ok(_) => {
                    info!("📜 on_init() called for '{}'", compiled.name);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("missing") && !msg.contains("not found") {
                        warn!("⚠ on_init() error in '{}': {}", compiled.name, msg);
                    }
                }
            }
        }
    }
}

/// Call on_update(dt) on all compiled scripts.
/// Run this every frame during play mode, after run_script_init.
pub fn run_script_update(
    mut runtime: ResMut<RuneRuntimeState>,
    time: Res<Time>,
) {
    let dt = time.delta_secs() as f64;

    #[cfg(feature = "realism-scripting")]
    {
        // Collect errors separately to avoid borrow conflict
        let mut errors = Vec::new();

        for (_idx, compiled) in runtime.compiled.iter() {
            let mut vm = rune::Vm::new(compiled.context.clone(), compiled.unit.clone());

            match vm.call(["on_update"], (dt,)) {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("missing") && !msg.contains("not found") {
                        errors.push((compiled.name.clone(), msg));
                    }
                }
            }
        }

        runtime.last_errors = errors;
    }

    let _ = dt;
}

/// Call on_exit() on all scripts before cleanup. Mirrors Godot's _exit_tree().
/// Run this when stopping play mode, BEFORE cleanup_scripts().
pub fn run_script_exit(
    runtime: Res<RuneRuntimeState>,
) {
    #[cfg(feature = "realism-scripting")]
    {
        for (_idx, compiled) in runtime.compiled.iter() {
            let mut vm = rune::Vm::new(compiled.context.clone(), compiled.unit.clone());
            match vm.call(["on_exit"], ()) {
                Ok(_) => {
                    info!("📜 on_exit() called for '{}'", compiled.name);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("missing") && !msg.contains("not found") {
                        warn!("⚠ on_exit() error in '{}': {}", compiled.name, msg);
                    }
                }
            }
        }
    }
}

/// Call on_ready() for scripts whose entity subtree is complete.
/// Unlike on_init() which fires immediately, on_ready() waits one frame
/// to ensure all ChildOf relationships are applied (deferred commands).
/// Mirrors Godot's _ready().
pub fn run_script_ready(
    mut runtime: ResMut<RuneRuntimeState>,
) {
    #[cfg(feature = "realism-scripting")]
    {
        let keys: Vec<u32> = runtime.compiled.keys().cloned().collect();
        for idx in keys {
            // on_ready fires one frame after on_init (initialized == true means init ran)
            let init_done = runtime.initialized.get(&idx).copied().unwrap_or(false);
            let ready_key = idx + 1_000_000; // Use offset key to track ready separately
            let ready_done = runtime.initialized.get(&ready_key).copied().unwrap_or(false);

            if init_done && !ready_done {
                runtime.initialized.insert(ready_key, true);

                let compiled = &runtime.compiled[&idx];
                let mut vm = rune::Vm::new(compiled.context.clone(), compiled.unit.clone());
                match vm.call(["on_ready"], ()) {
                    Ok(_) => {
                        info!("📜 on_ready() called for '{}'", compiled.name);
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if !msg.contains("missing") && !msg.contains("not found") {
                            warn!("⚠ on_ready() error in '{}': {}", compiled.name, msg);
                        }
                    }
                }
            }
        }
    }
}

/// Clear all compiled scripts. Call when stopping play mode.
pub fn cleanup_scripts(
    mut runtime: ResMut<RuneRuntimeState>,
) {
    #[cfg(feature = "realism-scripting")]
    {
        runtime.compiled.clear();
    }
    runtime.initialized.clear();
    runtime.last_errors.clear();
    info!("⏹ Rune script runtime cleaned up");
}
