//! Fillets and chamfers on real edges.
//!
//! These used to be a post-tessellation mesh soften: every crease in the
//! part was nudged inward by one shared radius, whichever edges were
//! listed, and the solid underneath was untouched, so a filleted part
//! measured, exported and collided as if it were sharp.
//!
//! Two exact paths now, chosen per body:
//!
//! 1. **Profile blends.** When every listed edge is one an extrude swept
//!    from a corner of its sketch profile (the vertical edges of a plate
//!    or a bracket, which is most fillets in practice), the corner is
//!    rounded in 2D and the prism is rebuilt from the rounded profile,
//!    then every boolean applied to the body since is replayed. The
//!    fillet surface comes out as a true cylinder built by the same sweep
//!    that built the walls. No boolean touches the blend, so the kernel's
//!    trouble with tangent surfaces never arises.
//!
//! 2. **Cutters.** Any other straight edge between two planar faces whose
//!    ends are convex gets a cutter shaped exactly like the material the
//!    blend removes, subtracted from the body. A chamfer cutter meets the
//!    part transversally and is well conditioned. A fillet cutter is
//!    tangent to both faces, which truck's booleans handle less reliably;
//!    when one fails the edge is reported by name rather than faked.
//!
//! Concave edges (blends that add material) and edges ending against an
//! inside corner are refused with that reason.

use std::collections::HashMap;

use truck_modeling::*;

use crate::build;
use crate::error::{CadError, CadResult};
use crate::eval::{boolean_and, boolean_not, boolean_or};
use crate::topology::{base_name, edges_of, find_edge, inherit_names, planar_frame, Body, EdgeInfo, Named, Prism, Replay};
use crate::FeatureOp;

/// How the second side of a chamfer is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Second {
    Equal,
    Distance(f64),
    /// Angle between the chamfer face and the first face, in radians.
    Angle(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BlendKind {
    Fillet { r: f64 },
    Chamfer { d: f64, second: Second },
}

impl BlendKind {
    pub(crate) fn size(&self) -> f64 {
        match *self {
            BlendKind::Fillet { r } => r,
            BlendKind::Chamfer { d, .. } => d,
        }
    }
    pub(crate) fn label(&self) -> &'static str {
        match self {
            BlendKind::Fillet { .. } => "fillet",
            BlendKind::Chamfer { .. } => "chamfer",
        }
    }
    /// The symmetric size, when the profile path can do this blend.
    fn symmetric(&self) -> Option<f64> {
        match *self {
            BlendKind::Fillet { r } => Some(r),
            BlendKind::Chamfer { d, second: Second::Equal } => Some(d),
            BlendKind::Chamfer { .. } => None,
        }
    }
}

fn err(feature: &str, reason: impl Into<String>) -> CadError {
    CadError::EvalFailed { feature: feature.to_string(), reason: reason.into() }
}

/// Blend the listed edges of one body. `edges` pairs each edge with its
/// position `k` in the feature's list, which names the new face
/// `{feature}.face.{k}`.
pub(crate) fn blend_body(
    body: &Body,
    edges: &[(usize, EdgeInfo)],
    feature: &str,
    kind: BlendKind,
    seq: u64,
    tol: f64,
) -> CadResult<(Body, Vec<String>, Vec<String>)> {
    if !(kind.size() > 0.0) {
        return Err(err(feature, format!("{} size must be positive", kind.label())));
    }
    if let (Some(r), Some(prism)) = (kind.symmetric(), &body.prism) {
        if let Some(plan) = lateral_plan(prism, edges) {
            return profile_blend(body, prism, &plan, feature, kind, r, seq, tol);
        }
    }
    cutter_blend(body, edges, feature, kind, seq, tol)
}

