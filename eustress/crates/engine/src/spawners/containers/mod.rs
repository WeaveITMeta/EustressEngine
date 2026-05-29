//! # Container spawners — Wave 3.E
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.1: the two
//! organizational-container classes (Folder + Model) that carry no
//! mesh, no physics, no material, no visual. Children inherit transform
//! / visibility / LOD via their own spawners; the container contributes
//! hierarchy + tags + attributes (+ for Model: primary-part reference
//! and world pivot).
//!
//! ## Why a sub-plugin
//!
//! Each `spawners::<group>` directory ships a self-contained Bevy
//! `Plugin` that registers its classes with the
//! [`ClassRegistry`][eustress_common::class_registry::ClassRegistry]
//! resource. Per `docs/process/AGENT_DISPATCH.md` LOOP 5: never add
//! resources to `StudioUiPlugin` (the legacy plugin) — register
//! everything via owned sub-plugins that the orchestrator wires into
//! the active `SlintUiPlugin` (or, in Wave 3, into the parent
//! `SpawnersPlugin` that task 3.A creates).
//!
//! ## Wave 3 wiring (task 3.A's job)
//!
//! Task 3.A creates `crates/engine/src/spawners/mod.rs` which:
//!
//! 1. Declares `pub mod containers;` (the line that pulls this module
//!    into the crate tree — without it the containers files are dead
//!    weight on disk).
//! 2. Declares one `pub mod <group>;` per spawner group (lights, gui,
//!    constraints, …).
//! 3. Bundles all sub-plugins into a single `SpawnersPlugin` that
//!    `SlintUiPlugin::build` adds. Order: `ClassRegistryPlugin` first
//!    (Wave 2.3, already mounted) → `SpawnersPlugin` after, so the
//!    `ClassRegistry` resource exists before any `register_class` call.
//!
//! Until 3.A lands, the files in this directory compile in isolation
//! (verified via `cargo check -p eustress-engine` once `mod containers`
//! is reachable) but are not yet active in the engine. That's by
//! design: spec §7.3 phases the cutover one class at a time behind the
//! `class-registry` cargo feature.
//!
//! ## What this sub-plugin does
//!
//! [`ContainersSpawnerPlugin`] registers [`FolderSpawner`] and
//! [`ModelSpawner`] with the [`ClassRegistry`] resource. That's it.
//! Both spawners are zero-sized; `register_class::<S>()` allocates one
//! `Box<dyn ClassSpawner>` per class and inserts into the registry's
//! `HashMap<ClassName, Box<dyn ClassSpawner>>`.
//!
//! Registration is **idempotent** at the plugin level — if a parent
//! plugin adds `ContainersSpawnerPlugin` twice, the second
//! `register_class` call panics (the registry's drift-bug guard).
//! Bevy's plugin system de-duplicates plugins by type by default, so
//! this is a defence-in-depth check rather than the expected path.
//!
//! ## What this sub-plugin does NOT do
//!
//! - Does NOT initialize `ClassRegistry` itself. That's
//!   `ClassRegistryPlugin`'s job (Wave 2.3). This plugin assumes the
//!   resource exists; `register_class` would panic if it didn't.
//! - Does NOT touch `drain_slint_actions`, `StudioUiPlugin`, or any
//!   Slint UI state. Per LOOP 5: container spawners are pure ECS, no
//!   UI side effects.
//! - Does NOT introduce new Bevy resources. Per the LOOP-5 drain
//!   assertion (Wave 2.3), any new `ResMut` that flows into the drain
//!   needs the matching `init_resource`; containers don't drain into
//!   the UI, so they sidestep that gate entirely.
//! - Does NOT enable the `class-registry` cargo feature. Per spec
//!   §7.2 the feature gates the legacy match-arm cutover (Wave 5);
//!   Wave 3.E just registers — the registry getter returning `Some`
//!   for `Folder` / `Model` is a no-op until Wave 5 wires the
//!   `file_loader` lookup.

pub mod folder;
pub mod model;

pub use folder::FolderSpawner;
pub use model::ModelSpawner;

use bevy::prelude::*;

use eustress_common::class_registry::RegisterClassExt;

/// Bevy plugin that registers every container [`ClassSpawner`] —
/// currently [`FolderSpawner`] + [`ModelSpawner`].
///
/// Wave 3.A's parent `SpawnersPlugin` adds this plugin alongside the
/// other group sub-plugins (lights, geometry, gui, …). See the module
/// docs for the wiring contract.
pub struct ContainersSpawnerPlugin;

impl Plugin for ContainersSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Both spawners are `Default`-constructible (zero-sized) — the
        // `register_class::<S>` extension method handles the instance
        // creation + box + insert.
        //
        // Order is irrelevant per spec §6.3 — the registry is keyed by
        // `ClassName`, not insertion order; double-registration of the
        // same class panics at registration time.
        app.register_class::<FolderSpawner>()
            .register_class::<ModelSpawner>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};
    use eustress_common::classes::ClassName;

    /// Adding `ContainersSpawnerPlugin` to an `App` registers both
    /// container spawners under their canonical `ClassName` keys.
    ///
    /// This is the integration test for the sub-plugin shape — task
    /// 3.A's parent plugin adds this alongside other group plugins,
    /// and a downstream test (Wave 5) asserts every `ClassName`
    /// variant has a spawner. This test pins the container slice of
    /// that invariant.
    #[test]
    fn plugin_registers_folder_and_model() {
        let mut app = App::new();
        // `ClassRegistry` would normally be initialised by
        // `ClassRegistryPlugin`. Initialising it directly here keeps
        // the test free of the LOOP-5 startup-system dependency.
        app.init_resource::<ClassRegistry>();
        app.add_plugins(ContainersSpawnerPlugin);

        let registry = app
            .world()
            .resource::<ClassRegistry>();

        assert!(
            registry.contains(ClassName::Folder),
            "ContainersSpawnerPlugin must register FolderSpawner"
        );
        assert!(
            registry.contains(ClassName::Model),
            "ContainersSpawnerPlugin must register ModelSpawner"
        );
        assert_eq!(
            registry.len(),
            2,
            "ContainersSpawnerPlugin registers exactly two spawners — \
             any more means another group's plugin leaked in"
        );

        // Each registered spawner's `class_name()` matches its key
        // — this is the same invariant the registry's `register`
        // method enforces via panic, repeated here as a regression
        // gate.
        let folder_spawner: &dyn ClassSpawner =
            registry.get(ClassName::Folder).unwrap();
        assert_eq!(folder_spawner.class_name(), ClassName::Folder);

        let model_spawner: &dyn ClassSpawner =
            registry.get(ClassName::Model).unwrap();
        assert_eq!(model_spawner.class_name(), ClassName::Model);
    }

    /// The plugin must be safe to add even when `ClassRegistry` is
    /// pre-populated — but not with conflicting keys. This test
    /// covers the "pristine registry" path; the conflict path is
    /// exercised by the registry's own `register` panic test in
    /// `common::class_registry::tests`.
    #[test]
    fn plugin_works_on_pristine_registry() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();

        let registry_before = app.world().resource::<ClassRegistry>();
        assert!(registry_before.is_empty());

        app.add_plugins(ContainersSpawnerPlugin);

        let registry_after = app.world().resource::<ClassRegistry>();
        assert_eq!(registry_after.len(), 2);
    }
}
