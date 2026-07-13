//! Spline path + terrain-conform core for the road tool.
//!
//! Pure math + `TerrainData` read/write — no `Entity`/`Commands`/ECS. The
//! caller (the `StudioPlugin` in the engine crate) owns node placement,
//! entity spawning, and the pristine-baseline snapshot; this module only
//! turns `[Vec3]` control points + a terrain baseline into (a) a smoothed
//! elevation profile, (b) terrain writes, and (c) a ribbon mesh.
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
//!    sample height at SPARSE knots (~15 m apart) from the baseline terrain,
//!    then fit those knots with the same natural-cubic-spline technique —
//!    the road's target elevation is smooth by construction.
//! 4. **Corridor stamp** — for every terrain texel near the path, find the
//!    SINGLE closest station (not every station within range — stamping
//!    per-station ridges the inside of a hairpin, since multiple stations
//!    claim the same texel there). Flat bed inside `half_width`, smoothstep
//!    shoulder out to `half_width + falloff` blending to the pristine
//!    baseline. Always reads from `baseline`, writes into `data` — so
//!    re-applying after a node edit re-stamps fresh rather than compounding
//!    a trench into already-carved terrain.

use bevy::prelude::*;
use super::{TerrainConfig, TerrainData};
use super::height_query::{height_at_world, set_height_at_world, set_splat_at_world};

// ============================================================================
// Self-contained natural cubic spline (Thomas tridiagonal algorithm) — see
// module docs for why this isn't `realism::numerics::interpolation`.
// ============================================================================