/// For edges that are all corners of the body's base profile: where each
/// corner is, as (region, loop, vertex, k). `None` when any edge is not.
fn lateral_plan(prism: &Prism, edges: &[(usize, EdgeInfo)]) -> Option<Vec<(usize, usize, usize, usize)>> {
    let mut at: HashMap<String, (usize, usize, usize, usize)> = HashMap::new();
    for (ri, r) in prism.regions.iter().enumerate() {
        for (li, lp) in std::iter::once(&r.outer).chain(r.holes.iter()).enumerate() {
            for (si, s) in lp.segs.iter().enumerate() {
                at.insert(s.tag.face_name(&prism.feature), (ri, li, si, lp.segs.len()));
            }
        }
    }
    let mut plan = Vec::new();
    for (k, e) in edges {
        let a = at.get(base_name(&e.faces[0]))?;
        let b = at.get(base_name(&e.faces[1]))?;
        if a.0 != b.0 || a.1 != b.1 {
            return None;
        }
        let n = a.3;
        let v = if b.2 == (a.2 + 1) % n {
            b.2
        } else if a.2 == (b.2 + 1) % n {
            a.2
        } else {
            return None;
        };
        plan.push((a.0, a.1, v, *k));
    }
    Some(plan)
}

#[allow(clippy::too_many_arguments)]
fn profile_blend(
    body: &Body,
    prism: &Prism,
    plan: &[(usize, usize, usize, usize)],
    feature: &str,
    kind: BlendKind,
    r: f64,
    seq: u64,
    tol: f64,
) -> CadResult<(Body, Vec<String>, Vec<String>)> {
    let fillet = matches!(kind, BlendKind::Fillet { .. });
    let mut regions = prism.regions.clone();
    let mut groups: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
    for &(ri, li, v, k) in plan {
        groups.entry((ri, li)).or_default().push((v, k));
    }
    for ((ri, li), mut list) in groups {
        // Highest vertex first: each blend inserts a segment, which would
        // shift every later vertex index.
        list.sort_by(|a, b| b.0.cmp(&a.0));
        list.dedup_by_key(|x| x.0);
        let lp = if li == 0 { &mut regions[ri].outer } else { &mut regions[ri].holes[li - 1] };
        for (v, k) in list {
            lp.blend_corner(v, r, fillet, format!("{feature}.face.{k}")).map_err(|e| err(feature, e.to_string()))?;
        }
    }
    let mut solid: Option<Solid> = None;
    let mut names: Vec<String> = Vec::new();
    for (i, region) in regions.iter().enumerate() {
        let (s, n) = build::prism(region, &prism.frame, prism.start, prism.length, &prism.feature, prism.region_index + i, prism.reversed)?;
        match solid.take() {
            None => {
                solid = Some(s);
                names = n;
            }
            Some(acc) => {
                let u = boolean_or(&acc, &s).ok_or_else(|| err(feature, "could not rejoin the profile regions"))?;
                let (s1, s2) = (vec![prism.seq; names.len()], vec![prism.seq; n.len()]);
                let (nn, _) = inherit_names(&u, &[Named { solid: &acc, names: &names, seq: &s1 }, Named { solid: &s, names: &n, seq: &s2 }], tol, feature);
                solid = Some(u);
                names = nn;
            }
        }
    }
    let mut solid = solid.ok_or_else(|| err(feature, "the body's profile is empty"))?;
    let mut seqs: Vec<u64> = names.iter().map(|n| if n.starts_with(feature) { seq } else { prism.seq }).collect();
    for rp in &prism.history {
        let tool_seq = vec![rp.tool_seq; rp.tool_names.len()];
        let r = match rp.op {
            FeatureOp::Subtract => boolean_not(&solid, &rp.tool),
            FeatureOp::Add => boolean_or(&solid, &rp.tool),
            FeatureOp::Intersect => boolean_and(&solid, &rp.tool),
            FeatureOp::NewBody => continue,
        }
        .ok_or_else(|| {
            err(
                feature,
                "the profile was rounded, but a later cut or join on this body could not be re-applied to it; \
                 the blend may be too large for the geometry around that corner",
            )
        })?;
        let (n, q) = inherit_names(
            &r,
            &[Named { solid: &solid, names: &names, seq: &seqs }, Named { solid: &rp.tool, names: &rp.tool_names, seq: &tool_seq }],
            tol,
            feature,
        );
        solid = r;
        names = n;
        seqs = q;
    }
    let mut p2 = prism.clone();
    p2.regions = regions;
    let note = format!(
        "{} edge(s) {}ed exactly from the profile of {}",
        plan.len(),
        kind.label(),
        prism.feature
    );
    Ok((Body { name: body.name.clone(), solid, face_names: names, face_seq: seqs, prism: Some(p2) }, vec![note], Vec::new()))
}

