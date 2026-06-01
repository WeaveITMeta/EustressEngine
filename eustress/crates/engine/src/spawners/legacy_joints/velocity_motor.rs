//! `VelocityMotorSpawner` — `ClassSpawner` for [`ClassName::VelocityMotor`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.A). Config-attach with
//! the Avian drive wiring deferred (see the group [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, VelocityMotor};
use eustress_common::{Attributes, Tags};

use super::{
    apply_metadata_edit, export_metadata, import_metadata, insert_optional_ref, instance_from_bag,
    read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::VelocityMotor`].
#[derive(Default)]
pub struct VelocityMotorSpawner;

impl ClassSpawner for VelocityMotorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::VelocityMotor
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::VelocityMotor, props);
        let name = instance.name.clone();
        let d = VelocityMotor::default();
        let comp = VelocityMotor {
            part0: read_optional_ref(props, "part0"),
            hole: read_optional_ref(props, "hole"),
            desired_angle: props.get_f32("desired_angle").unwrap_or(d.desired_angle),
            max_velocity: props.get_f32("max_velocity").unwrap_or(d.max_velocity),
        };
        // TODO(avian): wire a velocity-driven revolute joint once a ref→Entity
        // resolver exists on this branch.

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
        if let Some(mut comp) = world.get_mut::<VelocityMotor>(entity) {
            if props.get("part0").is_some() { comp.part0 = read_optional_ref(props, "part0"); }
            if props.get("hole").is_some() { comp.hole = read_optional_ref(props, "hole"); }
            if let Some(v) = props.get_f32("desired_angle") { comp.desired_angle = v; }
            if let Some(v) = props.get_f32("max_velocity") { comp.max_velocity = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Part0").and_then(|p| p.as_i32()) {
            bag.set("part0", PropertyValue::Int(v));
        }
        if let Some(v) = rbx.property("Hole").and_then(|p| p.as_i32()) {
            bag.set("hole", PropertyValue::Int(v));
        }
        for (rbx_key, key) in [("DesiredAngle", "desired_angle"), ("MaxVelocity", "max_velocity")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["part0", "hole"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
            for key in ["desired_angle", "max_velocity"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "VelocityMotor")),
        );
        if let Some(comp) = world.get::<VelocityMotor>(entity) {
            let mut props = toml::value::Table::new();
            insert_optional_ref(&mut props, "part0", comp.part0);
            insert_optional_ref(&mut props, "hole", comp.hole);
            props.insert("desired_angle".into(), toml::Value::Float(comp.desired_angle as f64));
            props.insert("max_velocity".into(), toml::Value::Float(comp.max_velocity as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_velocity_motor() {
        assert_eq!(VelocityMotorSpawner.class_name(), ClassName::VelocityMotor);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(VelocityMotorSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "VelocityMotor"
            name = "VM"
            [properties]
            hole = 4
            max_velocity = 0.25
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = VelocityMotorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("hole"), Some(4));
        assert_eq!(bag.get_f32("max_velocity"), Some(0.25));
    }
}
