//! Phase 1 — Gaussian-Splatting → physics collider extraction (scaffold).
//!
//! The pipeline (battle plan §4):
//!
//! ```text
//! splats → surface extract (2DGS / GOF) → decimate (eustress-mesh-edit)
//!        → convex decomposition (CoACD/V-HACD)  OR  CSG primitive fit (truck)
//!        → Avian colliders  (collider-from-mesh is enabled workspace-wide)
//! ```
//!
//! The splats remain the only rendered surface; colliders are an invisible
//! proxy. This module defines the intended API surface so the integration seam
//! is stable. **The extraction itself is not implemented yet** — it depends on a
//! surface-extraction path that does not exist in the Rust ecosystem today and
//! is the first real Phase-1 work item.

/// How an extracted proxy surface is turned into physics colliders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColliderStrategy {
    /// Triangle-mesh collider — static, non-moving scene shells (floors, walls).
    #[default]
    TriMesh,
    /// Convex decomposition (CoACD preferred, V-HACD fallback) — interactive
    /// dynamic objects. Cap hull count (~16–64) to keep contact cheap.
    ConvexDecomposition,
    /// CSG primitive fit (boxes / capsules / cylinders) via the truck kernel —
    /// lowest collider count, cleanest contact, parametric and editable. Best
    /// for blocky / architectural scans.
    CsgPrimitiveFit,
}

/// A vendor-neutral collider primitive. radiance has no Avian dependency, so it
/// emits these and the ENGINE converts them to Avian colliders (keeps the
/// renderer physics-engine-agnostic).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColliderPrimitive {
    /// Axis-aligned box: world-space center + half-extents.
    Box { center: [f32; 3], half_extents: [f32; 3] },
    /// Sphere: center + radius.
    Sphere { center: [f32; 3], radius: f32 },
}

/// A compound collider proxy extracted from a radiance field — the invisible
/// physics shadow of the splats.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompoundProxy {
    pub primitives: Vec<ColliderPrimitive>,
    pub strategy: ColliderStrategy,
}

/// Extract a coarse, INVISIBLE physics proxy from a splat / point cloud (Tier A
/// of the battle-plan §4 ladder).
///
/// Tier A = **voxel-occupancy box fit**: bucket points into a `voxel_size` grid
/// and emit one box per occupied cell. Coarse, but REAL — captured worlds become
/// collidable TODAY with no surface-extraction dependency (the blocker the finer
/// tiers wait on). `strategy` is recorded for the engine-side conversion; the
/// ConvexDecomposition / CsgPrimitiveFit tiers (2DGS/GOF surface → decimate →
/// CoACD or truck-CSG) refine this proxy later. Deterministic (sorted cells) so
/// extraction replays bit-identically.
pub fn extract_colliders(
    points: &[[f32; 3]],
    strategy: ColliderStrategy,
    voxel_size: f32,
) -> CompoundProxy {
    use std::collections::BTreeSet;
    let mut proxy = CompoundProxy { primitives: Vec::new(), strategy };
    if points.is_empty() || !(voxel_size > 0.0) {
        return proxy;
    }
    let inv = 1.0 / voxel_size;
    let half = voxel_size * 0.5;
    // BTreeSet → deterministic cell ordering (replayable extraction).
    let mut cells: BTreeSet<(i64, i64, i64)> = BTreeSet::new();
    for p in points {
        cells.insert((
            (p[0] * inv).floor() as i64,
            (p[1] * inv).floor() as i64,
            (p[2] * inv).floor() as i64,
        ));
    }
    proxy.primitives.reserve(cells.len());
    for (cx, cy, cz) in cells {
        let center = [
            (cx as f32 + 0.5) * voxel_size,
            (cy as f32 + 0.5) * voxel_size,
            (cz as f32 + 0.5) * voxel_size,
        ];
        proxy
            .primitives
            .push(ColliderPrimitive::Box { center, half_extents: [half, half, half] });
    }
    proxy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voxel_box_fit_dedups_cells() {
        // Three points: two share a 1.0 voxel cell, one is in another → 2 boxes.
        let points = [[0.1, 0.1, 0.1], [0.9, 0.2, 0.3], [5.0, 0.0, 0.0]];
        let proxy = extract_colliders(&points, ColliderStrategy::TriMesh, 1.0);
        assert_eq!(proxy.primitives.len(), 2);
        assert_eq!(proxy.strategy, ColliderStrategy::TriMesh);
    }

    #[test]
    fn empty_or_bad_input_is_empty() {
        assert!(extract_colliders(&[], ColliderStrategy::default(), 1.0).primitives.is_empty());
        assert!(extract_colliders(&[[0.0; 3]], ColliderStrategy::default(), 0.0).primitives.is_empty());
    }
}
