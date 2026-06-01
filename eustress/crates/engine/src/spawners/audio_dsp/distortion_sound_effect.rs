//! `DistortionSoundEffectSpawner` — `ClassSpawner` for [`ClassName::DistortionSoundEffect`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio DSP parameters; the DSP graph wiring is deferred (see the group
//! [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, DistortionSoundEffect};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::DistortionSoundEffect`].
#[derive(Default)]
pub struct DistortionSoundEffectSpawner;

impl ClassSpawner for DistortionSoundEffectSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::DistortionSoundEffect
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::DistortionSoundEffect, props);
        let name = instance.name.clone();
        let d = DistortionSoundEffect::default();
        let comp = DistortionSoundEffect {
            level: props.get_f32("level").unwrap_or(d.level),
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
        if let Some(mut comp) = world.get_mut::<DistortionSoundEffect>(entity) {
            if let Some(v) = props.get_f32("level") { comp.level = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Level").and_then(|p| p.as_f32()) { bag.set("level", PropertyValue::Float(v)); }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("level").and_then(|v| v.as_float()) { bag.set("level", PropertyValue::Float(v as f32)); }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "DistortionSoundEffect")),
        );
        if let Some(comp) = world.get::<DistortionSoundEffect>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("level".into(), toml::Value::Float(comp.level as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_matches() {
        assert_eq!(DistortionSoundEffectSpawner.class_name(), ClassName::DistortionSoundEffect);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(DistortionSoundEffectSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_first_field() {
        let toml_src = "[metadata]\nclass_name = \"DistortionSoundEffect\"\nname = \"X\"\n[properties]\nlevel= 0.5\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = DistortionSoundEffectSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("level"), Some(0.5));
    }
}
