//! `FolderSpawner` — minimal organizational-container spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::Folder`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.1 (containers row).
//!
//! ## What this is
//!
//! Folder is a **pure passthrough container**: no mesh, no physics, no
//! material, no visual. The Roblox equivalent is `Folder`. Children are
//! parented to the Folder and inherit transform / domain configuration;
//! the Folder itself contributes only hierarchy + tags + attributes.
//!
//! Mirror of the legacy `spawn::spawn_folder` helper at
//! `crates/engine/src/spawn.rs:313`. Bundle attached:
//!
//! - [`Transform`] (identity by default — containers have no position
//!   intrinsics; children carry their own transforms)
//! - [`Visibility`] (default — visibility is inherited by children)
//! - [`Instance`] (with `class_name = ClassName::Folder` and the
//!   `metadata.name` from the bag)
//! - [`Folder`] component (default — `assembly_mass` is computed by the
//!   recursive-mass system, not authored)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//! - [`Attributes`] (empty — populated by Wave 5+ attribute reader)
//! - [`Tags`] (empty — populated by Wave 5+ tag reader)
//!
//! ## Why no LOD
//!
//! Per spec §9 + LOOP-3 breaker in `docs/process/AGENT_DISPATCH.md`:
//! containers carry no LOD model. Children inherit LOD via their own
//! spawners. [`lod_components`](FolderSpawner::lod_components) returns
//! [`ComponentBundle::empty`] for every tier — the transition system
//! short-circuits on empty bundles per `lod.rs:99`.
//!
//! ## Wave 3.E scope
//!
//! This is the **minimal viable spawner** — just enough to bring up the
//! container half of the registry. The Folder's `DomainSyncConfig` (the
//! sync_config field that drives data-driven entity layout) is NOT yet
//! threaded through the PropertyBag — that's a Wave 3+ extension when
//! the parameters service is registry-aware. Right now the spawner
//! attaches `Folder::default()`, which leaves `assembly_mass = 0.0` for
//! the existing `update_assembly_mass_system` to populate.
//!
//! ## Persistence (`serialize` / `deserialize`)
//!
//! Wave 3.E ships **stub persistence**: empty byte vector out, empty bag
//! in. Folder state is entirely derivable from its `_instance.toml` and
//! its hierarchy — no rkyv mirror is needed until Wave 4 lights up the
//! Fjall write path for containers. Per spec §10 R9 the empty path is
//! safe: the worlddb write path skips classes without a registered
//! spawner; once Wave 4 needs Folder bytes, the
//! [`serialize`](FolderSpawner::serialize) /
//! [`deserialize`](FolderSpawner::deserialize) pair lights up via a
//! rkyv `ArchFolder` mirror.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Folder, Instance, PropertyValue};
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::Folder`].
///
/// State-less by design — `ClassSpawner` requires `Send + Sync + 'static`
/// (see common-side trait doc), so spawners are recipe holders, not
/// state. Per-spawn mutability flows through [`SpawnCtx`].
#[derive(Default)]
pub struct FolderSpawner;

