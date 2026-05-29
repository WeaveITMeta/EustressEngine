//! `DistanceConstraint` spawner — min/max distance joint.
//!
//! ## Avian mapping
//!
//! [`DistanceConstraint`] → [`avian3d::prelude::DistanceJoint`] with both
//! `min` and `max` set from the Eustress struct.
//!
//! Avian's `DistanceJoint::with_limits(min, max)` clamps `min <= max`;
//! the Eustress struct is also documented as "0.0 = no minimum,
//! f32::MAX = no maximum" — we pass both through directly. A `min ==
//! max` configuration produces a rigid distance (Avian handles this
//! correctly per its `DistanceLimit::ZERO` semantics).
//!
//! ## Entity ref resolution + `enabled`
//!
//! See [`super::weld`] — same patterns.
//!
//! [`DistanceConstraint`]: eustress_common::classes::DistanceConstraint

use bevy::prelude::*;

use avian3d::prelude::{DistanceJoint, JointDisabled};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, DistanceConstraint, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::weld::{read_optional_part_ref, read_transform};

/// [`ClassSpawner`] for `ClassName::DistanceConstraint`.
#[derive(Default)]
pub struct DistanceConstraintSpawner;

impl ClassSpawner for DistanceConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::DistanceConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let min_distance = props.get_f32("min_distance").unwrap_or(0.0);
        let max_distance = props.get_f32("max_distance").unwrap_or(5.0);
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let dist = DistanceConstraint {
            part0,
            part1,
            c0,
            c1,
            min_distance,
            max_distance,
            enabled,
        };

        let instance = instance_from_bag(ClassName::DistanceConstraint, props);
        let name = instance.name.clone();

        let joint = DistanceJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation)
            .with_limits(min_distance, max_distance);

        let mut ec = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            dist,
            joint,
            Name::new(name),
        ));
        if !enabled {
            ec.insert(JointDisabled);
        }
        ec.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<DistanceConstraint>(entity);
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
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(12);

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
            if let Some(d) = props.get("min_distance").and_then(|v| v.as_float()) {
                bag.set("min_distance", PropertyValue::Float(d as f32));
            }
            if let Some(d) = props.get("max_distance").and_then(|v| v.as_float()) {
                bag.set("max_distance", PropertyValue::Float(d as f32));
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
                toml::Value::String("DistanceConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(d) = world.get::<DistanceConstraint>(entity) {
            if let Some(p) = d.part0 {
                props.insert("part0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = d.part1 {
                props.insert("part1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("c0".into(), vec3_to_toml(d.c0.translation));
            props.insert("c1".into(), vec3_to_toml(d.c1.translation));
            props.insert(
                "min_distance".into(),
                toml::Value::Float(d.min_distance as f64),
            );
            props.insert(
                "max_distance".into(),
                toml::Value::Float(d.max_distance as f64),
            );
            props.insert("enabled".into(), toml::Value::Boolean(d.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
