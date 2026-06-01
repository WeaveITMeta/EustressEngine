//! `PoseSpawner` — `ClassSpawner` for [`ClassName::Pose`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! per-bone pose within a keyframe. See the group [`mod`](super) docs.
//!
//! `cframe` is a full [`Transform`]; it round-trips through TOML as three
//! arrays (`translation`, `rotation` quaternion xyzw, `scale`).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Pose, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

fn read_f32(arr: &[toml::Value], i: usize, default: f32) -> f32 {
    arr.get(i).and_then(|v| v.as_float()).map(|f| f as f32).unwrap_or(default)
}

/// Zero-sized spawner for [`ClassName::Pose`].
#[derive(Default)]
pub struct PoseSpawner;

impl ClassSpawner for PoseSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Pose
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Pose, props);
        let name = instance.name.clone();
        let d = Pose::default();
        let comp = Pose {
            cframe: props.get_transform("cframe").copied().unwrap_or(d.cframe),
            weight: props.get_f32("weight").unwrap_or(d.weight),
            easing_style: props.get_string("easing_style").map(str::to_string).unwrap_or(d.easing_style),
            easing_direction: props.get_string("easing_direction").map(str::to_string).unwrap_or(d.easing_direction),
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
        if let Some(mut comp) = world.get_mut::<Pose>(entity) {
            if let Some(t) = props.get_transform("cframe").copied() { comp.cframe = t; }
            if let Some(v) = props.get_f32("weight") { comp.weight = v; }
            if let Some(v) = props.get_string("easing_style") { comp.easing_style = v.to_string(); }
            if let Some(v) = props.get_string("easing_direction") { comp.easing_direction = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Weight").and_then(|p| p.as_f32()) {
            bag.set("weight", PropertyValue::Float(v));
        }
        for (rbx_key, key) in [("EasingStyle", "easing_style"), ("EasingDirection", "easing_direction")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            let mut t = Pose::default().cframe;
            let mut have_t = false;
            if let Some(arr) = props.get("translation").and_then(|v| v.as_array()) {
                t.translation = Vec3::new(read_f32(arr, 0, 0.0), read_f32(arr, 1, 0.0), read_f32(arr, 2, 0.0));
                have_t = true;
            }
            if let Some(arr) = props.get("rotation").and_then(|v| v.as_array()) {
                t.rotation = Quat::from_xyzw(read_f32(arr, 0, 0.0), read_f32(arr, 1, 0.0), read_f32(arr, 2, 0.0), read_f32(arr, 3, 1.0));
                have_t = true;
            }
            if let Some(arr) = props.get("scale").and_then(|v| v.as_array()) {
                t.scale = Vec3::new(read_f32(arr, 0, 1.0), read_f32(arr, 1, 1.0), read_f32(arr, 2, 1.0));
                have_t = true;
            }
            if have_t {
                bag.set("cframe", PropertyValue::Transform(t));
            }
            if let Some(v) = props.get("weight").and_then(|v| v.as_float()) {
                bag.set("weight", PropertyValue::Float(v as f32));
            }
            for key in ["easing_style", "easing_direction"] {
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
            toml::Value::Table(export_metadata(world, entity, "Pose")),
        );
        if let Some(comp) = world.get::<Pose>(entity) {
            let t = comp.cframe;
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
            props.insert("weight".into(), toml::Value::Float(comp.weight as f64));
            props.insert("easing_style".into(), toml::Value::String(comp.easing_style.clone()));
            props.insert("easing_direction".into(), toml::Value::String(comp.easing_direction.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_pose() {
        assert_eq!(PoseSpawner.class_name(), ClassName::Pose);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(PoseSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "Pose"
            name = "Spine1"
            [properties]
            translation = [0.0, 0.5, 0.0]
            weight = 0.75
            easing_style = "Cubic"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = PoseSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("weight"), Some(0.75));
        assert_eq!(bag.get_string("easing_style"), Some("Cubic"));
        assert!(bag.get_transform("cframe").is_some());
    }
}
