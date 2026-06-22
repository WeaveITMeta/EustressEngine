//! `BrickColorValueSpawner` — `ClassSpawner` for [`ClassName::BrickColorValue`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1
//! (ValueObjects, Wave 6.A). See `string_value.rs` for the shared rationale.
//!
//! `BrickColorValue` holds one BrickColor palette index (`i32`). Roblox
//! `BrickColorValue.Value` is a `BrickColor` (an index into the legacy
//! BrickColor palette). Eustress keeps the raw `i32`; downstream code resolves
//! it to an RGB triple via the palette table when a color is actually needed.
//!
//! That palette table now exists as [`eustress_common::brick_palette`]; the
//! stored index resolves to an sRGB triple via
//! [`eustress_common::brick_palette::srgb_for_index`]. Use
//! [`BrickColorValueSpawner::resolved_srgb`] as the convenience wrapper so the
//! resolution rule (and its out-of-range fallback) lives in one place.
//!
//! ## Roblox value
//!
//! A Roblox `BrickColor` may surface through the Wave 2 adapter as
//! [`RobloxPropertyValue::Int`] (the palette number) when the importer maps it
//! to its numeric index, or as [`RobloxPropertyValue::Other`] otherwise.
//! `import_from_roblox` reads the `Int` form when present; the Wave 4 importer
//! formalises BrickColor → index. The TOML path always carries the index.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BrickColorValue, ClassName, Instance, PropertyValue};
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::BrickColorValue`].
#[derive(Default)]
pub struct BrickColorValueSpawner;

impl BrickColorValueSpawner {
    /// Resolve a stored `BrickColorValue.value` index to its sRGB triple.
    ///
    /// Thin wrapper over [`eustress_common::brick_palette::srgb_for_index`] so
    /// downstream code (renderers, exporters, the color widget) has one
    /// canonical resolution point — including the out-of-range fallback that
    /// function applies.
    pub fn resolved_srgb(value: i32) -> [u8; 3] {
        eustress_common::brick_palette::srgb_for_index(value)
    }
}

impl ClassSpawner for BrickColorValueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BrickColorValue
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("BrickColorValue")
            .to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        // Default 194 is Roblox's "Medium stone grey" — the BrickColor default.
        let value = props.get_i32("value").unwrap_or(194);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::BrickColorValue,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                BrickColorValue { value },
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            let new_name = props.get_string("metadata.name").map(str::to_string);

            if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                if let Some(ref n) = new_name {
                    instance.name = n.clone();
                }
                if let Some(archivable) = props.get_bool("metadata.archivable") {
                    instance.archivable = archivable;
                }
            }

            if let Some(new_value) = props.get_i32("value") {
                if let Some(mut comp) = entity_mut.get_mut::<BrickColorValue>() {
                    comp.value = new_value;
                }
            }

            if let Some(ref n) = new_name {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(n.clone());
                }
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
        if let Some(value) = rbx.property("Value").and_then(|p| p.as_i32()) {
            bag.set("value", PropertyValue::Int(value));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);

        if let Some(meta) = toml_value.get("metadata") {
            if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
                bag.set("metadata.name", PropertyValue::String(name.to_string()));
            }
            if let Some(archivable) = meta.get("archivable").and_then(|v| v.as_bool()) {
                bag.set("metadata.archivable", PropertyValue::Bool(archivable));
            }
            if let Some(uuid) = meta.get("uuid").and_then(|v| v.as_str()) {
                bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
            }
        }

        if let Some(props) = toml_value.get("properties") {
            if let Some(value) = props.get("value").and_then(|v| v.as_integer()) {
                bag.set("value", PropertyValue::Int(value as i32));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("BrickColorValue".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert("archivable".to_string(), toml::Value::Boolean(instance.archivable));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(comp) = world.entity(entity).get::<BrickColorValue>() {
            let mut props = toml::value::Table::new();
            props.insert("value".to_string(), toml::Value::Integer(comp.value as i64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::RobloxPropertyValue;

    #[test]
    fn class_name_is_brick_color_value() {
        assert_eq!(BrickColorValueSpawner.class_name(), ClassName::BrickColorValue);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = BrickColorValueSpawner;
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_roblox_reads_index() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str { "BrickColorValue" }
            fn name(&self) -> &str { "TeamColor" }
            fn property(&self, key: &str) -> Option<RobloxPropertyValue> {
                match key {
                    "Value" => Some(RobloxPropertyValue::Int(21)), // Bright red
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> { Vec::new() }
            fn referent(&self) -> u64 { 1 }
        }
        let bag = BrickColorValueSpawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_i32("value"), Some(21));
    }

    #[test]
    fn import_from_toml_reads_value() {
        let toml_src = r#"
            [metadata]
            class_name = "BrickColorValue"
            name = "Hue"
            [properties]
            value = 1004
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BrickColorValueSpawner.import_from_toml(&value);
        assert_eq!(bag.get_i32("value"), Some(1004));
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(BrickColorValueSpawner.deserialize(&[]).is_empty());
    }

    #[test]
    fn resolved_srgb_matches_palette_table() {
        // The convenience wrapper must agree with the common palette fn for the
        // default index (194 = "Medium stone grey").
        assert_eq!(
            BrickColorValueSpawner::resolved_srgb(194),
            eustress_common::brick_palette::srgb_for_index(194),
        );
    }
}
