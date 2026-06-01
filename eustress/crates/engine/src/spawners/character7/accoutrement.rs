//! `AccoutrementSpawner` — `ClassSpawner` for [`ClassName::Accoutrement`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! legacy attached cosmetic. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{Accoutrement, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag, read_vec3_array, vec3_to_toml};

/// Zero-sized spawner for [`ClassName::Accoutrement`].
#[derive(Default)]
pub struct AccoutrementSpawner;

impl ClassSpawner for AccoutrementSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Accoutrement
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Accoutrement, props);
        let name = instance.name.clone();
        let comp = Accoutrement {
            attachment_point: props.get_vec3("attachment_point").unwrap_or(Accoutrement::default().attachment_point),
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
        if let Some(v) = props.get_vec3("attachment_point") {
            if let Some(mut comp) = world.get_mut::<Accoutrement>(entity) {
                comp.attachment_point = v;
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(1);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("attachment_point").and_then(|v| v.as_array()) {
                bag.set("attachment_point", PropertyValue::Vector3(read_vec3_array(arr)));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Accoutrement")),
        );
        if let Some(comp) = world.get::<Accoutrement>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("attachment_point".into(), vec3_to_toml(comp.attachment_point));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_accoutrement() {
        assert_eq!(AccoutrementSpawner.class_name(), ClassName::Accoutrement);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AccoutrementSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_point() {
        let toml_src = r#"
            [metadata]
            class_name = "Accoutrement"
            name = "Hat"
            [properties]
            attachment_point = [0.0, 1.0, 0.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AccoutrementSpawner.import_from_toml(&value);
        assert_eq!(bag.get_vec3("attachment_point"), Some(Vec3::new(0.0, 1.0, 0.0)));
    }
}
