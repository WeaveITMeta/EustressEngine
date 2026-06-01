//! `WrapDeformerSpawner` — `ClassSpawner` for [`ClassName::WrapDeformer`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! cage-mesh deformer for layered clothing.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, WrapDeformer};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::WrapDeformer`].
#[derive(Default)]
pub struct WrapDeformerSpawner;

impl ClassSpawner for WrapDeformerSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::WrapDeformer
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::WrapDeformer, props);
        let name = instance.name.clone();
        let d = WrapDeformer::default();
        let comp = WrapDeformer {
            enabled: props.get_bool("enabled").unwrap_or(d.enabled),
            cage_mesh_id: props.get_string("cage_mesh_id").map(str::to_string).unwrap_or(d.cage_mesh_id),
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
        if let Some(mut comp) = world.get_mut::<WrapDeformer>(entity) {
            if let Some(v) = props.get_bool("enabled") { comp.enabled = v; }
            if let Some(v) = props.get_string("cage_mesh_id") { comp.cage_mesh_id = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        if let Some(v) = rbx.property("CageMeshId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("cage_mesh_id", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("enabled", PropertyValue::Bool(v));
            }
            if let Some(v) = props.get("cage_mesh_id").and_then(|v| v.as_str()) {
                bag.set("cage_mesh_id", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "WrapDeformer")),
        );
        if let Some(comp) = world.get::<WrapDeformer>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("enabled".into(), toml::Value::Boolean(comp.enabled));
            props.insert("cage_mesh_id".into(), toml::Value::String(comp.cage_mesh_id.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_wrap_deformer() {
        assert_eq!(WrapDeformerSpawner.class_name(), ClassName::WrapDeformer);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(WrapDeformerSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "WrapDeformer"
            name = "Cage"
            [properties]
            enabled = false
            cage_mesh_id = "rbxassetid://9"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = WrapDeformerSpawner.import_from_toml(&value);
        assert_eq!(bag.get_bool("enabled"), Some(false));
        assert_eq!(bag.get_string("cage_mesh_id"), Some("rbxassetid://9"));
    }
}