fn cutter_blend(
    body: &Body,
    edges: &[(usize, EdgeInfo)],
    feature: &str,
    kind: BlendKind,
    seq: u64,
    tol: f64,
) -> CadResult<(Body, Vec<String>, Vec<String>)> {
    let mut current = body.clone();
    let mut failed: Vec<String> = Vec::new();
    let mut done = 0usize;
    for (k, e) in edges {
        // Earlier blends in this feature change the topology, so find the
        // edge again on the current body by name.
        let now = edges_of(&current);
        let Some(e2) = find_edge(&now, &e.name) else {
            failed.push(format!("'{}' no longer exists after an earlier blend in this feature", e.name));
            continue;
        };
        match cutter_for(&current, e2, *k, feature, kind) {
            Err(reason) => failed.push(format!("'{}': {reason}", e.name)),
            Ok((cut, cut_names)) => match boolean_not(&current.solid, &cut) {
                Some(r) => {
                    let cs = vec![seq; cut_names.len()];
                    let (n, q) = inherit_names(&r, &[current.named(), Named { solid: &cut, names: &cut_names, seq: &cs }], tol, feature);
                    if let Some(p) = &mut current.prism {
                        p.history.push(Replay { op: FeatureOp::Subtract, tool: cut.clone(), tool_names: cut_names.clone(), tool_seq: seq });
                    }
                    current.solid = r;
                    current.face_names = n;
                    current.face_seq = q;
                    done += 1;
                }
                None => failed.push(format!(
                    "'{}': the kernel could not subtract the {} cutter (truck's booleans are weakest where surfaces \
                     are tangent, which every fillet is)",
                    e.name,
                    kind.label()
                )),
            },
        }
    }
    if done == 0 {
        return Err(err(feature, failed.join("; ")));
    }
    let notes = vec![format!("{done} edge(s) {}ed", kind.label())];
    let degraded = if failed.is_empty() {
        Vec::new()
    } else {
        vec![format!("{} of {} edge(s) not {}ed: {}", failed.len(), edges.len(), kind.label(), failed.join("; "))]
    };
    Ok((current, notes, degraded))
}

/// Direction of the edge `p0 -> p1` as face `f` traverses it. Faces run
/// their outer boundary counter-clockwise about their outward normal, so
/// the face's interior lies to the left of this direction.
fn oriented_dir(f: &Face, p0: Point3, p1: Point3) -> Option<Vector3> {
    let tol = 1.0e-9 * (1.0 + (p1 - p0).magnitude());
    let close = |a: Point3, b: Point3| (a - b).magnitude() <= tol;
    for w in f.boundaries() {
        for e in w.edge_iter() {
            let (a, b) = (e.front().point(), e.back().point());
            if close(a, p0) && close(b, p1) {
                return Some((p1 - p0).normalize());
            }
            if close(a, p1) && close(b, p0) {
                return Some((p0 - p1).normalize());
            }
        }
    }
    None
}

