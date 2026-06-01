//! `PlaneConstraint` spawner — keep a part on a plane.
//!
//! ## Avian mapping (APPROXIMATION — no native joint)
//!
//! Roblox's `PlaneConstraint` (`Plane`) keeps `attachment1` coplanar with
//! `attachment0`'s plane: two translational degrees of freedom within the
//! plane plus free rotation, with the *normal* translation pinned to zero.
//!
//! **Avian 0.6 has no plane / 2-DOF translational joint.** A
//! [`avian3d::prelude::PrismaticJoint`] constrains motion to a *line*
//! (1 DOF), not a plane, so it is the wrong primitive. Rather than ship a
//! joint with the wrong DOF count (which would visibly over-constrain the
//! part), this spawner attaches **no Avian joint** and records the
//! [`PlaneConstraint`] component as the source of truth. Enforcement of
//! the plane is follow-up runtime work — the same "config component now,
//! solver later" shape the movers in [`crate::physics::movers`] use. The
//! `enabled` flag will gate that future enforcement system.
//!
//! ## Attachment ref resolution
//!
//! See [`super::rod`] — `attachment0`/`attachment1` are Eustress instance
//! IDs of `Attachment` entities, resolved downstream.
//!
//! [`PlaneConstraint`]: eustress_common::classes::PlaneConstraint

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PlaneConstraint, PropertyValue};

use super::instance_from_bag;
use super::rod::read_meta;
use super::weld::read_optional_part_ref;

/// [`ClassSpawner`] for `ClassName::PlaneConstraint`.
#[derive(Default)]
pub struct PlaneConstraintSpawner;

impl ClassSpawner for PlaneConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::PlaneConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let attachment0 = read_optional_part_ref(props, "attachment0");
        let attachment1 = read_optional_part_ref(props, "attachment1");
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let plane = PlaneConstraint {
            attachment0,
            attachment1,
            enabled,
        };

        let instance = instance_from_bag(ClassName::PlaneConstraint, props);
        let name = instance.name.clone();

        // No Avian joint — see module docs. The component is the source of
        // truth; a runtime plane-enforcement system is follow-up work.
        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                plane,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<PlaneConstraint>(entity);
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
        read_meta(toml_value, &mut bag);

        if let Some(props) = toml_value.get("properties") {
            if let Some(p) = props.get("attachment0").and_then(|v| v.as_integer()) {
                bag.set("attachment0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("attachment1").and_then(|v| v.as_integer()) {
                bag.set("attachment1", PropertyValue::Int(p as i32));
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
                toml::Value::String("PlaneConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(p) = world.get::<PlaneConstraint>(entity) {
            if let Some(v) = p.attachment0 {
                props.insert("attachment0".into(), toml::Value::Integer(v as i64));
            }
            if let Some(v) = p.attachment1 {
                props.insert("attachment1".into(), toml::Value::Integer(v as i64));
            }
            props.insert("enabled".into(), toml::Value::Boolean(p.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
