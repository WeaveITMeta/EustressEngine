//! `ModelSpawner` — Model container spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::Model`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.1 (containers row).
//!
//! ## What this is
//!
//! Model is a **container with a primary part reference and a world
//! pivot**. The Roblox equivalent is `Model`. Unlike [`Folder`], Model
//! tracks two transform-relevant fields:
//!
//! - `primary_part` — the entity whose pose anchors the group transform.
//!   Roblox calls this `Model.PrimaryPart`; the field is the link the
//!   game scripts use to move the whole assembly via
//!   `Model:SetPrimaryPartCFrame()`.
//! - `world_pivot` — the computed group pose. Roblox calls this
//!   `Model.WorldPivot`; it's the "where the model thinks it is" used by
//!   the editor's Pivot tool.
//!
//! Children are parented to the Model and inherit transform / domain
//! configuration; the Model itself contributes hierarchy + tags +
//! attributes + the primary-part/world-pivot pair.
//!
//! Mirror of the legacy `spawn::spawn_model` helper at
//! `crates/engine/src/spawn.rs:295`. Bundle attached:
//!
//! - [`Transform`] (identity by default — the `world_pivot` is the
//!   intentional model pose; per-frame sync between `world_pivot` and
//!   `Transform` is a Wave 4 system)
//! - [`Visibility`] (default — visibility is inherited by children)
//! - [`Instance`] (with `class_name = ClassName::Model` and the
//!   `metadata.name` from the bag)
//! - [`Model`] component with `primary_part` resolved from the bag's
//!   `model.primary_part_uuid` (when present) and `world_pivot` from the
//!   bag's `model.world_pivot` (when present)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//! - [`Attributes`] (empty — populated by Wave 5+ attribute reader)
//! - [`Tags`] (empty — populated by Wave 5+ tag reader)
//!
//! ## PrimaryPart — UUID-keyed reference
//!
//! Per task spec deliverable #3: Model "preserves PrimaryPart reference
//! (UUID-based via Wave 2.1's reverse index)". The PropertyBag carries
//! the primary part's stable UUID under `model.primary_part_uuid`; the
//! `Model.primary_part: Option<u32>` field on the component stores the
//! Bevy `Entity::index()` *after* the worlddb's `uuid_to_path` reverse
//! index resolves the reference.
//!
//! In Wave 3.E the resolution path is **deferred**: the spawner stamps
//! `None` on `Model.primary_part` and stashes the UUID string in the
//! PropertyBag for a Wave 4+ post-load pass to resolve. This matches
//! the existing `spawn::spawn_model` shape (which already takes a
//! `Model { primary_part, world_pivot, assembly_mass }` argument without
//! validating that the primary_part entity exists) and avoids the
//! chicken-and-egg problem where the primary part may not yet be
//! spawned at the moment the Model is spawned.
//!
//! The deferred-resolution contract:
//!
//! 1. Spawner stores UUID string in `props.get_string("model.primary_part_uuid")`.
//! 2. Spawner leaves `Model.primary_part = None`.
//! 3. Wave 4+ system walks Model entities, looks up the UUID via
//!    `WorldDb::lookup_path_by_uuid`, finds the entity via the existing
//!    `path_to_entity` map, and writes the entity bits into
//!    `Model.primary_part`.
//!
//! This is the same dance constraint resolution uses for
//! `Part0`/`Part1` references; documenting it here keeps Wave 4 on
//! pattern.
//!
//! ## WorldPivot
//!
//! `model.world_pivot` round-trips as a [`PropertyValue::Transform`]
//! — `Transform` is already a `PropertyBag` primitive (see
//! `class_registry::PropertyBag::get_transform`). Default is
//! [`Transform::IDENTITY`] when absent.
//!
//! ## Why no LOD
//!
//! Same as Folder — containers carry no LOD model. Children inherit LOD
//! via their own spawners. Per spec §9 + LOOP-3 breaker in
//! `docs/process/AGENT_DISPATCH.md`.
//! [`lod_components`](ModelSpawner::lod_components) returns
//! [`ComponentBundle::empty`] for every tier.
//!
//! ## Wave 3.E scope
//!
//! Minimal viable spawner — `assembly_mass` is computed by the existing
//! `update_assembly_mass_system` (it walks the hierarchy, not the bag),
//! so the spawner attaches `Model::default()` with `world_pivot` and
//! `primary_part` patched in from the bag. Domain configuration (the
//! `Parameters` component that turns a Model into a domain scope) is
//! NOT yet threaded — that's the parameters service's job once it's
//! registry-aware.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, Model, PropertyValue};
use eustress_common::{Attributes, Tags};

