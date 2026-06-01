//! `ClimbControllerSpawner` — `ClassSpawner` for [`ClassName::ClimbController`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7). Pure marker — the
//! [`ClimbController`](eustress_common::classes::ClimbController) component has no authored
//! fields; the spawner attaches it + the cross-cutting `Instance`/`Name`. The
//! runtime behavior is a later phase.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, ClimbController};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::ClimbController`].
#[derive(Default)]
pub struct ClimbControllerSpawner;

impl ClassSpawner for ClimbControllerSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ClimbController
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::ClimbController, props);
        let name = instance.name.clone();

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                ClimbController::default(),
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
        let mut bag = PropertyBag::with_capacity(1);
        import_metadata(toml_value, &mut bag);
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "ClimbController")),
        );
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_matches() {
        assert_eq!(ClimbControllerSpawner.class_name(), ClassName::ClimbController);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(ClimbControllerSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_name() {
        let toml_src = "[metadata]\nclass_name = \"ClimbController\"\nname = \"X\"\n";
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = ClimbControllerSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("metadata.name"), Some("X"));
    }
}
