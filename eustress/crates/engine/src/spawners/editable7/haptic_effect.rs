//! `HapticEffectSpawner` — `ClassSpawner` for [`ClassName::HapticEffect`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.G). Data-attach config
//! carrier: a controller-rumble effect of the named `type` (e.g. `Vibration`)
//! at `magnitude` 0-1. See the group [`mod`](super) docs. The haptics-playback
//! system is a later phase; the config round-trips here.
//!
//! Note the component field is `type_` (trailing underscore — `type` is a Rust
//! keyword); the bag / TOML key and the Roblox property use the clean `type` /
//! `Type` spelling.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, HapticEffect, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::HapticEffect`].
#[derive(Default)]
pub struct HapticEffectSpawner;

impl ClassSpawner for HapticEffectSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::HapticEffect
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::HapticEffect, props);
        let name = instance.name.clone();
        let d = HapticEffect::default();
        let comp = HapticEffect {
            type_: props.get_string("type").map(str::to_string).unwrap_or(d.type_),
            magnitude: props.get_f32("magnitude").unwrap_or(d.magnitude),
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
        if let Some(mut comp) = world.get_mut::<HapticEffect>(entity) {
            if let Some(v) = props.get_string("type") { comp.type_ = v.to_string(); }
            if let Some(v) = props.get_f32("magnitude") { comp.magnitude = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Type").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("type", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Magnitude").and_then(|p| p.as_f32()) {
            bag.set("magnitude", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("type").and_then(|v| v.as_str()) {
                bag.set("type", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("magnitude").and_then(|v| v.as_float()) {
                bag.set("magnitude", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "HapticEffect")),
        );
        if let Some(comp) = world.get::<HapticEffect>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("type".into(), toml::Value::String(comp.type_.clone()));
            props.insert("magnitude".into(), toml::Value::Float(comp.magnitude as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_haptic_effect() {
        assert_eq!(HapticEffectSpawner.class_name(), ClassName::HapticEffect);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(HapticEffectSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_effect() {
        let toml_src = r#"
            [metadata]
            class_name = "HapticEffect"
            name = "Rumble"
            [properties]
            type = "Vibration"
            magnitude = 0.75
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = HapticEffectSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("type"), Some("Vibration"));
        assert_eq!(bag.get_f32("magnitude"), Some(0.75));
    }
}
