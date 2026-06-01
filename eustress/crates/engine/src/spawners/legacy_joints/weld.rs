//! `WeldSpawner` — `ClassSpawner` for [`ClassName::Weld`] (legacy rigid weld).
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.A). Config-attach: the
//! spawner attaches the [`Weld`] component + cross-cutting `Instance`/`Name`
//! and persists it. The Avian joint wiring is deferred.
//!
//! `Weld` is the legacy predecessor of the Wave 3 `WeldConstraint`; its
//! `c0`/`c1` are full [`Transform`]s but round-trip translation-only through
//! the bag/TOML (matching the Wave 3 constraint convention).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, Weld};
use eustress_common::{Attributes, Tags};

use super::{
    anchor_to_toml, apply_metadata_edit, export_metadata, import_metadata, insert_optional_ref,
    instance_from_bag, read_anchor_array, read_anchor_transform, read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::Weld`].
#[derive(Default)]
pub struct WeldSpawner;

impl ClassSpawner for WeldSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Weld
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Weld, props);
        let name = instance.name.clone();
        let comp = Weld {
            part0: read_optional_ref(props, "part0"),
            part1: read_optional_ref(props, "part1"),
            c0: read_anchor_transform(props, "c0"),
            c1: read_anchor_transform(props, "c1"),
        };
        // TODO(avian): wire a `FixedJoint` once a Part0/Part1 → Entity resolver
        // system exists on this branch. The config component records intent.

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
        if let Some(mut comp) = world.get_mut::<Weld>(entity) {
            if props.get("part0").is_some() { comp.part0 = read_optional_ref(props, "part0"); }
            if props.get("part1").is_some() { comp.part1 = read_optional_ref(props, "part1"); }
            if props.get("c0").is_some() { comp.c0 = read_anchor_transform(props, "c0"); }
            if props.get("c1").is_some() { comp.c1 = read_anchor_transform(props, "c1"); }
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
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["part0", "part1"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
            for key in ["c0", "c1"] {
                if let Some(arr) = props.get(key).and_then(|v| v.as_array()) {
                    bag.set(key, PropertyValue::Transform(read_anchor_array(arr)));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Weld")),
        );
        if let Some(comp) = world.get::<Weld>(entity) {
            let mut props = toml::value::Table::new();
            insert_optional_ref(&mut props, "part0", comp.part0);
            insert_optional_ref(&mut props, "part1", comp.part1);
            props.insert("c0".into(), anchor_to_toml(comp.c0));
            props.insert("c1".into(), anchor_to_toml(comp.c1));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_weld() {
        assert_eq!(WeldSpawner.class_name(), ClassName::Weld);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(WeldSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_refs() {
        let toml_src = r#"
            [metadata]
            class_name = "Weld"
            name = "W"
            [properties]
            part0 = 5
            part1 = 9
            c0 = [1.0, 0.0, 0.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = WeldSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("part0"), Some(5));
        assert_eq!(bag.get_i32("part1"), Some(9));
        assert!(bag.get_transform("c0").is_some());
    }
}
