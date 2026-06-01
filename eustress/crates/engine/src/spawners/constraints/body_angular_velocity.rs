//! `BodyAngularVelocity` (legacy mover) spawner — config only.
//!
//! ## Runtime
//!
//! Places the legacy [`BodyAngularVelocity`] *configuration* component on
//! a child of the part it drives. The per-frame actuation lives in
//! [`crate::physics::movers::apply_body_angular_velocity_movers`], which
//! maps it onto the same maths as the modern
//! [`AngularVelocity`](eustress_common::classes::AngularVelocity) mover.
//!
//! [`BodyAngularVelocity`]: eustress_common::classes::BodyAngularVelocity

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BodyAngularVelocity, ClassName, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::BodyAngularVelocity`.
#[derive(Default)]
pub struct BodyAngularVelocitySpawner;

impl ClassSpawner for BodyAngularVelocitySpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BodyAngularVelocity
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let angular_velocity = props.get_vec3("angular_velocity").unwrap_or(Vec3::ZERO);
        let max_torque = props
            .get_vec3("max_torque")
            .unwrap_or(Vec3::splat(f32::MAX));
        let enabled = props.get_bool("enabled").unwrap_or(true);
        let p = props.get_f32("p").unwrap_or(3000.0);

        let mover = BodyAngularVelocity {
            angular_velocity,
            max_torque,
            p,
            enabled,
        };

        let instance = instance_from_bag(ClassName::BodyAngularVelocity, props);
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
        let _ = world.get::<BodyAngularVelocity>(entity);
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
        let mut bag = PropertyBag::with_capacity(6);

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
            if let Some(t) = props.get("max_torque").and_then(|v| v.as_array()) {
                bag.set("max_torque", PropertyValue::Vector3(read_vec3_array(t)));
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
                toml::Value::String("BodyAngularVelocity".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<BodyAngularVelocity>(entity) {
            props.insert("angular_velocity".into(), vec3_to_toml(m.angular_velocity));
            props.insert("max_torque".into(), vec3_to_toml(m.max_torque));
            props.insert("enabled".into(), toml::Value::Boolean(m.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
