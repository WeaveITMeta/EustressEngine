//! `AudioSearchParamsSpawner` — `ClassSpawner` for
//! [`ClassName::AudioSearchParams`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.E). Config-attach
//! tag-based audio routing filter; the routing wiring is deferred.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AudioSearchParams, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::AudioSearchParams`].
#[derive(Default)]
pub struct AudioSearchParamsSpawner;

impl ClassSpawner for AudioSearchParamsSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AudioSearchParams
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AudioSearchParams, props);
        let name = instance.name.clone();
        let comp = AudioSearchParams {
            tag: props.get_string("tag").map(str::to_string).unwrap_or_default(),
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
        if let Some(v) = props.get_string("tag") {
            if let Some(mut comp) = world.get_mut::<AudioSearchParams>(entity) {
                comp.tag = v.to_string();
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
        if let Some(v) = rbx.property("Tag").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("tag", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("tag").and_then(|v| v.as_str()) {
                bag.set("tag", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AudioSearchParams")),
        );
        if let Some(comp) = world.get::<AudioSearchParams>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("tag".into(), toml::Value::String(comp.tag.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_audio_search_params() {
        assert_eq!(AudioSearchParamsSpawner.class_name(), ClassName::AudioSearchParams);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AudioSearchParamsSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_tag() {
        let toml_src = r#"
            [metadata]
            class_name = "AudioSearchParams"
            name = "Filter"
            [properties]
            tag = "music"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AudioSearchParamsSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("tag"), Some("music"));
    }
}
