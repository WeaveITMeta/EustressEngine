//! `PrismaticConstraint` spawner — sliding-axis joint.
//!
//! ## Avian mapping
//!
//! [`PrismaticConstraint`] → [`avian3d::prelude::PrismaticJoint`].
//!
//! The Eustress struct carries:
//! - `axis` (`Vec3`, local) — slider axis, passed to
//!   [`PrismaticJoint::with_slider_axis`].
//! - `lower_limit` / `upper_limit` (`Option<f32>`) — both present means
//!   a finite [`avian3d::prelude::DistanceLimit`]; either absent means
//!   the joint slides without a translation limit.
//! - `motor_velocity` / `motor_max_force` — currently NOT wired into
//!   Avian's [`avian3d::prelude::LinearMotor`]. Once Wave 4's
//!   physics-LOD pass touches motors, this spawner adds the motor
//!   configuration. Today the motor fields are documented but the joint
//!   is passive.
//!
//! ## Entity ref resolution + `enabled`
//!
//! See [`super::weld`] — same patterns.
//!
//! [`PrismaticConstraint`]: eustress_common::classes::PrismaticConstraint

use bevy::prelude::*;

use avian3d::prelude::{DistanceLimit, JointDisabled, PrismaticJoint};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PrismaticConstraint, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::weld::{read_optional_part_ref, read_transform};

/// [`ClassSpawner`] for `ClassName::PrismaticConstraint`.
#[derive(Default)]
pub struct PrismaticConstraintSpawner;

impl ClassSpawner for PrismaticConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::PrismaticConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let axis = props.get_vec3("axis").unwrap_or(Vec3::X);
        let lower_limit = props.get_f32("lower_limit");
        let upper_limit = props.get_f32("upper_limit");
        let motor_velocity = props.get_f32("motor_velocity").unwrap_or(0.0);
        let motor_max_force = props.get_f32("motor_max_force").unwrap_or(0.0);
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let prism = PrismaticConstraint {
            part0,
            part1,
            c0,
            c1,
            axis,
            lower_limit,
            upper_limit,
            motor_velocity,
            motor_max_force,
            enabled,
        };

        let instance = instance_from_bag(ClassName::PrismaticConstraint, props);
        let name = instance.name.clone();

        let mut joint = PrismaticJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_slider_axis(axis)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation);

        if let (Some(lo), Some(hi)) = (lower_limit, upper_limit) {
            joint.limits = Some(DistanceLimit::new(lo, hi));
        }

        let mut ec = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            prism,
            joint,
            Name::new(name),
        ));
        if !enabled {
            ec.insert(JointDisabled);
        }
        ec.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<PrismaticConstraint>(entity);
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
        let mut bag = PropertyBag::with_capacity(14);

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
            if let Some(v) = props.get("lower_limit").and_then(|v| v.as_float()) {
                bag.set("lower_limit", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("upper_limit").and_then(|v| v.as_float()) {
                bag.set("upper_limit", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("motor_velocity").and_then(|v| v.as_float()) {
                bag.set("motor_velocity", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("motor_max_force").and_then(|v| v.as_float()) {
                bag.set("motor_max_force", PropertyValue::Float(v as f32));
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
                toml::Value::String("PrismaticConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(p) = world.get::<PrismaticConstraint>(entity) {
            if let Some(v) = p.part0 {
                props.insert("part0".into(), toml::Value::Integer(v as i64));
            }
            if let Some(v) = p.part1 {
                props.insert("part1".into(), toml::Value::Integer(v as i64));
            }
            props.insert("c0".into(), vec3_to_toml(p.c0.translation));
            props.insert("c1".into(), vec3_to_toml(p.c1.translation));
            props.insert("axis".into(), vec3_to_toml(p.axis));
            if let Some(v) = p.lower_limit {
                props.insert("lower_limit".into(), toml::Value::Float(v as f64));
            }
            if let Some(v) = p.upper_limit {
                props.insert("upper_limit".into(), toml::Value::Float(v as f64));
            }
            props.insert(
                "motor_velocity".into(),
                toml::Value::Float(p.motor_velocity as f64),
            );
            props.insert(
                "motor_max_force".into(),
                toml::Value::Float(p.motor_max_force as f64),
            );
            props.insert("enabled".into(), toml::Value::Boolean(p.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
