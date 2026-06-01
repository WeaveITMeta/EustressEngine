//! `CanvasGroupSpawner` — `ClassSpawner` for [`ClassName::CanvasGroup`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! GuiObject container that composites its children's color/transparency as
//! one layer.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{CanvasGroup, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::CanvasGroup`].
#[derive(Default)]
pub struct CanvasGroupSpawner;

impl ClassSpawner for CanvasGroupSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::CanvasGroup
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::CanvasGroup, props);
        let name = instance.name.clone();
        let d = CanvasGroup::default();
        let comp = CanvasGroup {
            group_color3: props.get_color3("group_color3").unwrap_or(d.group_color3),
            group_transparency: props.get_f32("group_transparency").unwrap_or(d.group_transparency),
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
        if let Some(mut comp) = world.get_mut::<CanvasGroup>(entity) {
            if let Some(v) = props.get_color3("group_color3") { comp.group_color3 = v; }
            if let Some(v) = props.get_f32("group_transparency") { comp.group_transparency = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("GroupTransparency").and_then(|p| p.as_f32()) {
            bag.set("group_transparency", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("group_color3").and_then(|v| v.as_array()) {
                let r = arr.first().and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
                let g = arr.get(1).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
                let b = arr.get(2).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
                bag.set("group_color3", PropertyValue::Color3([r, g, b]));
            }
            if let Some(v) = props.get("group_transparency").and_then(|v| v.as_float()) {
                bag.set("group_transparency", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "CanvasGroup")),
        );
        if let Some(comp) = world.get::<CanvasGroup>(entity) {
            let mut props = toml::value::Table::new();
            props.insert(
                "group_color3".into(),
                toml::Value::Array(comp.group_color3.iter().map(|c| toml::Value::Float(*c as f64)).collect()),
            );
            props.insert("group_transparency".into(), toml::Value::Float(comp.group_transparency as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_canvas_group() {
        assert_eq!(CanvasGroupSpawner.class_name(), ClassName::CanvasGroup);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(CanvasGroupSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "CanvasGroup"
            name = "Layer"
            [properties]
            group_transparency = 0.25
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = CanvasGroupSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("group_transparency"), Some(0.25));
    }
}
