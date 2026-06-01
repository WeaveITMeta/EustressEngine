//! `AngularVelocity` mover spawner — target angular velocity (config).
//!
//! ## Runtime
//!
//! Places the [`AngularVelocity`] *configuration* component (the
//! `eustress_common::classes` mover, **not** the Avian rigid-body
//! component of the same name) on a child of the part it drives. The
//! per-frame actuation lives in
//! [`crate::physics::movers::apply_angular_velocity_movers`].
//!
//! ## Avian mapping
//!
//! None at spawn time. The runtime system writes the target's Avian
//! [`AngularVelocity`](avian3d::prelude::AngularVelocity) toward the
//! configured `angular_velocity`, bounded by `max_torque`.
//!
//! [`AngularVelocity`]: eustress_common::classes::AngularVelocity

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
// The Eustress *config* mover — distinct from `avian3d::prelude::AngularVelocity`.
use eustress_common::classes::{AngularVelocity, ClassName, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::AngularVelocity`.
#[derive(Default)]
pub struct AngularVelocitySpawner;

impl ClassSpawner for AngularVelocitySpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AngularVelocity
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let angular_velocity = props.get_vec3("angular_velocity").unwrap_or(Vec3::ZERO);
        let max_torque = props.get_f32("max_torque").unwrap_or(0.0);
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let mover = AngularVelocity {
            angular_velocity,
            max_torque,
            enabled,
        };

        let instance = instance_from_bag(ClassName::AngularVelocity, props);
        let name = instance.name.clone();

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                mover,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<AngularVelocity>(entity);
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
        if let Some(t) = rbx.property("MaxTorque").and_then(|v| v.as_f32()) {
            bag.set("max_torque", PropertyValue::Float(t));
        }
        if let Some(e) = rbx.property("Enabled").and_then(|v| v.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(e));
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
            if let Some(v) = props.get("angular_velocity").and_then(|v| v.as_array()) {
                bag.set(
                    "angular_velocity",
                    PropertyValue::Vector3(read_vec3_array(v)),
                );
            }
            if let Some(t) = props.get("max_torque").and_then(|v| v.as_float()) {
                bag.set("max_torque", PropertyValue::Float(t as f32));
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
                toml::Value::String("AngularVelocity".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<AngularVelocity>(entity) {
            props.insert("angular_velocity".into(), vec3_to_toml(m.angular_velocity));
            props.insert("max_torque".into(), toml::Value::Float(m.max_torque as f64));
            props.insert("enabled".into(), toml::Value::Boolean(m.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
