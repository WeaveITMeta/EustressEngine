//! Mesh measurement — mass properties, topology, and clearance.
//!
//! These read an [`EvalMesh`] and report what it *is*, which is the half
//! of CAD that authoring alone cannot supply. Geometry here fails
//! silently and partially: a boolean can succeed and hand back a solid
//! with holes, and tessellation can drop a face within tolerance without
//! raising. Nothing downstream notices until a later step builds on the
//! corrupt body and the error surfaces misattributed. So the kernel
//! computes the probe values, and every surface — Studio panel, MCP
//! tool, exporter — reads the same ones.
//!
//! Volume and centre of mass come from the divergence theorem and are
//! therefore only meaningful on a *closed* surface. [`topology`] is what
//! establishes whether that holds, and callers are expected to report
//! the two together rather than quoting a volume for an open shell as
//! though it were sound.

use crate::eval::EvalMesh;

// ── Vector helpers (EvalMesh stores f32; all math here is f64) ───────

#[inline] fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0]-b[0], a[1]-b[1], a[2]-b[2]] }
#[inline] fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0]+b[0], a[1]+b[1], a[2]+b[2]] }
#[inline] fn mul(a: [f64; 3], s: f64) -> [f64; 3] { [a[0]*s, a[1]*s, a[2]*s] }
#[inline] fn dot(a: [f64; 3], b: [f64; 3]) -> f64 { a[0]*b[0] + a[1]*b[1] + a[2]*b[2] }
#[inline] fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1]*b[2] - a[2]*b[1], a[2]*b[0] - a[0]*b[2], a[0]*b[1] - a[1]*b[0]]
}
#[inline] fn len(a: [f64; 3]) -> f64 { dot(a, a).sqrt() }

#[inline]
fn tri(mesh: &EvalMesh, t: &[u32]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let g = |i: u32| {
        let v = mesh.positions[i as usize];
        [v[0] as f64, v[1] as f64, v[2] as f64]
    };
    (g(t[0]), g(t[1]), g(t[2]))
}

// ── Mass properties ─────────────────────────────────────────────────

/// Volume, area, centroid and bounds of a triangle mesh. All lengths in
/// metres, the engine-native unit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MassProps {
    /// Signed volume — negative means the surface winding is inverted.
    /// Callers that just want magnitude should use [`MassProps::volume`].
    pub signed_volume: f64,
    pub surface_area: f64,
    /// Volume-weighted centre of mass. Meaningless on an open surface.
    pub centroid: [f64; 3],
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub triangles: usize,
}

impl MassProps {
    /// Magnitude of the enclosed volume, independent of winding.
    pub fn volume(&self) -> f64 {
        self.signed_volume.abs()
    }
    pub fn size(&self) -> [f64; 3] {
        sub(self.max, self.min)
    }
    pub fn winding_inverted(&self) -> bool {
        self.signed_volume < 0.0
    }
}

/// Compute mass properties in a single pass over the triangles.
pub fn mass_properties(mesh: &EvalMesh) -> MassProps {
    let mut p = MassProps {
        min: [f64::MAX; 3],
        max: [f64::MIN; 3],
        ..Default::default()
    };
    if mesh.positions.is_empty() {
        p.min = [0.0; 3];
        p.max = [0.0; 3];
        return p;
    }
    for v in &mesh.positions {
        for a in 0..3 {
            let x = v[a] as f64;
            if x < p.min[a] { p.min[a] = x; }
            if x > p.max[a] { p.max[a] = x; }
        }
    }

    // First moment accumulator for the volume-weighted centroid:
    //   ∫x dV = (1/48) Σ n_k · [(a+b)² + (b+c)² + (c+a)²]
    let mut moment = [0.0f64; 3];
    for t in mesh.indices.chunks_exact(3) {
        let (a, b, c) = tri(mesh, t);
        let n = cross(sub(b, a), sub(c, a)); // |n| = 2 × area
        p.surface_area += 0.5 * len(n);
        // Divergence theorem: 6V = Σ a · (b × c)
        p.signed_volume += dot(a, cross(b, c)) / 6.0;
        for k in 0..3 {
            let s = a[k] + b[k];
            let u = b[k] + c[k];
            let w = c[k] + a[k];
            moment[k] += n[k] * (s * s + u * u + w * w);
        }
        p.triangles += 1;
    }
    if p.signed_volume.abs() > 1.0e-15 {
        for k in 0..3 {
            p.centroid[k] = moment[k] / 48.0 / p.signed_volume;
        }
    }
    p
}

// ── Topology ────────────────────────────────────────────────────────

