//! `ProximityPromptSpawner` — `ClassSpawner` for [`ClassName::ProximityPrompt`].
//!
//! A contextual hold-to-interact prompt anchored to its parent. The spawner
//! attaches the [`ProximityPrompt`] config; the per-frame distance +
//! line-of-sight check, prompt overlay, and `Triggered` event firing live in
//! [`crate::interaction::proximity`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, ProximityPrompt};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::ProximityPrompt`].
#[derive(Default)]
pub struct ProximityPromptSpawner;

impl ClassSpawner for ProximityPromptSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ProximityPrompt
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::ProximityPrompt, props);
        let name = instance.name.clone();
        let defaults = ProximityPrompt::default();

        let prompt = ProximityPrompt {
            action_text: props
                .get_string("action_text")
                .map(str::to_string)
                .unwrap_or(defaults.action_text),
            object_text: props
                .get_string("object_text")
                .map(str::to_string)
                .unwrap_or(defaults.object_text),
            hold_duration: props.get_f32("hold_duration").unwrap_or(defaults.hold_duration),
            max_activation_distance: props
                .get_f32("max_activation_distance")
                .unwrap_or(defaults.max_activation_distance),
            requires_line_of_sight: props
                .get_bool("requires_line_of_sight")
                .unwrap_or(defaults.requires_line_of_sight),
            exclusivity: props
                .get_string("exclusivity")
                .map(str::to_string)
                .unwrap_or(defaults.exclusivity),
            keyboard_key_code: props
                .get_string("keyboard_key_code")
                .map(str::to_string)
                .unwrap_or(defaults.keyboard_key_code),
            gamepad_key_code: props
                .get_string("gamepad_key_code")
                .map(str::to_string)
                .unwrap_or(defaults.gamepad_key_code),
            ui_offset: props
                .get_vec2("ui_offset")
                .map(Vec2::from)
                .unwrap_or(defaults.ui_offset),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                prompt,
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
            if let Some(mut prompt) = em.get_mut::<ProximityPrompt>() {
                if let Some(v) = props.get_string("action_text") { prompt.action_text = v.to_string(); }
                if let Some(v) = props.get_string("object_text") { prompt.object_text = v.to_string(); }
                if let Some(v) = props.get_f32("hold_duration") { prompt.hold_duration = v; }
                if let Some(v) = props.get_f32("max_activation_distance") { prompt.max_activation_distance = v; }
                if let Some(v) = props.get_bool("requires_line_of_sight") { prompt.requires_line_of_sight = v; }
                if let Some(v) = props.get_string("exclusivity") { prompt.exclusivity = v.to_string(); }
                if let Some(v) = props.get_string("keyboard_key_code") { prompt.keyboard_key_code = v.to_string(); }
                if let Some(v) = props.get_string("gamepad_key_code") { prompt.gamepad_key_code = v.to_string(); }
                if let Some(v) = props.get_vec2("ui_offset") { prompt.ui_offset = Vec2::from(v); }
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
        let mut bag = PropertyBag::with_capacity(8);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("ActionText").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("action_text", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("ObjectText").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("object_text", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("HoldDuration").and_then(|p| p.as_f32()) {
            bag.set("hold_duration", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("MaxActivationDistance").and_then(|p| p.as_f32()) {
            bag.set("max_activation_distance", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("RequiresLineOfSight").and_then(|p| p.as_bool()) {
            bag.set("requires_line_of_sight", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(12);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["action_text", "object_text", "exclusivity", "keyboard_key_code", "gamepad_key_code"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            for key in ["hold_duration", "max_activation_distance"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            if let Some(v) = props.get("requires_line_of_sight").and_then(|v| v.as_bool()) {
                bag.set("requires_line_of_sight", PropertyValue::Bool(v));
            }
            if let Some(arr) = props.get("ui_offset").and_then(|v| v.as_array()) {
                let x = arr.first().and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
                let y = arr.get(1).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
                bag.set("ui_offset", PropertyValue::Vector2([x, y]));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "ProximityPrompt")),
        );
        if let Some(p) = world.get::<ProximityPrompt>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("action_text".into(), toml::Value::String(p.action_text.clone()));
            props.insert("object_text".into(), toml::Value::String(p.object_text.clone()));
            props.insert("hold_duration".into(), toml::Value::Float(p.hold_duration as f64));
            props.insert(
                "max_activation_distance".into(),
                toml::Value::Float(p.max_activation_distance as f64),
            );
            props.insert(
                "requires_line_of_sight".into(),
                toml::Value::Boolean(p.requires_line_of_sight),
            );
            props.insert("exclusivity".into(), toml::Value::String(p.exclusivity.clone()));
            props.insert(
                "keyboard_key_code".into(),
                toml::Value::String(p.keyboard_key_code.clone()),
            );
            props.insert(
                "gamepad_key_code".into(),
                toml::Value::String(p.gamepad_key_code.clone()),
            );
            props.insert(
                "ui_offset".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(p.ui_offset.x as f64),
                    toml::Value::Float(p.ui_offset.y as f64),
                ]),
            );
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_proximity_prompt() {
        assert_eq!(ProximityPromptSpawner.class_name(), ClassName::ProximityPrompt);
    }

    #[test]
    fn import_from_toml_reads_action_and_hold() {
        let toml_src = r#"
            [metadata]
            class_name = "ProximityPrompt"
            name = "OpenDoor"
            [properties]
            action_text = "Open"
            object_text = "Door"
            hold_duration = 0.5
            requires_line_of_sight = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = ProximityPromptSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("action_text"), Some("Open"));
        assert_eq!(bag.get_string("object_text"), Some("Door"));
        assert_eq!(bag.get_f32("hold_duration"), Some(0.5));
        assert_eq!(bag.get_bool("requires_line_of_sight"), Some(false));
    }
}
