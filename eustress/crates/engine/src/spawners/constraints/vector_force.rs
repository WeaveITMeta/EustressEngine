//! `VectorForce` mover spawner — continuous force (config only).
//!
//! ## Runtime
//!
//! Places the [`VectorForce`] *configuration* component on a child of the
//! part it drives. The per-frame force actuation lives in
//! [`crate::physics::movers::apply_vector_force_movers`].
//!
//! ## Avian mapping
//!
//! None at spawn time. The runtime system applies the configured `force`
//! each frame via Avian's [`avian3d::prelude::Forces`] — world-space
//! (`apply_force`) or part-local (`apply_local_force`) depending on
//! `relative_to`.
//!
//! [`VectorForce`]: eustress_common::classes::VectorForce

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, VectorForce};
use eustress_common::services::physics::ForceRelativeTo;

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;
use super::{read_force_relative_to, write_force_relative_to};

/// [`ClassSpawner`] for `ClassName::VectorForce`.
#[derive(Default)]
pub struct VectorForceSpawner;

impl ClassSpawner for VectorForceSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::VectorForce
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let force = props.get_vec3("force").unwrap_or(Vec3::ZERO);
        let relative_to = props
            .get_string("relative_to")
            .map(read_force_relative_to)
            .unwrap_or(ForceRelativeTo::World);
        let apply_at_center_of_mass = props.get_bool("apply_at_center_of_mass").unwrap_or(true);
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let mover = VectorForce {
            force,
            relative_to,
            apply_at_center_of_mass,
            enabled,
        };

        let instance = instance_from_bag(ClassName::VectorForce, props);
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
        let _ = world.get::<VectorForce>(entity);
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
        if let Some(b) = rbx
            .property("ApplyAtCenterOfMass")
            .and_then(|v| v.as_bool())
        {
            bag.set("apply_at_center_of_mass", PropertyValue::Bool(b));
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
            if let Some(f) = props.get("force").and_then(|v| v.as_array()) {
                bag.set("force", PropertyValue::Vector3(read_vec3_array(f)));
            }
            if let Some(r) = props.get("relative_to").and_then(|v| v.as_str()) {
                bag.set("relative_to", PropertyValue::String(r.to_string()));
            }
            if let Some(b) = props
                .get("apply_at_center_of_mass")
                .and_then(|v| v.as_bool())
            {
                bag.set("apply_at_center_of_mass", PropertyValue::Bool(b));
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
                toml::Value::String("VectorForce".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(m) = world.get::<VectorForce>(entity) {
            props.insert("force".into(), vec3_to_toml(m.force));
            props.insert(
                "relative_to".into(),
                toml::Value::String(write_force_relative_to(m.relative_to).into()),
            );
            props.insert(
                "apply_at_center_of_mass".into(),
                toml::Value::Boolean(m.apply_at_center_of_mass),
            );
            props.insert("enabled".into(), toml::Value::Boolean(m.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
