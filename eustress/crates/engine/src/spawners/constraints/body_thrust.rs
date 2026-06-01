//! `BodyThrust` (legacy mover) spawner — config only.
//!
//! ## Runtime
//!
//! Places the legacy [`BodyThrust`] *configuration* component on a child
//! of the part it drives. The per-frame actuation lives in
//! [`crate::physics::movers::apply_body_thrust_movers`], which maps it
//! onto a *local-space* [`VectorForce`](eustress_common::classes::VectorForce)
//! (Roblox applies `BodyThrust.Force` in the part's local frame).
//!
//! `location` is the offset (in the part's local frame) the thrust is
//! applied at. Off-center application (which induces torque) is follow-up
//! work — the runtime currently applies the thrust at the center of mass.
//!
//! [`BodyThrust`]: eustress_common::classes::BodyThrust

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BodyThrust, ClassName, PropertyValue};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::BodyThrust`.
#[derive(Default)]
pub struct BodyThrustSpawner;

impl ClassSpawner for BodyThrustSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BodyThrust
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let force = props.get_vec3("force").unwrap_or(Vec3::ZERO);
        let location = props.get_vec3("location").unwrap_or(Vec3::ZERO);
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let mover = BodyThrust {
            force,
            location,
            enabled,
        };

        let instance = instance_from_bag(ClassName::BodyThrust, props);
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
        let _ = world.get::<BodyThrust>(entity);
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
            if let Some(f) = props.get("force").and_then(|v| v.as_array()) {
                bag.set("force", PropertyValue::Vector3(read_vec3_array(f)));
            }
            if let Some(l) = props.get("location").and_then(|v| v.as_array()) {
                bag.set("location", PropertyValue::Vector3(read_vec3_array(l)));
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
                toml::Value::String("BodyThrust".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<BodyThrust>(entity) {
            props.insert("force".into(), vec3_to_toml(m.force));
            props.insert("location".into(), vec3_to_toml(m.location));
            props.insert("enabled".into(), toml::Value::Boolean(m.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
