//! `DragDetectorSpawner` — `ClassSpawner` for [`ClassName::DragDetector`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.G). Data-attach
//! interaction: makes the parent part draggable under the named `drag_style`
//! (e.g. `TranslateLine`/`Rotate`). See the group [`mod`](super) docs. The
//! actual drag-handling system is a later phase; the config round-trips here.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, DragDetector, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::DragDetector`].
#[derive(Default)]
pub struct DragDetectorSpawner;

impl ClassSpawner for DragDetectorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::DragDetector
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::DragDetector, props);
        let name = instance.name.clone();
        let d = DragDetector::default();
        let comp = DragDetector {
            drag_style: props.get_string("drag_style").map(str::to_string).unwrap_or(d.drag_style),
            responsiveness: props.get_f32("responsiveness").unwrap_or(d.responsiveness),
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
        if let Some(mut comp) = world.get_mut::<DragDetector>(entity) {
            if let Some(v) = props.get_string("drag_style") { comp.drag_style = v.to_string(); }
            if let Some(v) = props.get_f32("responsiveness") { comp.responsiveness = v; }
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
        if let Some(v) = rbx.property("DragStyle").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("drag_style", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Responsiveness").and_then(|p| p.as_f32()) {
            bag.set("responsiveness", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("drag_style").and_then(|v| v.as_str()) {
                bag.set("drag_style", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("responsiveness").and_then(|v| v.as_float()) {
                bag.set("responsiveness", PropertyValue::Float(v as f32));
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
            toml::Value::Table(export_metadata(world, entity, "DragDetector")),
        );
        if let Some(comp) = world.get::<DragDetector>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("drag_style".into(), toml::Value::String(comp.drag_style.clone()));
            props.insert("responsiveness".into(), toml::Value::Float(comp.responsiveness as f64));
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
    fn class_name_is_drag_detector() {
        assert_eq!(DragDetectorSpawner.class_name(), ClassName::DragDetector);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(DragDetectorSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "DragDetector"
            name = "Handle"
            [properties]
            drag_style = "Rotate"
            responsiveness = 12.5
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = DragDetectorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("drag_style"), Some("Rotate"));
        assert_eq!(bag.get_f32("responsiveness"), Some(12.5));
        assert_eq!(bag.get_bool("enabled"), Some(false));
    }
}
