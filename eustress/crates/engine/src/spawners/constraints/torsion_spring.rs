//! `TorsionSpringConstraint` spawner — angular spring about one axis.
//!
//! ## Avian mapping
//!
//! [`TorsionSpringConstraint`] → [`avian3d::prelude::RevoluteJoint`]
//! (rotation about a single axis) plus an *angular* spring:
//! - the joint's `align_compliance` is set to `1 / stiffness` (compliance
//!   is the reciprocal of XPBD stiffness, the same mapping
//!   [`super::spring`] uses for its linear `with_compliance`);
//! - [`avian3d::prelude::JointDamping`] with `angular` set from the
//!   Eustress `damping` field (linear damping stays at the Avian default).
//!
//! `max_torque` (the restoring-torque ceiling) and `restitution`
//! (bounciness) have no direct single-field analogue on Avian's revolute
//! joint; they are recorded on the component for a future exact mapping.
//!
//! `stiffness == 0` ⇒ compliance `0` (perfectly rigid), the same semantic
//! the linear spring uses.
//!
//! ## Attachment ref resolution
//!
//! See [`super::rod`] — placeholder bodies; downstream resolver patches
//! `body1`/`body2` from `attachment0`/`attachment1`.
//!
//! [`TorsionSpringConstraint`]: eustress_common::classes::TorsionSpringConstraint

use bevy::prelude::*;

use avian3d::prelude::{JointDamping, RevoluteJoint};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, TorsionSpringConstraint};

use super::instance_from_bag;
use super::rod::read_meta;
use super::weld::read_optional_part_ref;

/// [`ClassSpawner`] for `ClassName::TorsionSpringConstraint`.
#[derive(Default)]
pub struct TorsionSpringConstraintSpawner;

impl ClassSpawner for TorsionSpringConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::TorsionSpringConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let attachment0 = read_optional_part_ref(props, "attachment0");
        let attachment1 = read_optional_part_ref(props, "attachment1");
        let stiffness = props.get_f32("stiffness").unwrap_or(100.0);
        let damping = props.get_f32("damping").unwrap_or(10.0);
        let max_torque = props.get_f32("max_torque").unwrap_or(1000.0);
        let restitution = props.get_f32("restitution").unwrap_or(0.0);

        let torsion = TorsionSpringConstraint {
            attachment0,
            attachment1,
            stiffness,
            damping,
            max_torque,
            restitution,
        };

        let instance = instance_from_bag(ClassName::TorsionSpringConstraint, props);
        let name = instance.name.clone();

        // compliance = 1/stiffness; 0 stiffness ⇒ compliance 0 (rigid),
        // matching the linear SpringConstraint semantic.
        let compliance = if stiffness <= 0.0 { 0.0 } else { 1.0 / stiffness };

        let mut joint = RevoluteJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER);
        // Angular spring stiffness about the hinge axis.
        joint.align_compliance = compliance;

        let joint_damping = JointDamping {
            linear: 0.0,
            angular: damping,
        };

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                torsion,
                joint,
                joint_damping,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<TorsionSpringConstraint>(entity);
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
        if let Some(s) = rbx.property("Stiffness").and_then(|v| v.as_f32()) {
            bag.set("stiffness", PropertyValue::Float(s));
        }
        if let Some(d) = rbx.property("Damping").and_then(|v| v.as_f32()) {
            bag.set("damping", PropertyValue::Float(d));
        }
        if let Some(t) = rbx.property("MaxTorque").and_then(|v| v.as_f32()) {
            bag.set("max_torque", PropertyValue::Float(t));
        }
        if let Some(r) = rbx.property("Restitution").and_then(|v| v.as_f32()) {
            bag.set("restitution", PropertyValue::Float(r));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(10);
        read_meta(toml_value, &mut bag);

        if let Some(props) = toml_value.get("properties") {
            if let Some(p) = props.get("attachment0").and_then(|v| v.as_integer()) {
                bag.set("attachment0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("attachment1").and_then(|v| v.as_integer()) {
                bag.set("attachment1", PropertyValue::Int(p as i32));
            }
            if let Some(s) = props.get("stiffness").and_then(|v| v.as_float()) {
                bag.set("stiffness", PropertyValue::Float(s as f32));
            }
            if let Some(d) = props.get("damping").and_then(|v| v.as_float()) {
                bag.set("damping", PropertyValue::Float(d as f32));
            }
            if let Some(t) = props.get("max_torque").and_then(|v| v.as_float()) {
                bag.set("max_torque", PropertyValue::Float(t as f32));
            }
            if let Some(r) = props.get("restitution").and_then(|v| v.as_float()) {
                bag.set("restitution", PropertyValue::Float(r as f32));
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
                toml::Value::String("TorsionSpringConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(t) = world.get::<TorsionSpringConstraint>(entity) {
            if let Some(p) = t.attachment0 {
                props.insert("attachment0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = t.attachment1 {
                props.insert("attachment1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("stiffness".into(), toml::Value::Float(t.stiffness as f64));
            props.insert("damping".into(), toml::Value::Float(t.damping as f64));
            props.insert("max_torque".into(), toml::Value::Float(t.max_torque as f64));
            props.insert("restitution".into(), toml::Value::Float(t.restitution as f64));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
