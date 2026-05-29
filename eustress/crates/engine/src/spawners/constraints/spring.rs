//! `SpringConstraint` spawner — distance joint with compliance + damping.
//!
//! ## Avian mapping
//!
//! [`SpringConstraint`] → [`avian3d::prelude::DistanceJoint`] (rigid
//! `min == max == rest_length`) plus:
//! - [`avian3d::prelude::DistanceJoint::with_compliance`] —
//!   `compliance = 1 / stiffness` (Avian uses compliance, the
//!   reciprocal of XPBD stiffness).
//! - [`avian3d::prelude::JointDamping`] — `linear` reads the Eustress
//!   `damping` field; angular damping is left at the Avian default
//!   (spring constraints model linear oscillation only).
//!
//! Per CLASS_REGISTRY.md §8.3 mapping table: "SpringConstraint →
//! DistanceJoint+stiffness".
//!
//! When `stiffness == 0` the Eustress doc says "infinitely stiff" — we
//! pass `compliance = 0` to Avian, which means perfectly rigid (the same
//! semantic).
//!
//! ## Entity ref resolution + `enabled`
//!
//! See [`super::weld`] — same patterns.
//!
//! [`SpringConstraint`]: eustress_common::classes::SpringConstraint

use bevy::prelude::*;

use avian3d::prelude::{DistanceJoint, JointDamping, JointDisabled};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, SpringConstraint};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::weld::{read_optional_part_ref, read_transform};

/// [`ClassSpawner`] for `ClassName::SpringConstraint`.
#[derive(Default)]
pub struct SpringConstraintSpawner;

impl ClassSpawner for SpringConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::SpringConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let rest_length = props.get_f32("rest_length").unwrap_or(5.0);
        let stiffness = props.get_f32("stiffness").unwrap_or(100.0);
        let damping = props.get_f32("damping").unwrap_or(1.0);
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let spring = SpringConstraint {
            part0,
            part1,
            c0,
            c1,
            rest_length,
            stiffness,
            damping,
            enabled,
        };

        let instance = instance_from_bag(ClassName::SpringConstraint, props);
        let name = instance.name.clone();

        // compliance = 1/stiffness; 0 stiffness is documented as
        // "infinitely stiff", which maps to compliance = 0 in Avian (same
        // semantic).
        let compliance = if stiffness <= 0.0 { 0.0 } else { 1.0 / stiffness };

        let joint = DistanceJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation)
            .with_limits(rest_length, rest_length)
            .with_compliance(compliance);

        let joint_damping = JointDamping {
            linear: damping,
            angular: 0.0,
        };

        let mut ec = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            spring,
            joint,
            joint_damping,
            Name::new(name),
        ));
        if !enabled {
            ec.insert(JointDisabled);
        }
        ec.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<SpringConstraint>(entity);
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
        if let Some(f) = rbx.property("FreeLength").and_then(|v| v.as_f32()) {
            bag.set("rest_length", PropertyValue::Float(f));
        }
        if let Some(f) = rbx.property("Stiffness").and_then(|v| v.as_f32()) {
            bag.set("stiffness", PropertyValue::Float(f));
        }
        if let Some(f) = rbx.property("Damping").and_then(|v| v.as_f32()) {
            bag.set("damping", PropertyValue::Float(f));
        }
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
            if let Some(v) = props.get("rest_length").and_then(|v| v.as_float()) {
                bag.set("rest_length", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("stiffness").and_then(|v| v.as_float()) {
                bag.set("stiffness", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("damping").and_then(|v| v.as_float()) {
                bag.set("damping", PropertyValue::Float(v as f32));
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
                toml::Value::String("SpringConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(s) = world.get::<SpringConstraint>(entity) {
            if let Some(p) = s.part0 {
                props.insert("part0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = s.part1 {
                props.insert("part1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("c0".into(), vec3_to_toml(s.c0.translation));
            props.insert("c1".into(), vec3_to_toml(s.c1.translation));
            props.insert(
                "rest_length".into(),
                toml::Value::Float(s.rest_length as f64),
            );
            props.insert(
                "stiffness".into(),
                toml::Value::Float(s.stiffness as f64),
            );
            props.insert("damping".into(), toml::Value::Float(s.damping as f64));
            props.insert("enabled".into(), toml::Value::Boolean(s.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
