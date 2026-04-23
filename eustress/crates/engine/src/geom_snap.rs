//! # Vertex / Edge / Face Snap (Phase 1)
//!
//! Modifier-key-driven snap target resolution. Hold V, E, or F during
//! a Move drag to force the snap category; otherwise the highest-
//! priority hit wins (vertex > edge > face > grid).
//!
//! ## Scope of v1
//!
//! Ships the snap-target resolver, modifier-key detection, and a
//! `GeomSnapState` resource that Move can read at drag time. The
//! resolver operates on **primitive-part analytic geometry** (cuboid
//! corners + edges + face centers) — it doesn't need arbitrary mesh
//! data because Eustress's authoring primitives cover the common
//! cases (Block / Wedge / Cylinder / Sphere). MeshPart support is a
//! follow-up using the common/mesh vertex buffers.
//!
//! ## Priority
//!
//! When no modifier is held:
//!   1. Nearest vertex within threshold
//!   2. Nearest edge-midpoint within threshold
//!   3. Nearest face-center within threshold
//!   4. Fall through to grid snap (handled by Move directly)
//!
//! When modifier held: restrict to that category only. Miss ⇒ no snap.

use bevy::prelude::*;
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapCategory {
    Vertex,
    Edge,
    Face,
    None,
}

impl SnapCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            SnapCategory::Vertex => "vertex",
            SnapCategory::Edge   => "edge",
            SnapCategory::Face   => "face",
            SnapCategory::None   => "",
        }
    }
}

/// Resolved snap-target candidate — passed back to the Move tool
/// during drag so it can override cursor-derived position with the
/// snap point.
#[derive(Debug, Clone, Copy)]
pub struct SnapTarget {
    pub point: Vec3,
    pub category: SnapCategory,
    /// Target entity that owns this vertex/edge/face — for visual
    /// highlight.
    pub entity: Entity,
}

#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct GeomSnapState {
    /// Forced category (V/E/F held). None = auto-priority.
    pub forced: Option<SnapCategory>,
    /// Most-recent resolved target for visual highlight.
    pub last_target: Option<SnapTarget>,
    /// Hit threshold in world units — matches the handle sizing
    /// floor so clicks feel consistent.
    pub threshold: f32,
}

// ============================================================================
// Modifier-key system
// ============================================================================

fn update_forced_category(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GeomSnapState>,
) {
    let held = |k| keys.pressed(k);
    state.forced = if held(KeyCode::KeyV) {
        Some(SnapCategory::Vertex)
    } else if held(KeyCode::KeyE) {
        Some(SnapCategory::Edge)
    } else if held(KeyCode::KeyF) {
        Some(SnapCategory::Face)
    } else {
        None
    };
    if state.threshold <= 0.0 {
        state.threshold = 0.5; // sensible default in studs
    }
}

// ============================================================================
// Resolver — compute the nearest snap target to a world point
// ============================================================================

/// Snap candidate as a simple value type — tools pass a slice of these
/// rather than a concrete Query so the resolver is query-shape
/// agnostic. Downstream callers feed live parts from their own
/// `unselected_query` or similar.
pub struct SnapCandidate {
    pub entity: Entity,
    pub transform: Transform,
    pub size: Vec3,
}

