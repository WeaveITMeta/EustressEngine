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
// Diagnostic formatting — shared by compile + hot-recompile + oneshot
// ============================================================================

/// A structured compile diagnostic, one per Rune error/warning. Emitted
/// from `format_compile_diagnostics` so tools can feed them into the
/// Output panel, MCP responses, or the Problems panel without
/// re-parsing the termcolor output.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone)]
pub struct CompileDiagnostic {
    /// Script name / source id (`<rune-script>` if anonymous).
    pub source: String,
    /// 1-based line number, or 0 if the diagnostic is file-level.
    pub line: u32,
    /// 1-based column, or 0 if unavailable.
    pub column: u32,
    /// Error text from Rune (one line).
    pub message: String,
    /// `true` for errors, `false` for warnings.
    pub is_error: bool,
}

/// Convert a byte offset inside a specific source to a 1-based
/// `(line, column)` pair. Uses `Source::pos_to_utf8_linecol` from the
/// public rune API (its return values are 0-based; we adjust) so we
/// stay in lock-step with the diagnostic positions the Rune compiler
/// reports internally.
#[cfg(feature = "realism-scripting")]
fn byte_offset_to_linecol(
    sources: &rune::Sources,
    source_id: rune::SourceId,
    offset: usize,
) -> (u32, u32) {
    let Some(source) = sources.get(source_id) else {
        return (0, 0);
    };
    let (line, col) = source.pos_to_utf8_linecol(offset);
    (line as u32 + 1, col as u32 + 1)
}

/// Format the diagnostics from a failed Rune compile into a
/// human-readable multi-line string **and** a structured list for
/// downstream tools. Returned text has one diagnostic per line so the
/// Output panel's line-splitter shows them as individual entries with
/// their own timestamp + level badge.
///
/// Format: `<script>:<line>:<col>: <severity>: <message>`. Falls back
/// to the termcolor-rendered dump if the structured API returns nothing
/// usable (rare — happens for build errors that precede diagnostic
/// emission).
#[cfg(feature = "realism-scripting")]
pub fn format_compile_diagnostics(
    script_name: &str,
    diagnostics: &rune::Diagnostics,
    sources: &rune::Sources,
    build_err: Option<&dyn std::fmt::Display>,
) -> (String, Vec<CompileDiagnostic>) {
    let mut structured: Vec<CompileDiagnostic> = Vec::new();
    let mut lines: Vec<String> = Vec::new();

    // Pass 1 — structured diagnostics. Mirrors the extraction the
    // script-editor analyzer uses
    // (`engine::script_editor::analyzer::convert_rune_diagnostic`) so
    // Output-panel messages and Problems-panel squiggles share the
    // same line:column numbers. `Spanned` is needed in scope for
    // `WarningDiagnostic::span()` (trait method).
    use rune::ast::Spanned;
    for diag in diagnostics.diagnostics() {
        use rune::diagnostics::{Diagnostic as RuneDiag, FatalDiagnosticKind};
        let (is_error, source_id, span, message) = match diag {
            RuneDiag::Fatal(fatal) => {
                let sid = fatal.source_id();
                let span = match fatal.kind() {
                    FatalDiagnosticKind::CompileError(err) => err.span(),
                    // Link + internal errors don't carry a usable
                    // span. Collapse to source start so the message
                    // still surfaces with a "?:?" position.
                    _ => rune::ast::Span::new(0u32, 0u32),
                };
                (true, sid, span, fatal.to_string())
            }
            RuneDiag::Warning(w) => (false, w.source_id(), w.span(), w.to_string()),
            // RuntimeWarning fires during VM execution, not compile —
            // we'd never see it here but the match has to be
            // exhaustive. Fall back to source-start + the Debug text
            // so nothing gets silently dropped if upstream adds more
            // variants.
            _ => (
                true,
                rune::SourceId::new(0),
                rune::ast::Span::new(0u32, 0u32),
                format!("{:?}", diag),
            ),
        };
        let (line, column) = byte_offset_to_linecol(sources, source_id, span.start.0 as usize);
        lines.push(format!(
            "{}:{}:{}: {}: {}",
            script_name,
            if line == 0 { "?".to_string() } else { line.to_string() },
            if column == 0 { "?".to_string() } else { column.to_string() },
            if is_error { "error" } else { "warning" },
            message.trim(),
        ));
        structured.push(CompileDiagnostic {
            source: script_name.to_string(),
            line, column,
            message: message.trim().to_string(),
            is_error,
        });
    }

    // Fallback: if no structured diagnostics landed, dump the
    // termcolor buffer so the user still sees something meaningful.
    if lines.is_empty() {
        let mut buf = rune::termcolor::Buffer::no_color();
        if diagnostics.emit(&mut buf, sources).is_ok() {
            let rendered = String::from_utf8_lossy(buf.as_slice()).trim().to_string();
            if !rendered.is_empty() { lines.push(rendered); }
        }
    }

    // Tack the build error on last — usually redundant with the
    // structured diagnostics, but surfaces parser-internal failures
    // that don't flow through `Diagnostics`.
    if let Some(e) = build_err {
        lines.push(format!("{}:?:?: error: {}", script_name, e));
    }

    if lines.is_empty() {
        lines.push(format!("{}:?:?: error: unknown compile failure", script_name));
    }

    (lines.join("\n"), structured)
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
                // Format the Rune `Diagnostics` into one
                // `file:line:col: error: message` per line so the
                // Output panel shows every diagnostic as its own row
                // with line numbers — not the terse
                // "Failed to build rune sources (see diagnostics for
                // details)" message the user complained about. We
                // intentionally do NOT `warn!()` the full text any
                // more: the terminal tracing stream isn't the right
                // place for script compile errors; the Output panel
                // is. Keep a single-line breadcrumb at debug level so
                // server logs still show *that* a compile failed.
                let (msg, _structured) = format_compile_diagnostics(
                    &script.name, &diagnostics, &sources, Some(&e as &dyn std::fmt::Display),
                );
                tracing::debug!("rune compile failed for '{}' — see Output panel", script.name);
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
            // Same policy as `compile_scripts`: push structured
            // `file:line:col: error: msg` lines into `last_errors`
            // so the Output-panel drainer shows each diagnostic as
            // its own row, and keep the tracing stream quiet
            // (debug-level breadcrumb only) so the terminal doesn't
            // duplicate what the Output panel already displays.
            let (msg, _structured) = format_compile_diagnostics(
                name, &diagnostics, &sources, Some(&e as &dyn std::fmt::Display),
            );
            tracing::debug!("rune hot-recompile failed for '{}' — see Output panel", name);
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
                    // ONLY drop "missing entry" errors — those mean the
                    // script legitimately didn't define `on_update` (the
                    // canonical signature check). Any other error
                    // (including runtime "method not found on f64",
                    // "field missing on <obj>", etc.) must surface —
                    // previously the filter `msg.contains("missing") ||
                    // msg.contains("not found")` was too broad, silently
                    // eating script bugs so the UI stayed frozen with
                    // no log trail. `Missing entry` is Rune's specific
                    // wording for an absent callback.
                    let is_missing_callback =
                        msg.contains("Missing entry `on_update`")
                        || msg.starts_with("Missing entry ");
                    if !is_missing_callback {
                        errors.push((compiled.name.clone(), msg));
                    }
                }
            }
        }

        // Errors surface via `runtime.last_errors` →
        // `drain_script_errors_to_output` → Eustress Output panel.
        // No `warn!` / terminal duplication — the Output panel is the
        // user-facing channel for script errors; the terminal is for
        // engine-internal diagnostics only.
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
