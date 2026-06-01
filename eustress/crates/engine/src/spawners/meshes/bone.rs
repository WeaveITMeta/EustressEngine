//! `BoneSpawner` — `ClassSpawner` for [`ClassName::Bone`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! skeletal bone for skinned-mesh deformation.
//!
//! The bone's local pose is a full [`Transform`]; it round-trips through the
//! bag as a `Transform` `PropertyValue` and through TOML as three arrays
//! (`translation`, `rotation` quaternion xyzw, `scale`).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{Bone, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

fn read_f32(arr: &[toml::Value], i: usize, default: f32) -> f32 {
    arr.get(i).and_then(|v| v.as_float()).map(|f| f as f32).unwrap_or(default)
}

/// Zero-sized spawner for [`ClassName::Bone`].
#[derive(Default)]
pub struct BoneSpawner;

impl ClassSpawner for BoneSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Bone
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Bone, props);
        let name = instance.name.clone();
        let comp = Bone {
            transform: props.get_transform("transform").copied().unwrap_or(Bone::default().transform),
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
        if let Some(t) = props.get_transform("transform").copied() {
            if let Some(mut comp) = world.get_mut::<Bone>(entity) {
                comp.transform = t;
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
            let mut t = Bone::default().transform;
            if let Some(arr) = props.get("translation").and_then(|v| v.as_array()) {
                t.translation = Vec3::new(read_f32(arr, 0, 0.0), read_f32(arr, 1, 0.0), read_f32(arr, 2, 0.0));
            }
            if let Some(arr) = props.get("rotation").and_then(|v| v.as_array()) {
                t.rotation = Quat::from_xyzw(
                    read_f32(arr, 0, 0.0),
                    read_f32(arr, 1, 0.0),
                    read_f32(arr, 2, 0.0),
                    read_f32(arr, 3, 1.0),
                );
            }
            if let Some(arr) = props.get("scale").and_then(|v| v.as_array()) {
                t.scale = Vec3::new(read_f32(arr, 0, 1.0), read_f32(arr, 1, 1.0), read_f32(arr, 2, 1.0));
            }
            bag.set("transform", PropertyValue::Transform(t));
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Bone")),
        );
        if let Some(comp) = world.get::<Bone>(entity) {
            let t = comp.transform;
            let mut props = toml::value::Table::new();
            props.insert(
                "translation".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(t.translation.x as f64),
                    toml::Value::Float(t.translation.y as f64),
                    toml::Value::Float(t.translation.z as f64),
                ]),
            );
            props.insert(
                "rotation".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(t.rotation.x as f64),
                    toml::Value::Float(t.rotation.y as f64),
                    toml::Value::Float(t.rotation.z as f64),
                    toml::Value::Float(t.rotation.w as f64),
                ]),
            );
            props.insert(
                "scale".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(t.scale.x as f64),
                    toml::Value::Float(t.scale.y as f64),
                    toml::Value::Float(t.scale.z as f64),
                ]),
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
    fn class_name_is_bone() {
        assert_eq!(BoneSpawner.class_name(), ClassName::Bone);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(BoneSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_translation() {
        let toml_src = r#"
            [metadata]
            class_name = "Bone"
            name = "Spine"
            [properties]
            translation = [1.0, 2.0, 3.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BoneSpawner.import_from_toml(&value);
        let t = bag.get_transform("transform").copied().unwrap();
        assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
    }
}
