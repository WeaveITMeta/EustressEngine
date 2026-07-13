//! Script-authored Studio plugins — Luau (Phase 2) and Rune (Phase 3).
//!
//! Phase 1 (`road_tool.rs`) proved the native `StudioPlugin`/`PluginApi`/
//! `TabRegistry` pipeline end-to-end with a Rust plugin. This module is the
//! second, independent producer into the SAME `TabRegistry` — a `.lua` or
//! `.rune` file dropped into `%LOCALAPPDATA%/Eustress/Plugins/` gets
//! discovered, executed/compiled once, and its registration calls
//! (`plugin:AddSection`/`plugin:AddButton` in Luau,
//! `plugin_add_section`/`plugin_add_button` in Rune) land on the shared
//! "plugins" tab alongside the native Road Builder section — no Rust
//! recompile. Both languages push onto the SAME `PluginBridge` queue (see
//! `eustress_common::script_plugins`) via `CallbackHandle` — one drain, one
//! teardown store, one Reload Plugins button for both.
//!
//! ## Why a global directory, not per-Space scripts
//! A plugin is installed editor tooling (Roblox's actual model —
//! `%LOCALAPPDATA%\Roblox\Plugins`, not something a place/world ships with).
//! Per-Space scripts would mean a downloaded/opened Space silently runs
//! privileged plugin code (this engine's Luau globals already give any
//! script unrestricted `HttpService`/`DataStoreService` access) the moment
//! it's opened — a real trust-boundary regression, not a convenience.
//!
//! ## Why one shared VM with per-plugin environments, not a second VM
//! `mlua`'s `sandbox(true)` does NOT give per-chunk write isolation (a
//! chunk's top-level global writes land on the one shared `lua.globals()`
//! table) — verified against the actual mlua 0.10.5 source, not assumed. A
//! second VM doesn't fix inter-plugin collisions either (two plugins in one
//! second VM still share ITS globals) and permanently doubles the ~20
//! `inject_*` service-exposure functions. The real fix is
//! `LuauRuntime::build_plugin_environment` (see `common/src/luau/runtime.rs`):
//! each plugin gets its own environment table via `Chunk::set_environment`,
//! with an `__index` metatable falling through to the real shared globals.
//!
//! ## Why the teardown store, not just registration
//! Registering is the easy half. A working **Reload Plugins** button — the
//! entire v1 dev loop, since there's no directory file-watcher yet — has to
//! remove exactly one plugin's own `TabRegistry` sections/buttons and
//! release its stored Lua callbacks (`RegistryKey`s are real, permanent
//! leaks in a long-lived shared VM if never removed) before re-running
//! discovery. `ScriptPluginRegistry` exists to make that removal precise.

use std::collections::HashMap;

use bevy::prelude::*;

use eustress_common::luau::LuauRuntimeState;
use eustress_common::script_plugins::{
    new_plugin_bridge, new_selection_cache, CallbackHandle, PendingPluginRegistration, PluginBridge, SelectionCache,
};

use crate::notifications::NotificationManager;
use crate::studio_plugins::tab_api::{TabButton, TabButtonSize, TabRegistry, TabSection};
use crate::studio_plugins::{PluginActionEvent, PluginApi, PluginCategory, PluginInfo, StudioPlugin};

/// The one shared ribbon tab every plugin (native or script) contributes
/// to — see Phase 1's decision to ship exactly one static "Plugins" tab.
const PLUGINS_TAB_ID: &str = "plugins";

// ============================================================================
// Resources
// ============================================================================

/// What ONE script plugin has registered — lets Reload tear down exactly
/// its own contributions to the shared "plugins" tab, leaving every other
/// plugin's (native or script) sections/buttons untouched.
#[derive(Default)]
struct ScriptPluginRegistrations {
    section_ids: Vec<String>,
    /// (section_id, button_id) — precise enough to remove one button
    /// without assuming it owns the whole section.
    button_ids: Vec<(String, String)>,
    action_ids: Vec<String>,
}

#[derive(Resource, Default)]
pub struct ScriptPluginRegistry {
    plugins: HashMap<String, ScriptPluginRegistrations>,
}

