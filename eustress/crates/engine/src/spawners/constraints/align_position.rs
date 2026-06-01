//! `AlignPosition` mover spawner — PD position controller (config only).
//!
//! ## Runtime
//!
//! This spawner only places the [`AlignPosition`] *configuration*
//! component on a child entity of the part it drives. The per-frame PD
//! actuation lives in
//! [`crate::physics::movers::apply_align_position_movers`].
//!
//! ## Avian mapping
//!
//! None at spawn time — `AlignPosition` is a data-driven mover, not a
//! solver joint. The runtime system reads `position` / `max_force` /
//! `max_velocity` / `responsiveness` / `rigidity_enabled` and applies a
//! clamped PD force to the target body via Avian's
//! [`avian3d::prelude::Forces`].
//!
//! [`AlignPosition`]: eustress_common::classes::AlignPosition

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AlignPosition, ClassName, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::rod::read_meta;
use super::weld::read_optional_part_ref;

/// [`ClassSpawner`] for `ClassName::AlignPosition`.
#[derive(Default)]
pub struct AlignPositionSpawner;

impl ClassSpawner for AlignPositionSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AlignPosition
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let attachment0 = read_optional_part_ref(props, "attachment0");
        let attachment1 = read_optional_part_ref(props, "attachment1");
        let position = props.get_vec3("position").unwrap_or(Vec3::ZERO);
        let max_force = props.get_f32("max_force").unwrap_or(100000.0);
        let max_velocity = props.get_f32("max_velocity").unwrap_or(1000.0);
        let responsiveness = props.get_f32("responsiveness").unwrap_or(35.0);
        let rigidity_enabled = props.get_bool("rigidity_enabled").unwrap_or(false);
        let apply_at_center_of_mass = props.get_bool("apply_at_center_of_mass").unwrap_or(false);
        let mode = props
            .get_string("mode")
            .unwrap_or("OneAttachment")
            .to_string();

        let align = AlignPosition {
            attachment0,
            attachment1,
            position,
            max_force,
            max_velocity,
            responsiveness,
            rigidity_enabled,
            apply_at_center_of_mass,
            mode,
        };

        let instance = instance_from_bag(ClassName::AlignPosition, props);
        let name = instance.name.clone();

        ctx.commands
            .spawn((
                Transform::from_translation(position),
                Visibility::default(),
                instance,
                align,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<AlignPosition>(entity);
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
        if let Some(f) = rbx.property("MaxForce").and_then(|v| v.as_f32()) {
            bag.set("max_force", PropertyValue::Float(f));
        }
        if let Some(v) = rbx.property("MaxVelocity").and_then(|v| v.as_f32()) {
            bag.set("max_velocity", PropertyValue::Float(v));
        }
        if let Some(r) = rbx.property("Responsiveness").and_then(|v| v.as_f32()) {
            bag.set("responsiveness", PropertyValue::Float(r));
        }
        if let Some(b) = rbx.property("RigidityEnabled").and_then(|v| v.as_bool()) {
            bag.set("rigidity_enabled", PropertyValue::Bool(b));
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
            if let Some(pos) = props.get("position").and_then(|v| v.as_array()) {
                bag.set("position", PropertyValue::Vector3(read_vec3_array(pos)));
            }
            if let Some(f) = props.get("max_force").and_then(|v| v.as_float()) {
                bag.set("max_force", PropertyValue::Float(f as f32));
            }
            if let Some(v) = props.get("max_velocity").and_then(|v| v.as_float()) {
                bag.set("max_velocity", PropertyValue::Float(v as f32));
            }
            if let Some(r) = props.get("responsiveness").and_then(|v| v.as_float()) {
                bag.set("responsiveness", PropertyValue::Float(r as f32));
            }
            if let Some(b) = props.get("rigidity_enabled").and_then(|v| v.as_bool()) {
                bag.set("rigidity_enabled", PropertyValue::Bool(b));
            }
            if let Some(b) = props
                .get("apply_at_center_of_mass")
                .and_then(|v| v.as_bool())
            {
                bag.set("apply_at_center_of_mass", PropertyValue::Bool(b));
            }
            if let Some(m) = props.get("mode").and_then(|v| v.as_str()) {
                bag.set("mode", PropertyValue::String(m.to_string()));
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
                toml::Value::String("AlignPosition".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(a) = world.get::<AlignPosition>(entity) {
            if let Some(p) = a.attachment0 {
                props.insert("attachment0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = a.attachment1 {
                props.insert("attachment1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("position".into(), vec3_to_toml(a.position));
            props.insert("max_force".into(), toml::Value::Float(a.max_force as f64));
            props.insert(
                "max_velocity".into(),
                toml::Value::Float(a.max_velocity as f64),
            );
            props.insert(
                "responsiveness".into(),
                toml::Value::Float(a.responsiveness as f64),
            );
            props.insert(
                "rigidity_enabled".into(),
                toml::Value::Boolean(a.rigidity_enabled),
            );
            props.insert(
                "apply_at_center_of_mass".into(),
                toml::Value::Boolean(a.apply_at_center_of_mass),
            );
            props.insert("mode".into(), toml::Value::String(a.mode.clone()));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
