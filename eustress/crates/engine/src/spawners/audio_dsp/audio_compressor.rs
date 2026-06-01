//! `AudioCompressorSpawner` — `ClassSpawner` for [`ClassName::AudioCompressor`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio DSP parameters; the DSP graph wiring is deferred (see the group
//! [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, AudioCompressor};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioCompressor`].
#[derive(Default)]
pub struct AudioCompressorSpawner;

impl ClassSpawner for AudioCompressorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioCompressor
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioCompressor, props);
        let name = instance.name.clone();
        let d = AudioCompressor::default();
        let comp = AudioCompressor {
            attack: props.get_f32("attack").unwrap_or(d.attack),
            release: props.get_f32("release").unwrap_or(d.release),
            threshold: props.get_f32("threshold").unwrap_or(d.threshold),
            ratio: props.get_f32("ratio").unwrap_or(d.ratio),
            makeup_gain: props.get_f32("makeup_gain").unwrap_or(d.makeup_gain),
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
        if let Some(mut comp) = world.get_mut::<AudioCompressor>(entity) {
            if let Some(v) = props.get_f32("attack") { comp.attack = v; }
            if let Some(v) = props.get_f32("release") { comp.release = v; }
            if let Some(v) = props.get_f32("threshold") { comp.threshold = v; }
            if let Some(v) = props.get_f32("ratio") { comp.ratio = v; }
            if let Some(v) = props.get_f32("makeup_gain") { comp.makeup_gain = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Attack").and_then(|p| p.as_f32()) { bag.set("attack", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Release").and_then(|p| p.as_f32()) { bag.set("release", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Threshold").and_then(|p| p.as_f32()) { bag.set("threshold", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Ratio").and_then(|p| p.as_f32()) { bag.set("ratio", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("GainMakeup").and_then(|p| p.as_f32()) { bag.set("makeup_gain", PropertyValue::Float(v)); }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("attack").and_then(|v| v.as_float()) { bag.set("attack", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("release").and_then(|v| v.as_float()) { bag.set("release", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("threshold").and_then(|v| v.as_float()) { bag.set("threshold", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("ratio").and_then(|v| v.as_float()) { bag.set("ratio", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("makeup_gain").and_then(|v| v.as_float()) { bag.set("makeup_gain", PropertyValue::Float(v as f32)); }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioCompressor")),
        );
        if let Some(comp) = world.get::<AudioCompressor>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("attack".into(), toml::Value::Float(comp.attack as f64));
            props.insert("release".into(), toml::Value::Float(comp.release as f64));
            props.insert("threshold".into(), toml::Value::Float(comp.threshold as f64));
            props.insert("ratio".into(), toml::Value::Float(comp.ratio as f64));
            props.insert("makeup_gain".into(), toml::Value::Float(comp.makeup_gain as f64));
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
        assert_eq!(AudioCompressorSpawner.class_name(), ClassName::AudioCompressor);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioCompressorSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_first_field() {
        let toml_src = "[metadata]\nclass_name = \"AudioCompressor\"\nname = \"X\"\n[properties]\nattack= 0.5\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioCompressorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("attack"), Some(0.5));
    }
}
