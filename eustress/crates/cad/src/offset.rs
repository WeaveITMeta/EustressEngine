//! Profile offset: the missing kernel primitive.
//!
//! Offset is the operation that sits underneath a whole family of
//! features the kernel currently fakes or refuses: a hollow shell is an
//! inward offset subtracted from the original; a clearance fit is an
//! outward offset; wall thickness, tolerance bands and seal grooves are
//! all "the same profile, moved perpendicular to itself by d".
//!
//! Because none of that existed, `Shell` shipped as an open-top
//! approximation (a scaled inner body punched out through +Z), and
//! `Fillet`/`Chamfer` shipped as post-tessellation mesh softening. Those
//! are three different workarounds for one absent operation.
//!
//! ## Scope, stated honestly
//!
//! This offsets a **2D profile**, not a 3D BRep. A true solid offset
//! needs offset surfaces plus self-intersection removal, which
//! `truck-modeling` 0.6 does not provide and which is a research problem
//! rather than an afternoon. A 2D profile offset is the tractable
//! subset, and it is enough to build a genuine enclosed shell for the
//! prismatic parts that make up most of what people model: offset the
//! profile inward, extrude it, subtract.
//!
//! ## What it refuses
//!
//! An offset large enough to collapse or invert the profile has no
//! answer, and returning a self-intersecting loop would hand the boolean
//! kernel garbage that fails much later with an unrelated message. Those
//! cases are detected here and reported with the offending distance.

use crate::error::{CadError, CadResult};
use crate::sketch::{Sketch, SketchEntity};

/// Twice the signed area of a closed polygon. Positive = counter-clockwise.
fn signed_area2(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    let mut acc = 0.0;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        acc += a[0] * b[1] - b[0] * a[1];
    }
    acc
}

/// Intersect two lines given as (point, direction). `None` when parallel.
fn line_intersect(
    p1: [f64; 2],
    d1: [f64; 2],
    p2: [f64; 2],
    d2: [f64; 2],
) -> Option<[f64; 2]> {
    let denom = d1[0] * d2[1] - d1[1] * d2[0];
    if denom.abs() < 1.0e-12 {
        return None;
    }
    let dx = p2[0] - p1[0];
    let dy = p2[1] - p1[1];
    let t = (dx * d2[1] - dy * d2[0]) / denom;
    Some([p1[0] + d1[0] * t, p1[1] + d1[1] * t])
}

/// Do closed polygons `pts` self-intersect (ignoring shared endpoints)?
fn self_intersects(pts: &[[f64; 2]]) -> bool {
    let n = pts.len();
    if n < 4 {
        return false;
    }
    let seg = |i: usize| (pts[i], pts[(i + 1) % n]);
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    for i in 0..n {
        for j in (i + 1)..n {
            // Skip adjacent segments; they legitimately share a vertex.
            if j == i || (j + 1) % n == i || (i + 1) % n == j {
                continue;
            }
            let (p1, p2) = seg(i);
            let (q1, q2) = seg(j);
            let d1 = cross(p1, p2, q1);
            let d2 = cross(p1, p2, q2);
            let d3 = cross(q1, q2, p1);
            let d4 = cross(q1, q2, p2);
            if ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) {
                return true;
            }
        }
    }
    false
}

