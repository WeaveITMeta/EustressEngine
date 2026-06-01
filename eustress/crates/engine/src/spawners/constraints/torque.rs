//! `Torque` mover spawner — continuous torque (config only).
//!
//! ## Runtime
//!
//! Places the [`Torque`] *configuration* component on a child of the part
//! it drives. The per-frame torque actuation lives in
//! [`crate::physics::movers::apply_torque_movers`].
//!
//! ## Avian mapping
//!
//! None at spawn time. The runtime system applies the configured `torque`
//! each frame via Avian's [`avian3d::prelude::Forces::apply_torque`],
//! rotating into world space first when `relative_to == Part`.
//!
//! [`Torque`]: eustress_common::classes::Torque

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, Torque};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::Torque`.
#[derive(Default)]
pub struct TorqueSpawner;

impl ClassSpawner for TorqueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Torque
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let torque = props.get_vec3("torque").unwrap_or(Vec3::ZERO);
        let relative_to = props
            .get_string("relative_to")
            .unwrap_or("Attachment0")
            .to_string();
        let enabled = props.get_bool("enabled").unwrap_or(true);
        let attachment0 = super::weld::read_optional_part_ref(props, "attachment0");

        let mover = Torque {
            attachment0,
            torque,
            relative_to,
            enabled,
        };

        let instance = instance_from_bag(ClassName::Torque, props);
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
        let _ = world.get::<Torque>(entity);
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
        if let Some(e) = rbx.property("Enabled").and_then(|v| v.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(e));
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
            if let Some(t) = props.get("torque").and_then(|v| v.as_array()) {
                bag.set("torque", PropertyValue::Vector3(read_vec3_array(t)));
            }
            if let Some(r) = props.get("relative_to").and_then(|v| v.as_str()) {
                bag.set("relative_to", PropertyValue::String(r.to_string()));
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
            meta.insert("class_name".into(), toml::Value::String("Torque".into()));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<Torque>(entity) {
            props.insert("torque".into(), vec3_to_toml(m.torque));
            props.insert(
                "relative_to".into(),
                toml::Value::String(m.relative_to.clone()),
            );
            props.insert("enabled".into(), toml::Value::Boolean(m.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
