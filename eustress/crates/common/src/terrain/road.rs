//! Spline path, elevation profile and drivable surface of a road.
//!
//! Pure math, no `Entity`/`Commands`/ECS. A road is a `TerrainSpline` layer
//! (see `layers` and `layer_instances`): the layer bake calls
//! [`build_road_path_with`] for the corridor's path and smoothed profile and
//! carves the ground to them, and `road_surface` lays the ribbon mesh
//! ([`build_ribbon_mesh`]) and the collision boxes ([`ribbon_segments`])
//! along the stations that bake produced ([`RoadPath::from_positions`]), so
//! the drivable surface and the carved ground follow one line.
//!
//! Deliberately self-contained rather than reusing
//! `realism::numerics::interpolation::{spline_build, spline_eval}` (an
//! equivalent natural-cubic-spline solver already exists there) — `terrain`
//! is NOT gated behind the `realism` feature (see `common/Cargo.toml`'s
//! default feature list), and reaching into a sibling opt-in feature from an
//! unconditionally-compiled module would silently make every `terrain`
//! consumer require `realism` too. The duplicated math is ~40 lines.
//!
//! ## Algorithm (elevation, not just XZ)
//! 1. **Path** — centripetal Catmull-Rom through the XZ of the control
//!    points (local control: moving one node doesn't disturb distant curve
//!    segments, unlike a global natural-spline solve; no cusp/loop
//!    pathologies at sharp turns).
//! 2. **Arc length** — a dense polyline sample of that curve gives a
//!    cumulative-length table; stations are placed at even arc-length
//!    spacing, not even parameter-`t` spacing (parameter spacing bunches up
//!    on tight curves).
//! 3. **Elevation profile** — sampling raw terrain height AT EVERY station
//!    would just reproduce every bump the mountain already has. Instead,
//!    sample height at SPARSE knots (~15 m apart) from the ground, then fit
//!    those knots with the same natural-cubic-spline technique, so the
//!    road's target elevation is smooth by construction.
//!
//! The corridor itself (for every cell, the closest point on the station
//! polyline, a flat bed and a smoothstep shoulder back to the ground) is the
//! layer bake's, in `layers`. It never writes the terrain's base, so moving a
//! node re-carves from the untouched ground instead of compounding a trench.

use bevy::prelude::*;

// ============================================================================
// Self-contained natural cubic spline (Thomas tridiagonal algorithm) — see
// module docs for why this isn't `realism::numerics::interpolation`.
// ============================================================================

pub(crate) fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Second-derivative solve for a natural cubic spline through `(xs[i], ys[i])`
/// (`xs` strictly increasing). Feed the result to [`spline_eval`].
fn spline_build(xs: &[f32], ys: &[f32]) -> Vec<f32> {
    let n = xs.len();
    if n < 3 {
        return vec![0.0; n];
    }

    let mut sub = vec![0.0f32; n];
    let mut diag = vec![1.0f32; n];
    let mut sup = vec![0.0f32; n];
    let mut rhs = vec![0.0f32; n];
    // Natural boundary conditions: d2[0] = d2[n-1] = 0 (diag already 1, rhs already 0).

    for i in 1..n - 1 {
        let h_im1 = (xs[i] - xs[i - 1]).max(1e-6);
        let h_i = (xs[i + 1] - xs[i]).max(1e-6);
        sub[i] = h_im1;
        diag[i] = 2.0 * (h_im1 + h_i);
        sup[i] = h_i;
        rhs[i] = 6.0 * ((ys[i + 1] - ys[i]) / h_i - (ys[i] - ys[i - 1]) / h_im1);
    }

    // Thomas algorithm (tridiagonal solve).
    let mut cp = vec![0.0f32; n];
    let mut rp = vec![0.0f32; n];
    cp[0] = sup[0] / diag[0];
    rp[0] = rhs[0] / diag[0];
    for i in 1..n {
        let denom = diag[i] - sub[i] * cp[i - 1];
        let denom = if denom.abs() < 1e-9 { 1e-9 } else { denom };
        cp[i] = sup[i] / denom;
        rp[i] = (rhs[i] - sub[i] * rp[i - 1]) / denom;
    }
    let mut d2 = vec![0.0f32; n];
    d2[n - 1] = rp[n - 1];
    for i in (0..n - 1).rev() {
        d2[i] = rp[i] - cp[i] * d2[i + 1];
    }
    d2
}