/// Offset a closed polyline by `d`, perpendicular to each edge.
///
/// Positive `d` grows the profile, negative shrinks it, regardless of
/// the winding the caller happened to author. Orientation is
/// normalised internally so "negative means smaller" always holds.
/// Corners are mitred (adjacent offset edges extended to their
/// intersection), which is the join a CAD offset is expected to make.
pub fn offset_closed_polyline(pts: &[[f64; 2]], d: f64) -> CadResult<Vec<[f64; 2]>> {
    if pts.len() < 3 {
        return Err(CadError::EvalFailed {
            feature: "Offset".into(),
            reason: format!("a closed profile needs at least 3 points, got {}", pts.len()),
        });
    }
    if d.abs() < 1.0e-12 {
        return Ok(pts.to_vec());
    }

    // Normalise to counter-clockwise so the inward normal is consistent.
    let area2 = signed_area2(pts);
    if area2.abs() < 1.0e-14 {
        return Err(CadError::EvalFailed {
            feature: "Offset".into(),
            reason: "profile encloses no area (points are collinear or duplicated)".into(),
        });
    }
    let mut work = pts.to_vec();
    let was_cw = area2 < 0.0;
    if was_cw {
        work.reverse();
    }

    let n = work.len();
    // For CCW, the OUTWARD normal of edge (a -> b) is (dy, -dx) normalised.
    let mut offset_lines: Vec<([f64; 2], [f64; 2])> = Vec::with_capacity(n);
    for i in 0..n {
        let a = work[i];
        let b = work[(i + 1) % n];
        let ex = b[0] - a[0];
        let ey = b[1] - a[1];
        let len = (ex * ex + ey * ey).sqrt();
        if len < 1.0e-12 {
            return Err(CadError::EvalFailed {
                feature: "Offset".into(),
                reason: format!("profile has a zero-length edge at point {i}"),
            });
        }
        let nx = ey / len;
        let ny = -ex / len;
        offset_lines.push(([a[0] + nx * d, a[1] + ny * d], [ex, ey]));
    }

    // Each new vertex is where consecutive offset edges meet.
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(n);
    for i in 0..n {
        let prev = offset_lines[(i + n - 1) % n];
        let cur = offset_lines[i];
        match line_intersect(prev.0, prev.1, cur.0, cur.1) {
            Some(p) => out.push(p),
            // Collinear neighbours: the offset edges are the same line,
            // so the offset start point is already the right vertex.
            None => out.push(cur.0),
        }
    }

    // An offset that eats the profile produces a reversed or vanishing
    // loop. Catching it here means the caller gets the distance that was
    // too large, instead of a boolean failing later for reasons that
    // look unrelated.
    //
    // Edge direction is the test, NOT enclosed area. Area alone is a
    // trap: on a symmetric profile every edge crosses to the far side at
    // the same distance, so the loop turns inside out while its winding,
    // and therefore the sign of its area, stays exactly as it was. A
    // 100 mm square offset inward by 60 mm comes back as a tidy 20 mm
    // square wound counter-clockwise, and an area check waves it
    // through. Per edge, the answer is unambiguous: if the offset edge
    // runs opposite to the edge it came from, the offset consumed it.
    for i in 0..n {
        let e = offset_lines[i].1;
        let a = out[i];
        let b = out[(i + 1) % n];
        if (b[0] - a[0]) * e[0] + (b[1] - a[1]) * e[1] <= 0.0 {
            return Err(CadError::EvalFailed {
                feature: "Offset".into(),
                reason: format!(
                    "offset of {d:.6} m consumes the profile: edge {i} is reversed or has \
                     no length left, so the inward distance exceeds half the narrowest span"
                ),
            });
        }
    }

    // `work` is counter-clockwise, so its area is positive by
    // construction. A non-positive area after offsetting means the loop
    // collapsed through itself. Kept as a backstop for the asymmetric
    // cases the per-edge test above does catch, since it costs nothing.
    let new_area2 = signed_area2(&out);
    if new_area2 <= 1.0e-14 {
        return Err(CadError::EvalFailed {
            feature: "Offset".into(),
            reason: format!(
                "offset of {d:.6} m collapses or inverts the profile: the inward distance \
                 exceeds half the narrowest span"
            ),
        });
    }
    if self_intersects(&out) {
        return Err(CadError::EvalFailed {
            feature: "Offset".into(),
            reason: format!(
                "offset of {d:.6} m makes the profile self-intersect at a concave corner; \
                 trimming self-intersections is not implemented, so use a smaller distance"
            ),
        });
    }

    if was_cw {
        out.reverse();
    }
    Ok(out)
}

