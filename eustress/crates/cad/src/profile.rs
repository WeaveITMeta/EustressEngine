//! Sketch profiles: closed loops, their nesting, and the planar faces
//! they bound.
//!
//! A profile is what an extrude, a revolve or a loft actually consumes:
//! the closed regions a sketch encloses. The builder this replaces took
//! the FIRST rectangle or circle it found and discarded every other
//! entity, and chained lines only; a plate outline with a cutout drawn
//! inside it extruded as a solid slab, an arc in an outline was dropped,
//! and two separate outlines extruded as one. Nothing reported any of it.
//!
//! Here every non-construction line, arc, circle and rectangle takes
//! part. Segments are welded end to end into loops, loops are nested by
//! containment, and the even-odd rule turns the nesting into regions: an
//! outer boundary plus the holes directly inside it. An island inside a
//! hole is a region of its own, exactly as in mainstream CAD.
//!
//! Every boundary segment remembers the sketch entity that drew it, so
//! the side face an extrude sweeps from it can be named after that
//! entity and found again after an upstream edit (see `topology`).

use std::f64::consts::{PI, TAU};

use truck_modeling::*;

use crate::error::{CadError, CadResult};
use crate::frame::Frame;
use crate::sketch::{Sketch, SketchEntity};

/// Where a boundary segment came from, which is what its face is named
/// after.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SegTag {
    /// Drawn by sketch entity `index`; `sub` distinguishes the pieces of
    /// an entity that produces several (a rectangle's four sides, a
    /// circle's two halves).
    Entity { index: usize, sub: Option<&'static str> },
    /// Created by a later operation (a profile blend) and named in full.
    Face(String),
}

impl SegTag {
    /// The face name a sweep of this segment gets under `feature`.
    pub fn face_name(&self, feature: &str) -> String {
        match self {
            SegTag::Entity { index, sub: Some(s) } => format!("{feature}.side.e{index}.{s}"),
            SegTag::Entity { index, sub: None } => format!("{feature}.side.e{index}"),
            SegTag::Face(name) => name.clone(),
        }
    }
}

/// Geometry of one boundary segment, in sketch coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegGeom {
    Line { a: [f64; 2], b: [f64; 2] },
    /// Circular arc from `a` to `b` about `c`, counter-clockwise when
    /// `ccw`. Always strictly less than a full turn.
    Arc { c: [f64; 2], r: f64, a: [f64; 2], b: [f64; 2], ccw: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    pub geom: SegGeom,
    pub tag: SegTag,
}

#[inline]
fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
#[inline]
fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = sub2(a, b);
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}
#[inline]
fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

impl SegGeom {
    pub fn start(&self) -> [f64; 2] {
        match *self {
            SegGeom::Line { a, .. } | SegGeom::Arc { a, .. } => a,
        }
    }
    pub fn end(&self) -> [f64; 2] {
        match *self {
            SegGeom::Line { b, .. } | SegGeom::Arc { b, .. } => b,
        }
    }
    pub fn reversed(&self) -> Self {
        match *self {
            SegGeom::Line { a, b } => SegGeom::Line { a: b, b: a },
            SegGeom::Arc { c, r, a, b, ccw } => SegGeom::Arc { c, r, a: b, b: a, ccw: !ccw },
        }
    }

    /// Signed sweep of an arc in radians, positive counter-clockwise.
    /// Zero for a line.
    pub fn sweep(&self) -> f64 {
        match *self {
            SegGeom::Line { .. } => 0.0,
            SegGeom::Arc { c, a, b, ccw, .. } => {
                let t0 = (a[1] - c[1]).atan2(a[0] - c[0]);
                let t1 = (b[1] - c[1]).atan2(b[0] - c[0]);
                let mut s = t1 - t0;
                if ccw {
                    while s <= 1.0e-12 {
                        s += TAU;
                    }
                    while s > TAU {
                        s -= TAU;
                    }
                } else {
                    while s >= -1.0e-12 {
                        s -= TAU;
                    }
                    while s < -TAU {
                        s += TAU;
                    }
                }
                s
            }
        }
    }