/// Zero-sized spawner for [`ClassName::Model`].
///
/// Holds no state — see [`super::folder::FolderSpawner`] for the same
/// rationale. Per-spawn mutability flows through [`SpawnCtx`].
#[derive(Default)]
pub struct ModelSpawner;

impl ClassSpawner for ModelSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Model
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        // Same key set as Folder for the common Instance fields.
        let name = props
            .get_string("metadata.name")
            .unwrap_or("Model")
            .to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props
            .get_bool("metadata.archivable")
            .unwrap_or(true);

        // Model-specific fields. PrimaryPart is left unresolved at
        // spawn time — see module docs for the deferred resolution
        // contract. WorldPivot defaults to identity when absent (matches
        // the legacy `Model::default()` shape).
        let world_pivot = props
            .get_transform("model.world_pivot")
            .copied()
            .unwrap_or(Transform::IDENTITY);

        let model_component = Model {
            // Wave 3.E: always None — Wave 4+ resolution pass reads the
            // UUID string from `model.primary_part_uuid` and writes the
            // entity bits here. See module docs.
            primary_part: None,
            world_pivot,
            // `assembly_mass` is computed by the existing
            // `update_assembly_mass_system` from the hierarchy — never
            // authored, never persisted. Spawner leaves the default
            // (0.0); the system fills it on the next tick.
            assembly_mass: 0.0,
        };

        // Mirror of `spawn::spawn_model` at `spawn.rs:295`. Component
        // order matches the legacy helper to preserve Bevy archetype
        // identity across the registry/legacy migration window (spec
        // §13 R3 — parity-test risk).
        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::Model,
                    archivable,
                    id: 0, // assigned by post-spawn id system
                    uuid,
                    ai: false,
                },
                model_component,
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // See `folder::FolderSpawner::serialize` — same Wave 3.E stub.
        // Wave 4 fills in a tagged rkyv archive (ArchModel) carrying
        // primary_part_uuid + world_pivot; assembly_mass stays
        // hierarchy-derived and is not persisted.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Cheap mutations only — Model has no respawn-required props.
        // - `metadata.name` + `metadata.archivable` → Instance in place.
        // - `model.world_pivot` → Model.world_pivot in place.
        // - `model.primary_part_uuid` → handled by Wave 4+ resolver;
        //   the spawner stamps None until then. An apply_edit that
        //   updates the UUID also stamps None and lets the resolver
        //   re-link on its next tick.
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                if let Some(new_name) = props.get_string("metadata.name") {
                    instance.name = new_name.to_string();
                }
                if let Some(archivable) = props.get_bool("metadata.archivable") {
                    instance.archivable = archivable;
                }
            }

            if let Some(mut model) = entity_mut.get_mut::<Model>() {
                if let Some(world_pivot) = props.get_transform("model.world_pivot") {
                    model.world_pivot = *world_pivot;
                }
                // PrimaryPart UUID change → mark as unresolved. The
                // Wave 4+ resolver picks it up on next tick.
                if props.get_string("model.primary_part_uuid").is_some() {
                    model.primary_part = None;
                }
            }

            // Mirror `Instance.name` into Bevy's `Name` for the
            // Inspector + debug overlay.
            if let Some(new_name) = props.get_string("metadata.name") {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(new_name.to_string());
                }
            }
        }

        false // never needs respawn
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Containers have no LOD model — children inherit. Same
        // contract as FolderSpawner. Deliverable #2 from the task
        // spec.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Roblox Model props mapped into the canonical Eustress key
        // space:
        //
        //     Name        → metadata.name
        //     Archivable  → metadata.archivable
        //     PrimaryPart → model.primary_part_uuid (referent resolved
        //                   to Eustress UUID by the importer pipeline;
        //                   see ROBLOX_IMPORT_SPEC.md §3 referent map).
        //                   At Wave 3.E the importer stub passes through
        //                   the raw referent as a string; Wave 4 swaps
        //                   in the real UUID after the full tree spawns.
        //     WorldPivot  → model.world_pivot (Roblox CFrame → Transform
        //                   via the importer's CFrame adapter; Wave 4).
        let mut bag = PropertyBag::with_capacity(4);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(archivable) = rbx.property("Archivable").and_then(|p| p.as_bool()) {
            bag.set("metadata.archivable", PropertyValue::Bool(archivable));
        }
        // PrimaryPart and WorldPivot are richer types than the Wave 2
        // `RobloxPropertyValue` stub carries; the importer (Wave 4)
        // populates them via a richer adapter. The empty bag entry is
        // safe — the spawner falls back to None / identity.
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the `class_schema/Model/_instance.toml` shape:
        //
        //     [metadata]
        //     class_name = "Model"
        //     name = "MyAssembly"
        //     archivable = true
        //     uuid = "..."                              # Wave 2.1
        //
        //     [model]
        //     primary_part_uuid = "..."                 # Wave 2.1
        //     world_pivot = { position = [0,0,0],
        //                     rotation = [0,0,0,1],
        //                     scale = [1,1,1] }
        //
        // Keys are emitted in template order per spec §4.3.
        let mut bag = PropertyBag::with_capacity(5);

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

        if let Some(model_section) = toml_value.get("model") {
            if let Some(primary_uuid) =
                model_section.get("primary_part_uuid").and_then(|v| v.as_str())
            {
                bag.set(
                    "model.primary_part_uuid",
                    PropertyValue::String(primary_uuid.to_string()),
                );
            }
            if let Some(pivot) = model_section.get("world_pivot") {
                if let Some(transform) = parse_world_pivot_table(pivot) {
                    bag.set("model.world_pivot", PropertyValue::Transform(transform));
                }
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        // Inverse of `import_from_toml`. Same key order — byte-stable
        // round-trip (spec §4 module doc).
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();
        let mut model_section = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("Model".to_string()),
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

        if let Some(model) = world.entity(entity).get::<Model>() {
            // PrimaryPart UUID is NOT round-tripped from the live
            // entity at Wave 3.E — the resolver writes
            // `Model.primary_part = Some(entity_index)`, which is a
            // process-local Bevy id and cannot survive a save. The
            // canonical primary_part_uuid lives in the on-disk TOML
            // and is preserved by the file_loader's load-merge path
            // (see `project_save_space_loadmerge.md`). Wave 4+ stamps
            // the UUID into a parallel component
            // (`PrimaryPartUuid(String)`) at resolve time and
            // re-emits it here.
            //
            // For now: only emit the world_pivot. The load-merge
            // contract preserves the on-disk `primary_part_uuid`
            // through save round-trips.

            // world_pivot is always written — identity when no pivot
            // has been set. That preserves the round-trip shape even
            // for hot-created Models.
            let mut pivot = toml::value::Table::new();
            pivot.insert(
                "position".to_string(),
                toml::Value::Array(vec![
                    toml::Value::Float(model.world_pivot.translation.x as f64),
                    toml::Value::Float(model.world_pivot.translation.y as f64),
                    toml::Value::Float(model.world_pivot.translation.z as f64),
                ]),
            );
            pivot.insert(
                "rotation".to_string(),
                toml::Value::Array(vec![
                    toml::Value::Float(model.world_pivot.rotation.x as f64),
                    toml::Value::Float(model.world_pivot.rotation.y as f64),
                    toml::Value::Float(model.world_pivot.rotation.z as f64),
                    toml::Value::Float(model.world_pivot.rotation.w as f64),
                ]),
            );
            pivot.insert(
                "scale".to_string(),
                toml::Value::Array(vec![
                    toml::Value::Float(model.world_pivot.scale.x as f64),
                    toml::Value::Float(model.world_pivot.scale.y as f64),
                    toml::Value::Float(model.world_pivot.scale.z as f64),
                ]),
            );
            model_section
                .insert("world_pivot".to_string(), toml::Value::Table(pivot));
        }

        root.insert("metadata".to_string(), toml::Value::Table(meta));
        if !model_section.is_empty() {
            root.insert("model".to_string(), toml::Value::Table(model_section));
        }
        toml::Value::Table(root)
    }
}

