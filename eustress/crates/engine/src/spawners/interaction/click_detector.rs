//! `ClickDetectorSpawner` — `ClassSpawner` for [`ClassName::ClickDetector`].
//!
//! Detects mouse clicks on its parent part. The spawner attaches the
//! [`ClickDetector`] config (`max_activation_distance` + `cursor_icon`);
//! the camera-raycast hit-testing + `MouseClick`/`MouseHoverEnter`/
//! `MouseHoverLeave` event firing lives in [`crate::interaction::click`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, ClickDetector, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::ClickDetector`].
#[derive(Default)]
pub struct ClickDetectorSpawner;

impl ClassSpawner for ClickDetectorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ClickDetector
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::ClickDetector, props);
        let name = instance.name.clone();
        let defaults = ClickDetector::default();

        let detector = ClickDetector {
            max_activation_distance: props
                .get_f32("max_activation_distance")
                .unwrap_or(defaults.max_activation_distance),
            cursor_icon: props
                .get_string("cursor_icon")
                .map(str::to_string)
                .unwrap_or(defaults.cursor_icon),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                detector,
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
        if let Ok(mut em) = world.get_entity_mut(entity) {
            if let Some(mut instance) = em.get_mut::<eustress_common::classes::Instance>() {
                if let Some(n) = props.get_string("metadata.name") {
                    instance.name = n.to_string();
                }
                if let Some(a) = props.get_bool("metadata.archivable") {
                    instance.archivable = a;
                }
            }
            if let Some(mut det) = em.get_mut::<ClickDetector>() {
                if let Some(v) = props.get_f32("max_activation_distance") {
                    det.max_activation_distance = v;
                }
                if let Some(v) = props.get_string("cursor_icon") {
                    det.cursor_icon = v.to_string();
                }
            }
            if let Some(n) = props.get_string("metadata.name") {
                if let Some(mut name) = em.get_mut::<Name>() {
                    name.set(n.to_string());
                }
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("MaxActivationDistance").and_then(|p| p.as_f32()) {
            bag.set("max_activation_distance", PropertyValue::Float(v));
        }
        if let Some(v) = rbx
            .property("CursorIcon")
            .and_then(|p| p.as_str().map(str::to_string))
        {
            bag.set("cursor_icon", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props
                .get("max_activation_distance")
                .and_then(|v| v.as_float())
            {
                bag.set("max_activation_distance", PropertyValue::Float(v as f32));
            }
            if let Some(v) = props.get("cursor_icon").and_then(|v| v.as_str()) {
                bag.set("cursor_icon", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "ClickDetector")),
        );
        if let Some(det) = world.get::<ClickDetector>(entity) {
            let mut props = toml::value::Table::new();
            props.insert(
                "max_activation_distance".into(),
                toml::Value::Float(det.max_activation_distance as f64),
            );
            props.insert("cursor_icon".into(), toml::Value::String(det.cursor_icon.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_click_detector() {
        assert_eq!(ClickDetectorSpawner.class_name(), ClassName::ClickDetector);
    }

    #[test]
    fn import_from_toml_reads_distance() {
        let toml_src = r#"
            [metadata]
            class_name = "ClickDetector"
            name = "Button"
            [properties]
            max_activation_distance = 16.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = ClickDetectorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("max_activation_distance"), Some(16.0));
    }
}
