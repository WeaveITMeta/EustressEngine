//! # Model Reflect Linked (Phase 1)
//!
//! Non-destructive mirror — when the user picks the "Linked" option in
//! Model Reflect, the tool spawns mirrored copies AND tags each copy
//! with a `MirrorLink` component that references the source + the
//! mirror plane. A per-frame system watches each source's Transform
//! for changes and re-applies the reflection to its linked mirror so
//! the pair stays in sync.
//!
//! ## Scope of v1
//!
//! - `MirrorLink { source, plane_normal, plane_point }` component.
//! - `propagate_mirror_links` system runs each frame; for every
//!   linked mirror whose source Transform has changed since the last
//!   scan, recompute the mirrored Transform and write it.
//! - Position + rotation reflect correctly; scale is not mirrored
//!   (scale-through-plane isn't meaningful for non-negative sizes).
//! - Delete-cascade: if the source entity despawns, the mirror stays
//!   (preserves user work) but becomes unlinked.
//!
//! ## What's deferred
//!
//! Wiring this into `ModelReflect` as a "Linked" option toggle is the
//! ModelReflect-side follow-up — the infrastructure here lets ModelReflect
//! insert the component on spawn, the runtime handles the rest.

use bevy::prelude::*;

/// Attaches to a mirrored entity, pointing back at its source.
/// While present, `propagate_mirror_links` keeps the mirror's Transform
/// in sync with a reflection of the source.
#[derive(Component, Debug, Clone, Copy)]
pub struct MirrorLink {
    pub source: Entity,
    /// Plane normal — `Vec3::Y` for XZ plane, etc.
    pub plane_normal: Vec3,
    /// Plane point — any point on the plane; origin is common.
    pub plane_point: Vec3,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct MirrorLinkPlugin;

impl Plugin for MirrorLinkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, propagate_mirror_links);
    }
}

// ============================================================================
// System
// ============================================================================

/// Reflect the source Transform onto each linked mirror. Runs every
/// frame; cheap because it only reads `Changed<Transform>` on sources
/// and writes Transform on the mirror.
///
/// `source_q` excludes `MirrorLink` to guarantee disjoint Transform
/// access with `mirror_q` (which only matches entities that DO carry
/// `MirrorLink`). Without this filter Bevy 0.18 flags a B0001
/// query-conflict panic: the same Transform column would be read
/// through `source_q` and written through `mirror_q` without a
/// disjoint filter. A mirror that also acts as a source isn't a
/// meaningful case anyway — it would loop back on itself.
fn propagate_mirror_links(
    source_q: Query<&Transform, (Changed<Transform>, Without<MirrorLink>)>,
    mut mirror_q: Query<(&MirrorLink, &mut Transform), Without<crate::selection_box::Selected>>,
) {
    for (link, mut mirror_t) in mirror_q.iter_mut() {
        let Ok(source_t) = source_q.get(link.source) else { continue; };
        let n = link.plane_normal.normalize();
        let p = link.plane_point;

        // Reflect position across the plane: p' = p - 2 ((p - plane_point) · n) n
        let rel = source_t.translation - p;
        let reflected_pos = source_t.translation - 2.0 * rel.dot(n) * n;

        // Reflect rotation: flip rotation axis through the plane + negate angle.
        let (axis, angle) = source_t.rotation.to_axis_angle();
        let reflected_axis = axis - 2.0 * axis.dot(n) * n;
        let reflected_rot = if reflected_axis.length_squared() > 1e-6 {
            Quat::from_axis_angle(reflected_axis.normalize(), -angle)
        } else {
            source_t.rotation
        };

        mirror_t.translation = reflected_pos;
        mirror_t.rotation    = reflected_rot;
        // Scale NOT mirrored — keeps sizes positive and avoids
        // inverted normals from a negative scale component.
    }
}
