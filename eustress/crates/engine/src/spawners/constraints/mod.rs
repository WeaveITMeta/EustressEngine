//! # Constraint spawners — Wave 3.D
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.3 — one
//! [`ClassSpawner`](crate::class_registry::ClassSpawner) implementation per
//! constraint/attachment `ClassName` variant.
//!
//! ## Spawners shipped here
//!
//! | `ClassName`            | Spawner                       | Avian joint mapping            |
//! | ---------------------- | ----------------------------- | ------------------------------ |
//! | `Attachment`           | [`AttachmentSpawner`]         | (none — anchor socket only)    |
//! | `WeldConstraint`       | [`WeldConstraintSpawner`]     | [`avian3d::prelude::FixedJoint`]      |
//! | `Motor6D`              | [`Motor6DSpawner`]            | [`avian3d::prelude::RevoluteJoint`]   |
//! | `HingeConstraint`      | [`HingeConstraintSpawner`]    | [`avian3d::prelude::RevoluteJoint`]   |
//! | `DistanceConstraint`   | [`DistanceConstraintSpawner`] | [`avian3d::prelude::DistanceJoint`]   |
//! | `PrismaticConstraint`  | [`PrismaticConstraintSpawner`]| [`avian3d::prelude::PrismaticJoint`]  |
//! | `BallSocketConstraint` | [`BallSocketConstraintSpawner`]| [`avian3d::prelude::SphericalJoint`] |
//! | `SpringConstraint`     | [`SpringConstraintSpawner`]   | [`avian3d::prelude::DistanceJoint`] + compliance + [`avian3d::prelude::JointDamping`] |
//! | `RopeConstraint`       | [`RopeConstraintSpawner`]     | [`avian3d::prelude::DistanceJoint`] with `min=0, max=length` |
//!
//! ## Design decisions
//!
//! ### Entity ref resolution
//!
//! Eustress constraint structs reference `Part0`/`Part1` by `Option<u32>`
//! (legacy Eustress instance IDs that map to Bevy entities via a runtime
//! `Query<(Entity, &Instance)>` walk). The `ClassSpawner::spawn` trait
//! receives only `commands` access — no World query — so we cannot resolve
//! IDs to entities at spawn time without expanding the trait surface
//! (out of scope this wave).
//!
//! Instead, we attach the Avian joint with [`Entity::PLACEHOLDER`] for
//! both bodies whenever a side is unresolved. A downstream system can
//! walk new joints, look up `Part0`/`Part1` against the live entity world
//! by Eustress ID, and call `entity.insert(FixedJoint { body1, body2, .. })`
//! to fix up the body refs. Avian tolerates placeholder bodies until the
//! joint actually runs in `PhysicsSchedule`; the joint plugin emits a
//! warn if the placeholder is still present when integration runs.
//!
//! This matches the existing pattern in `spawn.rs::spawn_weld_constraint`
//! et al — those legacy free fns also spawn the Eustress component
//! without resolving entity refs.
//!
//! ### `JointDisabled` for `enabled = false`
//!
//! Every constraint carries an `enabled: bool` field. When `enabled =
//! false`, the spawner inserts the Avian [`avian3d::prelude::JointDisabled`]
//! marker so the joint solver skips the entity. The Eustress component
//! still records the user's intent — re-enabling is one component edit
//! away.
//!
//! ### LOD: no per-tier behavior
//!
//! Constraints have no visual representation outside the editor's
//! adornment renderer, so [`ClassSpawner::lod_components`] returns an
//! empty bundle for every tier. The editor adornments live elsewhere
//! (`attachments.rs` / `motor6d.rs` / `constraint_editor_tool.rs`) and
//! are not LOD-managed today.
//!
//! ### `apply_edit` always returns `true`
//!
//! Constraint property edits (axis change, limit change, even
//! `enabled` toggle) interact with Avian's joint cache. The safest
//! semantics today is "respawn the joint entity" — Wave 4 may relax
//! this to in-place mutation for cheap fields once the apply-edit
//! contract is exercised end-to-end.
//!
//! ## Plugin
//!
//! [`ConstraintsSpawnerPlugin`] registers all 9 spawners with the
//! [`ClassRegistry`] via the [`RegisterClassExt`] extension trait. The
//! plugin is self-contained — it does not mount into `SlintUiPlugin`
//! itself; Wave 5 wires the plugin into the running app once all 3.x
//! groups have shipped (per the dispatch protocol the parent
//! `spawners/mod.rs` is owned by 3.A, which integrates every group).
//!
//! Until Wave 3.A integrates this module under `engine/src/lib.rs`'s
//! `pub mod spawners;` declaration, the files here compile in isolation
//! but are unreachable from the binary — that's by design (the
//! dispatch protocol's per-task ownership boundaries).
//!
//! [`ClassRegistry`]: crate::class_registry::ClassRegistry
//! [`RegisterClassExt`]: crate::class_registry::RegisterClassExt