/// The solid a blend of edge `e` removes, extended past the part so no
/// face of it is flush with a face of the body.
fn cutter_for(body: &Body, e: &EdgeInfo, k: usize, feature: &str, kind: BlendKind) -> std::result::Result<(Solid, Vec<String>), String> {
    if !e.straight {
        return Err("the edge is curved; only straight edges can be blended yet".into());
    }
    let faces: Vec<&Face> = body.solid.face_iter().collect();
    let (ia, ib) = (e.face_index[0], e.face_index[1]);
    let (fa, fb) = (faces[ia], faces[ib]);
    let pa = planar_frame(fa).ok_or_else(|| format!("face '{}' is not planar", e.faces[0]))?;
    let pb = planar_frame(fb).ok_or_else(|| format!("face '{}' is not planar", e.faces[1]))?;
    let (na, nb) = (pa.z, pb.z);
    let p0 = Point3::new(e.a[0], e.a[1], e.a[2]);
    let p1 = Point3::new(e.b[0], e.b[1], e.b[2]);
    let len = (p1 - p0).magnitude();
    if len < 1.0e-12 {
        return Err("the edge has no length".into());
    }
    let ev = (p1 - p0) / len;
    let ta = oriented_dir(fa, p0, p1).ok_or("could not find the edge in its first face")?;
    let tb = oriented_dir(fb, p0, p1).ok_or("could not find the edge in its second face")?;
    let da = na.cross(ta).normalize();
    let db = nb.cross(tb).normalize();
    if nb.dot(da) > -1.0e-9 {
        return Err(
            "the edge is concave (an inside corner); blends that add material are not implemented yet".into(),
        );
    }
    let phi = da.dot(db).clamp(-1.0, 1.0).acos();
    if !(phi > 1.0e-6 && phi < std::f64::consts::PI - 1.0e-6) {
        return Err("the faces meet flat, so there is no corner to blend".into());
    }
    let tol = 1.0e-9 * (1.0 + len);
    for (p, out_dir) in [(p0, -ev), (p1, ev)] {
        for (fi, f) in faces.iter().enumerate() {
            if fi == ia || fi == ib || !f.vertex_iter().any(|v| (v.point() - p).magnitude() <= tol) {
                continue;
            }
            let Some(pf) = planar_frame(f) else {
                return Err("an end of the edge meets a curved face; blending it is not implemented yet".into());
            };
            if pf.z.dot(out_dir) <= 1.0e-6 {
                return Err(
                    "an end of the edge runs into material (it ends at an inside corner); blending such an edge \
                     is not implemented yet"
                        .into(),
                );
            }
        }
    }
    let size = kind.size();
    let m = 2.0 * size + 1.0e-4;
    let base = p0 - ev * m;
    let (pts, arc_mid): (Vec<Point3>, Option<Point3>) = match kind {
        BlendKind::Chamfer { d, second } => {
            let d2 = match second {
                Second::Equal => d,
                Second::Distance(x) => x,
                Second::Angle(th) => {
                    // Law of sines in the triangle (corner, qa, qb).
                    let s = (th + phi).sin();
                    if !(th > 0.0) || s.abs() < 1.0e-9 {
                        return Err("the chamfer angle does not form a triangle with this corner".into());
                    }
                    d * th.sin() / s
                }
            };
            if !(d2 > 0.0) {
                return Err("the chamfer's second distance must be positive".into());
            }
            let qa = base + da * d;
            let qb = base + db * d2;
            (vec![qa, qa + na * m, base + (na + nb) * m, qb + nb * m, qb], None)
        }
        BlendKind::Fillet { r } => {
            let t = r / (phi * 0.5).tan();
            let ta_p = base + da * t;
            let tb_p = base + db * t;
            let c = base + (da + db).normalize() * (r / (phi * 0.5).sin());
            let mid = c + (base - c).normalize() * r;
            (vec![ta_p, ta_p + na * m, base + (na + nb) * m, tb_p + nb * m, tb_p], Some(mid))
        }
    };
    // Wind the section about +ev, so a sweep along +ev is outward-facing.
    let c0 = pts[0];
    let newell = (1..pts.len() - 1).fold(Vector3::new(0.0, 0.0, 0.0), |acc, i| acc + (pts[i] - c0).cross(pts[i + 1] - c0));
    let pts: Vec<Point3> = if newell.dot(ev) < 0.0 { pts.into_iter().rev().collect() } else { pts };
    let verts: Vec<Vertex> = pts.iter().map(|p| builder::vertex(*p)).collect();
    let mut edges: Vec<Edge> = (0..verts.len() - 1).map(|i| builder::line(&verts[i], &verts[i + 1])).collect();
    // The closing edge joins the two points on the body's faces: it is
    // the chamfer line, or the fillet arc.
    let last = verts.len() - 1;
    edges.push(match arc_mid {
        Some(mid) => builder::circle_arc(&verts[last], &verts[0], mid),
        None => builder::line(&verts[last], &verts[0]),
    });
    let wire: Wire = edges.into();
    let face = builder::try_attach_plane(&[wire]).map_err(|e| format!("could not build the cutter section: {e}"))?;
    let solid = builder::tsweep(&face, ev * (len + 2.0 * m));
    let mut names = vec![format!("{feature}.cutter.{k}.end0")];
    names.extend((0..4).map(|i| format!("{feature}.cutter.{k}.s{i}")));
    names.push(format!("{feature}.face.{k}"));
    names.push(format!("{feature}.cutter.{k}.end1"));
    if solid.face_iter().count() != names.len() {
        return Err("internal: the cutter came out with an unexpected number of faces".into());
    }
    Ok((solid, names))
}
