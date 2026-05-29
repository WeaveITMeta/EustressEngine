//! `RopeConstraint` spawner — slack-allowed maximum-distance joint.
//!
//! ## Avian mapping
//!
//! [`RopeConstraint`] → [`avian3d::prelude::DistanceJoint`] with `min =
//! 0`, `max = length`. Per CLASS_REGISTRY.md §8.3 mapping table:
//! "RopeConstraint → DistanceJoint with max_length only".
//!
//! Rope semantics:
//! - Bodies may approach each other freely (`min = 0`, the rope
//!   goes slack).
//! - Bodies are prevented from separating beyond `length` (the rope
//!   pulls taut).
//!
//! ## Entity ref resolution + `enabled`
//!
//! See [`super::weld`] — same patterns.
//!
//! [`RopeConstraint`]: eustress_common::classes::RopeConstraint

use bevy::prelude::*;

use avian3d::prelude::{DistanceJoint, JointDisabled};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, RopeConstraint};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::weld::{read_optional_part_ref, read_transform};

/// [`ClassSpawner`] for `ClassName::RopeConstraint`.
#[derive(Default)]
pub struct RopeConstraintSpawner;

impl ClassSpawner for RopeConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::RopeConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let length = props.get_f32("length").unwrap_or(10.0);
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let rope = RopeConstraint {
            part0,
            part1,
            c0,
            c1,
            length,
            enabled,
        };

        let instance = instance_from_bag(ClassName::RopeConstraint, props);
        let name = instance.name.clone();

        // Rope = slack-allowed max-distance constraint.
        let joint = DistanceJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation)
            .with_limits(0.0, length);

        let mut ec = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            rope,
            joint,
            Name::new(name),
        ));
        if !enabled {
            ec.insert(JointDisabled);
        }
        ec.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<RopeConstraint>(entity);
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, _world: &mut World, _entity: Entity, _props: &PropertyBag) -> bool {
        true
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::new();
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(f) = rbx.property("Length").and_then(|v| v.as_f32()) {
            bag.set("length", PropertyValue::Float(f));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(10);

        if let Some(name) = toml_value
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
        {
            bag.set("metadata.name", PropertyValue::String(name.to_string()));
        }
        if let Some(uuid) = toml_value
            .get("metadata")
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
        {
            bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
        }

        if let Some(props) = toml_value.get("properties") {
            if let Some(p) = props.get("part0").and_then(|v| v.as_integer()) {
                bag.set("part0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("part1").and_then(|v| v.as_integer()) {
                bag.set("part1", PropertyValue::Int(p as i32));
            }
            if let Some(c0) = props.get("c0").and_then(|v| v.as_array()) {
                bag.set("c0", PropertyValue::Vector3(read_vec3_array(c0)));
            }
            if let Some(c1) = props.get("c1").and_then(|v| v.as_array()) {
                bag.set("c1", PropertyValue::Vector3(read_vec3_array(c1)));
            }
            if let Some(v) = props.get("length").and_then(|v| v.as_float()) {
                bag.set("length", PropertyValue::Float(v as f32));
            }
            if let Some(e) = props.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("enabled", PropertyValue::Bool(e));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();
        let mut props = toml::value::Table::new();

        if let Some(instance) = world.get::<eustress_common::classes::Instance>(entity) {
            meta.insert("name".into(), toml::Value::String(instance.name.clone()));
            meta.insert(
                "class_name".into(),
                toml::Value::String("RopeConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(r) = world.get::<RopeConstraint>(entity) {
            if let Some(p) = r.part0 {
                props.insert("part0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = r.part1 {
                props.insert("part1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("c0".into(), vec3_to_toml(r.c0.translation));
            props.insert("c1".into(), vec3_to_toml(r.c1.translation));
            props.insert("length".into(), toml::Value::Float(r.length as f64));
            props.insert("enabled".into(), toml::Value::Boolean(r.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
