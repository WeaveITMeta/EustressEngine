//! Solid builders. Each returns the solid and a name for every one of
//! its faces, in `face_iter()` order, so nothing downstream ever has to
//! guess which face is which.
//!
//! The face order these rely on is truck's own, read from its source:
//!
//! - `tsweep` of a face: `[base face inverted, one side per boundary
//!   edge in boundary order, translated base]`.
//! - partial `rsweep`: the same, with the sides repeated once per
//!   division step (2 steps below 180 degrees, 3 above).
//! - full `rsweep`: no caps; one shell per boundary loop, 3 division
//!   steps, each step one face per edge of that loop.
//!
//! Every builder checks its face count against its name count and
//! refuses rather than hand back misnamed faces if a future truck
//! changes that order.

use std::f64::consts::{PI, TAU};

use truck_modeling::*;

use crate::error::{CadError, CadResult};
use crate::frame::Frame;
use crate::profile::{loop_wire, region_face, Loop, Region, Seg, SegGeom, SegTag};

fn kernel(msg: String) -> CadError {
    CadError::Kernel(msg)
}

fn region_suffix(i: usize) -> String {
    if i == 0 {
        String::new()
    } else {
        format!(".r{}", i + 1)
    }
}

fn check_count(solid: &Solid, names: &[String], what: &str) -> CadResult<()> {
    let n = solid.face_iter().count();
    if n == names.len() {
        Ok(())
    } else {
        Err(kernel(format!(
            "internal: {what} produced {n} faces but {} were named; the kernel's face order is \
             not what this build expects",
            names.len()
        )))
    }
}

/// Extrude `region` from `start` to `start + length` along `frame.z`.
///
/// `reversed` swaps which cap is called `cap_start`: the start cap is
/// always the one on the sketch-plane side, and an extrusion that ran
/// against the normal has it at the upper end.
pub(crate) fn prism(
    region: &Region,
    frame: &Frame,
    start: f64,
    length: f64,
    feature: &str,
    region_index: usize,
    reversed: bool,
) -> CadResult<(Solid, Vec<String>)> {
    if !(length > 0.0) {
        return Err(CadError::EvalFailed {
            feature: feature.to_string(),
            reason: "zero-length extrusion produces no solid".into(),
        });
    }
    let rf = region_face(region, &frame.offset(start))?;
    let solid = builder::tsweep(&rf.face, frame.z * length);
    let suffix = region_suffix(region_index);
    let (lo, hi) = if reversed { ("cap_end", "cap_start") } else { ("cap_start", "cap_end") };
    let mut names = Vec::with_capacity(rf.tags.len() + 2);
    names.push(format!("{feature}.{lo}{suffix}"));
    names.extend(rf.tags.iter().map(|t| t.face_name(feature)));
    names.push(format!("{feature}.{hi}{suffix}"));
    check_count(&solid, &names, feature)?;
    Ok((solid, names))
}

/// A full circle as two tagged half-arcs, counter-clockwise.
pub(crate) fn circle_loop(c: [f64; 2], r: f64, tag0: SegTag, tag1: SegTag) -> Loop {
    let right = [c[0] + r, c[1]];
    let left = [c[0] - r, c[1]];
    Loop {
        segs: vec![
            Seg { geom: SegGeom::Arc { c, r, a: right, b: left, ccw: true }, tag: tag0 },
            Seg { geom: SegGeom::Arc { c, r, a: left, b: right, ccw: true }, tag: tag1 },
        ],
    }
}

/// When every segment of `lp` is an arc of one circle, its centre and
/// radius.
fn circle_of(lp: &Loop) -> Option<([f64; 2], f64)> {
    let mut first: Option<([f64; 2], f64)> = None;
    let mut turn = 0.0;
    for s in &lp.segs {
        let SegGeom::Arc { c, r, .. } = s.geom else { return None };
        match first {
            None => first = Some((c, r)),
            Some((c0, r0)) => {
                let tol = 1.0e-9 * (1.0 + r0);
                if (c[0] - c0[0]).abs() > tol || (c[1] - c0[1]).abs() > tol || (r - r0).abs() > tol {
                    return None;
                }
            }
        }
        turn += s.geom.sweep().abs();
    }
    let got = first?;
    ((turn - TAU).abs() < 1.0e-6).then_some(got)
}

