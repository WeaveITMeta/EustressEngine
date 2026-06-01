//! `NoCollisionConstraintSpawner` — `ClassSpawner` for
//! [`ClassName::NoCollisionConstraint`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.A). Config-attach with
//! the collision-filter wiring deferred (see the group [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, NoCollisionConstraint, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{
    apply_metadata_edit, export_metadata, import_metadata, insert_optional_ref, instance_from_bag,
    read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::NoCollisionConstraint`].
#[derive(Default)]
pub struct NoCollisionConstraintSpawner;

impl ClassSpawner for NoCollisionConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::NoCollisionConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::NoCollisionConstraint, props);
        let name = instance.name.clone();
        let d = NoCollisionConstraint::default();
        let comp = NoCollisionConstraint {
            part0: read_optional_ref(props, "part0"),
            part1: read_optional_ref(props, "part1"),
            enabled: props.get_bool("enabled").unwrap_or(d.enabled),
        };
        // TODO(avian): register a collision-layer filter between Part0/Part1
        // once a ref→Entity resolver exists on this branch.

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
        if let Some(mut comp) = world.get_mut::<NoCollisionConstraint>(entity) {
            if props.get("part0").is_some() { comp.part0 = read_optional_ref(props, "part0"); }
            if props.get("part1").is_some() { comp.part1 = read_optional_ref(props, "part1"); }
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
        for (rbx_key, key) in [("Part0", "part0"), ("Part1", "part1")] {
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
            for key in ["part0", "part1"] {
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
            toml::Value::Table(export_metadata(world, entity, "NoCollisionConstraint")),
        );
        if let Some(comp) = world.get::<NoCollisionConstraint>(entity) {
            let mut props = toml::value::Table::new();
            insert_optional_ref(&mut props, "part0", comp.part0);
            insert_optional_ref(&mut props, "part1", comp.part1);
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
    fn class_name_is_no_collision_constraint() {
        assert_eq!(NoCollisionConstraintSpawner.class_name(), ClassName::NoCollisionConstraint);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(NoCollisionConstraintSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "NoCollisionConstraint"
            name = "NCC"
            [properties]
            part0 = 1
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = NoCollisionConstraintSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("part0"), Some(1));
        assert_eq!(bag.get_bool("enabled"), Some(false));
    }
}
