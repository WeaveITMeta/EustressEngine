//! `AnimatorSpawner` — animation-track manager spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::Animator`] per
//! `docs/architecture/CLASS_REGISTRY.md` §2 (trait) + §8.8 (animation).
//!
//! ## What this is
//!
//! `Animator` is the **playback controller** for a rig: it plays
//! [`KeyframeSequence`] clips on a `Humanoid` / `Model` skeleton. The
//! Roblox equivalent is `Animator` (child of `Humanoid` or
//! `AnimationController`). In Eustress it maps to a
//! `bevy::animation::AnimationPlayer` driven by the existing animation
//! runtime — this spawner only attaches the Eustress data component
//! ([`Animator`]); the playback systems (NOT owned by this task) read
//! it and drive the rig.
//!
//! Mirror of the legacy `spawn::spawn_animator` helper at
//! `crates/engine/src/spawn.rs:1550`. Bundle attached:
//!
//! - [`Transform`] (identity by default — the animator has no spatial
//!   intrinsics; it acts on its parent rig)
//! - [`Visibility`] (default — animators are non-visual; this is
//!   carried only for Bevy hierarchy uniformity, matching the legacy
//!   helper)
//! - [`Instance`] (with `class_name = ClassName::Animator` and the
//!   `metadata.name` from the bag)
//! - [`Animator`] component (`preferred_animation_speed`, `rig_type`)
//! - [`Name`] (Bevy core, mirrors `Instance.name`)
//!
//! ## Why no LOD
//!
//! Per spec §9 + LOOP-3 breaker in `docs/process/AGENT_DISPATCH.md`:
//! animation classes carry no standalone visual, so they have no LOD
//! model. The visible rig is owned by its `Model` / `Part` children,
//! which carry their own LOD via their own spawners.
//! [`lod_components`](AnimatorSpawner::lod_components) returns
//! [`ComponentBundle::empty`] for every tier — the transition system
//! short-circuits on empty bundles (see `lod.rs`).
//!
//! ## Persistence (`serialize` / `deserialize`)
//!
//! Stub persistence: empty byte vector out, empty bag in. Animator
//! state is derivable from its `_instance.toml`; the rkyv mirror lights
//! up when the Fjall write path opts in for animation classes (same
//! contract `FolderSpawner` documents). Per spec §10 the empty path is
//! safe — the worlddb write path skips classes without a registered
//! archive.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{Animator, ClassName, Instance, PropertyValue, RigType};

/// Zero-sized spawner for [`ClassName::Animator`].
///
/// State-less by design — `ClassSpawner` requires `Send + Sync +
/// 'static`, so spawners are recipe holders, not state. Per-spawn
/// mutability flows through [`SpawnCtx`].
#[derive(Default)]
pub struct AnimatorSpawner;

