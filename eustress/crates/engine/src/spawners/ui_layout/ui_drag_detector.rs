//! `UIDragDetectorSpawner` — `ClassSpawner` for [`ClassName::UIDragDetector`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: makes the parent GuiObject draggable. `drag_axis` is a 2-element
//! float array carried in the bag as `Vector2`.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIDragDetector};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, float_array_to_toml, import_metadata, instance_from_bag, read_float_array};

/// Zero-sized spawner for [`ClassName::UIDragDetector`].
#[derive(Default)]
pub struct UIDragDetectorSpawner;

impl ClassSpawner for UIDragDetectorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIDragDetector
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIDragDetector, props);
        let name = instance.name.clone();
        let d = UIDragDetector::default();
        let comp = UIDragDetector {
            drag_style: props.get_string("drag_style").map(str::to_string).unwrap_or(d.drag_style),
            drag_axis: props.get_vec2("drag_axis").unwrap_or(d.drag_axis),
            enabled: props.get_bool("enabled").unwrap_or(d.enabled),
            responsiveness: props.get_f32("responsiveness").unwrap_or(d.responsiveness),
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
        if let Some(mut comp) = world.get_mut::<UIDragDetector>(entity) {
            if let Some(v) = props.get_string("drag_style") { comp.drag_style = v.to_string(); }
            if let Some(v) = props.get_vec2("drag_axis") { comp.drag_axis = v; }
            if let Some(v) = props.get_bool("enabled") { comp.enabled = v; }
            if let Some(v) = props.get_f32("responsiveness") { comp.responsiveness = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("DragStyle").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("drag_style", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        if let Some(v) = rbx.property("Responsiveness").and_then(|p| p.as_f32()) {
            bag.set("responsiveness", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("drag_style").and_then(|v| v.as_str()) {
                bag.set("drag_style", PropertyValue::String(v.to_string()));
            }
            if let Some(arr) = props.get("drag_axis").and_then(|v| v.as_array()) {
                bag.set("drag_axis", PropertyValue::Vector2(read_float_array::<2>(arr)));
            }
            if let Some(v) = props.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("enabled", PropertyValue::Bool(v));
            }
            if let Some(v) = props.get("responsiveness").and_then(|v| v.as_float()) {
                bag.set("responsiveness", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UIDragDetector")),
        );
        if let Some(comp) = world.get::<UIDragDetector>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("drag_style".into(), toml::Value::String(comp.drag_style.clone()));
            props.insert("drag_axis".into(), float_array_to_toml(&comp.drag_axis));
            props.insert("enabled".into(), toml::Value::Boolean(comp.enabled));
            props.insert("responsiveness".into(), toml::Value::Float(comp.responsiveness as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_drag_detector() {
        assert_eq!(UIDragDetectorSpawner.class_name(), ClassName::UIDragDetector);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIDragDetectorSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIDragDetector"
            name = "Draggable"
            [properties]
            drag_style = "TranslateLine"
            drag_axis = [1.0, 0.0]
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIDragDetectorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("drag_style"), Some("TranslateLine"));
        assert_eq!(bag.get_vec2("drag_axis"), Some([1.0, 0.0]));
        assert_eq!(bag.get_bool("enabled"), Some(false));
    }
}
