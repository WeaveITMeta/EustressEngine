//! `AnimationSpawner` — `ClassSpawner` for [`ClassName::Animation`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! animation-asset reference. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{Animation, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::Animation`].
#[derive(Default)]
pub struct AnimationSpawner;

impl ClassSpawner for AnimationSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Animation
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Animation, props);
        let name = instance.name.clone();
        let comp = Animation {
            animation_id: props.get_string("animation_id").map(str::to_string).unwrap_or_default(),
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
        if let Some(v) = props.get_string("animation_id") {
            if let Some(mut comp) = world.get_mut::<Animation>(entity) {
                comp.animation_id = v.to_string();
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
        if let Some(v) = rbx.property("AnimationId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("animation_id", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("animation_id").and_then(|v| v.as_str()) {
                bag.set("animation_id", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Animation")),
        );
        if let Some(comp) = world.get::<Animation>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("animation_id".into(), toml::Value::String(comp.animation_id.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_animation() {
        assert_eq!(AnimationSpawner.class_name(), ClassName::Animation);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AnimationSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_id() {
        let toml_src = r#"
            [metadata]
            class_name = "Animation"
            name = "Walk"
            [properties]
            animation_id = "rbxassetid://507"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AnimationSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("animation_id"), Some("rbxassetid://507"));
    }
}
