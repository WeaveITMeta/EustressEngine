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
//! ## Wave 6.B additions
//!
//! ### Rigid joints (Avian solver-driven, like the Wave 3.D set)
//!
//! | `ClassName`               | Spawner                           | Avian joint mapping                                   |
//! | ------------------------- | --------------------------------- | ----------------------------------------------------- |
//! | `RodConstraint`           | [`RodConstraintSpawner`]          | [`avian3d::prelude::DistanceJoint`] `min == max`      |
//! | `CylindricalConstraint`   | [`CylindricalConstraintSpawner`]  | [`avian3d::prelude::PrismaticJoint`] (slide only — APPROX) |
//! | `TorsionSpringConstraint` | [`TorsionSpringConstraintSpawner`]| [`avian3d::prelude::RevoluteJoint`] + angular compliance/damping |
//! | `UniversalConstraint`     | [`UniversalConstraintSpawner`]    | [`avian3d::prelude::SphericalJoint`] + swing limit (APPROX) |
//! | `PlaneConstraint`         | [`PlaneConstraintSpawner`]        | (none — no Avian plane joint; runtime enforcement is follow-up) |
//!
//! ### Movers (config component → runtime system in [`crate::physics::movers`])
//!
//! These spawners attach **no Avian joint**: they place the Phase-0
//! configuration component on a child of the driven part, and the
//! `MoversPlugin` systems push force / velocity / torque onto the parent
//! body each physics frame. Mover spawners:
//! [`AlignPositionSpawner`], [`AlignOrientationSpawner`],
//! [`LinearVelocitySpawner`], [`AngularVelocitySpawner`],
//! [`VectorForceSpawner`], [`TorqueSpawner`], plus the legacy
//! [`BodyPositionSpawner`], [`BodyVelocitySpawner`], [`BodyGyroSpawner`],
//! [`BodyAngularVelocitySpawner`], [`BodyForceSpawner`],
//! [`BodyThrustSpawner`].
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
//! [`ConstraintsSpawnerPlugin`] registers all 26 spawners (9 Wave 3.D +
//! 17 Wave 6.B) with the [`ClassRegistry`] via the [`RegisterClassExt`]
//! extension trait. The plugin is self-contained — it does not mount into
//! `SlintUiPlugin` itself; the integration wave wires the plugin into the
//! running app (per the dispatch protocol the parent `spawners/mod.rs` is
//! owned by the integrating task).
//!
//! The Wave 6.B *movers* additionally require the
//! [`MoversPlugin`](crate::physics::movers::MoversPlugin) to be mounted
//! for their runtime force/velocity actuation — registering the spawner
//! only handles entity creation, not the per-frame physics.
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

// ── Wave 6.B: rigid joints + movers ───────────────────────────────────
//
// Rigid joints (Avian joint behind each):
pub mod cylindrical;
pub mod plane;
pub mod rod;
pub mod torsion_spring;
pub mod universal;
// Movers (config component → runtime system in `crate::physics::movers`):
pub mod align_orientation;
pub mod align_position;
pub mod angular_velocity;
pub mod body_angular_velocity;
pub mod body_force;
pub mod body_gyro;
pub mod body_position;
pub mod body_thrust;
pub mod body_velocity;
pub mod linear_velocity;
pub mod torque;
pub mod vector_force;

pub use attachment::AttachmentSpawner;
pub use ball_socket::BallSocketConstraintSpawner;
pub use distance::DistanceConstraintSpawner;
pub use hinge::HingeConstraintSpawner;
pub use motor6d::Motor6DSpawner;
pub use prismatic::PrismaticConstraintSpawner;
pub use rope::RopeConstraintSpawner;
pub use spring::SpringConstraintSpawner;
pub use weld::WeldConstraintSpawner;

