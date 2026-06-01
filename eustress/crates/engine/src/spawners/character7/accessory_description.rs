//! `AccessoryDescriptionSpawner` — `ClassSpawner` for
//! [`ClassName::AccessoryDescription`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! layered/rigid accessory specification. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AccessoryDescription, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AccessoryDescription`].
#[derive(Default)]
pub struct AccessoryDescriptionSpawner;

impl ClassSpawner for AccessoryDescriptionSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AccessoryDescription
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AccessoryDescription, props);
        let name = instance.name.clone();
        let d = AccessoryDescription::default();
        let comp = AccessoryDescription {
            accessory_type: props.get_string("accessory_type").map(str::to_string).unwrap_or(d.accessory_type),
            asset_id: props.get_string("asset_id").map(str::to_string).unwrap_or(d.asset_id),
            is_layered: props.get_bool("is_layered").unwrap_or(d.is_layered),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                comp,
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id();
        if let Some(parent) = ctx.parent_entity {
            ctx.commands.entity(entity).insert(ChildOf(parent));
        }
        entity
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        apply_metadata_edit(world, entity, props);
        if let Some(mut comp) = world.get_mut::<AccessoryDescription>(entity) {
            if let Some(v) = props.get_string("accessory_type") { comp.accessory_type = v.to_string(); }
            if let Some(v) = props.get_string("asset_id") { comp.asset_id = v.to_string(); }
            if let Some(v) = props.get_bool("is_layered") { comp.is_layered = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("AccessoryType").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("accessory_type", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("AssetId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("asset_id", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("IsLayered").and_then(|p| p.as_bool()) {
            bag.set("is_layered", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["accessory_type", "asset_id"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            if let Some(v) = props.get("is_layered").and_then(|v| v.as_bool()) {
                bag.set("is_layered", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AccessoryDescription")),
        );
        if let Some(comp) = world.get::<AccessoryDescription>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("accessory_type".into(), toml::Value::String(comp.accessory_type.clone()));
            props.insert("asset_id".into(), toml::Value::String(comp.asset_id.clone()));
            props.insert("is_layered".into(), toml::Value::Boolean(comp.is_layered));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_accessory_description() {
        assert_eq!(AccessoryDescriptionSpawner.class_name(), ClassName::AccessoryDescription);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AccessoryDescriptionSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "AccessoryDescription"
            name = "Acc"
            [properties]
            accessory_type = "Hat"
            is_layered = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AccessoryDescriptionSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("accessory_type"), Some("Hat"));
        assert_eq!(bag.get_bool("is_layered"), Some(true));
    }
}
