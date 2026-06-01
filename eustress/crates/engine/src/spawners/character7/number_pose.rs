//! `NumberPoseSpawner` — `ClassSpawner` for [`ClassName::NumberPose`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! scalar pose value within a keyframe. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, NumberPose, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::NumberPose`].
#[derive(Default)]
pub struct NumberPoseSpawner;

impl ClassSpawner for NumberPoseSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::NumberPose
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::NumberPose, props);
        let name = instance.name.clone();
        let comp = NumberPose {
            value: props.get_f32("value").unwrap_or(NumberPose::default().value),
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
        if let Some(v) = props.get_f32("value") {
            if let Some(mut comp) = world.get_mut::<NumberPose>(entity) {
                comp.value = v;
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Value").and_then(|p| p.as_f32()) {
            bag.set("value", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("value").and_then(|v| v.as_float()) {
                bag.set("value", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "NumberPose")),
        );
        if let Some(comp) = world.get::<NumberPose>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("value".into(), toml::Value::Float(comp.value as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_number_pose() {
        assert_eq!(NumberPoseSpawner.class_name(), ClassName::NumberPose);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(NumberPoseSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_value() {
        let toml_src = r#"
            [metadata]
            class_name = "NumberPose"
            name = "Curl"
            [properties]
            value = 0.42
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = NumberPoseSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("value"), Some(0.42));
    }
}
