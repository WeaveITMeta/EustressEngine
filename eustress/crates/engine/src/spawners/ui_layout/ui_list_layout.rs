//! `UIListLayoutSpawner` — `ClassSpawner` for [`ClassName::UIListLayout`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: auto-arranges sibling GuiObjects in a single row/column.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIListLayout};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIListLayout`].
#[derive(Default)]
pub struct UIListLayoutSpawner;

impl ClassSpawner for UIListLayoutSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIListLayout
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIListLayout, props);
        let name = instance.name.clone();
        let d = UIListLayout::default();
        let comp = UIListLayout {
            padding: props.get_f32("padding").unwrap_or(d.padding),
            fill_direction: props.get_string("fill_direction").map(str::to_string).unwrap_or(d.fill_direction),
            horizontal_alignment: props.get_string("horizontal_alignment").map(str::to_string).unwrap_or(d.horizontal_alignment),
            vertical_alignment: props.get_string("vertical_alignment").map(str::to_string).unwrap_or(d.vertical_alignment),
            sort_order: props.get_string("sort_order").map(str::to_string).unwrap_or(d.sort_order),
            wraps: props.get_bool("wraps").unwrap_or(d.wraps),
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
        if let Some(mut comp) = world.get_mut::<UIListLayout>(entity) {
            if let Some(v) = props.get_f32("padding") { comp.padding = v; }
            if let Some(v) = props.get_string("fill_direction") { comp.fill_direction = v.to_string(); }
            if let Some(v) = props.get_string("horizontal_alignment") { comp.horizontal_alignment = v.to_string(); }
            if let Some(v) = props.get_string("vertical_alignment") { comp.vertical_alignment = v.to_string(); }
            if let Some(v) = props.get_string("sort_order") { comp.sort_order = v.to_string(); }
            if let Some(v) = props.get_bool("wraps") { comp.wraps = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Padding").and_then(|p| p.as_f32()) {
            bag.set("padding", PropertyValue::Float(v));
        }
        for (rbx_key, key) in [
            ("FillDirection", "fill_direction"),
            ("HorizontalAlignment", "horizontal_alignment"),
            ("VerticalAlignment", "vertical_alignment"),
            ("SortOrder", "sort_order"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(7);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("padding").and_then(|v| v.as_float()) {
                bag.set("padding", PropertyValue::Float(v as f32));
            }
            for key in ["fill_direction", "horizontal_alignment", "vertical_alignment", "sort_order"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            if let Some(v) = props.get("wraps").and_then(|v| v.as_bool()) {
                bag.set("wraps", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UIListLayout")),
        );
        if let Some(comp) = world.get::<UIListLayout>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("padding".into(), toml::Value::Float(comp.padding as f64));
            props.insert("fill_direction".into(), toml::Value::String(comp.fill_direction.clone()));
            props.insert("horizontal_alignment".into(), toml::Value::String(comp.horizontal_alignment.clone()));
            props.insert("vertical_alignment".into(), toml::Value::String(comp.vertical_alignment.clone()));
            props.insert("sort_order".into(), toml::Value::String(comp.sort_order.clone()));
            props.insert("wraps".into(), toml::Value::Boolean(comp.wraps));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_list_layout() {
        assert_eq!(UIListLayoutSpawner.class_name(), ClassName::UIListLayout);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIListLayoutSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIListLayout"
            name = "Stack"
            [properties]
            padding = 4.0
            fill_direction = "Horizontal"
            wraps = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIListLayoutSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("padding"), Some(4.0));
        assert_eq!(bag.get_string("fill_direction"), Some("Horizontal"));
        assert_eq!(bag.get_bool("wraps"), Some(true));
    }
}