/// Parse a `world_pivot = { position = [...], rotation = [...], scale = [...] }`
/// table into a [`Transform`]. Missing components default to identity —
/// matches the "safe default" contract spec §2.1 documents for the
/// trait.
fn parse_world_pivot_table(value: &toml::Value) -> Option<Transform> {
    let table = value.as_table()?;
    let position = read_vec3(table.get("position")?).unwrap_or(Vec3::ZERO);
    let rotation = read_quat(table.get("rotation")).unwrap_or(Quat::IDENTITY);
    let scale = read_vec3(table.get("scale").unwrap_or(&toml::Value::Array(vec![])))
        .unwrap_or(Vec3::ONE);
    Some(Transform {
        translation: position,
        rotation,
        scale,
    })
}

fn read_vec3(value: &toml::Value) -> Option<Vec3> {
    let arr = value.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    Some(Vec3::new(
        arr[0].as_float().unwrap_or(0.0) as f32,
        arr[1].as_float().unwrap_or(0.0) as f32,
        arr[2].as_float().unwrap_or(0.0) as f32,
    ))
}

fn read_quat(value: Option<&toml::Value>) -> Option<Quat> {
    let arr = value?.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    Some(Quat::from_xyzw(
        arr[0].as_float().unwrap_or(0.0) as f32,
        arr[1].as_float().unwrap_or(0.0) as f32,
        arr[2].as_float().unwrap_or(0.0) as f32,
        arr[3].as_float().unwrap_or(1.0) as f32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `class_name()` returns the registered key. Catches mismatch at
    /// unit-test time rather than registration-panic time.
    #[test]
    fn class_name_is_model() {
        let spawner = ModelSpawner;
        assert_eq!(spawner.class_name(), ClassName::Model);
    }

    /// All four LOD tiers return empty bundles — containers have no
    /// LOD model. Deliverable #2 from the task spec.
    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = ModelSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            let bundle = spawner.lod_components(tier);
            assert!(
                bundle.is_empty(),
                "ModelSpawner must return empty LOD bundle at {} — \
                 containers have no LOD model (children inherit)",
                tier.as_str()
            );
        }
    }

    /// `import_from_toml` reads the canonical Model key space — the
    /// `[metadata]` + `[model]` sections.
    #[test]
    fn import_from_toml_reads_model_section() {
        let toml_src = r#"
            [metadata]
            class_name = "Model"
            name = "Vehicle"
            archivable = true
            uuid = "01234567-89ab-cdef-0123-456789abcdef"

            [model]
            primary_part_uuid = "fedcba98-7654-3210-fedc-ba9876543210"
            world_pivot = { position = [1.0, 2.0, 3.0], rotation = [0.0, 0.0, 0.0, 1.0], scale = [1.0, 1.0, 1.0] }
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let spawner = ModelSpawner;
        let bag = spawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("Vehicle"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
        assert_eq!(
            bag.get_string("model.primary_part_uuid"),
            Some("fedcba98-7654-3210-fedc-ba9876543210")
        );
        let pivot = bag.get_transform("model.world_pivot").unwrap();
        assert_eq!(pivot.translation, Vec3::new(1.0, 2.0, 3.0));
    }

    /// Missing `[model]` section produces an empty model side of the
    /// bag — spawner falls back to the safe defaults (None,
    /// identity).
    #[test]
    fn import_from_toml_missing_model_section_safe_defaults() {
        let toml_src = r#"
            [metadata]
            class_name = "Model"
            name = "BareModel"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let spawner = ModelSpawner;
        let bag = spawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("BareModel"));
        assert!(bag.get_string("model.primary_part_uuid").is_none());
        assert!(bag.get_transform("model.world_pivot").is_none());
    }

    /// `parse_world_pivot_table` handles partial tables gracefully —
    /// missing rotation defaults to identity, missing scale to ONE.
    #[test]
    fn parse_world_pivot_handles_partial_table() {
        let value: toml::Value = toml::from_str(
            r#"
            position = [5.0, 0.0, 0.0]
        "#,
        )
        .unwrap();
        let pivot = parse_world_pivot_table(&value).unwrap();
        assert_eq!(pivot.translation, Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(pivot.rotation, Quat::IDENTITY);
        assert_eq!(pivot.scale, Vec3::ONE);
    }

    /// `import_from_roblox` maps Roblox `Instance.Name` + `Archivable`
    /// into the canonical Eustress key space. PrimaryPart + WorldPivot
    /// are richer types not in the Wave 2 stub — Wave 4 fills those in.
    #[test]
    fn import_from_roblox_maps_basics() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str {
                "Model"
            }
            fn name(&self) -> &str {
                "RobloxModel"
            }
            fn property(
                &self,
                key: &str,
            ) -> Option<eustress_common::class_registry::RobloxPropertyValue> {
                match key {
                    "Archivable" => {
                        Some(eustress_common::class_registry::RobloxPropertyValue::Bool(true))
                    }
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> {
                Vec::new()
            }
            fn referent(&self) -> u64 {
                7
            }
        }

        let spawner = ModelSpawner;
        let bag = spawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("metadata.name"), Some("RobloxModel"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
    }

    /// Stub `serialize` emits no bytes; `deserialize` produces an
    /// empty bag. Wave 4 lights up the real archive.
    #[test]
    fn stub_persistence_round_trips_through_empty() {
        let spawner = ModelSpawner;
        let bag = spawner.deserialize(&[]);
        assert!(bag.is_empty());
    }
}
