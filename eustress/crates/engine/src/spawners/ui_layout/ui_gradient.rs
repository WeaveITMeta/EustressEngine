//! `UIGradientSpawner` — `ClassSpawner` for [`ClassName::UIGradient`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: color/transparency gradient overlay on the parent GuiObject.
//!
//! The keypoint lists (`color`, `transparency`) have no scalar `PropertyBag`
//! representation, so they are round-tripped losslessly through TOML by
//! `import_from_toml` / `export_to_toml` and default at `spawn`. The scalar
//! fields (`offset`, `rotation`, `enabled`) hydrate from the bag.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIGradient};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, float_array_to_toml, import_metadata, instance_from_bag, read_float_array};

/// Zero-sized spawner for [`ClassName::UIGradient`].
#[derive(Default)]
pub struct UIGradientSpawner;

impl ClassSpawner for UIGradientSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIGradient
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIGradient, props);
        let name = instance.name.clone();
        let defaults = UIGradient::default();
        let comp = UIGradient {
            color: defaults.color,
            offset: props.get_vec2("offset").unwrap_or(defaults.offset),
            rotation: props.get_f32("rotation").unwrap_or(defaults.rotation),
            transparency: defaults.transparency,
            enabled: props.get_bool("enabled").unwrap_or(defaults.enabled),
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
        if let Some(mut comp) = world.get_mut::<UIGradient>(entity) {
            if let Some(v) = props.get_vec2("offset") { comp.offset = v; }
            if let Some(v) = props.get_f32("rotation") { comp.rotation = v; }
            if let Some(v) = props.get_bool("enabled") { comp.enabled = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Rotation").and_then(|p| p.as_f32()) {
            bag.set("rotation", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("offset").and_then(|v| v.as_array()) {
                bag.set("offset", PropertyValue::Vector2(read_float_array::<2>(arr)));
            }
            if let Some(v) = props.get("rotation").and_then(|v| v.as_float()) {
                bag.set("rotation", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("enabled", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UIGradient")),
        );
        if let Some(comp) = world.get::<UIGradient>(entity) {
            let mut props = toml::value::Table::new();
            props.insert(
                "color".into(),
                toml::Value::Array(comp.color.iter().map(|c| float_array_to_toml(c)).collect()),
            );
            props.insert("offset".into(), float_array_to_toml(&comp.offset));
            props.insert("rotation".into(), toml::Value::Float(comp.rotation as f64));
            props.insert(
                "transparency".into(),
                float_array_to_toml(&comp.transparency),
            );
            props.insert("enabled".into(), toml::Value::Boolean(comp.enabled));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_gradient() {
        assert_eq!(UIGradientSpawner.class_name(), ClassName::UIGradient);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIGradientSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_scalars() {
        let toml_src = r#"
            [metadata]
            class_name = "UIGradient"
            name = "Fade"
            [properties]
            offset = [0.1, 0.2]
            rotation = 45.0
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIGradientSpawner.import_from_toml(&value);
        assert_eq!(bag.get_vec2("offset"), Some([0.1, 0.2]));
        assert_eq!(bag.get_f32("rotation"), Some(45.0));
        assert_eq!(bag.get_bool("enabled"), Some(false));
    }
}