/// Offset a loop perpendicular to itself: positive `d` grows the
/// enclosed area, negative shrinks it, whichever way it winds. Segment
/// order and tags are kept, so an offset loop lofts cleanly against its
/// source and its faces keep their names.
pub(crate) fn offset_loop(lp: &Loop, d: f64) -> CadResult<Loop> {
    if lp.segs.iter().all(|s| matches!(s.geom, SegGeom::Line { .. })) {
        let pts: Vec<[f64; 2]> = lp.segs.iter().map(|s| s.geom.start()).collect();
        let out = crate::offset::offset_closed_polyline(&pts, d)?;
        if out.len() != pts.len() {
            return Err(kernel("internal: polyline offset changed the vertex count".into()));
        }
        let n = out.len();
        return Ok(Loop {
            segs: (0..n)
                .map(|i| Seg {
                    geom: SegGeom::Line { a: out[i], b: out[(i + 1) % n] },
                    tag: lp.segs[i].tag.clone(),
                })
                .collect(),
        });
    }
    if let Some((c, r)) = circle_of(lp) {
        let r2 = r + d;
        if !(r2 > 1.0e-9) {
            return Err(CadError::EvalFailed {
                feature: "Offset".into(),
                reason: format!("an offset of {d:.6} m collapses a circle of radius {r:.6} m"),
            });
        }
        let k = r2 / r;
        let scale = |p: [f64; 2]| [c[0] + (p[0] - c[0]) * k, c[1] + (p[1] - c[1]) * k];
        return Ok(Loop {
            segs: lp
                .segs
                .iter()
                .map(|s| match s.geom {
                    SegGeom::Arc { a, b, ccw, .. } => Seg {
                        geom: SegGeom::Arc { c, r: r2, a: scale(a), b: scale(b), ccw },
                        tag: s.tag.clone(),
                    },
                    SegGeom::Line { .. } => s.clone(),
                })
                .collect(),
        });
    }
    Err(CadError::NotImplemented(
        "offsetting a profile loop that mixes lines and arcs is not supported yet; draft, thin \
         and shell need either straight sides or a full circle"
            .into(),
    ))
}

/// Offset a region's material by `d`: the outer boundary moves by `d`
/// and every hole by `-d`, so a negative `d` thins the material from
/// every side at once.
pub(crate) fn offset_region(region: &Region, d: f64) -> CadResult<Region> {
    Ok(Region {
        outer: offset_loop(&region.outer, d)?.oriented(true),
        holes: region
            .holes
            .iter()
            .map(|h| offset_loop(h, -d).map(|l| l.oriented(false)))
            .collect::<CadResult<Vec<_>>>()?,
    })
}

/// The regions of a thin-walled version of `regions`: a band `t` thick
/// inside every outer loop and around every hole.
pub(crate) fn thin_regions(regions: &[Region], t: f64, feature: &str) -> CadResult<Vec<Region>> {
    let rename = |lp: Loop| Loop {
        segs: lp
            .segs
            .into_iter()
            .map(|s| Seg { tag: SegTag::Face(format!("{}.inner", s.tag.face_name(feature))), geom: s.geom })
            .collect(),
    };
    let mut out = Vec::new();
    for r in regions {
        let inner = rename(offset_loop(&r.outer, -t)?).oriented(false);
        out.push(Region { outer: r.outer.clone(), holes: vec![inner] });
        for h in &r.holes {
            let grown = rename(offset_loop(h, t)?).oriented(true);
            out.push(Region { outer: grown, holes: vec![h.clone()] });
        }
    }
    Ok(out)
}

