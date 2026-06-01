//! `SurfaceAppearanceSpawner` — `ClassSpawner` for
//! [`ClassName::SurfaceAppearance`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach PBR
//! texture-map override for a MeshPart's surface.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, SurfaceAppearance};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

const FIELDS: [&str; 5] = ["color_map", "metalness_map", "normal_map", "roughness_map", "alpha_mode"];

/// Zero-sized spawner for [`ClassName::SurfaceAppearance`].
#[derive(Default)]
pub struct SurfaceAppearanceSpawner;

impl ClassSpawner for SurfaceAppearanceSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::SurfaceAppearance
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::SurfaceAppearance, props);
        let name = instance.name.clone();
        let d = SurfaceAppearance::default();
        let comp = SurfaceAppearance {
            color_map: props.get_string("color_map").map(str::to_string).unwrap_or(d.color_map),
            metalness_map: props.get_string("metalness_map").map(str::to_string).unwrap_or(d.metalness_map),
            normal_map: props.get_string("normal_map").map(str::to_string).unwrap_or(d.normal_map),
            roughness_map: props.get_string("roughness_map").map(str::to_string).unwrap_or(d.roughness_map),
            alpha_mode: props.get_string("alpha_mode").map(str::to_string).unwrap_or(d.alpha_mode),
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
        if let Some(mut comp) = world.get_mut::<SurfaceAppearance>(entity) {
            if let Some(v) = props.get_string("color_map") { comp.color_map = v.to_string(); }
            if let Some(v) = props.get_string("metalness_map") { comp.metalness_map = v.to_string(); }
            if let Some(v) = props.get_string("normal_map") { comp.normal_map = v.to_string(); }
            if let Some(v) = props.get_string("roughness_map") { comp.roughness_map = v.to_string(); }
            if let Some(v) = props.get_string("alpha_mode") { comp.alpha_mode = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [
            ("ColorMap", "color_map"),
            ("MetalnessMap", "metalness_map"),
            ("NormalMap", "normal_map"),
            ("RoughnessMap", "roughness_map"),
            ("AlphaMode", "alpha_mode"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in FIELDS {
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
            toml::Value::Table(export_metadata(world, entity, "SurfaceAppearance")),
        );
        if let Some(comp) = world.get::<SurfaceAppearance>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("color_map".into(), toml::Value::String(comp.color_map.clone()));
            props.insert("metalness_map".into(), toml::Value::String(comp.metalness_map.clone()));
            props.insert("normal_map".into(), toml::Value::String(comp.normal_map.clone()));
            props.insert("roughness_map".into(), toml::Value::String(comp.roughness_map.clone()));
            props.insert("alpha_mode".into(), toml::Value::String(comp.alpha_mode.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_surface_appearance() {
        assert_eq!(SurfaceAppearanceSpawner.class_name(), ClassName::SurfaceAppearance);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(SurfaceAppearanceSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "SurfaceAppearance"
            name = "PBR"
            [properties]
            alpha_mode = "Transparency"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = SurfaceAppearanceSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("alpha_mode"), Some("Transparency"));
    }
}
