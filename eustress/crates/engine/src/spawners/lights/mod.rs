//! # Light-class `ClassSpawner` implementations (Wave 3.A)
//!
//! One spawner per discrete light class per `CLASS_REGISTRY.md` §8.2 and
//! the worked example in `LIGHTING_AUDIT.md` §4.
//!
//! ## Coverage
//!
//! | `ClassName` | Spawner | Backing | Spec section |
//! |---|---|---|---|
//! | `PointLight`       | [`PointLightSpawner`]        | `bevy_pbr::PointLight`        | §4.2 |
//! | `SpotLight`        | [`SpotLightSpawner`]         | `bevy_pbr::SpotLight`         | §4.3 |
//! | `SurfaceLight`     | [`SurfaceLightSpawner`]      | emissive child quad + child `PointLight` (Option A) | §4.4 |
//! | `DirectionalLight` | [`DirectionalLightSpawner`]  | `bevy_pbr::DirectionalLight`  | §4.5 |
//!
//! The celestial path (`Star`/`Sun`, `Moon`, `Sky`, `Atmosphere`) is
//! deliberately NOT touched here — those classes already round-trip via
//! `plugins::lighting_plugin::hydrate_lighting_entities` and are
//! explicitly out of scope per `AGENT_DISPATCH.md` "Pre-existing Systems".
//!
//! ## Cargo-feature gating + registration
//!
//! Per spec §7.2 the `class-registry` cargo feature gates whether
//! `file_loader::spawn_directory_entry` consults the registry before
//! falling back to the legacy match arms. The registration call below
//! ALSO sits behind that feature so a `--no-default-features` build does
//! not pay the spawner-construction cost (small, but free is free).
//!
//! ## Mount point
//!
//! Wave 3.G (orchestrator-only) adds [`LightsSpawnerPlugin`] to
//! `SlintUiPlugin::build` after every Wave-3 group lands. Until then
//! this plugin is dead code and the legacy `spawn.rs` paths
//! (`spawn_point_light` etc.) keep being the runtime path.
//!
//! ## LOOP 5 — drain resource discipline
//!
//! None of the spawners register a new Bevy `Resource`. The trait API is
//! `Send + Sync` and stateless per instance (every spawner is a unit
//! struct with `Default`). If a future spawner needs runtime state it
//! must register the resource via
//! `app.init_resource::<R>().add_drain_resource::<R>(...)` per
//! `class_registry/loop5_assertion.rs` — the validator panics in debug
//! builds if the matching `init_resource` is missing.

use bevy::prelude::*;

use crate::class_registry::RegisterClassExt;

pub mod directional_light;
pub mod point_light;
pub mod spot_light;
pub mod surface_light;
pub(crate) mod toml_helpers;
pub(crate) mod wire;

pub use directional_light::DirectionalLightSpawner;
pub use point_light::PointLightSpawner;
pub use spot_light::SpotLightSpawner;
pub use surface_light::SurfaceLightSpawner;

/// Brightness multiplier applied when promoting a `SurfaceLight`'s
/// authoring brightness to the child `PointLight`'s lumens. Surfaces in
/// Roblox are area emitters; their brightness is a unitless scale, not a
/// lumens reading. Matches the legacy `spawn_surface_light` magic number
/// in `spawn.rs` so the new spawner ships at byte-equivalent visual
/// behavior for existing SurfaceLight entities.
pub(crate) const AREA_LIGHT_BRIGHTNESS_SCALE: f32 = 500.0;

/// Bevy plugin that registers all four light spawners with the
/// `ClassRegistry`.
///
/// The plugin is intentionally tiny — registration via
/// [`RegisterClassExt::register_class`] is the only side effect. The
/// orchestrator's Wave 3.G commit mounts this plugin from
/// `SlintUiPlugin::build` exactly once, after the
/// [`crate::class_registry::ClassRegistryPlugin`] has run (which
/// `init_resource::<ClassRegistry>`'d the registry).
///
/// Per spec §6.3 registration order is irrelevant — the registry is
/// keyed by `ClassName` and panics on double-registration, so the only
/// real failure mode is forgetting a plugin entirely. The startup-time
/// `log_registry_validation` (Wave 2.3) catches that.
pub struct LightsSpawnerPlugin;

impl Plugin for LightsSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // PointLight first — it's the worked example in the spec and
        // every other light spawner mirrors its shape. Registering it
        // first means a debug-build crash on a malformed
        // PointLightSpawner pins the failure to the most-documented
        // class (smallest surprise for the debugger).
        app.register_class::<PointLightSpawner>()
            .register_class::<SpotLightSpawner>()
            .register_class::<SurfaceLightSpawner>()
            .register_class::<DirectionalLightSpawner>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class_registry::{ClassRegistry, ClassRegistryPlugin};
    use eustress_common::classes::ClassName;

    /// All four spawners register without panic and the registry ends up
    /// with exactly four entries when the plugin is mounted standalone.
    /// This is the deliverable #2/#3 check from the task brief.
    #[test]
    fn plugin_registers_all_four_lights() {
        let mut app = App::new();
        app.add_plugins((ClassRegistryPlugin, LightsSpawnerPlugin));

        let registry = app
            .world()
            .get_resource::<ClassRegistry>()
            .expect("ClassRegistryPlugin must init the registry");

        assert_eq!(
            registry.len(),
            4,
            "LightsSpawnerPlugin must register exactly PointLight, SpotLight, \
             SurfaceLight, and DirectionalLight"
        );

        for class in [
            ClassName::PointLight,
            ClassName::SpotLight,
            ClassName::SurfaceLight,
            ClassName::DirectionalLight,
        ] {
            assert!(
                registry.contains(class),
                "registry must contain a spawner for {}",
                class.as_str()
            );
        }
    }
}