/// Evaluate the spline built by [`spline_build`] at `x` (domain-clamped).
fn spline_eval(xs: &[f32], ys: &[f32], d2: &[f32], x: f32) -> f32 {
    let n = xs.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return ys[0];
    }
    let x = x.clamp(xs[0], xs[n - 1]);
    let mut lo = 0usize;
    let mut hi = n - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if xs[mid] <= x { lo = mid; } else { hi = mid; }
    }
    let h = (xs[hi] - xs[lo]).max(1e-6);
    let a = (xs[hi] - x) / h;
    let b = (x - xs[lo]) / h;
    a * ys[lo] + b * ys[hi] + ((a * a * a - a) * d2[lo] + (b * b * b - b) * d2[hi]) * (h * h) / 6.0
}

// ============================================================================
// Centripetal Catmull-Rom path
// ============================================================================

/// Centripetal Catmull-Rom interpolation between `p1..p2` (with neighbours
/// `p0`,`p3` for tangent context) at local `t ∈ [0,1]`. Centripetal (α=0.5)
/// parameterization avoids the loop/cusp artifacts a uniform Catmull-Rom
/// produces when control points are unevenly spaced — exactly the case for
/// hand-placed road nodes.
fn catmull_rom_centripetal(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    fn knot(prev: f32, a: Vec2, b: Vec2) -> f32 {
        prev + (b - a).length().max(1e-4).sqrt()
    }
    let t0 = 0.0f32;
    let t1 = knot(t0, p0, p1);
    let t2 = knot(t1, p1, p2);
    let t3 = knot(t2, p2, p3);
    let tt = t1 + (t2 - t1) * t;

    let a1 = p0 * ((t1 - tt) / (t1 - t0)) + p1 * ((tt - t0) / (t1 - t0));
    let a2 = p1 * ((t2 - tt) / (t2 - t1)) + p2 * ((tt - t1) / (t2 - t1));
    let a3 = p2 * ((t3 - tt) / (t3 - t2)) + p3 * ((tt - t2) / (t3 - t2));
    let b1 = a1 * ((t2 - tt) / (t2 - t0)) + a2 * ((tt - t0) / (t2 - t0));
    let b2 = a2 * ((t3 - tt) / (t3 - t1)) + a3 * ((tt - t1) / (t3 - t1));
    b1 * ((t2 - tt) / (t2 - t1)) + b2 * ((tt - t1) / (t2 - t1))
}

/// Samples per Catmull-Rom segment in [`sample_path_xz`]'s dense polyline.
/// A named constant (not just the literal passed at its one call site)
/// because [`build_road_path_with`] also needs it: control point `i` sits at
/// EXACTLY dense-index `i * CATMULL_ROM_SAMPLES` (the `t=0` sample of
/// segment `i`), so elevation-knot derivation can index directly instead of
/// nearest-point-searching the polyline — a hairpin (this is a drift
/// mountain road; it will have hairpins) can bring two arc-length-distant
/// points close together in XZ, making a naive nearest-XZ search pick the
/// wrong one.
const CATMULL_ROM_SAMPLES: u32 = 24;

