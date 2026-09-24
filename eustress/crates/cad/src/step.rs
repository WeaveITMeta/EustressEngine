//! STEP (ISO 10303-21, AP214) export.
//!
//! The dependency on `truck-stepio` was declared from the start and never
//! called. Two things stood between it and a usable file:
//!
//! 1. **Units.** The writer declares millimetres
//!    (`SI_UNIT(.MILLI.,.METRE.)`) and writes coordinates as given. The
//!    kernel is metre-native, so an unscaled export opens 1000x too small
//!    in every other CAD package. Geometry is scaled by 1000 on the way out.
//!
//! 2. **Boolean edges.** Every edge a boolean creates is an
//!    `IntersectionCurve`, and truck-stepio 0.3 writes one as an
//!    `INTERSECTION_CURVE` whose second surface is emitted under the FIRST
//!    surface's entity id (`self.surface1().fmt(surface0_idx, f)` in
//!    `out/geometry.rs`). Two entities share a number, so any part with a
//!    hole in it would export as a file other tools reject. Such edges are
//!    replaced by their leader curve (the B-spline or polyline truck
//!    already keeps as the curve's approximation), which STEP writes as a
//!    plain B-spline through the same vertices.

use truck_modeling::*;
use truck_stepio::out::{CompleteStepDisplay, StepHeaderDescriptor, StepModels};

use crate::error::{CadError, CadResult};
use crate::topology::Body;

/// Metres to the millimetres the STEP writer declares.
const MM_PER_M: f64 = 1000.0;

/// The STEP text for `bodies`, one solid per body.
pub fn step_string(bodies: &[Body], name: &str) -> CadResult<String> {
    if bodies.is_empty() {
        return Err(CadError::EvalFailed { feature: "STEP".into(), reason: "the part has no bodies to export".into() });
    }
    let prepared: Vec<Solid> = bodies.iter().map(|b| exportable(&b.solid)).collect();
    let compressed: Vec<_> = prepared.iter().map(|s| s.compress()).collect();
    let models: StepModels<'_, Point3, Curve, Surface> = compressed.iter().collect();
    let header = StepHeaderDescriptor {
        file_name: format!("{name}.step"),
        organization_system: "Eustress CAD".to_string(),
        ..Default::default()
    };
    Ok(CompleteStepDisplay::new(models, header).to_string())
}

/// Write `bodies` to `path` as STEP.
pub fn write_step(bodies: &[Body], name: &str, path: &std::path::Path) -> CadResult<()> {
    let s = step_string(bodies, name)?;
    std::fs::write(path, s).map_err(|e| CadError::Io(format!("write {path:?}: {e}")))
}

/// A copy of `solid` in millimetres with no intersection curves left.
fn exportable(solid: &Solid) -> Solid {
    let mm = builder::transformed(solid, Matrix4::from_scale(MM_PER_M));
    mm.mapped(
        |p: &Point3| *p,
        |c: &Curve| match c {
            Curve::IntersectionCurve(ic) => leader_curve(ic.leader()),
            other => other.clone(),
        },
        |s: &Surface| s.clone(),
    )
}

fn leader_curve(l: &Leader) -> Curve {
    match l {
        Leader::BSpline(b) => Curve::BSplineCurve(b.clone()),
        Leader::Polyline(p) => {
            let pts = p.0.clone();
            match pts.len() {
                0 | 1 => {
                    let a = pts.first().copied().unwrap_or_else(Point3::origin);
                    Curve::Line(Line(a, a))
                }
                2 => Curve::Line(Line(pts[0], pts[1])),
                n => Curve::BSplineCurve(BSplineCurve::new(KnotVec::uniform_knot(1, n - 1), pts)),
            }
        }
    }
}
