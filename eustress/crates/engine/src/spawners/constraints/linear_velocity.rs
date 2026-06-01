//! `LinearVelocity` mover spawner — target linear velocity (config only).
//!
//! ## Runtime
//!
//! Places the [`LinearVelocity`] *configuration* component (the
//! `eustress_common::classes` mover, **not** the Avian rigid-body
//! component of the same name) on a child of the part it drives. The
//! per-frame velocity actuation lives in
//! [`crate::physics::movers::apply_linear_velocity_movers`].
//!
//! ## Avian mapping
//!
//! None at spawn time. The runtime system writes the target's Avian
//! [`LinearVelocity`](avian3d::prelude::LinearVelocity) toward
//! `line_velocity * line_direction` (line mode) or `vector_velocity`
//! (vector mode), bounded by `max_force`.
//!
//! [`LinearVelocity`]: eustress_common::classes::LinearVelocity

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
// The Eustress *config* mover — distinct from `avian3d::prelude::LinearVelocity`.
use eustress_common::classes::{ClassName, LinearVelocity, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::LinearVelocity`.
#[derive(Default)]
pub struct LinearVelocitySpawner;

impl ClassSpawner for LinearVelocitySpawner {
    fn class_name(&self) -> ClassName {
        ClassName::LinearVelocity
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let line_velocity = props.get_f32("line_velocity").unwrap_or(0.0);
        let line_direction = props.get_vec3("line_direction").unwrap_or(Vec3::Z);
        let vector_velocity = props.get_vec3("vector_velocity").unwrap_or(Vec3::ZERO);
        let max_force = props.get_f32("max_force").unwrap_or(0.0);
        let enabled = props.get_bool("enabled").unwrap_or(true);
        let attachment0 = super::weld::read_optional_part_ref(props, "attachment0");
        let plane_velocity = props
            .get_vec2("plane_velocity")
            .map(Vec2::from)
            .unwrap_or(Vec2::ZERO);
        let velocity_constraint_mode = props
            .get_string("velocity_constraint_mode")
            .unwrap_or("Vector")
            .to_string();
        let relative_to = props.get_string("relative_to").unwrap_or("World").to_string();

        let mover = LinearVelocity {
            attachment0,
            line_direction,
            line_velocity,
            plane_velocity,
            vector_velocity,
            max_force,
            velocity_constraint_mode,
            relative_to,
            enabled,
        };

        let instance = instance_from_bag(ClassName::LinearVelocity, props);
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
        let _ = world.get::<LinearVelocity>(entity);
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
        if let Some(v) = rbx.property("LineVelocity").and_then(|v| v.as_f32()) {
            bag.set("line_velocity", PropertyValue::Float(v));
        }
        if let Some(f) = rbx.property("MaxForce").and_then(|v| v.as_f32()) {
            bag.set("max_force", PropertyValue::Float(f));
        }
        if let Some(e) = rbx.property("Enabled").and_then(|v| v.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(e));
        }
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
            if let Some(v) = props.get("line_velocity").and_then(|v| v.as_float()) {
                bag.set("line_velocity", PropertyValue::Float(v as f32));
            }
            if let Some(d) = props.get("line_direction").and_then(|v| v.as_array()) {
                bag.set("line_direction", PropertyValue::Vector3(read_vec3_array(d)));
            }
            if let Some(v) = props.get("vector_velocity").and_then(|v| v.as_array()) {
                bag.set("vector_velocity", PropertyValue::Vector3(read_vec3_array(v)));
            }
            if let Some(f) = props.get("max_force").and_then(|v| v.as_float()) {
                bag.set("max_force", PropertyValue::Float(f as f32));
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
                toml::Value::String("LinearVelocity".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<LinearVelocity>(entity) {
            props.insert(
                "line_velocity".into(),
                toml::Value::Float(m.line_velocity as f64),
            );
            props.insert("line_direction".into(), vec3_to_toml(m.line_direction));
            props.insert("vector_velocity".into(), vec3_to_toml(m.vector_velocity));
            props.insert("max_force".into(), toml::Value::Float(m.max_force as f64));
            props.insert("enabled".into(), toml::Value::Boolean(m.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
