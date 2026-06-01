//! `UIAspectRatioConstraintSpawner` — `ClassSpawner` for
//! [`ClassName::UIAspectRatioConstraint`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: locks a GuiObject's aspect ratio.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIAspectRatioConstraint};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIAspectRatioConstraint`].
#[derive(Default)]
pub struct UIAspectRatioConstraintSpawner;

impl ClassSpawner for UIAspectRatioConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIAspectRatioConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIAspectRatioConstraint, props);
        let name = instance.name.clone();
        let d = UIAspectRatioConstraint::default();
        let comp = UIAspectRatioConstraint {
            aspect_ratio: props.get_f32("aspect_ratio").unwrap_or(d.aspect_ratio),
            aspect_type: props.get_string("aspect_type").map(str::to_string).unwrap_or(d.aspect_type),
            dominant_axis: props.get_string("dominant_axis").map(str::to_string).unwrap_or(d.dominant_axis),
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
        if let Some(mut comp) = world.get_mut::<UIAspectRatioConstraint>(entity) {
            if let Some(v) = props.get_f32("aspect_ratio") { comp.aspect_ratio = v; }
            if let Some(v) = props.get_string("aspect_type") { comp.aspect_type = v.to_string(); }
            if let Some(v) = props.get_string("dominant_axis") { comp.dominant_axis = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("AspectRatio").and_then(|p| p.as_f32()) {
            bag.set("aspect_ratio", PropertyValue::Float(v));
        }
        for (rbx_key, key) in [("AspectType", "aspect_type"), ("DominantAxis", "dominant_axis")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("aspect_ratio").and_then(|v| v.as_float()) {
                bag.set("aspect_ratio", PropertyValue::Float(v as f32));
            }
            for key in ["aspect_type", "dominant_axis"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UIAspectRatioConstraint")),
        );
        if let Some(comp) = world.get::<UIAspectRatioConstraint>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("aspect_ratio".into(), toml::Value::Float(comp.aspect_ratio as f64));
            props.insert("aspect_type".into(), toml::Value::String(comp.aspect_type.clone()));
            props.insert("dominant_axis".into(), toml::Value::String(comp.dominant_axis.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_aspect_ratio_constraint() {
        assert_eq!(UIAspectRatioConstraintSpawner.class_name(), ClassName::UIAspectRatioConstraint);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIAspectRatioConstraintSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIAspectRatioConstraint"
            name = "Ratio"
            [properties]
            aspect_ratio = 1.777
            dominant_axis = "Height"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIAspectRatioConstraintSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("aspect_ratio"), Some(1.777));
        assert_eq!(bag.get_string("dominant_axis"), Some("Height"));
    }
}