// Wave 6.B rigid joints.
pub use cylindrical::CylindricalConstraintSpawner;
pub use plane::PlaneConstraintSpawner;
pub use rod::RodConstraintSpawner;
pub use torsion_spring::TorsionSpringConstraintSpawner;
pub use universal::UniversalConstraintSpawner;
// Wave 6.B movers.
pub use align_orientation::AlignOrientationSpawner;
pub use align_position::AlignPositionSpawner;
pub use angular_velocity::AngularVelocitySpawner;
pub use body_angular_velocity::BodyAngularVelocitySpawner;
pub use body_force::BodyForceSpawner;
pub use body_gyro::BodyGyroSpawner;
pub use body_position::BodyPositionSpawner;
pub use body_thrust::BodyThrustSpawner;
pub use body_velocity::BodyVelocitySpawner;
pub use linear_velocity::LinearVelocitySpawner;
pub use torque::TorqueSpawner;
pub use vector_force::VectorForceSpawner;

// ── Bevy plugin that registers all 9 spawners ────────────────────────

/// Bevy plugin registering every constraint/attachment/mover spawner
/// (Wave 3.D + Wave 6.B) with the [`ClassRegistry`].
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
            .register_class::<RopeConstraintSpawner>()
            // ── Wave 6.B rigid joints ──────────────────────────────────
            .register_class::<RodConstraintSpawner>()
            .register_class::<CylindricalConstraintSpawner>()
            .register_class::<TorsionSpringConstraintSpawner>()
            .register_class::<UniversalConstraintSpawner>()
            .register_class::<PlaneConstraintSpawner>()
            // ── Wave 6.B movers ────────────────────────────────────────
            .register_class::<AlignPositionSpawner>()
            .register_class::<AlignOrientationSpawner>()
            .register_class::<LinearVelocitySpawner>()
            .register_class::<AngularVelocitySpawner>()
            .register_class::<VectorForceSpawner>()
            .register_class::<TorqueSpawner>()
            .register_class::<BodyPositionSpawner>()
            .register_class::<BodyVelocitySpawner>()
            .register_class::<BodyGyroSpawner>()
            .register_class::<BodyAngularVelocitySpawner>()
            .register_class::<BodyForceSpawner>()
            .register_class::<BodyThrustSpawner>();
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

/// Build the [`ConstraintRefs`] carrier from the property bag's resolved
/// reference UUIDs, so the joint resolver can bind this constraint's Avian
/// joint by stable identity once the referenced parts/attachments exist.
///
/// The Roblox importer writes resolved part/attachment references as UUID
/// strings under `[references]`, keyed by the raw Roblox property name
/// (`Part0`/`Part1` or `Attachment0`/`Attachment1`). Each constraint
/// spawner's `import_from_toml` mirrors those into the bag under
/// `references.<Name>`; this helper reads whichever pair the class uses.
///
/// `key0` / `key1` are the bag keys for the two ends, e.g.
/// `("references.Part0", "references.Part1")` for a weld or
/// `("references.Attachment0", "references.Attachment1")` for a rod.
///
/// [`ConstraintRefs`]: crate::physics::joint_resolver::ConstraintRefs
pub(crate) fn constraint_refs_from_bag(
    bag: &crate::class_registry::PropertyBag,
    key0: &str,
    key1: &str,
) -> crate::physics::joint_resolver::ConstraintRefs {
    crate::physics::joint_resolver::ConstraintRefs::new(
        bag.get_string(key0).map(|s| s.to_string()),
        bag.get_string(key1).map(|s| s.to_string()),
    )
}

