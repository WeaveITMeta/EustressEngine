//! `DialogSpawner` — `ClassSpawner` for [`ClassName::Dialog`].
//!
//! An NPC dialog-tree root anchored to its parent. The spawner attaches the
//! [`Dialog`] config; the conversation panel UI (initial prompt + child
//! `DialogChoice` buttons) lives in [`crate::interaction::dialog_ui`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Dialog, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::Dialog`].
#[derive(Default)]
pub struct DialogSpawner;

impl ClassSpawner for DialogSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Dialog
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Dialog, props);
        let name = instance.name.clone();
        let defaults = Dialog::default();

        let dialog = Dialog {
            initial_prompt: props
                .get_string("initial_prompt")
                .map(str::to_string)
                .unwrap_or(defaults.initial_prompt),
            purpose: props
                .get_string("purpose")
                .map(str::to_string)
                .unwrap_or(defaults.purpose),
            tone: props.get_string("tone").map(str::to_string).unwrap_or(defaults.tone),
            conversation_distance: props
                .get_f32("conversation_distance")
                .unwrap_or(defaults.conversation_distance),
            goodbye_dialog: props
                .get_string("goodbye_dialog")
                .map(str::to_string)
                .unwrap_or(defaults.goodbye_dialog),
            // Runtime flag — never seeded from disk.
            in_use: false,
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                dialog,
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
            if let Some(mut dialog) = em.get_mut::<Dialog>() {
                if let Some(v) = props.get_string("initial_prompt") { dialog.initial_prompt = v.to_string(); }
                if let Some(v) = props.get_string("purpose") { dialog.purpose = v.to_string(); }
                if let Some(v) = props.get_string("tone") { dialog.tone = v.to_string(); }
                if let Some(v) = props.get_f32("conversation_distance") { dialog.conversation_distance = v; }
                if let Some(v) = props.get_string("goodbye_dialog") { dialog.goodbye_dialog = v.to_string(); }
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
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("InitialPrompt").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("initial_prompt", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Purpose").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("purpose", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Tone").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("tone", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("ConversationDistance").and_then(|p| p.as_f32()) {
            bag.set("conversation_distance", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("GoodbyeDialog").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("goodbye_dialog", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["initial_prompt", "purpose", "tone", "goodbye_dialog"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            if let Some(v) = props.get("conversation_distance").and_then(|v| v.as_float()) {
                bag.set("conversation_distance", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Dialog")),
        );
        if let Some(d) = world.get::<Dialog>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("initial_prompt".into(), toml::Value::String(d.initial_prompt.clone()));
            props.insert("purpose".into(), toml::Value::String(d.purpose.clone()));
            props.insert("tone".into(), toml::Value::String(d.tone.clone()));
            props.insert(
                "conversation_distance".into(),
                toml::Value::Float(d.conversation_distance as f64),
            );
            props.insert("goodbye_dialog".into(), toml::Value::String(d.goodbye_dialog.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_dialog() {
        assert_eq!(DialogSpawner.class_name(), ClassName::Dialog);
    }

    #[test]
    fn import_from_toml_reads_prompt_and_distance() {
        let toml_src = r#"
            [metadata]
            class_name = "Dialog"
            name = "Shopkeeper"
            [properties]
            initial_prompt = "Welcome!"
            purpose = "Shop"
            conversation_distance = 8.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = DialogSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("initial_prompt"), Some("Welcome!"));
        assert_eq!(bag.get_string("purpose"), Some("Shop"));
        assert_eq!(bag.get_f32("conversation_distance"), Some(8.0));
    }
}
