//! `RodConstraint` spawner — rigid fixed-length link between two attachments.
//!
//! ## Avian mapping
//!
//! [`RodConstraint`] → [`avian3d::prelude::DistanceJoint`] with
//! `min == max == length`. A distance joint whose lower and upper limits
//! are equal behaves as a rigid rod: the two anchor points are held at a
//! fixed separation while the bodies are otherwise free to rotate about
//! the rod ends (the canonical Roblox `RodConstraint` behaviour).
//!
//! `thickness` is a visual-only property (the adornment renderer draws the
//! rod cylinder); `limit_angle0` / `limit_angle1` (degrees) are bend
//! limits at each end — Avian's `DistanceJoint` has no per-end angular
//! limit, so these are recorded on the component for a future
//! exact mapping but are not yet wired into the solver.
//!
//! ## Attachment ref resolution
//!
//! Like the Wave 3.D joints, the joint attaches with
//! [`Entity::PLACEHOLDER`] bodies; a downstream resolver maps
//! `attachment0`/`attachment1` (Eustress instance IDs of `Attachment`
//! entities) to the owning bodies and patches `body1`/`body2` + local
//! anchors. We pass `ZERO` anchors here (the attachment offsets are
//! resolved at patch time).
//!
//! [`RodConstraint`]: eustress_common::classes::RodConstraint

use bevy::prelude::*;

use avian3d::prelude::DistanceJoint;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, RodConstraint};

use super::weld::read_optional_part_ref;
use super::{constraint_refs_from_bag, instance_from_bag, read_references_into_bag};

/// [`ClassSpawner`] for `ClassName::RodConstraint`.
#[derive(Default)]
pub struct RodConstraintSpawner;

impl ClassSpawner for RodConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::RodConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let attachment0 = read_optional_part_ref(props, "attachment0");
        let attachment1 = read_optional_part_ref(props, "attachment1");
        let length = props.get_f32("length").unwrap_or(2.0);
        let thickness = props.get_f32("thickness").unwrap_or(0.15);
        let limit_angle0 = props.get_f32("limit_angle0").unwrap_or(0.0);
        let limit_angle1 = props.get_f32("limit_angle1").unwrap_or(0.0);

        let rod = RodConstraint {
            attachment0,
            attachment1,
            length,
            thickness,
            limit_angle0,
            limit_angle1,
        };

        let instance = instance_from_bag(ClassName::RodConstraint, props);
        let name = instance.name.clone();

        // min == max == length → rigid separation (a fixed-length rod).
        let joint = DistanceJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_limits(length, length);

        // Surface the resolved attachment-reference UUIDs so the joint
        // resolver can walk each attachment → owning body and bind.
        let refs =
            constraint_refs_from_bag(props, "references.Attachment0", "references.Attachment1");

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                rod,
                joint,
                refs,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<RodConstraint>(entity);
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
        bag.set(
            "metadata.name",
            PropertyValue::String(rbx.name().to_string()),
        );
        if let Some(l) = rbx.property("Length").and_then(|v| v.as_f32()) {
            bag.set("length", PropertyValue::Float(l));
        }
        if let Some(t) = rbx.property("Thickness").and_then(|v| v.as_f32()) {
            bag.set("thickness", PropertyValue::Float(t));
        }
        if let Some(a) = rbx.property("LimitAngle0").and_then(|v| v.as_f32()) {
            bag.set("limit_angle0", PropertyValue::Float(a));
        }
        if let Some(a) = rbx.property("LimitAngle1").and_then(|v| v.as_f32()) {
            bag.set("limit_angle1", PropertyValue::Float(a));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(10);
        read_meta(toml_value, &mut bag);
        // Resolved attachment references (`[references] Attachment0/1` =
        // UUID hex) → bag for the joint resolver.
        read_references_into_bag(toml_value, &mut bag, "Attachment0", "Attachment1");

        if let Some(props) = toml_value.get("properties") {
            if let Some(p) = props.get("attachment0").and_then(|v| v.as_integer()) {
                bag.set("attachment0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("attachment1").and_then(|v| v.as_integer()) {
                bag.set("attachment1", PropertyValue::Int(p as i32));
            }
            if let Some(l) = props.get("length").and_then(|v| v.as_float()) {
                bag.set("length", PropertyValue::Float(l as f32));
            }
            if let Some(t) = props.get("thickness").and_then(|v| v.as_float()) {
                bag.set("thickness", PropertyValue::Float(t as f32));
            }
            if let Some(a) = props.get("limit_angle0").and_then(|v| v.as_float()) {
                bag.set("limit_angle0", PropertyValue::Float(a as f32));
            }
            if let Some(a) = props.get("limit_angle1").and_then(|v| v.as_float()) {
                bag.set("limit_angle1", PropertyValue::Float(a as f32));
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
                toml::Value::String("RodConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(r) = world.get::<RodConstraint>(entity) {
            if let Some(p) = r.attachment0 {
                props.insert("attachment0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = r.attachment1 {
                props.insert("attachment1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("length".into(), toml::Value::Float(r.length as f64));
            props.insert("thickness".into(), toml::Value::Float(r.thickness as f64));
            props.insert(
                "limit_angle0".into(),
                toml::Value::Float(r.limit_angle0 as f64),
            );
            props.insert(
                "limit_angle1".into(),
                toml::Value::Float(r.limit_angle1 as f64),
            );
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}

/// Shared metadata reader for the Wave 6.B spawners — pulls
/// `metadata.name` / `metadata.uuid` from a `_instance.toml` body.
pub(super) fn read_meta(toml_value: &toml::Value, bag: &mut PropertyBag) {
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
}
