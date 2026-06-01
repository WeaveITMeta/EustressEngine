//! `ControllerPartSensorSpawner` — `ClassSpawner` for
//! [`ClassName::ControllerPartSensor`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach
//! contact sensor for a ControllerManager. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, ControllerPartSensor, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::ControllerPartSensor`].
#[derive(Default)]
pub struct ControllerPartSensorSpawner;

impl ClassSpawner for ControllerPartSensorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ControllerPartSensor
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::ControllerPartSensor, props);
        let name = instance.name.clone();
        let d = ControllerPartSensor::default();
        let comp = ControllerPartSensor {
            sensor_mode: props.get_string("sensor_mode").map(str::to_string).unwrap_or(d.sensor_mode),
            search_distance: props.get_f32("search_distance").unwrap_or(d.search_distance),
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
        if let Some(mut comp) = world.get_mut::<ControllerPartSensor>(entity) {
            if let Some(v) = props.get_string("sensor_mode") { comp.sensor_mode = v.to_string(); }
            if let Some(v) = props.get_f32("search_distance") { comp.search_distance = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("SensorMode").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("sensor_mode", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("SearchDistance").and_then(|p| p.as_f32()) {
            bag.set("search_distance", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(v) = props.get("sensor_mode").and_then(|v| v.as_str()) {
                bag.set("sensor_mode", PropertyValue::String(v.to_string()));
            }
            if let Some(v) = props.get("search_distance").and_then(|v| v.as_float()) {
                bag.set("search_distance", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "ControllerPartSensor")),
        );
        if let Some(comp) = world.get::<ControllerPartSensor>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("sensor_mode".into(), toml::Value::String(comp.sensor_mode.clone()));
            props.insert("search_distance".into(), toml::Value::Float(comp.search_distance as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_controller_part_sensor() {
        assert_eq!(ControllerPartSensorSpawner.class_name(), ClassName::ControllerPartSensor);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(ControllerPartSensorSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "ControllerPartSensor"
            name = "Floor"
            [properties]
            sensor_mode = "Ladder"
            search_distance = 8.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = ControllerPartSensorSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("sensor_mode"), Some("Ladder"));
        assert_eq!(bag.get_f32("search_distance"), Some(8.0));
    }
}
