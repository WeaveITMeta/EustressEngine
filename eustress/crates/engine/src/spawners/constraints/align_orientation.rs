//! `AlignOrientation` mover spawner — PD orientation controller (config).
//!
//! ## Runtime
//!
//! Places the [`AlignOrientation`] *configuration* component on a child
//! of the part it drives. The per-frame PD torque actuation lives in
//! [`crate::physics::movers::apply_align_orientation_movers`].
//!
//! ## Avian mapping
//!
//! None at spawn time. The runtime system reads the target `cframe`
//! orientation / `max_torque` / `responsiveness` / `rigidity_enabled` and
//! applies a clamped PD torque to the target body via
//! [`avian3d::prelude::Forces`].
//!
//! [`AlignOrientation`]: eustress_common::classes::AlignOrientation

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AlignOrientation, ClassName, PropertyValue};

use super::instance_from_bag;
use super::rod::read_meta;
use super::weld::read_optional_part_ref;

/// [`ClassSpawner`] for `ClassName::AlignOrientation`.
#[derive(Default)]
pub struct AlignOrientationSpawner;

impl ClassSpawner for AlignOrientationSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AlignOrientation
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let attachment0 = read_optional_part_ref(props, "attachment0");
        let attachment1 = read_optional_part_ref(props, "attachment1");
        let cframe = props
            .get_transform("cframe")
            .copied()
            .unwrap_or(Transform::IDENTITY);
        let max_torque = props.get_f32("max_torque").unwrap_or(100000.0);
        let max_angular_velocity = props.get_f32("max_angular_velocity").unwrap_or(1000.0);
        let responsiveness = props.get_f32("responsiveness").unwrap_or(35.0);
        let rigidity_enabled = props.get_bool("rigidity_enabled").unwrap_or(false);
        let mode = props
            .get_string("mode")
            .unwrap_or("OneAttachment")
            .to_string();
        let primary_axis_only = props.get_bool("primary_axis_only").unwrap_or(false);

        let align = AlignOrientation {
            attachment0,
            attachment1,
            cframe,
            max_torque,
            max_angular_velocity,
            responsiveness,
            rigidity_enabled,
            mode,
            primary_axis_only,
        };

        let instance = instance_from_bag(ClassName::AlignOrientation, props);
        let name = instance.name.clone();

        ctx.commands
            .spawn((
                cframe,
                Visibility::default(),
                instance,
                align,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<AlignOrientation>(entity);
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
        if let Some(v) = rbx.property("MaxAngularVelocity").and_then(|v| v.as_f32()) {
            bag.set("max_angular_velocity", PropertyValue::Float(v));
        }
        if let Some(r) = rbx.property("Responsiveness").and_then(|v| v.as_f32()) {
            bag.set("responsiveness", PropertyValue::Float(r));
        }
        if let Some(b) = rbx.property("RigidityEnabled").and_then(|v| v.as_bool()) {
            bag.set("rigidity_enabled", PropertyValue::Bool(b));
        }
        if let Some(b) = rbx.property("PrimaryAxisOnly").and_then(|v| v.as_bool()) {
            bag.set("primary_axis_only", PropertyValue::Bool(b));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(12);
        read_meta(toml_value, &mut bag);

        if let Some(props) = toml_value.get("properties") {
            if let Some(p) = props.get("attachment0").and_then(|v| v.as_integer()) {
                bag.set("attachment0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("attachment1").and_then(|v| v.as_integer()) {
                bag.set("attachment1", PropertyValue::Int(p as i32));
            }
            if let Some(t) = props.get("max_torque").and_then(|v| v.as_float()) {
                bag.set("max_torque", PropertyValue::Float(t as f32));
            }
            if let Some(v) = props
                .get("max_angular_velocity")
                .and_then(|v| v.as_float())
            {
                bag.set("max_angular_velocity", PropertyValue::Float(v as f32));
            }
            if let Some(r) = props.get("responsiveness").and_then(|v| v.as_float()) {
                bag.set("responsiveness", PropertyValue::Float(r as f32));
            }
            if let Some(b) = props.get("rigidity_enabled").and_then(|v| v.as_bool()) {
                bag.set("rigidity_enabled", PropertyValue::Bool(b));
            }
            if let Some(m) = props.get("mode").and_then(|v| v.as_str()) {
                bag.set("mode", PropertyValue::String(m.to_string()));
            }
            if let Some(b) = props.get("primary_axis_only").and_then(|v| v.as_bool()) {
                bag.set("primary_axis_only", PropertyValue::Bool(b));
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
                toml::Value::String("AlignOrientation".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(a) = world.get::<AlignOrientation>(entity) {
            if let Some(p) = a.attachment0 {
                props.insert("attachment0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = a.attachment1 {
                props.insert("attachment1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("max_torque".into(), toml::Value::Float(a.max_torque as f64));
            props.insert(
                "max_angular_velocity".into(),
                toml::Value::Float(a.max_angular_velocity as f64),
            );
            props.insert(
                "responsiveness".into(),
                toml::Value::Float(a.responsiveness as f64),
            );
            props.insert(
                "rigidity_enabled".into(),
                toml::Value::Boolean(a.rigidity_enabled),
            );
            props.insert("mode".into(), toml::Value::String(a.mode.clone()));
            props.insert(
                "primary_axis_only".into(),
                toml::Value::Boolean(a.primary_axis_only),
            );
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
