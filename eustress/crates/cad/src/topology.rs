//! Bodies, and names for their faces and edges that survive regeneration.
//!
//! ## Why names, and why these names
//!
//! Every edit to a parametric part rebuilds its geometry from scratch, so
//! a feature that operates on existing geometry (a fillet on an edge, a
//! sketch on a face, a shell opening a face) needs a way to say WHICH
//! edge or face that survives the rebuild. The previous reference format
//! was a positional index, `"Extrude1/edge-4"`, and the kernel never
//! resolved it at all: fillets applied to every crease in the part.
//! Positional indices would not have survived anyway, because a hole
//! drilled upstream renumbers everything after it.
//!
//! Names here are semantic and come from where the geometry came from:
//!
//! | face | name |
//! |------|------|
//! | extrude end caps | `Extrude1.cap_start`, `Extrude1.cap_end` |
//! | wall swept from sketch entity 3 | `Extrude1.side.e3` |
//! | one side of a rectangle (entity 0) | `Extrude1.side.e0.top` |
//! | a hole's bore | `Hole1.wall.0`, `Hole1.wall.1` |
//! | a copy made by Pattern1 | `Hole1.wall.0@Pattern1.2` |
//!
//! An edge is named by the two faces it separates, sorted and joined:
//! `"Extrude1.cap_end | Extrude1.side.e0.top"`. That is stable for the
//! same reason the face names are, and it reads as what it is.
//!
//! ## How names survive booleans
//!
//! truck-shapeops builds every face of a boolean result on a surface it
//! took, unchanged, from one of the operands: it trims faces, it never
//! invents surfaces (`divide_one_face` reuses `face.surface()`). So each
//! result face is identified by matching its surface against the named
//! faces of the operands. Planes are compared by their defining points,
//! not by plane equation, so two coplanar faces from different features
//! keep their own names. Anything else is compared by sampling the
//! surface over its parameter range. When a face is cut into pieces the
//! pieces share a surface and so share a name; the first by position
//! keeps it bare and the rest are numbered `#2`, `#3`.

use std::collections::HashMap;
use std::ops::Bound;

use truck_modeling::*;

use crate::frame::Frame;
use crate::profile::Region;
use crate::FeatureOp;

/// A solid with a name for every face.
#[derive(Debug, Clone)]
pub struct Body {
    /// Stable body name, taken from the feature that created it.
    pub name: String,
    pub solid: Solid,
    /// One name per face, in `solid.face_iter()` order.
    pub face_names: Vec<String>,
    /// Creation stamp of the feature that made each face. When two
    /// operand faces sit on identical surfaces, the older one wins.
    pub(crate) face_seq: Vec<u64>,
    /// How to rebuild this body from its profile, while it still is a
    /// prism plus booleans. Cleared by anything that is not.
    pub(crate) prism: Option<Prism>,
}

/// A body that is an extruded profile plus a list of booleans.
///
/// Keeping this lets a feature that edits the base profile (a blend on
/// an edge the extrude swept from a sketch corner, or a shell) rebuild
/// the prism exactly from 2D geometry and replay the booleans after it,
/// instead of cutting the result with a boolean it may not survive.
#[derive(Debug, Clone)]
pub(crate) struct Prism {
    pub feature: String,
    pub frame: Frame,
    pub regions: Vec<Region>,
    /// Which region of the extrude this was, so rebuilt caps keep their
    /// `.rN` names.
    pub region_index: usize,
    /// Lower end along `frame.z`.
    pub start: f64,
    /// Always positive.
    pub length: f64,
    /// The "start" cap is the one on the sketch plane side; when the
    /// extrusion ran against the normal it is the upper one.
    pub reversed: bool,
    pub seq: u64,
    pub history: Vec<Replay>,
}

/// One boolean applied to a prism body, recorded for replay.
#[derive(Debug, Clone)]
pub(crate) struct Replay {
    pub op: FeatureOp,
    pub tool: Solid,
    pub tool_names: Vec<String>,
    pub tool_seq: u64,
}

