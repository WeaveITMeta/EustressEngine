//! `MaterialVariantSpawner` — `ClassSpawner` for
//! [`ClassName::MaterialVariant`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach custom
//! material definition (PBR maps over a base material).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, MaterialVariant, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

const STR_FIELDS: [&str; 6] = [
    "base_material", "color_map", "metalness_map",
    "normal_map", "roughness_map", "material_pattern",
];

/// Zero-sized spawner for [`ClassName::MaterialVariant`].
#[derive(Default)]
pub struct MaterialVariantSpawner;

impl ClassSpawner for MaterialVariantSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::MaterialVariant
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::MaterialVariant, props);
        let name = instance.name.clone();
        let d = MaterialVariant::default();
        let comp = MaterialVariant {
            base_material: props.get_string("base_material").map(str::to_string).unwrap_or(d.base_material),
            color_map: props.get_string("color_map").map(str::to_string).unwrap_or(d.color_map),
            metalness_map: props.get_string("metalness_map").map(str::to_string).unwrap_or(d.metalness_map),
            normal_map: props.get_string("normal_map").map(str::to_string).unwrap_or(d.normal_map),
            roughness_map: props.get_string("roughness_map").map(str::to_string).unwrap_or(d.roughness_map),
            material_pattern: props.get_string("material_pattern").map(str::to_string).unwrap_or(d.material_pattern),
            studs_per_tile: props.get_f32("studs_per_tile").unwrap_or(d.studs_per_tile),
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
        if let Some(mut comp) = world.get_mut::<MaterialVariant>(entity) {
            if let Some(v) = props.get_string("base_material") { comp.base_material = v.to_string(); }
            if let Some(v) = props.get_string("color_map") { comp.color_map = v.to_string(); }
            if let Some(v) = props.get_string("metalness_map") { comp.metalness_map = v.to_string(); }
            if let Some(v) = props.get_string("normal_map") { comp.normal_map = v.to_string(); }
            if let Some(v) = props.get_string("roughness_map") { comp.roughness_map = v.to_string(); }
            if let Some(v) = props.get_string("material_pattern") { comp.material_pattern = v.to_string(); }
            if let Some(v) = props.get_f32("studs_per_tile") { comp.studs_per_tile = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(7);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [
            ("BaseMaterial", "base_material"),
            ("ColorMap", "color_map"),
            ("MetalnessMap", "metalness_map"),
            ("NormalMap", "normal_map"),
            ("RoughnessMap", "roughness_map"),
            ("MaterialPattern", "material_pattern"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        if let Some(v) = rbx.property("StudsPerTile").and_then(|p| p.as_f32()) {
            bag.set("studs_per_tile", PropertyValue::Float(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in STR_FIELDS {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            if let Some(v) = props.get("studs_per_tile").and_then(|v| v.as_float()) {
                bag.set("studs_per_tile", PropertyValue::Float(v as f32));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "MaterialVariant")),
        );
        if let Some(comp) = world.get::<MaterialVariant>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("base_material".into(), toml::Value::String(comp.base_material.clone()));
            props.insert("color_map".into(), toml::Value::String(comp.color_map.clone()));
            props.insert("metalness_map".into(), toml::Value::String(comp.metalness_map.clone()));
            props.insert("normal_map".into(), toml::Value::String(comp.normal_map.clone()));
            props.insert("roughness_map".into(), toml::Value::String(comp.roughness_map.clone()));
            props.insert("material_pattern".into(), toml::Value::String(comp.material_pattern.clone()));
            props.insert("studs_per_tile".into(), toml::Value::Float(comp.studs_per_tile as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_material_variant() {
        assert_eq!(MaterialVariantSpawner.class_name(), ClassName::MaterialVariant);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(MaterialVariantSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "MaterialVariant"
            name = "Variant"
            [properties]
            base_material = "Metal"
            studs_per_tile = 4.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = MaterialVariantSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("base_material"), Some("Metal"));
        assert_eq!(bag.get_f32("studs_per_tile"), Some(4.0));
    }
}
