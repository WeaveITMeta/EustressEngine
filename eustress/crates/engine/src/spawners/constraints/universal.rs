//! `UniversalConstraint` spawner — two-axis (U-joint) rotation.
//!
//! ## Avian mapping (APPROXIMATION)
//!
//! Roblox's `UniversalConstraint` is a universal/Cardan joint: the two
//! attached parts share a pivot and may rotate about two perpendicular
//! axes (the classic drive-shaft U-joint), bounded by `max_angle`.
//!
//! **Avian 0.6 has no native universal joint.** The closest single-joint
//! analogue is [`avian3d::prelude::SphericalJoint`] (3-DOF rotation about
//! a shared pivot) with its swing limited to the `max_angle` half-cone —
//! exactly the mapping [`super::ball_socket`] uses. The difference from a
//! true U-joint is that the spherical approximation also permits *twist*
//! about the line between the anchors, which a real universal joint
//! constrains. We leave Avian's `twist_limit` unset (matching ball &
//! socket); a true 2-axis universal would additionally pin twist to zero.
//!
//! `max_angle` is stored in **degrees** (Roblox convention) and converted
//! to radians for Avian's `AngleLimit`. `restitution` (bounciness) is
//! recorded on the component but has no direct Avian swing-limit field.
//!
//! ## Attachment ref resolution
//!
//! See [`super::rod`] — placeholder bodies; downstream resolver patches
//! `body1`/`body2` from `attachment0`/`attachment1`.
//!
//! [`UniversalConstraint`]: eustress_common::classes::UniversalConstraint

use bevy::prelude::*;

use avian3d::prelude::{AngleLimit, SphericalJoint};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UniversalConstraint};

use super::instance_from_bag;
use super::rod::read_meta;
use super::weld::read_optional_part_ref;

/// [`ClassSpawner`] for `ClassName::UniversalConstraint`.
#[derive(Default)]
pub struct UniversalConstraintSpawner;

impl ClassSpawner for UniversalConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UniversalConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let attachment0 = read_optional_part_ref(props, "attachment0");
        let attachment1 = read_optional_part_ref(props, "attachment1");
        let max_angle = props.get_f32("max_angle").unwrap_or(45.0);
        let restitution = props.get_f32("restitution").unwrap_or(0.0);

        let universal = UniversalConstraint {
            attachment0,
            attachment1,
            max_angle,
            restitution,
        };

        let instance = instance_from_bag(ClassName::UniversalConstraint, props);
        let name = instance.name.clone();

        let mut joint = SphericalJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER);
        // Degrees → radians; the U-joint half-cone maps to Avian's signed
        // swing range.
        let half = max_angle.to_radians();
        joint.swing_limit = Some(AngleLimit::new(-half, half));

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                universal,
                joint,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let _ = world.get::<UniversalConstraint>(entity);
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
        if let Some(a) = rbx.property("MaxAngle").and_then(|v| v.as_f32()) {
            bag.set("max_angle", PropertyValue::Float(a));
        }
        if let Some(r) = rbx.property("Restitution").and_then(|v| v.as_f32()) {
            bag.set("restitution", PropertyValue::Float(r));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        read_meta(toml_value, &mut bag);

        if let Some(props) = toml_value.get("properties") {
            if let Some(p) = props.get("attachment0").and_then(|v| v.as_integer()) {
                bag.set("attachment0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("attachment1").and_then(|v| v.as_integer()) {
                bag.set("attachment1", PropertyValue::Int(p as i32));
            }
            if let Some(a) = props.get("max_angle").and_then(|v| v.as_float()) {
                bag.set("max_angle", PropertyValue::Float(a as f32));
            }
            if let Some(r) = props.get("restitution").and_then(|v| v.as_float()) {
                bag.set("restitution", PropertyValue::Float(r as f32));
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
                toml::Value::String("UniversalConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(u) = world.get::<UniversalConstraint>(entity) {
            if let Some(p) = u.attachment0 {
                props.insert("attachment0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = u.attachment1 {
                props.insert("attachment1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("max_angle".into(), toml::Value::Float(u.max_angle as f64));
            props.insert("restitution".into(), toml::Value::Float(u.restitution as f64));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}
