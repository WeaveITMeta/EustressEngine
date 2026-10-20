//! # A part's collider follows its size
//!
//! Whatever writes `BasePart.size` (a scale drag, the Properties panel, a
//! script's `Size` in Play, a replicated write on the Player), the Avian
//! collider is rebuilt to match.

use bevy::prelude::*;

/// The size (and transform scale) an entity's collider was last rebuilt
/// for by [`rebuild_collider_on_size_change`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ColliderBuiltForSize {
    pub size: Vec3,
    pub scale: Vec3,
}

/// Rebuild the Avian `Collider` in-place whenever an entity's
/// `BasePart.size` changes — scale-tool drag, Properties-panel type-in,
/// paste-props, MCP resize, undo/redo, any write-back from disk. Without
/// this, the visual mesh resized but the collider stayed at the spawn
/// dimensions, so raycasts, surface snapping, selection hit-test, and
/// physics all stepped into "shadow-of-the-old-size" territory.
///
/// Only runs for entities that already have a `Collider` — we never
/// add physics to a part that was spawned without `can_collide`.
///
/// `Changed<BasePart>` also fires for colour, transparency and material
/// writes (a script tweening a part touches it every frame), so the size
/// the collider was last built for is remembered and anything else is
/// skipped.
pub fn rebuild_collider_on_size_change(
    mut commands: Commands,
    changed: Query<
        (
            Entity,
            &eustress_common::classes::BasePart,
            Option<&eustress_common::classes::Part>,
            &Transform,
            Option<&ColliderBuiltForSize>,
        ),
        (Changed<eustress_common::classes::BasePart>, With<avian3d::prelude::Collider>),
    >,
) {
    use avian3d::prelude::Collider;
    use eustress_common::classes::PartType;

    for (entity, base_part, part_opt, transform, built) in changed.iter() {
        if built.map_or(false, |b| b.size == base_part.size && b.scale == transform.scale) {
            continue;
        }
        commands
            .entity(entity)
            .insert(ColliderBuiltForSize { size: base_part.size, scale: transform.scale });
        // Sanitise dimensions first — a degenerate 0 / negative / non-finite
        // size would panic Avian's collider builder on the next physics step.
        let safe_size = Vec3::new(
            if base_part.size.x.is_finite() { base_part.size.x.abs().max(0.1) } else { 0.1 },
            if base_part.size.y.is_finite() { base_part.size.y.abs().max(0.1) } else { 0.1 },
            if base_part.size.z.is_finite() { base_part.size.z.abs().max(0.1) } else { 0.1 },
        );
        // Collider dimensions are LOCAL — Avian re-applies the entity's
        // transform scale. Use the same canonical cancellation as loading and
        // spawning, or a resize would silently change the part's collision
        // size relative to how it loaded.
        let half = eustress_common::part_draw::collider_local_half(safe_size, transform.scale);
        let collider = match part_opt.map(|p| p.shape) {
            Some(PartType::Ball) => Collider::sphere(half.x),
            Some(PartType::Cylinder) | Some(PartType::Cone) => {
                Collider::cylinder(half.x, half.y * 2.0)
            }
            _ => Collider::cuboid(half.x * 2.0, half.y * 2.0, half.z * 2.0),
        };
        commands.entity(entity).insert(collider);
    }
}
