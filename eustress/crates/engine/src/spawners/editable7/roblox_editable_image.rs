//! `RobloxEditableImageSpawner` — `ClassSpawner` for [`ClassName::RobloxEditableImage`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.G). Data-attach config
//! carrier: the engine-namespaced twin of [`EditableImage`], sized `width` ×
//! `height` pixels (Roblox `Size`). See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, RobloxEditableImage};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::RobloxEditableImage`].
#[derive(Default)]
pub struct RobloxEditableImageSpawner;

impl ClassSpawner for RobloxEditableImageSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::RobloxEditableImage
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::RobloxEditableImage, props);
        let name = instance.name.clone();
        let d = RobloxEditableImage::default();
        let comp = RobloxEditableImage {
            width: props.get_i32("width").unwrap_or(d.width),
            height: props.get_i32("height").unwrap_or(d.height),
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
        if let Some(mut comp) = world.get_mut::<RobloxEditableImage>(entity) {
            if let Some(v) = props.get_i32("width") { comp.width = v; }
            if let Some(v) = props.get_i32("height") { comp.height = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Width").and_then(|p| p.as_i32()) {
            bag.set("width", PropertyValue::Int(v));
        }
        if let Some(v) = rbx.property("Height").and_then(|p| p.as_i32()) {
            bag.set("height", PropertyValue::Int(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("width").and_then(|v| v.as_integer()) {
                bag.set("width", PropertyValue::Int(v as i32));
            }
            if let Some(v) = props.get("height").and_then(|v| v.as_integer()) {
                bag.set("height", PropertyValue::Int(v as i32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "RobloxEditableImage")),
        );
        if let Some(comp) = world.get::<RobloxEditableImage>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("width".into(), toml::Value::Integer(comp.width as i64));
            props.insert("height".into(), toml::Value::Integer(comp.height as i64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_roblox_editable_image() {
        assert_eq!(
            RobloxEditableImageSpawner.class_name(),
            ClassName::RobloxEditableImage
        );
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(RobloxEditableImageSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_dimensions() {
        let toml_src = r#"
            [metadata]
            class_name = "RobloxEditableImage"
            name = "Canvas"
            [properties]
            width = 512
            height = 512
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = RobloxEditableImageSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("width"), Some(512));
        assert_eq!(bag.get_i32("height"), Some(512));
    }
}
