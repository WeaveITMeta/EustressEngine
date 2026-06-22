//! Characterization tests for truck-shapeops 0.4 — pins the upstream
//! behavior eustress-cad works around, so a truck upgrade that
//! changes the behavior surfaces here instead of silently shifting
//! kernel semantics.
//!
//! Findings (2026-06-10, empirically derived):
//! - Difference works as `a AND not(b)` (`Solid::not()` inversion).
//! - shapeops has an ABSOLUTE scale floor: identical part/hole ratios
//!   succeed at unit scale and fail at centimeter scale, at every
//!   tolerance. This is why `eval::boolean_normalized` rescales
//!   operands to unit size around every boolean.
//! - Coplanar/flush operand faces degenerate the op (cuts must
//!   protrude — see the Hole arm's overcut).
//! - Blind cuts (cap face fully interior) work fine at unit scale.
//! - Ordering matters: scale-then-invert works; scaling an already-
//!   inverted solid breaks the op (eval's `boolean_not` inverts
//!   inside the normalized op for this reason).
//! - Boolean RESULTS carry IntersectionCurve edges whose surface
//!   evaluation diverges (panics) at meter scale — tessellation must
//!   re-normalize before triangulating (`tessellate_solid` does).
//! - The success landscape over tolerance is jagged and proportion-
//!   dependent (0.005 can succeed where 0.01 and 0.003 both fail) —
//!   hence the dense tolerance ladder in `eval`.

use truck_modeling::*;

fn cube(size: f64) -> Solid {
    let v = builder::vertex(Point3::origin());
    let e = builder::tsweep(&v, Vector3::unit_x() * size);
    let f = builder::tsweep(&e, Vector3::unit_y() * size);
    builder::tsweep(&f, Vector3::unit_z() * size)
}

/// Cylinder built the way eustress-cad's `extrude_circle` builds it:
/// two half-circle arcs chained into a wire, attached plane, tsweep.
fn cylinder_arcs(cx: f64, cy: f64, r: f64, z0: f64, h: f64) -> Solid {
    let v_r = builder::vertex(Point3::new(cx + r, cy, z0));
    let v_l = builder::vertex(Point3::new(cx - r, cy, z0));
    let top = Point3::new(cx, cy + r, z0);
    let bot = Point3::new(cx, cy - r, z0);
    let arc_upper = builder::circle_arc(&v_r, &v_l, top);
    let arc_lower = builder::circle_arc(&v_l, &v_r, bot);
    let wire: Wire = vec![arc_upper, arc_lower].into();
    let f = builder::try_attach_plane(&[wire]).unwrap();
    builder::tsweep(&f, Vector3::unit_z() * h)
}

fn difference(a: &Solid, b: &Solid, tol: f64) -> Option<Solid> {
    let mut b_inv = b.clone();
    b_inv.not();
    truck_shapeops::and(a, &b_inv, tol)
}

#[test]
fn difference_via_not_and_works_at_unit_scale() {
    // truck's own punched-cube example geometry, with the cylinder
    // built our way (chained arcs).
    let base = cube(1.0);
    let cyl = cylinder_arcs(0.5, 0.5, 0.25, -0.5, 2.0);
    assert!(difference(&base, &cyl, 0.05).is_some());
}

#[test]
fn blind_cut_works_at_unit_scale() {
    // Cap face fully interior to the cube — no intersection curve on
    // that face, classification alone must keep/drop it.
    let base = cube(1.0);
    let blind = cylinder_arcs(0.5, 0.5, 0.15, -0.5, 1.0);
    assert!(difference(&base, &blind, 0.03).is_some());
}

/// Slab the way `build_planar_face` does it: 4-line wire,
/// try_attach_plane, tsweep.
fn slab_from_wire(lo: f64, hi: f64, h: f64) -> Solid {
    let v00 = builder::vertex(Point3::new(lo, lo, 0.0));
    let v10 = builder::vertex(Point3::new(hi, lo, 0.0));
    let v11 = builder::vertex(Point3::new(hi, hi, 0.0));
    let v01 = builder::vertex(Point3::new(lo, hi, 0.0));
    let wire: Wire = vec![
        builder::line(&v00, &v10),
        builder::line(&v10, &v11),
        builder::line(&v11, &v01),
        builder::line(&v01, &v00),
    ]
    .into();
    let f = builder::try_attach_plane(&[wire]).unwrap();
    builder::tsweep(&f, Vector3::unit_z() * h)
}

/// Cylinder the way `extrude_circle(both_sides=true)` does it: face
/// built at z=0, translated down, then swept the full height.
fn cylinder_translated_face(cx: f64, cy: f64, r: f64, z0: f64, h: f64) -> Solid {
    let v_r = builder::vertex(Point3::new(cx + r, cy, 0.0));
    let v_l = builder::vertex(Point3::new(cx - r, cy, 0.0));
    let top = Point3::new(cx, cy + r, 0.0);
    let bot = Point3::new(cx, cy - r, 0.0);
    let arc_upper = builder::circle_arc(&v_r, &v_l, top);
    let arc_lower = builder::circle_arc(&v_l, &v_r, bot);
    let wire: Wire = vec![arc_upper, arc_lower].into();
    let f = builder::try_attach_plane(&[wire]).unwrap();
    let f = builder::translated(&f, Vector3::new(0.0, 0.0, z0));
    builder::tsweep(&f, Vector3::unit_z() * h)
}

#[test]
fn transform_scaled_solids_boolean_cleanly() {
    // `boolean_normalized` rescales operands via builder::transformed
    // before every op — pin that transform-scaled (not built-at-size)
    // geometry booleans fine at the normalized scale.
    let k = 22.9;
    let slab_small = slab_from_wire(-0.02, 0.02, 0.01);
    let cyl_small = cylinder_translated_face(0.0, 0.0, 0.005, -0.015, 0.03);
    let slab_scaled = builder::transformed(&slab_small, Matrix4::from_scale(k));
    let cyl_scaled = builder::transformed(&cyl_small, Matrix4::from_scale(k));
    assert!(difference(&slab_scaled, &cyl_scaled, 0.005).is_some());
}

/// CANARY — pins the upstream scale-floor bug. Same ratio as the
/// working unit-scale case, 50x smaller, fails at every reasonable
/// tolerance on shapeops 0.4. If this test ever FAILS (i.e. the op
/// starts succeeding), a truck upgrade fixed the floor:
/// `eval::boolean_normalized`'s rescaling can then be retired.
#[test]
fn scale_floor_still_present_upstream() {
    let base = cube(0.02);
    let cyl = cylinder_arcs(0.01, 0.01, 0.005, -0.01, 0.04);
    for tol in [0.005, 0.002, 0.001, 0.0005, 0.0002, 0.0001] {
        assert!(
            difference(&base, &cyl, tol).is_none(),
            "shapeops difference now WORKS at part scale (tol={tol}) — \
             upstream fixed the scale floor; consider removing \
             boolean_normalized's rescale"
        );
    }
}
