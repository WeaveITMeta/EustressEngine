//! Bevy plugin that wires the `ClassRegistry` Resource into the engine
//! AND mounts the LOOP-5 breaker validation system.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §5.2 + §6 + the Wave 2.3
//! protocol in `docs/process/AGENT_DISPATCH.md`.
//!
//! ## What this plugin does
//!
//! 1. `init_resource::<ClassRegistry>()` — the spawner registry from
//!    `eustress_common` is inserted into the App's world. Boots empty;
//!    Wave 3 populates it spawner-by-spawner.
//! 2. `init_resource::<DrainResourceChecklist>()` — the LOOP-5
//!    breaker's collection of "this plugin's drain system needs this
//!    resource" entries. Boots empty; opt-in by future plugins via
//!    [`AddDrainResourceExt::add_drain_resource`].
//! 3. `add_systems(Startup, log_registry_validation)` — startup-time
//!    info log of the registry's size. Warns about `ClassName` variants
//!    with no spawner once Wave 3 starts populating; on Wave 2.3 boot
//!    this just logs `class_registry: 0 spawners registered`.
//! 4. `add_systems(Startup, validate_drain_resources)` — startup-time
//!    LOOP-5 breaker. Panics in debug builds if any plugin registered
//!    a drain expectation whose resource isn't present.
//!
//! ## What this plugin does NOT do
//!
//! - Does NOT register any `ClassSpawner` implementations. Wave 3 ships
//!   those one PR at a time, each behind the `class-registry` cargo
//!   feature per spec §7.2.
//! - Does NOT modify `drain_slint_actions`, `StudioUiPlugin`, or any
//!   existing spawn path. The legacy match arms in `instance_loader`,
//!   `file_loader`, `gui_loader`, and `spawn.rs` keep their hardcoded
//!   dispatch exactly as today (spec §7.3 — Wave 3 starts the cutover).
//! - Does NOT enable any cargo feature. The `class-registry` feature
//!   exists nominally but is a no-op at this point in the project.
//!
//! ## Mount point
//!
//! Mounted from `SlintUiPlugin::build` via a single
//! `.add_plugins(ClassRegistryPlugin)` line. See `ui/slint_ui.rs:1113`.
//! Per spec §6.3 + LOOP-5 lesson: this plugin must run BEFORE any
//! plugin that opts in to the drain-resource checklist. SlintUiPlugin
//! satisfies that ordering by adding `ClassRegistryPlugin` at the top
//! of its own `build` (before its own `add_systems` for the drain).
//!
//! Per LOOP-5 lesson: this plugin must NEVER be added to the legacy
//! `StudioUiPlugin` (which isn't mounted at runtime); resources
//! registered there are invisible to the live engine's drain system —
//! that's the bug class the breaker exists to catch.

use bevy::prelude::*;

use eustress_common::class_registry::ClassRegistry;

use super::loop5_assertion::{
    validate_drain_resources, DrainResourceChecklist,
};

/// Bevy plugin that initialises [`ClassRegistry`] + the LOOP-5
/// drain-resource breaker.
///
/// See module docs for the full responsibility split between this
/// plugin and the Wave 3 spawner-registration plugins.
pub struct ClassRegistryPlugin;