/// Cloned into every plugin's environment closures at discovery time (see
/// `LuauRuntime::build_plugin_environment`) — the queue a `plugin:AddSection`/
/// `plugin:AddButton`/`plugin:Notify` call pushes onto, since those closures
/// run outside Bevy's system-execution context and can't hold `ResMut`.
#[derive(Resource, Clone)]
pub struct ScriptPluginBridgeRes(pub PluginBridge);
impl Default for ScriptPluginBridgeRes {
    fn default() -> Self {
        Self(new_plugin_bridge())
    }
}

/// A read-only cache `update_selection_cache_for_plugins` refreshes each
/// frame from the real selection manager; `plugin:GetSelection()` reads it
/// synchronously. Opposite direction from the bridge above (engine→script).
#[derive(Resource, Clone)]
pub struct SelectionCacheRes(pub SelectionCache);
impl Default for SelectionCacheRes {
    fn default() -> Self {
        Self(new_selection_cache())
    }
}

/// The single owner of every button callback's [`CallbackHandle`], keyed
/// by `action_id`, covering BOTH authoring languages. Dispatch looks
/// callbacks up here; teardown removes them from here AND, for a
/// `CallbackHandle::Lua`, from the Lua registry itself (`LuauRuntime::
/// remove_registry_value`) — a `RegistryKey` dropped without that call
/// leaks in the VM forever, it does not get garbage-collected on its own.
/// A `CallbackHandle::Rune` needs no such release — dropping the
/// `SyncFunction` is enough.
#[derive(Resource, Default)]
pub struct ScriptPluginCallbacks {
    by_action_id: HashMap<String, CallbackHandle>,
}

/// One-shot latch, mirroring `InsertClassesInitialized`'s pattern exactly —
/// discovery can't run at `Startup` (the Luau runtime itself only
/// initializes on the first `Update` tick, per `LuauPlugin::build`), so this
/// gates a `Update`-schedule system to run its real work exactly once, as
/// soon as the runtime is ready, however many frames that takes.
#[derive(Resource, Default)]
struct ScriptPluginsDiscovered(bool);

// ============================================================================
// Bevy Plugin wiring
// ============================================================================

pub struct ScriptPluginHostPlugin;

impl Plugin for ScriptPluginHostPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptPluginRegistry>()
            .init_resource::<ScriptPluginBridgeRes>()
            .init_resource::<SelectionCacheRes>()
            .init_resource::<ScriptPluginCallbacks>()
            .init_resource::<ScriptPluginsDiscovered>()
            .add_systems(
                Update,
                (
                    discover_script_plugins,
                    drain_script_plugin_bridge.after(discover_script_plugins),
                    update_selection_cache_for_plugins,
                ),
            )
            .add_systems(
                Update,
                (
                    handle_script_plugin_actions.after(crate::ui::slint_ui::SlintSystems::Drain),
                    handle_reload_plugins_action.after(crate::ui::slint_ui::SlintSystems::Drain),
                ),
            );
    }
}

// ============================================================================
// Discovery
// ============================================================================