    /// Point at fraction `t` in [0, 1] along the segment.
    pub fn at(&self, t: f64) -> [f64; 2] {
        match *self {
            SegGeom::Line { a, b } => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            SegGeom::Arc { c, r, a, .. } => {
                let t0 = (a[1] - c[1]).atan2(a[0] - c[0]);
                let th = t0 + self.sweep() * t;
                [c[0] + r * th.cos(), c[1] + r * th.sin()]
            }
        }
    }

    pub fn midpoint(&self) -> [f64; 2] {
        self.at(0.5)
    }

    pub fn length(&self) -> f64 {
        match *self {
            SegGeom::Line { a, b } => dist2(a, b),
            SegGeom::Arc { r, .. } => r * self.sweep().abs(),
        }
    }

    /// Twice the signed area contribution `∫ x dy - y dx` of this
    /// segment. Summed over a closed loop it is twice the enclosed area,
    /// exactly, arcs included.
    pub fn area2(&self) -> f64 {
        match *self {
            SegGeom::Line { a, b } => cross2(a, b),
            SegGeom::Arc { c, r, a, .. } => {
                let t0 = (a[1] - c[1]).atan2(a[0] - c[0]);
                let s = self.sweep();
                let t1 = t0 + s;
                r * (c[0] * (t1.sin() - t0.sin()) - c[1] * (t1.cos() - t0.cos())) + r * r * s
            }
        }
    }

    /// Polyline approximation without the end point, for containment
    /// tests. Arcs get a point every 10 degrees.
    fn polyline(&self) -> Vec<[f64; 2]> {
        match self {
            SegGeom::Line { a, .. } => vec![*a],
            SegGeom::Arc { .. } => {
                let n = ((self.sweep().abs() / (PI / 18.0)).ceil() as usize).max(2);
                (0..n).map(|i| self.at(i as f64 / n as f64)).collect()
            }
        }
    }
}

/// A closed chain of segments, each starting where the previous ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub segs: Vec<Seg>,
}

