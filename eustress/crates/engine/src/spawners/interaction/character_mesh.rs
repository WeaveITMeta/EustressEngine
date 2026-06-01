//! `CharacterMeshSpawner` — `ClassSpawner` for [`ClassName::CharacterMesh`].
//!
//! Replacement mesh + textures for one character body part. The spawner
//! attaches the [`CharacterMesh`] config; the limb mesh/texture override
//! lives in [`crate::interaction::appearance`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{CharacterMesh, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::CharacterMesh`].
#[derive(Default)]
pub struct CharacterMeshSpawner;

impl ClassSpawner for CharacterMeshSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::CharacterMesh
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::CharacterMesh, props);
        let name = instance.name.clone();
        let defaults = CharacterMesh::default();

        let mesh = CharacterMesh {
            mesh_id: props.get_string("mesh_id").map(str::to_string).unwrap_or(defaults.mesh_id),
            base_texture_id: props
                .get_string("base_texture_id")
                .map(str::to_string)
                .unwrap_or(defaults.base_texture_id),
            overlay_texture_id: props
                .get_string("overlay_texture_id")
                .map(str::to_string)
                .unwrap_or(defaults.overlay_texture_id),
            body_part: props.get_string("body_part").map(str::to_string).unwrap_or(defaults.body_part),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                mesh,
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
            if let Some(mut mesh) = em.get_mut::<CharacterMesh>() {
                if let Some(v) = props.get_string("mesh_id") { mesh.mesh_id = v.to_string(); }
                if let Some(v) = props.get_string("base_texture_id") { mesh.base_texture_id = v.to_string(); }
                if let Some(v) = props.get_string("overlay_texture_id") { mesh.overlay_texture_id = v.to_string(); }
                if let Some(v) = props.get_string("body_part") { mesh.body_part = v.to_string(); }
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
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("MeshId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("mesh_id", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("BaseTextureId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("base_texture_id", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("OverlayTextureId").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("overlay_texture_id", PropertyValue::String(v));
        }
        // Roblox uses a `BodyPart` enum (Head/Torso/LeftArm/…); surfaces as
        // a string through the Wave 2 adapter.
        if let Some(v) = rbx.property("BodyPart").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("body_part", PropertyValue::String(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["mesh_id", "base_texture_id", "overlay_texture_id", "body_part"] {
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
            toml::Value::Table(export_metadata(world, entity, "CharacterMesh")),
        );
        if let Some(m) = world.get::<CharacterMesh>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("mesh_id".into(), toml::Value::String(m.mesh_id.clone()));
            props.insert("base_texture_id".into(), toml::Value::String(m.base_texture_id.clone()));
            props.insert(
                "overlay_texture_id".into(),
                toml::Value::String(m.overlay_texture_id.clone()),
            );
            props.insert("body_part".into(), toml::Value::String(m.body_part.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_character_mesh() {
        assert_eq!(CharacterMeshSpawner.class_name(), ClassName::CharacterMesh);
    }

    #[test]
    fn import_from_toml_reads_mesh_and_part() {
        let toml_src = r#"
            [metadata]
            class_name = "CharacterMesh"
            name = "RobloyTorso"
            [properties]
            mesh_id = "rbxassetid://12345"
            body_part = "Torso"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = CharacterMeshSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("mesh_id"), Some("rbxassetid://12345"));
        assert_eq!(bag.get_string("body_part"), Some("Torso"));
    }
}
