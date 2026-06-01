//! `TremoloSoundEffectSpawner` — `ClassSpawner` for [`ClassName::TremoloSoundEffect`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio DSP parameters; the DSP graph wiring is deferred (see the group
//! [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, TremoloSoundEffect};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::TremoloSoundEffect`].
#[derive(Default)]
pub struct TremoloSoundEffectSpawner;

impl ClassSpawner for TremoloSoundEffectSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::TremoloSoundEffect
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::TremoloSoundEffect, props);
        let name = instance.name.clone();
        let d = TremoloSoundEffect::default();
        let comp = TremoloSoundEffect {
            duty: props.get_f32("duty").unwrap_or(d.duty),
            frequency: props.get_f32("frequency").unwrap_or(d.frequency),
            depth: props.get_f32("depth").unwrap_or(d.depth),
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
        if let Some(mut comp) = world.get_mut::<TremoloSoundEffect>(entity) {
            if let Some(v) = props.get_f32("duty") { comp.duty = v; }
            if let Some(v) = props.get_f32("frequency") { comp.frequency = v; }
            if let Some(v) = props.get_f32("depth") { comp.depth = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Duty").and_then(|p| p.as_f32()) { bag.set("duty", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Frequency").and_then(|p| p.as_f32()) { bag.set("frequency", PropertyValue::Float(v)); }
        if let Some(v) = rbx.property("Depth").and_then(|p| p.as_f32()) { bag.set("depth", PropertyValue::Float(v)); }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("duty").and_then(|v| v.as_float()) { bag.set("duty", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("frequency").and_then(|v| v.as_float()) { bag.set("frequency", PropertyValue::Float(v as f32)); }
            if let Some(v) = props.get("depth").and_then(|v| v.as_float()) { bag.set("depth", PropertyValue::Float(v as f32)); }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "TremoloSoundEffect")),
        );
        if let Some(comp) = world.get::<TremoloSoundEffect>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("duty".into(), toml::Value::Float(comp.duty as f64));
            props.insert("frequency".into(), toml::Value::Float(comp.frequency as f64));
            props.insert("depth".into(), toml::Value::Float(comp.depth as f64));
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
        assert_eq!(TremoloSoundEffectSpawner.class_name(), ClassName::TremoloSoundEffect);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(TremoloSoundEffectSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_first_field() {
        let toml_src = "[metadata]\nclass_name = \"TremoloSoundEffect\"\nname = \"X\"\n[properties]\nduty= 0.5\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = TremoloSoundEffectSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("duty"), Some(0.5));
    }
}