/// Densely sample the XZ path through `control_points` (need ≥2). Each
/// segment between consecutive points gets `samples_per_segment` steps;
/// missing neighbours at the path ends are approximated by mirroring the
/// nearest real point (standard open-curve boundary handling) rather than
/// wrapping — a road is not a closed loop.
fn sample_path_xz(control_points: &[Vec3], samples_per_segment: u32) -> Vec<Vec2> {
    let pts: Vec<Vec2> = control_points.iter().map(|p| Vec2::new(p.x, p.z)).collect();
    let n = pts.len();
    if n < 2 {
        return pts;
    }
    let mut out = Vec::with_capacity(n * samples_per_segment as usize);
    for i in 0..n - 1 {
        let p0 = if i == 0 { pts[0] * 2.0 - pts[1] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < n { pts[i + 2] } else { pts[n - 1] * 2.0 - pts[n - 2] };
        for s in 0..samples_per_segment {
            let t = s as f32 / samples_per_segment as f32;
            out.push(catmull_rom_centripetal(p0, p1, p2, p3, t));
        }
    }
    out.push(pts[n - 1]);
    out
}

/// The dense XZ polyline [`build_road_path_with`] follows through
/// `control_points`: every station it emits lies on it, so its bounding box
/// holds the whole path. Terrain layers take their footprint from it before
/// any elevation is sampled.
pub(crate) fn dense_path_xz(control_points: &[Vec3]) -> Vec<Vec2> {
    sample_path_xz(control_points, CATMULL_ROM_SAMPLES)
}

/// One station along the road's arc length.
#[derive(Clone, Copy, Debug)]
pub struct RoadStation {
    /// Cumulative arc length from the path start (metres).
    pub s: f32,
    /// World XZ position on the path (Y is the SMOOTHED profile, not raw terrain).
    pub pos: Vec3,
    /// Normalized tangent direction (XZ).
    pub tangent: Vec2,
}

/// A fully-resolved road path: densely arc-length-sampled stations carrying
/// a smoothed elevation profile, ready for a corridor bake or ribbon-mesh
/// generation.
pub struct RoadPath {
    pub stations: Vec<RoadStation>,
    pub total_length: f32,
}

impl RoadPath {
    /// The path through `positions`, world points in path order with the
    /// profile as their Y (the stations a layer bake carved a corridor
    /// along): arc length measured in XZ, as [`build_road_path_with`]
    /// measures it, and each tangent the XZ direction from the point before
    /// to the point after, one-sided at the ends. `None` for fewer than two
    /// points, a point that is not finite, or a path with no length.
    pub fn from_positions(positions: &[Vec3]) -> Option<RoadPath> {
        if positions.len() < 2 || positions.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let xz = |p: Vec3| Vec2::new(p.x, p.z);
        let last = positions.len() - 1;
        let mut s = 0.0f32;
        let mut stations = Vec::with_capacity(positions.len());
        for (i, &pos) in positions.iter().enumerate() {
            if i > 0 {
                s += (xz(pos) - xz(positions[i - 1])).length();
            }
            let tangent = (xz(positions[(i + 1).min(last)]) - xz(positions[i.saturating_sub(1)])).normalize_or_zero();
            stations.push(RoadStation { s, pos, tangent });
        }
        (s >= 1e-3).then_some(RoadPath { stations, total_length: s })
    }
}

/// Build a [`RoadPath`] from control points, with the ground its extra
/// elevation knots sample supplied as `ground(world_x, world_z)`, called once
/// per knot in path order. `station_spacing` governs corridor and render
/// density (~1-2 m is reasonable); `elevation_knot_spacing` governs how
/// coarsely the SMOOTHED profile is fit (~10-20 m; finer just reproduces
/// terrain bumps). Terrain layers pass the ground as the layers ordered
/// before them leave it, so a road laid over a noise layer follows the noisy
/// ground rather than the raster underneath.
pub fn build_road_path_with(
    control_points: &[Vec3],
    station_spacing: f32,
    elevation_knot_spacing: f32,
    mut ground: impl FnMut(f32, f32) -> f32,
) -> Option<RoadPath> {
    if control_points.len() < 2 || station_spacing <= 0.0 {
        return None;
    }
    let dense = sample_path_xz(control_points, CATMULL_ROM_SAMPLES);
    if dense.len() < 2 {
        return None;
    }
    let n_points = control_points.len();

    // Cumulative arc length over the dense polyline.
    let mut cum = Vec::with_capacity(dense.len());
    cum.push(0.0f32);
    for i in 1..dense.len() {
        cum.push(cum[i - 1] + (dense[i] - dense[i - 1]).length());
    }
    let total_length = *cum.last().unwrap_or(&0.0);
    if total_length < 1e-3 {
        return None;
    }

    // Elevation knots come from the CONTROL POINTS' own Y — never
    // discarded in favor of an independent terrain re-sample. Nodes get
    // their initial Y from a terrain raycast at placement time (see
    // `RoadNodePlaceTool`), and the existing move gizmo can adjust it
    // afterward; if the profile ignored `pos.y` and only re-sampled raw
    // terrain, dragging a node would have ZERO effect on the resulting
    // road — silently breaking the "edit nodes with the existing gizmo"
    // promise. The spline SMOOTHS between authored points; it does not
    // override what the user (or the placement raycast) actually set.
    // `elevation_knot_spacing` still bounds extra terrain-sampled knots
    // inserted on long inter-node stretches, so a very sparse road doesn't
    // travel arrow-straight in elevation between two distant nodes.
    // Control point `i` sits at EXACTLY dense-index `i * CATMULL_ROM_SAMPLES`
    // (the `t=0` sample of segment `i`), by `sample_path_xz`'s own
    // construction — direct index lookup, not a nearest-XZ search (which a
    // hairpin could fool into picking a different arm of the curve).
    let dense_index_of = |i: usize| -> usize {
        ((i as u32 * CATMULL_ROM_SAMPLES) as usize).min(dense.len() - 1)
    };

    let mut knot_s = Vec::with_capacity(n_points * 2);
    let mut knot_h = Vec::with_capacity(n_points * 2);
    for (i, cp) in control_points.iter().enumerate() {
        let s = cum[dense_index_of(i)];
        knot_s.push(s);
        knot_h.push(cp.y);

        // Insert extra terrain-sampled sub-knots on long stretches to the
        // NEXT node so a sparse road still follows the mountain between
        // widely-spaced control points, not just a straight elevation lerp.
        if i + 1 < n_points {
            let s_next = cum[dense_index_of(i + 1)];
            let span = s_next - s;
            let sub_knots = ((span / elevation_knot_spacing.max(1.0)).floor() as usize).min(64);
            for k in 1..=sub_knots {
                let sub_s = s + span * k as f32 / (sub_knots + 1) as f32;
                let xz = sample_polyline_at_arclength(&dense, &cum, sub_s);
                knot_s.push(sub_s);
                knot_h.push(ground(xz.x, xz.y));
            }
        }
    }
    let d2 = spline_build(&knot_s, &knot_h);

    // Emit stations at even arc-length spacing along the dense polyline,
    // with the smoothed profile's Y and a numerically-differenced tangent.
    let station_count = ((total_length / station_spacing).ceil() as usize + 1).max(2);
    let mut stations = Vec::with_capacity(station_count);
    for i in 0..station_count {
        let s = total_length * i as f32 / (station_count - 1) as f32;
        let xz = sample_polyline_at_arclength(&dense, &cum, s);
        let y = spline_eval(&knot_s, &knot_h, &d2, s);
        let ds = (total_length * 0.002).max(0.05);
        let xz_fwd = sample_polyline_at_arclength(&dense, &cum, (s + ds).min(total_length));
        let xz_back = sample_polyline_at_arclength(&dense, &cum, (s - ds).max(0.0));
        let tangent = (xz_fwd - xz_back).normalize_or_zero();
        stations.push(RoadStation { s, pos: Vec3::new(xz.x, y, xz.y), tangent });
    }

    Some(RoadPath { stations, total_length })
}

/// Interpolate the dense polyline `pts` (with cumulative lengths `cum`) at
/// arc length `s`.
fn sample_polyline_at_arclength(pts: &[Vec2], cum: &[f32], s: f32) -> Vec2 {
    let s = s.clamp(0.0, *cum.last().unwrap_or(&0.0));
    let mut lo = 0usize;
    let mut hi = cum.len() - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if cum[mid] <= s { lo = mid; } else { hi = mid; }
    }
    let seg_len = (cum[hi] - cum[lo]).max(1e-6);
    let t = ((s - cum[lo]) / seg_len).clamp(0.0, 1.0);
    pts[lo].lerp(pts[hi], t)
}

