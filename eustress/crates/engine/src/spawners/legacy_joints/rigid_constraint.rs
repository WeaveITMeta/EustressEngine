//! `RigidConstraintSpawner` — `ClassSpawner` for
//! [`ClassName::RigidConstraint`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.A). Config-attach with
//! the Avian `FixedJoint` wiring deferred (see the group [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, RigidConstraint};
use eustress_common::{Attributes, Tags};

use super::{
    apply_metadata_edit, export_metadata, import_metadata, insert_optional_ref, instance_from_bag,
    read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::RigidConstraint`].
#[derive(Default)]
pub struct RigidConstraintSpawner;

impl ClassSpawner for RigidConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::RigidConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::RigidConstraint, props);
        let name = instance.name.clone();
        let d = RigidConstraint::default();
        let comp = RigidConstraint {
            attachment0: read_optional_ref(props, "attachment0"),
            attachment1: read_optional_ref(props, "attachment1"),
            enabled: props.get_bool("enabled").unwrap_or(d.enabled),
        };
        // TODO(avian): wire a `FixedJoint` between the two attachments once a
        // ref→Entity resolver exists on this branch.

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                comp,
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id();
        if let Some(parent) = ctx.parent_entity {
            ctx.commands.entity(entity).insert(ChildOf(parent));
        }
        entity
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        apply_metadata_edit(world, entity, props);
        if let Some(mut comp) = world.get_mut::<RigidConstraint>(entity) {
            if props.get("attachment0").is_some() { comp.attachment0 = read_optional_ref(props, "attachment0"); }
            if props.get("attachment1").is_some() { comp.attachment1 = read_optional_ref(props, "attachment1"); }
            if let Some(v) = props.get_bool("enabled") { comp.enabled = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [("Attachment0", "attachment0"), ("Attachment1", "attachment1")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_i32()) {
                bag.set(key, PropertyValue::Int(v));
            }
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["attachment0", "attachment1"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
            if let Some(v) = props.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("enabled", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "RigidConstraint")),
        );
        if let Some(comp) = world.get::<RigidConstraint>(entity) {
            let mut props = toml::value::Table::new();
            insert_optional_ref(&mut props, "attachment0", comp.attachment0);
            insert_optional_ref(&mut props, "attachment1", comp.attachment1);
            props.insert("enabled".into(), toml::Value::Boolean(comp.enabled));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_rigid_constraint() {
        assert_eq!(RigidConstraintSpawner.class_name(), ClassName::RigidConstraint);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(RigidConstraintSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "RigidConstraint"
            name = "RC"
            [properties]
            attachment0 = 2
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = RigidConstraintSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("attachment0"), Some(2));
        assert_eq!(bag.get_bool("enabled"), Some(false));
    }
}
