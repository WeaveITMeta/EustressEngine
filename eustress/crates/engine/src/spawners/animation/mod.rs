//! `spawners::animation` — Wave 5.D group plugin.
//!
//! Bundles the two `ClassSpawner` impls for the animation classes
//! enumerated in `docs/architecture/CLASS_REGISTRY.md` §8.8:
//!
//! - [`AnimatorSpawner`] — `ClassName::Animator` (the playback
//!   controller; attaches the Eustress [`Animator`] component, which the
//!   existing animation runtime maps to a `bevy::animation::AnimationPlayer`).
//! - [`KeyframeSequenceSpawner`] — `ClassName::KeyframeSequence` (the
//!   animation-clip container; attaches the Eustress
//!   [`KeyframeSequence`] component carrying `looped` / `priority` /
//!   keyframes, mapped to a Bevy `AnimationClip` by the runtime).
//!
//! Neither class carries a standalone visual, so both spawners return an
//! empty LOD bundle at every tier (children of the animated rig carry
//! their own LOD via their own spawners).
//!
//! ## Why a sub-plugin
//!
//! Each wave group ships its own sub-plugin so the per-class
//! registration list stays reviewable in isolation. The parent
//! `spawners` orchestrator (Wave 5.E) aggregates every group plugin into
//! a single `app.add_plugins(...)` call inside `ClassRegistryPlugin::build`.
//!
//! Per `CLASS_REGISTRY.md` §7.2: registration is gated behind the
//! `class-registry` cargo feature at the call site so the legacy match
//! arms in `instance_loader`, `file_loader`, and `spawn.rs` stay
//! authoritative until Wave 5 deletes them. This sub-plugin is
//! intentionally self-contained — it registers two spawners (and the
//! Reflect impls their components need) and nothing else.
//!
//! ## What this plugin does NOT do
//!
//! - Does NOT touch the animation runtime / `bevy_animation` systems —
//!   those keep running and drive playback off the components this
//!   plugin's spawners attach.
//! - Does NOT add any `init_resource` calls — neither spawner needs a
//!   new Bevy resource. Per the LOOP-5 breaker the sub-plugin must NOT
//!   register new drain-requirement resources.
//! - Does NOT mutate `slint_ui.rs`, `lib.rs`, `main.rs`, or any other
//!   spawners subdir — those are wired by the Wave 5.E orchestrator.
//!
//! ## Wave 5.E integration point
//!
//! When 5.E updates `crates/engine/src/spawners/mod.rs`, it imports this
//! module via:
//!
//! ```ignore
//! pub mod animation;
//! ```
//!
//! and re-exports the plugin so `ClassRegistryPlugin::build` can call
//! `app.add_plugins(crate::spawners::animation::AnimationSpawnerPlugin)`
//! behind the `class-registry` feature. The `pub use` lines below cover
//! everything 5.E needs to surface upstream.

pub mod animator;
pub mod keyframe_sequence;

pub use animator::AnimatorSpawner;
pub use keyframe_sequence::KeyframeSequenceSpawner;

use bevy::prelude::*;

use eustress_common::class_registry::RegisterClassExt;
use eustress_common::classes::{
    AnimationPriority, Animator, EasingStyle, Keyframe, KeyframeSequence, RigType,
};

/// Sub-plugin that registers the Animator / KeyframeSequence spawners in
/// the central `ClassRegistry`.
///
/// Mount via `app.add_plugins(AnimationSpawnerPlugin)`. The
/// `ClassRegistry` resource must already exist (initialised by
/// `ClassRegistryPlugin` from Wave 2.3) before this plugin builds —
/// `register_class` panics on missing resource. The default Bevy
/// plugin-order rules satisfy this when the spawner sub-plugins are
/// added inside `ClassRegistryPlugin::build` (per the Wave 5.E
/// integration plan).
pub struct AnimationSpawnerPlugin;

impl Plugin for AnimationSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Reflection registration for the spawner-introduced components
        // (LOOP-7 breaker). The `Animator` / `KeyframeSequence` data
        // components — and the enums they nest — carry `#[reflect]`
        // derives in `eustress_common::classes` but are not registered
        // by any other plugin, so the Properties panel can't introspect
        // them until we register the Reflect type info here.
        //
        // `register_type` is idempotent in Bevy (registering a type
        // twice is a no-op), so this stays safe even if a future plugin
        // also registers them.
        app.register_type::<Animator>()
            .register_type::<RigType>()
            .register_type::<KeyframeSequence>()
            .register_type::<AnimationPriority>()
            .register_type::<Keyframe>()
            .register_type::<EasingStyle>();

        // Register the two spawners. Registration order is irrelevant —
        // the registry keys by `ClassName`, not insertion order. Per
        // `CLASS_REGISTRY.md` §5.1 a duplicate registration panics, which
        // surfaces drift (e.g. two `animation` plugins accidentally
        // added) at boot rather than at first spawn.
        app.register_class::<AnimatorSpawner>()
            .register_class::<KeyframeSequenceSpawner>();

        info!("animation spawner group: registered Animator / KeyframeSequence");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};
    use eustress_common::classes::ClassName;

    /// Both spawners must register and the registry must end up with
    /// exactly the two new entries (no double-registration).
    #[test]
    fn animation_plugin_registers_both_spawners() {
        let mut app = App::new();
        // Pre-init the registry the way ClassRegistryPlugin would; we
        // don't depend on it directly to keep the test self-contained.
        app.init_resource::<ClassRegistry>();
        app.add_plugins(AnimationSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        assert_eq!(registry.len(), 2, "exactly two new spawners");
        assert!(registry.contains(ClassName::Animator));
        assert!(registry.contains(ClassName::KeyframeSequence));
    }

    /// Adding the plugin twice panics at registration time — the
    /// drift-bug guard CLASS_REGISTRY.md §5.1 promises.
    #[test]
    #[should_panic(expected = "already registered")]
    fn double_plugin_add_panics() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(AnimationSpawnerPlugin);
        // Second add — must panic on the first duplicate spawner.
        app.add_plugins(AnimationSpawnerPlugin);
    }

    /// The plugin must register Reflect types for the animation
    /// components — LOOP-7 breaker.
    #[test]
    fn plugin_registers_animation_reflect_types() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(AnimationSpawnerPlugin);

        let type_registry = app.world().resource::<AppTypeRegistry>().read();
        assert!(
            type_registry
                .get(std::any::TypeId::of::<Animator>())
                .is_some(),
            "Animator must be reflect-registered"
        );
        assert!(
            type_registry
                .get(std::any::TypeId::of::<KeyframeSequence>())
                .is_some(),
            "KeyframeSequence must be reflect-registered"
        );
    }

    /// Smoke check: each spawner returns the class it claims to handle.
    #[test]
    fn class_name_matches_registration_key() {
        let a = AnimatorSpawner;
        let k = KeyframeSequenceSpawner;
        assert_eq!(
            <AnimatorSpawner as ClassSpawner>::class_name(&a),
            ClassName::Animator
        );
        assert_eq!(
            <KeyframeSequenceSpawner as ClassSpawner>::class_name(&k),
            ClassName::KeyframeSequence
        );
    }
}
