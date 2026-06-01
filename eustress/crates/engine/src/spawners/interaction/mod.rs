//! # Interaction & character-appearance spawners — Wave 6.D
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the 11 Roblox interaction / character-appearance classes. Each spawner
//! attaches its config component to a fresh entity; the *behavior* lives in
//! the sibling [`crate::interaction`] runtime module (the `InteractionPlugin`).
//!
//! | `ClassName`      | Component attached | Runtime system (in `crate::interaction`) |
//! |------------------|--------------------|------------------------------------------|
//! | `Tool`           | [`Tool`]           | `equip.rs` (equip/unequip + hotbar + activate) |
//! | `Accessory`      | [`Accessory`]      | `equip.rs` (attach to character attachment) |
//! | `ClickDetector`  | [`ClickDetector`]  | `click.rs` (camera raycast → MouseClick/Hover) |
//! | `ProximityPrompt`| [`ProximityPrompt`]| `proximity.rs` (distance + LoS → Triggered) |
//! | `Dialog`         | [`Dialog`]         | `dialog_ui.rs` (conversation panel) |
//! | `DialogChoice`   | [`DialogChoice`]   | `dialog_ui.rs` (choice buttons) |
//! | `BodyColors`     | [`BodyColors`]     | `appearance.rs` (recolor 6 limb groups) |
//! | `CharacterMesh`  | [`CharacterMesh`]  | `appearance.rs` (override limb mesh/texture) |
//! | `Shirt`          | [`Shirt`]          | `appearance.rs` (torso/legs texture) |
//! | `Pants`          | [`Pants`]          | `appearance.rs` (legs texture) |
//! | `ShirtGraphic`   | [`ShirtGraphic`]   | `appearance.rs` (front-torso decal) |
//!
//! ## Pattern
//!
//! Mirrors the Wave 6.A ValueObject group
//! ([`crate::spawners::value_objects`]): each spawner is a zero-sized
//! [`ClassSpawner`] that hydrates its component from the `PropertyBag` and
//! attaches the cross-cutting [`Instance`] + [`Name`]. The data layer is
//! Wave-6-Phase-0 work that already exists in
//! [`eustress_common::classes`]; this group wires the spawn + import/export.
//!
//! ## Why no LOD / stub persistence
//!
//! These classes are config carriers, not LOD-managed renderables. The
//! `lod_components` hook returns an empty bundle at every tier;
//! `serialize`/`deserialize` are stubbed (the value survives via TOML
//! round-trip) until a later wave lights up the Fjall write path — the same
//! contract every other 6.x spawner group ships under.
//!
//! [`Tool`]: eustress_common::classes::Tool
//! [`Accessory`]: eustress_common::classes::Accessory
//! [`ClickDetector`]: eustress_common::classes::ClickDetector
//! [`ProximityPrompt`]: eustress_common::classes::ProximityPrompt
//! [`Dialog`]: eustress_common::classes::Dialog
//! [`DialogChoice`]: eustress_common::classes::DialogChoice
//! [`BodyColors`]: eustress_common::classes::BodyColors
//! [`CharacterMesh`]: eustress_common::classes::CharacterMesh
//! [`Shirt`]: eustress_common::classes::Shirt
//! [`Pants`]: eustress_common::classes::Pants
//! [`ShirtGraphic`]: eustress_common::classes::ShirtGraphic
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod accessory;
pub mod body_colors;
pub mod character_mesh;
pub mod click_detector;
pub mod dialog;
pub mod dialog_choice;
pub mod pants;
pub mod proximity_prompt;
pub mod shirt;
pub mod shirt_graphic;
pub mod tool;

pub use accessory::AccessorySpawner;
pub use body_colors::BodyColorsSpawner;
pub use character_mesh::CharacterMeshSpawner;
pub use click_detector::ClickDetectorSpawner;
pub use dialog::DialogSpawner;
pub use dialog_choice::DialogChoiceSpawner;
pub use pants::PantsSpawner;
pub use proximity_prompt::ProximityPromptSpawner;
pub use shirt::ShirtSpawner;
pub use shirt_graphic::ShirtGraphicSpawner;
pub use tool::ToolSpawner;

/// Bevy plugin registering every interaction / character-appearance spawner
/// shipped by Wave 6.D with the
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
pub struct InteractionSpawnerPlugin;

impl Plugin for InteractionSpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<ToolSpawner>()
            .register_class::<AccessorySpawner>()
            .register_class::<ClickDetectorSpawner>()
            .register_class::<ProximityPromptSpawner>()
            .register_class::<DialogSpawner>()
            .register_class::<DialogChoiceSpawner>()
            .register_class::<BodyColorsSpawner>()
            .register_class::<CharacterMeshSpawner>()
            .register_class::<ShirtSpawner>()
            .register_class::<PantsSpawner>()
            .register_class::<ShirtGraphicSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every interaction entity carries.
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

/// Read a `[f32; 3]`-shaped TOML array into a [`Vec3`]. Missing/short arrays
/// yield `Vec3::ZERO`. Shared by spawners that round-trip a Vector3 prop.
pub(crate) fn read_vec3_array(arr: &[toml::Value]) -> Vec3 {
    let get = |i: usize| arr.get(i).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
    Vec3::new(get(0), get(1), get(2))
}

/// Serialize a [`Vec3`] to a 3-element TOML float array.
pub(crate) fn vec3_to_toml(v: Vec3) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(v.x as f64),
        toml::Value::Float(v.y as f64),
        toml::Value::Float(v.z as f64),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    /// Every interaction class this plugin owns — the registration roster.
    const INTERACTION_CLASSES: [ClassName; 11] = [
        ClassName::Tool,
        ClassName::Accessory,
        ClassName::ClickDetector,
        ClassName::ProximityPrompt,
        ClassName::Dialog,
        ClassName::DialogChoice,
        ClassName::BodyColors,
        ClassName::CharacterMesh,
        ClassName::Shirt,
        ClassName::Pants,
        ClassName::ShirtGraphic,
    ];

    /// Adding `InteractionSpawnerPlugin` registers all 11 spawners under
    /// their canonical `ClassName` keys, and each spawner's `class_name()`
    /// matches its registration key.
    #[test]
    fn plugin_registers_all_eleven_interaction_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(InteractionSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in INTERACTION_CLASSES {
            assert!(
                registry.contains(class),
                "InteractionSpawnerPlugin must register a spawner for {}",
                class.as_str()
            );
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }

        assert_eq!(
            registry.len(),
            11,
            "InteractionSpawnerPlugin registers exactly 11 spawners — \
             any more means another group's plugin leaked in"
        );
    }

    /// Every interaction class round-trips through
    /// `from_str(as_str()) == Ok(self)` — the invariant the Roblox importer
    /// and TOML loader rely on.
    #[test]
    fn class_name_round_trips_for_every_interaction_class() {
        for class in INTERACTION_CLASSES {
            let s = class.as_str();
            assert_eq!(
                ClassName::from_str(s),
                Ok(class),
                "ClassName round-trip failed for {s}"
            );
        }
    }
}