/// Ruled loft through `sections`, each a region placed by a frame.
///
/// Every section needs the same structure: as many holes, and loop for
/// loop as many segments. Each section's loops must already wind
/// counter-clockwise (outer) and clockwise (holes) about the direction
/// the loft travels. Side faces are named from the FIRST section's
/// segment tags, with `.s{k}` added for each span after the first.
pub(crate) fn loft(sections: &[(Region, Frame)], feature: &str) -> CadResult<(Solid, Vec<String>)> {
    if sections.len() < 2 {
        return Err(CadError::EvalFailed {
            feature: feature.to_string(),
            reason: "a loft needs at least two sections".into(),
        });
    }
    let loops_of = |r: &Region| -> Vec<Loop> { std::iter::once(r.outer.clone()).chain(r.holes.iter().cloned()).collect() };
    let first = loops_of(&sections[0].0);
    let wires: Vec<Vec<Wire>> = sections
        .iter()
        .map(|(r, f)| loops_of(r).iter().map(|lp| loop_wire(lp, f).0).collect())
        .collect();
    for (k, w) in wires.iter().enumerate().skip(1) {
        let same = w.len() == wires[0].len() && w.iter().zip(wires[0].iter()).all(|(a, b)| a.len() == b.len());
        if !same {
            return Err(CadError::EvalFailed {
                feature: feature.to_string(),
                reason: format!(
                    "section {} does not match section 1: every section needs the same number of \
                     holes and, loop for loop, the same number of edges",
                    k + 1
                ),
            });
        }
    }

    let mut faces: Vec<Face> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let start = builder::try_attach_plane(&wires[0])
        .map_err(|e| kernel(format!("loft start cap: {e}")))?
        .inverse();
    faces.push(start);
    names.push(format!("{feature}.cap_start"));
    for k in 0..wires.len() - 1 {
        for (j, (w0, w1)) in wires[k].iter().zip(wires[k + 1].iter()).enumerate() {
            let shell = builder::try_wire_homotopy(w0, w1).map_err(|e| kernel(format!("loft side: {e}")))?;
            for (face, seg) in shell.face_into_iter().zip(first[j].segs.iter()) {
                let base = seg.tag.face_name(feature);
                names.push(if k == 0 { base } else { format!("{base}.s{}", k + 1) });
                faces.push(face);
            }
        }
    }
    let end = builder::try_attach_plane(&wires[wires.len() - 1]).map_err(|e| kernel(format!("loft end cap: {e}")))?;
    faces.push(end);
    names.push(format!("{feature}.cap_end"));
    let solid = Solid::try_new(vec![faces.into()]).map_err(|e| kernel(format!(
        "the loft's faces do not close into a solid ({e}); check that the sections are ordered along \
         the loft and wind the same way"
    )))?;
    check_count(&solid, &names, feature)?;
    Ok((solid, names))
}

/// Make user loft sections compatible: wind each outline the same way
/// about the direction the loft travels, give every section the same
/// number of edges (splitting the longest ones), and rotate each outline
/// so its first vertex sits nearest the previous section's, which is
/// what keeps a loft from twisting.
pub(crate) fn prepare_loft_sections(sections: &mut [(Region, Frame)]) -> std::result::Result<(), String> {
    let n = sections.len();
    if n < 2 {
        return Err("a loft needs at least two sections".into());
    }
    let centroid = |r: &Region, f: &Frame| {
        let poly = r.outer.polygon();
        let k = poly.len().max(1) as f64;
        let (sx, sy) = poly.iter().fold((0.0, 0.0), |(a, b), p| (a + p[0], b + p[1]));
        f.to_world([sx / k, sy / k])
    };
    let centers: Vec<Point3> = sections.iter().map(|(r, f)| centroid(r, f)).collect();
    for k in 0..n {
        let d = if k + 1 < n { centers[k + 1] - centers[k] } else { centers[k] - centers[k - 1] };
        if d.magnitude() < 1.0e-12 {
            return Err(format!("sections {} and {} are at the same place", k.min(n - 2) + 1, k.min(n - 2) + 2));
        }
        if sections[k].1.z.dot(d) < 0.0 {
            let r = &mut sections[k].0;
            r.outer = r.outer.reversed();
        }
    }
    let target = sections.iter().map(|(r, _)| r.outer.segs.len()).max().unwrap_or(0);
    for (r, _) in sections.iter_mut() {
        while r.outer.segs.len() < target {
            let (i, _) = r
                .outer
                .segs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.geom.length().total_cmp(&b.1.geom.length()))
                .ok_or("empty section")?;
            let s = r.outer.segs[i].clone();
            let m = s.geom.midpoint();
            let (g0, g1) = match s.geom {
                SegGeom::Line { a, b } => (SegGeom::Line { a, b: m }, SegGeom::Line { a: m, b }),
                SegGeom::Arc { c, r: rad, a, b, ccw } => {
                    (SegGeom::Arc { c, r: rad, a, b: m, ccw }, SegGeom::Arc { c, r: rad, a: m, b, ccw })
                }
            };
            r.outer.segs[i] = Seg { geom: g0, tag: s.tag.clone() };
            r.outer.segs.insert(i + 1, Seg { geom: g1, tag: s.tag });
        }
    }
    for k in 1..n {
        let prev: Vec<Point3> = {
            let (r, f) = &sections[k - 1];
            r.outer.segs.iter().map(|s| f.to_world(s.geom.start())).collect()
        };
        let (r, f) = &mut sections[k];
        let cur: Vec<Point3> = r.outer.segs.iter().map(|s| f.to_world(s.geom.start())).collect();
        let m = cur.len();
        let best = (0..m)
            .min_by(|&x, &y| {
                let cost = |sh: usize| (0..m).map(|i| (cur[(i + sh) % m] - prev[i]).magnitude2()).sum::<f64>();
                cost(x).total_cmp(&cost(y))
            })
            .unwrap_or(0);
        r.outer.segs.rotate_left(best);
    }
    Ok(())
}

