//! # Scripting-class `ClassSpawner` implementations (Wave 5.C)
//!
//! One spawner per scripting class per `CLASS_REGISTRY.md` §8.12.
//!
//! ## Coverage
//!
//! | `ClassName` | Spawner | Source-carrying component | Source today |
//! |---|---|---|---|
//! | `SoulScript`           | [`SoulScriptSpawner`]           | `crate::soul::SoulScriptData`        | `file_loader.rs:674` / `:1471` |
//! | `LuauScript`           | [`LuauScriptSpawner`]           | `eustress_common::luau::LuauScript`  | `file_loader.rs:753` |
//! | `LuauLocalScript`      | [`LuauLocalScriptSpawner`]      | `eustress_common::luau::LuauLocalScript` | (uses LuauScript path) |
//! | `LuauModuleScript`     | [`LuauModuleScriptSpawner`]     | `eustress_common::luau::LuauModuleScript` | (uses LuauScript path) |
//! | `WorkshopConversation` | [`WorkshopConversationSpawner`] | `crate::soul::SoulScriptData` (transcript) | `workshop/persistence.rs` (stub) |
//!
//! ## The one rule: store the source, never execute it
//!
//! Every spawner here is a **pure source carrier**. It builds the entity,
//! stamps `Instance` + `Name` + `Transform` + `Visibility`, and writes the
//! script body onto the class's source-carrying component. It does NOT
//! touch the Rune VM, the mlua runtime, or the Soul build pipeline — those
//! existing systems detect the script component (`Added`/`Changed`) and
//! drive compile/run themselves. Script *source* edits flow through the
//! file-watcher reload path, so every `apply_edit` returns `false` (never
//! request a respawn).
//!
//! ## Why no LOD / no visual
//!
//! Scripts are invisible. `RENDER_CASCADE` / spec §2.1 list `Script`
//! alongside `Sound` and `Folder` as classes with "no horizon
//! representation". Every spawner's [`ClassSpawner::lod_components`] returns
//! [`ComponentBundle::empty`](eustress_common::class_registry::ComponentBundle::empty)
//! for all four tiers, short-circuiting the LOD transition system.
//!
//! ## Persistence is stubbed (Wave 5.C)
//!
//! `serialize` returns an empty `Vec<u8>` and `deserialize` returns an empty
//! `PropertyBag`, matching the Wave-3 container/audio spawners. Script
//! bodies round-trip through their on-disk source files + `_instance.toml`;
//! the script-group Fjall rkyv mirror lights up in a later wave. The empty
//! path is safe per spec §10 R9 — the worlddb write path skips classes
//! whose `serialize` yields no bytes.
//!
//! ## Mount point
//!
//! Wave 5.E (orchestrator-only) adds `pub mod scripting;` to
//! `spawners/mod.rs` and [`ScriptingSpawnerPlugin`] to the engine's plugin
//! graph. Until then this module is dead code and the legacy `file_loader`
//! match arms keep being the runtime spawn path.
//!
//! ## LOOP 5 — drain resource discipline
//!
//! None of the spawners register a new Bevy `Resource`. Every spawner is a
//! zero-sized `Default` unit struct; per-spawn data flows through the
//! `PropertyBag` argument. Nothing here touches `drain_slint_actions`.

use bevy::prelude::*;

use eustress_common::class_registry::RegisterClassExt;

pub mod luau_local_script;
pub mod luau_module_script;
pub mod luau_script;
pub mod soul_script;
pub mod workshop_conversation;

pub use luau_local_script::LuauLocalScriptSpawner;
pub use luau_module_script::LuauModuleScriptSpawner;
pub use luau_script::LuauScriptSpawner;
pub use soul_script::SoulScriptSpawner;
pub use workshop_conversation::WorkshopConversationSpawner;

/// Bevy plugin that registers all five scripting spawners with the
/// `ClassRegistry`.
///
/// Self-contained: registration via
/// [`RegisterClassExt::register_class`] is the only side effect. The
/// orchestrator's Wave 5.E commit mounts this plugin exactly once, after
/// [`crate::class_registry::ClassRegistryPlugin`] has run (which
/// `init_resource::<ClassRegistry>`'d the registry).
///
/// Registration order is irrelevant per spec §6.3 — the registry is keyed
/// by `ClassName` and panics on double-registration, so the only real
/// failure mode is forgetting a plugin entirely (the startup-time
/// `log_registry_validation` from Wave 2.3 catches that).
pub struct ScriptingSpawnerPlugin;

impl Plugin for ScriptingSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // SoulScript first — it's the reference impl every other scripting
        // spawner mirrors. All five are `Default`-constructible (zero-sized);
        // `register_class::<S>` handles the instance creation + box + insert.
        app.register_class::<SoulScriptSpawner>()
            .register_class::<LuauScriptSpawner>()
            .register_class::<LuauLocalScriptSpawner>()
            .register_class::<LuauModuleScriptSpawner>()
            .register_class::<WorkshopConversationSpawner>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::ClassRegistry;
    use eustress_common::classes::ClassName;

    /// All five spawners register without panic and the registry ends up
    /// with exactly five entries when the plugin is mounted standalone.
    /// Deliverable #2/#3 from the task brief.
    #[test]
    fn plugin_registers_all_five_scripting_classes() {
        let mut app = App::new();
        // `ClassRegistry` would normally be initialised by
        // `ClassRegistryPlugin`. Initialising it directly here keeps the
        // test free of the LOOP-5 startup-system dependency (mirrors the
        // containers-group test).
        app.init_resource::<ClassRegistry>();
        app.add_plugins(ScriptingSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();

        assert_eq!(
            registry.len(),
            5,
            "ScriptingSpawnerPlugin must register exactly SoulScript, LuauScript, \
             LuauLocalScript, LuauModuleScript, and WorkshopConversation"
        );

        for class in [
            ClassName::SoulScript,
            ClassName::LuauScript,
            ClassName::LuauLocalScript,
            ClassName::LuauModuleScript,
            ClassName::WorkshopConversation,
        ] {
            assert!(
                registry.contains(class),
                "registry must contain a spawner for {}",
                class.as_str()
            );
        }
    }
}
