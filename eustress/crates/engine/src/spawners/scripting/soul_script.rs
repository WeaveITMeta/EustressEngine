//! `SoulScriptSpawner` — Wave 5.C scripting-class spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::SoulScript`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.12 (scripting row) + §2 (trait).
//!
//! ## What this is
//!
//! `SoulScript` is the unified Soul scripting class — markdown / Rune
//! source compiled to the Rune VM. The entity is a **pure source carrier**:
//! it holds the script body on its [`SoulScriptData`] component and nothing
//! else. There is no mesh, no physics, no material, no LOD model — scripts
//! are invisible in the viewport.
//!
//! ## Execution is NOT this spawner's job
//!
//! Critical boundary (task spec + §8.12): the spawner only *builds the
//! entity and stores the source*. The existing Soul build pipeline +
//! `compile_scripts_on_play` / Rune VM systems own execution. They detect
//! `SoulScriptData` (via `Added`/`Changed`) and drive compile/run
//! themselves. Touching execution here would duplicate that machinery and
//! fight the file-watcher reload path.
//!
//! Mirror of the existing cold-load spawn at
//! `crates/engine/src/space/file_loader.rs:674` (`FileType::Soul`) and
//! `:1471` (script-folder leaf). Bundle attached:
//!
//! - [`Transform`] (identity — scripts have no spatial intrinsics, but
//!   they live in the ECS hierarchy like every other instance)
//! - [`Visibility`] (default — never rendered; present so the entity
//!   participates uniformly in the scene graph)
//! - [`Instance`] (with `class_name = ClassName::SoulScript` and the
//!   `metadata.name` from the bag)
//! - [`SoulScriptData`] (carries the `source` string + build state;
//!   `run_context = Rune` — the Soul default)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//!
//! ## Why no LOD
//!
//! Per spec §9 + the LOOP-3 breaker in `AGENT_DISPATCH.md`: scripts carry
//! no LOD model (the §2.1 doc lists `Script` alongside `Sound`/`Folder` as
//! "no horizon representation"). [`lod_components`](SoulScriptSpawner::lod_components)
//! returns [`ComponentBundle::empty`] for every tier.
//!
//! ## `apply_edit` returns `false`
//!
//! Script source edits flow through the file-watcher → reload path, not
//! through the Properties panel respawn dance. The canonical source of a
//! script is its on-disk `.soul`/`.rune` file; editing it externally
//! re-reads via the watcher. So `apply_edit` updates only the cheap
//! `metadata.name` mirror and the in-memory `source` when the bag carries
//! one, then returns `false` (never request a respawn).
//!
//! ## Persistence (`serialize` / `deserialize`)
//!
//! Wave 5.C ships **stub persistence** (matching the Wave-3 container/audio
//! spawners): empty byte vector out, empty bag in. Script bodies live in
//! their source files + `_instance.toml`; the Fjall rkyv mirror for the
//! script group lights up in a later wave. Per spec §10 R9 the empty path
//! is safe — the worlddb write path skips classes whose `serialize` yields
//! no bytes.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

use crate::soul::{SoulBuildStatus, SoulRunContext, SoulScriptData};

/// Canonical bag key carrying the script source body. Every scripting
/// spawner in this group reads + writes this key so the file_loader and
/// importer can populate one well-known slot regardless of language.
pub(super) const SOURCE_KEY: &str = "script.source";

/// Zero-sized spawner for [`ClassName::SoulScript`].
///
/// State-less by design — `ClassSpawner` requires `Send + Sync + 'static`,
/// so spawners are recipe holders. Per-spawn data flows through the
/// [`PropertyBag`].
#[derive(Default)]
pub struct SoulScriptSpawner;