impl Body {
    /// A freshly built body whose faces all come from one feature.
    pub(crate) fn fresh(name: String, solid: Solid, face_names: Vec<String>, seq: u64) -> Body {
        let n = face_names.len();
        Body { name, solid, face_names, face_seq: vec![seq; n], prism: None }
    }

    pub(crate) fn named(&self) -> Named<'_> {
        Named { solid: &self.solid, names: &self.face_names, seq: &self.face_seq }
    }

    pub fn face_count(&self) -> usize {
        self.face_names.len()
    }
}

/// A solid together with its face names, as a source of lineage.
pub(crate) struct Named<'a> {
    pub solid: &'a Solid,
    pub names: &'a [String],
    pub seq: &'a [u64],
}

// ── Surface identity ────────────────────────────────────────────────

/// Identity of the surface a face lies on. See the module docs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfKey {
    plane: bool,
    v: [f64; 15],
}

fn bound_or(b: Bound<f64>, default: f64) -> f64 {
    match b {
        Bound::Included(x) | Bound::Excluded(x) if x.is_finite() => x,
        _ => default,
    }
}

pub(crate) fn surf_key(s: &Surface) -> Option<SurfKey> {
    let mut v = [0.0; 15];
    if let Surface::Plane(p) = s {
        let o = p.origin();
        let a = o + p.u_axis();
        let b = o + p.v_axis();
        v[..9].copy_from_slice(&[o.x, o.y, o.z, a.x, a.y, a.z, b.x, b.y, b.z]);
        return Some(SurfKey { plane: true, v });
    }
    let s = s.clone();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let (ur, vr) = s.parameter_range();
        let (u0, u1) = (bound_or(ur.0, 0.0), bound_or(ur.1, 1.0));
        let (v0, v1) = (bound_or(vr.0, 0.0), bound_or(vr.1, 1.0));
        let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let mut out = [0.0; 15];
        for (i, (u, w)) in [(u0, v0), (u1, v0), (u0, v1), (u1, v1), (um, vm)].into_iter().enumerate() {
            let p = s.subs(u, w);
            out[i * 3..i * 3 + 3].copy_from_slice(&[p.x, p.y, p.z]);
        }
        SurfKey { plane: false, v: out }
    }))
    .ok()
}

fn keys_match(a: &SurfKey, b: &SurfKey, tol: f64) -> bool {
    a.plane == b.plane && a.v.iter().zip(b.v.iter()).all(|(x, y)| (x - y).abs() <= tol)
}

