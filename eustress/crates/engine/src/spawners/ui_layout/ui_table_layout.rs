//! `UITableLayoutSpawner` — `ClassSpawner` for [`ClassName::UITableLayout`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: auto-arranges children as a table. The UDim2 4-tuple `padding`
//! round-trips losslessly through TOML and defaults at `spawn`.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UITableLayout};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, float_array_to_toml, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UITableLayout`].
#[derive(Default)]
pub struct UITableLayoutSpawner;

impl ClassSpawner for UITableLayoutSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UITableLayout
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UITableLayout, props);
        let name = instance.name.clone();
        let d = UITableLayout::default();
        let comp = UITableLayout {
            padding: d.padding,
            fill_direction: props.get_string("fill_direction").map(str::to_string).unwrap_or(d.fill_direction),
            fill_empty_space_columns: props.get_bool("fill_empty_space_columns").unwrap_or(d.fill_empty_space_columns),
            fill_empty_space_rows: props.get_bool("fill_empty_space_rows").unwrap_or(d.fill_empty_space_rows),
            major_axis: props.get_string("major_axis").map(str::to_string).unwrap_or(d.major_axis),
            sort_order: props.get_string("sort_order").map(str::to_string).unwrap_or(d.sort_order),
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
        if let Some(mut comp) = world.get_mut::<UITableLayout>(entity) {
            if let Some(v) = props.get_string("fill_direction") { comp.fill_direction = v.to_string(); }
            if let Some(v) = props.get_bool("fill_empty_space_columns") { comp.fill_empty_space_columns = v; }
            if let Some(v) = props.get_bool("fill_empty_space_rows") { comp.fill_empty_space_rows = v; }
            if let Some(v) = props.get_string("major_axis") { comp.major_axis = v.to_string(); }
            if let Some(v) = props.get_string("sort_order") { comp.sort_order = v.to_string(); }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in [
            ("FillDirection", "fill_direction"),
            ("MajorAxis", "major_axis"),
            ("SortOrder", "sort_order"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        for (rbx_key, key) in [
            ("FillEmptySpaceColumns", "fill_empty_space_columns"),
            ("FillEmptySpaceRows", "fill_empty_space_rows"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_bool()) {
                bag.set(key, PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(7);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["fill_direction", "major_axis", "sort_order"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
            for key in ["fill_empty_space_columns", "fill_empty_space_rows"] {
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
            toml::Value::Table(export_metadata(world, entity, "UITableLayout")),
        );
        if let Some(comp) = world.get::<UITableLayout>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("padding".into(), float_array_to_toml(&comp.padding));
            props.insert("fill_direction".into(), toml::Value::String(comp.fill_direction.clone()));
            props.insert("fill_empty_space_columns".into(), toml::Value::Boolean(comp.fill_empty_space_columns));
            props.insert("fill_empty_space_rows".into(), toml::Value::Boolean(comp.fill_empty_space_rows));
            props.insert("major_axis".into(), toml::Value::String(comp.major_axis.clone()));
            props.insert("sort_order".into(), toml::Value::String(comp.sort_order.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_table_layout() {
        assert_eq!(UITableLayoutSpawner.class_name(), ClassName::UITableLayout);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UITableLayoutSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UITableLayout"
            name = "Table"
            [properties]
            major_axis = "ColumnMajor"
            fill_empty_space_rows = true
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UITableLayoutSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("major_axis"), Some("ColumnMajor"));
        assert_eq!(bag.get_bool("fill_empty_space_rows"), Some(true));
    }
}