/// One-shot: enumerate `%LOCALAPPDATA%/Eustress/Plugins/*.lua` + `*.rune`
/// (+ `<dir>/init.lua` for folder-style Luau plugins) and load each. Luau
/// files execute with their own isolated environment; Rune files compile
/// via the real `rune_runtime` path and call their top-level `register()`
/// function. Never runs its body twice — see `ScriptPluginsDiscovered`.
///
/// LAZILY CREATES the Luau VM if it doesn't exist yet: the EDITOR never
/// adds common's `LuauPlugin` (whose first-Update init this system
/// originally waited on) — in the editor the VM is otherwise only created
/// at Play start (`start_luau_scripts_on_play`), so waiting would mean
/// script plugins never load in Edit mode. Verified live 2026-07-13: a
/// full Edit session logged zero "Luau runtime initialized" lines.
/// (`RuneModuleRegistry` needs no such wait — populated at `Startup`.)
fn discover_script_plugins(
    mut discovered: ResMut<ScriptPluginsDiscovered>,
    mut runtime_state: ResMut<LuauRuntimeState>,
    module_registry: Res<crate::soul::rune_api::RuneModuleRegistry>,
    bridge: Res<ScriptPluginBridgeRes>,
    selection: Res<SelectionCacheRes>,
    mut notifications: ResMut<NotificationManager>,
) {
    if discovered.0 {
        return;
    }
    if runtime_state.runtime.is_none() {
        match eustress_common::luau::runtime::LuauRuntime::new() {
            Ok(rt) => {
                runtime_state.runtime = Some(rt);
                runtime_state.initialized = true;
                info!("script_plugin_host: Luau VM lazily initialized for Edit-mode plugin discovery");
            }
            Err(e) => {
                // Mark discovered so a broken VM doesn't retry (and re-warn)
                // every frame; Reload Plugins re-arms for another attempt.
                discovered.0 = true;
                notifications.error(format!("Script plugins disabled: Luau VM init failed: {e}"));
                return;
            }
        }
    }
    let Some(ref mut runtime) = runtime_state.runtime else {
        return; // unreachable after the lazy init above; kept as a guard
    };
    discovered.0 = true; // Whether or not any plugins are found, this runs exactly once.

    let Some(plugins_dir) = dirs::data_local_dir().map(|d| d.join("Eustress").join("Plugins")) else {
        warn!("script_plugin_host: could not resolve a local-data directory — script plugins disabled this session");
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&plugins_dir) {
        warn!("script_plugin_host: could not create {:?}: {}", plugins_dir, e);
        return;
    }

    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("script_plugin_host: could not read {:?}: {}", plugins_dir, e);
            return;
        }
    };

    let mut discovered_count = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        // Folder-style plugins are Luau-only for now (`init.lua`) — a
        // flat single file can be either language, told apart by
        // extension.
        let (script_path, is_rune) = if path.is_dir() {
            let init = path.join("init.lua");
            if init.is_file() { (init, false) } else { continue }
        } else {
            match path.extension().and_then(|e| e.to_str()) {
                Some("lua") => (path.clone(), false),
                Some("rune") => (path.clone(), true),
                _ => continue,
            }
        };

        let plugin_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown_plugin".to_string());

        let source = match std::fs::read_to_string(&script_path) {
            Ok(s) => s,
            Err(e) => {
                notifications.error(format!("Plugin '{plugin_id}': could not read {:?}: {e}", script_path));
                continue;
            }
        };

        if is_rune {
            match compile_and_run_rune_plugin(&source, &plugin_id, &module_registry, bridge.0.clone(), selection.0.clone()) {
                Ok(()) => {
                    discovered_count += 1;
                    info!("🔌 Script plugin loaded (Rune): {plugin_id} ({:?})", script_path);
                }
                Err(e) => {
                    notifications.error(format!("Plugin '{plugin_id}' failed to load: {e}"));
                }
            }
            continue;
        }

        let env = match runtime.build_plugin_environment(bridge.0.clone(), selection.0.clone(), plugin_id.clone()) {
            Ok(env) => env,
            Err(e) => {
                notifications.error(format!("Plugin '{plugin_id}': environment setup failed: {e}"));
                continue;
            }
        };

        match runtime.execute_chunk_with_env(&source, &plugin_id, env) {
            Ok(()) => {
                discovered_count += 1;
                info!("🔌 Script plugin loaded (Luau): {plugin_id} ({:?})", script_path);
            }
            Err(e) => {
                notifications.error(format!("Plugin '{plugin_id}' failed to load: {e}"));
            }
        }
    }

    if discovered_count > 0 {
        info!("🔌 Script plugin discovery: {discovered_count} plugin(s) loaded from {:?}", plugins_dir);
    }
}

