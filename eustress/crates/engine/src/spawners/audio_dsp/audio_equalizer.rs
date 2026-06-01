//! `AudioEqualizerSpawner` — `ClassSpawner` for [`ClassName::AudioEqualizer`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio DSP parameters; the DSP graph wiring is deferred (see the group
//! [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, AudioEqualizer};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioEqualizer`].
#[derive(Default)]
pub struct AudioEqualizerSpawner;

impl ClassSpawner for AudioEqualizerSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioEqualizer
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioEqualizer, props);
        let name = instance.name.clone();
        let d = AudioEqualizer::default();
        let comp = AudioEqualizer {
            low_gain: props.get_f32("low_gain").unwrap_or(d.low_gain),
            mid_gain: props.get_f32("mid_gain").unwrap_or(d.mid_gain),
            high_gain: props.get_f32("high_gain").unwrap_or(d.high_gain),
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
        if let Some(mut comp) = world.get_mut::<AudioEqualizer>(entity) {
            if let Some(v) = props.get_f32("low_gain") { comp.low_gain = v; }
            if let Some(v) = props.get_f32("mid_gain") { comp.mid_gain = v; }
            if let Some(v) = props.get_f32("high_gain") { comp.high_gain = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("LowGain").and_then(|p| p.as_f32()) { bag.set("low_gain", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("MidGain").and_then(|p| p.as_f32()) { bag.set("mid_gain", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("HighGain").and_then(|p| p.as_f32()) { bag.set("high_gain", PropertyValue::Float(v)); }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("low_gain").and_then(|v| v.as_float()) { bag.set("low_gain", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("mid_gain").and_then(|v| v.as_float()) { bag.set("mid_gain", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("high_gain").and_then(|v| v.as_float()) { bag.set("high_gain", PropertyValue::Float(v as f32)); }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioEqualizer")),
        );
        if let Some(comp) = world.get::<AudioEqualizer>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("low_gain".into(), toml::Value::Float(comp.low_gain as f64));
            props.insert("mid_gain".into(), toml::Value::Float(comp.mid_gain as f64));
            props.insert("high_gain".into(), toml::Value::Float(comp.high_gain as f64));
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
        assert_eq!(AudioEqualizerSpawner.class_name(), ClassName::AudioEqualizer);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioEqualizerSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_first_field() {
        let toml_src = "[metadata]\nclass_name = \"AudioEqualizer\"\nname = \"X\"\n[properties]\nlow_gain= 0.5\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioEqualizerSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("low_gain"), Some(0.5));
    }
}
