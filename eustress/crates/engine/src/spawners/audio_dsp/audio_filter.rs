//! `AudioFilterSpawner` — `ClassSpawner` for [`ClassName::AudioFilter`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! frequency-filter DSP node; the DSP graph wiring is deferred.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioFilter, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioFilter`].
#[derive(Default)]
pub struct AudioFilterSpawner;

impl ClassSpawner for AudioFilterSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioFilter
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioFilter, props);
        let name = instance.name.clone();
        let d = AudioFilter::default();
        let comp = AudioFilter {
            frequency: props.get_f32("frequency").unwrap_or(d.frequency),
            q: props.get_f32("q").unwrap_or(d.q),
            gain: props.get_f32("gain").unwrap_or(d.gain),
            filter_type: props.get_string("filter_type").map(str::to_string).unwrap_or(d.filter_type),
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
        if let Some(mut comp) = world.get_mut::<AudioFilter>(entity) {
            if let Some(v) = props.get_f32("frequency") { comp.frequency = v; }
            if let Some(v) = props.get_f32("q") { comp.q = v; }
            if let Some(v) = props.get_f32("gain") { comp.gain = v; }
            if let Some(v) = props.get_string("filter_type") { comp.filter_type = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [("Frequency", "frequency"), ("Q", "q"), ("Gain", "gain")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        if let Some(v) = rbx.property("FilterType").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("filter_type", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["frequency", "q", "gain"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            if let Some(v) = props.get("filter_type").and_then(|v| v.as_str()) {
                bag.set("filter_type", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioFilter")),
        );
        if let Some(comp) = world.get::<AudioFilter>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("frequency".into(), toml::Value::Float(comp.frequency as f64));
            props.insert("q".into(), toml::Value::Float(comp.q as f64));
            props.insert("gain".into(), toml::Value::Float(comp.gain as f64));
            props.insert("filter_type".into(), toml::Value::String(comp.filter_type.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_audio_filter() {
        assert_eq!(AudioFilterSpawner.class_name(), ClassName::AudioFilter);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioFilterSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioFilter"
            name = "LP"
            [properties]
            frequency = 800.0
            filter_type = "Highpass"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioFilterSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("frequency"), Some(800.0));
        assert_eq!(bag.get_string("filter_type"), Some("Highpass"));
    }
}
