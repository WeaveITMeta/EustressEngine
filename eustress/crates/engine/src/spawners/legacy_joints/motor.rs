//! `MotorSpawner` — `ClassSpawner` for [`ClassName::Motor`] (legacy motor).
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.A). Config-attach with
//! the Avian joint wiring deferred (see the group [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Motor, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{
    anchor_to_toml, apply_metadata_edit, export_metadata, import_metadata, insert_optional_ref,
    instance_from_bag, read_anchor_array, read_anchor_transform, read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::Motor`].
#[derive(Default)]
pub struct MotorSpawner;

impl ClassSpawner for MotorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Motor
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Motor, props);
        let name = instance.name.clone();
        let d = Motor::default();
        let comp = Motor {
            part0: read_optional_ref(props, "part0"),
            part1: read_optional_ref(props, "part1"),
            desired_angle: props.get_f32("desired_angle").unwrap_or(d.desired_angle),
            max_velocity: props.get_f32("max_velocity").unwrap_or(d.max_velocity),
            c0: read_anchor_transform(props, "c0"),
            c1: read_anchor_transform(props, "c1"),
        };
        // TODO(avian): wire a `RevoluteJoint` + motor drive once a Part0/Part1
        // → Entity resolver system exists on this branch.

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
        if let Some(mut comp) = world.get_mut::<Motor>(entity) {
            if props.get("part0").is_some() { comp.part0 = read_optional_ref(props, "part0"); }
            if props.get("part1").is_some() { comp.part1 = read_optional_ref(props, "part1"); }
            if let Some(v) = props.get_f32("desired_angle") { comp.desired_angle = v; }
            if let Some(v) = props.get_f32("max_velocity") { comp.max_velocity = v; }
            if props.get("c0").is_some() { comp.c0 = read_anchor_transform(props, "c0"); }
            if props.get("c1").is_some() { comp.c1 = read_anchor_transform(props, "c1"); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [("Part0", "part0"), ("Part1", "part1")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_i32()) {
                bag.set(key, PropertyValue::Int(v));
            }
        }
        for (rbx_key, key) in [("DesiredAngle", "desired_angle"), ("MaxVelocity", "max_velocity")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(7);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["part0", "part1"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
            for key in ["desired_angle", "max_velocity"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            for key in ["c0", "c1"] {
                if let Some(arr) = props.get(key).and_then(|v| v.as_array()) {
                    bag.set(key, PropertyValue::Transform(read_anchor_array(arr)));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Motor")),
        );
        if let Some(comp) = world.get::<Motor>(entity) {
            let mut props = toml::value::Table::new();
            insert_optional_ref(&mut props, "part0", comp.part0);
            insert_optional_ref(&mut props, "part1", comp.part1);
            props.insert("desired_angle".into(), toml::Value::Float(comp.desired_angle as f64));
            props.insert("max_velocity".into(), toml::Value::Float(comp.max_velocity as f64));
            props.insert("c0".into(), anchor_to_toml(comp.c0));
            props.insert("c1".into(), anchor_to_toml(comp.c1));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_motor() {
        assert_eq!(MotorSpawner.class_name(), ClassName::Motor);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(MotorSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "Motor"
            name = "M"
            [properties]
            desired_angle = 1.57
            max_velocity = 0.5
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = MotorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("desired_angle"), Some(1.57));
        assert_eq!(bag.get_f32("max_velocity"), Some(0.5));
    }
}
