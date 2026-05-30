//! `RemoteEventSpawner` — minimal networking-signal-container spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::RemoteEvent`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.11 (networking row).
//!
//! ## What this is
//!
//! `RemoteEvent` is an **empty signal container**: a one-way event
//! channel between client and server. The Roblox equivalent is
//! `RemoteEvent`. The entity carries no mesh, no physics, no material,
//! no visual — it exists purely so the Luau runtime can resolve a named
//! signal against it (`game.ReplicatedStorage.MyEvent:FireServer(...)`).
//!
//! The spawner attaches the data-only side of the signal — the
//! [`RemoteEvent`] component (`name` + `enabled` + diagnostic
//! `fire_count`). The live signal plumbing (the `on_server_event` /
//! `on_client_event` channels in
//! [`eustress_common::scripting::events::RemoteEvent`]) is wired up by
//! the Luau bridge at runtime, NOT authored here. This mirrors how the
//! engine separates the persisted ECS component from the runtime signal
//! object across the whole networking subsystem.
//!
//! Bundle attached (mirrors the `FolderSpawner` container pattern at
//! `spawners/containers/folder.rs`):
//!
//! - [`Transform`] (identity — signal containers have no position
//!   intrinsics; they sit in the hierarchy under a service like
//!   `ReplicatedStorage`)
//! - [`Visibility`] (default — never rendered, but required for Bevy's
//!   spatial bundle invariants)
//! - [`Instance`] (with `class_name = ClassName::RemoteEvent` and the
//!   `metadata.name` from the bag)
//! - [`RemoteEvent`] component (with `name` mirroring `Instance.name`;
//!   `enabled = true` and `fire_count = 0` from `Default`)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//! - [`Attributes`] (empty — populated by Wave 5+ attribute reader)
//! - [`Tags`] (empty — populated by Wave 5+ tag reader)
//!
//! ## Why no LOD
//!
//! Per spec §9 + LOOP-3 breaker in `docs/process/AGENT_DISPATCH.md`:
//! signal containers carry no LOD model — there is nothing to render at
//! any tier. [`lod_components`](RemoteEventSpawner::lod_components)
//! returns [`ComponentBundle::empty`] for every tier; the transition
//! system short-circuits on empty bundles.
//!
//! ## Persistence (`serialize` / `deserialize`)
//!
//! Wave 5.B ships **stub persistence**: empty byte vector out, empty bag
//! in. A `RemoteEvent`'s authored state is entirely its name + hierarchy
//! position, both derivable from its `_instance.toml`; no rkyv mirror is
//! needed until a later wave lights up the Fjall write path for signal
//! classes. Per the trait contract (spec §2.1) the empty path is safe —
//! an empty bag round-trips through `spawn` as a default `RemoteEvent`.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue};
// The Component (data-only) `RemoteEvent` lives in `luau::components`.
// NOTE: the crate-root `eustress_common::RemoteEvent` re-export resolves
// to the *runtime signal object* in `scripting::events` (not a Bevy
// `Component`); we deliberately reach for the `luau::components` path so
// `spawn` attaches the persistable ECS component, not the signal plumbing.
use eustress_common::luau::components::RemoteEvent;
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::RemoteEvent`].
///
/// State-less by design — `ClassSpawner` requires `Send + Sync + 'static`,
/// so spawners are recipe holders, not state. Per-spawn mutability flows
/// through [`SpawnCtx`].
#[derive(Default)]
pub struct RemoteEventSpawner;

