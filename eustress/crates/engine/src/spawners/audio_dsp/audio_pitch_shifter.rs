//! `AudioPitchShifterSpawner` — `ClassSpawner` for [`ClassName::AudioPitchShifter`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! audio DSP parameters; the DSP graph wiring is deferred (see the group
//! [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, AudioPitchShifter};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioPitchShifter`].
#[derive(Default)]
pub struct AudioPitchShifterSpawner;

impl ClassSpawner for AudioPitchShifterSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioPitchShifter
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioPitchShifter, props);
        let name = instance.name.clone();
        let d = AudioPitchShifter::default();
        let comp = AudioPitchShifter {
            pitch: props.get_f32("pitch").unwrap_or(d.pitch),
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
        if let Some(mut comp) = world.get_mut::<AudioPitchShifter>(entity) {
            if let Some(v) = props.get_f32("pitch") { comp.pitch = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Pitch").and_then(|p| p.as_f32()) { bag.set("pitch", PropertyValue::Float(v)); }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("pitch").and_then(|v| v.as_float()) { bag.set("pitch", PropertyValue::Float(v as f32)); }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioPitchShifter")),
        );
        if let Some(comp) = world.get::<AudioPitchShifter>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("pitch".into(), toml::Value::Float(comp.pitch as f64));
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
        assert_eq!(AudioPitchShifterSpawner.class_name(), ClassName::AudioPitchShifter);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioPitchShifterSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_first_field() {
        let toml_src = "[metadata]\nclass_name = \"AudioPitchShifter\"\nname = \"X\"\n[properties]\npitch= 0.5\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioPitchShifterSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("pitch"), Some(0.5));
    }
}
