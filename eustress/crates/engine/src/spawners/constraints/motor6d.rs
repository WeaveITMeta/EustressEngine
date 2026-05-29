//! `Motor6D` spawner — animated articulation joint.
//!
//! ## Avian mapping
//!
//! [`Motor6D`] → [`avian3d::prelude::RevoluteJoint`] (the closest
//! Avian-supported analogue to Roblox's Motor6D: a hinge with an
//! animated/desired angle). Roblox's Motor6D supports both rotation and
//! translation but in practice is used overwhelmingly for animation
//! rigs where rotation about a single bind-pose axis is sufficient.
//!
//! The `desired_angle` and `max_velocity` fields are NOT wired into an
//! [`avian3d::prelude::AngularMotor`] yet — the animation system reads
//! `Motor6D::transform` directly each frame and writes the resulting
//! local pose. A Wave 4 task may map `desired_angle` to a real motor
//! once the animation-driven path is reconciled with Avian's solver.
//!
//! ## Entity ref resolution
//!
//! Same as [`super::weld`] — joint attaches with [`Entity::PLACEHOLDER`]
//! on both bodies; a downstream resolver patches `body1`/`body2`.
//!
//! ## `enabled`
//!
//! Motor6D has no explicit `enabled` field today — every Motor6D is
//! active in its bind pose. The Avian joint is inserted unconditionally;
//! adding a runtime disable flag is a struct-field change tracked under
//! the lighting-audit follow-ups.
//!
//! [`Motor6D`]: eustress_common::classes::Motor6D

use bevy::prelude::*;

use avian3d::prelude::RevoluteJoint;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Motor6D, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::weld::{read_optional_part_ref, read_transform};

/// [`ClassSpawner`] for `ClassName::Motor6D`.
#[derive(Default)]
pub struct Motor6DSpawner;

impl ClassSpawner for Motor6DSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Motor6D
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let desired_angle = props.get_f32("desired_angle").unwrap_or(0.0);
        let max_velocity = props.get_f32("max_velocity").unwrap_or(0.1);

        let motor = Motor6D {
            part0,
            part1,
            c0,
            c1,
            transform: Transform::IDENTITY,
            desired_angle,
            max_velocity,
        };

        let instance = instance_from_bag(ClassName::Motor6D, props);
        let name = instance.name.clone();

        // RevoluteJoint with bind-pose anchors. The hinge axis defaults
        // to Vector::Z (Avian's default) — animation systems override the
        // local rotation per-frame regardless.
        let joint = RevoluteJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                motor,
                joint,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<Motor6D>(entity);
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

        if let Some(a) = rbx.property("DesiredAngle").and_then(|v| v.as_f32()) {
            bag.set("desired_angle", PropertyValue::Float(a));
        }
        if let Some(v) = rbx.property("MaxVelocity").and_then(|v| v.as_f32()) {
            bag.set("max_velocity", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);

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
            if let Some(a) = props.get("desired_angle").and_then(|v| v.as_float()) {
                bag.set("desired_angle", PropertyValue::Float(a as f32));
            }
            if let Some(v) = props.get("max_velocity").and_then(|v| v.as_float()) {
                bag.set("max_velocity", PropertyValue::Float(v as f32));
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
                toml::Value::String("Motor6D".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(motor) = world.get::<Motor6D>(entity) {
            if let Some(p) = motor.part0 {
                props.insert("part0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = motor.part1 {
                props.insert("part1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("c0".into(), vec3_to_toml(motor.c0.translation));
            props.insert("c1".into(), vec3_to_toml(motor.c1.translation));
            props.insert(
                "desired_angle".into(),
                toml::Value::Float(motor.desired_angle as f64),
            );
            props.insert(
                "max_velocity".into(),
                toml::Value::Float(motor.max_velocity as f64),
            );
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
