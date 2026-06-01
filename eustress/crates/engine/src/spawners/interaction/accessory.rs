//! `AccessorySpawner` — `ClassSpawner` for [`ClassName::Accessory`].
//!
//! A legacy hat/accessory: a child `Handle` part positioned by an
//! `Attachment`. The spawner attaches the [`Accessory`] config component
//! (its `attachment_point` name); the runtime attach behavior lives in
//! [`crate::interaction::equip`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{Accessory, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::Accessory`].
#[derive(Default)]
pub struct AccessorySpawner;

impl ClassSpawner for AccessorySpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Accessory
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Accessory, props);
        let name = instance.name.clone();

        let accessory = Accessory {
            attachment_point: props
                .get_string("attachment_point")
                .map(str::to_string)
                .unwrap_or_default(),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                accessory,
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
            if let Some(v) = props.get_string("attachment_point") {
                if let Some(mut acc) = em.get_mut::<Accessory>() {
                    acc.attachment_point = v.to_string();
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
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        // Roblox Accessory's attachment point comes from the child
        // Attachment's name; the Wave 4 importer resolves that. The flat
        // `AttachmentPoint` property is read here when present.
        if let Some(v) = rbx
            .property("AttachmentPoint")
            .and_then(|p| p.as_str().map(str::to_string))
        {
            bag.set("attachment_point", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("attachment_point").and_then(|v| v.as_str()) {
                bag.set("attachment_point", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Accessory")),
        );
        if let Some(acc) = world.get::<Accessory>(entity) {
            let mut props = toml::value::Table::new();
            props.insert(
                "attachment_point".into(),
                toml::Value::String(acc.attachment_point.clone()),
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
    fn class_name_is_accessory() {
        assert_eq!(AccessorySpawner.class_name(), ClassName::Accessory);
    }

    #[test]
    fn import_from_toml_reads_attachment_point() {
        let toml_src = r#"
            [metadata]
            class_name = "Accessory"
            name = "Hat"
            [properties]
            attachment_point = "HatAttachment"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AccessorySpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("attachment_point"), Some("HatAttachment"));
    }
}
