//! `BodyVelocity` (legacy mover) spawner — config only.
//!
//! ## Runtime
//!
//! Places the legacy [`BodyVelocity`] *configuration* component on a
//! child of the part it drives. The per-frame actuation lives in
//! [`crate::physics::movers::apply_body_velocity_movers`], which maps it
//! onto the same maths as the modern
//! [`LinearVelocity`](eustress_common::classes::LinearVelocity) mover.
//!
//! `BodyVelocity` predates the `enabled` flag — it has no toggle field,
//! so the runtime system always actuates it while present.
//!
//! [`BodyVelocity`]: eustress_common::services::physics::BodyVelocity

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue};
use eustress_common::services::physics::BodyVelocity;

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::BodyVelocity`.
#[derive(Default)]
pub struct BodyVelocitySpawner;

impl ClassSpawner for BodyVelocitySpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BodyVelocity
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let velocity = props.get_vec3("velocity").unwrap_or(Vec3::ZERO);
        let max_force = props
            .get_vec3("max_force")
            .unwrap_or(Vec3::splat(f32::MAX));
        let power = props.get_f32("power").unwrap_or(1.0);

        let mover = BodyVelocity {
            velocity,
            max_force,
            power,
        };

        let instance = instance_from_bag(ClassName::BodyVelocity, props);
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
        let _ = world.get::<BodyVelocity>(entity);
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
        if let Some(p) = rbx.property("Power").and_then(|v| v.as_f32()) {
            bag.set("power", PropertyValue::Float(p));
        }
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
            if let Some(v) = props.get("velocity").and_then(|v| v.as_array()) {
                bag.set("velocity", PropertyValue::Vector3(read_vec3_array(v)));
            }
            if let Some(f) = props.get("max_force").and_then(|v| v.as_array()) {
                bag.set("max_force", PropertyValue::Vector3(read_vec3_array(f)));
            }
            if let Some(p) = props.get("power").and_then(|v| v.as_float()) {
                bag.set("power", PropertyValue::Float(p as f32));
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
                toml::Value::String("BodyVelocity".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<BodyVelocity>(entity) {
            props.insert("velocity".into(), vec3_to_toml(m.velocity));
            props.insert("max_force".into(), vec3_to_toml(m.max_force));
            props.insert("power".into(), toml::Value::Float(m.power as f64));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
