//! # UI layout / GuiObject-modifier spawners — Wave 7.B
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the 15 Roblox UI layout / decoration classes that attach to a GuiObject
//! and modify how it (or its children) lay out and render.
//!
//! | `ClassName`               | Component attached            |
//! |---------------------------|-------------------------------|
//! | `UICorner`                | [`UICorner`]                  |
//! | `UIGradient`              | [`UIGradient`]                |
//! | `UIStroke`                | [`UIStroke`]                  |
//! | `UIListLayout`            | [`UIListLayout`]              |
//! | `UIGridLayout`            | [`UIGridLayout`]              |
//! | `UIPadding`               | [`UIPadding`]                 |
//! | `UIAspectRatioConstraint` | [`UIAspectRatioConstraint`]   |
//! | `UIScale`                 | [`UIScale`]                   |
//! | `UISizeConstraint`        | [`UISizeConstraint`]          |
//! | `UITextSizeConstraint`    | [`UITextSizeConstraint`]      |
//! | `UITableLayout`           | [`UITableLayout`]             |
//! | `UIPageLayout`            | [`UIPageLayout`]              |
//! | `UIFlexItem`              | [`UIFlexItem`]                |
//! | `CanvasGroup`             | [`CanvasGroup`]               |
//! | `UIDragDetector`          | [`UIDragDetector`]            |
//!
//! ## Pattern
//!
//! Mirrors the Wave 6.A ValueObject group
//! ([`crate::spawners::value_objects`]) + Wave 6.D interaction group: each
//! spawner is a zero-sized [`ClassSpawner`] that hydrates its component from
//! the `PropertyBag` and attaches the cross-cutting [`Instance`] + [`Name`].
//! These are **data-attach** modifiers — the spawner attaches + persists the
//! config; the actual layout/render application is a later phase.
//!
//! ## Why no LOD / stub persistence
//!
//! These are non-renderable config carriers (the parent GuiObject renders).
//! `lod_components` returns an empty bundle at every tier;
//! `serialize`/`deserialize` are stubbed (the value survives via TOML
//! round-trip) until a later wave lights up the Fjall write path — the same
//! contract every other Wave-6/7 spawner group ships under.
//!
//! [`UICorner`]: eustress_common::classes::UICorner
//! [`UIGradient`]: eustress_common::classes::UIGradient
//! [`UIStroke`]: eustress_common::classes::UIStroke
//! [`UIListLayout`]: eustress_common::classes::UIListLayout
//! [`UIGridLayout`]: eustress_common::classes::UIGridLayout
//! [`UIPadding`]: eustress_common::classes::UIPadding
//! [`UIAspectRatioConstraint`]: eustress_common::classes::UIAspectRatioConstraint
//! [`UIScale`]: eustress_common::classes::UIScale
//! [`UISizeConstraint`]: eustress_common::classes::UISizeConstraint
//! [`UITextSizeConstraint`]: eustress_common::classes::UITextSizeConstraint
//! [`UITableLayout`]: eustress_common::classes::UITableLayout
//! [`UIPageLayout`]: eustress_common::classes::UIPageLayout
//! [`UIFlexItem`]: eustress_common::classes::UIFlexItem
//! [`CanvasGroup`]: eustress_common::classes::CanvasGroup
//! [`UIDragDetector`]: eustress_common::classes::UIDragDetector
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod canvas_group;
pub mod ui_aspect_ratio_constraint;
pub mod ui_corner;
pub mod ui_drag_detector;
pub mod ui_flex_item;
pub mod ui_gradient;
pub mod ui_grid_layout;
pub mod ui_list_layout;
pub mod ui_padding;
pub mod ui_page_layout;
pub mod ui_scale;
pub mod ui_size_constraint;
pub mod ui_stroke;
pub mod ui_table_layout;
pub mod ui_text_size_constraint;

pub use canvas_group::CanvasGroupSpawner;
pub use ui_aspect_ratio_constraint::UIAspectRatioConstraintSpawner;
pub use ui_corner::UICornerSpawner;
pub use ui_drag_detector::UIDragDetectorSpawner;
pub use ui_flex_item::UIFlexItemSpawner;
pub use ui_gradient::UIGradientSpawner;
pub use ui_grid_layout::UIGridLayoutSpawner;
pub use ui_list_layout::UIListLayoutSpawner;
pub use ui_padding::UIPaddingSpawner;
pub use ui_page_layout::UIPageLayoutSpawner;
pub use ui_scale::UIScaleSpawner;
pub use ui_size_constraint::UISizeConstraintSpawner;
pub use ui_stroke::UIStrokeSpawner;
pub use ui_table_layout::UITableLayoutSpawner;
pub use ui_text_size_constraint::UITextSizeConstraintSpawner;

/// Bevy plugin registering every UI-layout / GuiObject-modifier spawner
/// shipped by Wave 7.B with the
/// [`ClassRegistry`][eustress_common::class_registry::ClassRegistry].
///
/// Wired into `SlintUiPlugin::build`'s `add_plugins` tuple alongside the
/// other spawner group plugins. The `ClassRegistryPlugin` must run first so
/// the registry resource exists before any `register_class` call — the
/// standard wiring contract for every spawner sub-plugin (see
/// [`crate::spawners`] module docs).
///
/// Registration order is irrelevant (the registry is keyed by `ClassName`);
/// double-registration of a class panics at registration time.
pub struct UiLayoutSpawnerPlugin;

