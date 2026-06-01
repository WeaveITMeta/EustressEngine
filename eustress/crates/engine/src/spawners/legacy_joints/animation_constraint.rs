//! `AnimationConstraintSpawner` — `ClassSpawner` for
//! [`ClassName::AnimationConstraint`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.A). Config-attach with
//! the drive-toward-pose actuation deferred (see the group [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{AnimationConstraint, ClassName, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{
    apply_metadata_edit, export_metadata, import_metadata, insert_optional_ref, instance_from_bag,
    read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::AnimationConstraint`].
#[derive(Default)]
pub struct AnimationConstraintSpawner;

impl ClassSpawner for AnimationConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::AnimationConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::AnimationConstraint, props);
        let name = instance.name.clone();
        let d = AnimationConstraint::default();
        let comp = AnimationConstraint {
            attachment0: read_optional_ref(props, "attachment0"),
            attachment1: read_optional_ref(props, "attachment1"),
            max_force: props.get_f32("max_force").unwrap_or(d.max_force),
            max_torque: props.get_f32("max_torque").unwrap_or(d.max_torque),
            rigidity_enabled: props.get_bool("rigidity_enabled").unwrap_or(d.rigidity_enabled),
        };
        // TODO(avian): drive Attachment0 toward Attachment1's pose with the
        // configured force/torque clamps once a ref→Entity resolver exists.

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
        if let Some(mut comp) = world.get_mut::<AnimationConstraint>(entity) {
            if props.get("attachment0").is_some() { comp.attachment0 = read_optional_ref(props, "attachment0"); }
            if props.get("attachment1").is_some() { comp.attachment1 = read_optional_ref(props, "attachment1"); }
            if let Some(v) = props.get_f32("max_force") { comp.max_force = v; }
            if let Some(v) = props.get_f32("max_torque") { comp.max_torque = v; }
            if let Some(v) = props.get_bool("rigidity_enabled") { comp.rigidity_enabled = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [("Attachment0", "attachment0"), ("Attachment1", "attachment1")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_i32()) {
                bag.set(key, PropertyValue::Int(v));
            }
        }
        for (rbx_key, key) in [("MaxForce", "max_force"), ("MaxTorque", "max_torque")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        if let Some(v) = rbx.property("RigidityEnabled").and_then(|p| p.as_bool()) {
            bag.set("rigidity_enabled", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["attachment0", "attachment1"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
            for key in ["max_force", "max_torque"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            if let Some(v) = props.get("rigidity_enabled").and_then(|v| v.as_bool()) {
                bag.set("rigidity_enabled", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "AnimationConstraint")),
        );
        if let Some(comp) = world.get::<AnimationConstraint>(entity) {
            let mut props = toml::value::Table::new();
            insert_optional_ref(&mut props, "attachment0", comp.attachment0);
            insert_optional_ref(&mut props, "attachment1", comp.attachment1);
            props.insert("max_force".into(), toml::Value::Float(comp.max_force as f64));
            props.insert("max_torque".into(), toml::Value::Float(comp.max_torque as f64));
            props.insert("rigidity_enabled".into(), toml::Value::Boolean(comp.rigidity_enabled));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_animation_constraint() {
        assert_eq!(AnimationConstraintSpawner.class_name(), ClassName::AnimationConstraint);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(AnimationConstraintSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "AnimationConstraint"
            name = "AC"
            [properties]
            max_force = 500.0
            rigidity_enabled = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = AnimationConstraintSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("max_force"), Some(500.0));
        assert_eq!(bag.get_bool("rigidity_enabled"), Some(true));
    }
}