impl Loop {
    /// Enclosed area, positive when counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        0.5 * self.segs.iter().map(|s| s.geom.area2()).sum::<f64>()
    }

    pub fn reversed(&self) -> Loop {
        Loop {
            segs: self
                .segs
                .iter()
                .rev()
                .map(|s| Seg { geom: s.geom.reversed(), tag: s.tag.clone() })
                .collect(),
        }
    }

    /// The same loop wound counter-clockwise (`ccw`) or clockwise.
    pub fn oriented(self, ccw: bool) -> Loop {
        if (self.signed_area() > 0.0) == ccw {
            self
        } else {
            self.reversed()
        }
    }

    pub fn polygon(&self) -> Vec<[f64; 2]> {
        self.segs.iter().flat_map(|s| s.geom.polyline()).collect()
    }

    /// Even-odd point-in-polygon against the polyline approximation.
    pub fn contains(&self, p: [f64; 2]) -> bool {
        let poly = self.polygon();
        let n = poly.len();
        let mut inside = false;
        let mut j = n.wrapping_sub(1);
        for i in 0..n {
            let (a, b) = (poly[i], poly[j]);
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if p[0] < x {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }

    /// Bounding extent, for scale-aware tolerances.
    pub fn extent(&self) -> f64 {
        let poly = self.polygon();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &poly {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        dist2(lo, hi)
    }

    /// Replace the corner at `vertex` (the end of segment `vertex - 1`
    /// and start of segment `vertex`) with a tangent arc of radius `r`
    /// (`fillet`) or a straight cut `r` back along each side
    /// (`!fillet`). Both sides must be lines.
    ///
    /// This is exact: a profile blend is plain 2D geometry, so a prism
    /// rebuilt from the blended profile has a true cylindrical fillet on
    /// that edge, with no boolean involved. The new segment is tagged
    /// with `name`, which becomes the name of the face it sweeps into.
    pub fn blend_corner(&mut self, vertex: usize, r: f64, fillet: bool, name: String) -> CadResult<()> {
        let n = self.segs.len();
        if n < 3 {
            return Err(blend_err("a profile needs at least three sides to blend a corner"));
        }
        let vi = vertex % n;
        let pi = (vi + n - 1) % n;
        let (SegGeom::Line { a: p_start, b: corner }, SegGeom::Line { a: corner2, b: n_end }) =
            (self.segs[pi].geom, self.segs[vi].geom)
        else {
            return Err(blend_err(
                "only a corner between two straight sides can be blended from the profile; \
                 this corner touches an arc",
            ));
        };
        if dist2(corner, corner2) > 1.0e-9 * (1.0 + self.extent()) {
            return Err(blend_err("internal: loop is not continuous at the blended corner"));
        }
        let u = unit(sub2(p_start, corner));
        let w = unit(sub2(n_end, corner));
        let cos_phi = (u[0] * w[0] + u[1] * w[1]).clamp(-1.0, 1.0);
        let phi = cos_phi.acos();
        if !(phi > 1.0e-6 && phi < PI - 1.0e-6) {
            return Err(blend_err("the corner is straight, so there is nothing to blend"));
        }
        // Distance back along each side to the tangent point.
        let t = if fillet { r / (phi * 0.5).tan() } else { r };
        let len_prev = dist2(p_start, corner);
        let len_next = dist2(n_end, corner);
        if t >= len_prev - 1.0e-12 || t >= len_next - 1.0e-12 {
            return Err(blend_err(&format!(
                "a blend of {r:.6} m needs {t:.6} m of straight side on each side of the corner, \
                 but the sides are {len_prev:.6} m and {len_next:.6} m"
            )));
        }
        let ta = [corner[0] + u[0] * t, corner[1] + u[1] * t];
        let tb = [corner[0] + w[0] * t, corner[1] + w[1] * t];
        let blend = if fillet {
            // Centre along the bisector, r / sin(phi / 2) from the corner.
            let bis = unit([u[0] + w[0], u[1] + w[1]]);
            let dc = r / (phi * 0.5).sin();
            let c = [corner[0] + bis[0] * dc, corner[1] + bis[1] * dc];
            // Travel ta -> tb turns the same way the corner did.
            let ccw = cross2(sub2(ta, c), sub2(tb, c)) > 0.0;
            SegGeom::Arc { c, r, a: ta, b: tb, ccw }
        } else {
            SegGeom::Line { a: ta, b: tb }
        };
        self.segs[pi].geom = SegGeom::Line { a: p_start, b: ta };
        self.segs[vi].geom = SegGeom::Line { a: tb, b: n_end };
        self.segs.insert(vi, Seg { geom: blend, tag: SegTag::Face(name) });
        Ok(())
    }
}

fn unit(v: [f64; 2]) -> [f64; 2] {
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if l > 0.0 {
        [v[0] / l, v[1] / l]
    } else {
        [0.0, 0.0]
    }
}

fn blend_err(reason: &str) -> CadError {
    CadError::EvalFailed { feature: "Blend".into(), reason: reason.into() }
}

/// An outer boundary and the holes directly inside it. The outer loop is
/// counter-clockwise and every hole clockwise, in sketch coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

impl Region {
    pub fn area(&self) -> f64 {
        self.outer.signed_area() + self.holes.iter().map(Loop::signed_area).sum::<f64>()
    }

    /// Every boundary segment in face-boundary order: the outer loop,
    /// then each hole. A sweep produces side faces in exactly this order.
    pub fn segments(&self) -> impl Iterator<Item = &Seg> {
        self.outer.segs.iter().chain(self.holes.iter().flat_map(|h| h.segs.iter()))
    }
}

/// Everything a sketch encloses.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub regions: Vec<Region>,
    /// Segments that belonged to no closed loop. They are drawn, but
    /// enclose nothing, so no feature consumes them; the count is
    /// reported so a loop left open by accident is visible.
    pub open_segments: usize,
}

/// Build the profile of a sketch.
pub fn profile_of(sk: &Sketch) -> CadResult<Profile> {
    let mut loops: Vec<Loop> = Vec::new();
    let mut chain: Vec<Seg> = Vec::new();

    for (index, e) in sk.entities.iter().enumerate() {
        match *e {
            SketchEntity::Rectangle { p1, p2 } => {
                let (x0, x1) = (p1[0].min(p2[0]), p1[0].max(p2[0]));
                let (y0, y1) = (p1[1].min(p2[1]), p1[1].max(p2[1]));
                if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
                    continue;
                }
                let tag = |sub| SegTag::Entity { index, sub: Some(sub) };
                loops.push(Loop {
                    segs: vec![
                        Seg { geom: SegGeom::Line { a: [x0, y0], b: [x1, y0] }, tag: tag("bottom") },
                        Seg { geom: SegGeom::Line { a: [x1, y0], b: [x1, y1] }, tag: tag("right") },
                        Seg { geom: SegGeom::Line { a: [x1, y1], b: [x0, y1] }, tag: tag("top") },
                        Seg { geom: SegGeom::Line { a: [x0, y1], b: [x0, y0] }, tag: tag("left") },
                    ],
                });
            }
            SketchEntity::Circle { center, radius } => {
                if radius > 0.0 {
                    loops.push(circle_loop(center, radius, index));
                }
            }
            SketchEntity::Arc { center, start_angle, sweep, radius } => {
                if !(radius > 0.0) || sweep.abs() < 1.0e-12 {
                    continue;
                }
                if sweep.abs() >= TAU - 1.0e-9 {
                    loops.push(circle_loop(center, radius, index));
                    continue;
                }
                let a = [center[0] + radius * start_angle.cos(), center[1] + radius * start_angle.sin()];
                let end = start_angle + sweep;
                let b = [center[0] + radius * end.cos(), center[1] + radius * end.sin()];
                chain.push(Seg {
                    geom: SegGeom::Arc { c: center, r: radius, a, b, ccw: sweep > 0.0 },
                    tag: SegTag::Entity { index, sub: None },
                });
            }
            SketchEntity::Line { p1, p2 } => {
                if dist2(p1, p2) > 0.0 {
                    chain.push(Seg {
                        geom: SegGeom::Line { a: p1, b: p2 },
                        tag: SegTag::Entity { index, sub: None },
                    });
                }
            }
            SketchEntity::Point { .. } | SketchEntity::Construction { .. } => {}
        }
    }

    let (mut traced, open_segments) = trace_loops(chain)?;
    loops.append(&mut traced);

    // Drop loops that enclose nothing (two coincident lines, say).
    let scale = loops.iter().map(Loop::extent).fold(0.0_f64, f64::max).max(1.0e-9);
    loops.retain(|l| l.signed_area().abs() > (scale * 1.0e-9).powi(2));

    if loops.is_empty() {
        let what = if open_segments > 0 {
            format!(
                "{open_segments} segment(s) were drawn but none of them close into a loop: check \
                 that each outline ends exactly where it began"
            )
        } else {
            "the sketch contains no lines, arcs, circles or rectangles".to_string()
        };
        return Err(CadError::EvalFailed {
            feature: "Profile".into(),
            reason: format!("sketch has no closed profile — {what}"),
        });
    }

    Ok(Profile { regions: nest(loops), open_segments })
}