// ============================================================================
// Drivable surface: ribbon mesh and collision boxes
// ============================================================================

/// Road cross-section parameters.
#[derive(Clone, Copy, Debug)]
pub struct RoadProfile {
    /// Half-width of the flat driving bed (metres).
    pub half_width: f32,
    /// Width of the shoulder beyond `half_width` over which the corridor
    /// blends back into the ground (metres).
    pub shoulder_falloff: f32,
}

impl Default for RoadProfile {
    fn default() -> Self {
        Self { half_width: 4.0, shoulder_falloff: 6.0 }
    }
}

/// How much longer than its segment each collision box is, metres, so
/// neighbouring boxes overlap rather than leave a seam where the road bends.
const SEGMENT_OVERLAP: f32 = 0.1;

/// One box of a road's collision surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadSegmentBox {
    /// World centre of the box.
    pub center: Vec3,
    /// Local -Z runs along the road, local +Y is up the road's grade.
    pub rotation: Quat,
    /// Full side lengths along local X (across the bed), Y and Z (along it).
    pub size: Vec3,
}

/// The boxes a collider chains along `path` so a wheel meets the surface
/// [`build_ribbon_mesh`] draws with the same `lift`: one per pair of
/// stations, as wide as the bed, `thickness` thick with its top face `lift`
/// above the profile and the rest in the ground (so a fast wheel cannot pass
/// through), and [`SEGMENT_OVERLAP`] longer than its segment.
pub fn ribbon_segments(path: &RoadPath, profile: RoadProfile, lift: f32, thickness: f32) -> Vec<RoadSegmentBox> {
    path.stations
        .windows(2)
        .filter_map(|pair| {
            let along = pair[1].pos - pair[0].pos;
            let length = along.length();
            if !(length > 1e-4) {
                return None;
            }
            let rotation = Transform::IDENTITY.looking_to(along / length, Vec3::Y).rotation;
            let up = rotation * Vec3::Y;
            Some(RoadSegmentBox {
                center: (pair[0].pos + pair[1].pos) * 0.5 + up * (lift - thickness * 0.5),
                rotation,
                size: Vec3::new(profile.half_width * 2.0, thickness, length + SEGMENT_OVERLAP),
            })
        })
        .collect()
}

