//! `BodyPartDescriptionSpawner` — `ClassSpawner` for
//! [`ClassName::BodyPartDescription`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! single body-part asset entry. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BodyPartDescription, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::BodyPartDescription`].
#[derive(Default)]
pub struct BodyPartDescriptionSpawner;

impl ClassSpawner for BodyPartDescriptionSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BodyPartDescription
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::BodyPartDescription, props);
        let name = instance.name.clone();
        let d = BodyPartDescription::default();
        let comp = BodyPartDescription {
            body_part: props.get_string("body_part").map(str::to_string).unwrap_or(d.body_part),
            asset_id: props.get_string("asset_id").map(str::to_string).unwrap_or(d.asset_id),
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
        if let Some(mut comp) = world.get_mut::<BodyPartDescription>(entity) {
            if let Some(v) = props.get_string("body_part") { comp.body_part = v.to_string(); }
            if let Some(v) = props.get_string("asset_id") { comp.asset_id = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("BodyPart").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("body_part", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("AssetId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("asset_id", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["body_part", "asset_id"] {
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
            toml::Value::Table(export_metadata(world, entity, "BodyPartDescription")),
        );
        if let Some(comp) = world.get::<BodyPartDescription>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("body_part".into(), toml::Value::String(comp.body_part.clone()));
            props.insert("asset_id".into(), toml::Value::String(comp.asset_id.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_body_part_description() {
        assert_eq!(BodyPartDescriptionSpawner.class_name(), ClassName::BodyPartDescription);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(BodyPartDescriptionSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "BodyPartDescription"
            name = "Head"
            [properties]
            body_part = "Head"
            asset_id = "rbxassetid://3"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BodyPartDescriptionSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("body_part"), Some("Head"));
        assert_eq!(bag.get_string("asset_id"), Some("rbxassetid://3"));
    }
}
