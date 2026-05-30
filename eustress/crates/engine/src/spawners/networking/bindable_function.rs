//! `BindableFunctionSpawner` — minimal networking-signal-container spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::BindableFunction`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.11 (networking row).
//!
//! ## What this is
//!
//! `BindableFunction` is an **empty signal container**: an in-process
//! request-response (synchronous call) within the same execution context.
//! It does NOT cross the network boundary. The Roblox equivalent is
//! `BindableFunction`. The entity carries no mesh, no physics, no
//! material, no visual — it exists purely so the Luau runtime can resolve
//! a named in-process call against it.
//!
//! The spawner attaches the data-only side — the [`BindableFunction`]
//! component (`name` + `enabled` + diagnostic `invoke_count`). The live
//! `OnInvoke` callback (in
//! [`eustress_common::scripting::events::BindableFunction`]) is wired by
//! the Luau bridge at runtime, NOT authored here.
//!
//! Bundle attached (mirrors the `FolderSpawner` container pattern at
//! `spawners/containers/folder.rs`):
//!
//! - [`Transform`] (identity)
//! - [`Visibility`] (default — never rendered)
//! - [`Instance`] (with `class_name = ClassName::BindableFunction` and
//!   the `metadata.name` from the bag)
//! - [`BindableFunction`] component (with `name` mirroring `Instance.name`;
//!   `enabled = true` and `invoke_count = 0` from `Default`)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//! - [`Attributes`] (empty — Wave 5+ reader)
//! - [`Tags`] (empty — Wave 5+ reader)
//!
//! ## Why no LOD / Persistence
//!
//! Identical rationale to [`RemoteEventSpawner`]: no LOD model (empty
//! bundle every tier), stub persistence (empty bytes / empty bag) until
//! a later wave lights up the Fjall write path for signal classes.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue};
// Data-only Component `BindableFunction` (NOT the crate-root signal-object
// re-export from `scripting::events`). See `remote_event.rs` module docs
// for why the explicit `luau::components` path is required.
use eustress_common::luau::components::BindableFunction;
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::BindableFunction`].
#[derive(Default)]
pub struct BindableFunctionSpawner;

impl ClassSpawner for BindableFunctionSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BindableFunction
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("BindableFunction")
            .to_string();

        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        let signal = BindableFunction {
            name: name.clone(),
            ..Default::default()
        };

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::BindableFunction,
                    archivable,
                    id: 0, // assigned by post-spawn id system
                    uuid,
                    ai: false,
                },
                signal,
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

            if let Some(mut signal) = entity_mut.get_mut::<BindableFunction>() {
                if let Some(ref n) = new_name {
                    signal.name = n.clone();
                }
                if let Some(enabled) = props.get_bool("enabled") {
                    signal.enabled = enabled;
                }
            }

            if let Some(ref n) = new_name {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(n.clone());
                }
            }
        }

        false // never needs respawn
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(archivable) = rbx.property("Archivable").and_then(|p| p.as_bool()) {
            bag.set("metadata.archivable", PropertyValue::Bool(archivable));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);

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

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("BindableFunction".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert(
                "archivable".to_string(),
                toml::Value::Boolean(instance.archivable),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }

        root.insert("metadata".to_string(), toml::Value::Table(meta));
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_bindable_function() {
        assert_eq!(
            BindableFunctionSpawner.class_name(),
            ClassName::BindableFunction
        );
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = BindableFunctionSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            assert!(
                spawner.lod_components(tier).is_empty(),
                "BindableFunctionSpawner must return empty LOD bundle at {}",
                tier.as_str()
            );
        }
    }

    #[test]
    fn import_from_toml_reads_metadata_section() {
        let toml_src = r#"
            [metadata]
            class_name = "BindableFunction"
            name = "ComputeScore"
            archivable = true
            uuid = "01234567-89ab-cdef-0123-456789abcdef"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = BindableFunctionSpawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("ComputeScore"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
        assert_eq!(
            bag.get_string("metadata.uuid"),
            Some("01234567-89ab-cdef-0123-456789abcdef")
        );
    }

    #[test]
    fn import_from_toml_empty_returns_empty_bag() {
        let value: toml::Value = toml::from_str("").unwrap();
        assert!(BindableFunctionSpawner.import_from_toml(&value).is_empty());
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(BindableFunctionSpawner.deserialize(&[]).is_empty());
    }
}
