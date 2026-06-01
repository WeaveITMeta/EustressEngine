//! `WrapLayerSpawner` — `ClassSpawner` for [`ClassName::WrapLayer`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! layered-clothing wrap layer.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, WrapLayer};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::WrapLayer`].
#[derive(Default)]
pub struct WrapLayerSpawner;

impl ClassSpawner for WrapLayerSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::WrapLayer
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::WrapLayer, props);
        let name = instance.name.clone();
        let d = WrapLayer::default();
        let comp = WrapLayer {
            enabled: props.get_bool("enabled").unwrap_or(d.enabled),
            cage_mesh_id: props.get_string("cage_mesh_id").map(str::to_string).unwrap_or(d.cage_mesh_id),
            reference_mesh_id: props.get_string("reference_mesh_id").map(str::to_string).unwrap_or(d.reference_mesh_id),
            order: props.get_i32("order").unwrap_or(d.order),
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
        if let Some(mut comp) = world.get_mut::<WrapLayer>(entity) {
            if let Some(v) = props.get_bool("enabled") { comp.enabled = v; }
            if let Some(v) = props.get_string("cage_mesh_id") { comp.cage_mesh_id = v.to_string(); }
            if let Some(v) = props.get_string("reference_mesh_id") { comp.reference_mesh_id = v.to_string(); }
            if let Some(v) = props.get_i32("order") { comp.order = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        for (rbx_key, key) in [("CageMeshId", "cage_mesh_id"), ("ReferenceMeshId", "reference_mesh_id")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        if let Some(v) = rbx.property("Order").and_then(|p| p.as_i32()) {
            bag.set("order", PropertyValue::Int(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("enabled", PropertyValue::Bool(v));
            }
            for key in ["cage_mesh_id", "reference_mesh_id"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            if let Some(v) = props.get("order").and_then(|v| v.as_integer()) {
                bag.set("order", PropertyValue::Int(v as i32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "WrapLayer")),
        );
        if let Some(comp) = world.get::<WrapLayer>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("enabled".into(), toml::Value::Boolean(comp.enabled));
            props.insert("cage_mesh_id".into(), toml::Value::String(comp.cage_mesh_id.clone()));
            props.insert("reference_mesh_id".into(), toml::Value::String(comp.reference_mesh_id.clone()));
            props.insert("order".into(), toml::Value::Integer(comp.order as i64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_wrap_layer() {
        assert_eq!(WrapLayerSpawner.class_name(), ClassName::WrapLayer);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(WrapLayerSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "WrapLayer"
            name = "Layer"
            [properties]
            order = 3
            cage_mesh_id = "rbxassetid://7"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = WrapLayerSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("order"), Some(3));
        assert_eq!(bag.get_string("cage_mesh_id"), Some("rbxassetid://7"));
    }
}
