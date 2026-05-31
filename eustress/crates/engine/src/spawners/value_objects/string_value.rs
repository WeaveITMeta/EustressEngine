//! `StringValueSpawner` — `ClassSpawner` for [`ClassName::StringValue`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1
//! (ValueObjects). The first "full vertical" class group (Wave 6.A).
//!
//! ## What this is
//!
//! `StringValue` is a **non-visual value container**: it holds exactly one
//! UTF-8 string and nothing else. The Roblox equivalent is `StringValue`,
//! whose payload lives in a property literally named `Value`. Luau scripts
//! read/write it to stash data on the instance tree (e.g. a `StringValue`
//! named "Difficulty" with value "Hard").
//!
//! The spawner attaches the data-only side: the [`StringValue`] component
//! carrying the string. No mesh, no physics, no material — `Visibility`
//! defaults (the entity is never rendered).
//!
//! Bundle attached (mirrors the `FolderSpawner` container pattern at
//! `spawners/containers/folder.rs`):
//!
//! - [`Transform`] (identity — ValueObjects have no spatial intrinsics)
//! - [`Visibility`] (default — never rendered)
//! - [`Instance`] (`class_name = ClassName::StringValue`, `metadata.name`)
//! - [`StringValue`] component (the real payload, from `value`)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//! - [`Attributes`] / [`Tags`] (empty — Wave 5+ readers)
//!
//! ## The value is the point
//!
//! `import_from_roblox` reads the Roblox `Value` property (a
//! [`RobloxPropertyValue::String`]) into `component.value` — this is what
//! makes an imported `StringValue` actually carry "hello" rather than an
//! empty default.
//!
//! ## Why no LOD / stub persistence
//!
//! Identical rationale to [`FolderSpawner`][crate::spawners::containers::FolderSpawner]:
//! non-visual ⇒ empty LOD bundle at every tier; stub persistence (empty bytes
//! / empty bag) until a later wave lights up the Fjall write path for value
//! classes. The value survives via TOML round-trip in the meantime.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue, StringValue};
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::StringValue`].
#[derive(Default)]
pub struct StringValueSpawner;

impl ClassSpawner for StringValueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::StringValue
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("StringValue")
            .to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        // The payload — read from the canonical `value` key. Defaults to the
        // empty string (matches `StringValue::default()` and Roblox's default
        // empty `StringValue.Value`).
        let value = props.get_string("value").unwrap_or_default().to_string();

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::StringValue,
                    archivable,
                    id: 0, // assigned by post-spawn id system
                    uuid,
                    ai: false,
                },
                StringValue { value },
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            let new_name = props.get_string("metadata.name").map(str::to_string);

            if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                if let Some(ref n) = new_name {
                    instance.name = n.clone();
                }
                if let Some(archivable) = props.get_bool("metadata.archivable") {
                    instance.archivable = archivable;
                }
            }

            // The value mutation — cheap, no respawn.
            if let Some(new_value) = props.get_string("value") {
                if let Some(mut comp) = entity_mut.get_mut::<StringValue>() {
                    comp.value = new_value.to_string();
                }
            }

            if let Some(ref n) = new_name {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(n.clone());
                }
            }
        }

        false // never needs respawn
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        // THE value: Roblox stores it in a property named "Value".
        if let Some(value) = rbx.property("Value").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("value", PropertyValue::String(value));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);

        if let Some(meta) = toml_value.get("metadata") {
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

        // The value lives under [properties].value (folder-form _instance.toml
        // groups class payload under [properties]).
        if let Some(props) = toml_value.get("properties") {
            if let Some(value) = props.get("value").and_then(|v| v.as_str()) {
                bag.set("value", PropertyValue::String(value.to_string()));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("StringValue".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert(
                "archivable".to_string(),
                toml::Value::Boolean(instance.archivable),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(comp) = world.entity(entity).get::<StringValue>() {
            let mut props = toml::value::Table::new();
            props.insert("value".to_string(), toml::Value::String(comp.value.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::RobloxPropertyValue;

    #[test]
    fn class_name_is_string_value() {
        assert_eq!(StringValueSpawner.class_name(), ClassName::StringValue);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = StringValueSpawner;
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_roblox_reads_value_property() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str { "StringValue" }
            fn name(&self) -> &str { "Difficulty" }
            fn property(&self, key: &str) -> Option<RobloxPropertyValue> {
                match key {
                    "Value" => Some(RobloxPropertyValue::String("Hard".to_string())),
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> { Vec::new() }
            fn referent(&self) -> u64 { 7 }
        }

        let bag = StringValueSpawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("metadata.name"), Some("Difficulty"));
        assert_eq!(bag.get_string("value"), Some("Hard"));
    }

    #[test]
    fn import_from_toml_reads_value() {
        let toml_src = r#"
            [metadata]
            class_name = "StringValue"
            name = "Greeting"
            [properties]
            value = "hello"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = StringValueSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("metadata.name"), Some("Greeting"));
        assert_eq!(bag.get_string("value"), Some("hello"));
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(StringValueSpawner.deserialize(&[]).is_empty());
    }
}