/// Strip a `#k` disambiguation suffix.
pub(crate) fn base_name(name: &str) -> &str {
    match name.rfind('#') {
        Some(i) if name[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < name.len() => &name[..i],
        _ => name,
    }
}

/// Name every face of `result` after the operand face it was cut from.
///
/// `fallback` names any face no operand accounts for; with truck's
/// surface-preserving booleans that should not happen, and the fallback
/// makes it visible rather than silent if it ever does.
pub(crate) fn inherit_names(
    result: &Solid,
    sources: &[Named<'_>],
    tol: f64,
    fallback: &str,
) -> (Vec<String>, Vec<u64>) {
    let src: Vec<(SurfKey, &str, u64)> = sources
        .iter()
        .flat_map(|s| {
            s.solid
                .face_iter()
                .zip(s.names.iter().zip(s.seq.iter()))
                .filter_map(|(f, (n, q))| surf_key(&f.surface()).map(|k| (k, base_name(n), *q)))
        })
        .collect();
    let mut names = Vec::new();
    let mut seqs = Vec::new();
    for (i, f) in result.face_iter().enumerate() {
        let best = surf_key(&f.surface()).and_then(|k| {
            src.iter()
                .filter(|(sk, _, _)| keys_match(sk, &k, tol))
                .min_by_key(|(_, _, q)| *q)
        });
        match best {
            Some((_, n, q)) => {
                names.push((*n).to_string());
                seqs.push(*q);
            }
            None => {
                names.push(format!("{fallback}.face{i}"));
                seqs.push(u64::MAX);
            }
        }
    }
    disambiguate(result, &mut names);
    (names, seqs)
}

/// Give faces that share a name a stable `#k` suffix, ordered by where
/// they are, so the numbering does not depend on truck's face order.
pub(crate) fn disambiguate(solid: &Solid, names: &mut [String]) {
    let centroids: Vec<[f64; 3]> = solid.face_iter().map(face_centroid).collect();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, n) in names.iter().enumerate() {
        groups.entry(base_name(n).to_string()).or_default().push(i);
    }
    for (base, mut idx) in groups {
        if idx.len() < 2 {
            if let Some(&i) = idx.first() {
                names[i] = base;
            }
            continue;
        }
        idx.sort_by(|&a, &b| lex_cmp(centroids.get(a), centroids.get(b)));
        for (k, &i) in idx.iter().enumerate() {
            names[i] = if k == 0 { base.clone() } else { format!("{base}#{}", k + 1) };
        }
    }
}

fn lex_cmp(a: Option<&[f64; 3]>, b: Option<&[f64; 3]>) -> std::cmp::Ordering {
    // Quantize so the order is immune to last-bit noise from the
    // scale-normalized booleans.
    let q = |v: Option<&[f64; 3]>| v.map(|p| (*p).map(|x| (x * 1.0e7).round() as i64)).unwrap_or([0; 3]);
    q(a).cmp(&q(b))
}

/// Average of a face's boundary vertices. A cheap, deterministic
/// position for ordering; not the area centroid.
pub(crate) fn face_centroid(f: &Face) -> [f64; 3] {
    let mut s = [0.0; 3];
    let mut n = 0.0;
    for v in f.vertex_iter() {
        let p = v.point();
        s[0] += p.x;
        s[1] += p.y;
        s[2] += p.z;
        n += 1.0;
    }
    if n > 0.0 {
        [s[0] / n, s[1] / n, s[2] / n]
    } else {
        s
    }
}

// ── Queries ─────────────────────────────────────────────────────────

/// A face, as the tool surface and the sketch-on-face resolver see it.
#[derive(Debug, Clone)]
pub struct FaceInfo {
    pub body: String,
    pub name: String,
    pub index: usize,
    /// Outward frame when the face is planar: origin at the boundary
    /// centroid, normal pointing out of the material.
    pub plane: Option<Frame>,
    pub centroid: [f64; 3],
    pub kind: &'static str,
}

/// An edge between two faces.
#[derive(Debug, Clone)]
pub struct EdgeInfo {
    pub body: String,
    pub name: String,
    pub faces: [String; 2],
    pub face_index: [usize; 2],
    pub a: [f64; 3],
    pub b: [f64; 3],
    /// Geometrically straight: a line, or the meeting of two planes.
    pub straight: bool,
}

impl EdgeInfo {
    pub fn length(&self) -> f64 {
        let d = [self.b[0] - self.a[0], self.b[1] - self.a[1], self.b[2] - self.a[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

/// The outward plane of a face, when it is planar.
///
/// Surfaces truck builds as `Plane` are read directly. A ruled or swept
/// B-spline surface can also be flat (a loft between parallel straight
/// edges), so other surfaces are tested by sampling a 3x3 grid.
pub(crate) fn planar_frame(f: &Face) -> Option<Frame> {
    let sign = if f.orientation() { 1.0 } else { -1.0 };
    let c = face_centroid(f);
    let centroid = Point3::new(c[0], c[1], c[2]);
    match f.surface() {
        Surface::Plane(p) => {
            let n = p.normal() * sign;
            let o = centroid - n * (centroid - p.origin()).dot(n);
            Frame::from_origin_normal(o, n)
        }
        s => {
            let s2 = s.clone();
            let sample = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let (ur, vr) = s2.parameter_range();
                let (u0, u1) = (bound_or(ur.0, 0.0), bound_or(ur.1, 1.0));
                let (v0, v1) = (bound_or(vr.0, 0.0), bound_or(vr.1, 1.0));
                let mut pts = Vec::with_capacity(9);
                for i in 0..3 {
                    for j in 0..3 {
                        let u = u0 + (u1 - u0) * i as f64 * 0.5;
                        let v = v0 + (v1 - v0) * j as f64 * 0.5;
                        pts.push(s2.subs(u, v));
                    }
                }
                let nm = s2.normal(0.5 * (u0 + u1), 0.5 * (v0 + v1));
                (pts, nm)
            }))
            .ok()?;
            let (pts, nm) = sample;
            let n = (pts[6] - pts[0]).cross(pts[2] - pts[0]);
            if !(n.magnitude() > 1.0e-18) {
                return None;
            }
            let n = n.normalize();
            let scale = (pts[8] - pts[0]).magnitude().max(1.0e-12);
            if pts.iter().any(|p| (*p - pts[0]).dot(n).abs() > scale * 1.0e-9) {
                return None;
            }
            let n = if n.dot(nm) * sign < 0.0 { -n } else { n };
            let o = centroid - n * (centroid - pts[0]).dot(n);
            Frame::from_origin_normal(o, n)
        }
    }
}

fn surface_kind(f: &Face) -> &'static str {
    match f.surface() {
        Surface::Plane(_) => "plane",
        Surface::RevolutedCurve(_) => "revolved",
        Surface::BSplineSurface(_) => "bspline",
        Surface::NurbsSurface(_) => "nurbs",
    }
}

pub fn faces_of(body: &Body) -> Vec<FaceInfo> {
    body.solid
        .face_iter()
        .enumerate()
        .map(|(index, f)| {
            let plane = planar_frame(f);
            FaceInfo {
                body: body.name.clone(),
                name: body.face_names.get(index).cloned().unwrap_or_default(),
                index,
                kind: if plane.is_some() { "planar" } else { surface_kind(f) },
                plane,
                centroid: face_centroid(f),
            }
        })
        .collect()
}

/// Every edge shared by exactly two faces, named by the pair.
pub fn edges_of(body: &Body) -> Vec<EdgeInfo> {
    let planar: Vec<bool> = body.solid.face_iter().map(|f| planar_frame(f).is_some()).collect();
    let mut by_id: HashMap<EdgeID, Vec<(usize, Edge)>> = HashMap::new();
    let mut order: Vec<EdgeID> = Vec::new();
    for (fi, f) in body.solid.face_iter().enumerate() {
        for e in f.edge_iter() {
            let id = e.id();
            let slot = by_id.entry(id).or_default();
            if slot.is_empty() {
                order.push(id);
            }
            slot.push((fi, e));
        }
    }
    let mut out: Vec<EdgeInfo> = Vec::new();
    for id in order {
        let uses = &by_id[&id];
        if uses.len() != 2 {
            continue;
        }
        let (f0, e0) = &uses[0];
        let f1 = uses[1].0;
        let n0 = body.face_names.get(*f0).cloned().unwrap_or_default();
        let n1 = body.face_names.get(f1).cloned().unwrap_or_default();
        let (names, idx) = if n0 <= n1 { ([n0, n1], [*f0, f1]) } else { ([n1, n0], [f1, *f0]) };
        let a = e0.front().point();
        let b = e0.back().point();
        out.push(EdgeInfo {
            body: body.name.clone(),
            name: format!("{} | {}", names[0], names[1]),
            faces: names,
            face_index: idx,
            a: [a.x, a.y, a.z],
            b: [b.x, b.y, b.z],
            straight: matches!(e0.curve(), Curve::Line(_)) || (planar[*f0] && planar[f1]),
        });
    }
    // Two faces can meet along more than one edge (a face cut in two by
    // a slot still borders the wall on both sides). Number them by
    // position, like duplicate face names.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, e) in out.iter().enumerate() {
        groups.entry(e.name.clone()).or_default().push(i);
    }
    for (base, mut idx) in groups {
        if idx.len() < 2 {
            continue;
        }
        let mid = |i: usize| {
            let e = &out[i];
            [(e.a[0] + e.b[0]) * 0.5, (e.a[1] + e.b[1]) * 0.5, (e.a[2] + e.b[2]) * 0.5]
        };
        idx.sort_by(|&x, &y| lex_cmp(Some(&mid(x)), Some(&mid(y))));
        for (k, &i) in idx.iter().enumerate() {
            if k > 0 {
                out[i].name = format!("{base}#{}", k + 1);
            }
        }
    }
    out.sort_by(|x, y| x.name.cmp(&y.name));
    out
}

