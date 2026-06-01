//! `UIFlexItemSpawner` — `ClassSpawner` for [`ClassName::UIFlexItem`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: per-child flexbox sizing within a UIListLayout.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIFlexItem};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIFlexItem`].
#[derive(Default)]
pub struct UIFlexItemSpawner;

impl ClassSpawner for UIFlexItemSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIFlexItem
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIFlexItem, props);
        let name = instance.name.clone();
        let d = UIFlexItem::default();
        let comp = UIFlexItem {
            flex_mode: props.get_string("flex_mode").map(str::to_string).unwrap_or(d.flex_mode),
            grow_ratio: props.get_f32("grow_ratio").unwrap_or(d.grow_ratio),
            shrink_ratio: props.get_f32("shrink_ratio").unwrap_or(d.shrink_ratio),
            item_line_alignment: props.get_string("item_line_alignment").map(str::to_string).unwrap_or(d.item_line_alignment),
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
        if let Some(mut comp) = world.get_mut::<UIFlexItem>(entity) {
            if let Some(v) = props.get_string("flex_mode") { comp.flex_mode = v.to_string(); }
            if let Some(v) = props.get_f32("grow_ratio") { comp.grow_ratio = v; }
            if let Some(v) = props.get_f32("shrink_ratio") { comp.shrink_ratio = v; }
            if let Some(v) = props.get_string("item_line_alignment") { comp.item_line_alignment = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [("FlexMode", "flex_mode"), ("ItemLineAlignment", "item_line_alignment")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        for (rbx_key, key) in [("GrowRatio", "grow_ratio"), ("ShrinkRatio", "shrink_ratio")] {
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
            for key in ["flex_mode", "item_line_alignment"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            for key in ["grow_ratio", "shrink_ratio"] {
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
            toml::Value::Table(export_metadata(world, entity, "UIFlexItem")),
        );
        if let Some(comp) = world.get::<UIFlexItem>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("flex_mode".into(), toml::Value::String(comp.flex_mode.clone()));
            props.insert("grow_ratio".into(), toml::Value::Float(comp.grow_ratio as f64));
            props.insert("shrink_ratio".into(), toml::Value::Float(comp.shrink_ratio as f64));
            props.insert("item_line_alignment".into(), toml::Value::String(comp.item_line_alignment.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_flex_item() {
        assert_eq!(UIFlexItemSpawner.class_name(), ClassName::UIFlexItem);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIFlexItemSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIFlexItem"
            name = "Flex"
            [properties]
            flex_mode = "Grow"
            grow_ratio = 2.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIFlexItemSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("flex_mode"), Some("Grow"));
        assert_eq!(bag.get_f32("grow_ratio"), Some(2.0));
    }
}
