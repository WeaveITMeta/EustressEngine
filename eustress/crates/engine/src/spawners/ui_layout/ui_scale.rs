//! `UIScaleSpawner` — `ClassSpawner` for [`ClassName::UIScale`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: uniform scale multiplier applied to a GuiObject subtree.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIScale};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIScale`].
#[derive(Default)]
pub struct UIScaleSpawner;

impl ClassSpawner for UIScaleSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIScale
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIScale, props);
        let name = instance.name.clone();
        let comp = UIScale {
            scale: props.get_f32("scale").unwrap_or_else(|| UIScale::default().scale),
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
        if let Some(v) = props.get_f32("scale") {
            if let Some(mut comp) = world.get_mut::<UIScale>(entity) {
                comp.scale = v;
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
        if let Some(v) = rbx.property("Scale").and_then(|p| p.as_f32()) {
            bag.set("scale", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("scale").and_then(|v| v.as_float()) {
                bag.set("scale", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UIScale")),
        );
        if let Some(comp) = world.get::<UIScale>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("scale".into(), toml::Value::Float(comp.scale as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_scale() {
        assert_eq!(UIScaleSpawner.class_name(), ClassName::UIScale);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIScaleSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_scale() {
        let toml_src = r#"
            [metadata]
            class_name = "UIScale"
            name = "Zoom"
            [properties]
            scale = 1.5
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIScaleSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("scale"), Some(1.5));
    }
}
