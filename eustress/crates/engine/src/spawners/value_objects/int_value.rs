//! `IntValueSpawner` — `ClassSpawner` for [`ClassName::IntValue`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1
//! (ValueObjects, Wave 6.A). See `string_value.rs` for the shared rationale
//! (non-visual value container, empty LOD, stub persistence, TOML round-trip).
//!
//! `IntValue` holds one 64-bit signed integer. The Roblox equivalent is
//! `IntValue`, payload in the `Value` property (a 64-bit int).
//!
//! ## i64 vs the `PropertyBag`
//!
//! The component's `value` is `i64` (full Roblox range). The crossing-the-API
//! [`PropertyBag`] only carries `PropertyValue::Int(i32)`, so the bag is
//! lossy above `i32::MAX`. The two paths that have a true i64 in hand —
//! `import_from_toml` (TOML integers are i64) and `apply_edit` — handle the
//! `i64`→bag step explicitly. The on-disk `export_to_toml` writes the full
//! `i64`, so persistence is lossless; only the in-memory bag interchange
//! truncates, which the Wave 4 `PropertyValue::Int64` extension removes.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, IntValue, PropertyValue};
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::IntValue`].
#[derive(Default)]
pub struct IntValueSpawner;

impl ClassSpawner for IntValueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::IntValue
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props.get_string("metadata.name").unwrap_or("IntValue").to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let value = props.get_i32("value").unwrap_or(0) as i64;

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::IntValue,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                IntValue { value },
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

            if let Some(new_value) = props.get_i32("value") {
                if let Some(mut comp) = entity_mut.get_mut::<IntValue>() {
                    comp.value = new_value as i64;
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
        if let Some(value) = rbx.property("Value").and_then(|p| p.as_i32()) {
            bag.set("value", PropertyValue::Int(value));
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
            if let Some(value) = props.get("value").and_then(|v| v.as_integer()) {
                // TOML integers are i64; the bag carries i32 (see module docs).
                bag.set("value", PropertyValue::Int(value as i32));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("IntValue".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert("archivable".to_string(), toml::Value::Boolean(instance.archivable));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(comp) = world.entity(entity).get::<IntValue>() {
            let mut props = toml::value::Table::new();
            // Lossless on disk — write the full i64.
            props.insert("value".to_string(), toml::Value::Integer(comp.value));
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
    fn class_name_is_int_value() {
        assert_eq!(IntValueSpawner.class_name(), ClassName::IntValue);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = IntValueSpawner;
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_roblox_reads_value_property() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str { "IntValue" }
            fn name(&self) -> &str { "Score" }
            fn property(&self, key: &str) -> Option<RobloxPropertyValue> {
                match key {
                    "Value" => Some(RobloxPropertyValue::Int(42)),
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> { Vec::new() }
            fn referent(&self) -> u64 { 1 }
        }
        let bag = IntValueSpawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("metadata.name"), Some("Score"));
        assert_eq!(bag.get_i32("value"), Some(42));
    }

    #[test]
    fn import_from_toml_reads_value() {
        let toml_src = r#"
            [metadata]
            class_name = "IntValue"
            name = "Lives"
            [properties]
            value = 3
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = IntValueSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("value"), Some(3));
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(IntValueSpawner.deserialize(&[]).is_empty());
    }
}