/// Given a list of candidate entities (typically unselected parts) and
/// a cursor world position, pick the best snap target per
/// [`GeomSnapState`]. Pure function — no side effects.
///
/// The candidate entities should exclude those being dragged so the
/// leader doesn't snap to its own corners.
pub fn resolve_snap_target(
    cursor_world: Vec3,
    state: &GeomSnapState,
    candidates: &[SnapCandidate],
) -> Option<SnapTarget> {
    let threshold = state.threshold.max(0.01);

    let mut best_vertex: Option<(f32, Vec3, Entity)> = None;
    let mut best_edge_mid: Option<(f32, Vec3, Entity)> = None;
    let mut best_face_ctr: Option<(f32, Vec3, Entity)> = None;

    for cand in candidates {
        let t = cand.transform;
        let entity = cand.entity;
        let half = cand.size * 0.5;

        // Vertices — 8 corners of the OBB.
        for corner in OBB_CORNERS {
            let local = Vec3::new(corner[0] * half.x, corner[1] * half.y, corner[2] * half.z);
            let world = t.translation + t.rotation * local;
            let d = (world - cursor_world).length();
            if d < threshold {
                if best_vertex.map_or(true, |(bd, _, _)| d < bd) {
                    best_vertex = Some((d, world, entity));
                }
            }
        }

        // Edge midpoints — 12 edges of the OBB.
        for (a, b) in OBB_EDGES {
            let la = Vec3::new(a[0] * half.x, a[1] * half.y, a[2] * half.z);
            let lb = Vec3::new(b[0] * half.x, b[1] * half.y, b[2] * half.z);
            let mid_local = (la + lb) * 0.5;
            let mid_world = t.translation + t.rotation * mid_local;
            let d = (mid_world - cursor_world).length();
            if d < threshold {
                if best_edge_mid.map_or(true, |(bd, _, _)| d < bd) {
                    best_edge_mid = Some((d, mid_world, entity));
                }
            }
        }

        // Face centers — 6 faces, along ±X/Y/Z.
        for (axis, sign) in FACE_NORMALS {
            let local = match axis {
                0 => Vec3::new(sign * half.x, 0.0, 0.0),
                1 => Vec3::new(0.0, sign * half.y, 0.0),
                _ => Vec3::new(0.0, 0.0, sign * half.z),
            };
            let world = t.translation + t.rotation * local;
            let d = (world - cursor_world).length();
            if d < threshold {
                if best_face_ctr.map_or(true, |(bd, _, _)| d < bd) {
                    best_face_ctr = Some((d, world, entity));
                }
            }
        }
    }

    // Apply priority / forced filter.
    let pick_vertex = || best_vertex.map(|(_, p, e)| SnapTarget { point: p, category: SnapCategory::Vertex, entity: e });
    let pick_edge   = || best_edge_mid.map(|(_, p, e)| SnapTarget { point: p, category: SnapCategory::Edge, entity: e });
    let pick_face   = || best_face_ctr.map(|(_, p, e)| SnapTarget { point: p, category: SnapCategory::Face, entity: e });

    match state.forced {
        Some(SnapCategory::Vertex) => pick_vertex(),
        Some(SnapCategory::Edge)   => pick_edge(),
        Some(SnapCategory::Face)   => pick_face(),
        Some(SnapCategory::None) | None => {
            // Priority: vertex > edge > face
            pick_vertex().or_else(pick_edge).or_else(pick_face)
        }
    }
}

// Keep the `_` AABB util imported even though we don't use it here —
// future MeshPart support will need it for unioned bounds.
#[allow(dead_code)]
fn _silence_aabb_unused(t: &Transform, half: Vec3) -> (Vec3, Vec3) {
    calculate_rotated_aabb(t.translation, half, t.rotation)
}

// ============================================================================
// OBB corner / edge tables
// ============================================================================

const OBB_CORNERS: [[f32; 3]; 8] = [
    [-1.0, -1.0, -1.0], [ 1.0, -1.0, -1.0],
    [-1.0,  1.0, -1.0], [ 1.0,  1.0, -1.0],
    [-1.0, -1.0,  1.0], [ 1.0, -1.0,  1.0],
    [-1.0,  1.0,  1.0], [ 1.0,  1.0,  1.0],
];

// 12 edges — indexed by corner pairs (skipping permutations already
// covered).
const OBB_EDGES: [([f32; 3], [f32; 3]); 12] = [
    // Bottom face (-Y)
    ([-1.0, -1.0, -1.0], [ 1.0, -1.0, -1.0]),
    ([ 1.0, -1.0, -1.0], [ 1.0, -1.0,  1.0]),
    ([ 1.0, -1.0,  1.0], [-1.0, -1.0,  1.0]),
    ([-1.0, -1.0,  1.0], [-1.0, -1.0, -1.0]),
    // Top face (+Y)
    ([-1.0,  1.0, -1.0], [ 1.0,  1.0, -1.0]),
    ([ 1.0,  1.0, -1.0], [ 1.0,  1.0,  1.0]),
    ([ 1.0,  1.0,  1.0], [-1.0,  1.0,  1.0]),
    ([-1.0,  1.0,  1.0], [-1.0,  1.0, -1.0]),
    // Verticals connecting bottom to top.
    ([-1.0, -1.0, -1.0], [-1.0,  1.0, -1.0]),
    ([ 1.0, -1.0, -1.0], [ 1.0,  1.0, -1.0]),
    ([ 1.0, -1.0,  1.0], [ 1.0,  1.0,  1.0]),
    ([-1.0, -1.0,  1.0], [-1.0,  1.0,  1.0]),
];

const FACE_NORMALS: [(u8, f32); 6] = [
    (0, -1.0), (0, 1.0),
    (1, -1.0), (1, 1.0),
    (2, -1.0), (2, 1.0),
];

// ============================================================================
// Plugin
// ============================================================================

pub struct GeomSnapPlugin;

impl Plugin for GeomSnapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GeomSnapState>()
            .add_systems(Update, update_forced_category);
    }
}