/// Edge-sharing census of a mesh. On a closed manifold every edge is
/// shared by exactly two triangles; ones and threes are how a dropped
/// face or a self-intersecting boolean actually shows up in the output.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TopoReport {
    /// Distinct positions after welding — lower than `positions.len()`
    /// whenever tessellation split corners at creases.
    pub welded_vertices: usize,
    /// Edges used by exactly one triangle: the surface is open there.
    pub boundary_edges: usize,
    /// Edges used by three or more triangles.
    pub nonmanifold_edges: usize,
    pub degenerate_triangles: usize,
}

impl TopoReport {
    /// Closed: no edge is used by only one triangle.
    pub fn is_watertight(&self) -> bool {
        self.boundary_edges == 0
    }
    /// Closed *and* every edge used by exactly two triangles.
    pub fn is_manifold(&self) -> bool {
        self.boundary_edges == 0 && self.nonmanifold_edges == 0
    }
}

/// Weld coincident positions to `weld_eps`, then count triangles per edge.
///
/// Welding is keyed on POSITION, never on index. [`crate::tessellate_solid`]
/// splits corners at creases, so one physical edge carries different
/// indices on each side — an index-keyed census would report every
/// crease in the model as a hole.
pub fn topology(mesh: &EvalMesh, weld_eps: f64) -> TopoReport {
    use std::collections::HashMap;
    let mut r = TopoReport::default();
    if mesh.indices.is_empty() {
        return r;
    }
    let q = weld_eps.max(1.0e-9);
    let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut welded: Vec<u32> = Vec::with_capacity(mesh.positions.len());
    for v in &mesh.positions {
        let key = (
            (v[0] as f64 / q).round() as i64,
            (v[1] as f64 / q).round() as i64,
            (v[2] as f64 / q).round() as i64,
        );
        let next = ids.len() as u32;
        welded.push(*ids.entry(key).or_insert(next));
    }
    r.welded_vertices = ids.len();

    // Sliver threshold scales with the model, so a millimetre part and a
    // ten-metre part aren't judged against the same absolute area.
    let diag = {
        let mp = mass_properties(mesh);
        len(mp.size()).max(1.0e-9)
    };
    let area_eps = (diag * 1.0e-7).powi(2);

    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for t in mesh.indices.chunks_exact(3) {
        let (wa, wb, wc) = (
            welded[t[0] as usize],
            welded[t[1] as usize],
            welded[t[2] as usize],
        );
        let (a, b, c) = tri(mesh, t);
        let area = 0.5 * len(cross(sub(b, a), sub(c, a)));
        if wa == wb || wb == wc || wc == wa || area < area_eps {
            r.degenerate_triangles += 1;
        }
        // Slivers still carry real surface, so their edges are counted —
        // dropping them would fabricate boundary edges on their healthy
        // neighbours. Only fully collapsed pairs are skipped.
        for (u, w) in [(wa, wb), (wb, wc), (wc, wa)] {
            if u == w {
                continue;
            }
            let key = if u < w { (u, w) } else { (w, u) };
            *edges.entry(key).or_insert(0) += 1;
        }
    }
    for count in edges.values() {
        match count {
            1 => r.boundary_edges += 1,
            2 => {}
            _ => r.nonmanifold_edges += 1,
        }
    }
    r
}

// ── Clearance ───────────────────────────────────────────────────────

/// Closest point on triangle `abc` to `p`
/// (Ericson, *Real-Time Collision Detection* §5.1.5).
fn closest_point_on_tri(p: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 { return a; }

    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 { return b; }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, mul(ab, d1 / (d1 - d3)));
    }

    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 { return c; }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, mul(ac, d2 / (d2 - d6)));
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add(b, mul(sub(c, b), w));
    }

    let denom = 1.0 / (va + vb + vc);
    add(add(a, mul(ab, vb * denom)), mul(ac, vc * denom))
}

/// Squared distance between two segments.
fn seg_seg_dist2(p1: [f64; 3], q1: [f64; 3], p2: [f64; 3], q2: [f64; 3]) -> f64 {
    const EPS: f64 = 1.0e-12;
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);

    let (mut s, mut t);
    if a <= EPS && e <= EPS {
        return dot(r, r);
    }
    if a <= EPS {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= EPS {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            s = if denom > EPS { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
        }
    }
    let c1 = add(p1, mul(d1, s));
    let c2 = add(p2, mul(d2, t));
    let d = sub(c1, c2);
    dot(d, d)
}