impl Plugin for ClassRegistryPlugin {
    fn build(&self, app: &mut App) {
        // Idempotent: `init_resource` is a no-op when the resource
        // already exists. Some test harnesses pre-insert a populated
        // `ClassRegistry`; those keep their override.
        app.init_resource::<ClassRegistry>();

        // Same idempotency for the LOOP-5 checklist. Future plugins
        // may call `add_drain_resource` before `ClassRegistryPlugin`
        // is added (plugin order in Bevy is not strictly controlled);
        // `AddDrainResourceExt` already creates the checklist on
        // demand, so this is the second line of defence.
        app.init_resource::<DrainResourceChecklist>();

        // Wave 3 will populate the registry here behind the
        // `class-registry` feature flag. Wave 2.3 ships the plumbing
        // only — the registration list is intentionally empty.
        //
        // The shape of the future addition is:
        //
        // #[cfg(feature = "class-registry")]
        // {
        //     use eustress_common::class_registry::RegisterClassExt;
        //     app.register_class::<spawners::lights::PointLightSpawner>()
        //        .register_class::<spawners::geometry::PartSpawner>()
        //        // … 80 more (see CLASS_REGISTRY.md §8) …
        //        ;
        // }
        //
        // Per spec §6.3 registration order is irrelevant (keyed by
        // ClassName, double-registration panics) — the list is just a
        // place to put one line per spawner as each one ships.

        app.add_systems(
            Startup,
            (
                // Order matters: log first (info, never panics) so the
                // engine always has the registry size in its boot log
                // even if the LOOP-5 validator panics next. That makes
                // the panic easier to reproduce — you can see the
                // engine got far enough to enter Startup.
                log_registry_validation,
                validate_drain_resources,
            )
                .chain(),
        );
    }
}

/// Startup-time consistency check + info log for the spawner registry.
///
/// Mirrors the pattern of `class_schema::log_schema_validation` exactly
/// — keep the logging contract identical so an operator who reads both
/// sets of warns recognises the same shape.
///
/// At this Wave (2.3) the registry is always empty, so this just logs
/// `class_registry: 0 spawners registered`. Once Wave 3 starts
/// registering spawners, this system extends to warn-iterate every
/// `ClassName` variant and report any that have no spawner.
///
/// Per spec §5.2: an "as_str()" walk of every variant gives the
/// operator a `WARN ClassName::PointLight has no spawner` line per
/// gap. Wave 2.3 skips the variant walk (no spawners = every variant
/// would warn, which is noise); Wave 3 enables it via the same single
/// flag the registration list uses.
pub fn log_registry_validation(registry: Res<ClassRegistry>) {
    let n = registry.len();
    if n == 0 {
        // Wave 2.3 baseline — this is the line the orchestrator's
        // verification gate greps for.
        info!("class_registry: 0 spawners registered");
        return;
    }

    info!("class_registry: {} spawners registered", n);

    // Wave 3 hook (kept commented to flag the next agent's work):
    //
    //     for class in eustress_common::classes::ClassName::ALL {
    //         if !registry.contains(class) {
    //             warn!(
    //                 "class_registry: ClassName::{} has no spawner — \
    //                  file_loader will fall back to hardcoded match arm",
    //                 class.as_str(),
    //             );
    //         }
    //     }
    //
    // The `ClassName::ALL` const slice doesn't exist yet; Wave 3 adds
    // it alongside the first spawner registration so the warn-loop
    // can light up class-by-class as gaps shrink.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin's `build` step must leave the registry present + empty
    /// — exactly the Wave 2.3 baseline the orchestrator's gate looks
    /// for. If this stops being true (e.g. someone slips a
    /// `register_class` call in for testing) the gate fails.
    #[test]
    fn plugin_build_leaves_registry_empty() {
        let mut app = App::new();
        app.add_plugins(ClassRegistryPlugin);

        let registry = app
            .world()
            .get_resource::<ClassRegistry>()
            .expect("ClassRegistryPlugin must init_resource::<ClassRegistry>");
        assert!(
            registry.is_empty(),
            "Wave 2.3 ships zero spawners — registration is Wave 3's job"
        );
        assert_eq!(registry.len(), 0);
    }

    /// The plugin's `build` step must also create the LOOP-5 checklist
    /// resource so plugins which register expectations AFTER it don't
    /// have to lazy-create it (the extension trait covers them either
    /// way, but the standard path is the plugin-creates-it path).
    #[test]
    fn plugin_build_creates_drain_checklist() {
        let mut app = App::new();
        app.add_plugins(ClassRegistryPlugin);

        let checklist = app
            .world()
            .get_resource::<DrainResourceChecklist>()
            .expect("ClassRegistryPlugin must init the DrainResourceChecklist");
        assert!(checklist.is_empty(), "no expectations registered yet");
    }
}
