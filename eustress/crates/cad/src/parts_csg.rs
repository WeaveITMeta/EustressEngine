//! Oriented-primitive CSG — engine Studio Boolean path.
//!
//! Converts oriented Block / Ball / Cylinder specs into truck solids,
//! runs the same scale-normalized booleans as the feature-tree
//! evaluator, and returns a tessellated mesh for Bevy.
//!
//! Keeps truck types inside this crate so the engine only depends on
//! [`EvalMesh`] + this API.

use truck_base::cgmath64::*;
use truck_modeling::*;

use crate::eval::{
    boolean_and, boolean_not, boolean_or, evaluate_tree, tessellate_solid, EvalMesh,
    DEFAULT_MESH_TOLERANCE,
};
use crate::feature::{BooleanOp, EndCondition, Feature, FeatureOp};
use crate::feature_tree::{FeatureEntry, FeatureTree};
use crate::sketch::{Sketch, SketchEntity};
use crate::{CadError, CadResult};

/// Primitive shape that can be lifted to a truck solid.
#[derive(Debug, Clone, Copy)]
pub enum OrientedShape {
    /// Full extents in local space (not half-extents).
    Block { size: [f64; 3] },
    /// Sphere of the given radius (local origin = center).
    Ball { radius: f64 },
    /// Cylinder along local Z — radius in XY, height along Z
    /// (matches the feature-tree Extrude-of-circle convention).
    Cylinder { radius: f64, height: f64 },
}

/// One solid operand in world space.
#[derive(Debug, Clone)]
pub struct OrientedSolid {
    pub shape: OrientedShape,
    /// World-space translation of the part center.
    pub translation: [f64; 3],
    /// World-space rotation as `[x, y, z, w]` quaternion.
    pub rotation_xyzw: [f64; 4],
}

/// Run a multi-body CSG fold over oriented primitives and return a mesh.
///
/// - **Union**: `A ∪ B ∪ C …`
/// - **Difference**: `A − B − C …` (first body is the target)
/// - **Intersect**: `A ∩ B ∩ C …`
///
/// Requires at least two operands.
pub fn boolean_oriented_solids(op: BooleanOp, parts: &[OrientedSolid]) -> CadResult<EvalMesh> {
    if parts.len() < 2 {
        return Err(CadError::EvalFailed {
            feature: "CSG".into(),
            reason: "need at least 2 solids".into(),
        });
    }

    let mut solids: Vec<Solid> = Vec::with_capacity(parts.len());
    for (i, p) in parts.iter().enumerate() {
        let local = solid_from_shape(&p.shape).map_err(|e| CadError::EvalFailed {
            feature: "CSG".into(),
            reason: format!("solid {i}: {e}"),
        })?;
        let mat = world_matrix(p.translation, p.rotation_xyzw);
        solids.push(builder::transformed(&local, mat));
    }

    let mut acc = solids.remove(0);
    for next in solids {
        let result = match op {
            BooleanOp::Union => boolean_or(&acc, &next),
            BooleanOp::Difference => boolean_not(&acc, &next),
            BooleanOp::Intersect => boolean_and(&acc, &next),
        };
        acc = result.ok_or_else(|| {
            CadError::Kernel(
                "boolean produced no result (non-manifold, coplanar faces, or scale failure)"
                    .into(),
            )
        })?;
    }

    Ok(tessellate_solid(&acc, DEFAULT_MESH_TOLERANCE))
}

fn solid_from_shape(shape: &OrientedShape) -> CadResult<Solid> {
    match *shape {
        OrientedShape::Block { size: [sx, sy, sz] } => {
            let hx = sx.abs() * 0.5;
            let hy = sy.abs() * 0.5;
            let depth = sz.abs().max(1e-6);
            extrude_profile(
                vec![SketchEntity::Rectangle {
                    p1: [-hx, -hy],
                    p2: [hx, hy],
                }],
                depth,
            )
        }
        OrientedShape::Ball { radius } => {
            // v0: axis-aligned bounding cube. True sphere solid needs a
            // revolve-of-semicircle path not yet wired for free profiles.
            // Still useful for Union/Intersect roughing; Difference will
            // leave cubic cutouts until sphere revolve lands.
            let d = radius.abs().max(1e-6) * 2.0;
            solid_from_shape(&OrientedShape::Block {
                size: [d, d, d],
            })
        }
        OrientedShape::Cylinder { radius, height } => {
            let r = radius.abs().max(1e-6);
            let h = height.abs().max(1e-6);
            extrude_profile(
                vec![SketchEntity::Circle {
                    center: [0.0, 0.0],
                    radius: r,
                }],
                h,
            )
        }
    }
}

fn extrude_profile(entities: Vec<SketchEntity>, depth_m: f64) -> CadResult<Solid> {
    let sk = Sketch {
        plane: "xy".into(),
        entities,
        dimensions: vec![],
        constraints: vec![],
    };
    let tree = FeatureTree {
        variables: Default::default(),
        entries: vec![
            FeatureEntry::Sketch {
                name: "S".into(),
                body: sk,
            },
            FeatureEntry::Feature {
                name: "E".into(),
                body: Feature::Extrude {
                    sketch: "S".into(),
                    depth: format!("{depth_m} m"),
                    end_condition: EndCondition::default(),
                    combine: FeatureOp::NewBody,
                    draft_angle: "0 deg".into(),
                    both_sides: true,
                },
            },
        ],
        metadata: Default::default(),
    };
    let out = evaluate_tree(&tree)?;
    out.body.ok_or_else(|| CadError::EvalFailed {
        feature: "CSG".into(),
        reason: "extrude produced no body".into(),
    })
}

fn world_matrix(translation: [f64; 3], rotation_xyzw: [f64; 4]) -> Matrix4 {
    // Expand quaternion → rotation matrix directly: truck's cgmath64
    // module aliases only Vector/Matrix/Point types, so cgmath's
    // Quaternion isn't nameable here without a new dependency. The
    // expansion assumes a unit quaternion — normalize defensively
    // since the input crosses an f32→f64 boundary from Bevy.
    let [x, y, z, w] = rotation_xyzw;
    let n = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = if n > 1.0e-12 {
        (x / n, y / n, z / n, w / n)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    // Column-major argument order, matching eval.rs's reflection_matrix.
    Matrix4::new(
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        0.0,
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        0.0,
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
        translation[0],
        translation[1],
        translation[2],
        1.0,
    )
}
