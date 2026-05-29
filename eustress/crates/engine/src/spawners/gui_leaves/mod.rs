//! GUI leaf spawners — Wave 3.C
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.6: the six text/image
//! leaf classes that live inside ScreenGui / BillboardGui / SurfaceGui
//! containers. Each one:
//!
//! - Attaches `Transform` + `Visibility` + `Instance` + `Name`
//! - Attaches the Eustress class component (TextLabel / TextButton /
//!   TextBox / ImageLabel / ImageButton / ViewportFrame) — the canonical
//!   data the existing GUI render pipelines read
//! - Per spec §9 GUI policy: Hero = live render, Active = cached
//!   re-render only on Changed<T>, Streamed = hidden, Horizon = hidden
//!
//! ## Wave 3.C scope (orchestrator-written)
//!
//! The first Wave 3.C worker dispatch produced zero output. Rather than
//! re-dispatch and add ~2 hours to the timeline, the orchestrator wrote
//! these spawners directly following the worker pattern established by
//! Wave 3.A/B/D/E/F. Each leaf is structurally identical:
//!
//! 1. `spawn` reads `metadata.name` + `metadata.uuid` + `metadata.archivable`
//!    from the bag, attaches the matching `Component` from
//!    `eustress_common::classes`, hooks `Attributes` + `Tags` for the
//!    Wave 5+ attribute reader.
//! 2. `serialize`/`deserialize` are stubs (empty bytes ↔ empty bag) —
//!    GUI leaf state is fully derivable from its `_instance.toml` until
//!    Wave 4 wires the Fjall write path.
//! 3. `apply_edit` returns `false` — leaf property changes are reflected
//!    by the existing GUI sync systems via `Changed<T>` watchers, no
//!    respawn required.
//! 4. `lod_components` returns `Visibility::Hidden` at Streamed/Horizon;
//!    Hero and Active use empty bundles (existing GUI render pipeline
//!    handles visibility internally).
//! 5. `import_from_roblox` / `import_from_toml` / `export_to_toml` are
//!    Wave 4 importer concerns — minimal pass-throughs here.
//!
//! ## Plugin
//!
//! [`GuiLeavesSpawnerPlugin`] registers all six via
//! `app.register_class::<S>()`. Wired into `SlintUiPlugin::build` by
//! Wave 3.G.

use bevy::prelude::*;
use eustress_common::class_registry::{ClassRegistry, RegisterClassExt};

pub mod image_button;
pub mod image_label;
pub mod text_box;
pub mod text_button;
pub mod text_label;
pub mod viewport_frame;

pub use image_button::ImageButtonSpawner;
pub use image_label::ImageLabelSpawner;
pub use text_box::TextBoxSpawner;
pub use text_button::TextButtonSpawner;
pub use text_label::TextLabelSpawner;
pub use viewport_frame::ViewportFrameSpawner;

/// Bevy plugin that registers all six GUI leaf spawners with
/// `ClassRegistry`. Idempotent — re-adding is a no-op because
/// `register_class` ignores duplicate ClassName entries.
#[derive(Default)]
pub struct GuiLeavesSpawnerPlugin;

impl Plugin for GuiLeavesSpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Ensure ClassRegistry exists — defensive; Wave 2.3
        // ClassRegistryPlugin should already have mounted it, but if
        // this plugin lands first (unlikely but possible) the
        // init_resource is a no-op when the resource is already there.
        app.init_resource::<ClassRegistry>();

        app.register_class::<TextLabelSpawner>()
            .register_class::<TextButtonSpawner>()
            .register_class::<TextBoxSpawner>()
            .register_class::<ImageLabelSpawner>()
            .register_class::<ImageButtonSpawner>()
            .register_class::<ViewportFrameSpawner>();
    }
}