/// Revolve a region about an axis.
///
/// `angle` at or past a full turn becomes an exact full revolution:
/// a value a hair under 2π would otherwise produce a partial revolve
/// whose two caps coincide.
#[allow(clippy::too_many_arguments)]
pub(crate) fn revolve(
    region: &Region,
    frame: &Frame,
    origin: Point3,
    axis: Vector3,
    angle: f64,
    both_sides: bool,
    feature: &str,
    region_index: usize,
) -> CadResult<(Solid, Vec<String>)> {
    let len = axis.magnitude();
    if !(len > 1.0e-12) {
        return Err(CadError::EvalFailed { feature: feature.to_string(), reason: "revolve axis has no direction".into() });
    }
    let axis = axis / len;
    let full = angle.abs() >= TAU - 1.0e-9;
    let angle = if full { TAU.copysign(angle) } else { angle };
    if angle.abs() < 1.0e-9 {
        return Err(CadError::EvalFailed { feature: feature.to_string(), reason: "revolve angle is zero".into() });
    }
    let rf = region_face(region, frame)?;
    let face = if both_sides && !full {
        builder::rotated(&rf.face, origin, axis, Rad(-angle * 0.5))
    } else {
        rf.face
    };
    let solid = builder::rsweep(&face, origin, axis, Rad(angle));

    let suffix = region_suffix(region_index);
    let step_name = |tag: &SegTag, s: usize| {
        let base = tag.face_name(feature);
        if s == 0 {
            base
        } else {
            format!("{base}.{s}")
        }
    };
    let mut names = Vec::new();
    if full {
        let mut at = 0usize;
        for lp in std::iter::once(&region.outer).chain(region.holes.iter()) {
            let tags = &rf.tags[at..at + lp.segs.len()];
            at += lp.segs.len();
            for s in 0..3 {
                names.extend(tags.iter().map(|t| step_name(t, s)));
            }
        }
    } else {
        let steps = if angle.abs() < PI { 2 } else { 3 };
        names.push(format!("{feature}.cap_start{suffix}"));
        for s in 0..steps {
            names.extend(rf.tags.iter().map(|t| step_name(t, s)));
        }
        names.push(format!("{feature}.cap_end{suffix}"));
    }
    if solid.face_iter().count() != names.len() {
        // Fall back to positional names rather than fail a revolve that
        // built correctly; the geometry is right, only the naming guess
        // was not.
        names = (0..solid.face_iter().count()).map(|i| format!("{feature}.face{i}{suffix}")).collect();
    }
    Ok((solid, names))
}

/// A slab filling the half-space BEHIND `frame`'s plane (the side the
/// normal points away from), `extent` wide in every in-plane direction.
/// The face lying on the plane is named `{feature}.cut`.
pub(crate) fn halfspace_slab(frame: &Frame, extent: f64, feature: &str) -> CadResult<(Solid, Vec<String>)> {
    let e = extent;
    let t = |s| SegTag::Face(format!("{feature}.slab.{s}"));
    let lp = Loop {
        segs: vec![
            Seg { geom: SegGeom::Line { a: [-e, -e], b: [e, -e] }, tag: t("0") },
            Seg { geom: SegGeom::Line { a: [e, -e], b: [e, e] }, tag: t("1") },
            Seg { geom: SegGeom::Line { a: [e, e], b: [-e, e] }, tag: t("2") },
            Seg { geom: SegGeom::Line { a: [-e, e], b: [-e, -e] }, tag: t("3") },
        ],
    };
    let region = Region { outer: lp, holes: Vec::new() };
    let (solid, mut names) = prism(&region, frame, -2.0 * e, 2.0 * e, feature, 0, false)?;
    let last = names.len() - 1;
    names[0] = format!("{feature}.slab.far");
    names[last] = format!("{feature}.cut");
    Ok((solid, names))
}

