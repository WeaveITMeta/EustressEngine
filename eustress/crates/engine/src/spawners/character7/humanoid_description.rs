//! `HumanoidDescriptionSpawner` — `ClassSpawner` for
//! [`ClassName::HumanoidDescription`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! avatar body-part + scale specification. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, HumanoidDescription, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

const STR_FIELDS: [(&str, &str); 7] = [
    ("Face", "face"),
    ("Head", "head"),
    ("Torso", "torso"),
    ("LeftArm", "left_arm"),
    ("RightArm", "right_arm"),
    ("LeftLeg", "left_leg"),
    ("RightLeg", "right_leg"),
];
const F32_FIELDS: [(&str, &str); 5] = [
    ("HeightScale", "height_scale"),
    ("WidthScale", "width_scale"),
    ("HeadScale", "head_scale"),
    ("BodyTypeScale", "body_type_scale"),
    ("ProportionScale", "proportion_scale"),
];

/// Zero-sized spawner for [`ClassName::HumanoidDescription`].
#[derive(Default)]
pub struct HumanoidDescriptionSpawner;

impl ClassSpawner for HumanoidDescriptionSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::HumanoidDescription
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::HumanoidDescription, props);
        let name = instance.name.clone();
        let d = HumanoidDescription::default();
        let s = |k: &str, def: &str| props.get_string(k).map(str::to_string).unwrap_or_else(|| def.to_string());
        let comp = HumanoidDescription {
            face: s("face", &d.face),
            head: s("head", &d.head),
            torso: s("torso", &d.torso),
            left_arm: s("left_arm", &d.left_arm),
            right_arm: s("right_arm", &d.right_arm),
            left_leg: s("left_leg", &d.left_leg),
            right_leg: s("right_leg", &d.right_leg),
            height_scale: props.get_f32("height_scale").unwrap_or(d.height_scale),
            width_scale: props.get_f32("width_scale").unwrap_or(d.width_scale),
            head_scale: props.get_f32("head_scale").unwrap_or(d.head_scale),
            body_type_scale: props.get_f32("body_type_scale").unwrap_or(d.body_type_scale),
            proportion_scale: props.get_f32("proportion_scale").unwrap_or(d.proportion_scale),
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
        if let Some(mut comp) = world.get_mut::<HumanoidDescription>(entity) {
            if let Some(v) = props.get_string("face") { comp.face = v.to_string(); }
            if let Some(v) = props.get_string("head") { comp.head = v.to_string(); }
            if let Some(v) = props.get_string("torso") { comp.torso = v.to_string(); }
            if let Some(v) = props.get_string("left_arm") { comp.left_arm = v.to_string(); }
            if let Some(v) = props.get_string("right_arm") { comp.right_arm = v.to_string(); }
            if let Some(v) = props.get_string("left_leg") { comp.left_leg = v.to_string(); }
            if let Some(v) = props.get_string("right_leg") { comp.right_leg = v.to_string(); }
            if let Some(v) = props.get_f32("height_scale") { comp.height_scale = v; }
            if let Some(v) = props.get_f32("width_scale") { comp.width_scale = v; }
            if let Some(v) = props.get_f32("head_scale") { comp.head_scale = v; }
            if let Some(v) = props.get_f32("body_type_scale") { comp.body_type_scale = v; }
            if let Some(v) = props.get_f32("proportion_scale") { comp.proportion_scale = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(13);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in STR_FIELDS {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        for (rbx_key, key) in F32_FIELDS {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(13);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for (_, key) in STR_FIELDS {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            for (_, key) in F32_FIELDS {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "HumanoidDescription")),
        );
        if let Some(comp) = world.get::<HumanoidDescription>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("face".into(), toml::Value::String(comp.face.clone()));
            props.insert("head".into(), toml::Value::String(comp.head.clone()));
            props.insert("torso".into(), toml::Value::String(comp.torso.clone()));
            props.insert("left_arm".into(), toml::Value::String(comp.left_arm.clone()));
            props.insert("right_arm".into(), toml::Value::String(comp.right_arm.clone()));
            props.insert("left_leg".into(), toml::Value::String(comp.left_leg.clone()));
            props.insert("right_leg".into(), toml::Value::String(comp.right_leg.clone()));
            props.insert("height_scale".into(), toml::Value::Float(comp.height_scale as f64));
            props.insert("width_scale".into(), toml::Value::Float(comp.width_scale as f64));
            props.insert("head_scale".into(), toml::Value::Float(comp.head_scale as f64));
            props.insert("body_type_scale".into(), toml::Value::Float(comp.body_type_scale as f64));
            props.insert("proportion_scale".into(), toml::Value::Float(comp.proportion_scale as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_humanoid_description() {
        assert_eq!(HumanoidDescriptionSpawner.class_name(), ClassName::HumanoidDescription);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(HumanoidDescriptionSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "HumanoidDescription"
            name = "Avatar"
            [properties]
            head = "rbxassetid://1"
            height_scale = 1.1
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = HumanoidDescriptionSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("head"), Some("rbxassetid://1"));
        assert_eq!(bag.get_f32("height_scale"), Some(1.1));
    }
}
