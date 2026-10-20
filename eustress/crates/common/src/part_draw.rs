//! # What a drawn part carries
//!
//! The mesh file a part was drawn from and the part it stands for, shared
//! by Studio's spawners and Play's apply step.

use bevy::prelude::*;

use crate::classes::PartType;

/// Tracks the source .glb file for a part's mesh (file-system-first architecture).
/// When present, the mesh was loaded from this path rather than generated inline.
/// The Scale Tool uses Transform.scale instead of regenerating the mesh.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct MeshSource {
    /// Relative path to the .glb file (from engine assets root)
    pub path: String,
}

impl MeshSource {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// Map PartType to the corresponding .glb file path in assets/parts/
pub fn part_type_to_glb_path(part_type: &PartType) -> &'static str {
    match part_type {
        PartType::Block => "parts/block.glb",
        PartType::Ball => "parts/ball.glb",
        PartType::Cylinder => "parts/cylinder.glb",
        PartType::Wedge => "parts/wedge.glb",
        PartType::CornerWedge => "parts/corner_wedge.glb",
        PartType::Cone => "parts/cone.glb",
    }
}

/// Component to track which part this entity represents
#[derive(Component)]
pub struct PartEntity {
    pub part_id: String,
}

/// Local-space collider half-extents that, after Avian re-applies the
/// entity's (global) Transform scale to the collider, yield a *world*
/// collider matching the part's visible `size`.
///
/// ## Why (verified 2026-05-25 against live scene + Avian 0.6 source)
///
/// Parts render a UNIT GLB mesh scaled by `Transform.scale`, and Avian
/// scales the collider by the SAME accumulated scale
/// (`propagate_collider_transforms` assigns `ColliderTransform.scale` =
/// global scale; `update_collider_scale`'s child-collider path applies it
/// ungated). Building at `size/2` therefore double-counts the scale: a part
/// of size `s` (whose local `Transform.scale == s`, parent Model at scale 1)
/// got a world collider of `(s/2)·s = s²/2` half-extent — verified by the
/// non-linear signature the user saw (size 3 → 9 wide, size 5.2 → 27 wide,
/// size 0.9 → 0.81). Dividing the local half-extents by the local Transform
/// scale cancels Avian's multiply so the world collider resolves to `size/2`
/// (a UNIT collider when `scale == size`). The cancellation is robust to
/// ancestor (folder/Model) scale: Avian applies `A·S`, and
/// `(size/2 / S)·(A·S) = size/2 · A`, which equals the visible extent for
/// both the unit-mesh convention (`S=size`) and the baked-mesh one (`S=1`).
pub fn collider_local_half(size: Vec3, transform_scale: Vec3) -> Vec3 {
    let inv = |s: f32| if s.is_finite() && s.abs() > 1e-4 { 1.0 / s.abs() } else { 1.0 };
    Vec3::new(
        size.x * 0.5 * inv(transform_scale.x),
        size.y * 0.5 * inv(transform_scale.y),
        size.z * 0.5 * inv(transform_scale.z),
    )
}