/// A cylinder of radius `r` on `frame`'s z axis, from `start` to
/// `start + length`, with faces named `{prefix}.wall.0/1`,
/// `{prefix}.{near}` at the start and `{prefix}.{far}` at the end.
pub(crate) fn cylinder(
    frame: &Frame,
    r: f64,
    start: f64,
    length: f64,
    prefix: &str,
    near: &str,
    far: &str,
) -> CadResult<(Solid, Vec<String>)> {
    let lp = circle_loop(
        [0.0, 0.0],
        r,
        SegTag::Face(format!("{prefix}.wall.0")),
        SegTag::Face(format!("{prefix}.wall.1")),
    );
    let region = Region { outer: lp, holes: Vec::new() };
    let (solid, mut names) = prism(&region, frame, start, length, prefix, 0, false)?;
    let last = names.len() - 1;
    names[0] = format!("{prefix}.{near}");
    names[last] = format!("{prefix}.{far}");
    Ok((solid, names))
}

/// A solid cone on `frame`'s z axis: radius `r_top` at `top` (a
/// position along z), narrowing to a point at `apex`. Used for true
/// countersinks, which the hole feature previously cut as a straight
/// counterbore at a hard-coded depth.
pub(crate) fn cone(frame: &Frame, r_top: f64, top: f64, apex: f64, prefix: &str) -> CadResult<(Solid, Vec<String>)> {
    let a = frame.point(0.0, 0.0, top);
    let rim = frame.point(r_top, 0.0, top);
    let b = frame.point(0.0, 0.0, apex);
    let va = builder::vertex(a);
    let vr = builder::vertex(rim);
    let vb = builder::vertex(b);
    let wire: Wire = vec![builder::line(&va, &vr), builder::line(&vr, &vb)].into();
    // truck revolves about the line through the wire's first vertex;
    // its own closed-cone example runs the wire DOWN the axis while the
    // axis points UP, so the axis here points from apex back to top.
    let axis = (a - b).normalize();
    let shell = builder::cone(&wire, axis, Rad(TAU));
    let solid = Solid::try_new(vec![shell]).map_err(|e| kernel(format!("countersink cone: {e}")))?;
    let solid = ensure_outward(solid);
    let names = (0..solid.face_iter().count()).map(|i| format!("{prefix}.countersink.{i}")).collect();
    Ok((solid, names))
}

/// Flip a solid that came out inside-out.
///
/// Decided by the sign of the enclosed volume of a coarse tessellation,
/// which is cheap for the small construction bodies this is used on and
/// independent of how the builder happened to wind its faces.
pub(crate) fn ensure_outward(mut solid: Solid) -> Solid {
    let mesh = crate::eval::tessellate_solid(&solid, 0.02 * crate::eval::solid_size(&solid).max(1.0e-9));
    if crate::measure::mass_properties(&mesh).signed_volume < 0.0 {
        solid.not();
    }
    solid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f64) -> Region {
        let t = |i: usize| SegTag::Entity { index: i, sub: None };
        Region {
            outer: Loop {
                segs: vec![
                    Seg { geom: SegGeom::Line { a: [-s, -s], b: [s, -s] }, tag: t(0) },
                    Seg { geom: SegGeom::Line { a: [s, -s], b: [s, s] }, tag: t(1) },
                    Seg { geom: SegGeom::Line { a: [s, s], b: [-s, s] }, tag: t(2) },
                    Seg { geom: SegGeom::Line { a: [-s, s], b: [-s, -s] }, tag: t(3) },
                ],
            },
            holes: Vec::new(),
        }
    }

    #[test]
    fn prism_names_every_face() {
        let (solid, names) = prism(&square(0.5), &Frame::xy(), 0.0, 1.0, "E", 0, false).unwrap();
        assert_eq!(solid.face_iter().count(), 6);
        assert_eq!(names[0], "E.cap_start");
        assert_eq!(names[5], "E.cap_end");
        assert_eq!(names[1], "E.side.e0");
    }

    #[test]
    fn offset_region_shrinks_outer_and_grows_holes() {
        let mut r = square(1.0);
        r.holes.push(circle_loop([0.0, 0.0], 0.25, SegTag::Face("h0".into()), SegTag::Face("h1".into())).oriented(false));
        let o = offset_region(&r, -0.1).unwrap();
        let expect = 1.8 * 1.8 - PI * 0.35 * 0.35;
        assert!((o.area() - expect).abs() < 1e-9, "{}", o.area());
    }

    #[test]
    fn a_two_section_loft_between_squares_is_a_frustum() {
        let a = (square(0.5), Frame::xy());
        let b = (offset_region(&square(0.5), -0.25).unwrap(), Frame::xy().offset(1.0));
        let (solid, names) = loft(&[a, b], "L").unwrap();
        assert_eq!(names.len(), 6);
        assert_eq!(solid.face_iter().count(), 6);
    }
}
