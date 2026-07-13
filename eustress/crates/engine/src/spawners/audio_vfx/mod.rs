//! `spawners::audio_vfx` — Wave 3.F group plugin.
//!
//! Bundles the three `ClassSpawner` impls for the audio + VFX classes
//! enumerated in `docs/architecture/CLASS_REGISTRY.md` §8.4 (VFX) and
//! §8.7 (Camera & Audio):
//!
//! - [`SoundSpawner`] — `ClassName::Sound` (`bevy::audio::AudioPlayer`
//!   + `PlaybackSettings` + RollOffMode → SpatialScale conversion).
//! - [`ParticleEmitterSpawner`] — `ClassName::ParticleEmitter`
//!   (**STUB**: real `bevy_hanabi` integration is deferred to Wave 4;
//!   today's spawner attaches the Eustress component + a placeholder
//!   billboard visual).
//! - [`BeamSpawner`] — `ClassName::Beam` (stretched cylinder mesh
//!   between the two attachments + LOD-aware material swap).
//!
//! ## Why a sub-plugin
//!
//! Each Wave-3 group (lights, geometry, GUI containers, VFX, …) ships
//! its own sub-plugin so the per-class registration list stays
//! reviewable in isolation. The parent `spawners::SpawnersPlugin`
//! (created by Wave 3.A) aggregates every group plugin into a single
//! `app.add_plugins(...)` call inside `ClassRegistryPlugin::build`.
//!
//! Per `CLASS_REGISTRY.md` §7.2: registration is gated behind the
//! `class-registry` cargo feature so the legacy match arms in
//! `instance_loader`, `file_loader`, `gui_loader`, and `spawn.rs` stay
//! authoritative until Wave 5 deletes them. This sub-plugin is
//! intentionally self-contained — it registers three spawners and
//! nothing else.
//!
//! ## What this plugin does NOT do
//!
//! - Does NOT touch the existing audio pipeline (`SoundService`,
//!   `BeamsPlugin`, `particles.rs`) — those keep running and are still
//!   the load-bearing path on every Space.
//! - Does NOT add any `init_resource` calls — every dependency is a
//!   shared Bevy asset store already in the engine boot. Per LOOP-5
//!   breaker the sub-plugin must NOT register new drain-requirement
//!   resources.
//! - Does NOT mutate `slint_ui.rs` or any UI-facing system.
//! - Does NOT register components for reflection (the existing
//!   `SoundPlugin` / `particles.rs` / `beams.rs` already do so).
//!
//! ## Wave 3.A integration point
//!
//! When 3.A ships `crates/engine/src/spawners/mod.rs`, it imports this
//! module via:
//!
//! ```ignore
//! pub mod audio_vfx;
//! ```
//!
//! and re-exports the plugin so `ClassRegistryPlugin::build` can call
//! `app.add_plugins(crate::spawners::audio_vfx::AudioVfxSpawnerPlugin)`
//! behind the `class-registry` feature. This file's three `pub use`
//! lines below cover everything 3.A needs to surface upstream.

pub mod beam;
pub mod particle_emitter;
pub mod sound;

pub use beam::{sync_beam_transforms, BeamLodMode, BeamSegmentLink, BeamSpawner};
pub use particle_emitter::{ParticleEmitterPlaceholder, ParticleEmitterSpawner};
pub use sound::SoundSpawner;

use bevy::prelude::*;

use eustress_common::class_registry::RegisterClassExt;

/// Sub-plugin that registers the Sound / ParticleEmitter / Beam
/// spawners in the central `ClassRegistry`.
///
/// Mount via `app.add_plugins(AudioVfxSpawnerPlugin)`. The
/// `ClassRegistry` resource must already exist (initialised by
/// `ClassRegistryPlugin` from Wave 2.3) before this plugin builds —
/// `register_class` panics on missing resource. The default Bevy
/// plugin-order rules satisfy this when the spawner sub-plugins are
/// added inside `ClassRegistryPlugin::build` (per the Wave 3.A
/// integration plan).
pub struct AudioVfxSpawnerPlugin;

