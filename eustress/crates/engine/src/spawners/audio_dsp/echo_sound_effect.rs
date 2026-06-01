//! `EchoSoundEffectSpawner` — `ClassSpawner` for [`ClassName::EchoSoundEffect`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio DSP parameters; the DSP graph wiring is deferred (see the group
//! [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, EchoSoundEffect};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::EchoSoundEffect`].
#[derive(Default)]
pub struct EchoSoundEffectSpawner;

impl ClassSpawner for EchoSoundEffectSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::EchoSoundEffect
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::EchoSoundEffect, props);
        let name = instance.name.clone();
        let d = EchoSoundEffect::default();
        let comp = EchoSoundEffect {
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
        if let Some(mut comp) = world.get_mut::<EchoSoundEffect>(entity) {
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
        if let Some(v) = rbx.property("Delay").and_then(|p| p.as_f32()) { bag.set("delay", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Feedback").and_then(|p| p.as_f32()) { bag.set("feedback", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("DryLevel").and_then(|p| p.as_f32()) { bag.set("dry_level", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("WetLevel").and_then(|p| p.as_f32()) { bag.set("wet_level", PropertyValue::Float(v)); }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("delay").and_then(|v| v.as_float()) { bag.set("delay", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("feedback").and_then(|v| v.as_float()) { bag.set("feedback", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("dry_level").and_then(|v| v.as_float()) { bag.set("dry_level", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("wet_level").and_then(|v| v.as_float()) { bag.set("wet_level", PropertyValue::Float(v as f32)); }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "EchoSoundEffect")),
        );
        if let Some(comp) = world.get::<EchoSoundEffect>(entity) {
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
    fn class_name_matches() {
        assert_eq!(EchoSoundEffectSpawner.class_name(), ClassName::EchoSoundEffect);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(EchoSoundEffectSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_first_field() {
        let toml_src = "[metadata]\nclass_name = \"EchoSoundEffect\"\nname = \"X\"\n[properties]\ndelay= 0.5\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = EchoSoundEffectSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("delay"), Some(0.5));
    }
}
