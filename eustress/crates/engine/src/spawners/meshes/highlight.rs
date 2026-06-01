//! `HighlightSpawner` — `ClassSpawner` for [`ClassName::Highlight`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! fill/outline highlight overlay on a part or model.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Highlight, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, color3_to_toml, export_metadata, import_metadata, instance_from_bag, read_color3};

/// Zero-sized spawner for [`ClassName::Highlight`].
#[derive(Default)]
pub struct HighlightSpawner;

impl ClassSpawner for HighlightSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Highlight
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Highlight, props);
        let name = instance.name.clone();
        let d = Highlight::default();
        let comp = Highlight {
            fill_color: props.get_color3("fill_color").unwrap_or(d.fill_color),
            fill_transparency: props.get_f32("fill_transparency").unwrap_or(d.fill_transparency),
            outline_color: props.get_color3("outline_color").unwrap_or(d.outline_color),
            outline_transparency: props.get_f32("outline_transparency").unwrap_or(d.outline_transparency),
            depth_mode: props.get_string("depth_mode").map(str::to_string).unwrap_or(d.depth_mode),
            enabled: props.get_bool("enabled").unwrap_or(d.enabled),
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
        if let Some(mut comp) = world.get_mut::<Highlight>(entity) {
            if let Some(v) = props.get_color3("fill_color") { comp.fill_color = v; }
            if let Some(v) = props.get_f32("fill_transparency") { comp.fill_transparency = v; }
            if let Some(v) = props.get_color3("outline_color") { comp.outline_color = v; }
            if let Some(v) = props.get_f32("outline_transparency") { comp.outline_transparency = v; }
            if let Some(v) = props.get_string("depth_mode") { comp.depth_mode = v.to_string(); }
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
        for (rbx_key, key) in [
            ("FillTransparency", "fill_transparency"),
            ("OutlineTransparency", "outline_transparency"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        if let Some(v) = rbx.property("DepthMode").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("depth_mode", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(7);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("fill_color").and_then(|v| v.as_array()) {
                bag.set("fill_color", PropertyValue::Color3(read_color3(arr)));
            }
            if let Some(arr) = props.get("outline_color").and_then(|v| v.as_array()) {
                bag.set("outline_color", PropertyValue::Color3(read_color3(arr)));
            }
            for key in ["fill_transparency", "outline_transparency"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            if let Some(v) = props.get("depth_mode").and_then(|v| v.as_str()) {
                bag.set("depth_mode", PropertyValue::String(v.to_string()));
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
            toml::Value::Table(export_metadata(world, entity, "Highlight")),
        );
        if let Some(comp) = world.get::<Highlight>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("fill_color".into(), color3_to_toml(comp.fill_color));
            props.insert("fill_transparency".into(), toml::Value::Float(comp.fill_transparency as f64));
            props.insert("outline_color".into(), color3_to_toml(comp.outline_color));
            props.insert("outline_transparency".into(), toml::Value::Float(comp.outline_transparency as f64));
            props.insert("depth_mode".into(), toml::Value::String(comp.depth_mode.clone()));
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
    fn class_name_is_highlight() {
        assert_eq!(HighlightSpawner.class_name(), ClassName::Highlight);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(HighlightSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "Highlight"
            name = "Glow"
            [properties]
            fill_transparency = 0.3
            depth_mode = "Occluded"
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = HighlightSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("fill_transparency"), Some(0.3));
        assert_eq!(bag.get_string("depth_mode"), Some("Occluded"));
        assert_eq!(bag.get_bool("enabled"), Some(false));
    }
}
