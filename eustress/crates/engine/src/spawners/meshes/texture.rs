//! `TextureSpawner` — `ClassSpawner` for [`ClassName::Texture`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.C). Data-attach
//! tiling texture decal applied to a face of a part.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, Texture};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, color3_to_toml, export_metadata, import_metadata, instance_from_bag, read_color3};

/// Zero-sized spawner for [`ClassName::Texture`].
#[derive(Default)]
pub struct TextureSpawner;

impl ClassSpawner for TextureSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Texture
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Texture, props);
        let name = instance.name.clone();
        let d = Texture::default();
        let comp = Texture {
            texture: props.get_string("texture").map(str::to_string).unwrap_or(d.texture),
            face: props.get_string("face").map(str::to_string).unwrap_or(d.face),
            studs_per_tile_u: props.get_f32("studs_per_tile_u").unwrap_or(d.studs_per_tile_u),
            studs_per_tile_v: props.get_f32("studs_per_tile_v").unwrap_or(d.studs_per_tile_v),
            offset_studs_u: props.get_f32("offset_studs_u").unwrap_or(d.offset_studs_u),
            offset_studs_v: props.get_f32("offset_studs_v").unwrap_or(d.offset_studs_v),
            color3: props.get_color3("color3").unwrap_or(d.color3),
            transparency: props.get_f32("transparency").unwrap_or(d.transparency),
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
        if let Some(mut comp) = world.get_mut::<Texture>(entity) {
            if let Some(v) = props.get_string("texture") { comp.texture = v.to_string(); }
            if let Some(v) = props.get_string("face") { comp.face = v.to_string(); }
            if let Some(v) = props.get_f32("studs_per_tile_u") { comp.studs_per_tile_u = v; }
            if let Some(v) = props.get_f32("studs_per_tile_v") { comp.studs_per_tile_v = v; }
            if let Some(v) = props.get_f32("offset_studs_u") { comp.offset_studs_u = v; }
            if let Some(v) = props.get_f32("offset_studs_v") { comp.offset_studs_v = v; }
            if let Some(v) = props.get_color3("color3") { comp.color3 = v; }
            if let Some(v) = props.get_f32("transparency") { comp.transparency = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Texture").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("texture", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Face").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("face", PropertyValue::String(v));
        }
        for (rbx_key, key) in [
            ("StudsPerTileU", "studs_per_tile_u"),
            ("StudsPerTileV", "studs_per_tile_v"),
            ("OffsetStudsU", "offset_studs_u"),
            ("OffsetStudsV", "offset_studs_v"),
            ("Transparency", "transparency"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(9);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["texture", "face"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            for key in [
                "studs_per_tile_u", "studs_per_tile_v",
                "offset_studs_u", "offset_studs_v", "transparency",
            ] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            if let Some(arr) = props.get("color3").and_then(|v| v.as_array()) {
                bag.set("color3", PropertyValue::Color3(read_color3(arr)));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Texture")),
        );
        if let Some(comp) = world.get::<Texture>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("texture".into(), toml::Value::String(comp.texture.clone()));
            props.insert("face".into(), toml::Value::String(comp.face.clone()));
            props.insert("studs_per_tile_u".into(), toml::Value::Float(comp.studs_per_tile_u as f64));
            props.insert("studs_per_tile_v".into(), toml::Value::Float(comp.studs_per_tile_v as f64));
            props.insert("offset_studs_u".into(), toml::Value::Float(comp.offset_studs_u as f64));
            props.insert("offset_studs_v".into(), toml::Value::Float(comp.offset_studs_v as f64));
            props.insert("color3".into(), color3_to_toml(comp.color3));
            props.insert("transparency".into(), toml::Value::Float(comp.transparency as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_texture() {
        assert_eq!(TextureSpawner.class_name(), ClassName::Texture);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(TextureSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "Texture"
            name = "Tex"
            [properties]
            face = "Top"
            studs_per_tile_u = 4.0
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = TextureSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("face"), Some("Top"));
        assert_eq!(bag.get_f32("studs_per_tile_u"), Some(4.0));
    }
}
