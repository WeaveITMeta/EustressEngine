//! `TextChatCommandSpawner` — `ClassSpawner` for [`ClassName::TextChatCommand`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.G). Data-attach config
//! carrier: registers a slash-command (`primary_alias` / `secondary_alias`) on
//! the text-chat system. See the group [`mod`](super) docs. The command
//! dispatch system is a later phase; the config round-trips here.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, TextChatCommand};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::TextChatCommand`].
#[derive(Default)]
pub struct TextChatCommandSpawner;

impl ClassSpawner for TextChatCommandSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::TextChatCommand
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::TextChatCommand, props);
        let name = instance.name.clone();
        let d = TextChatCommand::default();
        let comp = TextChatCommand {
            primary_alias: props.get_string("primary_alias").map(str::to_string).unwrap_or(d.primary_alias),
            secondary_alias: props.get_string("secondary_alias").map(str::to_string).unwrap_or(d.secondary_alias),
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
        if let Some(mut comp) = world.get_mut::<TextChatCommand>(entity) {
            if let Some(v) = props.get_string("primary_alias") { comp.primary_alias = v.to_string(); }
            if let Some(v) = props.get_string("secondary_alias") { comp.secondary_alias = v.to_string(); }
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
        if let Some(v) = rbx.property("PrimaryAlias").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("primary_alias", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("SecondaryAlias").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("secondary_alias", PropertyValue::String(v));
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
            if let Some(v) = props.get("primary_alias").and_then(|v| v.as_str()) {
                bag.set("primary_alias", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("secondary_alias").and_then(|v| v.as_str()) {
                bag.set("secondary_alias", PropertyValue::String(v.to_string()));
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
            toml::Value::Table(export_metadata(world, entity, "TextChatCommand")),
        );
        if let Some(comp) = world.get::<TextChatCommand>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("primary_alias".into(), toml::Value::String(comp.primary_alias.clone()));
            props.insert("secondary_alias".into(), toml::Value::String(comp.secondary_alias.clone()));
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
    fn class_name_is_text_chat_command() {
        assert_eq!(TextChatCommandSpawner.class_name(), ClassName::TextChatCommand);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(TextChatCommandSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_aliases() {
        let toml_src = r#"
            [metadata]
            class_name = "TextChatCommand"
            name = "Help"
            [properties]
            primary_alias = "/help"
            secondary_alias = "/h"
            enabled = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = TextChatCommandSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("primary_alias"), Some("/help"));
        assert_eq!(bag.get_string("secondary_alias"), Some("/h"));
        assert_eq!(bag.get_bool("enabled"), Some(true));
    }
}
