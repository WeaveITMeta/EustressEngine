//! `BuoyancySensorSpawner` — `ClassSpawner` for [`ClassName::BuoyancySensor`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.G). Data-attach sensor:
//! reports whether the parent part is `fully_submerged` / `touching_surface`
//! in a fluid volume. See the group [`mod`](super) docs for the shared
//! rationale. These are runtime-read fields persisted across the TOML
//! round-trip; the fluid-query system that drives them is a later phase.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BuoyancySensor, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::BuoyancySensor`].
#[derive(Default)]
pub struct BuoyancySensorSpawner;

impl ClassSpawner for BuoyancySensorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BuoyancySensor
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::BuoyancySensor, props);
        let name = instance.name.clone();
        let d = BuoyancySensor::default();
        let comp = BuoyancySensor {
            fully_submerged: props.get_bool("fully_submerged").unwrap_or(d.fully_submerged),
            touching_surface: props.get_bool("touching_surface").unwrap_or(d.touching_surface),
        };

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
        if let Some(mut comp) = world.get_mut::<BuoyancySensor>(entity) {
            if let Some(v) = props.get_bool("fully_submerged") { comp.fully_submerged = v; }
            if let Some(v) = props.get_bool("touching_surface") { comp.touching_surface = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("FullySubmerged").and_then(|p| p.as_bool()) {
            bag.set("fully_submerged", PropertyValue::Bool(v));
        }
        if let Some(v) = rbx.property("TouchingSurface").and_then(|p| p.as_bool()) {
            bag.set("touching_surface", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("fully_submerged").and_then(|v| v.as_bool()) {
                bag.set("fully_submerged", PropertyValue::Bool(v));
            }
            if let Some(v) = props.get("touching_surface").and_then(|v| v.as_bool()) {
                bag.set("touching_surface", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "BuoyancySensor")),
        );
        if let Some(comp) = world.get::<BuoyancySensor>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("fully_submerged".into(), toml::Value::Boolean(comp.fully_submerged));
            props.insert("touching_surface".into(), toml::Value::Boolean(comp.touching_surface));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_buoyancy_sensor() {
        assert_eq!(BuoyancySensorSpawner.class_name(), ClassName::BuoyancySensor);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(BuoyancySensorSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_flags() {
        let toml_src = r#"
            [metadata]
            class_name = "BuoyancySensor"
            name = "Float"
            [properties]
            fully_submerged = true
            touching_surface = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BuoyancySensorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_bool("fully_submerged"), Some(true));
        assert_eq!(bag.get_bool("touching_surface"), Some(false));
    }
}
