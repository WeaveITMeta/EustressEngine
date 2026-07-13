//! Script-plugin registration bridge — the data path from a script
//! plugin's calls (Luau `plugin:AddSection(...)`/`plugin:AddButton(...)`,
//! Rune `plugin_add_section(...)`/`plugin_add_button(...)`) to the
//! engine's `TabRegistry`.
//!
//! Both languages' closures/free-functions run OUTSIDE Bevy's
//! system-execution context — they cannot hold a `ResMut<TabRegistry>`.
//! The fix mirrors how the native `PluginApi` already solves the
//! identical problem for Rust plugins: a call pushes onto a plain `Vec`
//! (here, behind an `Arc<Mutex<_>>` since Luau closures must be `Send`),
//! and a separate Bevy system drains it into `TabRegistry` on its own
//! schedule.
//!
//! This module holds ONLY the shared data shape (no engine-crate types —
//! `common` cannot depend on `engine`). The engine-side drain, teardown
//! store, and `TabRegistry` writes live in
//! `engine/src/script_plugin_host.rs`.
//!
//! ## Why one bridge, not one per language
//! The Section/Button/Notify registration shape is language-agnostic —
//! the callback handle ([`CallbackHandle`]) is the *entire* structural
//! difference between a Luau `RegistryKey` and a Rune `SyncFunction`.
//! Unifying them means ONE queue, ONE drain system, ONE teardown store,
//! ONE Reload Plugins button covering both languages — not two parallel
//! pipelines that must be kept in lockstep by hand (this codebase already
//! paid for that failure mode once, in the duplicate-`StudioState` bug:
//! two definitions of the same state meant a drain wrote to the wrong
//! one and buttons silently broke).

use std::sync::{Arc, Mutex};

/// A stored plugin-button callback, one variant per authoring language.
/// This IS the only place a Luau plugin and a Rune plugin's registration
/// path differ — everything else in this module (registration shape,
/// queue, teardown) is shared.
pub enum CallbackHandle {
    /// A Luau closure, stored in the `mlua::Lua` registry (see
    /// `LuauRuntime::build_plugin_environment`). Owned by whichever
    /// `mlua::Lua` instance created it — never move it across VMs.
    #[cfg(feature = "luau")]
    Lua(mlua::RegistryKey),
    /// A Rune closure, converted via `Function::into_sync()` at
    /// registration time so it can be stored past the call that created
    /// it (see `engine::soul::rune_ecs_module::plugin_add_button`).
    /// Unlike a Luau `RegistryKey`, this needs no explicit release on
    /// teardown — dropping it is enough.
    #[cfg(feature = "realism-scripting")]
    Rune(rune::runtime::SyncFunction),
}

impl std::fmt::Debug for CallbackHandle {
    // Neither `mlua::RegistryKey` nor `rune::runtime::SyncFunction`
    // implement `Debug` — a stable opaque marker per variant is all any
    // caller needs; nothing inspects a callback's contents.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "luau")]
            CallbackHandle::Lua(_) => f.write_str("CallbackHandle::Lua(..)"),
            #[cfg(feature = "realism-scripting")]
            CallbackHandle::Rune(_) => f.write_str("CallbackHandle::Rune(..)"),
        }
    }
}

/// One registration call queued by a script plugin (Luau's `plugin` table
/// or Rune's `plugin_*` free functions). `plugin_id` self-attributes every
/// entry so the engine-side drain can route it to the right teardown
/// bucket without the caller needing any engine-crate type.
///
/// `Debug` only, NOT `Clone` — no `CallbackHandle` variant is `Clone`,
/// and nothing here actually needs to clone a pending registration: the
/// drain system moves values out of the queue via `std::mem::take` and
/// consumes each by value.
#[derive(Debug)]
pub enum PendingPluginRegistration {
    /// There is only ever ONE "plugins" tab (registered natively at
    /// Startup) — script plugins add sections/buttons to it, they never
    /// register a new tab. No `Tab` variant on purpose: `TabRegistry::
    /// register_tab` doesn't dedupe by id, so a script calling it would
    /// silently create a duplicate the render side never sees.
    Section { plugin_id: String, tab_id: String, section_id: String, label: String },
    Button {
        plugin_id: String,
        tab_id: String,
        section_id: String,
        button_id: String,
        label: String,
        icon: Option<String>,
        tooltip: String,
        action_id: String,
        /// "small" | "normal" | "large" — a plain string, not
        /// `studio_plugins::tab_api::TabButtonSize`, since `common` cannot
        /// depend on `engine`; parsed engine-side on drain.
        size: String,
        /// The callback to invoke when this button's action fires, stored
        /// so it survives past this call. See [`CallbackHandle`].
        callback: CallbackHandle,
    },
    Notify { plugin_id: String, level: String, message: String },
}

/// Read-only cache a Bevy system refreshes each frame from the real
/// selection manager, and `plugin:GetSelection()` / `plugin_get_selection()`
/// read synchronously — the opposite direction from `PluginBridge` (engine
/// → script, not script → engine), so it's a plain shared `Vec`, not a
/// pending queue.
pub type SelectionCache = Arc<Mutex<Vec<String>>>;

pub fn new_selection_cache() -> SelectionCache {
    Arc::new(Mutex::new(Vec::new()))
}

/// Shared queue a plugin (either language) pushes into and the
/// engine-side discovery/drain system reads from. `Arc<Mutex<_>>` (not a
/// channel) — the queue is drained in full each time, not item-by-item
/// streamed.
pub type PluginBridge = Arc<Mutex<Vec<PendingPluginRegistration>>>;

/// Construct a fresh, empty bridge queue.
pub fn new_plugin_bridge() -> PluginBridge {
    Arc::new(Mutex::new(Vec::new()))
}
