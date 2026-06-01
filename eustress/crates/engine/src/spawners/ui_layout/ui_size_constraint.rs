//! `UISizeConstraintSpawner` — `ClassSpawner` for
//! [`ClassName::UISizeConstraint`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: clamps a GuiObject's absolute pixel size. `min_size`/`max_size`
//! round-trip as 2-element float arrays (carried in the bag as `Vector2`).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UISizeConstraint};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, float_array_to_toml, import_metadata, instance_from_bag, read_float_array};

/// Zero-sized spawner for [`ClassName::UISizeConstraint`].
#[derive(Default)]
pub struct UISizeConstraintSpawner;

impl ClassSpawner for UISizeConstraintSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UISizeConstraint
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UISizeConstraint, props);
        let name = instance.name.clone();
        let d = UISizeConstraint::default();
        let comp = UISizeConstraint {
            min_size: props.get_vec2("min_size").unwrap_or(d.min_size),
            max_size: props.get_vec2("max_size").unwrap_or(d.max_size),
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
        if let Some(mut comp) = world.get_mut::<UISizeConstraint>(entity) {
            if let Some(v) = props.get_vec2("min_size") { comp.min_size = v; }
            if let Some(v) = props.get_vec2("max_size") { comp.max_size = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(1);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            if let Some(arr) = props.get("min_size").and_then(|v| v.as_array()) {
                bag.set("min_size", PropertyValue::Vector2(read_float_array::<2>(arr)));
            }
            if let Some(arr) = props.get("max_size").and_then(|v| v.as_array()) {
                bag.set("max_size", PropertyValue::Vector2(read_float_array::<2>(arr)));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UISizeConstraint")),
        );
        if let Some(comp) = world.get::<UISizeConstraint>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("min_size".into(), float_array_to_toml(&comp.min_size));
            props.insert("max_size".into(), float_array_to_toml(&comp.max_size));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_size_constraint() {
        assert_eq!(UISizeConstraintSpawner.class_name(), ClassName::UISizeConstraint);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UISizeConstraintSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_sizes() {
        let toml_src = r#"
            [metadata]
            class_name = "UISizeConstraint"
            name = "Clamp"
            [properties]
            min_size = [10.0, 20.0]
            max_size = [200.0, 300.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UISizeConstraintSpawner.import_from_toml(&value);
        assert_eq!(bag.get_vec2("min_size"), Some([10.0, 20.0]));
        assert_eq!(bag.get_vec2("max_size"), Some([200.0, 300.0]));
    }
}
