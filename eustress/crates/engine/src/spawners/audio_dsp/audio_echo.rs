//! `AudioEchoSpawner` — `ClassSpawner` for [`ClassName::AudioEcho`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! echo/delay DSP parameters; the DSP graph wiring is deferred.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioEcho, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioEcho`].
#[derive(Default)]
pub struct AudioEchoSpawner;

impl ClassSpawner for AudioEchoSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioEcho
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioEcho, props);
        let name = instance.name.clone();
        let d = AudioEcho::default();
        let comp = AudioEcho {
            delay: props.get_f32("delay").unwrap_or(d.delay),
            feedback: props.get_f32("feedback").unwrap_or(d.feedback),
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
        if let Some(mut comp) = world.get_mut::<AudioEcho>(entity) {
            if let Some(v) = props.get_f32("delay") { comp.delay = v; }
            if let Some(v) = props.get_f32("feedback") { comp.feedback = v; }
            if let Some(v) = props.get_f32("dry_level") { comp.dry_level = v; }
            if let Some(v) = props.get_f32("wet_level") { comp.wet_level = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [
            ("DelayTime", "delay"),
            ("Feedback", "feedback"),
            ("DryLevel", "dry_level"),
            ("WetLevel", "wet_level"),
        ] {
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
            for key in ["delay", "feedback", "dry_level", "wet_level"] {
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
            toml::Value::Table(export_metadata(world, entity, "AudioEcho")),
        );
        if let Some(comp) = world.get::<AudioEcho>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("delay".into(), toml::Value::Float(comp.delay as f64));
            props.insert("feedback".into(), toml::Value::Float(comp.feedback as f64));
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
    fn class_name_is_audio_echo() {
        assert_eq!(AudioEchoSpawner.class_name(), ClassName::AudioEcho);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioEchoSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioEcho"
            name = "Echo"
            [properties]
            delay = 0.25
            feedback = 0.6
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioEchoSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("delay"), Some(0.25));
        assert_eq!(bag.get_f32("feedback"), Some(0.6));
    }
}