impl ClassSpawner for AnimatorSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Animator
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        // Canonical name accessor (mirrors `FolderSpawner`). Fall back to
        // "Animator" so a hot-create with an empty bag still produces a
        // labelled entity.
        let name = props
            .get_string("metadata.name")
            .unwrap_or("Animator")
            .to_string();

        // UUID provenance from Wave 2.1 — stamped on the Instance when
        // present so the worlddb reverse index resolves this entity.
        let uuid = props.get_uuid().unwrap_or_default().to_string();

        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        // Build the Animator data component from the bag, defaulting per
        // `Animator::default()` (speed 1.0, RigType::R15) for any key the
        // source omits.
        let mut animator = Animator::default();
        if let Some(speed) = props.get_f32("preferred_animation_speed") {
            animator.preferred_animation_speed = speed;
        }
        if let Some(rig) = props.get_enum("rig_type") {
            animator.rig_type = rig_type_from_str(rig);
        }

        // Mirror of `spawn::spawn_animator` at `spawn.rs:1550`. Component
        // order matches the legacy helper to preserve Bevy archetype
        // identity across the registry/legacy migration window.
        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::Animator,
                    archivable,
                    id: 0, // assigned by post-spawn id system
                    uuid,
                    ai: false,
                },
                animator,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // Stub persistence — Wave 4 fills in a tagged rkyv archive once
        // the Fjall write path opts in for animation classes.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        // Inverse of `serialize` — empty in, empty out for the stub. The
        // empty bag round-trips through `spawn` as a default Animator.
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Cheap mutations only — Animator has no respawn-required props.
        // Writable surface props: `metadata.name`, `metadata.archivable`
        // (on `Instance`), and `preferred_animation_speed` / `rig_type`
        // (on `Animator`). All mutate in place.
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                if let Some(new_name) = props.get_string("metadata.name") {
                    instance.name = new_name.to_string();
                }
                if let Some(archivable) = props.get_bool("metadata.archivable") {
                    instance.archivable = archivable;
                }
            }

            if let Some(mut animator) = entity_mut.get_mut::<Animator>() {
                if let Some(speed) = props.get_f32("preferred_animation_speed") {
                    animator.preferred_animation_speed = speed;
                }
                if let Some(rig) = props.get_enum("rig_type") {
                    animator.rig_type = rig_type_from_str(rig);
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
        // Animation classes have no standalone visual — the rig's
        // children carry LOD. Returning an empty bundle short-circuits
        // the apply_lod_transitions system. Same bundle for all four
        // tiers (Hero, Active, Streamed, Horizon).
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Roblox `Animator` carries `PreferredAnimationSpeed` (a float)
        // beyond Instance basics. `RigType` is implicit from the rig in
        // Roblox, so we leave it at the Eustress default unless a TOML
        // override is present.
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(speed) = rbx
            .property("PreferredAnimationSpeed")
            .and_then(|p| p.as_f32())
        {
            bag.set(
                "preferred_animation_speed",
                PropertyValue::Float(speed),
            );
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the `class_schema/Animator/_instance.toml` shape:
        //
        //     [metadata]
        //     class_name = "Animator"
        //     name = "MyAnimator"
        //     archivable = true
        //     uuid = "01234567-89ab-..."   # Wave 2.1
        //
        //     [properties]
        //     preferred_animation_speed = 1.0
        //     rig_type = "R15"
        //
        // Keys are emitted in template order per spec §4.3 — same order
        // `export_to_toml` writes them back.
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
            if let Some(speed) = properties
                .get("preferred_animation_speed")
                .and_then(|v| v.as_float())
            {
                bag.set(
                    "preferred_animation_speed",
                    PropertyValue::Float(speed as f32),
                );
            }
            if let Some(rig) = properties.get("rig_type").and_then(|v| v.as_str()) {
                bag.set("rig_type", PropertyValue::Enum(rig.to_string()));
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
            toml::Value::String("Animator".to_string()),
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

        if let Some(animator) = world.entity(entity).get::<Animator>() {
            let mut props = toml::value::Table::new();
            props.insert(
                "preferred_animation_speed".to_string(),
                toml::Value::Float(animator.preferred_animation_speed as f64),
            );
            props.insert(
                "rig_type".to_string(),
                toml::Value::String(rig_type_to_str(animator.rig_type).to_string()),
            );
            root.insert("properties".to_string(), toml::Value::Table(props));
        }

        toml::Value::Table(root)
    }
}

/// Map a `RigType` enum string to the typed variant. Unknown strings
/// fall back to the `Animator::default()` rig (`R15`) so malformed TOML
/// degrades gracefully rather than failing the whole load.
fn rig_type_from_str(s: &str) -> RigType {
    match s {
        "Humanoid" => RigType::Humanoid,
        "R15" => RigType::R15,
        "R6" => RigType::R6,
        "Custom" => RigType::Custom,
        _ => RigType::R15,
    }
}

/// Inverse of [`rig_type_from_str`] — stable discriminant names for the
/// TOML round-trip.
fn rig_type_to_str(rig: RigType) -> &'static str {
    match rig {
        RigType::Humanoid => "Humanoid",
        RigType::R15 => "R15",
        RigType::R6 => "R6",
        RigType::Custom => "Custom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `class_name()` returns the registered key. The registry's
    /// `register()` panics on mismatch — this test catches the
    /// regression at unit-test time.
    #[test]
    fn class_name_is_animator() {
        let spawner = AnimatorSpawner;
        assert_eq!(spawner.class_name(), ClassName::Animator);
    }

    /// All four LOD tiers return empty bundles — animation has no
    /// standalone visual.
    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = AnimatorSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            let bundle = spawner.lod_components(tier);
            assert!(
                bundle.is_empty(),
                "AnimatorSpawner must return empty LOD bundle at {} — \
                 animation has no standalone visual",
                tier.as_str()
            );
        }
    }

    /// `import_from_toml` reads metadata + the animator properties.
    #[test]
    fn import_from_toml_reads_properties() {
        let toml_src = r#"
            [metadata]
            class_name = "Animator"
            name = "RigDriver"
            archivable = true
            uuid = "01234567-89ab-cdef-0123-456789abcdef"

            [properties]
            preferred_animation_speed = 1.5
            rig_type = "R6"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let spawner = AnimatorSpawner;
        let bag = spawner.import_from_toml(&value);

        assert_eq!(bag.get_string("metadata.name"), Some("RigDriver"));
        assert_eq!(bag.get_bool("metadata.archivable"), Some(true));
        assert_eq!(bag.get_f32("preferred_animation_speed"), Some(1.5));
        assert_eq!(bag.get_enum("rig_type"), Some("R6"));
    }

    /// `import_from_roblox` maps `Name` + `PreferredAnimationSpeed`.
    #[test]
    fn import_from_roblox_maps_speed() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str {
                "Animator"
            }
            fn name(&self) -> &str {
                "RobloxAnimator"
            }
            fn property(
                &self,
                key: &str,
            ) -> Option<eustress_common::class_registry::RobloxPropertyValue> {
                match key {
                    "PreferredAnimationSpeed" => {
                        Some(eustress_common::class_registry::RobloxPropertyValue::Float(2.0))
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

        let spawner = AnimatorSpawner;
        let bag = spawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("metadata.name"), Some("RobloxAnimator"));
        assert_eq!(bag.get_f32("preferred_animation_speed"), Some(2.0));
    }

    /// Empty TOML input produces an empty bag — safe-default contract.
    #[test]
    fn import_from_toml_empty_returns_empty_bag() {
        let value: toml::Value = toml::from_str("").unwrap();
        let spawner = AnimatorSpawner;
        let bag = spawner.import_from_toml(&value);
        assert!(bag.is_empty());
    }

    /// Stub `serialize`/`deserialize` round-trips through empty.
    #[test]
    fn stub_persistence_round_trips_through_empty() {
        let spawner = AnimatorSpawner;
        let bag = spawner.deserialize(&[]);
        assert!(bag.is_empty());
    }

    /// RigType string mapping round-trips through both directions.
    #[test]
    fn rig_type_string_round_trip() {
        for rig in [
            RigType::Humanoid,
            RigType::R15,
            RigType::R6,
            RigType::Custom,
        ] {
            assert_eq!(rig_type_from_str(rig_type_to_str(rig)), rig);
        }
        // Unknown strings degrade to the default rig.
        assert_eq!(rig_type_from_str("Nonsense"), RigType::R15);
    }
}