/// Compile a `.rune` plugin source via the REAL compile path
/// (`rune::prepare(...).build()` — the same one `common::soul::
/// rune_runtime::compile_scripts` uses for Soul Scripts, just run
/// standalone here since plugins load in Edit mode, not just Play mode)
/// and call its top-level `register()` function with this plugin's
/// identity pushed onto the thread-local stack. Unlike Luau's implicit
/// top-to-bottom chunk execution, Rune requires an explicit function —
/// a plugin's registration calls (`plugin_add_section`/`plugin_add_button`)
/// belong in `pub fn register() { ... }`.
///
/// Never calls the AI build pipeline (`build_pipeline.rs`) or the Claude
/// API — a `.rune` PLUGIN ships as already-generated source, exactly like
/// a `.lua` plugin ships as already-written source. Compile diagnostics
/// (syntax errors etc.) are returned as a plugin-attributed message
/// rather than surfacing as a panic or a silent no-op.
#[cfg(feature = "realism-scripting")]
fn compile_and_run_rune_plugin(
    source: &str,
    plugin_id: &str,
    module_registry: &crate::soul::rune_api::RuneModuleRegistry,
    bridge: PluginBridge,
    selection: SelectionCache,
) -> Result<(), String> {
    let rune_context = module_registry.build_context()?;
    let runtime_ctx = rune_context.runtime().map_err(|e| format!("runtime context build failed: {e}"))?;

    let mut sources = rune::Sources::new();
    let src = rune::Source::memory(source).map_err(|e| format!("source error: {e}"))?;
    sources.insert(src).map_err(|e| format!("source insert error: {e}"))?;

    let mut diagnostics = rune::Diagnostics::new();
    let build_result = rune::prepare(&mut sources)
        .with_context(&rune_context)
        .with_diagnostics(&mut diagnostics)
        .build();

    let unit = match build_result {
        Ok(unit) => unit,
        Err(e) => {
            let (msg, _structured) = eustress_common::soul::rune_runtime::format_compile_diagnostics(
                plugin_id, &diagnostics, &sources, Some(&e as &dyn std::fmt::Display),
            );
            return Err(msg);
        }
    };

    let mut vm = rune::Vm::new(std::sync::Arc::new(runtime_ctx), std::sync::Arc::new(unit));
    let _guard = crate::soul::rune_ecs_module::PluginContextGuard::new(
        plugin_id.to_string(), bridge, selection,
    );
    match vm.call(["register"], ()) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!(
            "register() error: {e} (every Rune plugin must define `pub fn register()` — \
             that's where plugin_add_section/plugin_add_button calls belong)"
        )),
    }
}

#[cfg(not(feature = "realism-scripting"))]
fn compile_and_run_rune_plugin(
    _source: &str,
    _plugin_id: &str,
    _module_registry: &crate::soul::rune_api::RuneModuleRegistry,
    _bridge: PluginBridge,
    _selection: SelectionCache,
) -> Result<(), String> {
    Err("Rune plugin support requires the realism-scripting feature".to_string())
}

// ============================================================================
// Drain — pending registrations → TabRegistry + teardown store
// ============================================================================

/// Every frame, drain whatever `plugin:AddSection`/`AddButton`/`Notify`
/// calls have queued since the last drain (discovery calls these
/// synchronously during `execute_chunk_with_env`, so the first drain after
/// discovery picks up everything a plugin registered at load time; a
/// future Reload does the same for a fresh discovery pass).
fn drain_script_plugin_bridge(
    bridge: Res<ScriptPluginBridgeRes>,
    mut tab_registry: ResMut<TabRegistry>,
    mut registry: ResMut<ScriptPluginRegistry>,
    mut callbacks: ResMut<ScriptPluginCallbacks>,
    mut notifications: ResMut<NotificationManager>,
) {
    let pending: Vec<PendingPluginRegistration> = {
        let Ok(mut queue) = bridge.0.lock() else { return };
        if queue.is_empty() {
            return;
        }
        std::mem::take(&mut *queue)
    };

    for item in pending {
        match item {
            PendingPluginRegistration::Section { plugin_id, tab_id, section_id, label } => {
                if tab_id != PLUGINS_TAB_ID {
                    notifications.warning(format!(
                        "Plugin '{plugin_id}': AddSection targeted tab '{tab_id}', only \"{PLUGINS_TAB_ID}\" exists — ignored"
                    ));
                    continue;
                }
                tab_registry.add_section(&tab_id, TabSection {
                    name: section_id.clone(),
                    id: section_id.clone(),
                    label,
                    buttons: Vec::new(),
                    collapsible: false,
                    collapsed: false,
                });
                registry.plugins.entry(plugin_id).or_default().section_ids.push(section_id);
            }
            PendingPluginRegistration::Button { plugin_id, tab_id, section_id, button_id, label, icon, tooltip, action_id, size, callback } => {
                if tab_id != PLUGINS_TAB_ID {
                    notifications.warning(format!(
                        "Plugin '{plugin_id}': AddButton targeted tab '{tab_id}', only \"{PLUGINS_TAB_ID}\" exists — ignored"
                    ));
                    continue;
                }
                if callbacks.by_action_id.contains_key(&action_id) {
                    notifications.warning(format!(
                        "Plugin '{plugin_id}': action id '{action_id}' is already registered by another button — the newest registration wins"
                    ));
                }
                let size = match size.as_str() {
                    "small" => TabButtonSize::Small,
                    "large" => TabButtonSize::Large,
                    "medium" => TabButtonSize::Medium,
                    _ => TabButtonSize::Normal,
                };
                tab_registry.add_button(&tab_id, &section_id, TabButton {
                    label,
                    icon,
                    action: action_id.clone(),
                    size,
                    id: button_id.clone(),
                    tooltip: Some(tooltip),
                    action_id: action_id.clone(),
                });
                callbacks.by_action_id.insert(action_id.clone(), callback);
                let entry = registry.plugins.entry(plugin_id).or_default();
                entry.button_ids.push((section_id, button_id));
                entry.action_ids.push(action_id);
            }
            PendingPluginRegistration::Notify { plugin_id, level, message } => {
                let text = format!("[{plugin_id}] {message}");
                match level.as_str() {
                    "success" => notifications.success(text),
                    "warning" => notifications.warning(text),
                    "error" => notifications.error(text),
                    _ => notifications.info(text),
                }
            }
        }
    }
}

