//! # Editable / sensor / chat / haptics spawners — Wave 7.G
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the final batch of pure data-attach / marker classes that carry runtime
//! config but have no render footprint of their own.
//!
//! | `ClassName`                  | Component attached                  |
//! |------------------------------|-------------------------------------|
//! | `TextChannel`                | [`TextChannel`]                     |
//! | `EditableImage`              | [`EditableImage`]                   |
//! | `RobloxEditableImage`        | [`RobloxEditableImage`]             |
//! | `BuoyancySensor`             | [`BuoyancySensor`]                  |
//! | `DragDetector`               | [`DragDetector`]                    |
//! | `TextChatCommand`            | [`TextChatCommand`]                 |
//! | `TextChatMessageProperties`  | [`TextChatMessageProperties`]       |
//! | `HapticEffect`               | [`HapticEffect`]                    |
//!
//! ## Pattern
//!
//! Mirrors the Wave 6.A ValueObject group
//! ([`crate::spawners::value_objects`]) + Wave 7.B UI-layout group: each
//! spawner is a zero-sized [`ClassSpawner`] that hydrates its component from
//! the `PropertyBag` and attaches the cross-cutting [`Instance`] + [`Name`].
//! These are **data-attach** carriers — the spawner attaches + persists the
//! config; the actual runtime behavior (image editing, buoyancy queries, drag
//! handling, chat dispatch, haptics playback) is a later phase.
//!
//! ## Why no LOD / stub persistence
//!
//! These are non-renderable config carriers. `lod_components` returns an empty
//! bundle at every tier; `serialize`/`deserialize` are stubbed (the value
//! survives via TOML round-trip) until a later wave lights up the Fjall write
//! path — the same contract every other Wave-6/7 spawner group ships under.
//!
//! [`TextChannel`]: eustress_common::classes::TextChannel
//! [`EditableImage`]: eustress_common::classes::EditableImage
//! [`RobloxEditableImage`]: eustress_common::classes::RobloxEditableImage
//! [`BuoyancySensor`]: eustress_common::classes::BuoyancySensor
//! [`DragDetector`]: eustress_common::classes::DragDetector
//! [`TextChatCommand`]: eustress_common::classes::TextChatCommand
//! [`TextChatMessageProperties`]: eustress_common::classes::TextChatMessageProperties
//! [`HapticEffect`]: eustress_common::classes::HapticEffect
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod buoyancy_sensor;
pub mod drag_detector;
pub mod editable_image;
pub mod haptic_effect;
pub mod roblox_editable_image;
pub mod text_channel;
pub mod text_chat_command;
pub mod text_chat_message_properties;

pub use buoyancy_sensor::BuoyancySensorSpawner;
pub use drag_detector::DragDetectorSpawner;
pub use editable_image::EditableImageSpawner;
pub use haptic_effect::HapticEffectSpawner;
pub use roblox_editable_image::RobloxEditableImageSpawner;
pub use text_channel::TextChannelSpawner;
pub use text_chat_command::TextChatCommandSpawner;
pub use text_chat_message_properties::TextChatMessagePropertiesSpawner;

/// Bevy plugin registering every editable / sensor / chat / haptics spawner
/// shipped by Wave 7.G with the
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
pub struct Editable7SpawnerPlugin;

impl Plugin for Editable7SpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<TextChannelSpawner>()
            .register_class::<EditableImageSpawner>()
            .register_class::<RobloxEditableImageSpawner>()
            .register_class::<BuoyancySensorSpawner>()
            .register_class::<DragDetectorSpawner>()
            .register_class::<TextChatCommandSpawner>()
            .register_class::<TextChatMessagePropertiesSpawner>()
            .register_class::<HapticEffectSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every Wave 7.G entity carries.
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

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    /// Every Wave 7.G class this plugin owns — the registration roster.
    const EDITABLE7_CLASSES: [ClassName; 8] = [
        ClassName::TextChannel,
        ClassName::EditableImage,
        ClassName::RobloxEditableImage,
        ClassName::BuoyancySensor,
        ClassName::DragDetector,
        ClassName::TextChatCommand,
        ClassName::TextChatMessageProperties,
        ClassName::HapticEffect,
    ];

    /// Adding `Editable7SpawnerPlugin` registers all 8 spawners under their
    /// canonical `ClassName` keys, and each spawner's `class_name()` matches
    /// its registration key.
    #[test]
    fn plugin_registers_all_eight_editable7_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(Editable7SpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in EDITABLE7_CLASSES {
            assert!(
                registry.contains(class),
                "Editable7SpawnerPlugin must register a spawner for {}",
                class.as_str()
            );
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }
        assert_eq!(registry.len(), 8);
    }

    /// Every Wave 7.G class round-trips through `from_str(as_str())`.
    #[test]
    fn class_name_round_trips_for_every_editable7_class() {
        for class in EDITABLE7_CLASSES {
            let s = class.as_str();
            assert_eq!(ClassName::from_str(s), Ok(class));
        }
    }
}