/// Look an edge up by name. The two face names may be given in either
/// order, and a `#k` suffix picks one of several edges between the same
/// pair.
pub fn find_edge<'a>(edges: &'a [EdgeInfo], query: &str) -> Option<&'a EdgeInfo> {
    let q = query.trim();
    if let Some(e) = edges.iter().find(|e| e.name == q) {
        return Some(e);
    }
    let (pair, suffix) = match q.rfind('#') {
        Some(i) if q[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < q.len() => (&q[..i], Some(&q[i..])),
        _ => (q, None),
    };
    let mut parts: Vec<&str> = pair.split('|').map(str::trim).collect();
    if parts.len() != 2 {
        return None;
    }
    parts.sort_unstable();
    let canonical = match suffix {
        Some(s) => format!("{} | {}{s}", parts[0], parts[1]),
        None => format!("{} | {}", parts[0], parts[1]),
    };
    edges.iter().find(|e| e.name == canonical)
}

/// Every body vertex projected onto an axis, as (min, max).
pub(crate) fn extent_along(solid: &Solid, origin: Point3, axis: Vector3) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for f in solid.face_iter() {
        for v in f.vertex_iter() {
            let d = (v.point() - origin).dot(axis);
            lo = lo.min(d);
            hi = hi.max(d);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// Axis-aligned bounds of a solid's vertices.
pub(crate) fn bounds(solid: &Solid) -> Option<([f64; 3], [f64; 3])> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for f in solid.face_iter() {
        for v in f.vertex_iter() {
            let p = v.point();
            for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                lo[k] = lo[k].min(c);
                hi[k] = hi[k].max(c);
            }
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

pub(crate) fn bounds_overlap(a: &([f64; 3], [f64; 3]), b: &([f64; 3], [f64; 3]), margin: f64) -> bool {
    (0..3).all(|k| a.0[k] <= b.1[k] + margin && b.0[k] <= a.1[k] + margin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_name_strips_only_numeric_suffixes() {
        assert_eq!(base_name("Extrude1.cap_end#2"), "Extrude1.cap_end");
        assert_eq!(base_name("Extrude1.cap_end"), "Extrude1.cap_end");
        assert_eq!(base_name("odd#name"), "odd#name");
        assert_eq!(base_name("trailing#"), "trailing#");
    }

    #[test]
    fn edge_lookup_accepts_either_face_order() {
        let e = EdgeInfo {
            body: "B".into(),
            name: "A.x | A.y".into(),
            faces: ["A.x".into(), "A.y".into()],
            face_index: [0, 1],
            a: [0.0; 3],
            b: [1.0, 0.0, 0.0],
            straight: true,
        };
        let edges = vec![e];
        assert!(find_edge(&edges, "A.y | A.x").is_some());
        assert!(find_edge(&edges, "A.x|A.y").is_some());
        assert!(find_edge(&edges, "A.x | A.z").is_none());
    }
}