/// Refreshes the selection cache `plugin:GetSelection()` reads. Mirrors the
/// existing native-plugin selection sync (`sync_mindspace_selection`,
/// `studio_plugins/mod.rs`) but writes to the script-plugin-facing cache
/// instead of a `PluginApi.selected_entities` field.
fn update_selection_cache_for_plugins(
    selection: Res<SelectionCacheRes>,
    selection_manager: Option<Res<crate::rendering::BevySelectionManager>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let selected = selection_manager.0.read().get_selected();
    if let Ok(mut cache) = selection.0.lock() {
        *cache = selected;
    }
}

// ============================================================================
// Dispatch — PluginActionEvent -> stored Lua callback
// ============================================================================

/// Independent `MessageReader<PluginActionEvent>` cursor (Bevy Messages
/// support multiple readers) — does not touch `handle_plugin_action_events`
/// (native "mindspace:"/"soul:" arms) or `handle_road_tool_actions`
/// ("road:*" arms); this one only fires a stored callback if the
/// action_id is one a script plugin registered, dispatching to the Luau
/// or Rune runtime depending on which `CallbackHandle` variant is stored.
fn handle_script_plugin_actions(
    mut events: MessageReader<PluginActionEvent>,
    callbacks: Res<ScriptPluginCallbacks>,
    registry: Res<ScriptPluginRegistry>,
    mut runtime_state: ResMut<LuauRuntimeState>,
    bridge: Res<ScriptPluginBridgeRes>,
    selection: Res<SelectionCacheRes>,
    mut notifications: ResMut<NotificationManager>,
) {
    for event in events.read() {
        let Some(handle) = callbacks.by_action_id.get(&event.action_id) else { continue };
        match handle {
            CallbackHandle::Lua(key) => {
                let Some(ref runtime) = runtime_state.runtime else { continue };
                if let Err(e) = runtime.call_plugin_callback(key) {
                    notifications.error(format!("Plugin action '{}' failed: {e}", event.action_id));
                }
            }
            #[cfg(feature = "realism-scripting")]
            CallbackHandle::Rune(sync_fn) => {
                // Attribute the callback to its owning plugin so a call
                // to `plugin_notify`/`plugin_add_button` mid-click
                // self-attributes correctly — same reason discovery-time
                // execution pushes a guard.
                let plugin_id = registry.plugins.iter()
                    .find(|(_, regs)| regs.action_ids.iter().any(|a| a == &event.action_id))
                    .map(|(id, _)| id.clone())
                    .unwrap_or_else(|| "unknown_plugin".to_string());
                let _guard = crate::soul::rune_ecs_module::PluginContextGuard::new(
                    plugin_id, bridge.0.clone(), selection.0.clone(),
                );
                // `SyncFunction::call` returns rune's own `VmResult<T>` —
                // a distinct enum from `std::result::Result` despite the
                // matching `Ok`/`Err` variant names, so a bare `if let
                // Err(e) = ...` resolves `Err` via the prelude's
                // `Result::Err` and mismatches. `.into_result()` converts
                // to a real `Result<(), VmError>` first.
                if let Err(e) = sync_fn.call::<()>(()).into_result() {
                    notifications.error(format!("Plugin action '{}' failed: {e}", event.action_id));
                }
            }
        }
    }
    let _ = &mut runtime_state; // ResMut only needed for the Option check above; no mutation.
}

