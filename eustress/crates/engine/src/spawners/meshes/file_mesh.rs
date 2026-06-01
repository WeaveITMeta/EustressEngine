//! `FileMeshSpawner` — `ClassSpawner` for [`ClassName::FileMesh`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! legacy file-backed mesh shape modifier with a texture.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, FileMesh, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, color3_to_toml, export_metadata, import_metadata, instance_from_bag, read_color3, read_vec3_array, vec3_to_toml};

/// Zero-sized spawner for [`ClassName::FileMesh`].
#[derive(Default)]
pub struct FileMeshSpawner;

impl ClassSpawner for FileMeshSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::FileMesh
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::FileMesh, props);
        let name = instance.name.clone();
        let d = FileMesh::default();
        let comp = FileMesh {
            mesh_id: props.get_string("mesh_id").map(str::to_string).unwrap_or(d.mesh_id),
            texture_id: props.get_string("texture_id").map(str::to_string).unwrap_or(d.texture_id),
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
        if let Some(mut comp) = world.get_mut::<FileMesh>(entity) {
            if let Some(v) = props.get_string("mesh_id") { comp.mesh_id = v.to_string(); }
            if let Some(v) = props.get_string("texture_id") { comp.texture_id = v.to_string(); }
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
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("MeshId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("mesh_id", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("TextureId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("texture_id", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["mesh_id", "texture_id"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
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
            toml::Value::Table(export_metadata(world, entity, "FileMesh")),
        );
        if let Some(comp) = world.get::<FileMesh>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("mesh_id".into(), toml::Value::String(comp.mesh_id.clone()));
            props.insert("texture_id".into(), toml::Value::String(comp.texture_id.clone()));
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
    fn class_name_is_file_mesh() {
        assert_eq!(FileMeshSpawner.class_name(), ClassName::FileMesh);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(FileMeshSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "FileMesh"
            name = "Mesh"
            [properties]
            mesh_id = "rbxassetid://123"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = FileMeshSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("mesh_id"), Some("rbxassetid://123"));
    }
}
