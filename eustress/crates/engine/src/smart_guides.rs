//! # Smart Alignment Guides (Phase 1)
//!
//! Blender/Fusion-style alignment guides — during drag, if the leader's
//! AABB center / edges / faces line up with any unselected part's
//! center / edges / faces (within a pixel threshold), show a guide
//! line + snap. Speeds up common building tasks like "line this wall
//! up with that wall's inside face."
//!
//! ## Scope of v1
//!
//! Ships the sensor resource + per-frame scan that populates a list of
//! candidate alignment lines, plus a `GuideHit` resolver the Move-tool
//! drag path can read to apply a snap. Visual rendering of the guide
//! lines (dashed world-space segments via HandleAdornment) is stubbed
//! here and fully wired once Move-tool integration lands.
//!
//! ## Algorithm
//!
//! For each non-selected part's world-space AABB, emit three candidate
//! planes per axis (min / center / max). During drag, test the leader
//! AABB's corresponding coordinates against each plane; any within
//! `threshold` is a hit. Closest hit wins per axis.
//!
//! **Complexity note**: v1 is `O(N)` per frame against all unselected
//! parts. The doc calls out R-tree as the target implementation — that
//! lands in v2 once we have > 1k entities in a universe and see
//! measurable frame cost. Today's typical universe size (≤ 200 parts)
//! doesn't justify the complexity.

use bevy::prelude::*;
use crate::selection_box::Selected;
use crate::classes::BasePart;
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// State
// ============================================================================

/// A single candidate alignment plane — the drag pathway tests the
/// leader's coordinate on this axis against `value` and applies a snap
/// if within threshold.
#[derive(Debug, Clone, Copy)]
pub struct GuidePlane {
    pub axis: u8,   // 0 = X, 1 = Y, 2 = Z
    pub value: f32, // world-space coordinate along `axis`
    pub source: Entity, // which part contributed this plane
    pub kind: GuideKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    Min,
    Center,
    Max,
}

#[derive(Resource, Default)]
pub struct SmartGuidesState {
    /// Per-frame scan output — consumed by the Move tool's drag path.
    pub planes: Vec<GuidePlane>,
    /// Hit threshold in world units. Defaults to 0.1 studs (~10 cm).
    pub threshold: f32,
    /// User toggle — when off, the sensor skips scanning entirely.
    pub enabled: bool,
}

// ============================================================================
// Sensor — populate candidate planes every frame
// ============================================================================

fn refresh_guides(
    mut state: ResMut<SmartGuidesState>,
    candidates: Query<(Entity, &GlobalTransform, &BasePart), Without<Selected>>,
) {
    // Honor user toggle + cheap exit when nothing could snap.
    if !state.enabled {
        state.planes.clear();
        return;
    }
    if state.threshold <= 0.0 {
        state.threshold = 0.1;
    }

    state.planes.clear();
    state.planes.reserve(candidates.iter().count() * 9);

    for (entity, gt, bp) in candidates.iter() {
        let t = gt.compute_transform();
        let (mn, mx) = calculate_rotated_aabb(t.translation, bp.size * 0.5, t.rotation);
        let center = (mn + mx) * 0.5;
        for axis in 0..3 {
            let min_v = match axis { 0 => mn.x, 1 => mn.y, _ => mn.z };
            let ctr_v = match axis { 0 => center.x, 1 => center.y, _ => center.z };
            let max_v = match axis { 0 => mx.x, 1 => mx.y, _ => mx.z };
            state.planes.push(GuidePlane { axis, value: min_v, source: entity, kind: GuideKind::Min });
            state.planes.push(GuidePlane { axis, value: ctr_v, source: entity, kind: GuideKind::Center });
            state.planes.push(GuidePlane { axis, value: max_v, source: entity, kind: GuideKind::Max });
        }
    }
}

// ============================================================================
// Resolver — given leader AABB, find best alignment hits
// ============================================================================

/// Result of testing the leader's three axes against all candidate
/// planes — one hit per axis if any was within threshold.
#[derive(Debug, Default, Clone, Copy)]
pub struct GuideSnap {
    pub x: Option<GuidePlane>,
    pub y: Option<GuidePlane>,
    pub z: Option<GuidePlane>,
}

/// For a given leader-AABB (in world space), pick the closest
/// alignment plane per axis. Returns `None`-fields if no plane is
/// within threshold. Drag code reads the per-axis offsets and applies
/// them to the leader translation.
pub fn resolve_guide_snap(
    state: &SmartGuidesState,
    leader_min: Vec3,
    leader_max: Vec3,
) -> GuideSnap {
    let leader_center = (leader_min + leader_max) * 0.5;
    let mut snap = GuideSnap::default();

    for plane in &state.planes {
        let (leader_min_v, leader_ctr_v, leader_max_v) = match plane.axis {
            0 => (leader_min.x, leader_center.x, leader_max.x),
            1 => (leader_min.y, leader_center.y, leader_max.y),
            _ => (leader_min.z, leader_center.z, leader_max.z),
        };
        // Match the leader's edge / center most aligned with this
        // plane kind.
        let leader_coord = match plane.kind {
            GuideKind::Min    => leader_min_v,
            GuideKind::Center => leader_ctr_v,
            GuideKind::Max    => leader_max_v,
        };
        let d = (plane.value - leader_coord).abs();
        if d > state.threshold { continue; }

        let slot = match plane.axis {
            0 => &mut snap.x,
            1 => &mut snap.y,
            _ => &mut snap.z,
        };
        let better = match slot {
            None => true,
            Some(existing) => d < (existing.value - leader_coord).abs(),
        };
        if better { *slot = Some(*plane); }
    }

    snap
}

// ============================================================================
// Plugin
// ============================================================================

pub struct SmartGuidesPlugin;

impl Plugin for SmartGuidesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SmartGuidesState>()
            .add_systems(Update, refresh_guides);
    }
}