impl ClassSpawner for FolderSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Folder
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        // The bag carries `metadata.name` via the canonical key set by
        // every importer (see `class_registry::PropertyBag::get_uuid`
        // for the parallel `metadata.uuid` accessor — Wave 2.1
        // groundwork). Fall back to "Folder" when the bag is empty so
        // hot-create via the Insert menu (no source TOML yet) still
        // produces a labelled entity.
        let name = props
            .get_string("metadata.name")
            .unwrap_or("Folder")
            .to_string();

        // Optional UUID provenance from Wave 2.1 — we stamp it on the
        // Instance when present so the worlddb's `uuid_to_path` reverse
        // index resolves this entity correctly. Empty string when
        // absent (matches `Instance::default()`).
        let uuid = props.get_uuid().unwrap_or_default().to_string();

        // Archivable defaults to true (same as `Instance::default()` and
        // the legacy `spawn_folder` path) unless explicitly overridden
        // — Roblox Studio writes `archivable = false` for some
        // editor-only folders.
        let archivable = props
            .get_bool("metadata.archivable")
            .unwrap_or(true);

        // Mirror of `spawn::spawn_folder` at `spawn.rs:313`. Component
        // order matches the legacy helper to preserve Bevy archetype
        // identity across the registry/legacy migration window (spec
        // §13 R3 — parity-test risk).
        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::Folder,
                    archivable,
                    id: 0, // assigned by post-spawn id system
                    uuid,
                    ai: false,
                },
                Folder::default(),
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // See module docs — Wave 3.E ships stub persistence. Wave 4 fills
        // in a tagged rkyv archive once the Fjall write path opts in for
        // containers.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        // Inverse of `serialize` — empty in, empty out for the Wave 3.E
        // stub. The empty bag round-trips through `spawn` as a default
        // Folder; matches the "missing source data → safe default"
        // contract spec §2.1 documents for the trait.
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Cheap mutations only — Folder has no respawn-required props.
        // The two writable surface props are `metadata.name` and
        // `metadata.archivable`; both live on the `Instance` component
        // and can be mutated in place.
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                if let Some(new_name) = props.get_string("metadata.name") {
                    instance.name = new_name.to_string();
                }
                if let Some(archivable) = props.get_bool("metadata.archivable") {
                    instance.archivable = archivable;
                }
            }

            // `Name` mirrors `Instance.name` for Bevy's debug overlay and
            // the Inspector; keep them in lockstep here so the
            // Properties panel doesn't need a separate sync system.
            if let Some(new_name) = props.get_string("metadata.name") {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(new_name.to_string());
                }
            }
        }

        false // never needs respawn
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Containers have no LOD model — children inherit. Returning an
        // empty bundle short-circuits the apply_lod_transitions system
        // (see `lod.rs:99` ComponentBundle::is_empty).
        //
        // Same bundle for all four tiers (Hero, Active, Streamed,
        // Horizon) — deliverable #2 from the task spec.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Folder maps 1:1 between Roblox and Eustress — the class has
        // no Roblox-specific properties beyond Instance basics
        // (`Name`, `Archivable`). Map both into the canonical key
        // space defined by `class_schema/Folder/_instance.toml`.
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(archivable) = rbx.property("Archivable").and_then(|p| p.as_bool()) {
            bag.set("metadata.archivable", PropertyValue::Bool(archivable));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the `class_schema/Folder/_instance.toml` shape:
        //
        //     [metadata]
        //     class_name = "Folder"
        //     name = "MyFolder"
        //     archivable = true
        //     uuid = "01234567-89ab-..."   # Wave 2.1
        //
        // Keys are emitted in template order per spec §4.3 — same order
        // `export_to_toml` writes them back.
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
        // TOML stays byte-stable across reloads (Fjall change detection
        // + git diff hygiene per spec §4 module doc).
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        // The `class_name` is always written — needed by the file_loader
        // dispatch to route TOML → spawner without name-sniffing.
        meta.insert(
            "class_name".to_string(),
            toml::Value::String("Folder".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert(
                "name".to_string(),
                toml::Value::String(instance.name.clone()),
            );
            meta.insert(
                "archivable".to_string(),
                toml::Value::Boolean(instance.archivable),
            );
            if !instance.uuid.is_empty() {
                meta.insert(
                    "uuid".to_string(),
                    toml::Value::String(instance.uuid.clone()),
                );
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
    /// `register()` panics on mismatch — this test catches the
    /// regression at unit-test time instead of plugin-build time.
    #[test]
    fn class_name_is_folder() {
        let spawner = FolderSpawner;
        assert_eq!(spawner.class_name(), ClassName::Folder);
    }

    /// All four LOD tiers return empty bundles — containers have no
    /// LOD model. Deliverable #2 from the task spec.
    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = FolderSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            let bundle = spawner.lod_components(tier);
            assert!(
                bundle.is_empty(),
                "FolderSpawner must return empty LOD bundle at {} — \
                 containers have no LOD model (children inherit)",
                tier.as_str()
            );
        }
    }

    /// `import_from_toml` reads the canonical key space.
    #[test]
    fn import_from_toml_reads_metadata_section() {
        let toml_src = r#"
            [metadata]
            class_name = "Folder"
            name = "ParkBenches"
            archivable = true
            uuid = "01234567-89ab-cdef-0123-456789abcdef"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let spawner = FolderSpawner;
        let bag = spawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("ParkBenches"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
        assert_eq!(
            bag.get_string("metadata.uuid"),
            Some("01234567-89ab-cdef-0123-456789abcdef")
        );
    }

    /// `import_from_roblox` maps Roblox `Instance.Name` + `Archivable`
    /// into the canonical Eustress key space.
    #[test]
    fn import_from_roblox_maps_basics() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str {
                "Folder"
            }
            fn name(&self) -> &str {
                "RobloxFolder"
            }
            fn property(
                &self,
                key: &str,
            ) -> Option<eustress_common::class_registry::RobloxPropertyValue> {
                match key {
                    "Archivable" => {
                        Some(eustress_common::class_registry::RobloxPropertyValue::Bool(false))
                    }
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> {
                Vec::new()
            }
            fn referent(&self) -> u64 {
                42
            }
        }

        let spawner = FolderSpawner;
        let bag = spawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("metadata.name"), Some("RobloxFolder"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(false));
    }

    /// Empty TOML input produces an empty bag — matches the safe-default
    /// contract spec §2.1 documents.
    #[test]
    fn import_from_toml_empty_returns_empty_bag() {
        let value: toml::Value = toml::from_str("").unwrap();
        let spawner = FolderSpawner;
        let bag = spawner.import_from_toml(&value);
        assert!(bag.is_empty());
    }

    /// Stub `serialize` emits no bytes; the matching `deserialize`
    /// produces an empty bag. Wave 4 lights up the real archive — this
    /// test pins the stub shape so a future commit doesn't accidentally
    /// drop the contract mid-migration.
    #[test]
    fn stub_persistence_round_trips_through_empty() {
        let spawner = FolderSpawner;
        let bag = spawner.deserialize(&[]);
        assert!(bag.is_empty());
    }
}
