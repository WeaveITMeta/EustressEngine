//! `UIStrokeSpawner` — `ClassSpawner` for [`ClassName::UIStroke`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: outline stroke around the parent GuiObject.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIStroke};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIStroke`].
#[derive(Default)]
pub struct UIStrokeSpawner;

impl ClassSpawner for UIStrokeSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIStroke
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIStroke, props);
        let name = instance.name.clone();
        let defaults = UIStroke::default();
        let comp = UIStroke {
            color: props.get_color3("color").unwrap_or(defaults.color),
            thickness: props.get_f32("thickness").unwrap_or(defaults.thickness),
            transparency: props.get_f32("transparency").unwrap_or(defaults.transparency),
            apply_stroke_mode: props
                .get_string("apply_stroke_mode")
                .map(str::to_string)
                .unwrap_or(defaults.apply_stroke_mode),
            line_join_mode: props
                .get_string("line_join_mode")
                .map(str::to_string)
                .unwrap_or(defaults.line_join_mode),
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
        if let Some(mut comp) = world.get_mut::<UIStroke>(entity) {
            if let Some(v) = props.get_color3("color") { comp.color = v; }
            if let Some(v) = props.get_f32("thickness") { comp.thickness = v; }
            if let Some(v) = props.get_f32("transparency") { comp.transparency = v; }
            if let Some(v) = props.get_string("apply_stroke_mode") { comp.apply_stroke_mode = v.to_string(); }
            if let Some(v) = props.get_string("line_join_mode") { comp.line_join_mode = v.to_string(); }
            if let Some(v) = props.get_bool("enabled") { comp.enabled = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Thickness").and_then(|p| p.as_f32()) {
            bag.set("thickness", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("Transparency").and_then(|p| p.as_f32()) {
            bag.set("transparency", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("color").and_then(|v| v.as_array()) {
                let r = arr.first().and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
                let g = arr.get(1).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
                let b = arr.get(2).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
                bag.set("color", PropertyValue::Color3([r, g, b]));
            }
            if let Some(v) = props.get("thickness").and_then(|v| v.as_float()) {
                bag.set("thickness", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("transparency").and_then(|v| v.as_float()) {
                bag.set("transparency", PropertyValue::Float(v as f32));
            }
            for key in ["apply_stroke_mode", "line_join_mode"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
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
            toml::Value::Table(export_metadata(world, entity, "UIStroke")),
        );
        if let Some(comp) = world.get::<UIStroke>(entity) {
            let mut props = toml::value::Table::new();
            props.insert(
                "color".into(),
                toml::Value::Array(comp.color.iter().map(|c| toml::Value::Float(*c as f64)).collect()),
            );
            props.insert("thickness".into(), toml::Value::Float(comp.thickness as f64));
            props.insert("transparency".into(), toml::Value::Float(comp.transparency as f64));
            props.insert("apply_stroke_mode".into(), toml::Value::String(comp.apply_stroke_mode.clone()));
            props.insert("line_join_mode".into(), toml::Value::String(comp.line_join_mode.clone()));
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
    fn class_name_is_ui_stroke() {
        assert_eq!(UIStrokeSpawner.class_name(), ClassName::UIStroke);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIStrokeSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIStroke"
            name = "Outline"
            [properties]
            thickness = 3.0
            line_join_mode = "Bevel"
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIStrokeSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("thickness"), Some(3.0));
        assert_eq!(bag.get_string("line_join_mode"), Some("Bevel"));
        assert_eq!(bag.get_bool("enabled"), Some(false));
    }
}
