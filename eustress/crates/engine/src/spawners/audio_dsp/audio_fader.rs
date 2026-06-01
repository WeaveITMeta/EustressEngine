//! `AudioFaderSpawner` — `ClassSpawner` for [`ClassName::AudioFader`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! volume-fader node; the DSP graph wiring is deferred.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioFader, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioFader`].
#[derive(Default)]
pub struct AudioFaderSpawner;

impl ClassSpawner for AudioFaderSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioFader
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioFader, props);
        let name = instance.name.clone();
        let d = AudioFader::default();
        let comp = AudioFader {
            volume: props.get_f32("volume").unwrap_or(d.volume),
            bypass: props.get_bool("bypass").unwrap_or(d.bypass),
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
        if let Some(mut comp) = world.get_mut::<AudioFader>(entity) {
            if let Some(v) = props.get_f32("volume") { comp.volume = v; }
            if let Some(v) = props.get_bool("bypass") { comp.bypass = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Volume").and_then(|p| p.as_f32()) {
            bag.set("volume", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("Bypass").and_then(|p| p.as_bool()) {
            bag.set("bypass", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("volume").and_then(|v| v.as_float()) {
                bag.set("volume", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("bypass").and_then(|v| v.as_bool()) {
                bag.set("bypass", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioFader")),
        );
        if let Some(comp) = world.get::<AudioFader>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("volume".into(), toml::Value::Float(comp.volume as f64));
            props.insert("bypass".into(), toml::Value::Boolean(comp.bypass));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_audio_fader() {
        assert_eq!(AudioFaderSpawner.class_name(), ClassName::AudioFader);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioFaderSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioFader"
            name = "Fade"
            [properties]
            volume = 0.8
            bypass = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioFaderSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("volume"), Some(0.8));
        assert_eq!(bag.get_bool("bypass"), Some(true));
    }
}
