//! `HingeConstraint` spawner — single-axis revolute joint.
//!
//! ## Avian mapping
//!
//! [`HingeConstraint`] → [`avian3d::prelude::RevoluteJoint`].
//!
//! The Eustress struct carries:
//! - `axis` (`Vec3`, local to `Part0`) — passed to
//!   [`RevoluteJoint::with_hinge_axis`].
//! - `lower_angle` / `upper_angle` (`Option<f32>`, radians) — both
//!   present means a finite [`avian3d::prelude::AngleLimit`]; either
//!   absent means the joint rotates without limit.
//!
//! ## Entity ref resolution + `enabled`
//!
//! See [`super::weld`] — same `Entity::PLACEHOLDER` pattern, same
//! `JointDisabled` insert when `enabled = false`.
//!
//! [`HingeConstraint`]: eustress_common::classes::HingeConstraint

use bevy::prelude::*;

use avian3d::prelude::{AngleLimit, JointDisabled, RevoluteJoint};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, HingeConstraint, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::weld::{read_optional_part_ref, read_transform};

/// [`ClassSpawner`] for `ClassName::HingeConstraint`.
#[derive(Default)]
pub struct HingeConstraintSpawner;

impl ClassSpawner for HingeConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::HingeConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let axis = props.get_vec3("axis").unwrap_or(Vec3::Y);
        let lower = props.get_f32("lower_angle");
        let upper = props.get_f32("upper_angle");
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let hinge = HingeConstraint {
            part0,
            part1,
            c0,
            c1,
            axis,
            lower_angle: lower,
            upper_angle: upper,
            enabled,
        };

        let instance = instance_from_bag(ClassName::HingeConstraint, props);
        let name = instance.name.clone();

        let mut joint = RevoluteJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_hinge_axis(axis)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation);

        if let (Some(lo), Some(hi)) = (lower, upper) {
            joint.angle_limit = Some(AngleLimit::new(lo, hi));
        }

        let mut ec = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            hinge,
            joint,
            Name::new(name),
        ));
        if !enabled {
            ec.insert(JointDisabled);
        }
        ec.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<HingeConstraint>(entity);
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
        if let Some(e) = rbx.property("Enabled").and_then(|v| v.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(e));
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
            if let Some(axis) = props.get("axis").and_then(|v| v.as_array()) {
                bag.set("axis", PropertyValue::Vector3(read_vec3_array(axis)));
            }
            if let Some(a) = props.get("lower_angle").and_then(|v| v.as_float()) {
                bag.set("lower_angle", PropertyValue::Float(a as f32));
            }
            if let Some(a) = props.get("upper_angle").and_then(|v| v.as_float()) {
                bag.set("upper_angle", PropertyValue::Float(a as f32));
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
                toml::Value::String("HingeConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(hinge) = world.get::<HingeConstraint>(entity) {
            if let Some(p) = hinge.part0 {
                props.insert("part0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = hinge.part1 {
                props.insert("part1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("c0".into(), vec3_to_toml(hinge.c0.translation));
            props.insert("c1".into(), vec3_to_toml(hinge.c1.translation));
            props.insert("axis".into(), vec3_to_toml(hinge.axis));
            if let Some(a) = hinge.lower_angle {
                props.insert("lower_angle".into(), toml::Value::Float(a as f64));
            }
            if let Some(a) = hinge.upper_angle {
                props.insert("upper_angle".into(), toml::Value::Float(a as f64));
            }
            props.insert("enabled".into(), toml::Value::Boolean(hinge.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