impl ClassSpawner for RemoteEventSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::RemoteEvent
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        // The bag carries `metadata.name` via the canonical key set by
        // every importer. Fall back to "RemoteEvent" when the bag is
        // empty so hot-create via the Insert menu (no source TOML yet)
        // still produces a labelled entity.
        let name = props
            .get_string("metadata.name")
            .unwrap_or("RemoteEvent")
            .to_string();

        // Optional UUID provenance (Wave 2.1) — stamped on the Instance
        // when present so the worlddb's `uuid_to_path` reverse index
        // resolves this entity. Empty string when absent.
        let uuid = props.get_uuid().unwrap_or_default().to_string();

        // Archivable defaults to true (matches `Instance::default()`)
        // unless explicitly overridden.
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        // The signal component mirrors `Instance.name` so scripts that
        // look the channel up by name resolve to the same string the
        // Explorer shows. `enabled`/`fire_count` come from `Default`.
        let signal = RemoteEvent {
            name: name.clone(),
            ..Default::default()
        };

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::RemoteEvent,
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
        // Stub persistence — see module docs. A later wave fills in a
        // tagged rkyv archive once the Fjall write path opts in for
        // signal classes.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        // Inverse of `serialize` — empty in, empty out. The empty bag
        // round-trips through `spawn` as a default `RemoteEvent`; matches
        // the "missing source data → safe default" contract (spec §2.1).
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Cheap mutations only — `RemoteEvent` has no respawn-required
        // props. Writable surface props: `metadata.name`,
        // `metadata.archivable`, and `enabled`; all mutate in place.
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

            if let Some(mut signal) = entity_mut.get_mut::<RemoteEvent>() {
                if let Some(ref n) = new_name {
                    signal.name = n.clone();
                }
                if let Some(enabled) = props.get_bool("enabled") {
                    signal.enabled = enabled;
                }
            }

            // `Name` mirrors `Instance.name` for Bevy's debug overlay and
            // the Inspector; keep them in lockstep.
            if let Some(ref n) = new_name {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(n.clone());
                }
            }
        }

        false // never needs respawn
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Signal containers have no LOD model — nothing renders at any
        // tier. Returning an empty bundle short-circuits the
        // apply_lod_transitions system. Same bundle for all four tiers.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // RemoteEvent maps 1:1 between Roblox and Eustress — the class
        // has no Roblox-specific properties beyond Instance basics
        // (`Name`, `Archivable`). Wave fills richer mapping if Roblox
        // ever exposes per-event config.
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(archivable) = rbx.property("Archivable").and_then(|p| p.as_bool()) {
            bag.set("metadata.archivable", PropertyValue::Bool(archivable));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the `class_schema/RemoteEvent/_instance.toml` shape:
        //
        //     [metadata]
        //     class_name = "RemoteEvent"
        //     name = "DamageEvent"
        //     archivable = true
        //     uuid = "01234567-89ab-..."   # Wave 2.1
        //
        // Keys emitted in template order per spec §4.3.
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
        // Inverse of `import_from_toml`. Same key order so the on-disk
        // TOML stays byte-stable across reloads.
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        // `class_name` is always written — needed by the file_loader
        // dispatch to route TOML → spawner without name-sniffing.
        meta.insert(
            "class_name".to_string(),
            toml::Value::String("RemoteEvent".to_string()),
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

    /// `class_name()` returns the registered key. The registry's
    /// `register()` panics on mismatch — this catches the regression at
    /// unit-test time instead of plugin-build time.
    #[test]
    fn class_name_is_remote_event() {
        let spawner = RemoteEventSpawner;
        assert_eq!(spawner.class_name(), ClassName::RemoteEvent);
    }

    /// All four LOD tiers return empty bundles — signal containers have
    /// no LOD model.
    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = RemoteEventSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            assert!(
                spawner.lod_components(tier).is_empty(),
                "RemoteEventSpawner must return empty LOD bundle at {}",
                tier.as_str()
            );
        }
    }

    /// `import_from_toml` reads the canonical key space.
    #[test]
    fn import_from_toml_reads_metadata_section() {
        let toml_src = r#"
            [metadata]
            class_name = "RemoteEvent"
            name = "DamageEvent"
            archivable = true
            uuid = "01234567-89ab-cdef-0123-456789abcdef"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = RemoteEventSpawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("DamageEvent"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
        assert_eq!(
            bag.get_string("metadata.uuid"),
            Some("01234567-89ab-cdef-0123-456789abcdef")
        );
    }

    /// Empty TOML input produces an empty bag — safe-default contract.
    #[test]
    fn import_from_toml_empty_returns_empty_bag() {
        let value: toml::Value = toml::from_str("").unwrap();
        assert!(RemoteEventSpawner.import_from_toml(&value).is_empty());
    }

    /// Stub `serialize`/`deserialize` round-trip through empty.
    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(RemoteEventSpawner.deserialize(&[]).is_empty());
    }
}
