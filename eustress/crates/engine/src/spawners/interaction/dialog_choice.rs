//! `DialogChoiceSpawner` — `ClassSpawner` for [`ClassName::DialogChoice`].
//!
//! A single branch within a [`Dialog`] tree. The spawner attaches the
//! [`DialogChoice`] config (`user_dialog` + `response_dialog`); the
//! conversation panel that renders these as buttons lives in
//! [`crate::interaction::dialog_ui`].
//!
//! [`Dialog`]: eustress_common::classes::Dialog

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, DialogChoice, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::DialogChoice`].
#[derive(Default)]
pub struct DialogChoiceSpawner;

impl ClassSpawner for DialogChoiceSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::DialogChoice
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::DialogChoice, props);
        let name = instance.name.clone();

        let choice = DialogChoice {
            user_dialog: props.get_string("user_dialog").map(str::to_string).unwrap_or_default(),
            response_dialog: props
                .get_string("response_dialog")
                .map(str::to_string)
                .unwrap_or_default(),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                choice,
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
            if let Some(mut choice) = em.get_mut::<DialogChoice>() {
                if let Some(v) = props.get_string("user_dialog") { choice.user_dialog = v.to_string(); }
                if let Some(v) = props.get_string("response_dialog") { choice.response_dialog = v.to_string(); }
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
        if let Some(v) = rbx.property("UserDialog").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("user_dialog", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("ResponseDialog").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("response_dialog", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["user_dialog", "response_dialog"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "DialogChoice")),
        );
        if let Some(c) = world.get::<DialogChoice>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("user_dialog".into(), toml::Value::String(c.user_dialog.clone()));
            props.insert("response_dialog".into(), toml::Value::String(c.response_dialog.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_dialog_choice() {
        assert_eq!(DialogChoiceSpawner.class_name(), ClassName::DialogChoice);
    }

    #[test]
    fn import_from_toml_reads_user_and_response() {
        let toml_src = r#"
            [metadata]
            class_name = "DialogChoice"
            name = "Choice1"
            [properties]
            user_dialog = "How much?"
            response_dialog = "Ten gold."
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = DialogChoiceSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("user_dialog"), Some("How much?"));
        assert_eq!(bag.get_string("response_dialog"), Some("Ten gold."));
    }
}