/// A generated quad-strip ribbon mesh along the road path.
pub struct RoadRibbonMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Build a quad-strip ribbon (UV by arc length) `lift` above the carved bed
/// to avoid z-fighting with the terrain mesh. This is the drivable surface:
/// the caller gives it a collider of [`ribbon_segments`] (an ECS concern),
/// since the terrain's own heightfield collider sits `lift` below it.
pub fn build_ribbon_mesh(path: &RoadPath, profile: RoadProfile, lift: f32) -> RoadRibbonMesh {
    let mut positions = Vec::with_capacity(path.stations.len() * 2);
    let mut normals = Vec::with_capacity(path.stations.len() * 2);
    let mut uvs = Vec::with_capacity(path.stations.len() * 2);
    let mut indices = Vec::with_capacity(path.stations.len().saturating_sub(1) * 6);

    for st in &path.stations {
        let normal_xz = Vec2::new(-st.tangent.y, st.tangent.x);
        let left = st.pos + Vec3::new(normal_xz.x, 0.0, normal_xz.y) * profile.half_width;
        let right = st.pos - Vec3::new(normal_xz.x, 0.0, normal_xz.y) * profile.half_width;
        positions.push([left.x, left.y + lift, left.z]);
        positions.push([right.x, right.y + lift, right.z]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([0.0, st.s]);
        uvs.push([1.0, st.s]);
    }

    for i in 0..path.stations.len().saturating_sub(1) {
        let base = (i * 2) as u32;
        // Two triangles per quad, counter-clockwise seen from above (left
        // lies on the tangent's anticlockwise side), so back-face culling
        // keeps the top face.
        indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }

    RoadRibbonMesh { positions, normals, uvs, indices }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_build_eval_passes_through_knots() {
        let xs = vec![0.0, 10.0, 20.0, 30.0, 40.0];
        let ys = vec![0.0, 5.0, 2.0, 8.0, 3.0];
        let d2 = spline_build(&xs, &ys);
        for i in 0..xs.len() {
            let v = spline_eval(&xs, &ys, &d2, xs[i]);
            assert!((v - ys[i]).abs() < 1e-3, "knot {i}: expected {}, got {v}", ys[i]);
        }
    }

    #[test]
    fn catmull_rom_passes_through_control_points() {
        let pts = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 5.0),
            Vec3::new(20.0, 0.0, -5.0),
            Vec3::new(30.0, 0.0, 0.0),
        ];
        let dense = sample_path_xz(&pts, 16);
        // First and last dense samples should equal the first/last control points.
        assert!((dense[0] - Vec2::new(0.0, 0.0)).length() < 1e-3);
        assert!((dense.last().unwrap() - &Vec2::new(30.0, 0.0)).length() < 1e-3);
    }

    #[test]
    fn build_road_path_with_produces_stations_spanning_full_length() {
        let pts = vec![
            Vec3::new(-20.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(20.0, 0.0, 0.0),
        ];
        let path = build_road_path_with(&pts, 2.0, 15.0, |_, _| 0.0).expect("should build");
        assert!(path.total_length > 0.0);
        assert!((path.stations.first().unwrap().s - 0.0).abs() < 1e-3);
        assert!((path.stations.last().unwrap().s - path.total_length).abs() < 1e-3);
    }

    #[test]
    fn a_path_through_stations_measures_xz_and_takes_neighbour_tangents() {
        let path = RoadPath::from_positions(&[
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(3.0, 5.0, 4.0),
            Vec3::new(6.0, 5.0, 8.0),
        ])
        .expect("a path");
        assert_eq!(path.stations.len(), 3);
        assert!((path.stations[1].s - 5.0).abs() < 1e-5, "arc length is measured in XZ");
        assert!((path.total_length - 10.0).abs() < 1e-5);
        assert_eq!(path.stations[1].pos, Vec3::new(3.0, 5.0, 4.0), "the profile is the points' own Y");
        for station in &path.stations {
            assert!((station.tangent - Vec2::new(0.6, 0.8)).length() < 1e-5, "{:?}", station.tangent);
        }

        assert!(RoadPath::from_positions(&[Vec3::ZERO]).is_none());
        assert!(RoadPath::from_positions(&[Vec3::ONE, Vec3::ONE + Vec3::Y]).is_none(), "no length on the ground");
        assert!(RoadPath::from_positions(&[Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0)]).is_none());
    }

    #[test]
    fn the_ribbon_faces_up_and_its_boxes_meet_it() {
        let path = RoadPath::from_positions(&[
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0),
            Vec3::new(20.0, 12.0, 0.0),
        ])
        .expect("a path");
        let profile = RoadProfile { half_width: 4.0, shoulder_falloff: 2.0 };

        let mesh = build_ribbon_mesh(&path, profile, 0.05);
        assert_eq!(mesh.indices.len(), 12);
        let at = |i: u32| Vec3::from(mesh.positions[i as usize]);
        for triangle in mesh.indices.chunks(3) {
            let normal = (at(triangle[1]) - at(triangle[0])).cross(at(triangle[2]) - at(triangle[0]));
            assert!(normal.y > 0.0, "triangle {triangle:?} faces down, so culling hides it from above");
        }

        let boxes = ribbon_segments(&path, profile, 0.05, 0.3);
        assert_eq!(boxes.len(), 2, "one box per pair of stations");

        // Level: full bed width, a little longer than the segment, the top
        // face 5 cm above the profile.
        let level = boxes[0];
        assert!((level.center - Vec3::new(5.0, 9.9, 0.0)).length() < 1e-4, "{:?}", level.center);
        assert!((level.size - Vec3::new(8.0, 0.3, 10.1)).length() < 1e-4, "{:?}", level.size);

        // Climbing: local -Z along the segment, local +Y leaning back against
        // the climb, and the top face's centre `lift` above the midpoint.
        let climb = boxes[1];
        let forward = climb.rotation * Vec3::NEG_Z;
        assert!((forward - Vec3::new(10.0, 2.0, 0.0).normalize()).length() < 1e-4, "{forward:?}");
        let up = climb.rotation * Vec3::Y;
        assert!(up.y > 0.9 && up.x < 0.0, "{up:?}");
        let top = climb.center + up * 0.15;
        assert!((top - (Vec3::new(15.0, 11.0, 0.0) + up * 0.05)).length() < 1e-4, "{top:?}");
    }
}
