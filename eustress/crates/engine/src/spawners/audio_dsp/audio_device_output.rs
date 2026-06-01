//! `AudioDeviceOutputSpawner` — `ClassSpawner` for
//! [`ClassName::AudioDeviceOutput`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! speaker/device-output node; the playback wiring is deferred.
//!
//! `player` is an optional Eustress instance-id reference (`Option<u32>`),
//! round-tripped through the bag as `Int` (`-1` ⇒ `None`).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioDeviceOutput, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

fn read_player(props: &PropertyBag) -> Option<u32> {
    props
        .get_i32("player")
        .and_then(|i| if i < 0 { None } else { Some(i as u32) })
}

/// Zero-sized spawner for [`ClassName::AudioDeviceOutput`].
#[derive(Default)]
pub struct AudioDeviceOutputSpawner;

impl ClassSpawner for AudioDeviceOutputSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioDeviceOutput
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioDeviceOutput, props);
        let name = instance.name.clone();
        let comp = AudioDeviceOutput {
            player: read_player(props),
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
        if props.get("player").is_some() {
            if let Some(mut comp) = world.get_mut::<AudioDeviceOutput>(entity) {
                comp.player = read_player(props);
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
        if let Some(v) = rbx.property("Player").and_then(|p| p.as_i32()) {
            bag.set("player", PropertyValue::Int(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("player").and_then(|v| v.as_integer()) {
                bag.set("player", PropertyValue::Int(v as i32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioDeviceOutput")),
        );
        if let Some(comp) = world.get::<AudioDeviceOutput>(entity) {
            let mut props = toml::value::Table::new();
            if let Some(p) = comp.player {
                props.insert("player".into(), toml::Value::Integer(p as i64));
            }
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_audio_device_output() {
        assert_eq!(AudioDeviceOutputSpawner.class_name(), ClassName::AudioDeviceOutput);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioDeviceOutputSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_player() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioDeviceOutput"
            name = "Spk"
            [properties]
            player = 7
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioDeviceOutputSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("player"), Some(7));
    }
}
