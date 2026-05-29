//! `BallSocketConstraint` spawner — spherical (3-DOF rotation) joint.
//!
//! ## Avian mapping
//!
//! [`BallSocketConstraint`] → [`avian3d::prelude::SphericalJoint`].
//!
//! The Eustress struct carries a single `cone_angle` (`Option<f32>`)
//! cone-limit half-angle. When present, we map it to Avian's swing limit
//! via [`avian3d::prelude::AngleLimit::new`] with `-cone_angle ..
//! +cone_angle` — Avian models swing as a signed angle range about the
//! [`SphericalJoint::twist_axis`]. Avian's twist limit is left unset
//! (Eustress doesn't model twist independently today).
//!
//! ## Entity ref resolution + `enabled`
//!
//! See [`super::weld`] — same patterns.
//!
//! [`BallSocketConstraint`]: eustress_common::classes::BallSocketConstraint

use bevy::prelude::*;

use avian3d::prelude::{AngleLimit, JointDisabled, SphericalJoint};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BallSocketConstraint, ClassName, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::weld::{read_optional_part_ref, read_transform};

/// [`ClassSpawner`] for `ClassName::BallSocketConstraint`.
#[derive(Default)]
pub struct BallSocketConstraintSpawner;

impl ClassSpawner for BallSocketConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BallSocketConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let cone_angle = props.get_f32("cone_angle");
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let ball = BallSocketConstraint {
            part0,
            part1,
            c0,
            c1,
            cone_angle,
            enabled,
        };

        let instance = instance_from_bag(ClassName::BallSocketConstraint, props);
        let name = instance.name.clone();

        let mut joint = SphericalJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation);

        if let Some(half) = cone_angle {
            // Eustress represents the cone as a single half-angle (max
            // deviation from the twist axis); Avian's swing limit is a
            // signed range about the twist axis.
            joint.swing_limit = Some(AngleLimit::new(-half, half));
        }

        let mut ec = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            ball,
            joint,
            Name::new(name),
        ));
        if !enabled {
            ec.insert(JointDisabled);
        }
        ec.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<BallSocketConstraint>(entity);
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
            if let Some(a) = props.get("cone_angle").and_then(|v| v.as_float()) {
                bag.set("cone_angle", PropertyValue::Float(a as f32));
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
                toml::Value::String("BallSocketConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(b) = world.get::<BallSocketConstraint>(entity) {
            if let Some(p) = b.part0 {
                props.insert("part0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = b.part1 {
                props.insert("part1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("c0".into(), vec3_to_toml(b.c0.translation));
            props.insert("c1".into(), vec3_to_toml(b.c1.translation));
            if let Some(a) = b.cone_angle {
                props.insert("cone_angle".into(), toml::Value::Float(a as f64));
            }
            props.insert("enabled".into(), toml::Value::Boolean(b.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
