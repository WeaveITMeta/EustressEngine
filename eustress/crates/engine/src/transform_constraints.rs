//! # Transform Constraints (Phase 2)
//!
//! Non-physical editor constraints on Transform components — align-to
//! (keep this part's axis parallel to a reference axis), distribute-to
//! (keep N parts evenly spaced along an axis between two bookends),
//! lock-axis (pin a position component to its initial value).
//!
//! These are **authoring-time** constraints — they run each frame
//! as an editor system and correct drift, but they do NOT become
//! Avian physics constraints. Once the user's happy with the layout,
//! they can remove the constraint components and the layout sticks.
//!
//! ## Components
//!
//! - `AlignToAxis { reference, axis, world_axis }` — keep the entity's
//!   chosen local axis aligned with a reference entity's chosen local
//!   axis, or with a world axis.
//! - `DistributeAlong { bookend_a, bookend_b, slot, total }` — this
//!   entity is slot `slot / total` of N evenly-distributed parts
//!   between `bookend_a` and `bookend_b`.
//! - `LockAxis { axes: {x,y,z} }` — each true component freezes that
//!   translation coordinate to its value at the time the component
//!   was added.
//!
//! ## Scope of v1
//!
//! Ships the three components + per-frame solver systems. UI wiring
//! (Explorer toggle, ribbon button) lands in a follow-up.

use bevy::prelude::*;

// ============================================================================
// Components
// ============================================================================

/// Axis identifier — reused across all three constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CAxis { X, Y, Z }

impl CAxis {
    pub fn to_vec3(self) -> Vec3 {
        match self {
            CAxis::X => Vec3::X,
            CAxis::Y => Vec3::Y,
            CAxis::Z => Vec3::Z,
        }
    }
}

/// Keep this entity's chosen local axis aligned with a reference axis.
/// When `reference` is `None`, aligns to the world axis `world_axis`.
#[derive(Component, Debug, Clone, Copy)]
pub struct AlignToAxis {
    pub reference: Option<Entity>,
    /// Which of our local axes to align.
    pub our_axis: CAxis,
    /// Reference's local axis (ignored when `reference` is None —
    /// then `world_axis` is used).
    pub ref_axis: CAxis,
    pub world_axis: Vec3,
}

/// This entity is slot `slot / total` of N evenly-distributed parts
/// between two bookend entities. A per-frame system positions the
/// entity at `lerp(a, b, slot / (total - 1))`.
#[derive(Component, Debug, Clone, Copy)]
pub struct DistributeAlong {
    pub bookend_a: Entity,
    pub bookend_b: Entity,
    pub slot: u32,
    pub total: u32,
}

/// Per-axis lock — when true, that translation component is pinned
/// to `pinned_value` and can't drift.
#[derive(Component, Debug, Clone, Copy)]
pub struct LockAxis {
    pub lock_x: bool,
    pub lock_y: bool,
    pub lock_z: bool,
    pub pinned_value: Vec3,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct TransformConstraintsPlugin;

impl Plugin for TransformConstraintsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            solve_align_to_axis,
            solve_distribute_along,
            solve_lock_axis,
        ));
    }
}

// ============================================================================
// Solver systems
// ============================================================================

fn solve_align_to_axis(
    ref_q: Query<&Transform, Without<AlignToAxis>>,
    mut q: Query<(&AlignToAxis, &mut Transform)>,
) {
    for (cstr, mut t) in q.iter_mut() {
        let target_world = if let Some(r) = cstr.reference {
            ref_q.get(r).ok()
                .map(|rt| (rt.rotation * cstr.ref_axis.to_vec3()).normalize())
                .unwrap_or(cstr.world_axis.normalize_or_zero())
        } else {
            cstr.world_axis.normalize_or_zero()
        };
        if target_world.length_squared() < 1e-6 { continue; }

        let current_world = (t.rotation * cstr.our_axis.to_vec3()).normalize();
        if (current_world - target_world).length_squared() < 1e-6 { continue; }

        // Compose a correction rotation onto the existing orientation.
        let correction = Quat::from_rotation_arc(current_world, target_world);
        t.rotation = correction * t.rotation;
    }
}

fn solve_distribute_along(
    bookend_q: Query<&Transform, Without<DistributeAlong>>,
    mut q: Query<(&DistributeAlong, &mut Transform)>,
) {
    for (cstr, mut t) in q.iter_mut() {
        if cstr.total < 2 { continue; }
        let Ok(a) = bookend_q.get(cstr.bookend_a) else { continue };
        let Ok(b) = bookend_q.get(cstr.bookend_b) else { continue };
        let s = cstr.slot as f32 / (cstr.total - 1) as f32;
        t.translation = a.translation.lerp(b.translation, s);
    }
}

fn solve_lock_axis(
    mut q: Query<(&LockAxis, &mut Transform)>,
) {
    for (lock, mut t) in q.iter_mut() {
        if lock.lock_x { t.translation.x = lock.pinned_value.x; }
        if lock.lock_y { t.translation.y = lock.pinned_value.y; }
        if lock.lock_z { t.translation.z = lock.pinned_value.z; }
    }
}
