//! `AudioPlayerSpawner` — `ClassSpawner` for [`ClassName::AudioPlayer`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio-asset playback node; the DSP graph wiring is deferred.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioPlayer, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioPlayer`].
#[derive(Default)]
pub struct AudioPlayerSpawner;

impl ClassSpawner for AudioPlayerSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioPlayer
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioPlayer, props);
        let name = instance.name.clone();
        let d = AudioPlayer::default();
        let comp = AudioPlayer {
            asset_id: props.get_string("asset_id").map(str::to_string).unwrap_or(d.asset_id),
            looping: props.get_bool("looping").unwrap_or(d.looping),
            playback_speed: props.get_f32("playback_speed").unwrap_or(d.playback_speed),
            volume: props.get_f32("volume").unwrap_or(d.volume),
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
        if let Some(mut comp) = world.get_mut::<AudioPlayer>(entity) {
            if let Some(v) = props.get_string("asset_id") { comp.asset_id = v.to_string(); }
            if let Some(v) = props.get_bool("looping") { comp.looping = v; }
            if let Some(v) = props.get_f32("playback_speed") { comp.playback_speed = v; }
            if let Some(v) = props.get_f32("volume") { comp.volume = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("AssetId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("asset_id", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Looping").and_then(|p| p.as_bool()) {
            bag.set("looping", PropertyValue::Bool(v));
        }
        for (rbx_key, key) in [("PlaybackSpeed", "playback_speed"), ("Volume", "volume")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("asset_id").and_then(|v| v.as_str()) {
                bag.set("asset_id", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("looping").and_then(|v| v.as_bool()) {
                bag.set("looping", PropertyValue::Bool(v));
            }
            for key in ["playback_speed", "volume"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioPlayer")),
        );
        if let Some(comp) = world.get::<AudioPlayer>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("asset_id".into(), toml::Value::String(comp.asset_id.clone()));
            props.insert("looping".into(), toml::Value::Boolean(comp.looping));
            props.insert("playback_speed".into(), toml::Value::Float(comp.playback_speed as f64));
            props.insert("volume".into(), toml::Value::Float(comp.volume as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_audio_player() {
        assert_eq!(AudioPlayerSpawner.class_name(), ClassName::AudioPlayer);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioPlayerSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioPlayer"
            name = "Track"
            [properties]
            asset_id = "rbxassetid://5"
            looping = true
            volume = 0.5
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioPlayerSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("asset_id"), Some("rbxassetid://5"));
        assert_eq!(bag.get_bool("looping"), Some(true));
        assert_eq!(bag.get_f32("volume"), Some(0.5));
    }
}
