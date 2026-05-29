//! # GUI-container spawners — Wave 3.B
//!
//! Five [`ClassSpawner`] implementations covering the GUI-container
//! family per `docs/architecture/CLASS_REGISTRY.md` §8.5:
//!
//! | Class            | Spawner                                                |
//! |------------------|--------------------------------------------------------|
//! | [`ScreenGui`]    | [`screen_gui::ScreenGuiSpawner`]                       |
//! | [`BillboardGui`] | [`billboard_gui::BillboardGuiSpawner`]                 |
//! | [`SurfaceGui`]   | [`surface_gui::SurfaceGuiSpawner`]                     |
//! | [`Frame`]        | [`frame::FrameSpawner`]                                |
//! | [`ScrollingFrame`] | [`scrolling_frame::ScrollingFrameSpawner`]           |
//!
//! All five implementations mirror the existing `spawn::spawn_*` /
//! `gui_loader::spawn_*_element` paths so the Wave 5 cutover stays
//! byte-equivalent. The trait surface (serialize, deserialize, apply_edit,
//! lod_components, import_from_roblox, import_from_toml, export_to_toml)
//! is implemented end-to-end; the persistence path currently uses a
//! bincode 1.x round-trip of the [`PropertyBag`] gated by a per-class
//! schema-tag byte, to be replaced with class-specific rkyv mirror
//! structs in Wave 5+ (per spec Appendix A).
//!
//! ## Plugin
//!
//! [`GuiContainersSpawnerPlugin`] registers all five spawners with the
//! [`ClassRegistry`] resource. Wave 3.G wires this plugin into the
//! engine's top-level plugin chain; this module deliberately does NOT
//! mutate `spawners/mod.rs`, `lib.rs`, `main.rs`, or `slint_ui.rs`.
//!
//! ## LOOP-5 safety
//!
//! No new resources are introduced — the plugin reaches into the
//! pre-existing [`ClassRegistry`] resource via [`RegisterClassExt`]. No
//! `add_drain_resource` calls are needed. Per spec §6.3 registration
//! order is irrelevant (keyed by `ClassName`, double-registration
//! panics) so the plugin can be added before or after any other Wave 3
//! sub-plugin.
//!
//! [`ClassSpawner`]: eustress_common::class_registry::ClassSpawner
//! [`ClassRegistry`]: eustress_common::class_registry::ClassRegistry
//! [`PropertyBag`]: eustress_common::class_registry::PropertyBag
//! [`RegisterClassExt`]: eustress_common::class_registry::RegisterClassExt
//! [`ScreenGui`]: eustress_common::classes::ScreenGui
//! [`BillboardGui`]: eustress_common::classes::BillboardGui
//! [`SurfaceGui`]: eustress_common::classes::SurfaceGui
//! [`Frame`]: eustress_common::classes::Frame
//! [`ScrollingFrame`]: eustress_common::classes::ScrollingFrame

use bevy::prelude::*;

use eustress_common::class_registry::RegisterClassExt;

pub mod billboard_gui;
pub mod frame;
pub mod screen_gui;
pub mod scrolling_frame;
pub mod surface_gui;

pub use billboard_gui::BillboardGuiSpawner;
pub use frame::FrameSpawner;
pub use screen_gui::ScreenGuiSpawner;
pub use scrolling_frame::ScrollingFrameSpawner;
pub use surface_gui::SurfaceGuiSpawner;

/// Bevy plugin that registers every GUI-container spawner with the
/// [`ClassRegistry`] resource.
///
/// Wave 3.G wires this into the engine plugin chain via a single
/// `app.add_plugins(GuiContainersSpawnerPlugin)` line; until then the
/// spawners are inert (the registry stays empty, callers continue
/// hitting the legacy `gui_loader` / `spawn::spawn_*` paths per spec
/// §7.3 fallback contract).
///
/// [`ClassRegistry`]: eustress_common::class_registry::ClassRegistry
pub struct GuiContainersSpawnerPlugin;

impl Plugin for GuiContainersSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Per spec §5.2: `ClassRegistryPlugin` must run BEFORE any
        // plugin that registers spawners. The engine bootstrap (Wave
        // 2.3) mounts `ClassRegistryPlugin` inside `SlintUiPlugin::build`
        // before any Wave 3 spawner plugin is added. We rely on that
        // ordering — if a downstream plugin chain adds this plugin
        // first, `register_class` will panic at the missing resource
        // lookup, surfacing the ordering bug loudly rather than
        // silently boot-skipping (which is exactly the spec §5.2 +
        // LOOP-5 guarantee).
        app.register_class::<ScreenGuiSpawner>()
            .register_class::<BillboardGuiSpawner>()
            .register_class::<SurfaceGuiSpawner>()
            .register_class::<FrameSpawner>()
            .register_class::<ScrollingFrameSpawner>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::ClassRegistry;
    use eustress_common::classes::ClassName;

    /// Standing up the plugin (after `ClassRegistry` is in place) must
    /// register exactly five spawners — one per class in §8.5.
    #[test]
    fn plugin_registers_five_spawners() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(GuiContainersSpawnerPlugin);

        let registry = app
            .world()
            .get_resource::<ClassRegistry>()
            .expect("ClassRegistry must exist");
        assert_eq!(
            registry.len(),
            5,
            "GuiContainersSpawnerPlugin must register exactly 5 spawners (§8.5)"
        );
        assert!(registry.contains(ClassName::ScreenGui));
        assert!(registry.contains(ClassName::BillboardGui));
        assert!(registry.contains(ClassName::SurfaceGui));
        assert!(registry.contains(ClassName::Frame));
        assert!(registry.contains(ClassName::ScrollingFrame));
    }

    /// Each spawner's `class_name()` matches its registration key —
    /// re-registering would panic (per `ClassRegistry::register`).
    /// This guards against the "spawner reports wrong class_name"
    /// drift bug spec §5.1 explicitly warns about.
    #[test]
    fn spawner_class_names_match_registration_keys() {
        use eustress_common::class_registry::ClassSpawner;
        assert_eq!(ScreenGuiSpawner.class_name(), ClassName::ScreenGui);
        assert_eq!(BillboardGuiSpawner.class_name(), ClassName::BillboardGui);
        assert_eq!(SurfaceGuiSpawner.class_name(), ClassName::SurfaceGui);
        assert_eq!(FrameSpawner.class_name(), ClassName::Frame);
        assert_eq!(ScrollingFrameSpawner.class_name(), ClassName::ScrollingFrame);
    }
}