/// Exact squared distance between two triangles: the minimum over
/// vertex-to-face (both directions) and all nine edge-edge pairs.
fn tri_tri_dist2(
    a: ([f64; 3], [f64; 3], [f64; 3]),
    b: ([f64; 3], [f64; 3], [f64; 3]),
) -> f64 {
    let mut best = f64::MAX;
    let av = [a.0, a.1, a.2];
    let bv = [b.0, b.1, b.2];
    for &p in &av {
        let d = sub(p, closest_point_on_tri(p, b.0, b.1, b.2));
        best = best.min(dot(d, d));
    }
    for &p in &bv {
        let d = sub(p, closest_point_on_tri(p, a.0, a.1, a.2));
        best = best.min(dot(d, d));
    }
    for i in 0..3 {
        let (p1, q1) = (av[i], av[(i + 1) % 3]);
        for j in 0..3 {
            let (p2, q2) = (bv[j], bv[(j + 1) % 3]);
            best = best.min(seg_seg_dist2(p1, q1, p2, q2));
        }
    }
    best
}

fn tri_aabbs(mesh: &EvalMesh) -> Vec<([f64; 3], [f64; 3])> {
    mesh.indices
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = tri(mesh, t);
            let mut lo = [0.0f64; 3];
            let mut hi = [0.0f64; 3];
            for k in 0..3 {
                lo[k] = a[k].min(b[k]).min(c[k]);
                hi[k] = a[k].max(b[k]).max(c[k]);
            }
            (lo, hi)
        })
        .collect()
}

#[inline]
fn aabb_gap2(x: &([f64; 3], [f64; 3]), y: &([f64; 3], [f64; 3])) -> f64 {
    let mut s = 0.0;
    for k in 0..3 {
        let d = (y.0[k] - x.1[k]).max(x.0[k] - y.1[k]).max(0.0);
        s += d * d;
    }
    s
}

/// Minimum surface-to-surface distance between two meshes, in metres.
///
/// Returns `(distance, exact)`. When the triangle-pair count would make
/// the exact sweep pathological the result degrades to the bounding-box
/// separation — a strict lower bound — and `exact` is `false`, rather
/// than the call hanging.
pub fn min_distance(a: &EvalMesh, b: &EvalMesh) -> (f64, bool) {
    const PAIR_BUDGET: usize = 30_000_000;
    let (ta, tb) = (a.indices.len() / 3, b.indices.len() / 3);
    if ta == 0 || tb == 0 {
        return (f64::NAN, false);
    }
    if ta.saturating_mul(tb) > PAIR_BUDGET {
        let (ma, mb) = (mass_properties(a), mass_properties(b));
        return (aabb_gap2(&(ma.min, ma.max), &(mb.min, mb.max)).sqrt(), false);
    }
    let boxes_a = tri_aabbs(a);
    let boxes_b = tri_aabbs(b);
    let mut best = f64::MAX;
    for (i, t) in a.indices.chunks_exact(3).enumerate() {
        let va = tri(a, t);
        for (j, u) in b.indices.chunks_exact(3).enumerate() {
            // Cheap reject before paying for the full triangle solve.
            if aabb_gap2(&boxes_a[i], &boxes_b[j]) >= best {
                continue;
            }
            let d2 = tri_tri_dist2(va, tri(b, u));
            if d2 < best {
                best = d2;
                if best <= 0.0 {
                    return (0.0, true);
                }
            }
        }
    }
    (best.sqrt(), true)
}

// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Axis-aligned box centred at the origin, outward winding.
    fn unit_box(sx: f64, sy: f64, sz: f64) -> EvalMesh {
        let (hx, hy, hz) = (sx / 2.0, sy / 2.0, sz / 2.0);
        let p: Vec<[f32; 3]> = vec![
            [-hx as f32, -hy as f32, -hz as f32], // 0
            [ hx as f32, -hy as f32, -hz as f32], // 1
            [ hx as f32,  hy as f32, -hz as f32], // 2
            [-hx as f32,  hy as f32, -hz as f32], // 3
            [-hx as f32, -hy as f32,  hz as f32], // 4
            [ hx as f32, -hy as f32,  hz as f32], // 5
            [ hx as f32,  hy as f32,  hz as f32], // 6
            [-hx as f32,  hy as f32,  hz as f32], // 7
        ];
        // CCW seen from outside each face.
        let idx: Vec<u32> = vec![
            4, 5, 6, 4, 6, 7, // +Z
            1, 0, 3, 1, 3, 2, // -Z
            5, 1, 2, 5, 2, 6, // +X
            0, 4, 7, 0, 7, 3, // -X
            3, 7, 6, 3, 6, 2, // +Y
            0, 1, 5, 0, 5, 4, // -Y
        ];
        EvalMesh { positions: p, normals: vec![], uvs: vec![], indices: idx }
    }

    #[test]
    fn box_volume_area_and_centroid_match_analytic() {
        // 100 × 60 × 10 mm, the plate from the CAD acceptance test.
        let (x, y, z) = (0.100, 0.060, 0.010);
        let m = unit_box(x, y, z);
        let p = mass_properties(&m);

        let expect_v = x * y * z;
        let expect_a = 2.0 * (x * y + y * z + z * x);
        // The acceptance test's own bar is 0.1%.
        assert!(
            (p.volume() - expect_v).abs() / expect_v < 1.0e-3,
            "volume {} vs analytic {expect_v}",
            p.volume()
        );
        assert!(
            (p.surface_area - expect_a).abs() / expect_a < 1.0e-3,
            "area {} vs analytic {expect_a}",
            p.surface_area
        );
        for k in 0..3 {
            assert!(p.centroid[k].abs() < 1.0e-6, "centroid off origin: {:?}", p.centroid);
        }
        assert!(!p.winding_inverted());
        assert_eq!(p.triangles, 12);
    }

    #[test]
    fn reversed_winding_reports_negative_volume() {
        let mut m = unit_box(0.1, 0.1, 0.1);
        for t in m.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
        let p = mass_properties(&m);
        assert!(p.winding_inverted(), "flipped winding should read negative");
        // Magnitude is unchanged — only the sign carries the defect.
        assert!((p.volume() - 0.001).abs() / 0.001 < 1.0e-3);
    }

    #[test]
    fn closed_box_is_manifold() {
        let m = unit_box(0.1, 0.06, 0.01);
        let t = topology(&m, 1.0e-6);
        assert!(t.is_watertight(), "boundary edges: {}", t.boundary_edges);
        assert!(t.is_manifold(), "non-manifold edges: {}", t.nonmanifold_edges);
        assert_eq!(t.welded_vertices, 8, "a box has 8 distinct corners");
        assert_eq!(t.degenerate_triangles, 0);
    }

    #[test]
    fn dropped_face_shows_up_as_boundary_edges() {
        // Exactly the failure mode this exists to catch: a tessellation
        // that loses one face still "succeeds" and still renders.
        let mut m = unit_box(0.1, 0.1, 0.1);
        m.indices.truncate(m.indices.len() - 6); // drop the -Y face
        let t = topology(&m, 1.0e-6);
        assert!(!t.is_watertight(), "an open shell must not report watertight");
        assert_eq!(t.boundary_edges, 4, "one missing quad = 4 boundary edges");
    }

    #[test]
    fn welding_is_by_position_so_creases_are_not_holes() {
        // Duplicate every vertex and rewrite indices to use the copies,
        // mimicking the crease-splitting tessellate_solid performs. The
        // surface is unchanged, so the census must be unchanged too.
        let base = unit_box(0.1, 0.1, 0.1);
        let n = base.positions.len() as u32;
        let mut m = base.clone();
        m.positions.extend_from_slice(&base.positions);
        for (k, i) in m.indices.iter_mut().enumerate() {
            if k % 2 == 0 {
                *i += n;
            }
        }
        let t = topology(&m, 1.0e-6);
        assert_eq!(t.welded_vertices, 8, "duplicates must weld back to 8");
        assert!(t.is_manifold(), "crease splits must not read as holes");
    }

    #[test]
    fn min_distance_matches_a_known_gap() {
        let a = unit_box(0.1, 0.1, 0.1);
        let mut b = unit_box(0.1, 0.1, 0.1);
        // Shift +X by 0.25 m → faces at +0.05 and +0.20, gap 0.15 m.
        for p in b.positions.iter_mut() {
            p[0] += 0.25;
        }
        let (d, exact) = min_distance(&a, &b);
        assert!(exact);
        assert!((d - 0.15).abs() < 1.0e-6, "expected 0.15 m gap, got {d}");
    }

    #[test]
    fn overlapping_bodies_report_zero_distance() {
        let a = unit_box(0.1, 0.1, 0.1);
        let mut b = unit_box(0.1, 0.1, 0.1);
        for p in b.positions.iter_mut() {
            p[0] += 0.05; // half-overlap
        }
        let (d, _) = min_distance(&a, &b);
        assert!(d < 1.0e-9, "interpenetrating bodies should read ~0, got {d}");
    }

    #[test]
    fn empty_mesh_is_handled_without_panicking() {
        let m = EvalMesh::default();
        let p = mass_properties(&m);
        assert_eq!(p.triangles, 0);
        assert_eq!(p.volume(), 0.0);
        let t = topology(&m, 1.0e-6);
        assert_eq!(t.welded_vertices, 0);
        // An empty mesh has no boundary, but it is not a solid either —
        // callers gate on triangle count, not on is_watertight alone.
        assert!(t.is_watertight());
    }
}