impl Plugin for AudioVfxSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Reflection registration for spawner-introduced components.
        // The Eustress data components (Sound, ParticleEmitter, Beam)
        // are already registered by the legacy plugins; we only add
        // the new spawner-owned ones here.
        app.register_type::<ParticleEmitterPlaceholder>()
            .register_type::<BeamSegmentLink>()
            .register_type::<BeamLodMode>();

        // Register the three spawners. Registration order is
        // irrelevant — the registry keys by `ClassName`, not insertion
        // order. Per `CLASS_REGISTRY.md` §5.1 a duplicate registration
        // panics, which surfaces drift (e.g. two `audio_vfx` plugins
        // accidentally added) at boot rather than at first spawn.
        app.register_class::<SoundSpawner>()
            .register_class::<ParticleEmitterSpawner>()
            .register_class::<BeamSpawner>();

        // The sync system BeamSegmentLink's doc comment always promised —
        // keeps every Beam taut between its two attachments every frame, so
        // dragging a mind-map node carries its edges with it.
        app.add_systems(Update, sync_beam_transforms);

        info!(
            "audio_vfx spawner group: registered Sound / ParticleEmitter (STUB — Wave 4 bevy_hanabi) / Beam"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};
    use eustress_common::classes::ClassName;

    /// All three spawners must register and the registry must end up
    /// with exactly the three new entries (no double-registration).
    #[test]
    fn audio_vfx_plugin_registers_all_three_spawners() {
        let mut app = App::new();
        // Pre-init the registry the way ClassRegistryPlugin would; we
        // don't depend on it directly to keep the test self-contained.
        app.init_resource::<ClassRegistry>();
        app.add_plugins(AudioVfxSpawnerPlugin);

        let registry = app
            .world()
            .resource::<ClassRegistry>();
        assert_eq!(registry.len(), 3, "exactly three new spawners");
        assert!(registry.contains(ClassName::Sound));
        assert!(registry.contains(ClassName::ParticleEmitter));
        assert!(registry.contains(ClassName::Beam));
    }

    /// Adding the plugin twice should panic at registration time —
    /// the drift-bug guard CLASS_REGISTRY.md §5.1 promises.
    ///
    /// Note: Bevy app teardown after a panic is unreliable; this test
    /// is `#[should_panic]` to keep the contract documented without
    /// adding a flaky harness dependency.
    #[test]
    #[should_panic(expected = "already registered")]
    fn double_plugin_add_panics() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(AudioVfxSpawnerPlugin);
        // Second add — must panic on the first duplicate spawner.
        app.add_plugins(AudioVfxSpawnerPlugin);
    }

    /// The plugin must register Reflect types for its new components —
    /// LOOP 7 breaker.
    #[test]
    fn plugin_registers_new_component_reflect_types() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(AudioVfxSpawnerPlugin);

        // The registry of registered Reflect types lives on
        // `AppTypeRegistry`; if these types aren't present the
        // Properties panel can't introspect them.
        let type_registry = app.world().resource::<AppTypeRegistry>().read();
        assert!(
            type_registry
                .get(std::any::TypeId::of::<ParticleEmitterPlaceholder>())
                .is_some(),
            "ParticleEmitterPlaceholder must be reflect-registered"
        );
        assert!(
            type_registry
                .get(std::any::TypeId::of::<BeamSegmentLink>())
                .is_some(),
            "BeamSegmentLink must be reflect-registered"
        );
    }

    /// Smoke check: each spawner returns the class it claims to handle
    /// (per `ClassRegistry::register`'s drift-bug guard, this is the
    /// invariant the registry enforces at boot).
    #[test]
    fn class_name_matches_registration_key() {
        let s = SoundSpawner;
        let p = ParticleEmitterSpawner;
        let b = BeamSpawner;
        assert_eq!(<SoundSpawner as ClassSpawner>::class_name(&s), ClassName::Sound);
        assert_eq!(
            <ParticleEmitterSpawner as ClassSpawner>::class_name(&p),
            ClassName::ParticleEmitter
        );
        assert_eq!(<BeamSpawner as ClassSpawner>::class_name(&b), ClassName::Beam);
    }
}