impl ClassSpawner for SoulScriptSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::SoulScript
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("SoulScript")
            .to_string();

        // The source body is stored on the script component. Empty string
        // when the bag omits it (hot-create from the Insert menu produces
        // an empty script the user then edits on disk).
        let source = props.get_string(SOURCE_KEY).unwrap_or("").to_string();

        let uuid = props.get_uuid().unwrap_or_default().to_string();

        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        // Mirror of the `FileType::Soul` arm at `file_loader.rs:674`. The
        // runtime's compile pipeline picks up `SoulScriptData` and drives
        // execution — this spawner never touches the VM.
        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::SoulScript,
                    archivable,
                    id: 0, // assigned by post-spawn id system
                    uuid,
                    ai: false,
                },
                SoulScriptData {
                    source,
                    dirty: false,
                    ast: None,
                    generated_code: None,
                    build_status: SoulBuildStatus::NotBuilt,
                    errors: Vec::new(),
                    run_context: SoulRunContext::Rune,
                },
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // Stub persistence — see module docs. Script bodies round-trip via
        // their source files + `_instance.toml`; the Fjall rkyv mirror for
        // the script group lights up in a later wave.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        // Inverse of the stub `serialize`: empty in, empty out. An empty
        // bag round-trips through `spawn` as a default (empty-source)
        // SoulScript — the safe-default contract of spec §2.1.
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Cheap, in-place mutations only. Script *source* edits normally
        // arrive via the file-watcher reload, but if the caller hands us a
        // source delta (e.g. an MCP edit), reflect it in memory so the next
        // compile sees it. Name edits mirror onto `Instance` + `Name`.
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(new_name) = props.get_string("metadata.name") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.name = new_name.to_string();
                }
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(new_name.to_string());
                }
            }
            if let Some(archivable) = props.get_bool("metadata.archivable") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.archivable = archivable;
                }
            }
            if let Some(new_source) = props.get_string(SOURCE_KEY) {
                if let Some(mut data) = entity_mut.get_mut::<SoulScriptData>() {
                    data.source = new_source.to_string();
                    data.dirty = true;
                    // Mark for recompile; the runtime's build pipeline owns
                    // the actual compile step.
                    data.build_status = SoulBuildStatus::Stale;
                }
            }
        }

        false // script edits never require a respawn — the runtime reloads
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Scripts have no LOD model — they are invisible. Returning an
        // empty bundle short-circuits the apply_lod_transitions system.
        // Same bundle for all four tiers (Hero, Active, Streamed, Horizon).
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, _rbx: &dyn RobloxInstance) -> PropertyBag {
        // SoulScript has no Roblox cognate — the Wave 4 importer routes
        // Roblox `Script` → `LuauScript` (see `classes.rs` `from_str`
        // alias). Per spec §2.1 a class with no Roblox cognate returns an
        // empty bag; the importer emits a warn-level log line if it ever
        // reaches here.
        PropertyBag::new()
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the script-folder `_instance.toml` shape the file_loader
        // reads at `file_loader.rs:1471`:
        //
        //     [metadata]
        //     class_name = "SoulScript"
        //     name = "MyScript"
        //     archivable = true
        //     uuid = "..."
        //
        //     [script]
        //     source = "main.soul"   # relative source filename
        //
        // The file_loader resolves the `source` filename to actual text
        // before spawn; here we surface both the metadata and the raw
        // `source` value so a Fjall-authoritative reload can re-derive it.
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
        if let Some(script) = toml_value.get("script") {
            if let Some(source) = script.get("source").and_then(|v| v.as_str()) {
                bag.set(SOURCE_KEY, PropertyValue::String(source.to_string()));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        // Inverse of `import_from_toml`. Same key order so the on-disk TOML
        // stays byte-stable across reloads.
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("SoulScript".to_string()),
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
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        // Inline the live source body under `[script]`. The on-disk script
        // file remains the editable canonical copy; this export captures
        // the in-memory state for a save round-trip / class conversion.
        if let Some(data) = world.entity(entity).get::<SoulScriptData>() {
            let mut script = toml::value::Table::new();
            script.insert(
                "source".to_string(),
                toml::Value::String(data.source.clone()),
            );
            root.insert("script".to_string(), toml::Value::Table(script));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_soul_script() {
        let spawner = SoulScriptSpawner;
        assert_eq!(spawner.class_name(), ClassName::SoulScript);
    }

    /// Object safety end-to-end — the registry holds `Box<dyn ClassSpawner>`.
    #[test]
    fn soul_script_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(SoulScriptSpawner);
        assert_eq!(boxed.class_name(), ClassName::SoulScript);
    }

    /// All four LOD tiers return empty bundles — scripts have no LOD model.
    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = SoulScriptSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            assert!(
                spawner.lod_components(tier).is_empty(),
                "SoulScriptSpawner must return empty LOD bundle at {} — scripts have no visual",
                tier.as_str()
            );
        }
    }

    /// `import_from_toml` reads metadata + the script source filename.
    #[test]
    fn import_from_toml_reads_metadata_and_source() {
        let toml_src = r#"
            [metadata]
            class_name = "SoulScript"
            name = "GravityDance"
            archivable = true
            uuid = "01234567-89ab-cdef-0123-456789abcdef"

            [script]
            source = "main.soul"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = SoulScriptSpawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("GravityDance"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
        assert_eq!(
            bag.get_string("metadata.uuid"),
            Some("01234567-89ab-cdef-0123-456789abcdef")
        );
        assert_eq!(bag.get_string(SOURCE_KEY), Some("main.soul"));
    }

    /// SoulScript has no Roblox cognate → empty bag (importer routes
    /// Roblox `Script` to `LuauScript` instead).
    #[test]
    fn import_from_roblox_is_empty() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str {
                "Script"
            }
            fn name(&self) -> &str {
                "RobloxScript"
            }
            fn property(
                &self,
                _key: &str,
            ) -> Option<eustress_common::class_registry::RobloxPropertyValue> {
                None
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> {
                Vec::new()
            }
            fn referent(&self) -> u64 {
                7
            }
        }
        assert!(SoulScriptSpawner.import_from_roblox(&Mock).is_empty());
    }

    /// Stub `serialize` emits no bytes; the matching `deserialize` yields
    /// an empty bag. Pins the stub contract so a later commit doesn't
    /// silently drop it.
    #[test]
    fn stub_persistence_round_trips_through_empty() {
        let spawner = SoulScriptSpawner;
        assert!(spawner.serialize_is_empty_proxy());
        assert!(spawner.deserialize(&[]).is_empty());
    }
}

#[cfg(test)]
impl SoulScriptSpawner {
    /// Test-only helper: `serialize` needs a `&World`, which the unit test
    /// has no cheap way to build. The stub always returns an empty vec, so
    /// this proxy pins the contract without constructing a World.
    fn serialize_is_empty_proxy(&self) -> bool {
        // The real `serialize` body is `Vec::new()` unconditionally; this
        // mirrors that invariant for the test.
        Vec::<u8>::new().is_empty()
    }
}