fn circle_loop(c: [f64; 2], r: f64, index: usize) -> Loop {
    let right = [c[0] + r, c[1]];
    let left = [c[0] - r, c[1]];
    Loop {
        segs: vec![
            Seg {
                geom: SegGeom::Arc { c, r, a: right, b: left, ccw: true },
                tag: SegTag::Entity { index, sub: Some("0") },
            },
            Seg {
                geom: SegGeom::Arc { c, r, a: left, b: right, ccw: true },
                tag: SegTag::Entity { index, sub: Some("1") },
            },
        ],
    }
}

/// Weld segment endpoints and walk the resulting graph into loops.
///
/// Each welded point must join exactly two segments. A point where three
/// or more meet is a branch, and which loop it belongs to is ambiguous
/// without splitting the arrangement into faces, so it is refused with
/// its location rather than resolved by guesswork.
fn trace_loops(segs: Vec<Seg>) -> CadResult<(Vec<Loop>, usize)> {
    if segs.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let extent = {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for s in &segs {
            for p in [s.geom.start(), s.geom.end()] {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        dist2(lo, hi)
    };
    let tol = (extent * 1.0e-7).max(1.0e-9);

    // Node for each endpoint.
    let mut nodes: Vec<[f64; 2]> = Vec::new();
    let node_of = |p: [f64; 2], nodes: &mut Vec<[f64; 2]>| -> usize {
        if let Some(i) = nodes.iter().position(|q| dist2(*q, p) <= tol) {
            i
        } else {
            nodes.push(p);
            nodes.len() - 1
        }
    };
    let ends: Vec<(usize, usize)> = segs
        .iter()
        .map(|s| (node_of(s.geom.start(), &mut nodes), node_of(s.geom.end(), &mut nodes)))
        .collect();

    let mut incident: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, &(a, b)) in ends.iter().enumerate() {
        incident[a].push(i);
        incident[b].push(i);
    }
    if let Some((n, inc)) = incident.iter().enumerate().find(|(_, inc)| inc.len() > 2) {
        let p = nodes[n];
        return Err(CadError::EvalFailed {
            feature: "Profile".into(),
            reason: format!(
                "{} segments meet at ({:.6}, {:.6}); a profile must be made of simple closed \
                 loops, so each endpoint may join exactly two segments. Split the outline into \
                 separate loops, or move the extra segment to construction geometry.",
                inc.len(),
                p[0],
                p[1]
            ),
        });
    }

    let mut used = vec![false; segs.len()];
    let mut loops = Vec::new();
    let mut open = 0usize;
    for first in 0..segs.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut chain = vec![segs[first].clone()];
        let start_node = ends[first].0;
        let mut at = ends[first].1;
        let mut closed = false;
        while !closed {
            let next = incident[at].iter().copied().find(|&j| !used[j]);
            let Some(j) = next else { break };
            used[j] = true;
            let (a, b) = ends[j];
            let seg = if a == at {
                at = b;
                segs[j].clone()
            } else {
                at = a;
                Seg { geom: segs[j].geom.reversed(), tag: segs[j].tag.clone() }
            };
            chain.push(seg);
            closed = at == start_node;
        }
        if closed && chain.len() >= 2 {
            // Snap every joint to its welded node so the loop is exactly
            // continuous; the truck wire is closed by vertex identity, and
            // a gap of a nanometre would otherwise leave it open.
            loops.push(snap_loop(Loop { segs: chain }));
        } else {
            open += chain.len();
        }
    }
    Ok((loops, open))
}

