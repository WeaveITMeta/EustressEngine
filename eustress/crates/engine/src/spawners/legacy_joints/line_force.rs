//! `LineForceSpawner` — `ClassSpawner` for [`ClassName::LineForce`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.A). Config-attach with
//! the per-frame force actuation deferred (see the group [`mod`](super) docs).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, LineForce, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{
    apply_metadata_edit, export_metadata, import_metadata, insert_optional_ref, instance_from_bag,
    read_optional_ref,
};

/// Zero-sized spawner for [`ClassName::LineForce`].
#[derive(Default)]
pub struct LineForceSpawner;

impl ClassSpawner for LineForceSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::LineForce
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::LineForce, props);
        let name = instance.name.clone();
        let d = LineForce::default();
        let comp = LineForce {
            attachment0: read_optional_ref(props, "attachment0"),
            attachment1: read_optional_ref(props, "attachment1"),
            magnitude: props.get_f32("magnitude").unwrap_or(d.magnitude),
            max_force: props.get_f32("max_force").unwrap_or(d.max_force),
            apply_at_center_of_mass: props.get_bool("apply_at_center_of_mass").unwrap_or(d.apply_at_center_of_mass),
            inverse_square_law: props.get_bool("inverse_square_law").unwrap_or(d.inverse_square_law),
            reaction_force_enabled: props.get_bool("reaction_force_enabled").unwrap_or(d.reaction_force_enabled),
        };
        // TODO(avian): apply the force per-frame between the two attachments
        // once a ref→Entity resolver + force-application system exist here.

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
        if let Some(mut comp) = world.get_mut::<LineForce>(entity) {
            if props.get("attachment0").is_some() { comp.attachment0 = read_optional_ref(props, "attachment0"); }
            if props.get("attachment1").is_some() { comp.attachment1 = read_optional_ref(props, "attachment1"); }
            if let Some(v) = props.get_f32("magnitude") { comp.magnitude = v; }
            if let Some(v) = props.get_f32("max_force") { comp.max_force = v; }
            if let Some(v) = props.get_bool("apply_at_center_of_mass") { comp.apply_at_center_of_mass = v; }
            if let Some(v) = props.get_bool("inverse_square_law") { comp.inverse_square_law = v; }
            if let Some(v) = props.get_bool("reaction_force_enabled") { comp.reaction_force_enabled = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(7);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [("Attachment0", "attachment0"), ("Attachment1", "attachment1")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_i32()) {
                bag.set(key, PropertyValue::Int(v));
            }
        }
        for (rbx_key, key) in [("Magnitude", "magnitude"), ("MaxForce", "max_force")] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        for (rbx_key, key) in [
            ("ApplyAtCenterOfMass", "apply_at_center_of_mass"),
            ("InverseSquareLaw", "inverse_square_law"),
            ("ReactionForceEnabled", "reaction_force_enabled"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_bool()) {
                bag.set(key, PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["attachment0", "attachment1"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_integer()) {
                    bag.set(key, PropertyValue::Int(v as i32));
                }
            }
            for key in ["magnitude", "max_force"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            for key in ["apply_at_center_of_mass", "inverse_square_law", "reaction_force_enabled"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_bool()) {
                    bag.set(key, PropertyValue::Bool(v));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "LineForce")),
        );
        if let Some(comp) = world.get::<LineForce>(entity) {
            let mut props = toml::value::Table::new();
            insert_optional_ref(&mut props, "attachment0", comp.attachment0);
            insert_optional_ref(&mut props, "attachment1", comp.attachment1);
            props.insert("magnitude".into(), toml::Value::Float(comp.magnitude as f64));
            props.insert("max_force".into(), toml::Value::Float(comp.max_force as f64));
            props.insert("apply_at_center_of_mass".into(), toml::Value::Boolean(comp.apply_at_center_of_mass));
            props.insert("inverse_square_law".into(), toml::Value::Boolean(comp.inverse_square_law));
            props.insert("reaction_force_enabled".into(), toml::Value::Boolean(comp.reaction_force_enabled));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_line_force() {
        assert_eq!(LineForceSpawner.class_name(), ClassName::LineForce);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(LineForceSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "LineForce"
            name = "LF"
            [properties]
            magnitude = 100.0
            inverse_square_law = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = LineForceSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("magnitude"), Some(100.0));
        assert_eq!(bag.get_bool("inverse_square_law"), Some(true));
    }
}
