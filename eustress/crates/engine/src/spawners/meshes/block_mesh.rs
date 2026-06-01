//! `BlockMeshSpawner` — `ClassSpawner` for [`ClassName::BlockMesh`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! legacy block-mesh shape modifier. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BlockMesh, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, color3_to_toml, export_metadata, import_metadata, instance_from_bag, read_color3, read_vec3_array, vec3_to_toml};

/// Zero-sized spawner for [`ClassName::BlockMesh`].
#[derive(Default)]
pub struct BlockMeshSpawner;

impl ClassSpawner for BlockMeshSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BlockMesh
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::BlockMesh, props);
        let name = instance.name.clone();
        let d = BlockMesh::default();
        let comp = BlockMesh {
            offset: props.get_vec3("offset").unwrap_or(d.offset),
            scale: props.get_vec3("scale").unwrap_or(d.scale),
            vertex_color: props.get_color3("vertex_color").unwrap_or(d.vertex_color),
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
        if let Some(mut comp) = world.get_mut::<BlockMesh>(entity) {
            if let Some(v) = props.get_vec3("offset") { comp.offset = v; }
            if let Some(v) = props.get_vec3("scale") { comp.scale = v; }
            if let Some(v) = props.get_color3("vertex_color") { comp.vertex_color = v; }
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
        let mut bag = PropertyBag::with_capacity(4);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("offset").and_then(|v| v.as_array()) {
                bag.set("offset", PropertyValue::Vector3(read_vec3_array(arr)));
            }
            if let Some(arr) = props.get("scale").and_then(|v| v.as_array()) {
                bag.set("scale", PropertyValue::Vector3(read_vec3_array(arr)));
            }
            if let Some(arr) = props.get("vertex_color").and_then(|v| v.as_array()) {
                bag.set("vertex_color", PropertyValue::Color3(read_color3(arr)));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "BlockMesh")),
        );
        if let Some(comp) = world.get::<BlockMesh>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("offset".into(), vec3_to_toml(comp.offset));
            props.insert("scale".into(), vec3_to_toml(comp.scale));
            props.insert("vertex_color".into(), color3_to_toml(comp.vertex_color));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_block_mesh() {
        assert_eq!(BlockMeshSpawner.class_name(), ClassName::BlockMesh);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(BlockMeshSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "BlockMesh"
            name = "Block"
            [properties]
            scale = [2.0, 2.0, 2.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BlockMeshSpawner.import_from_toml(&value);
        assert_eq!(bag.get_vec3("scale"), Some(Vec3::splat(2.0)));
    }
}