/// Make each segment start exactly where the previous one ended.
fn snap_loop(mut l: Loop) -> Loop {
    let n = l.segs.len();
    for i in 0..n {
        let prev_end = l.segs[(i + n - 1) % n].geom.end();
        match &mut l.segs[i].geom {
            SegGeom::Line { a, .. } | SegGeom::Arc { a, .. } => *a = prev_end,
        }
    }
    l
}

/// Even-odd nesting: loops at even depth are outer boundaries, each loop
/// at odd depth is a hole in the innermost even loop that contains it.
fn nest(loops: Vec<Loop>) -> Vec<Region> {
    let n = loops.len();
    // A point on each loop decides containment, since loops never cross.
    let probe: Vec<[f64; 2]> = loops.iter().map(|l| l.segs[0].geom.midpoint()).collect();
    let area: Vec<f64> = loops.iter().map(|l| l.signed_area().abs()).collect();
    let contains = |outer: usize, inner: usize| outer != inner && loops[outer].contains(probe[inner]);
    let depth: Vec<usize> = (0..n).map(|i| (0..n).filter(|&j| contains(j, i)).count()).collect();

    let mut regions: Vec<(usize, Region)> = Vec::new();
    for i in 0..n {
        if depth[i] % 2 == 0 {
            regions.push((
                i,
                Region { outer: loops[i].clone().oriented(true), holes: Vec::new() },
            ));
        }
    }
    for h in 0..n {
        if depth[h] % 2 == 1 {
            // The smallest even-depth loop containing this one, one level up.
            let parent = (0..n)
                .filter(|&o| depth[o] + 1 == depth[h] && contains(o, h))
                .min_by(|&a, &b| area[a].total_cmp(&area[b]));
            if let Some(p) = parent {
                if let Some((_, r)) = regions.iter_mut().find(|(i, _)| *i == p) {
                    r.holes.push(loops[h].clone().oriented(false));
                }
            }
        }
    }
    regions.into_iter().map(|(_, r)| r).collect()
}