/// The ordered closed loop of a sketch built from `Line` entities.
///
/// Mirrors the chain walk `build_planar_face` performs, so a profile the
/// extruder accepts is a profile this can offset.
fn line_loop(sk: &Sketch) -> Option<Vec<[f64; 2]>> {
    let mut segs: Vec<([f64; 2], [f64; 2])> = sk
        .entities
        .iter()
        .filter_map(|e| match e {
            SketchEntity::Line { p1, p2 } => Some((*p1, *p2)),
            _ => None,
        })
        .collect();
    if segs.len() < 3 {
        return None;
    }
    let close = |a: [f64; 2], b: [f64; 2]| {
        (a[0] - b[0]).abs() < 1.0e-9 && (a[1] - b[1]).abs() < 1.0e-9
    };
    let (start, mut cursor) = segs.remove(0);
    let mut loop_pts = vec![start, cursor];
    while !segs.is_empty() {
        let Some(idx) = segs.iter().position(|(a, b)| close(*a, cursor) || close(*b, cursor))
        else {
            return None; // chain breaks, so not a closed loop
        };
        let (a, b) = segs.remove(idx);
        cursor = if close(a, cursor) { b } else { a };
        if close(cursor, start) {
            break;
        }
        loop_pts.push(cursor);
    }
    if loop_pts.len() < 3 {
        return None;
    }
    Some(loop_pts)
}

