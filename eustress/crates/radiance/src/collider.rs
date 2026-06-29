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

// TODO(Phase 1): `pub fn extract_colliders(cloud, strategy) -> Compound`.
// Blocked on a surface-extraction step (2DGS/GOF → mesh). Once a proxy mesh
// exists, decimate via `eustress-mesh-edit`, then either Avian
// `Collider::convex_decomposition` (collider-from-mesh) or a `eustress-cad`
// (truck) CSG primitive fit. Tracked in
// docs/architecture/GAUSSIAN_SPLATTING_BATTLE_PLAN.md §4.