/// A planar truck face for `region` placed by `frame`, plus the tag of
/// every boundary edge in the order a sweep turns them into side faces.
pub struct RegionFace {
    pub face: Face,
    pub tags: Vec<SegTag>,
}

pub fn region_face(region: &Region, frame: &Frame) -> CadResult<RegionFace> {
    let mut wires: Vec<Wire> = Vec::with_capacity(1 + region.holes.len());
    let mut tags = Vec::new();
    for lp in std::iter::once(&region.outer).chain(region.holes.iter()) {
        let (wire, mut t) = loop_wire(lp, frame);
        wires.push(wire);
        tags.append(&mut t);
    }
    let face = builder::try_attach_plane(&wires).map_err(|e| CadError::Kernel(format!(
        "could not build a planar face from the profile: {e}"
    )))?;
    // The outer loop is counter-clockwise in the sketch frame, so the
    // face normal must come out along the sketch normal. A sweep along
    // the normal then yields an outward-facing solid.
    if let Surface::Plane(p) = face.surface() {
        if p.normal().dot(frame.z) < 0.0 {
            return Err(CadError::Kernel(
                "internal: profile face normal disagrees with the sketch normal".into(),
            ));
        }
    }
    Ok(RegionFace { face, tags })
}

/// A closed truck wire for a loop. The closing edge reuses the first
/// vertex: truck decides closure by vertex identity, not position.
pub(crate) fn loop_wire(lp: &Loop, frame: &Frame) -> (Wire, Vec<SegTag>) {
    let verts: Vec<Vertex> = lp.segs.iter().map(|s| builder::vertex(frame.to_world(s.geom.start()))).collect();
    let n = verts.len();
    let mut edges: Vec<Edge> = Vec::with_capacity(n);
    for (i, s) in lp.segs.iter().enumerate() {
        let (v0, v1) = (&verts[i], &verts[(i + 1) % n]);
        let edge = match s.geom {
            SegGeom::Line { .. } => builder::line(v0, v1),
            SegGeom::Arc { .. } => builder::circle_arc(v0, v1, frame.to_world(s.geom.midpoint())),
        };
        edges.push(edge);
    }
    (edges.into(), lp.segs.iter().map(|s| s.tag.clone()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::SketchEntity as E;

    fn sketch(entities: Vec<E>) -> Sketch {
        Sketch { entities, ..Default::default() }
    }

    #[test]
    fn a_rectangle_with_a_circle_inside_is_one_region_with_a_hole() {
        let p = profile_of(&sketch(vec![
            E::Rectangle { p1: [-0.05, -0.03], p2: [0.05, 0.03] },
            E::Circle { center: [0.0, 0.0], radius: 0.01 },
        ]))
        .unwrap();
        assert_eq!(p.regions.len(), 1);
        assert_eq!(p.regions[0].holes.len(), 1);
        let expect = 0.1 * 0.06 - PI * 0.01 * 0.01;
        assert!((p.regions[0].area() - expect).abs() < 1e-12, "{}", p.regions[0].area());
    }

    #[test]
    fn two_separate_outlines_are_two_regions() {
        let p = profile_of(&sketch(vec![
            E::Rectangle { p1: [0.0, 0.0], p2: [1.0, 1.0] },
            E::Rectangle { p1: [2.0, 0.0], p2: [3.0, 1.0] },
        ]))
        .unwrap();
        assert_eq!(p.regions.len(), 2);
        assert!(p.regions.iter().all(|r| r.holes.is_empty()));
    }

    #[test]
    fn an_island_inside_a_hole_is_its_own_region() {
        let p = profile_of(&sketch(vec![
            E::Rectangle { p1: [-3.0, -3.0], p2: [3.0, 3.0] },
            E::Rectangle { p1: [-2.0, -2.0], p2: [2.0, 2.0] },
            E::Rectangle { p1: [-1.0, -1.0], p2: [1.0, 1.0] },
        ]))
        .unwrap();
        assert_eq!(p.regions.len(), 2);
        let total: f64 = p.regions.iter().map(Region::area).sum();
        assert!((total - (36.0 - 16.0 + 4.0)).abs() < 1e-9, "{total}");
    }

    #[test]
    fn a_line_and_arc_outline_closes_and_measures_exactly() {
        // A "D": diameter along y, half-disk to the right.
        let p = profile_of(&sketch(vec![
            E::Line { p1: [0.0, 1.0], p2: [0.0, -1.0] },
            E::Arc { center: [0.0, 0.0], start_angle: -PI / 2.0, sweep: PI, radius: 1.0 },
        ]))
        .unwrap();
        assert_eq!(p.regions.len(), 1);
        assert!((p.regions[0].area() - PI / 2.0).abs() < 1e-12, "{}", p.regions[0].area());
    }

    #[test]
    fn clockwise_input_is_reoriented() {
        let p = profile_of(&sketch(vec![
            E::Line { p1: [0.0, 0.0], p2: [0.0, 1.0] },
            E::Line { p1: [0.0, 1.0], p2: [1.0, 1.0] },
            E::Line { p1: [1.0, 1.0], p2: [1.0, 0.0] },
            E::Line { p1: [1.0, 0.0], p2: [0.0, 0.0] },
        ]))
        .unwrap();
        assert!(p.regions[0].outer.signed_area() > 0.0);
    }

    #[test]
    fn an_open_outline_is_refused_by_name() {
        let err = profile_of(&sketch(vec![
            E::Line { p1: [0.0, 0.0], p2: [1.0, 0.0] },
            E::Line { p1: [1.0, 0.0], p2: [1.0, 1.0] },
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("close"), "{err}");
    }

    #[test]
    fn a_branch_is_refused_with_its_location() {
        let err = profile_of(&sketch(vec![
            E::Line { p1: [0.0, 0.0], p2: [1.0, 0.0] },
            E::Line { p1: [0.0, 0.0], p2: [0.0, 1.0] },
            E::Line { p1: [0.0, 0.0], p2: [-1.0, 0.0] },
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("meet at"), "{err}");
    }

    #[test]
    fn construction_geometry_takes_no_part() {
        let p = profile_of(&sketch(vec![
            E::Rectangle { p1: [0.0, 0.0], p2: [1.0, 1.0] },
            E::Construction { p1: [-5.0, 0.5], p2: [5.0, 0.5] },
        ]))
        .unwrap();
        assert_eq!(p.regions.len(), 1);
        assert_eq!(p.open_segments, 0);
    }

    #[test]
    fn a_fillet_blend_removes_the_corner_exactly() {
        let mut p = profile_of(&sketch(vec![E::Rectangle { p1: [0.0, 0.0], p2: [1.0, 1.0] }])).unwrap();
        let lp = &mut p.regions[0].outer;
        // Vertex 1 is the (1, 0) corner: end of "bottom", start of "right".
        lp.blend_corner(1, 0.1, true, "F.face.0".into()).unwrap();
        // A fillet of radius r removes r^2 (1 - pi / 4) from a right angle.
        let expect = 1.0 - 0.01 * (1.0 - PI / 4.0);
        assert!((lp.signed_area() - expect).abs() < 1e-12, "{}", lp.signed_area());
        assert_eq!(lp.segs.len(), 5);
    }

    #[test]
    fn a_chamfer_blend_removes_a_triangle() {
        let mut p = profile_of(&sketch(vec![E::Rectangle { p1: [0.0, 0.0], p2: [1.0, 1.0] }])).unwrap();
        let lp = &mut p.regions[0].outer;
        lp.blend_corner(1, 0.2, false, "C.face.0".into()).unwrap();
        assert!((lp.signed_area() - (1.0 - 0.02)).abs() < 1e-12, "{}", lp.signed_area());
    }

    #[test]
    fn region_faces_face_the_sketch_normal() {
        let p = profile_of(&sketch(vec![
            E::Rectangle { p1: [-1.0, -1.0], p2: [1.0, 1.0] },
            E::Circle { center: [0.0, 0.0], radius: 0.5 },
        ]))
        .unwrap();
        for frame in [Frame::xy(), Frame::xz(), Frame::yz()] {
            let rf = region_face(&p.regions[0], &frame).unwrap();
            assert_eq!(rf.tags.len(), 4 + 2);
            match rf.face.surface() {
                Surface::Plane(pl) => assert!(pl.normal().dot(frame.z) > 0.999),
                other => panic!("not a plane: {other:?}"),
            }
        }
    }
}