/// Read the `[references]` table from a `_instance.toml` body into the bag
/// under `references.<Key>` for the given pair of Roblox ref property
/// names. Called by each constraint spawner's `import_from_toml` so the
/// resolved UUIDs survive into [`constraint_refs_from_bag`].
pub(crate) fn read_references_into_bag(
    toml_value: &toml::Value,
    bag: &mut crate::class_registry::PropertyBag,
    name0: &str,
    name1: &str,
) {
    use eustress_common::classes::PropertyValue;
    let Some(refs) = toml_value.get("references").and_then(|v| v.as_table()) else {
        return;
    };
    // Accept the importer's PascalCase key and a lowercase hand-authored
    // fallback.
    let read = |name: &str| -> Option<String> {
        refs.get(name)
            .or_else(|| refs.get(&name.to_ascii_lowercase()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };
    if let Some(u) = read(name0) {
        bag.set(format!("references.{name0}"), PropertyValue::String(u));
    }
    if let Some(u) = read(name1) {
        bag.set(format!("references.{name1}"), PropertyValue::String(u));
    }
}

// ── Mover helpers: `ForceRelativeTo` ⇄ on-disk string ─────────────────

/// Parse the on-disk / property-bag `relative_to` string into the
/// [`ForceRelativeTo`] enum. Accepts the Roblox spelling (`"World"` /
/// `"Attachment0"`/`"Attachment1"` → part-local) case-insensitively, plus
/// the short `"Part"` form. Anything unrecognised falls back to `World`
/// (the safest default — no surprise local-frame rotation).
///
/// [`ForceRelativeTo`]: eustress_common::services::physics::ForceRelativeTo
pub(crate) fn read_force_relative_to(
    s: &str,
) -> eustress_common::services::physics::ForceRelativeTo {
    use eustress_common::services::physics::ForceRelativeTo;
    match s.to_ascii_lowercase().as_str() {
        "part" | "attachment0" | "attachment1" | "local" => ForceRelativeTo::Part,
        _ => ForceRelativeTo::World,
    }
}

/// Emit a [`ForceRelativeTo`] as its canonical on-disk string.
///
/// [`ForceRelativeTo`]: eustress_common::services::physics::ForceRelativeTo
pub(crate) fn write_force_relative_to(
    rel: eustress_common::services::physics::ForceRelativeTo,
) -> &'static str {
    use eustress_common::services::physics::ForceRelativeTo;
    match rel {
        ForceRelativeTo::World => "World",
        ForceRelativeTo::Part => "Part",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin registers all 26 spawners without duplicates. The
    /// registry's own `register` panics on duplicate keys, so a
    /// successful `build` is itself the proof — but we also assert the
    /// final count to surface a missed spawner if anyone removes one
    /// from the `build` chain.
    ///
    /// 26 = the original Wave 3.D set (Attachment + 8 joints) plus the
    /// Wave 6.B set (5 rigid joints + 12 movers).
    #[test]
    fn plugin_registers_all_spawners() {
        use crate::class_registry::{ClassRegistry, ClassRegistryPlugin};

        let mut app = App::new();
        app.add_plugins(ClassRegistryPlugin);
        app.add_plugins(ConstraintsSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        assert_eq!(
            registry.len(),
            26,
            "ConstraintsSpawnerPlugin must register exactly 26 spawners — \
             Attachment + 8 Wave 3.D joints + 5 Wave 6.B rigid joints + \
             12 Wave 6.B movers"
        );

        use eustress_common::classes::ClassName;
        for class in [
            // Wave 3.D
            ClassName::Attachment,
            ClassName::WeldConstraint,
            ClassName::Motor6D,
            ClassName::HingeConstraint,
            ClassName::DistanceConstraint,
            ClassName::PrismaticConstraint,
            ClassName::BallSocketConstraint,
            ClassName::SpringConstraint,
            ClassName::RopeConstraint,
            // Wave 6.B rigid joints
            ClassName::RodConstraint,
            ClassName::CylindricalConstraint,
            ClassName::TorsionSpringConstraint,
            ClassName::UniversalConstraint,
            ClassName::PlaneConstraint,
            // Wave 6.B movers
            ClassName::AlignPosition,
            ClassName::AlignOrientation,
            ClassName::LinearVelocity,
            ClassName::AngularVelocity,
            ClassName::VectorForce,
            ClassName::Torque,
            ClassName::BodyPosition,
            ClassName::BodyVelocity,
            ClassName::BodyGyro,
            ClassName::BodyAngularVelocity,
            ClassName::BodyForce,
            ClassName::BodyThrust,
        ] {
            assert!(
                registry.contains(class),
                "ConstraintsSpawnerPlugin must register {class:?}"
            );
        }
    }
}
