//! `KeyframeSequenceSpawner` — animation-clip container spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::KeyframeSequence`] per
//! `docs/architecture/CLASS_REGISTRY.md` §2 (trait) + §8.8 (animation).
//!
//! ## What this is
//!
//! `KeyframeSequence` is the **animation-clip container** — a sequence
//! of poses over time that an [`Animator`](super::animator) plays on a
//! rig. The Roblox equivalent is `KeyframeSequence` (the editable form
//! of an `Animation` asset). In Eustress it maps to a Bevy
//! `AnimationClip`; this spawner attaches the Eustress data component
//! ([`KeyframeSequence`]) carrying `looped`, `priority`, and the
//! keyframe list. Sampling / clip construction is owned by the
//! animation runtime (NOT this task).
//!
//! Mirror of the legacy `spawn::spawn_keyframe_sequence` helper at
//! `crates/engine/src/spawn.rs:1567`. Bundle attached:
//!
//! - [`Transform`] (identity by default — a clip is data, not spatial)
//! - [`Visibility`] (default — clips are non-visual; carried for Bevy
//!   hierarchy uniformity, matching the legacy helper)
//! - [`Instance`] (with `class_name = ClassName::KeyframeSequence` and
//!   the `metadata.name` from the bag)
//! - [`KeyframeSequence`] component (`looped`, `priority`, `keyframes`)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//!
//! ## Why no LOD
//!
//! Per spec §9 + LOOP-3 breaker: a clip carries no visual, so it has no
//! LOD model. [`lod_components`](KeyframeSequenceSpawner::lod_components)
//! returns [`ComponentBundle::empty`] for every tier.
//!
//! ## Keyframe data scope
//!
//! Keyframes are a `Vec<Keyframe>` of `{ time, pose, easing }`. The
//! TOML round-trip here ships the **scalar metadata** (`looped`,
//! `priority`) — the canonical authoring surface mirrored by the legacy
//! `keyframesequence_from_properties` path (`scene.rs:1483`, which maps
//! only `Loop` + `Priority`). The keyframe array itself is bulk pose
//! data better suited to the Wave-4 rkyv archive than to hand-edited
//! TOML; `spawn`/`deserialize` accept it but `import_from_toml` leaves
//! it empty (defaulting to `KeyframeSequence::default().keyframes`). This
//! matches how Roblox keyframes arrive as child `Keyframe` instances
//! rather than inline properties.
//!
//! ## Persistence (`serialize` / `deserialize`)
//!
//! Stub persistence: empty byte vector out, empty bag in. Same contract
//! `FolderSpawner` / `AnimatorSpawner` document — Wave 4 lights up the
//! rkyv archive that carries the full keyframe list.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{
    AnimationPriority, ClassName, Instance, KeyframeSequence, PropertyValue,
};

/// Zero-sized spawner for [`ClassName::KeyframeSequence`].
///
/// State-less by design — `ClassSpawner` requires `Send + Sync +
/// 'static`, so spawners are recipe holders, not state. Per-spawn
/// mutability flows through [`SpawnCtx`].
#[derive(Default)]
pub struct KeyframeSequenceSpawner;

