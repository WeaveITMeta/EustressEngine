//! `AudioReverbSpawner` — `ClassSpawner` for [`ClassName::AudioReverb`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! reverb DSP parameters; the DSP graph wiring is deferred.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioReverb, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

const F32_FIELDS: [(&str, &str); 5] = [
    ("DecayTime", "decay_time"),
    ("Density", "density"),
    ("Diffusion", "diffusion"),
    ("DryLevel", "dry_level"),
    ("WetLevel", "wet_level"),
];

/// Zero-sized spawner for [`ClassName::AudioReverb`].
#[derive(Default)]
pub struct AudioReverbSpawner;

impl ClassSpawner for AudioReverbSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioReverb
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioReverb, props);
        let name = instance.name.clone();
        let d = AudioReverb::default();
        let comp = AudioReverb {
            decay_time: props.get_f32("decay_time").unwrap_or(d.decay_time),
            density: props.get_f32("density").unwrap_or(d.density),
            diffusion: props.get_f32("diffusion").unwrap_or(d.diffusion),
            dry_level: props.get_f32("dry_level").unwrap_or(d.dry_level),
            wet_level: props.get_f32("wet_level").unwrap_or(d.wet_level),
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
        if let Some(mut comp) = world.get_mut::<AudioReverb>(entity) {
            if let Some(v) = props.get_f32("decay_time") { comp.decay_time = v; }
            if let Some(v) = props.get_f32("density") { comp.density = v; }
            if let Some(v) = props.get_f32("diffusion") { comp.diffusion = v; }
            if let Some(v) = props.get_f32("dry_level") { comp.dry_level = v; }
            if let Some(v) = props.get_f32("wet_level") { comp.wet_level = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in F32_FIELDS {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for (_, key) in F32_FIELDS {
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
            toml::Value::Table(export_metadata(world, entity, "AudioReverb")),
        );
        if let Some(comp) = world.get::<AudioReverb>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("decay_time".into(), toml::Value::Float(comp.decay_time as f64));
            props.insert("density".into(), toml::Value::Float(comp.density as f64));
            props.insert("diffusion".into(), toml::Value::Float(comp.diffusion as f64));
            props.insert("dry_level".into(), toml::Value::Float(comp.dry_level as f64));
            props.insert("wet_level".into(), toml::Value::Float(comp.wet_level as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_audio_reverb() {
        assert_eq!(AudioReverbSpawner.class_name(), ClassName::AudioReverb);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioReverbSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioReverb"
            name = "Hall"
            [properties]
            decay_time = 3.0
            wet_level = -6.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioReverbSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("decay_time"), Some(3.0));
        assert_eq!(bag.get_f32("wet_level"), Some(-6.0));
    }
}
