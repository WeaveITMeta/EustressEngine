//! `UITextSizeConstraintSpawner` — `ClassSpawner` for
//! [`ClassName::UITextSizeConstraint`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: clamps auto-scaled text size.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UITextSizeConstraint};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UITextSizeConstraint`].
#[derive(Default)]
pub struct UITextSizeConstraintSpawner;

impl ClassSpawner for UITextSizeConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UITextSizeConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UITextSizeConstraint, props);
        let name = instance.name.clone();
        let d = UITextSizeConstraint::default();
        let comp = UITextSizeConstraint {
            min_text_size: props.get_f32("min_text_size").unwrap_or(d.min_text_size),
            max_text_size: props.get_f32("max_text_size").unwrap_or(d.max_text_size),
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
        if let Some(mut comp) = world.get_mut::<UITextSizeConstraint>(entity) {
            if let Some(v) = props.get_f32("min_text_size") { comp.min_text_size = v; }
            if let Some(v) = props.get_f32("max_text_size") { comp.max_text_size = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("MinTextSize").and_then(|p| p.as_f32()) {
            bag.set("min_text_size", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("MaxTextSize").and_then(|p| p.as_f32()) {
            bag.set("max_text_size", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["min_text_size", "max_text_size"] {
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
            toml::Value::Table(export_metadata(world, entity, "UITextSizeConstraint")),
        );
        if let Some(comp) = world.get::<UITextSizeConstraint>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("min_text_size".into(), toml::Value::Float(comp.min_text_size as f64));
            props.insert("max_text_size".into(), toml::Value::Float(comp.max_text_size as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_text_size_constraint() {
        assert_eq!(UITextSizeConstraintSpawner.class_name(), ClassName::UITextSizeConstraint);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UITextSizeConstraintSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UITextSizeConstraint"
            name = "TextClamp"
            [properties]
            min_text_size = 8.0
            max_text_size = 48.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UITextSizeConstraintSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("min_text_size"), Some(8.0));
        assert_eq!(bag.get_f32("max_text_size"), Some(48.0));
    }
}
