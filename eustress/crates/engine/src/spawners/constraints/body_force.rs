//! `BodyForce` (legacy mover) spawner — config only.
//!
//! ## Runtime
//!
//! Places the legacy [`BodyForce`] *configuration* component on a child
//! of the part it drives. The per-frame actuation lives in
//! [`crate::physics::movers::apply_body_force_movers`], which maps it
//! onto the same maths as the modern
//! [`VectorForce`](eustress_common::classes::VectorForce) mover (world
//! space by default).
//!
//! `BodyForce` predates the `enabled` flag — it has no toggle field.
//!
//! [`BodyForce`]: eustress_common::services::physics::BodyForce

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue};
use eustress_common::services::physics::{BodyForce, ForceRelativeTo};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::{read_force_relative_to, write_force_relative_to};

/// [`ClassSpawner`] for `ClassName::BodyForce`.
#[derive(Default)]
pub struct BodyForceSpawner;

impl ClassSpawner for BodyForceSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BodyForce
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let force = props.get_vec3("force").unwrap_or(Vec3::ZERO);
        let relative_to = props
            .get_string("relative_to")
            .map(read_force_relative_to)
            .unwrap_or(ForceRelativeTo::World);

        let mover = BodyForce { force, relative_to };

        let instance = instance_from_bag(ClassName::BodyForce, props);
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
        let _ = world.get::<BodyForce>(entity);
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
            if let Some(r) = props.get("relative_to").and_then(|v| v.as_str()) {
                bag.set("relative_to", PropertyValue::String(r.to_string()));
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
            meta.insert("class_name".into(), toml::Value::String("BodyForce".into()));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<BodyForce>(entity) {
            props.insert("force".into(), vec3_to_toml(m.force));
            props.insert(
                "relative_to".into(),
                toml::Value::String(write_force_relative_to(m.relative_to).into()),
            );
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
