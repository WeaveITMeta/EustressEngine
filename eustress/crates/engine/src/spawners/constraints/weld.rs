//! `WeldConstraint` spawner — rigid two-body weld.
//!
//! ## Avian mapping
//!
//! [`WeldConstraint`] → [`avian3d::prelude::FixedJoint`]. A fixed joint
//! locks all 6 degrees of freedom between two bodies — the canonical
//! Roblox WeldConstraint behavior.
//!
//! Per spec §13: when a heavy contact-graph is the cost driver, the spec
//! suggests collapsing welded bodies into a single rigid body. That
//! optimization belongs to a Wave 4 physics-LOD pass; this spawner
//! attaches the joint unconditionally.
//!
//! ## Entity ref resolution
//!
//! `Part0` / `Part1` are `Option<u32>` Eustress instance IDs. The
//! `ClassSpawner::spawn` API exposes only `commands`, not a `Query`, so
//! we cannot map IDs to entities at spawn time. The joint is attached
//! with [`Entity::PLACEHOLDER`] for whichever side is unresolved — a
//! downstream resolver system reads `WeldConstraint::part0`/`part1` and
//! patches the [`FixedJoint::body1`]/`body2` fields.
//!
//! ## `enabled = false`
//!
//! Inserts [`avian3d::prelude::JointDisabled`] when the Eustress
//! `enabled` flag is `false`. The Eustress component still records the
//! user's intent so re-enabling is one component edit away.
//!
//! [`WeldConstraint`]: eustress_common::classes::WeldConstraint

use bevy::prelude::*;

use avian3d::prelude::{FixedJoint, JointDisabled};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, WeldConstraint};

use super::attachment::{read_vec3_array, vec3_to_toml};
use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::WeldConstraint`.
#[derive(Default)]
pub struct WeldConstraintSpawner;

impl ClassSpawner for WeldConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::WeldConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let part0 = read_optional_part_ref(props, "part0");
        let part1 = read_optional_part_ref(props, "part1");
        let c0 = read_transform(props, "c0");
        let c1 = read_transform(props, "c1");
        let enabled = props.get_bool("enabled").unwrap_or(true);

        let weld = WeldConstraint {
            part0,
            part1,
            c0,
            c1,
            enabled,
        };

        let instance = instance_from_bag(ClassName::WeldConstraint, props);
        let name = instance.name.clone();

        // Avian joint with placeholder bodies. A downstream resolver
        // walks `Query<(Entity, &WeldConstraint, &mut FixedJoint)>` and
        // patches body1/body2 once Part0/Part1 are mapped to entities.
        let joint = FixedJoint::new(Entity::PLACEHOLDER, Entity::PLACEHOLDER)
            .with_local_anchor1(c0.translation)
            .with_local_anchor2(c1.translation);

        let mut ec = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            weld,
            joint,
            Name::new(name),
        ));
        if !enabled {
            ec.insert(JointDisabled);
        }
        ec.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        // See `super::attachment::AttachmentSpawner::serialize` — Wave 2
        // defers to the legacy Fjall write path.
        let _ = world.get::<WeldConstraint>(entity);
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, _world: &mut World, _entity: Entity, _props: &PropertyBag) -> bool {
        // Joint topology changes (Part0/Part1 swap, anchor change) require
        // a fresh Avian joint entity — `true` triggers the respawn dance
        // in the caller.
        true
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::new();
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(p) = rbx.property("Enabled").and_then(|v| v.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(p));
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
            if let Some(p) = props.get("part0").and_then(|v| v.as_integer()) {
                bag.set("part0", PropertyValue::Int(p as i32));
            }
            if let Some(p) = props.get("part1").and_then(|v| v.as_integer()) {
                bag.set("part1", PropertyValue::Int(p as i32));
            }
            if let Some(c0) = props.get("c0").and_then(|v| v.as_array()) {
                bag.set("c0", PropertyValue::Vector3(read_vec3_array(c0)));
            }
            if let Some(c1) = props.get("c1").and_then(|v| v.as_array()) {
                bag.set("c1", PropertyValue::Vector3(read_vec3_array(c1)));
            }
            if let Some(enabled) = props.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("enabled", PropertyValue::Bool(enabled));
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
                toml::Value::String("WeldConstraint".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(weld) = world.get::<WeldConstraint>(entity) {
            if let Some(p) = weld.part0 {
                props.insert("part0".into(), toml::Value::Integer(p as i64));
            }
            if let Some(p) = weld.part1 {
                props.insert("part1".into(), toml::Value::Integer(p as i64));
            }
            props.insert("c0".into(), vec3_to_toml(weld.c0.translation));
            props.insert("c1".into(), vec3_to_toml(weld.c1.translation));
            props.insert("enabled".into(), toml::Value::Boolean(weld.enabled));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}

// ── Shared helpers used by every joint spawner in this group ──────────

/// Read a `part0`/`part1`-style optional entity-id reference from the
/// bag. Bag stores it as `PropertyValue::Int`; `None` means "unresolved
/// at spawn time" (the joint will be patched later).
pub(super) fn read_optional_part_ref(bag: &PropertyBag, key: &str) -> Option<u32> {
    bag.get_i32(key).and_then(|i| {
        if i < 0 {
            None
        } else {
            Some(i as u32)
        }
    })
}

/// Read a `c0`/`c1` Transform from the bag. The bag stores these as
/// `Vector3` (just the translation) for parity with the on-disk TOML
/// schema; rotation is identity. When Wave 4 extends the schema to
/// include rotation, this reads `PropertyValue::Transform` directly.
pub(super) fn read_transform(bag: &PropertyBag, key: &str) -> Transform {
    if let Some(t) = bag.get_transform(key) {
        return *t;
    }
    if let Some(v) = bag.get_vec3(key) {
        return Transform::from_translation(v);
    }
    Transform::IDENTITY
}
