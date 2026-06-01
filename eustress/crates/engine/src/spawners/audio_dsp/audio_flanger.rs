//! `AudioFlangerSpawner` — `ClassSpawner` for [`ClassName::AudioFlanger`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio DSP parameters; the DSP graph wiring is deferred (see the group
//! [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, AudioFlanger};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioFlanger`].
#[derive(Default)]
pub struct AudioFlangerSpawner;

impl ClassSpawner for AudioFlangerSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioFlanger
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioFlanger, props);
        let name = instance.name.clone();
        let d = AudioFlanger::default();
        let comp = AudioFlanger {
            rate: props.get_f32("rate").unwrap_or(d.rate),
            depth: props.get_f32("depth").unwrap_or(d.depth),
            mix: props.get_f32("mix").unwrap_or(d.mix),
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
        if let Some(mut comp) = world.get_mut::<AudioFlanger>(entity) {
            if let Some(v) = props.get_f32("rate") { comp.rate = v; }
            if let Some(v) = props.get_f32("depth") { comp.depth = v; }
            if let Some(v) = props.get_f32("mix") { comp.mix = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Rate").and_then(|p| p.as_f32()) { bag.set("rate", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Depth").and_then(|p| p.as_f32()) { bag.set("depth", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Mix").and_then(|p| p.as_f32()) { bag.set("mix", PropertyValue::Float(v)); }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("rate").and_then(|v| v.as_float()) { bag.set("rate", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("depth").and_then(|v| v.as_float()) { bag.set("depth", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("mix").and_then(|v| v.as_float()) { bag.set("mix", PropertyValue::Float(v as f32)); }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioFlanger")),
        );
        if let Some(comp) = world.get::<AudioFlanger>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("rate".into(), toml::Value::Float(comp.rate as f64));
            props.insert("depth".into(), toml::Value::Float(comp.depth as f64));
            props.insert("mix".into(), toml::Value::Float(comp.mix as f64));
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
        assert_eq!(AudioFlangerSpawner.class_name(), ClassName::AudioFlanger);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioFlangerSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_first_field() {
        let toml_src = "[metadata]\nclass_name = \"AudioFlanger\"\nname = \"X\"\n[properties]\nrate= 0.5\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioFlangerSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("rate"), Some(0.5));
    }
}
