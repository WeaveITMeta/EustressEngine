//! `CylindricalConstraint` spawner — slide + spin about one axis.
//!
//! ## Avian mapping (APPROXIMATION)
//!
//! Roblox's `CylindricalConstraint` permits BOTH translation along an
//! axis AND rotation about that same axis (two coupled, independently
//! actuated degrees of freedom) — like a drawer slide that can also twist,
//! or a screw shaft.
//!
//! **Avian 0.6 has no native cylindrical joint and cannot cleanly
//! compound two joints on the same body pair** (two solver constraints on
//! the same `(body1, body2)` fight each other and jitter). We therefore
//! approximate with a single [`avian3d::prelude::PrismaticJoint`] that
//! captures the *slide* DOF (the dominant engineering use: pistons,
//! elevators, telescoping shafts), bounded by `lower_limit..upper_limit`.
//!
//! The free-spin DOF about the slider axis is **dropped** in this pass
//! (a `PrismaticJoint` locks all relative rotation). The linear actuator
//! (`actuator_type` / `motor_max_force` / `servo_target`) and the angular
//! actuator (`angular_actuator_type` / `angular_max_torque` /
//! `angular_servo_target`) are recorded on the component but not yet wired
//! into Avian's [`avian3d::prelude::LinearMotor`] — the same "actuator
//! fields documented but passive" stance the Wave 3.D
//! [`super::prismatic`] spawner takes.
//!
//! ## Attachment ref resolution
//!
//! See [`super::rod`] — placeholder bodies, downstream resolver patches
//! `body1`/`body2` from `attachment0`/`attachment1`.
//!
//! [`CylindricalConstraint`]: eustress_common::classes::CylindricalConstraint

use bevy::prelude::*;

use avian3d::prelude::{DistanceLimit, PrismaticJoint};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, CylindricalConstraint, PropertyValue};

use super::instance_from_bag;
use super::rod::read_meta;
use super::weld::read_optional_part_ref;

/// [`ClassSpawner`] for `ClassName::CylindricalConstraint`.
#[derive(Default)]
pub struct CylindricalConstraintSpawner;

impl ClassSpawner for CylindricalConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::CylindricalConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let attachment0 = read_optional_part_ref(props, "attachment0");
        let attachment1 = read_optional_part_ref(props, "attachment1");
        let lower_limit = props.get_f32("lower_limit").unwrap_or(-5.0);
        let upper_limit = props.get_f32("upper_limit").unwrap_or(5.0);
        let motor_max_force = props.get_f32("motor_max_force").unwrap_or(0.0);
        let servo_target = props.get_f32("servo_target").unwrap_or(0.0);
        let angular_max_torque = props.get_f32("angular_max_torque").unwrap_or(0.0);
        let angular_servo_target = props.get_f32("angular_servo_target").unwrap_or(0.0);
        let actuator_type = props
            .get_string("actuator_type")
            .unwrap_or("None")
            .to_string();
        let angular_actuator_type = props
            .get_string("angular_actuator_type")
            .unwrap_or("None")
            .to_string();

        let cyl = CylindricalConstraint {
            attachment0,
            attachment1,
            lower_limit,
            upper_limit,
            motor_max_force,
            servo_target,
            angular_max_torque,
            angular_servo_target,
            actuator_type,
            angular_actuator_type,
        };

        let instance = instance_from_bag(ClassName::CylindricalConstraint, props);
        let name = instance.name.clone();

        // Approximation: prismatic slide only — slider axis defaults to X
        // (Avian's default); the downstream resolver may reorient it from
        // the attachment basis. Slide limits applied directly.
        let mut joint = PrismaticJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER);
        joint.limits = Some(DistanceLimit::new(lower_limit, upper_limit));

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                cyl,
                joint,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<CylindricalConstraint>(entity);
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
        if let Some(v) = rbx.property("LowerLimit").and_then(|v| v.as_f32()) {
            bag.set("lower_limit", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("UpperLimit").and_then(|v| v.as_f32()) {
            bag.set("upper_limit", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("MotorMaxForce").and_then(|v| v.as_f32()) {
            bag.set("motor_max_force", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("ServoTarget").and_then(|v| v.as_f32()) {
            bag.set("servo_target", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("AngularActuatorType").and_then(|v| v.as_str().map(str::to_string)) {
            bag.set("angular_actuator_type", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("ActuatorType").and_then(|v| v.as_str().map(str::to_string)) {
            bag.set("actuator_type", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(14);
        read_meta(toml_value, &mut bag);

        if let Some(props) = toml_value.get("properties") {
            if let Some(p) = props.get("attachment0").and_then(|v| v.as_integer()) {
                bag.set("attachment0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("attachment1").and_then(|v| v.as_integer()) {
                bag.set("attachment1", PropertyValue::Int(p as i32));
            }
            if let Some(v) = props.get("lower_limit").and_then(|v| v.as_float()) {
                bag.set("lower_limit", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("upper_limit").and_then(|v| v.as_float()) {
                bag.set("upper_limit", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("motor_max_force").and_then(|v| v.as_float()) {
                bag.set("motor_max_force", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("servo_target").and_then(|v| v.as_float()) {
                bag.set("servo_target", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("angular_max_torque").and_then(|v| v.as_float()) {
                bag.set("angular_max_torque", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props
                .get("angular_servo_target")
                .and_then(|v| v.as_float())
            {
                bag.set("angular_servo_target", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("actuator_type").and_then(|v| v.as_str()) {
                bag.set("actuator_type", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("angular_actuator_type").and_then(|v| v.as_str()) {
                bag.set("angular_actuator_type", PropertyValue::String(v.to_string()));
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
                toml::Value::String("CylindricalConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(c) = world.get::<CylindricalConstraint>(entity) {
            if let Some(p) = c.attachment0 {
                props.insert("attachment0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = c.attachment1 {
                props.insert("attachment1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("lower_limit".into(), toml::Value::Float(c.lower_limit as f64));
            props.insert("upper_limit".into(), toml::Value::Float(c.upper_limit as f64));
            props.insert(
                "motor_max_force".into(),
                toml::Value::Float(c.motor_max_force as f64),
            );
            props.insert(
                "servo_target".into(),
                toml::Value::Float(c.servo_target as f64),
            );
            props.insert(
                "angular_max_torque".into(),
                toml::Value::Float(c.angular_max_torque as f64),
            );
            props.insert(
                "angular_servo_target".into(),
                toml::Value::Float(c.angular_servo_target as f64),
            );
            props.insert(
                "actuator_type".into(),
                toml::Value::String(c.actuator_type.clone()),
            );
            props.insert(
                "angular_actuator_type".into(),
                toml::Value::String(c.angular_actuator_type.clone()),
            );
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