fn smoothstep(t: f32) -> f32 {
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
/// because [`build_road_path`] also needs it: control point `i` sits at
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
/// a smoothed elevation profile, ready for terrain-conform or ribbon-mesh
/// generation.
pub struct RoadPath {
    pub stations: Vec<RoadStation>,
    pub total_length: f32,
}

/// Build a [`RoadPath`] from control points against `baseline` terrain
/// (elevation knots are sampled from `baseline`, NEVER from `data` being
/// actively edited — see module docs on why conform always reads a
/// pristine snapshot). `station_spacing` governs stamp/render density
/// (~1-2 m is reasonable); `elevation_knot_spacing` governs how coarsely the
/// SMOOTHED profile is fit (~10-20 m — finer just reproduces terrain bumps).
pub fn build_road_path(
    config: &TerrainConfig,
    baseline: &TerrainData,
    control_points: &[Vec3],
    station_spacing: f32,
    elevation_knot_spacing: f32,
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
                knot_h.push(height_at_world(config, baseline, xz.x, xz.y));
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
// Terrain conform — closest-approach corridor stamp
// ============================================================================

/// Road cross-section parameters.
#[derive(Clone, Copy, Debug)]
pub struct RoadProfile {
    /// Half-width of the flat driving bed (metres).
    pub half_width: f32,
    /// Extra distance beyond `half_width` over which the shoulder
    /// smoothsteps back down to the untouched baseline.
    pub shoulder_falloff: f32,
}

impl Default for RoadProfile {
    fn default() -> Self {
        Self { half_width: 4.0, shoulder_falloff: 6.0 }
    }
}

/// Result of a terrain-conform pass, for UI feedback.
#[derive(Debug, Default)]
pub struct ConformResult {
    pub cells_written: usize,
}

/// Stamp `path` into `data` (mutated), reading ONLY from `baseline` (never
/// from `data`) — every call is a fresh re-stamp from the pristine terrain,
/// so repeated Apply presses after moving a node never compound into a
/// trench. Resolves each texel by its SINGLE closest station (not every
/// station within range) to avoid ridging on the inside of tight turns.
///
/// `cell_size` should match the terrain's own cell spacing
/// (`chunk_size / chunk_resolution`) — finer sampling wastes work, coarser
/// leaves gaps.
pub fn conform_terrain_to_road(
    config: &TerrainConfig,
    baseline: &TerrainData,
    data: &mut TerrainData,
    path: &RoadPath,
    profile: RoadProfile,
    cell_size: f32,
) -> ConformResult {
    let mut result = ConformResult::default();
    if path.stations.len() < 2 || cell_size <= 0.0 {
        return result;
    }

    let reach = profile.half_width + profile.shoulder_falloff;

    // World-space AABB of the path expanded by the corridor's full reach.
    let (mut min_x, mut max_x, mut min_z, mut max_z) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for st in &path.stations {
        min_x = min_x.min(st.pos.x - reach);
        max_x = max_x.max(st.pos.x + reach);
        min_z = min_z.min(st.pos.z - reach);
        max_z = max_z.max(st.pos.z + reach);
    }

    let mut x = min_x;
    while x <= max_x {
        let mut z = min_z;
        while z <= max_z {
            // Closest-approach: scan stations for the nearest one (linear —
            // fine for a single mountain road's station count; a coarse
            // spatial bucket over stations is the natural follow-up if this
            // ever needs to run on a much longer road).
            let mut best_dist_sq = f32::MAX;
            let mut best_station: Option<&RoadStation> = None;
            for st in &path.stations {
                let d = Vec2::new(x - st.pos.x, z - st.pos.z).length_squared();
                if d < best_dist_sq {
                    best_dist_sq = d;
                    best_station = Some(st);
                }
            }
            if let Some(st) = best_station {
                // Signed lateral offset from the path centerline at this station.
                let to_point = Vec2::new(x - st.pos.x, z - st.pos.z);
                let normal = Vec2::new(-st.tangent.y, st.tangent.x);
                let lateral = to_point.dot(normal).abs();

                if lateral <= reach {
                    let baseline_h = height_at_world(config, baseline, x, z);
                    let (target_h, weight, channel) = if lateral <= profile.half_width {
                        (st.pos.y, 1.0, 2usize) // flat bed, full overwrite, dirt
                    } else {
                        let t = (lateral - profile.half_width) / profile.shoulder_falloff.max(1e-3);
                        let blend = 1.0 - smoothstep(t); // 1 at bed edge, 0 at baseline
                        (st.pos.y * blend + baseline_h * (1.0 - blend), blend.max(0.05), 1usize) // rock shoulder
                    };
                    set_height_at_world(config, data, x, z, target_h, weight);
                    set_splat_at_world(config, data, x, z, channel, weight);
                    result.cells_written += 1;
                }
            }
            z += cell_size;
        }
        x += cell_size;
    }

    result
}

// ============================================================================
// Ribbon mesh (visible/drivable surface)
// ============================================================================

/// A generated quad-strip ribbon mesh along the road path.
pub struct RoadRibbonMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Build a quad-strip ribbon (UV by arc length) slightly above the conformed
/// bed to avoid z-fighting with the terrain mesh. This is the actual
/// drivable surface — it carries its own collider (added by the caller, an
/// ECS concern) because terrain colliders are disabled project-wide.
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
        // Two triangles per quad, matching the ribbon's left/right winding.
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }

    RoadRibbonMesh { positions, normals, uvs, indices }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 64.0,
            chunk_resolution: 32,
            chunks_x: 4,
            chunks_z: 4,
            lod_levels: 1,
            lod_distances: vec![64.0],
            view_distance: 512.0,
            height_scale: 50.0,
            seed: 1,
        }
    }

    fn flat_data(config: &TerrainConfig) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        data
    }

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
    fn build_road_path_produces_stations_spanning_full_length() {
        let config = test_config();
        let data = flat_data(&config);
        let pts = vec![
            Vec3::new(-20.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(20.0, 0.0, 0.0),
        ];
        let path = build_road_path(&config, &data, &pts, 2.0, 15.0).expect("should build");
        assert!(path.total_length > 0.0);
        assert!((path.stations.first().unwrap().s - 0.0).abs() < 1e-3);
        assert!((path.stations.last().unwrap().s - path.total_length).abs() < 1e-3);
    }

    #[test]
    fn conform_flattens_bed_and_preserves_far_field() {
        let config = test_config();
        let baseline = flat_data(&config);
        let mut data = flat_data(&config);
        let pts = vec![Vec3::new(-30.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(30.0, 10.0, 0.0)];
        let path = build_road_path(&config, &baseline, &pts, 2.0, 15.0).expect("should build");
        let profile = RoadProfile { half_width: 4.0, shoulder_falloff: 6.0 };
        let cell_size = config.chunk_size / config.chunk_resolution as f32;
        let result = conform_terrain_to_road(&config, &baseline, &mut data, &path, profile, cell_size);
        assert!(result.cells_written > 0);

        // On the centerline, height should read close to the road's target (10.0).
        let bed_h = height_at_world(&config, &data, 0.0, 0.0);
        assert!((bed_h - 10.0).abs() < 2.0, "expected bed near 10.0, got {bed_h}");

        // Far outside the corridor, height should remain the untouched baseline (0.0).
        let far_h = height_at_world(&config, &data, 0.0, 200.0);
        assert!(far_h.abs() < 1.0, "expected far-field untouched near 0.0, got {far_h}");
    }

    #[test]
    fn reapply_from_baseline_does_not_dig_trench() {
        let config = test_config();
        let baseline = flat_data(&config);
        let mut data = flat_data(&config);
        let pts = vec![Vec3::new(-30.0, 10.0, 0.0), Vec3::new(30.0, 10.0, 0.0)];
        let path = build_road_path(&config, &baseline, &pts, 2.0, 15.0).expect("should build");
        let profile = RoadProfile::default();
        let cell_size = config.chunk_size / config.chunk_resolution as f32;

        conform_terrain_to_road(&config, &baseline, &mut data, &path, profile, cell_size);
        let first_pass = height_at_world(&config, &data, 0.0, 0.0);
        // Re-apply several times — always from `baseline`, into the ALREADY-carved `data`.
        for _ in 0..5 {
            conform_terrain_to_road(&config, &baseline, &mut data, &path, profile, cell_size);
        }
        let repeated_pass = height_at_world(&config, &data, 0.0, 0.0);
        assert!((first_pass - repeated_pass).abs() < 0.5, "re-apply should not drift: {first_pass} vs {repeated_pass}");
    }
}