use bevy::prelude::*;

use crate::class_registry::RegisterClassExt;

// ── Per-class spawners ────────────────────────────────────────────────

pub mod attachment;
pub mod ball_socket;
pub mod distance;
pub mod hinge;
pub mod motor6d;
pub mod prismatic;
pub mod rope;
pub mod spring;
pub mod weld;

pub use attachment::AttachmentSpawner;
pub use ball_socket::BallSocketConstraintSpawner;
pub use distance::DistanceConstraintSpawner;
pub use hinge::HingeConstraintSpawner;
pub use motor6d::Motor6DSpawner;
pub use prismatic::PrismaticConstraintSpawner;
pub use rope::RopeConstraintSpawner;
pub use spring::SpringConstraintSpawner;
pub use weld::WeldConstraintSpawner;

// ── Bevy plugin that registers all 9 spawners ────────────────────────

/// Bevy plugin registering every constraint/attachment spawner shipped
/// by Wave 3.D with the [`ClassRegistry`].
///
/// Mounts via `app.add_plugins(ConstraintsSpawnerPlugin)`. The Wave 2.3
/// [`ClassRegistryPlugin`] must run before this — `ConstraintsSpawnerPlugin`
/// reaches into the registry resource the moment its `build` runs, so
/// the resource must already exist.
///
/// Plugin-order contract: add `ClassRegistryPlugin` then
/// `ConstraintsSpawnerPlugin` in the same plugin group, or order them
/// explicitly via Bevy's plugin-ordering API.
///
/// [`ClassRegistry`]: crate::class_registry::ClassRegistry
/// [`ClassRegistryPlugin`]: crate::class_registry::ClassRegistryPlugin
pub struct ConstraintsSpawnerPlugin;

impl Plugin for ConstraintsSpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<AttachmentSpawner>()
            .register_class::<WeldConstraintSpawner>()
            .register_class::<Motor6DSpawner>()
            .register_class::<HingeConstraintSpawner>()
            .register_class::<DistanceConstraintSpawner>()
            .register_class::<PrismaticConstraintSpawner>()
            .register_class::<BallSocketConstraintSpawner>()
            .register_class::<SpringConstraintSpawner>()
            .register_class::<RopeConstraintSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the Bevy [`Instance`] component that every constraint entity
/// carries. Reads `metadata.name` from the property bag, falls back to
/// the class's default name when absent.
///
/// [`Instance`]: eustress_common::classes::Instance
pub(crate) fn instance_from_bag(
    class_name: eustress_common::classes::ClassName,
    bag: &crate::class_registry::PropertyBag,
) -> eustress_common::classes::Instance {
    let mut instance = eustress_common::classes::Instance {
        class_name,
        ..Default::default()
    };
    if let Some(name) = bag.get_string("metadata.name") {
        instance.name = name.to_string();
    }
    if let Some(uuid) = bag.get_uuid() {
        instance.uuid = uuid.to_string();
    }
    if let Some(archivable) = bag.get_bool("metadata.archivable") {
        instance.archivable = archivable;
    }
    instance
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin registers all 9 spawners without duplicates. The
    /// registry's own `register` panics on duplicate keys, so a
    /// successful `build` is itself the proof — but we also assert the
    /// final count to surface a missed spawner if anyone removes one
    /// from the `build` chain.
    #[test]
    fn plugin_registers_all_nine_spawners() {
        use crate::class_registry::{ClassRegistry, ClassRegistryPlugin};

        let mut app = App::new();
        app.add_plugins(ClassRegistryPlugin);
        app.add_plugins(ConstraintsSpawnerPlugin);

        let registry = app
            .world()
            .resource::<ClassRegistry>();
        assert_eq!(
            registry.len(),
            9,
            "ConstraintsSpawnerPlugin must register exactly 9 spawners — \
             Attachment + 8 joint constraints"
        );

        use eustress_common::classes::ClassName;
        for class in [
            ClassName::Attachment,
            ClassName::WeldConstraint,
            ClassName::Motor6D,
            ClassName::HingeConstraint,
            ClassName::DistanceConstraint,
            ClassName::PrismaticConstraint,
            ClassName::BallSocketConstraint,
            ClassName::SpringConstraint,
            ClassName::RopeConstraint,
        ] {
            assert!(
                registry.contains(class),
                "ConstraintsSpawnerPlugin must register {class:?}"
            );
        }
    }
}
