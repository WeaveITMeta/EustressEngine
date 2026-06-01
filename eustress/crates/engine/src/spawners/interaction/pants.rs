//! `PantsSpawner` — `ClassSpawner` for [`ClassName::Pants`].
//!
//! Character pants (clothing template). The spawner attaches the [`Pants`]
//! config (`pants_template` asset ID); the legs texture application lives in
//! [`crate::interaction::appearance`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Pants, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::Pants`].
#[derive(Default)]
pub struct PantsSpawner;

impl ClassSpawner for PantsSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Pants
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Pants, props);
        let name = instance.name.clone();

        let pants = Pants {
            pants_template: props
                .get_string("pants_template")
                .map(str::to_string)
                .unwrap_or_default(),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                pants,
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
            if let Some(v) = props.get_string("pants_template") {
                if let Some(mut pants) = em.get_mut::<Pants>() {
                    pants.pants_template = v.to_string();
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
        if let Some(v) = rbx.property("PantsTemplate").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("pants_template", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("pants_template").and_then(|v| v.as_str()) {
                bag.set("pants_template", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Pants")),
        );
        if let Some(p) = world.get::<Pants>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("pants_template".into(), toml::Value::String(p.pants_template.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_pants() {
        assert_eq!(PantsSpawner.class_name(), ClassName::Pants);
    }

    #[test]
    fn import_from_toml_reads_template() {
        let toml_src = r#"
            [metadata]
            class_name = "Pants"
            name = "BluePants"
            [properties]
            pants_template = "rbxassetid://888"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = PantsSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("pants_template"), Some("rbxassetid://888"));
    }
}
