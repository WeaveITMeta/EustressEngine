//! `UICornerSpawner` — `ClassSpawner` for [`ClassName::UICorner`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: rounds the corners of the parent GuiObject. See the group
//! [`mod`](super) docs for the shared rationale.
//!
//! `corner_radius` is a [`UDim`](eustress_common::ui_types::UDim) (scale +
//! offset); it round-trips through TOML as the 2-element array
//! `[scale, offset]`.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UICorner};
use eustress_common::ui_types::UDim;
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UICorner`].
#[derive(Default)]
pub struct UICornerSpawner;

impl ClassSpawner for UICornerSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UICorner
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UICorner, props);
        let name = instance.name.clone();
        let mut comp = UICorner::default();
        // `corner_radius` arrives as a Vector2 [scale, offset] in the bag.
        if let Some([scale, offset]) = props.get_vec2("corner_radius") {
            comp.corner_radius = UDim::new(scale, offset);
        }

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
        if let Some([scale, offset]) = props.get_vec2("corner_radius") {
            if let Some(mut comp) = world.get_mut::<UICorner>(entity) {
                comp.corner_radius = UDim::new(scale, offset);
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        // Roblox CornerRadius is a UDim; the stub adapter only exposes scalar
        // floats, so map a flat `CornerRadius` float into the offset axis.
        if let Some(offset) = rbx.property("CornerRadius").and_then(|p| p.as_f32()) {
            bag.set("corner_radius", PropertyValue::Vector2([0.0, offset]));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("corner_radius").and_then(|v| v.as_array()) {
                let scale = arr.first().and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
                let offset = arr.get(1).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
                bag.set("corner_radius", PropertyValue::Vector2([scale, offset]));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UICorner")),
        );
        if let Some(comp) = world.get::<UICorner>(entity) {
            let mut props = toml::value::Table::new();
            props.insert(
                "corner_radius".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(comp.corner_radius.scale as f64),
                    toml::Value::Float(comp.corner_radius.offset as f64),
                ]),
            );
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_corner() {
        assert_eq!(UICornerSpawner.class_name(), ClassName::UICorner);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UICornerSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_corner_radius() {
        let toml_src = r#"
            [metadata]
            class_name = "UICorner"
            name = "Round"
            [properties]
            corner_radius = [0.0, 8.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UICornerSpawner.import_from_toml(&value);
        assert_eq!(bag.get_vec2("corner_radius"), Some([0.0, 8.0]));
    }
}
