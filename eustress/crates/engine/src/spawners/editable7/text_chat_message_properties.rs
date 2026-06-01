//! `TextChatMessagePropertiesSpawner` — `ClassSpawner` for
//! [`ClassName::TextChatMessageProperties`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.G). Data-attach config
//! carrier: overrides the `prefix_text` / `text` rendered for a chat message.
//! See the group [`mod`](super) docs. The chat-render system is a later phase;
//! the config round-trips here.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, TextChatMessageProperties};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::TextChatMessageProperties`].
#[derive(Default)]
pub struct TextChatMessagePropertiesSpawner;

impl ClassSpawner for TextChatMessagePropertiesSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::TextChatMessageProperties
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::TextChatMessageProperties, props);
        let name = instance.name.clone();
        let d = TextChatMessageProperties::default();
        let comp = TextChatMessageProperties {
            prefix_text: props.get_string("prefix_text").map(str::to_string).unwrap_or(d.prefix_text),
            text: props.get_string("text").map(str::to_string).unwrap_or(d.text),
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
        if let Some(mut comp) = world.get_mut::<TextChatMessageProperties>(entity) {
            if let Some(v) = props.get_string("prefix_text") { comp.prefix_text = v.to_string(); }
            if let Some(v) = props.get_string("text") { comp.text = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("PrefixText").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("prefix_text", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Text").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("text", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("prefix_text").and_then(|v| v.as_str()) {
                bag.set("prefix_text", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("text").and_then(|v| v.as_str()) {
                bag.set("text", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "TextChatMessageProperties")),
        );
        if let Some(comp) = world.get::<TextChatMessageProperties>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("prefix_text".into(), toml::Value::String(comp.prefix_text.clone()));
            props.insert("text".into(), toml::Value::String(comp.text.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_text_chat_message_properties() {
        assert_eq!(
            TextChatMessagePropertiesSpawner.class_name(),
            ClassName::TextChatMessageProperties
        );
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(TextChatMessagePropertiesSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_text() {
        let toml_src = r#"
            [metadata]
            class_name = "TextChatMessageProperties"
            name = "Styled"
            [properties]
            prefix_text = "[ADMIN]"
            text = "hello world"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = TextChatMessagePropertiesSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("prefix_text"), Some("[ADMIN]"));
        assert_eq!(bag.get_string("text"), Some("hello world"));
    }
}
