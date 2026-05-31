//! `BinaryStringValueSpawner` — `ClassSpawner` for
//! [`ClassName::BinaryStringValue`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1
//! (ValueObjects, Wave 6.A). See `string_value.rs` for the shared rationale.
//!
//! `BinaryStringValue` holds opaque binary data. Roblox `BinaryStringValue`
//! stores raw bytes; Eustress keeps them as a (typically base64-encoded)
//! `String` so the value survives TOML round-trips. The payload lives in the
//! Roblox `Value` property.
//!
//! Structurally identical to [`StringValueSpawner`][crate::spawners::value_objects::StringValueSpawner]
//! — only the semantics (opaque bytes, not display text) and the `ClassName`
//! differ.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BinaryStringValue, ClassName, Instance, PropertyValue};
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::BinaryStringValue`].
#[derive(Default)]
pub struct BinaryStringValueSpawner;

impl ClassSpawner for BinaryStringValueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BinaryStringValue
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("BinaryStringValue")
            .to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let value = props.get_string("value").unwrap_or_default().to_string();

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::BinaryStringValue,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                BinaryStringValue { value },
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

            if let Some(new_value) = props.get_string("value") {
                if let Some(mut comp) = entity_mut.get_mut::<BinaryStringValue>() {
                    comp.value = new_value.to_string();
                }
            }

            if let Some(ref n) = new_name {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(n.clone());
                }
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
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
            toml::Value::String("BinaryStringValue".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert("archivable".to_string(), toml::Value::Boolean(instance.archivable));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(comp) = world.entity(entity).get::<BinaryStringValue>() {
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
    fn class_name_is_binary_string_value() {
        assert_eq!(
            BinaryStringValueSpawner.class_name(),
            ClassName::BinaryStringValue
        );
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = BinaryStringValueSpawner;
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_roblox_reads_value_property() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str { "BinaryStringValue" }
            fn name(&self) -> &str { "Blob" }
            fn property(&self, key: &str) -> Option<RobloxPropertyValue> {
                match key {
                    "Value" => Some(RobloxPropertyValue::String("aGVsbG8=".to_string())),
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> { Vec::new() }
            fn referent(&self) -> u64 { 1 }
        }
        let bag = BinaryStringValueSpawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("value"), Some("aGVsbG8="));
    }

    #[test]
    fn import_from_toml_reads_value() {
        let toml_src = r#"
            [metadata]
            class_name = "BinaryStringValue"
            name = "Payload"
            [properties]
            value = "ZGF0YQ=="
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BinaryStringValueSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("value"), Some("ZGF0YQ=="));
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(BinaryStringValueSpawner.deserialize(&[]).is_empty());
    }
}
