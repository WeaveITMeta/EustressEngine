//! `AudioDeviceInputSpawner` — `ClassSpawner` for
//! [`ClassName::AudioDeviceInput`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! microphone/device-input node; the capture wiring is deferred.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioDeviceInput, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioDeviceInput`].
#[derive(Default)]
pub struct AudioDeviceInputSpawner;

impl ClassSpawner for AudioDeviceInputSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioDeviceInput
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioDeviceInput, props);
        let name = instance.name.clone();
        let comp = AudioDeviceInput {
            active: props.get_bool("active").unwrap_or(AudioDeviceInput::default().active),
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
        if let Some(v) = props.get_bool("active") {
            if let Some(mut comp) = world.get_mut::<AudioDeviceInput>(entity) {
                comp.active = v;
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Active").and_then(|p| p.as_bool()) {
            bag.set("active", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("active").and_then(|v| v.as_bool()) {
                bag.set("active", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioDeviceInput")),
        );
        if let Some(comp) = world.get::<AudioDeviceInput>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("active".into(), toml::Value::Boolean(comp.active));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_audio_device_input() {
        assert_eq!(AudioDeviceInputSpawner.class_name(), ClassName::AudioDeviceInput);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioDeviceInputSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_active() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioDeviceInput"
            name = "Mic"
            [properties]
            active = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioDeviceInputSpawner.import_from_toml(&value);
        assert_eq!(bag.get_bool("active"), Some(true));
    }
}