// ============================================================================
// Reload Plugins — native micro-plugin, the v1 dev loop
// ============================================================================

/// Registers ONLY the "Reload Plugins" button — a tiny native `StudioPlugin`
/// separate from `RoadToolPlugin` (which is about roads, not plugin-host
/// meta-controls) and separate from the script-plugin machinery above (this
/// button is native so it exists even with zero script plugins installed).
#[derive(Default)]
pub struct PluginHostControlsPlugin;

impl StudioPlugin for PluginHostControlsPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "plugin-host-controls".to_string(),
            name: "Plugin Host Controls".to_string(),
            version: "0.1.0".to_string(),
            author: "Eustress".to_string(),
            description: "Reload script-authored (Luau + Rune) plugins without restarting the engine.".to_string(),
            icon: None,
            category: PluginCategory::Utility,
            permissions: Vec::new(),
        }
    }

    fn on_enable(&mut self, api: &mut PluginApi) {
        api.register_tab(PLUGINS_TAB_ID, "Plugins", None::<String>, 0, "plugin-host-controls");
        api.add_tab_section(PLUGINS_TAB_ID, "plugin-host", "Plugin Host");
        api.add_tab_button(PLUGINS_TAB_ID, "plugin-host", "reload-plugins", "Reload Plugins", Some("~"),
            "Tear down and re-run every script-authored (.lua/.rune) plugin", "pluginhost:reload", crate::studio_plugins::TabButtonSize::Normal);
    }
}

/// Handles `"pluginhost:reload"`: tears down every SCRIPT plugin's own
/// `TabRegistry` sections/buttons and releases its stored callbacks (for
/// a `CallbackHandle::Lua`, both the `ScriptPluginCallbacks` entry AND the
/// underlying `RegistryKey` — dropping a `RegistryKey` value does not free
/// its VM-side slot, `remove_registry_value` does; a `CallbackHandle::Rune`
/// needs no such release, dropping the `SyncFunction` is enough), then
/// re-arms discovery. Native plugins (Road Builder, this very button) are
/// untouched — only entries tracked in `ScriptPluginRegistry` are removed.
pub fn handle_reload_plugins_action(
    mut events: MessageReader<PluginActionEvent>,
    mut tab_registry: ResMut<TabRegistry>,
    mut registry: ResMut<ScriptPluginRegistry>,
    mut callbacks: ResMut<ScriptPluginCallbacks>,
    mut discovered: ResMut<ScriptPluginsDiscovered>,
    mut runtime_state: ResMut<LuauRuntimeState>,
    mut notifications: ResMut<NotificationManager>,
) {
    for event in events.read() {
        if event.action_id != "pluginhost:reload" {
            continue;
        }

        let plugin_ids: Vec<String> = registry.plugins.keys().cloned().collect();
        for plugin_id in &plugin_ids {
            let Some(entry) = registry.plugins.remove(plugin_id) else { continue };
            for section_id in &entry.section_ids {
                tab_registry.remove_section(PLUGINS_TAB_ID, section_id);
            }
            for (section_id, button_id) in &entry.button_ids {
                tab_registry.remove_button(PLUGINS_TAB_ID, section_id, button_id);
            }
            for action_id in &entry.action_ids {
                if let Some(handle) = callbacks.by_action_id.remove(action_id) {
                    match handle {
                        CallbackHandle::Lua(key) => {
                            if let Some(ref runtime) = runtime_state.runtime {
                                if let Err(e) = runtime.remove_registry_value(key) {
                                    warn!("Reload: failed to release callback for '{action_id}': {e}");
                                }
                            }
                        }
                        #[cfg(feature = "realism-scripting")]
                        CallbackHandle::Rune(_) => {
                            // No explicit release needed — dropping the
                            // `SyncFunction` here is sufficient.
                        }
                    }
                }
            }
        }

        discovered.0 = false; // re-arm discover_script_plugins for one more pass
        notifications.info(format!("Reloading {} script plugin(s)...", plugin_ids.len()));
    }
}