impl ClassSpawner for KeyframeSequenceSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::KeyframeSequence
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        // Canonical name accessor (mirrors `FolderSpawner`). Fall back to
        // "KeyframeSequence" so a hot-create with an empty bag still
        // produces a labelled entity.
        let name = props
            .get_string("metadata.name")
            .unwrap_or("KeyframeSequence")
            .to_string();

        // UUID provenance from Wave 2.1.
        let uuid = props.get_uuid().unwrap_or_default().to_string();

        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        // Build the data component from the bag, defaulting per
        // `KeyframeSequence::default()` (not looped, Core priority, no
        // keyframes) for any key the source omits. The keyframe `Vec`
        // itself is not authored via the bag in this stub — it stays
        // empty and is populated by the Wave-4 rkyv path / runtime.
        let mut sequence = KeyframeSequence::default();
        if let Some(looped) = props.get_bool("looped") {
            sequence.looped = looped;
        }
        if let Some(priority) = props.get_enum("priority") {
            sequence.priority = animation_priority_from_str(priority);
        }

        // Mirror of `spawn::spawn_keyframe_sequence` at `spawn.rs:1567`.
        // Component order matches the legacy helper to preserve Bevy
        // archetype identity across the registry/legacy migration window.
        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::KeyframeSequence,
                    archivable,
                    id: 0, // assigned by post-spawn id system
                    uuid,
                    ai: false,
                },
                sequence,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // Stub persistence — Wave 4 fills in a tagged rkyv archive
        // carrying the full keyframe list once the Fjall write path opts
        // in for animation classes.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        // Inverse of `serialize` — empty in, empty out for the stub.
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Cheap mutations only — no respawn-required props. Writable
        // surface props: `metadata.name`, `metadata.archivable` (on
        // `Instance`), and `looped` / `priority` (on `KeyframeSequence`).
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                if let Some(new_name) = props.get_string("metadata.name") {
                    instance.name = new_name.to_string();
                }
                if let Some(archivable) = props.get_bool("metadata.archivable") {
                    instance.archivable = archivable;
                }
            }

            if let Some(mut sequence) = entity_mut.get_mut::<KeyframeSequence>() {
                if let Some(looped) = props.get_bool("looped") {
                    sequence.looped = looped;
                }
                if let Some(priority) = props.get_enum("priority") {
                    sequence.priority = animation_priority_from_str(priority);
                }
            }

            // `Name` mirrors `Instance.name`; keep them in lockstep.
            if let Some(new_name) = props.get_string("metadata.name") {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(new_name.to_string());
                }
            }
        }

        false // never needs respawn
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // A clip carries no visual — no LOD model. Empty bundle
        // short-circuits the apply_lod_transitions system. Same bundle
        // for all four tiers (Hero, Active, Streamed, Horizon).
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Roblox `KeyframeSequence` carries `Loop` (bool) and `Priority`
        // (AnimationPriority enum) beyond Instance basics. The keyframes
        // themselves are child `Keyframe` instances, handled by the
        // importer's hierarchy walk — not mapped as scalar props here.
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(looped) = rbx.property("Loop").and_then(|p| p.as_bool()) {
            bag.set("looped", PropertyValue::Bool(looped));
        }
        if let Some(priority) = rbx
            .property("Priority")
            .and_then(|p| p.as_str().map(str::to_owned))
        {
            bag.set("priority", PropertyValue::Enum(priority));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the `class_schema/KeyframeSequence/_instance.toml` shape:
        //
        //     [metadata]
        //     class_name = "KeyframeSequence"
        //     name = "WalkCycle"
        //     archivable = true
        //     uuid = "01234567-89ab-..."   # Wave 2.1
        //
        //     [properties]
        //     looped = true
        //     priority = "Movement"
        //
        // Keys are emitted in template order per spec §4.3 — same order
        // `export_to_toml` writes them back. The keyframe array is not
        // an inline TOML property (see module docs).
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

        if let Some(properties) = toml_value.get("properties") {
            if let Some(looped) = properties.get("looped").and_then(|v| v.as_bool()) {
                bag.set("looped", PropertyValue::Bool(looped));
            }
            if let Some(priority) = properties.get("priority").and_then(|v| v.as_str()) {
                bag.set("priority", PropertyValue::Enum(priority.to_string()));
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
            toml::Value::String("KeyframeSequence".to_string()),
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

        if let Some(sequence) = world.entity(entity).get::<KeyframeSequence>() {
            let mut props = toml::value::Table::new();
            props.insert(
                "looped".to_string(),
                toml::Value::Boolean(sequence.looped),
            );
            props.insert(
                "priority".to_string(),
                toml::Value::String(
                    animation_priority_to_str(sequence.priority).to_string(),
                ),
            );
            root.insert("properties".to_string(), toml::Value::Table(props));
        }

        toml::Value::Table(root)
    }
}

/// Map an `AnimationPriority` enum string to the typed variant. Unknown
/// strings fall back to the `KeyframeSequence::default()` priority
/// (`Core`) so malformed TOML degrades gracefully.
fn animation_priority_from_str(s: &str) -> AnimationPriority {
    match s {
        "Core" => AnimationPriority::Core,
        "Idle" => AnimationPriority::Idle,
        "Movement" => AnimationPriority::Movement,
        "Action" => AnimationPriority::Action,
        _ => AnimationPriority::Core,
    }
}

/// Inverse of [`animation_priority_from_str`] — stable discriminant
/// names for the TOML round-trip.
fn animation_priority_to_str(priority: AnimationPriority) -> &'static str {
    match priority {
        AnimationPriority::Core => "Core",
        AnimationPriority::Idle => "Idle",
        AnimationPriority::Movement => "Movement",
        AnimationPriority::Action => "Action",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `class_name()` returns the registered key.
    #[test]
    fn class_name_is_keyframe_sequence() {
        let spawner = KeyframeSequenceSpawner;
        assert_eq!(spawner.class_name(), ClassName::KeyframeSequence);
    }

    /// All four LOD tiers return empty bundles — a clip has no visual.
    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = KeyframeSequenceSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            let bundle = spawner.lod_components(tier);
            assert!(
                bundle.is_empty(),
                "KeyframeSequenceSpawner must return empty LOD bundle at {} — \
                 a clip has no standalone visual",
                tier.as_str()
            );
        }
    }

    /// `import_from_toml` reads metadata + the clip properties.
    #[test]
    fn import_from_toml_reads_properties() {
        let toml_src = r#"
            [metadata]
            class_name = "KeyframeSequence"
            name = "WalkCycle"
            archivable = true
            uuid = "01234567-89ab-cdef-0123-456789abcdef"

            [properties]
            looped = true
            priority = "Movement"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let spawner = KeyframeSequenceSpawner;
        let bag = spawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("WalkCycle"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
        assert_eq!(bag.get_bool("looped"), Some(true));
        assert_eq!(bag.get_enum("priority"), Some("Movement"));
    }

    /// `import_from_roblox` maps `Name` + `Loop` + `Priority`.
    #[test]
    fn import_from_roblox_maps_loop_and_priority() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str {
                "KeyframeSequence"
            }
            fn name(&self) -> &str {
                "RobloxClip"
            }
            fn property(
                &self,
                key: &str,
            ) -> Option<eustress_common::class_registry::RobloxPropertyValue> {
                match key {
                    "Loop" => {
                        Some(eustress_common::class_registry::RobloxPropertyValue::Bool(true))
                    }
                    "Priority" => Some(
                        eustress_common::class_registry::RobloxPropertyValue::String(
                            "Action".to_string(),
                        ),
                    ),
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> {
                Vec::new()
            }
            fn referent(&self) -> u64 {
                9
            }
        }

        let spawner = KeyframeSequenceSpawner;
        let bag = spawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("metadata.name"), Some("RobloxClip"));
        assert_eq!(bag.get_bool("looped"), Some(true));
        assert_eq!(bag.get_enum("priority"), Some("Action"));
    }

    /// Empty TOML input produces an empty bag — safe-default contract.
    #[test]
    fn import_from_toml_empty_returns_empty_bag() {
        let value: toml::Value = toml::from_str("").unwrap();
        let spawner = KeyframeSequenceSpawner;
        let bag = spawner.import_from_toml(&value);
        assert!(bag.is_empty());
    }

    /// Stub `serialize`/`deserialize` round-trips through empty.
    #[test]
    fn stub_persistence_round_trips_through_empty() {
        let spawner = KeyframeSequenceSpawner;
        let bag = spawner.deserialize(&[]);
        assert!(bag.is_empty());
    }

    /// AnimationPriority string mapping round-trips through both
    /// directions.
    #[test]
    fn priority_string_round_trip() {
        for priority in [
            AnimationPriority::Core,
            AnimationPriority::Idle,
            AnimationPriority::Movement,
            AnimationPriority::Action,
        ] {
            assert_eq!(
                animation_priority_from_str(animation_priority_to_str(priority)),
                priority
            );
        }
        // Unknown strings degrade to the default priority.
        assert_eq!(animation_priority_from_str("Nonsense"), AnimationPriority::Core);
    }
}