impl Plugin for UiLayoutSpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<UICornerSpawner>()
            .register_class::<UIGradientSpawner>()
            .register_class::<UIStrokeSpawner>()
            .register_class::<UIListLayoutSpawner>()
            .register_class::<UIGridLayoutSpawner>()
            .register_class::<UIPaddingSpawner>()
            .register_class::<UIAspectRatioConstraintSpawner>()
            .register_class::<UIScaleSpawner>()
            .register_class::<UISizeConstraintSpawner>()
            .register_class::<UITextSizeConstraintSpawner>()
            .register_class::<UITableLayoutSpawner>()
            .register_class::<UIPageLayoutSpawner>()
            .register_class::<UIFlexItemSpawner>()
            .register_class::<CanvasGroupSpawner>()
            .register_class::<UIDragDetectorSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every UI-layout entity carries.
/// Reads `metadata.name` / `metadata.uuid` / `metadata.archivable` from the
/// bag, falling back to the class default name when absent.
pub(crate) fn instance_from_bag(class_name: ClassName, bag: &PropertyBag) -> Instance {
    let name = bag
        .get_string("metadata.name")
        .unwrap_or(class_name.as_str())
        .to_string();
    Instance {
        name,
        class_name,
        archivable: bag.get_bool("metadata.archivable").unwrap_or(true),
        id: 0, // assigned by the post-spawn id system
        uuid: bag.get_uuid().unwrap_or_default().to_string(),
        ai: false,
    }
}

/// Copy the canonical `metadata.*` keys out of a `toml::Value`'s `[metadata]`
/// table into `bag`. Shared by every spawner's `import_from_toml`.
pub(crate) fn import_metadata(toml_value: &toml::Value, bag: &mut PropertyBag) {
    let Some(meta) = toml_value.get("metadata") else {
        return;
    };
    if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
        bag.set("metadata.name", PropertyValue::String(name.to_string()));
    }
    if let Some(archivable) = meta.get("archivable").and_then(|v| v.as_bool()) {
        bag.set("metadata.archivable", PropertyValue::Bool(archivable));
    }
    if let Some(uuid) = meta.get("uuid").and_then(|v| v.as_str()) {
        bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
    }
}

/// Emit the canonical `[metadata]` table for `export_to_toml`. Returns the
/// table so each spawner can attach its own `[properties]` alongside.
pub(crate) fn export_metadata(
    world: &World,
    entity: Entity,
    class_name: &str,
) -> toml::value::Table {
    let mut meta = toml::value::Table::new();
    meta.insert(
        "class_name".to_string(),
        toml::Value::String(class_name.to_string()),
    );
    if let Some(instance) = world.get::<Instance>(entity) {
        meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
        meta.insert(
            "archivable".to_string(),
            toml::Value::Boolean(instance.archivable),
        );
        if !instance.uuid.is_empty() {
            meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
        }
    }
    meta
}

/// Apply the canonical `metadata.*` edits (name + archivable) to an
/// already-spawned entity in place, keeping the Bevy [`Name`] in lockstep
/// with `Instance.name`. Shared by every spawner's `apply_edit`.
pub(crate) fn apply_metadata_edit(world: &mut World, entity: Entity, props: &PropertyBag) {
    if let Ok(mut em) = world.get_entity_mut(entity) {
        let new_name = props.get_string("metadata.name").map(str::to_string);
        if let Some(mut instance) = em.get_mut::<Instance>() {
            if let Some(ref n) = new_name {
                instance.name = n.clone();
            }
            if let Some(a) = props.get_bool("metadata.archivable") {
                instance.archivable = a;
            }
        }
        if let Some(ref n) = new_name {
            if let Some(mut name) = em.get_mut::<Name>() {
                name.set(n.clone());
            }
        }
    }
}

/// Read a `[f32; N]`-shaped TOML array of floats. Missing entries yield 0.0.
pub(crate) fn read_float_array<const N: usize>(arr: &[toml::Value]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = arr.get(i).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
    }
    out
}

/// Serialize a `[f32; N]` to a TOML float array.
pub(crate) fn float_array_to_toml(arr: &[f32]) -> toml::Value {
    toml::Value::Array(arr.iter().map(|f| toml::Value::Float(*f as f64)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    /// Every UI-layout class this plugin owns — the registration roster.
    const UI_LAYOUT_CLASSES: [ClassName; 15] = [
        ClassName::UICorner,
        ClassName::UIGradient,
        ClassName::UIStroke,
        ClassName::UIListLayout,
        ClassName::UIGridLayout,
        ClassName::UIPadding,
        ClassName::UIAspectRatioConstraint,
        ClassName::UIScale,
        ClassName::UISizeConstraint,
        ClassName::UITextSizeConstraint,
        ClassName::UITableLayout,
        ClassName::UIPageLayout,
        ClassName::UIFlexItem,
        ClassName::CanvasGroup,
        ClassName::UIDragDetector,
    ];

    /// Adding `UiLayoutSpawnerPlugin` registers all 15 spawners under their
    /// canonical `ClassName` keys, and each spawner's `class_name()` matches
    /// its registration key.
    #[test]
    fn plugin_registers_all_fifteen_ui_layout_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(UiLayoutSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in UI_LAYOUT_CLASSES {
            assert!(
                registry.contains(class),
                "UiLayoutSpawnerPlugin must register a spawner for {}",
                class.as_str()
            );
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }
        assert_eq!(registry.len(), 15);
    }

    /// Every UI-layout class round-trips through `from_str(as_str())`.
    #[test]
    fn class_name_round_trips_for_every_ui_layout_class() {
        for class in UI_LAYOUT_CLASSES {
            let s = class.as_str();
            assert_eq!(ClassName::from_str(s), Ok(class));
        }
    }
}