/// Offset every profile in a sketch by `d`, returning the new entities.
///
/// Handles the three profile forms `build_planar_face` recognises, in
/// the same priority order, so anything extrudable is offsettable.
pub fn offset_sketch_entities(sk: &Sketch, d: f64) -> CadResult<Vec<SketchEntity>> {
    // Rectangle: offsetting each side by d moves both corners by d.
    if let Some(SketchEntity::Rectangle { p1, p2 }) = sk
        .entities
        .iter()
        .find(|e| matches!(e, SketchEntity::Rectangle { .. }))
    {
        let (min_x, max_x) = (p1[0].min(p2[0]), p1[0].max(p2[0]));
        let (min_y, max_y) = (p1[1].min(p2[1]), p1[1].max(p2[1]));
        let (nx0, nx1) = (min_x - d, max_x + d);
        let (ny0, ny1) = (min_y - d, max_y + d);
        if nx1 - nx0 < 1.0e-9 || ny1 - ny0 < 1.0e-9 {
            return Err(CadError::EvalFailed {
                feature: "Offset".into(),
                reason: format!(
                    "offset of {d:.6} m collapses the rectangle ({:.4} x {:.4} m)",
                    max_x - min_x,
                    max_y - min_y
                ),
            });
        }
        return Ok(vec![SketchEntity::Rectangle {
            p1: [nx0, ny0],
            p2: [nx1, ny1],
        }]);
    }

    // Circle: the offset of a circle is a concentric circle.
    if let Some(SketchEntity::Circle { center, radius }) = sk
        .entities
        .iter()
        .find(|e| matches!(e, SketchEntity::Circle { .. }))
    {
        let r = radius + d;
        if r <= 1.0e-9 {
            return Err(CadError::EvalFailed {
                feature: "Offset".into(),
                reason: format!(
                    "offset of {d:.6} m collapses the circle (radius {radius:.6} m)"
                ),
            });
        }
        return Ok(vec![SketchEntity::Circle {
            center: *center,
            radius: r,
        }]);
    }

    // Closed polyline.
    let Some(loop_pts) = line_loop(sk) else {
        return Err(CadError::EvalFailed {
            feature: "Offset".into(),
            reason: "sketch has no offsettable profile. Expected a rectangle, a circle, or at \
                     least 3 lines forming a closed loop"
                .into(),
        });
    };
    let offset_pts = offset_closed_polyline(&loop_pts, d)?;
    let n = offset_pts.len();
    Ok((0..n)
        .map(|i| SketchEntity::Line {
            p1: offset_pts[i],
            p2: offset_pts[(i + 1) % n],
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: f64) -> Vec<[f64; 2]> {
        vec![[-s, -s], [s, -s], [s, s], [-s, s]]
    }

    #[test]
    fn outward_offset_grows_a_square() {
        let out = offset_closed_polyline(&sq(0.05), 0.01).unwrap();
        assert_eq!(out.len(), 4);
        let a = signed_area2(&out).abs() / 2.0;
        // 100mm square grown 10mm per side -> 120mm square.
        assert!((a - 0.12 * 0.12).abs() < 1e-9, "area {a}");
    }

    #[test]
    fn inward_offset_shrinks_a_square() {
        let out = offset_closed_polyline(&sq(0.05), -0.01).unwrap();
        let a = signed_area2(&out).abs() / 2.0;
        assert!((a - 0.08 * 0.08).abs() < 1e-9, "area {a}");
    }

    #[test]
    fn winding_does_not_change_the_meaning_of_negative() {
        let mut cw = sq(0.05);
        cw.reverse();
        let out = offset_closed_polyline(&cw, -0.01).unwrap();
        let a = signed_area2(&out).abs() / 2.0;
        assert!((a - 0.08 * 0.08).abs() < 1e-9, "clockwise input shrank differently: {a}");
    }

    #[test]
    fn over_shrinking_is_refused_not_inverted() {
        // `sq(0.05)` spans -0.05..0.05, so half the narrowest span is
        // 0.05 and anything past that has no answer.
        //
        // The symmetric case is the one that matters: every edge crosses
        // to the far side together, so the result stays counter-clockwise
        // with a positive area and looks, to any area-based check, like a
        // perfectly good 20 mm square. It is not: it is the profile
        // turned inside out.
        let e = offset_closed_polyline(&sq(0.05), -0.06).unwrap_err();
        let msg = e.to_string();
        assert!(
            msg.contains("consumes") || msg.contains("collapses") || msg.contains("inverts"),
            "{msg}"
        );
    }

    #[test]
    fn shrinking_to_exactly_the_limit_is_refused() {
        // At d = -0.05 every edge has zero length left. The polygon has
        // degenerated to a point, and a point is not a profile.
        assert!(offset_closed_polyline(&sq(0.05), -0.05).is_err());
    }

    #[test]
    fn over_shrinking_a_rectangle_is_refused_on_the_short_axis() {
        // Asymmetric, so the short edges reverse while the long ones
        // still have length: the failure has to be found per edge, not
        // from the shape as a whole.
        let r = vec![[-0.05, -0.01], [0.05, -0.01], [0.05, 0.01], [-0.05, 0.01]];
        assert!(offset_closed_polyline(&r, -0.02).is_err());
        // Just inside the limit still works, so the check is not simply
        // refusing everything.
        let ok = offset_closed_polyline(&r, -0.005).unwrap();
        let a = signed_area2(&ok).abs() / 2.0;
        assert!((a - 0.09 * 0.01).abs() < 1e-9, "area {a}");
    }

    #[test]
    fn degenerate_profiles_are_refused() {
        assert!(offset_closed_polyline(&[[0.0, 0.0], [1.0, 0.0]], 0.1).is_err());
        // Collinear -> encloses no area.
        let collinear = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
        assert!(offset_closed_polyline(&collinear, 0.1).is_err());
    }

    #[test]
    fn zero_offset_is_identity() {
        let s = sq(0.05);
        let out = offset_closed_polyline(&s, 0.0).unwrap();
        assert_eq!(out, s);
    }

    #[test]
    fn l_shape_concave_corner_offsets() {
        // An L: the concave corner is where a naive offset goes wrong.
        let l = vec![
            [0.0, 0.0], [0.10, 0.0], [0.10, 0.04],
            [0.04, 0.04], [0.04, 0.10], [0.0, 0.10],
        ];
        let out = offset_closed_polyline(&l, -0.005).unwrap();
        assert_eq!(out.len(), 6);
        let a = signed_area2(&out).abs() / 2.0;
        let orig = signed_area2(&l).abs() / 2.0;
        assert!(a < orig, "inward offset should shrink: {a} vs {orig}");
        assert!(a > 0.0);
    }

    #[test]
    fn circle_profile_offsets_radially() {
        let sk = Sketch {
            plane: "xy".into(),
            entities: vec![SketchEntity::Circle { center: [0.0, 0.0], radius: 0.01 }],
            dimensions: vec![],
            constraints: vec![],
        };
        let out = offset_sketch_entities(&sk, -0.002).unwrap();
        match out[0] {
            SketchEntity::Circle { radius, .. } => assert!((radius - 0.008).abs() < 1e-12),
            _ => panic!("expected a circle"),
        }
    }

    #[test]
    fn line_loop_profile_round_trips() {
        let s = sq(0.05);
        let sk = Sketch {
            plane: "xy".into(),
            entities: (0..4)
                .map(|i| SketchEntity::Line { p1: s[i], p2: s[(i + 1) % 4] })
                .collect(),
            dimensions: vec![],
            constraints: vec![],
        };
        let out = offset_sketch_entities(&sk, -0.01).unwrap();
        assert_eq!(out.len(), 4);
    }
}
