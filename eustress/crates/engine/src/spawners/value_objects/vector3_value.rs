//! `Vector3ValueSpawner` — `ClassSpawner` for [`ClassName::Vector3Value`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1
//! (ValueObjects, Wave 6.A). See `string_value.rs` for the shared rationale.
//!
//! `Vector3Value` holds one 3D vector `[x, y, z]`. Roblox `Vector3Value`,
//! payload in the `Value` property (a `Vector3`).
//!
//! ## Composite value vs the Wave 2 Roblox adapter
//!
//! Same constraint as `Color3Value`: the Wave 2 [`RobloxPropertyValue`] has no
//! `Vector3` variant, so a Roblox `Vector3` arrives as
//! [`RobloxPropertyValue::Other`]. `import_from_roblox` captures only the name;
//! the Wave 4 importer fills `value`. The TOML path carries the full vector
//! today. Inside the [`PropertyBag`] the vector travels as a
//! [`PropertyValue::Vector3`] ([`Vec3`]); the component stores the equivalent
//! `[f32; 3]`.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue, Vector3Value};
use eustress_common::{Attributes, Tags};

/// Read a 3-element float array from a `toml::Value` as a [`Vec3`], defaulting
/// missing components to `0.0`. Returns `None` only when the key isn't an array.
fn toml_vec3(v: &toml::Value) -> Option<Vec3> {
    let arr = v.as_array()?;
    let comp = |i: usize| arr.get(i).and_then(|x| x.as_float()).unwrap_or(0.0) as f32;
    Some(Vec3::new(comp(0), comp(1), comp(2)))
}

/// Zero-sized spawner for [`ClassName::Vector3Value`].
#[derive(Default)]
pub struct Vector3ValueSpawner;

impl ClassSpawner for Vector3ValueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Vector3Value
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props.get_string("metadata.name").unwrap_or("Vector3Value").to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let v = props.get_vec3("value").unwrap_or(Vec3::ZERO);
        let value = [v.x, v.y, v.z];

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::Vector3Value,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                Vector3Value { value },
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

            if let Some(v) = props.get_vec3("value") {
                if let Some(mut comp) = entity_mut.get_mut::<Vector3Value>() {
                    comp.value = [v.x, v.y, v.z];
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
        // Wave 2 adapter cannot carry a Vector3 (see module docs).
        let mut bag = PropertyBag::with_capacity(1);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
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
            if let Some(v) = props.get("value").and_then(toml_vec3) {
                bag.set("value", PropertyValue::Vector3(v));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("Vector3Value".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert("archivable".to_string(), toml::Value::Boolean(instance.archivable));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(comp) = world.entity(entity).get::<Vector3Value>() {
            let arr = comp
                .value
                .iter()
                .map(|c| toml::Value::Float(*c as f64))
                .collect::<Vec<_>>();
            let mut props = toml::value::Table::new();
            props.insert("value".to_string(), toml::Value::Array(arr));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_vector3_value() {
        assert_eq!(Vector3ValueSpawner.class_name(), ClassName::Vector3Value);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = Vector3ValueSpawner;
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_vec3_array() {
        let toml_src = r#"
            [metadata]
            class_name = "Vector3Value"
            name = "Spawn"
            [properties]
            value = [10.0, 5.0, -3.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = Vector3ValueSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("metadata.name"), Some("Spawn"));
        assert_eq!(bag.get_vec3("value"), Some(Vec3::new(10.0, 5.0, -3.0)));
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(Vector3ValueSpawner.deserialize(&[]).is_empty());
    }
}
