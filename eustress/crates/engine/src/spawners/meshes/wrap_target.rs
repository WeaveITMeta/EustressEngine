//! `WrapTargetSpawner` — `ClassSpawner` for [`ClassName::WrapTarget`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! layered-clothing wrap target body.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, WrapTarget};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::WrapTarget`].
#[derive(Default)]
pub struct WrapTargetSpawner;

impl ClassSpawner for WrapTargetSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::WrapTarget
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::WrapTarget, props);
        let name = instance.name.clone();
        let d = WrapTarget::default();
        let comp = WrapTarget {
            cage_mesh_id: props.get_string("cage_mesh_id").map(str::to_string).unwrap_or(d.cage_mesh_id),
            stiffness: props.get_f32("stiffness").unwrap_or(d.stiffness),
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
        if let Some(mut comp) = world.get_mut::<WrapTarget>(entity) {
            if let Some(v) = props.get_string("cage_mesh_id") { comp.cage_mesh_id = v.to_string(); }
            if let Some(v) = props.get_f32("stiffness") { comp.stiffness = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("CageMeshId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("cage_mesh_id", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Stiffness").and_then(|p| p.as_f32()) {
            bag.set("stiffness", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("cage_mesh_id").and_then(|v| v.as_str()) {
                bag.set("cage_mesh_id", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("stiffness").and_then(|v| v.as_float()) {
                bag.set("stiffness", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "WrapTarget")),
        );
        if let Some(comp) = world.get::<WrapTarget>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("cage_mesh_id".into(), toml::Value::String(comp.cage_mesh_id.clone()));
            props.insert("stiffness".into(), toml::Value::Float(comp.stiffness as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_wrap_target() {
        assert_eq!(WrapTargetSpawner.class_name(), ClassName::WrapTarget);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(WrapTargetSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "WrapTarget"
            name = "Body"
            [properties]
            stiffness = 0.7
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = WrapTargetSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("stiffness"), Some(0.7));
    }
}
