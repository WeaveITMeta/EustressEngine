//! `UIGridLayoutSpawner` — `ClassSpawner` for [`ClassName::UIGridLayout`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: auto-arranges sibling GuiObjects in a grid.
//!
//! The UDim2 4-tuples (`cell_size`, `cell_padding`) round-trip losslessly
//! through TOML and default at `spawn` (no scalar bag representation).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIGridLayout};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, float_array_to_toml, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIGridLayout`].
#[derive(Default)]
pub struct UIGridLayoutSpawner;

impl ClassSpawner for UIGridLayoutSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIGridLayout
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIGridLayout, props);
        let name = instance.name.clone();
        let d = UIGridLayout::default();
        let comp = UIGridLayout {
            cell_size: d.cell_size,
            cell_padding: d.cell_padding,
            fill_direction: props.get_string("fill_direction").map(str::to_string).unwrap_or(d.fill_direction),
            sort_order: props.get_string("sort_order").map(str::to_string).unwrap_or(d.sort_order),
            start_corner: props.get_string("start_corner").map(str::to_string).unwrap_or(d.start_corner),
            horizontal_alignment: props.get_string("horizontal_alignment").map(str::to_string).unwrap_or(d.horizontal_alignment),
            vertical_alignment: props.get_string("vertical_alignment").map(str::to_string).unwrap_or(d.vertical_alignment),
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
        if let Some(mut comp) = world.get_mut::<UIGridLayout>(entity) {
            if let Some(v) = props.get_string("fill_direction") { comp.fill_direction = v.to_string(); }
            if let Some(v) = props.get_string("sort_order") { comp.sort_order = v.to_string(); }
            if let Some(v) = props.get_string("start_corner") { comp.start_corner = v.to_string(); }
            if let Some(v) = props.get_string("horizontal_alignment") { comp.horizontal_alignment = v.to_string(); }
            if let Some(v) = props.get_string("vertical_alignment") { comp.vertical_alignment = v.to_string(); }
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
            ("SortOrder", "sort_order"),
            ("StartCorner", "start_corner"),
            ("HorizontalAlignment", "horizontal_alignment"),
            ("VerticalAlignment", "vertical_alignment"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(7);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["fill_direction", "sort_order", "start_corner", "horizontal_alignment", "vertical_alignment"] {
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
            toml::Value::Table(export_metadata(world, entity, "UIGridLayout")),
        );
        if let Some(comp) = world.get::<UIGridLayout>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("cell_size".into(), float_array_to_toml(&comp.cell_size));
            props.insert("cell_padding".into(), float_array_to_toml(&comp.cell_padding));
            props.insert("fill_direction".into(), toml::Value::String(comp.fill_direction.clone()));
            props.insert("sort_order".into(), toml::Value::String(comp.sort_order.clone()));
            props.insert("start_corner".into(), toml::Value::String(comp.start_corner.clone()));
            props.insert("horizontal_alignment".into(), toml::Value::String(comp.horizontal_alignment.clone()));
            props.insert("vertical_alignment".into(), toml::Value::String(comp.vertical_alignment.clone()));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_grid_layout() {
        assert_eq!(UIGridLayoutSpawner.class_name(), ClassName::UIGridLayout);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIGridLayoutSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIGridLayout"
            name = "Grid"
            [properties]
            fill_direction = "Vertical"
            start_corner = "TopRight"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIGridLayoutSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("fill_direction"), Some("Vertical"));
        assert_eq!(bag.get_string("start_corner"), Some("TopRight"));
    }
}
