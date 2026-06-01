//! `IKControlSpawner` — `ClassSpawner` for [`ClassName::IKControl`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! inverse-kinematics chain control. See the group [`mod`](super) docs.
//!
//! The chain/effector/target are optional Eustress instance-id references
//! (`Option<u32>`), round-tripped through the bag as `Int` (`-1` ⇒ `None`).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, IKControl, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{
    apply_metadata_edit, export_metadata, import_metadata, instance_from_bag, read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::IKControl`].
#[derive(Default)]
pub struct IKControlSpawner;

impl ClassSpawner for IKControlSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::IKControl
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::IKControl, props);
        let name = instance.name.clone();
        let d = IKControl::default();
        let comp = IKControl {
            chain_root: read_optional_ref(props, "chain_root"),
            end_effector: read_optional_ref(props, "end_effector"),
            target: read_optional_ref(props, "target"),
            weight: props.get_f32("weight").unwrap_or(d.weight),
            type_: props.get_string("type").map(str::to_string).unwrap_or(d.type_),
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
        if let Some(mut comp) = world.get_mut::<IKControl>(entity) {
            if props.get("chain_root").is_some() { comp.chain_root = read_optional_ref(props, "chain_root"); }
            if props.get("end_effector").is_some() { comp.end_effector = read_optional_ref(props, "end_effector"); }
            if props.get("target").is_some() { comp.target = read_optional_ref(props, "target"); }
            if let Some(v) = props.get_f32("weight") { comp.weight = v; }
            if let Some(v) = props.get_string("type") { comp.type_ = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [("ChainRoot", "chain_root"), ("EndEffector", "end_effector"), ("Target", "target")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_i32()) {
                bag.set(key, PropertyValue::Int(v));
            }
        }
        if let Some(v) = rbx.property("Weight").and_then(|p| p.as_f32()) {
            bag.set("weight", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("Type").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("type", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["chain_root", "end_effector", "target"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
            if let Some(v) = props.get("weight").and_then(|v| v.as_float()) {
                bag.set("weight", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("type").and_then(|v| v.as_str()) {
                bag.set("type", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "IKControl")),
        );
        if let Some(comp) = world.get::<IKControl>(entity) {
            let mut props = toml::value::Table::new();
            if let Some(v) = comp.chain_root { props.insert("chain_root".into(), toml::Value::Integer(v as i64)); }
            if let Some(v) = comp.end_effector { props.insert("end_effector".into(), toml::Value::Integer(v as i64)); }
            if let Some(v) = comp.target { props.insert("target".into(), toml::Value::Integer(v as i64)); }
            props.insert("weight".into(), toml::Value::Float(comp.weight as f64));
            props.insert("type".into(), toml::Value::String(comp.type_.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ik_control() {
        assert_eq!(IKControlSpawner.class_name(), ClassName::IKControl);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(IKControlSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "IKControl"
            name = "IK"
            [properties]
            target = 12
            weight = 0.8
            type = "LookAt"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = IKControlSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("target"), Some(12));
        assert_eq!(bag.get_f32("weight"), Some(0.8));
        assert_eq!(bag.get_string("type"), Some("LookAt"));
    }
}
