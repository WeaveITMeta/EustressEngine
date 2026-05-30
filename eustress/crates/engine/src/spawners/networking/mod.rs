//! # Networking spawners — Wave 5.B
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.11: the four
//! networking-signal classes (`RemoteEvent`, `RemoteFunction`,
//! `BindableEvent`, `BindableFunction`) that carry no mesh, no physics,
//! no material, no visual. They are **empty signal containers** — the
//! entity exists purely so the Luau runtime can resolve a named signal
//! (event / RPC) against it at runtime. The spawner attaches the
//! data-only ECS component (`name` + `enabled` + a diagnostic counter);
//! the live signal plumbing is wired by the Luau bridge, not authored at
//! spawn time.
//!
//! These are the simplest spawners in the project — closer to
//! [`FolderSpawner`][crate::spawners::containers::FolderSpawner] than to
//! any geometry/light spawner. No LOD model (empty bundle at every
//! tier), stub persistence (empty bytes / empty bag), and `apply_edit`
//! never requests a respawn.
//!
//! ## Why a sub-plugin
//!
//! Each `spawners::<group>` directory ships a self-contained Bevy
//! `Plugin` that registers its classes with the
//! [`ClassRegistry`][eustress_common::class_registry::ClassRegistry]
//! resource. Per `docs/process/AGENT_DISPATCH.md` LOOP 5: never add
//! resources to `StudioUiPlugin` (the legacy plugin) — register
//! everything via owned sub-plugins that the orchestrator wires into the
//! active `SpawnersPlugin` (Wave 5.E's job).
//!
//! ## Wave 5.E wiring (orchestrator's job, NOT this task)
//!
//! Wave 5.E adds `pub mod networking;` to `spawners/mod.rs` and bundles
//! [`NetworkingSpawnerPlugin`] into the parent `SpawnersPlugin` alongside
//! the other group sub-plugins. Order: `ClassRegistryPlugin` first (Wave
//! 2.3, already mounted) → `SpawnersPlugin` after, so the
//! `ClassRegistry` resource exists before any `register_class` call.
//!
//! Until 5.E lands, the files in this directory compile in isolation but
//! are not yet active in the engine. That's by design — spec §7.3 phases
//! the cutover one class at a time.
//!
//! ## What this sub-plugin does
//!
//! [`NetworkingSpawnerPlugin`] registers all four spawners with the
//! [`ClassRegistry`] resource. Each spawner is zero-sized;
//! `register_class::<S>()` allocates one `Box<dyn ClassSpawner>` per
//! class and inserts into the registry's
//! `HashMap<ClassName, Box<dyn ClassSpawner>>`.
//!
//! Registration is **idempotent** at the plugin level — Bevy
//! de-duplicates plugins by type, and the registry panics on a duplicate
//! `ClassName` key (the drift-bug guard). Adding this plugin twice is a
//! defence-in-depth panic, not the expected path.
//!
//! ## What this sub-plugin does NOT do
//!
//! - Does NOT initialise `ClassRegistry` itself (that's
//!   `ClassRegistryPlugin`'s job — this plugin assumes it exists).
//! - Does NOT touch `drain_slint_actions`, `StudioUiPlugin`, or any
//!   Slint UI state. Per LOOP 5: signal spawners are pure ECS.
//! - Does NOT introduce new Bevy resources — sidesteps the LOOP-5 drain
//!   assertion entirely.

pub mod bindable_event;
pub mod bindable_function;
pub mod remote_event;
pub mod remote_function;

pub use bindable_event::BindableEventSpawner;
pub use bindable_function::BindableFunctionSpawner;
pub use remote_event::RemoteEventSpawner;
pub use remote_function::RemoteFunctionSpawner;

use bevy::prelude::*;

use eustress_common::class_registry::RegisterClassExt;

/// Bevy plugin that registers every networking-signal [`ClassSpawner`] —
/// [`RemoteEventSpawner`], [`RemoteFunctionSpawner`],
/// [`BindableEventSpawner`], and [`BindableFunctionSpawner`].
///
/// Wave 5.E's parent `SpawnersPlugin` adds this plugin alongside the
/// other group sub-plugins (containers, lights, gui, …). See the module
/// docs for the wiring contract.
pub struct NetworkingSpawnerPlugin;

impl Plugin for NetworkingSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // All four spawners are `Default`-constructible (zero-sized) —
        // `register_class::<S>` handles instance creation + box + insert.
        //
        // Order is irrelevant per spec §6.3 — the registry is keyed by
        // `ClassName`, not insertion order; double-registration of the
        // same class panics at registration time.
        app.register_class::<RemoteEventSpawner>()
            .register_class::<RemoteFunctionSpawner>()
            .register_class::<BindableEventSpawner>()
            .register_class::<BindableFunctionSpawner>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};
    use eustress_common::classes::ClassName;

    /// Adding `NetworkingSpawnerPlugin` to an `App` registers all four
    /// networking spawners under their canonical `ClassName` keys.
    #[test]
    fn plugin_registers_all_four_networking_spawners() {
        let mut app = App::new();
        // `ClassRegistry` would normally be initialised by
        // `ClassRegistryPlugin`. Initialising it directly here keeps the
        // test free of the LOOP-5 startup-system dependency.
        app.init_resource::<ClassRegistry>();
        app.add_plugins(NetworkingSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();

        for class in [
            ClassName::RemoteEvent,
            ClassName::RemoteFunction,
            ClassName::BindableEvent,
            ClassName::BindableFunction,
        ] {
            assert!(
                registry.contains(class),
                "NetworkingSpawnerPlugin must register a spawner for {}",
                class.as_str()
            );
            // Each registered spawner's `class_name()` matches its key —
            // the same invariant the registry's `register` enforces via
            // panic, repeated here as a regression gate.
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }

        assert_eq!(
            registry.len(),
            4,
            "NetworkingSpawnerPlugin registers exactly four spawners — \
             any more means another group's plugin leaked in"
        );
    }

    /// The plugin must be safe to add on a pristine (empty) registry.
    #[test]
    fn plugin_works_on_pristine_registry() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();

        assert!(app.world().resource::<ClassRegistry>().is_empty());

        app.add_plugins(NetworkingSpawnerPlugin);

        assert_eq!(app.world().resource::<ClassRegistry>().len(), 4);
    }
}
