//! `UIPaddingSpawner` — `ClassSpawner` for [`ClassName::UIPadding`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: inner padding for a container GuiObject.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIPadding};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIPadding`].
#[derive(Default)]
pub struct UIPaddingSpawner;

impl ClassSpawner for UIPaddingSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIPadding
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIPadding, props);
        let name = instance.name.clone();
        let d = UIPadding::default();
        let comp = UIPadding {
            padding_top: props.get_f32("padding_top").unwrap_or(d.padding_top),
            padding_bottom: props.get_f32("padding_bottom").unwrap_or(d.padding_bottom),
            padding_left: props.get_f32("padding_left").unwrap_or(d.padding_left),
            padding_right: props.get_f32("padding_right").unwrap_or(d.padding_right),
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
        if let Some(mut comp) = world.get_mut::<UIPadding>(entity) {
            if let Some(v) = props.get_f32("padding_top") { comp.padding_top = v; }
            if let Some(v) = props.get_f32("padding_bottom") { comp.padding_bottom = v; }
            if let Some(v) = props.get_f32("padding_left") { comp.padding_left = v; }
            if let Some(v) = props.get_f32("padding_right") { comp.padding_right = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [
            ("PaddingTop", "padding_top"),
            ("PaddingBottom", "padding_bottom"),
            ("PaddingLeft", "padding_left"),
            ("PaddingRight", "padding_right"),
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
            for key in ["padding_top", "padding_bottom", "padding_left", "padding_right"] {
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
            toml::Value::Table(export_metadata(world, entity, "UIPadding")),
        );
        if let Some(comp) = world.get::<UIPadding>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("padding_top".into(), toml::Value::Float(comp.padding_top as f64));
            props.insert("padding_bottom".into(), toml::Value::Float(comp.padding_bottom as f64));
            props.insert("padding_left".into(), toml::Value::Float(comp.padding_left as f64));
            props.insert("padding_right".into(), toml::Value::Float(comp.padding_right as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_padding() {
        assert_eq!(UIPaddingSpawner.class_name(), ClassName::UIPadding);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIPaddingSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIPadding"
            name = "Pad"
            [properties]
            padding_top = 10.0
            padding_left = 5.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIPaddingSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("padding_top"), Some(10.0));
        assert_eq!(bag.get_f32("padding_left"), Some(5.0));
    }
}
