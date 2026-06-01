//! `BodyColorsSpawner` — `ClassSpawner` for [`ClassName::BodyColors`].
//!
//! Per-limb BrickColor palette for a character. The spawner attaches the
//! [`BodyColors`] config (6 BrickColor palette indices); the recolor of the
//! character's rendered limb materials lives in
//! [`crate::interaction::appearance`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BodyColors, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag};

/// The six BrickColor fields in [`BodyColors`], in canonical order.
const COLOR_KEYS: [&str; 6] = [
    "head_color",
    "torso_color",
    "left_arm_color",
    "right_arm_color",
    "left_leg_color",
    "right_leg_color",
];

/// Zero-sized spawner for [`ClassName::BodyColors`].
#[derive(Default)]
pub struct BodyColorsSpawner;

impl ClassSpawner for BodyColorsSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BodyColors
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::BodyColors, props);
        let name = instance.name.clone();
        let defaults = BodyColors::default();

        let colors = BodyColors {
            head_color: props.get_i32("head_color").unwrap_or(defaults.head_color),
            torso_color: props.get_i32("torso_color").unwrap_or(defaults.torso_color),
            left_arm_color: props.get_i32("left_arm_color").unwrap_or(defaults.left_arm_color),
            right_arm_color: props.get_i32("right_arm_color").unwrap_or(defaults.right_arm_color),
            left_leg_color: props.get_i32("left_leg_color").unwrap_or(defaults.left_leg_color),
            right_leg_color: props.get_i32("right_leg_color").unwrap_or(defaults.right_leg_color),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                colors,
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
            if let Some(mut colors) = em.get_mut::<BodyColors>() {
                if let Some(v) = props.get_i32("head_color") { colors.head_color = v; }
                if let Some(v) = props.get_i32("torso_color") { colors.torso_color = v; }
                if let Some(v) = props.get_i32("left_arm_color") { colors.left_arm_color = v; }
                if let Some(v) = props.get_i32("right_arm_color") { colors.right_arm_color = v; }
                if let Some(v) = props.get_i32("left_leg_color") { colors.left_leg_color = v; }
                if let Some(v) = props.get_i32("right_leg_color") { colors.right_leg_color = v; }
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
        let mut bag = PropertyBag::with_capacity(7);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        // Roblox BodyColors stores BrickColor properties: HeadColor,
        // TorsoColor, LeftArmColor, RightArmColor, LeftLegColor,
        // RightLegColor. The Wave 4 importer maps BrickColor → palette index.
        for (rbx_key, our_key) in [
            ("HeadColor", "head_color"),
            ("TorsoColor", "torso_color"),
            ("LeftArmColor", "left_arm_color"),
            ("RightArmColor", "right_arm_color"),
            ("LeftLegColor", "left_leg_color"),
            ("RightLegColor", "right_leg_color"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_i32()) {
                bag.set(our_key, PropertyValue::Int(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in COLOR_KEYS {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "BodyColors")),
        );
        if let Some(c) = world.get::<BodyColors>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("head_color".into(), toml::Value::Integer(c.head_color as i64));
            props.insert("torso_color".into(), toml::Value::Integer(c.torso_color as i64));
            props.insert("left_arm_color".into(), toml::Value::Integer(c.left_arm_color as i64));
            props.insert("right_arm_color".into(), toml::Value::Integer(c.right_arm_color as i64));
            props.insert("left_leg_color".into(), toml::Value::Integer(c.left_leg_color as i64));
            props.insert("right_leg_color".into(), toml::Value::Integer(c.right_leg_color as i64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_body_colors() {
        assert_eq!(BodyColorsSpawner.class_name(), ClassName::BodyColors);
    }

    #[test]
    fn import_from_toml_reads_six_colors() {
        let toml_src = r#"
            [metadata]
            class_name = "BodyColors"
            name = "Skin"
            [properties]
            head_color = 1
            torso_color = 21
            left_arm_color = 1
            right_arm_color = 1
            left_leg_color = 23
            right_leg_color = 23
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BodyColorsSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("torso_color"), Some(21));
        assert_eq!(bag.get_i32("left_leg_color"), Some(23));
    }
}
