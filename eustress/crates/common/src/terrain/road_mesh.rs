//! What a designed road becomes: its surface meshes, its painted markings,
//! and the shaped ground of its corridor.
//!
//! Pure math over a [`road_design`](super::road_design) design, no ECS:
//! - [`build_road`] lays one mesh per surface (carriageway, shoulders, kerbs,
//!   sidewalks) along the stations, UVs in metres (u across, v along), and
//!   the markings as thin strips just above the carriageway: a yellow centre
//!   line, dashed where the passing sight distance is there and doubled
//!   where it is not, white lane lines between same-direction lanes, white
//!   edge lines.
//! - [`Corridor`] answers the ground the road leaves at any point: the
//!   formation within it, the daylight slope out to the catch point, and
//!   the natural ground beyond. The terrain bake samples it, so the carved
//!   ground and the surface meshes come from one cross-section.

use super::road_design::{
    cross_slope, Alignment, CrossSection, EdgeTreatment, ProfileMode, RoadSpec, RoadStandard, SectionPoint,
    Station, VerticalProfile, Vpi, Zone, P2,
};

/// Triangle-list mesh data, `f32` for the GPU.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// A quad strip between two polylines `a` and `b` of equal length (rows
    /// along the road), each vertex with its UV.
    pub(super) fn strip(&mut self, a: &[([f64; 3], [f64; 2])], b: &[([f64; 3], [f64; 2])]) {
        let n = a.len().min(b.len());
        if n < 2 {
            return;
        }
        let base = self.positions.len() as u32;
        for i in 0..n {
            for (p, uv) in [a[i], b[i]] {
                self.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                self.uvs.push([uv[0] as f32, uv[1] as f32]);
                self.normals.push([0.0, 1.0, 0.0]);
            }
        }
        for i in 0..n as u32 - 1 {
            let (a0, b0, a1, b1) = (
                base + 2 * i,
                base + 2 * i + 1,
                base + 2 * i + 2,
                base + 2 * i + 3,
            );
            self.indices.extend([a0, a1, b0, b0, a1, b1]);
        }
    }

    /// One quad `a b c d` (counter-clockwise seen from the side it faces),
    /// with the flat normal of that side, and UVs from `uv`. A quad with two
    /// corners at one point is the triangle the other three make: its normal
    /// comes from whichever of `a b c` and `a c d` has area, and it is left
    /// out only when neither does.
    pub(super) fn quad(&mut self, q: [[f64; 3]; 4], uv: [[f64; 2]; 4]) {
        let [a, b, c, d] = q;
        let normal = |b: [f64; 3], c: [f64; 3]| {
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        };
        let len = |n: [f64; 3]| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let mut n = normal(b, c);
        let mut l = len(n);
        if !(l > 1e-12) {
            n = normal(c, d);
            l = len(n);
        }
        if !(l > 1e-12) {
            return;
        }
        let n = [(n[0] / l) as f32, (n[1] / l) as f32, (n[2] / l) as f32];
        let base = self.positions.len() as u32;
        for (p, t) in q.iter().zip(uv) {
            self.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            self.uvs.push([t[0] as f32, t[1] as f32]);
            self.normals.push(n);
        }
        self.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Wind every triangle to face up (counter-clockwise seen from above, the
    /// side a back-face-culling renderer draws: every road surface is seen
    /// from above), then average the face normals into the vertices.
    pub(super) fn finish_normals(&mut self) {
        let mut acc = vec![[0.0f64; 3]; self.positions.len()];
        for t in self.indices.chunks_exact_mut(3) {
            let p = |i: u32| {
                let v = self.positions[i as usize];
                [v[0] as f64, v[1] as f64, v[2] as f64]
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let mut n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            if n[1] < 0.0 {
                n = [-n[0], -n[1], -n[2]];
                t.swap(1, 2);
            }
            for &i in t.iter() {
                for k in 0..3 {
                    acc[i as usize][k] += n[k];
                }
            }
        }
        for (dst, n) in self.normals.iter_mut().zip(acc) {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            *dst = if l > 1e-12 {
                [(n[0] / l) as f32, (n[1] / l) as f32, (n[2] / l) as f32]
            } else {
                [0.0, 1.0, 0.0]
            };
        }
    }
}

/// What a mesh of a built road is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Carriageway,
    Shoulder,
    Kerb,
    Sidewalk,
    /// Yellow paint: the centre line.
    MarkingYellow,
    /// White paint: lane lines, edge lines.
    MarkingWhite,
}

/// The meshes of one built road, one per surface.
#[derive(Clone, Debug, Default)]
pub struct RoadMeshes {
    pub parts: Vec<(Surface, MeshData)>,
    /// Where the centre line is doubled (no passing), as station ranges.
    pub no_passing: Vec<(f64, f64)>,
}

impl RoadMeshes {
    pub(super) fn part(&mut self, surface: Surface) -> &mut MeshData {
        if let Some(i) = self.parts.iter().position(|(s, _)| *s == surface) {
            return &mut self.parts[i].1;
        }
        self.parts.push((surface, MeshData::default()));
        &mut self.parts.last_mut().expect("just pushed").1
    }
}

/// Passing sight distance a two-lane road needs at a design speed, metres
/// (AASHTO, rounded).
pub fn passing_sight_distance(design_speed: f64) -> f64 {
    let table = [
        (30.0, 200.0),
        (40.0, 270.0),
        (50.0, 345.0),
        (60.0, 410.0),
        (70.0, 485.0),
        (80.0, 540.0),
        (90.0, 615.0),
        (100.0, 670.0),
        (110.0, 730.0),
    ];
    let v = design_speed.clamp(30.0, 110.0);
    let i = table
        .windows(2)
        .position(|w| v <= w[1].0)
        .unwrap_or(table.len() - 2);
    let (a, b) = (table[i], table[i + 1]);
    a.1 + (b.1 - a.1) * (v - a.0) / (b.0 - a.0)
}

/// Station ranges where a driver at `s` cannot see `need` metres ahead in
/// both directions: over a crest (eye and object 1.08 m up) or round a curve
/// whose radius hides it behind a 5 m roadside clearance.
pub fn no_passing_zones(
    stations: &[Station],
    profile: &VerticalProfile,
    need: f64,
) -> Vec<(f64, f64)> {
    let n = stations.len();
    if n < 2 {
        return Vec::new();
    }
    let total = stations[n - 1].s;
    let r_needed = need * need / (8.0 * 5.0);
    let blocked = |s: f64| -> bool {
        for dir in [1.0f64, -1.0] {
            let end = s + dir * need;
            if end < 0.0 || end > total {
                continue; // the road ends first: nothing to pass into
            }
            // Vertical: the sight line between eye and object clears the profile.
            let (z0, z1) = (profile.elevation(s) + 1.08, profile.elevation(end) + 1.08);
            let mut t = 10.0;
            while t < need {
                let q = s + dir * t;
                if profile.elevation(q) > z0 + (z1 - z0) * (t / need) {
                    return true;
                }
                t += 10.0;
            }
            // Horizontal: any curve on the way tighter than the sight clearance allows.
            let (lo, hi) = if dir > 0.0 { (s, end) } else { (end, s) };
            if stations
                .iter()
                .any(|st| st.s >= lo && st.s <= hi && st.curvature.abs() > 1.0 / r_needed)
            {
                return true;
            }
        }
        false
    };
    let mut zones = Vec::new();
    let mut start: Option<f64> = None;
    let step = (total / 400.0).max(5.0);
    let mut s = 0.0;
    while s <= total + 1e-9 {
        match (blocked(s), start) {
            (true, None) => start = Some(s),
            (false, Some(a)) => {
                zones.push((a, s));
                start = None;
            }
            _ => {}
        }
        s += step;
    }
    if let Some(a) = start {
        zones.push((a, total));
    }
    close_short_passing_zones(zones, total, minimum_passing_zone(need))
}

/// Shortest stretch marked for passing, metres: a driver needs time to see
/// the zone, decide and complete the pass, so a shorter gap between two
/// no-passing stretches stays no-passing (about 120 m at low design speeds,
/// rising with the passing sight distance `need` to 240 m).
pub fn minimum_passing_zone(need: f64) -> f64 {
    (need / 3.0).clamp(120.0, 240.0)
}

/// `zones` (no-passing station ranges, in order, over a road `total` metres
/// long) with every passing gap shorter than `minimum` closed, including the
/// gaps before the first zone and after the last.
fn close_short_passing_zones(zones: Vec<(f64, f64)>, total: f64, minimum: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(zones.len());
    for (a, b) in zones {
        match out.last_mut() {
            Some(last) if a - last.1 < minimum => last.1 = last.1.max(b),
            None if a < minimum => out.push((0.0, b)),
            _ => out.push((a, b)),
        }
    }
    if let Some(last) = out.last_mut() {
        if total - last.1 < minimum {
            last.1 = total;
        }
    }
    out
}

/// The curvature of the curve a station belongs to: the largest curvature of
/// the same sign within `reach` metres (stations inside a spiral get the
/// arc's, so superelevation ramps over the spiral toward the arc's value).
fn full_curvature(stations: &[Station], i: usize, reach: f64) -> f64 {
    let k = stations[i].curvature;
    if k.abs() < 1e-9 {
        return 0.0;
    }
    let s = stations[i].s;
    stations
        .iter()
        .filter(|st| (st.s - s).abs() <= reach && st.curvature.signum() == k.signum())
        .map(|st| st.curvature)
        .fold(k, |a, b| if b.abs() > a.abs() { b } else { a })
}

/// The cross-section at every station.
pub fn sections(stations: &[Station], standard: &RoadStandard) -> Vec<CrossSection> {
    (0..stations.len())
        .map(|i| {
            let k_full = full_curvature(stations, i, 120.0);
            CrossSection::formation(
                standard,
                cross_slope(standard, stations[i].curvature, k_full),
            )
        })
        .collect()
}

fn world(st: &Station, profile: &VerticalProfile, offset: f64, elevation: f64) -> [f64; 3] {
    let p = st.pos.offset(st.heading, inside_limit(st.curvature, offset));
    [p.x, profile.elevation(st.s) + elevation, p.z]
}

/// Fraction of a curve's radius the inside of a road may reach toward its
/// centre: past the centre the inside edge would turn back on itself.
const INSIDE_REACH: f64 = 0.9;

/// `offset` across a station of signed curvature `k`, held short of the
/// curve's centre on the inside (positive curvature turns right, toward
/// positive offsets), so a curve tighter than the road is wide never folds
/// its inside edge back over itself.
pub fn inside_limit(k: f64, offset: f64) -> f64 {
    if k * offset > 0.0 {
        let limit = INSIDE_REACH / k.abs();
        offset.signum() * offset.abs().min(limit)
    } else {
        offset
    }
}

/// How a road's markings meet its ends. A road that ends at a junction stops
/// its lines before the crosswalk and stop bar there, and holds the centre
/// line solid on the approach (no passing near an intersection).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoadEnds {
    /// Unpainted length at the start and at the end, metres.
    pub bare_start: f64,
    pub bare_end: f64,
    /// Length after the bare start, and before the bare end, over which the
    /// centre line is solid, metres.
    pub solid_start: f64,
    pub solid_end: f64,
}

/// Lay the meshes of a road: surfaces from the cross-sections at every
/// station, markings from the standard. `lift` raises everything above the
/// carved ground so the terrain never shows through (a few centimetres).
pub fn build_road(
    stations: &[Station],
    profile: &VerticalProfile,
    standard: &RoadStandard,
    lift: f64,
) -> RoadMeshes {
    build_road_with(stations, profile, standard, lift, RoadEnds::default())
}

/// [`build_road`] with its markings fitted to the junctions at its ends.
pub fn build_road_with(
    stations: &[Station],
    profile: &VerticalProfile,
    standard: &RoadStandard,
    lift: f64,
    ends: RoadEnds,
) -> RoadMeshes {
    build_road_zoned(stations, profile, standard, lift, ends, &RoadZones::default())
}

/// Station ranges along a road that change how it is drawn, each `(from,
/// to)` metres along it, in any order:
/// - `no_passing`: marked no-passing, besides what its sight lines give (a
///   tunnel, a junction's approach);
/// - `bare`: surfaced but left unpainted (a junction's crosswalk and stop
///   bar are painted there by the junction);
/// - `gaps`: neither surfaced nor painted (a junction's box covers it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadZones {
    pub no_passing: Vec<(f64, f64)>,
    pub bare: Vec<(f64, f64)>,
    pub gaps: Vec<(f64, f64)>,
}

fn in_ranges(ranges: &[(f64, f64)], s: f64) -> bool {
    ranges.iter().any(|&(a, b)| s >= a.min(b) && s <= a.max(b))
}

/// The station and cross-section at `s` metres along, interpolated between
/// the stations either side (heading the short way round).
fn pose_at(stations: &[Station], xs: &[CrossSection], s: f64) -> (Station, CrossSection) {
    let n = stations.len();
    let i = stations.partition_point(|st| st.s < s).clamp(1, n - 1);
    let (a, b) = (&stations[i - 1], &stations[i]);
    let t = ((s - a.s) / (b.s - a.s).max(1e-9)).clamp(0.0, 1.0);
    let mut dh = b.heading - a.heading;
    while dh > std::f64::consts::PI {
        dh -= 2.0 * std::f64::consts::PI;
    }
    while dh < -std::f64::consts::PI {
        dh += 2.0 * std::f64::consts::PI;
    }
    let st = Station {
        s,
        pos: P2::new(a.pos.x + (b.pos.x - a.pos.x) * t, a.pos.z + (b.pos.z - a.pos.z) * t),
        heading: a.heading + dh * t,
        curvature: a.curvature + (b.curvature - a.curvature) * t,
    };
    let mix = |p: &[SectionPoint], q: &[SectionPoint]| -> Vec<SectionPoint> {
        p.iter()
            .zip(q)
            .map(|(u, v)| SectionPoint {
                offset: u.offset + (v.offset - u.offset) * t,
                elevation: u.elevation + (v.elevation - u.elevation) * t,
                zone: u.zone,
            })
            .collect()
    };
    let (xa, xb) = (&xs[i - 1], &xs[i]);
    let x = CrossSection {
        positive: mix(&xa.positive, &xb.positive),
        negative: mix(&xa.negative, &xb.negative),
        cross_slope: xa.cross_slope + (xb.cross_slope - xa.cross_slope) * t,
    };
    (st, x)
}

/// The runs `[from, to]` of `0..=total` where `keep` holds, split exactly at
/// `breaks` (which must include every place `keep` can change), each run
/// with the breakpoints inside it.
fn runs(total: f64, breaks: &[f64], keep: impl Fn(f64) -> bool) -> Vec<Vec<f64>> {
    let mut pts: Vec<f64> = breaks.iter().copied().filter(|s| *s >= 0.0 && *s <= total).collect();
    pts.push(0.0);
    pts.push(total);
    pts.sort_by(|a, b| a.total_cmp(b));
    pts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut out: Vec<Vec<f64>> = Vec::new();
    let mut cur: Vec<f64> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if keep(0.5 * (a + b)) {
            if cur.is_empty() {
                cur.push(a);
            }
            cur.push(b);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out.retain(|r| r.len() >= 2);
    out
}

/// [`build_road_with`] drawn to `zones`: the gaps left out, the bare ranges
/// unpainted, the extra no-passing ranges marked so. Every run of surface
/// and every line starts and stops exactly where its range does, and every
/// dash is drawn whole wherever it falls between stations.
pub fn build_road_zoned(
    stations: &[Station],
    profile: &VerticalProfile,
    standard: &RoadStandard,
    lift: f64,
    ends: RoadEnds,
    zones: &RoadZones,
) -> RoadMeshes {
    let mut out = RoadMeshes::default();
    if stations.len() < 2 {
        return out;
    }
    let xs = sections(stations, standard);
    let total = stations[stations.len() - 1].s;
    let mut gaps = zones.gaps.clone();
    let mut bare_ranges = zones.bare.clone();
    if ends.bare_start > 0.0 {
        bare_ranges.push((0.0, ends.bare_start));
    }
    if ends.bare_end > 0.0 {
        bare_ranges.push((total - ends.bare_end, total));
    }
    gaps.retain(|(a, b)| b > a);
    let edges = |ranges: &[(f64, f64)]| ranges.iter().flat_map(|&(a, b)| [a, b]).collect::<Vec<f64>>();
    let station_s: Vec<f64> = stations.iter().map(|st| st.s).collect();
    // Surfaces: every run outside the gaps, each formation stretch per side.
    let surface_breaks: Vec<f64> = station_s.iter().copied().chain(edges(&gaps)).collect();
    for run in runs(total, &surface_breaks, |s| !in_ranges(&gaps, s)) {
        let poses: Vec<(Station, CrossSection)> = run.iter().map(|&s| pose_at(stations, &xs, s)).collect();
        for positive in [true, false] {
            let half = |x: &CrossSection| if positive { x.positive.clone() } else { x.negative.clone() };
            let count = half(&poses[0].1).len();
            for j in 0..count.saturating_sub(1) {
                let surface = match half(&poses[0].1)[j].zone {
                    Zone::Lane | Zone::Median => Surface::Carriageway,
                    Zone::Shoulder => Surface::Shoulder,
                    Zone::Kerb => Surface::Kerb,
                    Zone::Sidewalk => Surface::Sidewalk,
                    _ => continue,
                };
                let row = |k: usize| -> Vec<([f64; 3], [f64; 2])> {
                    poses
                        .iter()
                        .map(|(st, x)| {
                            let p = half(x)[k];
                            (world(st, profile, p.offset, p.elevation + lift), [p.offset, st.s])
                        })
                        .collect()
                };
                let (a, b) = (row(j), row(j + 1));
                // Wind counter-clockwise seen from above whichever side it is.
                if positive {
                    out.part(surface).strip(&b, &a)
                } else {
                    out.part(surface).strip(&a, &b)
                }
            }
        }
    }
    // Markings on the carriageway plane, a centimetre above it.
    let need = passing_sight_distance(standard.design_speed);
    let mut no_passing = no_passing_zones(stations, profile, need);
    no_passing.extend_from_slice(&zones.no_passing);
    if ends.solid_start > 0.0 {
        no_passing.push((0.0, ends.bare_start + ends.solid_start));
    }
    if ends.solid_end > 0.0 {
        no_passing.push((total - ends.bare_end - ends.solid_end, total));
    }
    // A kerbed two-way street keeps its centre line solid throughout, as towns paint them.
    if standard.edge == EdgeTreatment::KerbAndSidewalk && !standard.one_way {
        no_passing.push((0.0, total));
    }
    out.no_passing = no_passing.clone();
    let unpainted = |s: f64| in_ranges(&gaps, s) || in_ranges(&bare_ranges, s);
    let lane_elev = |x: &CrossSection, off: f64| -> f64 {
        // Interpolate the formation elevation at an offset across the lanes.
        let side = if off >= 0.0 { &x.positive } else { &x.negative };
        for w in side.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (lo, hi) = if a.offset.abs() <= b.offset.abs() { (a, b) } else { (b, a) };
            if off.abs() >= lo.offset.abs() && off.abs() <= hi.offset.abs() {
                let t = (off.abs() - lo.offset.abs()) / (hi.offset.abs() - lo.offset.abs()).max(1e-9);
                return lo.elevation + (hi.elevation - lo.elevation) * t;
            }
        }
        side[0].elevation
    };
    // surface, offset, width, dash (on, gap), follows the passing rule
    let mut lines: Vec<(Surface, f64, f64, Option<(f64, f64)>, bool)> = Vec::new();
    let half_cw = standard.carriageway_width() / 2.0;
    // Edge lines just inside the lanes; along a parking lane, on its edge.
    let edge_in = if standard.parking { 0.06 } else { 0.25 };
    if standard.one_way {
        // Yellow along the left of travel, white along the right, white
        // dashes between the lanes.
        lines.push((Surface::MarkingWhite, half_cw - edge_in, 0.15, None, false));
        lines.push((Surface::MarkingYellow, -(half_cw - edge_in), 0.15, None, false));
        for lane in 1..standard.lanes_total() {
            let off = -half_cw + lane as f64 * standard.lane_width;
            lines.push((Surface::MarkingWhite, off, 0.12, Some((3.0, 9.0)), false));
        }
    } else {
        lines.push((Surface::MarkingWhite, half_cw - edge_in, 0.15, None, false));
        lines.push((Surface::MarkingWhite, -(half_cw - edge_in), 0.15, None, false));
        for lane in 1..standard.lanes_per_direction {
            let off = standard.median_width / 2.0 + lane as f64 * standard.lane_width;
            lines.push((Surface::MarkingWhite, off, 0.12, Some((3.0, 9.0)), false));
            lines.push((Surface::MarkingWhite, -off, 0.12, Some((3.0, 9.0)), false));
        }
        if standard.median_width <= 0.0 {
            lines.push((Surface::MarkingYellow, 0.0, 0.12, Some((3.0, 9.0)), true));
        }
    }
    let zone_breaks: Vec<f64> = station_s
        .iter()
        .copied()
        .chain(edges(&gaps))
        .chain(edges(&bare_ranges))
        .chain(edges(&no_passing))
        .collect();
    for (surface, off, width, dash, passing_rule) in lines {
        // The passing rule turns a dashed centre line into two solid lines
        // 0.12 m either side of the centre wherever passing is not allowed.
        let variants: Vec<(f64, bool)> =
            if passing_rule { vec![(-0.12, false), (0.12, false), (0.0, true)] } else { vec![(0.0, true)] };
        for (centre, dashed_ok) in variants {
            let dashes = |s: f64| dash.map_or(true, |(on, gap)| s.rem_euclid(on + gap) < on);
            let keep = |s: f64| {
                let painted = if passing_rule {
                    if dashed_ok {
                        !in_ranges(&no_passing, s) && dashes(s)
                    } else {
                        in_ranges(&no_passing, s)
                    }
                } else {
                    dashes(s)
                };
                painted && !unpainted(s)
            };
            let mut breaks = zone_breaks.clone();
            if let Some((on, gap)) = dash.filter(|_| dashed_ok) {
                let period = on + gap;
                let mut k = 0.0;
                while k * period <= total {
                    breaks.push(k * period);
                    breaks.push(k * period + on);
                    k += 1.0;
                }
            }
            for run in runs(total, &breaks, keep) {
                let poses: Vec<(Station, CrossSection)> = run.iter().map(|&s| pose_at(stations, &xs, s)).collect();
                let at = |st: &Station, x: &CrossSection, o: f64| world(st, profile, o, lane_elev(x, o) + lift + 0.012);
                let a: Vec<_> = poses.iter().map(|(st, x)| (at(st, x, off + centre - width / 2.0), [0.0, st.s])).collect();
                let b: Vec<_> = poses.iter().map(|(st, x)| (at(st, x, off + centre + width / 2.0), [1.0, st.s])).collect();
                out.part(surface).strip(&a, &b);
            }
        }
    }
    // Parking stalls: a tick across each parking lane every PARKING_STALL.
    if standard.parking {
        let mut s = PARKING_STALL;
        while s < total - PARKING_STALL / 2.0 {
            if !unpainted(s - PARKING_TICK) && !unpainted(s + PARKING_TICK) {
                for side in [-1.0, 1.0] {
                    let (o0, o1) = (side * half_cw, side * (half_cw + standard.shoulder_width * 0.85));
                    let row = |sa: f64| {
                        let (st, x) = pose_at(stations, &xs, sa);
                        let at = |o: f64| (world(&st, profile, o, lane_elev(&x, o) + lift + 0.012), [o, sa]);
                        (at(o0), at(o1))
                    };
                    let (a0, b0) = row(s - PARKING_TICK / 2.0);
                    let (a1, b1) = row(s + PARKING_TICK / 2.0);
                    out.part(Surface::MarkingWhite).strip(&[a0, b0], &[a1, b1]);
                }
            }
            s += PARKING_STALL;
        }
    }
    for (_, m) in &mut out.parts {
        m.finish_normals();
    }
    out
}

/// Length of one parking stall along the kerb, metres.
const PARKING_STALL: f64 = 6.1;
/// Width of a stall tick along the road, metres.
const PARKING_TICK: f64 = 0.1;

/// The ground a road leaves: the formation and daylight slopes along its
/// stations over the natural ground.
pub struct Corridor<'a, G: Fn(P2) -> f64> {
    stations: &'a [Station],
    profile: &'a VerticalProfile,
    sections: Vec<CrossSection>,
    standard: &'a RoadStandard,
    ground: G,
    /// Whether the daylight slopes cut at the rock slope.
    pub rock: bool,
}

impl<'a, G: Fn(P2) -> f64> Corridor<'a, G> {
    pub fn new(
        stations: &'a [Station],
        profile: &'a VerticalProfile,
        standard: &'a RoadStandard,
        ground: G,
    ) -> Self {
        let sections = sections(stations, standard);
        Corridor {
            stations,
            profile,
            sections,
            standard,
            ground,
            rock: false,
        }
    }

    /// Ground height at `p`: the formation where `p` lies across the road,
    /// the cut or fill slope out to where it meets the natural ground, the
    /// natural ground beyond. The nearest station is found by scanning, so
    /// callers sampling a large area should test a bounding box first.
    pub fn height_at(&self, p: P2) -> f64 {
        let natural = (self.ground)(p);
        // Nearest station.
        let (mut best, mut bi) = (f64::INFINITY, 0usize);
        for (i, st) in self.stations.iter().enumerate() {
            let d = (st.pos.x - p.x).powi(2) + (st.pos.z - p.z).powi(2);
            if d < best {
                best = d;
                bi = i;
            }
        }
        let st = &self.stations[bi];
        let along = (p.x - st.pos.x) * st.heading.cos() + (p.z - st.pos.z) * st.heading.sin();
        if along.abs()
            > 2.0
                * (self.stations.get(1).map_or(1.0, |s| s.s) - self.stations[0].s)
                    .abs()
                    .max(0.5)
            && (bi == 0 || bi == self.stations.len() - 1)
        {
            return natural; // beyond the road's ends
        }
        let lateral = -(p.x - st.pos.x) * st.heading.sin() + (p.z - st.pos.z) * st.heading.cos();
        section_height(
            &self.sections[bi],
            lateral,
            self.profile.elevation(st.s),
            natural,
            self.standard,
            self.rock,
        )
        .0
    }
}

/// The ground a road's cross-section leaves `lateral` metres from its
/// centreline (positive to the right of travel), where the profile stands
/// at `profile_y` over natural ground `ground`: the formation within it,
/// the cut or fill slope from its edge out to where that meets the ground,
/// the ground beyond. Returns the height and the zone it lies in (`None`
/// where the road leaves the ground alone). The terrain bake calls this
/// for every cell along a designed road.
pub fn section_height(
    section: &CrossSection,
    lateral: f64,
    profile_y: f64,
    ground: f64,
    standard: &RoadStandard,
    rock: bool,
) -> (f64, Option<Zone>) {
    let half = if lateral >= 0.0 { &section.positive } else { &section.negative };
    let edge_z = profile_y + half.last().map_or(0.0, |e| e.elevation);
    let side = Side::default_for(standard, ground > edge_z, rock);
    section_height_with(section, lateral, profile_y, ground, &side)
}

/// Fill slopes tried after the class's own when it cannot reach the ground
/// within the daylight band, as horizontal over vertical: a steep fill, then
/// a reinforced one. Past these a retaining wall holds the road.
const STEEP_FILL_SLOPES: [f64; 2] = [1.5, 1.0];

/// How far beyond the formation's edge a cut wall stands, metres: the raster
/// under a wall right at the edge would ramp up over the shoulder.
pub const WALL_CLEARANCE: f64 = 1.0;

/// What one side of a road does beyond its formation at one station.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SideKind {
    /// A slope of `run` horizontal over 1 vertical from the edge down (fill)
    /// or up (cut) to the ground.
    Slope { run: f64 },
    /// A retaining wall at the edge (plus [`WALL_CLEARANCE`] in cut) from
    /// the formation to the ground; the ground beyond is left as it is.
    Wall,
}

/// One side of a road at one station, chosen from the ground across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Side {
    pub kind: SideKind,
    /// Whether the ground stands above the formation's edge (a cut).
    pub cut: bool,
    /// Distance from the formation's edge to where the side meets the
    /// ground, metres (a wall's clearance for a wall).
    pub reach: f64,
    /// Ground height where the side meets it.
    pub foot: f64,
}

impl Side {
    /// The class's own slope with no ground read: what a road without
    /// chosen sides daylights with.
    pub fn default_for(standard: &RoadStandard, cut: bool, rock: bool) -> Side {
        let run = match (cut, rock) {
            (true, true) => standard.rock_cut_slope,
            (true, false) => standard.cut_slope,
            (false, _) => standard.fill_slope,
        };
        Side { kind: SideKind::Slope { run }, cut, reach: f64::INFINITY, foot: f64::NAN }
    }
}

/// Offsets from a formation's edge at which a side reads the ground, metres,
/// up to `reach`: close together near the edge, wider apart beyond.
fn side_samples(reach: f64) -> impl Iterator<Item = f64> {
    [0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 15.0, 18.0, 22.0, 26.0, 30.0, 35.0, 40.0, 50.0, 60.0]
        .into_iter()
        .take_while(move |d| *d <= reach + 1e-9)
}

/// The side of a road whose formation edge stands at `edge_z`, the ground
/// across from it read outward from the edge (`(offset, height)` pairs, as
/// [`side_samples`] spaces them, out to the daylight band): the flattest
/// slope of the class that meets the ground within them (soil then rock in cut; the class's, steep, then reinforced in
/// fill), or a retaining wall when none does.
pub fn choose_side(standard: &RoadStandard, edge_z: f64, ground: &[(f64, f64)]) -> Side {
    let Some(&(_, first)) = ground.first() else {
        return Side::default_for(standard, false, false);
    };
    let cut = first > edge_z;
    let runs: Vec<f64> = if cut {
        vec![standard.cut_slope, standard.rock_cut_slope]
    } else {
        std::iter::once(standard.fill_slope).chain(STEEP_FILL_SLOPES).collect()
    };
    for run in runs {
        let run = run.max(1e-3);
        // Where the slope line crosses the ground between two samples.
        let mut prev = (0.0, edge_z);
        for &(d, g) in ground {
            let line = if cut { edge_z + d / run } else { edge_z - d / run };
            let met = if cut { line >= g } else { line <= g };
            if met {
                let (d0, g0) = prev;
                let line0 = if cut { edge_z + d0 / run } else { edge_z - d0 / run };
                let (a, b) = (line0 - g0, line - g);
                let t = if (a - b).abs() > 1e-9 { (a / (a - b)).clamp(0.0, 1.0) } else { 1.0 };
                let reach = d0 + (d - d0) * t;
                return Side { kind: SideKind::Slope { run }, cut, reach, foot: g0 + (g - g0) * t };
            }
            prev = (d, g);
        }
    }
    let wall_at = if cut { WALL_CLEARANCE } else { 0.0 };
    let foot = ground.iter().find(|(d, _)| *d >= wall_at).map_or(first, |(_, g)| *g);
    Side { kind: SideKind::Wall, cut, reach: wall_at, foot }
}

/// [`section_height`] with the side chosen for this station: its slope, or
/// a wall (the formation's level out to the wall in cut, the ground left as
/// it is beyond).
pub fn section_height_with(
    section: &CrossSection,
    lateral: f64,
    profile_y: f64,
    ground: f64,
    side: &Side,
) -> (f64, Option<Zone>) {
    let half = if lateral >= 0.0 { &section.positive } else { &section.negative };
    let Some(edge) = half.last() else {
        return (ground, None);
    };
    let edge_offset = edge.offset.abs();
    let a = lateral.abs();
    if a <= edge_offset {
        return formation_height(half, a, profile_y);
    }
    let edge_z = profile_y + edge.elevation;
    let d = a - edge_offset;
    match side.kind {
        SideKind::Wall if side.cut && d <= side.reach => (edge_z, Some(Zone::Ditch)),
        SideKind::Wall => (ground, None),
        SideKind::Slope { run } => {
            let run = run.max(1e-3);
            if ground > edge_z {
                let h = ground.min(edge_z + d / run);
                (h, (h < ground).then_some(Zone::CutSlope))
            } else {
                let h = ground.max(edge_z - d / run);
                (h, (h > ground).then_some(Zone::FillSlope))
            }
        }
    }
}

/// Height and zone `a` metres out on one half of a formation.
fn formation_height(half: &[SectionPoint], a: f64, profile_y: f64) -> (f64, Option<Zone>) {
    for w in half.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if a >= lo.offset.abs() && a <= hi.offset.abs() {
            let t = (a - lo.offset.abs()) / (hi.offset.abs() - lo.offset.abs()).max(1e-9);
            return (profile_y + lo.elevation + (hi.elevation - lo.elevation) * t, Some(lo.zone));
        }
    }
    (profile_y + half[0].elevation, Some(half[0].zone))
}

/// How far out along its slope a side's wedge runs, metres: past the cell
/// the terrain raster cannot resolve at the formation's edge.
const WEDGE_REACH: f64 = 4.0;
/// How far below its slope a wedge's outer edge tucks into the ground.
/// Stations either side a wedge's reach is averaged over, and the stations
/// over which a run of wedges tapers in and out.
const WEDGE_SMOOTH: usize = 5;
const WEDGE_TAPER: usize = 8;
const WEDGE_TUCK: f64 = 0.8;
/// Height of a fill wall's parapet above the formation's edge.
const PARAPET: f64 = 0.8;
/// How far a wall runs past the ground at its foot (into it in fill, above
/// it in cut), metres.
const WALL_OVERLAP: f64 = 0.5;

/// The meshes beyond a designed road's formation: earth wedges down the
/// first metres of every slope, closing the step the terrain raster leaves
/// at the formation's edge, and the retaining walls.
#[derive(Clone, Debug, Default)]
pub struct SideMeshes {
    pub wedges: MeshData,
    pub walls: MeshData,
}

/// Build the side meshes of `design`, its formation lifted `lift` as its
/// surfaces are. A wedge runs from the formation's edge along the side's
/// slope for [`WEDGE_REACH`] (or to where it meets the ground, if sooner),
/// then tucks [`WEDGE_TUCK`] down into the ground. A wall is two-faced: from
/// a parapet above the edge down past the ground in fill, from below the
/// formation up past the ground in cut, [`WALL_CLEARANCE`] out.
pub fn build_sides(design: &DesignedRoad, lift: f64) -> SideMeshes {
    build_sides_zoned(design, lift, &RoadZones::default())
}

/// [`build_sides`] with no wedge, slope or wall where a junction's `gaps`
/// cover the road: the junction's box stands there, so the earth must not
/// run through it.
pub fn build_sides_zoned(design: &DesignedRoad, lift: f64, zones: &RoadZones) -> SideMeshes {
    let mut out = SideMeshes::default();
    let n = design.stations.len().min(design.sides.len()).min(design.sections.len());
    // One row of points per station and side; a strip joins two rows of the same kind.
    for side in 0..2 {
        let sign = if side == 0 { 1.0 } else { -1.0 };
        // Each station's slope (edge offset, edge height, its drop per metre
        // out, and how far the wedge reaches), smoothed into rows below.
        let mut slopes: Vec<Option<(f64, f64, f64, f64)>> = Vec::with_capacity(n);
        let mut wall_rows: Vec<Option<([f64; 3], [f64; 3], f64)>> = Vec::with_capacity(n);
        for i in 0..n {
            let st = &design.stations[i];
            let x = &design.sections[i];
            let half = if side == 0 { &x.positive } else { &x.negative };
            let edge = half.last().filter(|_| design.structure_at(i).is_none() && !in_ranges(&zones.gaps, st.s));
            let Some(edge) = edge else {
                // A bridge or tunnel carries the road here: no slope, no wall.
                slopes.push(None);
                wall_rows.push(None);
                continue;
            };
            let s = design.sides[i][side];
            let y = design.profile.elevation(st.s);
            let edge_z = y + edge.elevation;
            match s.kind {
                SideKind::Slope { run } => {
                    let d = s.reach.min(WEDGE_REACH).max(0.0);
                    let per_metre = if s.cut { 1.0 / run.max(1e-3) } else { -1.0 / run.max(1e-3) };
                    slopes.push(Some((edge.offset, edge_z, per_metre, d)));
                    wall_rows.push(None);
                }
                SideKind::Wall => {
                    let off = edge.offset + sign * s.reach;
                    let (top, bottom) = if s.cut {
                        (s.foot.max(edge_z) + WALL_OVERLAP, edge_z - WALL_OVERLAP)
                    } else {
                        (edge_z + lift + PARAPET, s.foot.min(edge_z) - WALL_OVERLAP)
                    };
                    let p_top = world(st, &design.profile, off, top - y);
                    let p_bottom = world(st, &design.profile, off, bottom - y);
                    wall_rows.push(Some((p_top, p_bottom, st.s)));
                    slopes.push(None);
                }
            }
        }
        // The wedges' reach, averaged over neighbouring stations of the same
        // run and tapered to nothing at a run's ends, so their outer edge is
        // a smooth line rather than a saw.
        let mut wedge_rows: Vec<Option<[([f64; 3], [f64; 2]); 3]>> = vec![None; n];
        let mut i = 0;
        while i < n {
            let start = i;
            while i < n && slopes[i].is_some() {
                i += 1;
            }
            let run: Vec<(f64, f64, f64, f64)> = slopes[start..i].iter().map(|v| v.expect("in the run")).collect();
            let len = run.len();
            for (k, &(e, edge_z, per_metre, _)) in run.iter().enumerate() {
                let (a, b) = (k.saturating_sub(WEDGE_SMOOTH), (k + WEDGE_SMOOTH).min(len.saturating_sub(1)));
                let mean = run[a..=b].iter().map(|r| r.3).sum::<f64>() / (b - a + 1) as f64;
                let ends = (k.min(len - 1 - k) as f64 / WEDGE_TAPER as f64).min(1.0);
                let d = mean * ends;
                let st = &design.stations[start + k];
                let y = design.profile.elevation(st.s);
                let at = |off: f64, z: f64| world(st, &design.profile, off, z - y);
                let out_off = e + sign * d;
                let drop = per_metre * d;
                wedge_rows[start + k] = Some([
                    (at(e, edge_z + lift), [0.0, st.s]),
                    (at(out_off, edge_z + drop), [d, st.s]),
                    (at(out_off, edge_z + drop - WEDGE_TUCK * ends), [d + WEDGE_TUCK, st.s]),
                ]);
            }
            i += 1;
        }
        // Wedges: two strips per run (slope, then the tuck), wound to face up and out.
        let mut i = 0;
        while i < n {
            let start = i;
            while i < n && wedge_rows[i].is_some() {
                i += 1;
            }
            if i - start >= 2 {
                let rows: Vec<[([f64; 3], [f64; 2]); 3]> = wedge_rows[start..i].iter().map(|r| r.unwrap()).collect();
                for (a, b) in [(0usize, 1usize), (1, 2)] {
                    let la: Vec<_> = rows.iter().map(|r| r[a]).collect();
                    let lb: Vec<_> = rows.iter().map(|r| r[b]).collect();
                    if side == 0 { out.wedges.strip(&lb, &la) } else { out.wedges.strip(&la, &lb) }
                }
            }
            i += 1;
        }
        // Walls: a quad per pair of neighbouring wall rows, both faces.
        for w in wall_rows.windows(2) {
            let (Some((t0, b0, s0)), Some((t1, b1, s1))) = (w[0], w[1]) else { continue };
            let h0 = t0[1] - b0[1];
            let h1 = t1[1] - b1[1];
            let uv = [[s0, 0.0], [s1, 0.0], [s1, h1], [s0, h0]];
            out.walls.quad([b0, b1, t1, t0], uv);
            out.walls.quad([b1, b0, t0, t1], [uv[1], uv[0], uv[3], uv[2]]);
        }
    }
    out.wedges.finish_normals();
    out
}

/// What carries a stretch of road where the ground cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureKind {
    /// Fill deeper than the class allows: a deck on piers over the ground.
    Bridge,
    /// Cut deeper than the class allows: a bore under the ground.
    Tunnel,
}

/// A bridge or tunnel over stations `first..=last` (`s0` to `s1` metres
/// along the road).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Structure {
    pub kind: StructureKind,
    pub first: usize,
    pub last: usize,
    pub s0: f64,
    pub s1: f64,
}

/// Fraction of the class's deepest cut (fill) at which a tunnel's portals
/// (a bridge's abutments) stand: the approach cutting (embankment) runs that
/// deep before the structure takes over.
const PORTAL_DEPTH_FRACTION: f64 = 0.6;
/// Least ground over a tunnel's crown at its portals, metres: the bore
/// starts where the hill can close over its lining.
const MIN_PORTAL_COVER: f64 = 3.5;
/// Shortest tunnel and bridge, metres: shorter ones stay a cut or a fill.
const MIN_TUNNEL: f64 = 40.0;
const MIN_BRIDGE: f64 = 20.0;

/// The bridges and tunnels of a road whose `profile` runs over `ground`
/// (read under each of `stations`): wherever the fill (cut) passes the
/// class's `max_fill` (`max_cut`), widened to where it falls back to
/// [`PORTAL_DEPTH_FRACTION`] of it, neighbours of one kind merged and spans
/// under the minimum length dropped.
pub fn find_structures(
    stations: &[Station],
    profile: &VerticalProfile,
    ground: &[f64],
    standard: &RoadStandard,
) -> Vec<Structure> {
    let n = stations.len().min(ground.len());
    let depth: Vec<f64> = (0..n).map(|i| ground[i] - profile.elevation(stations[i].s)).collect();
    let mut out: Vec<Structure> = Vec::new();
    for (kind, limit, min_len) in [
        (StructureKind::Tunnel, standard.max_cut, MIN_TUNNEL),
        (StructureKind::Bridge, standard.max_fill, MIN_BRIDGE),
    ] {
        if !(limit > 0.0) {
            continue;
        }
        // Depth into the ground for a tunnel, height over it for a bridge.
        let over = |i: usize| if kind == StructureKind::Tunnel { depth[i] } else { -depth[i] };
        let mut i = 0;
        let mut spans: Vec<(usize, usize)> = Vec::new();
        while i < n {
            if over(i) <= limit {
                i += 1;
                continue;
            }
            let (mut a, mut b) = (i, i);
            while b + 1 < n && over(b + 1) > limit {
                b += 1;
            }
            i = b + 1;
            let portal = if kind == StructureKind::Tunnel {
                let crown = TUNNEL_WALL + structure_half_width(standard);
                (limit * PORTAL_DEPTH_FRACTION).max(crown + MIN_PORTAL_COVER).min(limit)
            } else {
                limit * PORTAL_DEPTH_FRACTION
            };
            while a > 0 && over(a - 1) > portal {
                a -= 1;
            }
            while b + 1 < n && over(b + 1) > portal {
                b += 1;
            }
            match spans.last_mut() {
                Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
                _ => spans.push((a, b)),
            }
        }
        for (a, b) in spans {
            let (s0, s1) = (stations[a].s, stations[b].s);
            if s1 - s0 >= min_len {
                out.push(Structure { kind, first: a, last: b, s0, s1 });
            }
        }
    }
    out.sort_by(|x, y| x.s0.total_cmp(&y.s0));
    out
}

/// Depth of a bridge deck under the formation, metres.
const DECK_DEPTH: f64 = 1.6;
/// Height of a bridge parapet over the formation's edge.
const BRIDGE_PARAPET: f64 = 1.1;
/// Thickness of a bridge parapet: its inner face stands at the formation's
/// edge, its outer face (flush with the deck's fascia) this far out.
const BRIDGE_PARAPET_WIDTH: f64 = 0.45;
/// Distance between bridge piers, and the clear length kept from each end.
const PIER_SPACING: f64 = 30.0;
const PIER_END_CLEAR: f64 = 12.0;
/// Thickness of a pier along the road; its width across is this fraction of
/// the formation.
const PIER_THICKNESS: f64 = 1.8;
const PIER_WIDTH_FRACTION: f64 = 0.55;
/// A tunnel's walls rise this high over the formation's edge before its
/// arch springs; the arch spans the formation.
const TUNNEL_WALL: f64 = 4.0;
/// Segments of a tunnel arch.
const ARCH_SEGMENTS: usize = 10;
/// A portal's collar stands this far out around the bore.
const PORTAL_MARGIN: f64 = 4.0;
/// How far a portal's collar reaches below the road, metres.
const PORTAL_FOOTING: f64 = 1.0;
/// A portal's headwall: how far out of the bore its face stands (past the
/// raster cell the ground ramps across at the portal, so the hill is behind
/// it), how thick it is, how far over the ground line it rises, and the
/// most it reaches either side beyond the structure (its wings), metres.
const HEADWALL_OUT: f64 = 2.8;
const HEADWALL_THICKNESS: f64 = 1.0;
/// How far into the bore a portal's cap runs from the portal, metres: past
/// the raster cell (2.5 m) the ground ramps across there.
const PORTAL_CAP_IN: f64 = 3.0;
/// How far under the road's surface a headwall's sill stops, metres: the
/// wall runs on below the road across the opening.
const PORTAL_SILL_BELOW: f64 = 0.02;
/// A tunnel portal's approach cut: over this many metres out from the portal
/// the cut slope steepens from the class's to [`PORTAL_CUT_RUN`] (horizontal
/// over vertical), a rock face at the portal, as portals are cut.
const PORTAL_APPROACH: f64 = 30.0;
const PORTAL_CUT_RUN: f64 = 0.4;
/// How far a headwall's ends run on into the cut slopes beside it, metres,
/// so no gap opens between the wall and the ground.
const HEADWALL_WING_BURY: f64 = 1.5;
/// How far a portal's collar stands proud of its headwall, metres.
const COLLAR_PROUD: f64 = 0.4;
const HEADWALL_ABOVE_GROUND: f64 = 0.8;
const HEADWALL_MAX_WING: f64 = 30.0;

/// The concrete of a road's bridges and tunnels: decks, parapets, piers,
/// abutments and portals in `concrete`, the inside of every bore in
/// `lining`. Every face is two-sided.
#[derive(Clone, Debug, Default)]
pub struct StructureMeshes {
    pub concrete: MeshData,
    pub lining: MeshData,
}

impl MeshData {
    /// [`MeshData::quad`] facing both ways.
    pub(super) fn quad2(&mut self, q: [[f64; 3]; 4]) {
        let len = |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let (w, h) = (len(q[0], q[1]), len(q[1], q[2]));
        self.quad(q, [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]);
        self.quad([q[1], q[0], q[3], q[2]], [[w, 0.0], [0.0, 0.0], [0.0, h], [w, h]]);
    }
}

/// How far a bridge's deck or a tunnel's floor reaches either side of the
/// centreline: the lanes and shoulders, and a kerbed road's sidewalks. A
/// ditch has no place on a structure: its parapets and walls stand at the
/// shoulder's edge.
pub fn structure_half_width(standard: &RoadStandard) -> f64 {
    match standard.edge {
        EdgeTreatment::KerbAndSidewalk => standard.formation_half_width(),
        _ => standard.carriageway_width() / 2.0 + standard.shoulder_width,
    }
}

/// Build the bridges and tunnels of `design`, its formation lifted `lift`.
pub fn build_structures(design: &DesignedRoad, lift: f64) -> StructureMeshes {
    let mut out = StructureMeshes::default();
    let reach = structure_half_width(&design.standard);
    let edges = |i: usize| -> (f64, f64, f64, f64) {
        let x = &design.sections[i];
        (reach, x.elevation_at(reach), -reach, x.elevation_at(-reach))
    };
    let at = |i: usize, off: f64, z_rel: f64| world(&design.stations[i], &design.profile, off, z_rel);
    for s in &design.structures {
        let (a, b) = (s.first, s.last.min(design.stations.len() - 1));
        match s.kind {
            StructureKind::Bridge => {
                let w = BRIDGE_PARAPET_WIDTH;
                for i in a..b {
                    let (po, pe, no, ne) = edges(i);
                    let (po1, pe1, no1, ne1) = edges(i + 1);
                    // The underside, out to the parapets' outer faces.
                    out.concrete.quad2([at(i, no - w, ne - DECK_DEPTH), at(i, po + w, pe - DECK_DEPTH), at(i + 1, po1 + w, pe1 - DECK_DEPTH), at(i + 1, no1 - w, ne1 - DECK_DEPTH)]);
                    // Each parapet is solid: the fascia and its outer face in
                    // one plane from under the deck to its top, its top, and
                    // its inner face down to the road's edge.
                    for (o, e, o1, e1, out_by) in [(po, pe, po1, pe1, w), (no, ne, no1, ne1, -w)] {
                        let (oo, oo1) = (o + out_by, o1 + out_by);
                        let (top, top1) = (e + lift + BRIDGE_PARAPET, e1 + lift + BRIDGE_PARAPET);
                        out.concrete.quad2([at(i, oo, e - DECK_DEPTH), at(i + 1, oo1, e1 - DECK_DEPTH), at(i + 1, oo1, top1), at(i, oo, top)]);
                        out.concrete.quad2([at(i, o, top), at(i + 1, o1, top1), at(i + 1, oo1, top1), at(i, oo, top)]);
                        out.concrete.quad2([at(i, o, e + lift), at(i + 1, o1, e1 + lift), at(i + 1, o1, top1), at(i, o, top)]);
                    }
                }
                // Piers every PIER_SPACING, clear of the ends.
                let mut next = s.s0 + PIER_END_CLEAR;
                for i in a..=b {
                    let st = &design.stations[i];
                    if st.s + 1e-9 < next || st.s > s.s1 - PIER_END_CLEAR {
                        continue;
                    }
                    next = st.s + PIER_SPACING;
                    let (po, pe, no, _) = edges(i);
                    let half_w = 0.5 * (po - no).abs() * PIER_WIDTH_FRACTION;
                    let top = pe - DECK_DEPTH;
                    let foot = design.ground.get(i).copied().unwrap_or(0.0) - design.profile.elevation(st.s) - 1.0;
                    pier(&mut out.concrete, st, &design.profile, half_w, PIER_THICKNESS / 2.0, top, foot);
                }
                // Abutments: a wall across each end down into the ground,
                // the full width of the deck and its parapets, and each
                // parapet's end closed over it.
                for i in [a, b] {
                    let (po, pe, no, ne) = edges(i);
                    let g = design.ground.get(i).copied().unwrap_or(0.0) - design.profile.elevation(design.stations[i].s);
                    out.concrete.quad2([at(i, no - w, g - 1.0), at(i, po + w, g - 1.0), at(i, po + w, pe + lift), at(i, no - w, ne + lift)]);
                    for (o, e, out_by) in [(po, pe, w), (no, ne, -w)] {
                        let top = e + lift + BRIDGE_PARAPET;
                        out.concrete.quad2([at(i, o, e + lift), at(i, o + out_by, e + lift), at(i, o + out_by, top), at(i, o, top)]);
                    }
                }
            }
            StructureKind::Tunnel => {
                let ring = |i: usize| -> Vec<[f64; 3]> {
                    let (po, pe, no, ne) = edges(i);
                    let (c, r) = (0.5 * (po + no), 0.5 * (po - no).abs());
                    let spring = pe.max(ne) + TUNNEL_WALL;
                    let mut pts = vec![at(i, po, pe), at(i, po, spring)];
                    for k in 1..ARCH_SEGMENTS {
                        let th = std::f64::consts::PI * k as f64 / ARCH_SEGMENTS as f64;
                        pts.push(at(i, c + r * th.cos(), spring + r * th.sin()));
                    }
                    pts.push(at(i, no, spring));
                    pts.push(at(i, no, ne));
                    pts
                };
                // The lining at any distance along (between stations), so it
                // can run out past the portal stations to each collar's face.
                let ring_at = |s_at: f64| -> Vec<[f64; 3]> {
                    let (st, x) = pose_at(&design.stations, &design.sections, s_at);
                    let at = |off: f64, z_rel: f64| world(&st, &design.profile, off, z_rel);
                    let (po, pe, no, ne) = (reach, x.elevation_at(reach), -reach, x.elevation_at(-reach));
                    let (c, r) = (0.5 * (po + no), 0.5 * (po - no).abs());
                    let spring = pe.max(ne) + TUNNEL_WALL;
                    let mut pts = vec![at(po, pe), at(po, spring)];
                    for k in 1..ARCH_SEGMENTS {
                        let th = std::f64::consts::PI * k as f64 / ARCH_SEGMENTS as f64;
                        pts.push(at(c + r * th.cos(), spring + r * th.sin()));
                    }
                    pts.push(at(no, spring));
                    pts.push(at(no, ne));
                    pts
                };
                let out_by = HEADWALL_OUT + COLLAR_PROUD;
                let mut rings = vec![ring_at(design.stations[a].s - out_by)];
                rings.extend((a..=b).map(ring));
                rings.push(ring_at(design.stations[b].s + out_by));
                for w in rings.windows(2) {
                    for k in 0..w[0].len() - 1 {
                        out.lining.quad2([w[0][k], w[1][k], w[1][k + 1], w[0][k + 1]]);
                    }
                }
                // Portals: an arched concrete collar round each mouth. Its
                // inner edge is the lining's own profile and its outer edge
                // the same arch PORTAL_MARGIN further out, down to footings
                // below the road. It runs from the mouth one station into
                // the bore, so it is solid: a face out of the hill, a face
                // into the bore, the rim over the top and the reveal where
                // it meets the lining.
                for (mouth, dir) in [(a, 1.0), (b, -1.0)] {
                    let s_m = design.stations[mouth].s;
                    let profile = |s_at: f64, grow: f64| -> Vec<[f64; 3]> {
                        let (st, x) = pose_at(&design.stations, &design.sections, s_at);
                        let at = |off: f64, z_rel: f64| world(&st, &design.profile, off, z_rel);
                        let (po, pe, no, ne) = (reach, x.elevation_at(reach), -reach, x.elevation_at(-reach));
                        let (c, r) = (0.5 * (po + no), 0.5 * (po - no).abs());
                        let sign = (po - no).signum();
                        let spring = pe.max(ne) + TUNNEL_WALL;
                        let foot = pe.min(ne) - PORTAL_FOOTING;
                        let (wp, wn) = (po + sign * grow, no - sign * grow);
                        let mut pts = vec![at(wp, foot), at(wp, spring)];
                        for k in 1..ARCH_SEGMENTS {
                            let th = std::f64::consts::PI * k as f64 / ARCH_SEGMENTS as f64;
                            pts.push(at(c + (r + grow) * th.cos(), spring + (r + grow) * th.sin()));
                        }
                        pts.push(at(wn, spring));
                        pts.push(at(wn, foot));
                        pts
                    };
                    let (s_front, s_back) = (s_m - dir * (HEADWALL_OUT + COLLAR_PROUD), s_m - dir * (HEADWALL_OUT - HEADWALL_THICKNESS));
                    let (front_in, front_out) = (profile(s_front, 0.0), profile(s_front, PORTAL_MARGIN));
                    let (back_in, back_out) = (profile(s_back, 0.0), profile(s_back, PORTAL_MARGIN));
                    for k in 0..front_in.len() - 1 {
                        // The face out of the hill and the face into the bore.
                        out.concrete.quad2([front_in[k], front_out[k], front_out[k + 1], front_in[k + 1]]);
                        out.concrete.quad2([back_in[k], back_out[k], back_out[k + 1], back_in[k + 1]]);
                        // The rim and the reveal.
                        out.concrete.quad2([front_out[k], back_out[k], back_out[k + 1], front_out[k + 1]]);
                        out.concrete.quad2([front_in[k], back_in[k], back_in[k + 1], front_in[k + 1]]);
                    }
                    // The collar's two ends at the footings.
                    for k in [0, front_in.len() - 1] {
                        out.concrete.quad2([front_in[k], front_out[k], back_out[k], back_in[k]]);
                    }
                }
                // Headwalls: at each mouth, a vertical wall across the
                // cutting, from footings under the road up each cut slope to
                // where it meets the ground, with the arched opening in it.
                // The hill stands behind it, so no terrain edge shows round
                // the mouth. It stands HEADWALL_OUT out of the bore, past the
                // raster cell the ground ramps across, and the collar stands
                // proud of it.
                for (mouth, inward) in [(a, 1.0), (b, -1.0)] {
                    let s_m = design.stations[mouth].s;
                    let reach = structure_half_width(&design.standard);
                    // Each face's outline, for the rim between them.
                    let mut outlines: Vec<Vec<[f64; 3]>> = Vec::new();
                    for face_s in [s_m - inward * HEADWALL_OUT, s_m - inward * (HEADWALL_OUT - HEADWALL_THICKNESS)] {
                        let (st, x) = pose_at(&design.stations, &design.sections, face_s);
                        let pe = x.elevation_at(reach);
                        let ne = x.elevation_at(-reach);
                        let spring = pe.max(ne) + TUNNEL_WALL;
                        let foot = pe.min(ne) - PORTAL_FOOTING;
                        let p = |off: f64, z: f64| world(&st, &design.profile, off, z);
                        // The opening, from the road's surface up (below it the
                        // wall runs on under the road as a sill, so nothing is
                        // seen under the road's edge into the carved bore),
                        // then where a ray from the arch's centre through each
                        // of its points meets the wall's outline.
                        let (sill_p, sill_n) = (pe - PORTAL_SILL_BELOW, ne - PORTAL_SILL_BELOW);
                        let mut inner: Vec<(f64, f64)> = vec![(reach, sill_p), (reach, spring)];
                        for k in 1..ARCH_SEGMENTS {
                            let th = std::f64::consts::PI * k as f64 / ARCH_SEGMENTS as f64;
                            inner.push((reach * th.cos(), spring + reach * th.sin()));
                        }
                        inner.push((-reach, spring));
                        inner.push((-reach, sill_n));
                        // The outline: up each cut slope from the floor's edge
                        // to where it meets the ground (HEADWALL_ABOVE_GROUND
                        // proud of it), then straight across the top; never
                        // inside the collar.
                        let fh = design.standard.formation_half_width();
                        let need_top = spring + reach + PORTAL_MARGIN + 0.5;
                        let need_w = reach + PORTAL_MARGIN + 0.5;
                        let side = |k: usize, sign: f64| -> ((f64, f64), (f64, f64)) {
                            let sd = design.sides.get(mouth).map(|v| v[k]).unwrap_or_else(|| Side::default_for(&design.standard, true, false));
                            let floor = x.elevation_at(sign * fh);
                            let top_z = (sd.foot - design.profile.elevation(face_s) + HEADWALL_ABOVE_GROUND).max(need_top);
                            let w_top = (fh + sd.reach + HEADWALL_WING_BURY).clamp(need_w, reach + HEADWALL_MAX_WING);
                            ((sign * (fh + HEADWALL_WING_BURY).max(need_w), floor - PORTAL_FOOTING), (sign * w_top, top_z))
                        };
                        let ((pb, _), (pt, pzt)) = side(0, 1.0);
                        let ((nb, _), (nt, nzt)) = side(1, -1.0);
                        let poly = [(pb, foot), (pt, pzt), (nt, nzt), (nb, foot)];
                        outlines.push(poly.iter().map(|&(o, z)| p(o, z)).collect());
                        // Where a ray from the arch's centre (horizontal below
                        // the spring line) through each opening point meets it.
                        let cast = |o: f64, z: f64| -> (f64, f64) {
                            let (ox, oz, dx, dz) = if z <= spring + 1e-9 { (0.0, z, o.signum(), 0.0) } else { (0.0, spring, o, z - spring) };
                            let mut best = (f64::INFINITY, (o, z));
                            for e in 0..poly.len() {
                                let (p, q) = (poly[e], poly[(e + 1) % poly.len()]);
                                let (ex, ez) = (q.0 - p.0, q.1 - p.1);
                                let den = dx * ez - dz * ex;
                                if den.abs() < 1e-12 { continue; }
                                let t = ((p.0 - ox) * ez - (p.1 - oz) * ex) / den;
                                let u = ((p.0 - ox) * dz - (p.1 - oz) * dx) / den;
                                if t > 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&u) && t < best.0 {
                                    best = (t, (ox + dx * t, oz + dz * t));
                                }
                            }
                            best.1
                        };
                        let outer: Vec<(f64, f64)> = inner.iter().map(|&(o, z)| cast(o, z)).collect();
                        // Walk the opening and the outline together, putting
                        // in each outline corner the rays pass.
                        let mut ring_in = Vec::new();
                        let mut ring_out = Vec::new();
                        let edge_of = |pt: (f64, f64)| -> usize {
                            (0..poly.len())
                                .min_by(|&e1, &e2| {
                                    let dist = |e: usize| {
                                        let (p, q) = (poly[e], poly[(e + 1) % poly.len()]);
                                        let (ex, ez) = (q.0 - p.0, q.1 - p.1);
                                        let t = (((pt.0 - p.0) * ex + (pt.1 - p.1) * ez) / (ex * ex + ez * ez).max(1e-12)).clamp(0.0, 1.0);
                                        ((p.0 + ex * t - pt.0).powi(2) + (p.1 + ez * t - pt.1).powi(2)).sqrt()
                                    };
                                    dist(e1).total_cmp(&dist(e2))
                                })
                                .unwrap_or(0)
                        };
                        for k in 0..inner.len() {
                            if k > 0 {
                                let (e0, e1) = (edge_of(outer[k - 1]), edge_of(outer[k]));
                                // Corners strictly between the two edges, going round (0,1,2,3 = right side, top, left side, bottom).
                                let mut e = e0;
                                while e != e1 {
                                    let c = poly[(e + 1) % poly.len()];
                                    ring_in.push(p(inner[k - 1].0, inner[k - 1].1));
                                    ring_out.push(p(c.0, c.1));
                                    e = (e + 1) % poly.len();
                                }
                            }
                            ring_in.push(p(inner[k].0, inner[k].1));
                            ring_out.push(p(outer[k].0, outer[k].1));
                        }
                        for k in 0..ring_in.len() - 1 {
                            out.concrete.quad2([ring_in[k], ring_out[k], ring_out[k + 1], ring_in[k + 1]]);
                        }
                        // The sill: the wall's whole width from its footing up
                        // to just under the road.
                        let (o0, o1) = (outer[0], outer[outer.len() - 1]);
                        out.concrete.quad2([p(pb, foot), p(o0.0, o0.1), p(o1.0, o1.1), p(nb, foot)]);
                    }
                    // The rim between the two faces: the top and both slopes (the
                    // bottom edge is under the road).
                    if let [front, back] = &outlines[..] {
                        for e in 0..3 {
                            out.concrete.quad2([front[e], front[e + 1], back[e + 1], back[e]]);
                        }
                    }
                    // The cap: behind the wall the ground ramps down across a
                    // raster cell, through the bore's carve, which would leave
                    // a pit open to the sky behind the wall's top. A slab at
                    // the carve's top closes it, the carve's width, from the
                    // wall's back face to past that cell.
                    let (s0, s1) = (s_m - inward * (HEADWALL_OUT - HEADWALL_THICKNESS), s_m + inward * PORTAL_CAP_IN);
                    let top = design.profile.elevation(s0).max(design.profile.elevation(s1)) + TUNNEL_WALL + reach + CARVE_MARGIN + 0.05;
                    let w = reach + CARVE_MARGIN;
                    let corner = |s_at: f64, off: f64| {
                        let (st, _) = pose_at(&design.stations, &design.sections, s_at);
                        world(&st, &design.profile, off, top - design.profile.elevation(s_at))
                    };
                    out.concrete.quad2([corner(s0, w), corner(s1, w), corner(s1, -w), corner(s0, -w)]);
                }
            }
        }
    }
    out
}

/// Longest carved box along a bore, and how far a carve reaches out past
/// each portal (over the raster cell the portal face ramps across).
const CARVE_STEP: f64 = 6.0;
/// The carve starts at the headwall's face, so it opens the whole ramp of
/// ground behind the wall. (Its first box's overlap and the lean of its
/// upright axis on a grade reach about a metre further out, where only the
/// cutting's open floor under the road lies.)
const CARVE_OUTSIDE: f64 = HEADWALL_OUT;
/// Room left around the lining by the carve, metres.
const CARVE_MARGIN: f64 = 2.5;
/// How far neighbouring boxes overlap along the bore, so a curve's chords
/// leave no ground standing at their joints.
const CARVE_OVERLAP: f64 = 1.0;
/// How long the plumb box at each end of a bore is, metres: longer than the
/// next box's overlap and lean reach, so nothing leaning passes the headwall.
const CARVE_PLUMB: f64 = 3.0;

/// A box a tunnel's bore carves out of the terrain: its `center`, its unit
/// axes (`across` the road, toward the left of travel, `up`, and `along` the
/// road: a right-handed frame, so the three make a rotation as they stand)
/// and its full `size` along them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoreCarve {
    pub center: [f64; 3],
    pub across: [f64; 3],
    pub up: [f64; 3],
    pub along: [f64; 3],
    pub size: [f64; 3],
}

/// The boxes that open `design`'s tunnels through the terrain, end to end
/// from [`CARVE_OUTSIDE`] outside one portal to the same outside the other.
/// Each box runs from one station (or a point between, when stations stand
/// more than [`CARVE_STEP`] apart) to the next, overlapping its neighbour by
/// [`CARVE_OVERLAP`], so no ground, and no end face of the carve, stands
/// inside the bore.
pub fn tunnel_carves(design: &DesignedRoad) -> Vec<BoreCarve> {
    let mut out = Vec::new();
    let n = design.stations.len().min(design.ground.len());
    if n < 2 {
        return out;
    }
    let stations = &design.stations[..n];
    let half = structure_half_width(&design.standard);
    let height = TUNNEL_WALL + half + CARVE_MARGIN + 0.3;
    let width = 2.0 * (half + CARVE_MARGIN);
    // The centre line's formation point and the ground over it at `s`,
    // interpolated between the stations either side.
    let at = |s: f64| -> ([f64; 3], f64) {
        let i = stations.partition_point(|st| st.s < s).clamp(1, n - 1);
        let (a, b) = (&stations[i - 1], &stations[i]);
        let t = ((s - a.s) / (b.s - a.s).max(1e-9)).clamp(0.0, 1.0);
        let x = a.pos.x + (b.pos.x - a.pos.x) * t;
        let z = a.pos.z + (b.pos.z - a.pos.z) * t;
        let ground = design.ground[i - 1] + (design.ground[i] - design.ground[i - 1]) * t;
        ([x, design.profile.elevation(s), z], ground)
    };
    let (first, last) = (stations[0].s, stations[n - 1].s);
    for t in design.structures.iter().filter(|s| s.kind == StructureKind::Tunnel) {
        let (start, end) = ((t.s0 - CARVE_OUTSIDE).max(first), (t.s1 + CARVE_OUTSIDE).min(last));
        if !(end > start) {
            continue;
        }
        // The two ends, a plumb box's length in from each, every station
        // between those, and points between stations that stand further
        // apart than a box is long.
        let plumb_len = CARVE_PLUMB.min((end - start) / 2.0);
        let (inner_a, inner_b) = (start + plumb_len, end - plumb_len);
        let mut knots = vec![start, inner_a];
        for st in stations.iter().filter(|st| st.s > inner_a && st.s < inner_b) {
            knots.push(st.s);
        }
        if inner_b > inner_a {
            knots.push(inner_b);
        }
        knots.push(end);
        let mut breaks = vec![start];
        for w in knots.windows(2) {
            let pieces = ((w[1] - w[0]) / CARVE_STEP).ceil().max(1.0) as usize;
            for k in 1..=pieces {
                breaks.push(w[0] + (w[1] - w[0]) * k as f64 / pieces as f64);
            }
        }
        let last_w = breaks.len().saturating_sub(2);
        for (wi, w) in breaks.windows(2).enumerate() {
            let ((pa, ga), (pb, gb)) = (at(w[0]), at(w[1]));
            let _ = (ga, gb);
            let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if !(len > 1e-6) {
                continue;
            }
            // The two end boxes stand plumb, so each outer face is the
            // headwall's own vertical plane at every height (a box leaning
            // with the grade would reach in front of the wall at its foot on
            // one portal and its top on the other); tall enough to hold the
            // bore as the floor climbs across them. The rest lean with the
            // grade.
            let plumb = wi == 0 || wi == last_w;
            let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
            if plumb && !(flat > 1e-6) {
                continue;
            }
            let (along, len, rise) = if plumb { ([d[0] / flat, 0.0, d[2] / flat], flat, d[1].abs()) } else { ([d[0] / len, d[1] / len, d[2] / len], len, 0.0) };
            // Up: world up with its part along the road taken out.
            let dot = along[1];
            let up_raw = [-dot * along[0], 1.0 - dot * along[1], -dot * along[2]];
            let ul = (up_raw[0].powi(2) + up_raw[1].powi(2) + up_raw[2].powi(2)).sqrt();
            let up = [up_raw[0] / ul, up_raw[1] / ul, up_raw[2] / ul];
            // across = up x along (toward the left of travel): right-handed.
            let across = [
                up[1] * along[2] - up[2] * along[1],
                up[2] * along[0] - up[0] * along[2],
                up[0] * along[1] - up[1] * along[0],
            ];
            let box_height = height + rise;
            let mid = box_height / 2.0 - 0.3 - rise / 2.0;
            // Neighbours overlap; the outer ends of a tunnel's first and last
            // boxes stop exactly at the headwalls' faces.
            let (back, fore) = (if wi == 0 { 0.0 } else { CARVE_OVERLAP / 2.0 }, if wi == last_w { 0.0 } else { CARVE_OVERLAP / 2.0 });
            let shift = (fore - back) / 2.0;
            let center = [
                0.5 * (pa[0] + pb[0]) + up[0] * mid + along[0] * shift,
                0.5 * (pa[1] + pb[1]) + up[1] * mid + along[1] * shift,
                0.5 * (pa[2] + pb[2]) + up[2] * mid + along[2] * shift,
            ];
            out.push(BoreCarve { center, across, up, along, size: [width, box_height, len + back + fore] });
        }
    }
    out
}

/// A box pier at `st`: `half_w` either side of the centreline, `half_t`
/// along the road, from `top` down to `foot` (both relative to the profile).
fn pier(m: &mut MeshData, st: &Station, profile: &VerticalProfile, half_w: f64, half_t: f64, top: f64, foot: f64) {
    let (dx, dz) = (st.heading.cos(), st.heading.sin());
    let corner = |a: f64, w: f64, z: f64| {
        let p = P2::new(st.pos.x + dx * a, st.pos.z + dz * a).offset(st.heading, w);
        [p.x, profile.elevation(st.s) + z, p.z]
    };
    let c = [(-half_t, -half_w), (half_t, -half_w), (half_t, half_w), (-half_t, half_w)];
    for k in 0..4 {
        let (a0, w0) = c[k];
        let (a1, w1) = c[(k + 1) % 4];
        m.quad2([corner(a0, w0, foot), corner(a1, w1, foot), corner(a1, w1, top), corner(a0, w0, top)]);
    }
}

/// The surfaces and markings of `design`, lifted `lift`, its tunnels marked
/// no-passing.
pub fn build_designed(design: &DesignedRoad, lift: f64) -> RoadMeshes {
    build_designed_zoned(design, lift, &RoadZones::default())
}

/// [`build_designed`] with `zones` (a junction's gaps and bare approaches)
/// added to its tunnels' no-passing.
pub fn build_designed_zoned(design: &DesignedRoad, lift: f64, zones: &RoadZones) -> RoadMeshes {
    let mut zones = zones.clone();
    zones
        .no_passing
        .extend(design.structures.iter().filter(|s| s.kind == StructureKind::Tunnel).map(|s| (s.s0, s.s1)));
    build_road_zoned(&design.stations, &design.profile, &design.standard, lift, RoadEnds::default(), &zones)
}

/// A road designed through its points: stations along the fitted
/// alignment, the profile, the standard and a cross-section per station.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignedRoad {
    pub standard: RoadStandard,
    pub stations: Vec<Station>,
    pub profile: VerticalProfile,
    pub sections: Vec<CrossSection>,
    /// The ground under each station, as read when designing.
    pub ground: Vec<f64>,
    /// Each station's sides, positive (right of travel) then negative,
    /// chosen from the ground across it ([`choose_side`]).
    pub sides: Vec<[Side; 2]>,
    /// Its bridges and tunnels, in order along it ([`find_structures`]).
    pub structures: Vec<Structure>,
}

impl DesignedRoad {
    /// The structure carrying the road at station index `i`, if any.
    pub fn structure_at(&self, i: usize) -> Option<&Structure> {
        self.structures.iter().find(|s| i >= s.first && i <= s.last)
    }
}

/// Design a road through `points` (each its place and height, in path
/// order) to `spec`, its carriageway `width` metres wide (0 keeps the
/// class's), with stations `spacing` metres apart and the ground under
/// them read through `ground`. The profile comes from `spec`: graded to the
/// ground, through the points' heights, or following the ground. Each
/// station's sides are chosen from the ground read across them out to
/// `daylight` metres beyond the formation ([`choose_side`]).
pub fn design_spline(
    points: &[(P2, f64)],
    spec: RoadSpec,
    width: f64,
    spacing: f64,
    daylight: f64,
    mut ground: impl FnMut(P2) -> f64,
) -> Option<DesignedRoad> {
    if points.len() < 2 || !(spacing > 0.0) {
        return None;
    }
    let standard = spec.standard(width);
    let xz: Vec<P2> = points.iter().map(|(p, _)| *p).collect();
    let alignment = Alignment::fit_through(&xz, &standard, None)?;
    let stations = alignment.stations(spacing);
    if stations.len() < 2 {
        return None;
    }
    let ground_line: Vec<f64> = stations.iter().map(|st| ground(st.pos)).collect();
    // `auto` places its VPIs every `spacing` from 0; stations sit a little
    // off that grid (the last one short of it), so each VPI moves to its
    // station's own distance.
    let auto = |line: &[f64]| {
        let grid = VerticalProfile::auto(line, spacing, &standard, 0.3);
        let vpis = grid
            .vpis
            .iter()
            .map(|v| {
                let i = ((v.s / spacing).round() as usize).min(stations.len() - 1);
                Vpi {
                    s: stations[i].s,
                    ..*v
                }
            })
            .collect();
        VerticalProfile::from_vpis(vpis, &standard)
    };
    let mut profile = match spec.profile {
        ProfileMode::Auto => auto(&ground_line),
        ProfileMode::Explicit => {
            // Each point's height at its station: the nearest one at or after
            // the previous point's, so a road that comes back past itself
            // never takes a point from the wrong pass.
            let mut from = 0usize;
            let mut through: Vec<(f64, f64)> = Vec::with_capacity(points.len());
            for (p, y) in points {
                let i = (from..stations.len())
                    .min_by(|&a, &b| {
                        let (sa, sb) = (&stations[a].pos, &stations[b].pos);
                        let da = (sa.x - p.x).powi(2) + (sa.z - p.z).powi(2);
                        let db = (sb.x - p.x).powi(2) + (sb.z - p.z).powi(2);
                        da.total_cmp(&db)
                    })
                    .unwrap_or(stations.len() - 1);
                from = i;
                if through.last().is_some_and(|&(s, _)| (stations[i].s - s).abs() < 1e-6) {
                    continue;
                }
                through.push((stations[i].s, *y));
            }
            if let Some(first) = through.first_mut() {
                first.0 = 0.0;
            }
            if let Some(last) = through.last_mut() {
                last.0 = stations[stations.len() - 1].s;
            }
            VerticalProfile::through(&through, &standard)
        }
        ProfileMode::Ground => {
            // The ground smoothed over about 10 m, at every station.
            let half = ((5.0 / spacing).round() as usize).max(1);
            let n = ground_line.len();
            let vpis = (0..n)
                .map(|i| {
                    let (a, b) = (i.saturating_sub(half), (i + half).min(n - 1));
                    Vpi {
                        s: stations[i].s,
                        elevation: ground_line[a..=b].iter().sum::<f64>() / (b - a + 1) as f64,
                        curve_length: 0.0,
                    }
                })
                .collect();
            VerticalProfile { vpis }
        }
    };
    let mut structures = find_structures(&stations, &profile, &ground_line, &standard);
    if spec.profile == ProfileMode::Auto && structures.iter().any(|s| s.kind == StructureKind::Tunnel) {
        // Graded to the ground, a road would climb over the mountain its
        // tunnel goes through: grade it again to a ground that runs straight
        // between each tunnel's portals, then find the structures again.
        let mut through = ground_line.clone();
        for t in structures.iter().filter(|s| s.kind == StructureKind::Tunnel) {
            let (a, b) = (t.first, t.last);
            for i in a..=b {
                let u = (stations[i].s - stations[a].s) / (stations[b].s - stations[a].s).max(1e-9);
                through[i] = ground_line[a] + (ground_line[b] - ground_line[a]) * u;
            }
        }
        profile = auto(&through);
        structures = find_structures(&stations, &profile, &ground_line, &standard);
    }
    let sections = sections(&stations, &standard);
    // How far each station stands out from the nearest tunnel portal, along
    // the road, where it lies in that portal's approach cut.
    let approach: Vec<Option<f64>> = stations
        .iter()
        .enumerate()
        .map(|(i, st)| {
            structures
                .iter()
                .filter(|t| t.kind == StructureKind::Tunnel)
                .filter_map(|t| {
                    let out = if i <= t.first { stations[t.first].s - st.s } else if i >= t.last { st.s - stations[t.last].s } else { return None };
                    (out <= PORTAL_APPROACH).then_some(out)
                })
                .reduce(f64::min)
        })
        .collect();
    let sides = stations
        .iter()
        .zip(&sections)
        .zip(&approach)
        .map(|((st, x), near_portal)| {
            let y = profile.elevation(st.s);
            // In a portal's approach the cut steepens toward a rock face at
            // the portal, so the cutting (and the headwall closing it) is no
            // wider than the portal needs.
            let local = match near_portal {
                Some(out) => {
                    let run = PORTAL_CUT_RUN + (standard.cut_slope - PORTAL_CUT_RUN) * (out / PORTAL_APPROACH).clamp(0.0, 1.0);
                    RoadStandard { cut_slope: run, rock_cut_slope: standard.rock_cut_slope.min(run), ..standard }
                }
                None => standard,
            };
            [&x.positive, &x.negative].map(|half| {
                let edge = half.last().expect("a formation has an edge");
                let read: Vec<(f64, f64)> = side_samples(daylight)
                    .map(|d| {
                        let off = edge.offset + edge.offset.signum() * d;
                        (d, ground(st.pos.offset(st.heading, inside_limit(st.curvature, off))))
                    })
                    .collect();
                choose_side(&local, y + edge.elevation, &read)
            })
        })
        .collect();
    Some(DesignedRoad {
        standard,
        stations,
        profile,
        sections,
        ground: ground_line,
        sides,
        structures,
    })
}

/// Build a whole design in one call: fit the alignment through `points` (the
/// road passes through each),
/// grade the auto profile over `ground`, and lay the road. Returns the
/// stations, the profile and the meshes.
pub fn design_and_build(
    points: &[P2],
    standard: &RoadStandard,
    ground: impl Fn(P2) -> f64,
    step: f64,
) -> Option<(Alignment, Vec<Station>, VerticalProfile, RoadMeshes)> {
    let alignment = Alignment::fit_through(points, standard, None)?;
    let stations = alignment.stations(step);
    let ground_line: Vec<f64> = stations.iter().map(|st| ground(st.pos)).collect();
    let profile = VerticalProfile::auto(&ground_line, step, standard, 0.3);
    let meshes = build_road(&stations, &profile, standard, 0.04);
    Some((alignment, stations, profile, meshes))
}

#[cfg(test)]
mod tests {
    use super::super::road_design::RoadClass;
    use super::*;

    fn slope_ground(p: P2) -> f64 {
        0.25 * p.z + 3.0 * (p.x * 0.02).sin()
    }

    #[test]
    fn a_built_road_has_every_surface_and_marking() {
        let std = RoadStandard::of(RoadClass::Rural);
        let pts = [
            P2::new(0.0, 0.0),
            P2::new(600.0, 0.0),
            P2::new(900.0, 300.0),
        ];
        let (_, stations, profile, meshes) =
            design_and_build(&pts, &std, |p| 0.02 * p.x, 1.0).unwrap();
        let has = |s: Surface| {
            meshes
                .parts
                .iter()
                .any(|(k, m)| *k == s && m.triangle_count() > 0)
        };
        assert!(
            has(Surface::Carriageway)
                && has(Surface::Shoulder)
                && has(Surface::MarkingWhite)
                && has(Surface::MarkingYellow)
        );
        for (_, m) in &meshes.parts {
            assert_eq!(m.positions.len(), m.normals.len());
            assert!(m.normals.iter().all(|n| n[1] > 0.5), "surfaces face up");
            for t in m.indices.chunks_exact(3) {
                let p = |i: u32| m.positions[i as usize];
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                let ny = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
                assert!(ny >= -1e-6, "a triangle is wound to face down");
            }
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
        }
        // The carriageway sits on the profile (crown aside).
        let cw = &meshes
            .parts
            .iter()
            .find(|(k, _)| *k == Surface::Carriageway)
            .unwrap()
            .1;
        let mid = stations[stations.len() / 2];
        let near = cw
            .positions
            .iter()
            .min_by(|a, b| {
                let da = (a[0] as f64 - mid.pos.x).powi(2) + (a[2] as f64 - mid.pos.z).powi(2);
                let db = (b[0] as f64 - mid.pos.x).powi(2) + (b[2] as f64 - mid.pos.z).powi(2);
                da.total_cmp(&db)
            })
            .unwrap();
        assert!(
            (near[1] as f64 - profile.elevation(mid.s)).abs() < 0.3,
            "{} vs {}",
            near[1],
            profile.elevation(mid.s)
        );
    }

    /// The ground across a side on a uniform hillside of `deg`, rising away
    /// from the road (`up`) or falling, read as `choose_side` reads it.
    fn hillside(deg: f64, up: bool, edge_z: f64, reach: f64) -> Vec<(f64, f64)> {
        let g = deg.to_radians().tan() * if up { 1.0 } else { -1.0 };
        side_samples(reach).map(|d| (d, edge_z + 0.01 * g.signum() + g * d)).collect()
    }

    #[test]
    fn sides_steepen_with_the_hillside_and_end_in_walls() {
        let std = RoadStandard::of(RoadClass::Mountain);
        let run = |s: Side| match s.kind {
            SideKind::Slope { run } => Some(run),
            SideKind::Wall => None,
        };
        // 20 degrees: the class's own soil cut and fill.
        let cut = choose_side(&std, 0.0, &hillside(20.0, true, 0.0, 30.0));
        assert!(cut.cut && run(cut) == Some(std.cut_slope) && cut.reach < 30.0);
        let fill = choose_side(&std, 0.0, &hillside(20.0, false, 0.0, 30.0));
        assert!(!fill.cut && run(fill) == Some(std.fill_slope));
        // 38 degrees, steeper than both: rock cut, reinforced fill.
        let cut = choose_side(&std, 0.0, &hillside(38.0, true, 0.0, 30.0));
        assert_eq!(run(cut), Some(std.rock_cut_slope));
        let fill = choose_side(&std, 0.0, &hillside(38.0, false, 0.0, 30.0));
        assert_eq!(run(fill), Some(1.0));
        // The foot is where the slope meets the ground.
        let SideKind::Slope { run: r } = fill.kind else { unreachable!() };
        assert!((fill.foot - (0.0 - fill.reach / r)).abs() < 0.05, "{fill:?}");
        // 55 degrees down: no slope reaches; a wall holds the road.
        let wall = choose_side(&std, 0.0, &hillside(55.0, false, 0.0, 30.0));
        assert!(matches!(wall.kind, SideKind::Wall) && !wall.cut);
        // A cut wall stands its clearance out; the bake keeps that strip level.
        let cut_wall = choose_side(&std, 0.0, &hillside(75.0, true, 0.0, 30.0));
        assert!(matches!(cut_wall.kind, SideKind::Wall) && cut_wall.cut && cut_wall.reach == WALL_CLEARANCE);
        let x = CrossSection::formation(&std, 0.0);
        let edge = x.positive.last().unwrap();
        let (h, z) = section_height_with(&x, edge.offset + 0.5, 10.0, 40.0, &cut_wall);
        assert!((h - (10.0 + edge.elevation)).abs() < 1e-9 && z == Some(Zone::Ditch));
        let (h, z) = section_height_with(&x, edge.offset + 3.0, 10.0, 40.0, &cut_wall);
        assert!(h == 40.0 && z.is_none(), "beyond the wall the ground stays");
    }

    #[test]
    fn a_road_across_a_steep_hillside_gets_wedges_and_a_fill_wall() {
        let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Explicit);
        let pts = [(P2::new(0.0, 0.0), 0.0), (P2::new(200.0, 0.0), 0.0)];
        // 38 degrees rising toward +z: a rock cut up, a reinforced fill down.
        let g38 = 38f64.to_radians().tan();
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, |p| g38 * p.z).unwrap();
        assert_eq!(d.sides.len(), d.stations.len());
        assert!(d.sides.iter().all(|s| s[0].cut && !s[1].cut));
        let m = build_sides(&d, 0.02);
        assert!(m.wedges.triangle_count() > 0 && m.walls.triangle_count() == 0);
        // 55 degrees: the fill side needs a wall, with a parapet over the road.
        let g55 = 55f64.to_radians().tan();
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, |p| g55 * p.z).unwrap();
        assert!(d.sides.iter().all(|s| matches!(s[1].kind, SideKind::Wall)));
        let m = build_sides(&d, 0.02);
        assert!(m.walls.triangle_count() > 0);
        let top = m.walls.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(top > 0.5, "a parapet stands over the road: {top}");
        for (p, n) in m.walls.positions.iter().zip(&m.walls.normals) {
            assert!(p.iter().all(|v| v.is_finite()));
            assert!(n[1].abs() < 1e-3, "a wall faces sideways: {n:?}");
        }
        // The wedges face up and stay out of the carriageway.
        let half = d.standard.formation_half_width() as f32;
        for (p, n) in m.wedges.positions.iter().zip(&m.wedges.normals) {
            assert!(n[1] >= -1e-3);
            assert!(p[2].abs() >= half - 1e-3, "a wedge vertex inside the formation: {p:?}");
        }
    }

    #[test]
    fn structures_stand_at_the_shoulder_and_portals_are_covered() {
        let pts = [(P2::new(0.0, 0.0), 0.0), (P2::new(1200.0, 0.0), 0.0)];
        let bump = |h: f64| move |p: P2| h * (-((p.x - 600.0) / 150.0).powi(2)).exp();
        let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Auto);
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, bump(80.0)).unwrap();
        let half = structure_half_width(&d.standard);
        assert!(half < d.standard.formation_half_width() - 2.0, "no ditch on a structure");
        let m = build_structures(&d, 0.02);
        // The lining's walls rise from the shoulder's edge: nothing stands out in the ditch.
        let widest = m.lining.positions.iter().map(|p| p[2].abs()).fold(0.0f32, f32::max) as f64;
        assert!((widest - half).abs() < 0.01, "the lining reaches {widest:.2} m, the shoulder {half:.2}");
        // Each portal has the hill closed over its crown.
        let t = *d.structures.iter().find(|s| s.kind == StructureKind::Tunnel).unwrap();
        for i in [t.first, t.last] {
            let cover = d.ground[i] - d.profile.elevation(d.stations[i].s) - (TUNNEL_WALL + half);
            assert!(cover >= MIN_PORTAL_COVER - 0.6, "{cover:.2} m of hill over the crown at a portal");
        }
        // A bridge's deck and parapets stand at the shoulder's edge too: no trough.
        let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Explicit);
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, bump(-40.0)).unwrap();
        let m = build_structures(&d, 0.02);
        let widest = m.concrete.positions.iter().map(|p| p[2].abs()).fold(0.0f32, f32::max) as f64;
        assert!((widest - (half + BRIDGE_PARAPET_WIDTH)).abs() < 0.01, "the parapet's outer face stands {widest:.2} m out, the shoulder {half:.2}");
        // Its inner face is the shoulder's edge: nothing of it stands over the road.
        let inner = m.concrete.positions.iter().filter(|p| p[1] > 0.5).map(|p| p[2].abs()).fold(f32::MAX, f32::min) as f64;
        assert!((inner - half).abs() < 0.01, "the parapet's inner face stands {inner:.2} m out, the shoulder {half:.2}");
    }

    #[test]
    fn a_bridge_parapet_is_a_solid_wall() {
        // A bridge's parapets have a thickness: a top as wide as the wall
        // over every bay, and each end closed, so no edge of a sheet shows.
        let pts = [(P2::new(0.0, 0.0), 0.0), (P2::new(1200.0, 0.0), 0.0)];
        let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Explicit);
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, |p: P2| -40.0 * (-((p.x - 600.0) / 150.0).powi(2)).exp()).unwrap();
        let bridge = *d.structures.iter().find(|s| s.kind == StructureKind::Bridge).expect("a bridge");
        let half = structure_half_width(&d.standard);
        let m = build_structures(&d, 0.02);
        let top = m.concrete.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        let within = |a: [f32; 3]| {
            let across = f64::from(a[2].abs());
            across >= half - 0.01 && across <= half + BRIDGE_PARAPET_WIDTH + 0.01
        };
        let (mut top_area, mut cap_area) = (0.0f64, 0.0f64);
        for t in m.concrete.indices.chunks_exact(3) {
            let v = |i: u32| m.concrete.positions[i as usize];
            let (p, q, r) = (v(t[0]), v(t[1]), v(t[2]));
            let (u, w) = ([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [r[0] - p[0], r[1] - p[1], r[2] - p[2]]);
            let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let area = 0.5 * f64::from(len);
            // Flat at the parapets' top, across the wall's thickness.
            if [p, q, r].iter().all(|a| (a[1] - top).abs() < 0.05 && within(*a)) {
                top_area += area;
            }
            // An end: upright, facing along the road, across the wall, its foot
            // at the road's crowned edge (a little under the profile).
            if n[0].abs() > 0.9 * len && [p, q, r].iter().all(|a| within(*a) && a[1] > -0.5) {
                cap_area += area;
            }
        }
        // quad2 draws both faces, so each surface counts twice.
        let expected = 2.0 * 2.0 * (bridge.s1 - bridge.s0) * BRIDGE_PARAPET_WIDTH;
        assert!(top_area > 0.9 * expected, "the parapets' tops cover {top_area:.1} m2 of {expected:.1}");
        let cap = 2.0 * 4.0 * BRIDGE_PARAPET_WIDTH * BRIDGE_PARAPET;
        assert!(cap_area > 0.9 * cap, "the parapets' ends close {cap_area:.2} m2 of {cap:.2}");
    }

    #[test]
    fn a_portal_closes_the_frame_down_to_the_arch() {
        let pts = [(P2::new(0.0, 0.0), 0.0), (P2::new(1200.0, 0.0), 0.0)];
        let hill = |p: P2| 80.0 * (-((p.x - 600.0) / 150.0).powi(2)).exp();
        let d = design_spline(&pts, RoadSpec::new(RoadClass::Mountain, ProfileMode::Auto), 6.5, 2.0, 30.0, hill).unwrap();
        let t = *d.structures.iter().find(|s| s.kind == StructureKind::Tunnel).unwrap();
        let m = build_structures(&d, 0.02);
        let half = structure_half_width(&d.standard);
        let st = &d.stations[t.first];
        let y = d.profile.elevation(st.s) + d.sections[t.first].elevation_at(half);
        // Concrete triangles standing in the plane x = `px`, as (z, y). The
        // road runs along +x, so the west portal's faces stand at smaller x.
        let plane = |px: f64| -> Vec<[(f64, f64); 3]> {
            m.concrete
                .indices
                .chunks_exact(3)
                .filter_map(|tr| {
                    let p = |i: u32| m.concrete.positions[i as usize];
                    let (a, b, c) = (p(tr[0]), p(tr[1]), p(tr[2]));
                    let on = |v: [f32; 3]| (v[0] as f64 - px).abs() < 0.05;
                    (on(a) && on(b) && on(c)).then(|| [(a[2] as f64, a[1] as f64), (b[2] as f64, b[1] as f64), (c[2] as f64, c[1] as f64)])
                })
                .collect()
        };
        let covered = |tris: &[[(f64, f64); 3]], z: f64, h: f64| {
            tris.iter().any(|t| {
                let s = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) * (h - a.1) - (b.1 - a.1) * (z - a.0);
                let (d1, d2, d3) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
                !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
            })
        };
        let arch_top = y + TUNNEL_WALL + half;
        // The headwall's face: concrete all round the opening, out across the
        // cutting and up past the ground line; the bore itself open.
        let wall = plane(st.pos.x - HEADWALL_OUT);
        for z in [half - 0.3, -(half - 0.3)] {
            assert!(covered(&wall, z, arch_top - 0.3), "hillside shows between the wall and the arch at z {z:.2}");
        }
        assert!(covered(&wall, half + PORTAL_MARGIN + 0.3, y + 1.0), "the wall stops short beside the collar");
        let ground = d.ground[t.first];
        // No wider than the portal's rock cut: near its top the wall ends
        // where that steep face meets the ground, not where the class's
        // slope would (about 18 m further out each side here).
        let depth = ground - y;
        let fh = d.standard.formation_half_width();
        let rock_edge = fh + depth * PORTAL_CUT_RUN + HEADWALL_WING_BURY;
        for z in [rock_edge + 3.0, -(rock_edge + 3.0)] {
            assert!(!covered(&wall, z, ground - 1.0), "the headwall runs {z:.1} m out, past the portal's rock cut");
        }
        assert!(covered(&wall, 0.0, ground + 0.4), "the wall stops under the ground line");
        assert!(!covered(&wall, 0.0, y + TUNNEL_WALL), "the headwall blocks its own bore");
        let road_at_wall = d.profile.elevation(st.s - HEADWALL_OUT);
        for z in [0.0, half - 0.3, -(half - 0.3)] {
            assert!(covered(&wall, z, road_at_wall - 0.3), "the bore shows under the road at z {z:.2}: no sill");
        }
        assert!(!covered(&wall, 0.0, road_at_wall + 0.3), "the sill stands above the road");
        // Behind the wall, over the carve the ground ramps through, a cap
        // closes the pit from above, buried at the carve's top.
        let cap_floor = y + TUNNEL_WALL + half + 1.0;
        let from_above = |px: f64, pz: f64| {
            m.concrete.indices.chunks_exact(3).any(|tr| {
                let v = |i: u32| m.concrete.positions[i as usize];
                let (a, b, c) = (v(tr[0]), v(tr[1]), v(tr[2]));
                if [a, b, c].iter().any(|q| (q[1] as f64) < cap_floor) {
                    return false;
                }
                let s = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]) as f64 * (pz - a[2] as f64) - (b[2] - a[2]) as f64 * (px - a[0] as f64);
                let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
                !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
            })
        };
        for dx in [-1.7, -0.5, 1.0, 2.4] {
            for z in [0.0, half + CARVE_MARGIN - 0.2, -(half + CARVE_MARGIN - 0.2)] {
                assert!(from_above(st.pos.x + dx, z), "the carve shows from above behind the wall at {dx:.1} m, z {z:.1}");
            }
        }
        // The collar's face, proud of the wall: arched, not square.
        let collar = plane(st.pos.x - HEADWALL_OUT - COLLAR_PROUD);
        let yc = d.profile.elevation(st.s - HEADWALL_OUT - COLLAR_PROUD) + d.sections[t.first].elevation_at(half);
        let crown = yc + TUNNEL_WALL + half + PORTAL_MARGIN - 0.2;
        assert!(covered(&collar, 0.0, crown), "the collar runs over the crown");
        assert!(!covered(&collar, half + PORTAL_MARGIN - 0.2, crown), "a square corner stands beside the crown");
        assert!(!covered(&collar, 0.0, y + TUNNEL_WALL), "the collar blocks its own bore");
        // The carve reaches no more than its overlap and lean in front of the
        // headwall, and there only across the road's own formation (the
        // cutting's open floor); behind the wall it opens the ground ramp.
        let carves = tunnel_carves(&d);
        let (west, east) = (st.pos.x - HEADWALL_OUT, d.stations[t.last].pos.x + HEADWALL_OUT);
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for c in &carves {
            assert!(c.size[0] / 2.0 <= d.standard.formation_half_width(), "the carve is wider than the cutting's floor");
            for corner in 0..8 {
                let sg = |bit: usize| if corner & bit == 0 { -0.5 } else { 0.5 };
                let x = c.center[0] + sg(1) * c.size[0] * c.across[0] + sg(2) * c.size[1] * c.up[0] + sg(4) * c.size[2] * c.along[0];
                (lo, hi) = (lo.min(x), hi.max(x));
            }
        }
        assert!(lo >= west - 0.05, "a carve reaches {:.2} m in front of the west wall", west - lo);
        assert!(hi <= east + 0.05, "a carve reaches {:.2} m in front of the east wall", hi - east);
        assert!(lo <= west + 0.1 && hi >= east - 0.1, "ground ramp left standing behind a wall: the carve spans {lo:.2}..{hi:.2}, the walls {west:.2}..{east:.2}");
    }

    #[test]
    fn a_two_sided_quad_with_a_repeated_corner_shows_from_both_sides() {
        // A headwall's fan: the opening's point repeated, as a corner of the
        // outline is put in. Each side must keep its triangle, or a culling
        // renderer shows a hole from one side.
        let (p, o1, o2) = ([0.0, 0.0, 0.0], [0.0, 4.0, 2.0], [0.0, 4.0, -2.0]);
        for q in [[p, o1, o2, p], [o1, p, p, o2]] {
            let mut m = MeshData::default();
            m.quad2(q);
            let sides: Vec<f32> = m
                .indices
                .chunks_exact(3)
                .filter_map(|t| {
                    let v = |i: u32| m.positions[i as usize];
                    let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
                    let n0 = (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]);
                    (n0.abs() > 1e-6).then_some(n0.signum())
                })
                .collect();
            assert!(sides.contains(&1.0) && sides.contains(&-1.0), "a side of {q:?} has no triangle: {sides:?}");
        }
    }

    #[test]
    fn wedges_run_smoothly_and_taper_at_their_ends() {
        // A road along a hillside whose steepness swings every few stations.
        let pts = [(P2::new(0.0, 0.0), 0.0), (P2::new(300.0, 0.0), 0.0)];
        let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Explicit);
        let ground = |p: P2| p.z * (0.35 + 0.25 * (p.x / 6.0).sin());
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, ground).unwrap();
        let m = build_sides(&d, 0.02);
        assert!(m.wedges.triangle_count() > 0);
        // The outer edge (the farthest vertex out at each station) changes
        // smoothly from station to station.
        let mut outer: std::collections::BTreeMap<i64, f32> = std::collections::BTreeMap::new();
        for p in &m.wedges.positions {
            let k = (p[0] / 2.0).round() as i64;
            let e = outer.entry(k).or_insert(0.0);
            *e = e.max(p[2].abs());
        }
        let v: Vec<f32> = outer.values().copied().collect();
        let worst = v.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(worst < 0.8, "the wedges' outer edge jumps {worst:.2} m between stations");
    }

    #[test]
    fn a_hill_gets_a_tunnel_and_a_valley_a_bridge() {
        let pts = [(P2::new(0.0, 0.0), 0.0), (P2::new(1200.0, 0.0), 0.0)];
        let bump = |h: f64| move |p: P2| h * (-((p.x - 600.0) / 150.0).powi(2)).exp();
        // An 80 m hill: Auto would climb it; the road bores through instead.
        let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Auto);
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, bump(80.0)).unwrap();
        let t: Vec<_> = d.structures.iter().filter(|s| s.kind == StructureKind::Tunnel).collect();
        assert_eq!(t.len(), 1, "{:?}", d.structures);
        assert!(t[0].s0 < 600.0 && t[0].s1 > 600.0 && t[0].s1 - t[0].s0 >= MIN_TUNNEL);
        let top = d.stations.iter().map(|st| d.profile.elevation(st.s)).fold(f64::MIN, f64::max);
        // It may climb the flanks at its grade, never over the summit: the
        // bore keeps more than a full cut of mountain over it.
        assert!(top < 80.0 - d.standard.max_cut, "the profile stays under the hill: {top}");
        let m = build_structures(&d, 0.02);
        assert!(m.lining.triangle_count() > 0 && m.concrete.triangle_count() > 0, "lining and portals");
        // The whole bore is carved, portal to portal: no end face of ground stands in it.
        let carves = tunnel_carves(&d);
        assert!(!carves.is_empty());
        for c in &carves {
            let along_s = c.center[0]; // the road runs along +x from x = 0
            assert!(along_s > t[0].s0 - CARVE_OUTSIDE - CARVE_STEP && along_s < t[0].s1 + CARVE_OUTSIDE + CARVE_STEP, "a carve outside the tunnel at x {along_s}");
            let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            assert!(dot(c.across, c.up).abs() < 1e-9 && dot(c.up, c.along).abs() < 1e-9);
            // Right-handed: across = up x along.
            let x = [
                c.up[1] * c.along[2] - c.up[2] * c.along[1],
                c.up[2] * c.along[0] - c.up[0] * c.along[2],
                c.up[0] * c.along[1] - c.up[1] * c.along[0],
            ];
            assert!(dot(x, c.across) > 0.999);
        }
        assert!(carves.iter().any(|c| (c.center[0] - 600.0).abs() < 10.0), "the bore is carved under the summit too");
        // Through the tunnel the centre line is double.
        let road = build_designed(&d, 0.02);
        assert!(road.no_passing.iter().any(|&(a, b)| a <= t[0].s0 + 1.0 && b >= t[0].s1 - 1.0));
        // A 40 m deep valley under a flat road: a bridge on piers.
        let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Explicit);
        let d = design_spline(&pts, spec, 6.5, 2.0, 30.0, bump(-40.0)).unwrap();
        let b: Vec<_> = d.structures.iter().filter(|s| s.kind == StructureKind::Bridge).collect();
        assert_eq!(b.len(), 1, "{:?}", d.structures);
        let m = build_structures(&d, 0.02);
        assert!(m.concrete.triangle_count() > 0 && m.lining.triangle_count() == 0);
        let lowest = m.concrete.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(lowest < -35.0, "piers reach the valley floor: {lowest}");
        // No slopes or walls are drawn under the bridge.
        let sides = build_sides(&d, 0.02);
        let (s0, s1) = (b[0].s0 as f32, b[0].s1 as f32);
        assert!(sides.wedges.positions.iter().all(|p| !(p[0] > s0 + 1.0 && p[0] < s1 - 1.0)));
    }

    #[test]
    fn a_bore_is_carved_end_to_end_with_no_ground_left_inside() {
        let inside = |c: &BoreCarve, p: [f64; 3]| {
            let d = [p[0] - c.center[0], p[1] - c.center[1], p[2] - c.center[2]];
            let dot = |a: [f64; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
            dot(c.across).abs() <= c.size[0] / 2.0 && dot(c.up).abs() <= c.size[1] / 2.0 && dot(c.along).abs() <= c.size[2] / 2.0
        };
        // The bake's widest station spacing, straight and on a curve.
        let hill = |p: P2| 80.0 * (-((p.x - 600.0).powi(2) + (p.z - 0.0).powi(2)) / (150.0f64 * 150.0)).exp();
        let routes: [Vec<(P2, f64)>; 2] = [
            vec![(P2::new(0.0, 0.0), 0.0), (P2::new(1200.0, 0.0), 0.0)],
            vec![(P2::new(0.0, -300.0), 0.0), (P2::new(600.0, 0.0), 0.0), (P2::new(0.0, 300.0), 0.0)],
        ];
        for pts in &routes {
            let spec = RoadSpec::new(RoadClass::Mountain, ProfileMode::Auto);
            let d = design_spline(pts, spec, 6.5, 4.0, 30.0, hill).unwrap();
            let t = *d.structures.iter().find(|s| s.kind == StructureKind::Tunnel).expect("a tunnel");
            let carves = tunnel_carves(&d);
            let half = structure_half_width(&d.standard);
            // Walk the bore every half metre, 1 m over the formation and on
            // the crown's line, the whole length of the tunnel.
            let n = d.stations.len();
            let mut checked = 0;
            let mut s = t.s0 - CARVE_OUTSIDE + 0.5;
            while s < t.s1 + CARVE_OUTSIDE - 0.5 {
                let i = d.stations.partition_point(|st| st.s < s).clamp(1, n - 1);
                let (a, b) = (&d.stations[i - 1], &d.stations[i]);
                let f = (s - a.s) / (b.s - a.s);
                let (x, z) = (a.pos.x + (b.pos.x - a.pos.x) * f, a.pos.z + (b.pos.z - a.pos.z) * f);
                let g = d.ground[i - 1] + (d.ground[i] - d.ground[i - 1]) * f;
                let y = d.profile.elevation(s);
                let _ = g;
                {
                    for lift in [1.0, TUNNEL_WALL + half - 0.5] {
                        let p = [x, y + lift, z];
                        assert!(carves.iter().any(|c| inside(c, p)), "ground left in the bore at s {s:.1}, {lift} m up");
                        checked += 1;
                    }
                }
                s += 0.5;
            }
            assert!(checked > 40, "the walk reached the thin cover at both portals: {checked}");
            // Out past each portal, over the cell its face ramps across.
            for s in [t.s0 - CARVE_OUTSIDE + 0.5, t.s1 + CARVE_OUTSIDE - 0.5] {
                let i = d.stations.partition_point(|st| st.s < s).clamp(1, n - 1);
                let (a, b) = (&d.stations[i - 1], &d.stations[i]);
                let f = (s - a.s) / (b.s - a.s);
                let p = [a.pos.x + (b.pos.x - a.pos.x) * f, d.profile.elevation(s) + 1.0, a.pos.z + (b.pos.z - a.pos.z) * f];
                assert!(carves.iter().any(|c| inside(c, p)), "the portal at s {s:.1} is open");
            }
        }
    }

    #[test]
    fn lanes_one_way_parking_and_sidewalks_shape_the_road() {
        use super::super::road_design::{EdgeChoice, PARKING_LANE};
        // A three-lane arterial each way with parking and sidewalks.
        let spec = RoadSpec { lanes: 3, parking: true, edge: EdgeChoice::Sidewalk, ..RoadSpec::new(RoadClass::Arterial, ProfileMode::Explicit) };
        let s = spec.standard(0.0);
        assert_eq!((s.lanes_per_direction, s.lanes_total(), s.one_way, s.parking), (3, 6, false, true));
        assert!((s.carriageway_width() - 6.0 * s.lane_width).abs() < 1e-9);
        assert_eq!(s.edge, EdgeTreatment::KerbAndSidewalk);
        assert!((s.shoulder_width - PARKING_LANE).abs() < 1e-9 && s.sidewalk_width >= 3.0);
        // A three-lane one-way street 10.5 m wide: 3.5 m lanes, no median.
        let one = RoadSpec { lanes: 3, one_way: true, edge: EdgeChoice::Sidewalk, ..RoadSpec::new(RoadClass::Collector, ProfileMode::Explicit) };
        let o = one.standard(10.5);
        assert_eq!((o.lanes_total(), o.median_width), (3, 0.0));
        assert!((o.lane_width - 3.5).abs() < 1e-9 && (o.half_lanes_width() - 5.25).abs() < 1e-9);
        // Its markings: no centre line; yellow along the left, white
        // dashes between the lanes, white along the right.
        let pts = [(P2::new(0.0, 0.0), 0.0), (P2::new(200.0, 0.0), 0.0)];
        let d = design_spline(&pts, one, 10.5, 2.0, 4.0, |_| 0.0).unwrap();
        let m = build_designed(&d, 0.0);
        let part = |k: Surface| m.parts.iter().find(|(s, _)| *s == k).map(|(_, p)| p.clone()).unwrap_or_default();
        let (yellow, white) = (part(Surface::MarkingYellow), part(Surface::MarkingWhite));
        assert!(yellow.triangle_count() > 0 && yellow.positions.iter().all(|p| p[2] < -4.5), "yellow only along the left edge");
        for lane_line in [-1.75f32, 1.75] {
            assert!(white.positions.iter().any(|p| (p[2] - lane_line).abs() < 0.1), "a lane line at {lane_line}");
        }
        // Parking: stall ticks across each parking lane.
        let pd = design_spline(&pts, spec, 0.0, 2.0, 4.0, |_| 0.0).unwrap();
        let pm = build_designed(&pd, 0.0);
        let pw = pm.parts.iter().find(|(s, _)| *s == Surface::MarkingWhite).map(|(_, p)| p.clone()).unwrap();
        let half = (s.carriageway_width() / 2.0) as f32;
        for side in [-1.0f32, 1.0] {
            let ticks = pw.positions.iter().filter(|p| p[2] * side > half + 1.5 && p[2] * side < half + PARKING_LANE as f32).count();
            assert!(ticks >= 2 * 30, "a stall tick every 6.1 m along 200 m on side {side}: {ticks} outer vertices");
        }
        // A kerbed two-way street holds its centre line solid end to end.
        let local = design_spline(&pts, RoadSpec::new(RoadClass::Local, ProfileMode::Explicit), 0.0, 2.0, 4.0, |_| 0.0).unwrap();
        let lm = build_designed(&local, 0.0);
        assert!(lm.no_passing.iter().any(|&(a, b)| a <= 0.0 && b >= 199.0), "{:?}", lm.no_passing);
    }

    #[test]
    fn dashes_are_whole_and_zones_cut_exactly() {
        // A dash 3 m on, 9 m off, over stations 2.7 m apart (none on a dash edge).
        let breaks: Vec<f64> = (0..=20).map(|k| k as f64 * 2.7).chain((0..=5).flat_map(|k| [k as f64 * 12.0, k as f64 * 12.0 + 3.0])).collect();
        let r = runs(54.0, &breaks, |s| s.rem_euclid(12.0) < 3.0);
        assert_eq!(r.len(), 5, "{r:?}");
        for run in &r {
            let len = run.last().unwrap() - run[0];
            assert!((len - 3.0).abs() < 1e-9, "a dash of {len} m");
        }
        // A straight Rural road with a gap and a bare stretch.
        let std = RoadStandard::of(RoadClass::Rural);
        let a = Alignment::fit(&[P2::new(0.0, 0.0), P2::new(300.0, 0.0)], &std, None).unwrap();
        let st = a.stations(2.7);
        let flat = VerticalProfile::from_vpis(
            vec![Vpi { s: 0.0, elevation: 0.0, curve_length: 0.0 }, Vpi { s: 300.0, elevation: 0.0, curve_length: 0.0 }],
            &std,
        );
        let zones = RoadZones { gaps: vec![(100.0, 130.0)], bare: vec![(130.0, 137.0)], no_passing: vec![] };
        let m = build_road_zoned(&st, &flat, &std, 0.02, RoadEnds::default(), &zones);
        let part = |s: Surface| m.parts.iter().find(|(k, _)| *k == s).map(|(_, d)| d.clone()).unwrap();
        let cw = part(Surface::Carriageway);
        assert!(cw.positions.iter().all(|p| !(p[0] > 100.01 && p[0] < 129.99)), "no surface in the gap");
        assert!(cw.positions.iter().any(|p| (p[0] - 100.0).abs() < 1e-3), "the surface stops exactly at 100 m");
        assert!(cw.positions.iter().any(|p| (p[0] - 130.0).abs() < 1e-3), "and starts again exactly at 130 m");
        let white = part(Surface::MarkingWhite);
        assert!(white.positions.iter().all(|p| !(p[0] > 100.01 && p[0] < 136.99)), "no paint in the gap or bare stretch");
    }

    #[test]
    fn short_passing_gaps_stay_no_passing() {
        let min = minimum_passing_zone(270.0);
        assert_eq!(min, 120.0);
        // A 60 m gap closes; a 300 m one stays; short ends close too.
        let zones = vec![(50.0, 200.0), (260.0, 400.0), (700.0, 900.0)];
        assert_eq!(close_short_passing_zones(zones, 1000.0, min), vec![(0.0, 400.0), (700.0, 1000.0)]);
        // Nothing to close.
        assert_eq!(close_short_passing_zones(vec![], 1000.0, min), vec![]);
        assert_eq!(close_short_passing_zones(vec![(400.0, 500.0)], 1000.0, min), vec![(400.0, 500.0)]);
    }

    #[test]
    fn the_centre_line_doubles_over_a_crest() {
        let std = RoadStandard::of(RoadClass::Rural);
        // A sharp crest at 500 m on a straight road.
        let stations: Vec<Station> = (0..=1000)
            .map(|i| Station {
                s: i as f64,
                pos: P2::new(i as f64, 0.0),
                heading: 0.0,
                curvature: 0.0,
            })
            .collect();
        let profile = VerticalProfile::from_vpis(
            vec![
                super::super::road_design::Vpi {
                    s: 0.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
                super::super::road_design::Vpi {
                    s: 500.0,
                    elevation: 30.0,
                    curve_length: 0.0,
                },
                super::super::road_design::Vpi {
                    s: 1000.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
            ],
            &std,
        );
        let zones = no_passing_zones(
            &stations,
            &profile,
            passing_sight_distance(std.design_speed),
        );
        assert!(
            zones.iter().any(|&(a, b)| a <= 500.0 && b >= 500.0),
            "{zones:?}"
        );
        // Flat and straight: passing everywhere.
        let flat = VerticalProfile::from_vpis(
            vec![
                super::super::road_design::Vpi {
                    s: 0.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
                super::super::road_design::Vpi {
                    s: 1000.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
            ],
            &std,
        );
        assert!(no_passing_zones(&stations, &flat, 485.0).is_empty());
    }

    #[test]
    fn the_corridor_carves_a_bench_and_meets_the_ground() {
        let std = RoadStandard::of(RoadClass::Mountain);
        let pts = [P2::new(0.0, 0.0), P2::new(400.0, 0.0)];
        let a = Alignment::fit(&pts, &std, None).unwrap();
        let st = a.stations(1.0);
        let g: Vec<f64> = st.iter().map(|s| slope_ground(s.pos)).collect();
        let prof = VerticalProfile::auto(&g, 1.0, &std, 0.3);
        let c = Corridor::new(&st, &prof, &std, slope_ground);
        // On the centreline: the profile.
        let mid = P2::new(200.0, 0.0);
        assert!((c.height_at(mid) - prof.elevation(200.0)).abs() < 0.05);
        // Far away: the natural ground.
        let far = P2::new(200.0, 300.0);
        assert!((c.height_at(far) - slope_ground(far)).abs() < 1e-9);
        // Uphill (+z: the positive side of a +x heading): a cut slope, never above the natural ground.
        for d in [6.0, 10.0, 20.0, 40.0] {
            let p = P2::new(200.0, d);
            assert!(c.height_at(p) <= slope_ground(p) + 1e-9);
        }
    }

    #[test]
    fn a_section_reports_its_zones_and_daylights_both_ways() {
        let std = RoadStandard::of(RoadClass::Rural);
        let x = CrossSection::formation(&std, cross_slope(&std, 0.0, 0.0));
        let half = x.positive.last().unwrap().offset.abs();
        // The crown is the profile; the lane zone is reported.
        let (h, z) = section_height(&x, 0.0, 100.0, 90.0, &std, false);
        assert!((h - 100.0).abs() < 1e-9 && z == Some(Zone::Lane));
        // Ground far above: a cut, rising at the cut slope, never above the ground.
        let (h, z) = section_height(&x, half + 3.0, 100.0, 140.0, &std, false);
        let edge = 100.0 + x.positive.last().unwrap().elevation;
        assert!(z == Some(Zone::CutSlope));
        assert!((h - (edge + 3.0 / std.cut_slope)).abs() < 1e-9);
        // Ground far below: a fill, falling at the fill slope.
        let (h, z) = section_height(&x, -(half + 4.0), 100.0, 60.0, &std, false);
        let edge = 100.0 + x.negative.last().unwrap().elevation;
        assert!(z == Some(Zone::FillSlope));
        assert!((h - (edge - 4.0 / std.fill_slope)).abs() < 1e-9);
        // Past where the slope meets the ground: the ground, untouched.
        let (h, z) = section_height(&x, half + 200.0, 100.0, 101.0, &std, false);
        assert!(h == 101.0 && z.is_none());
    }

    #[test]
    fn design_spline_honours_each_profile_mode() {
        let ground = |p: P2| 0.05 * p.x + 2.0 * (p.x * 0.1).sin();
        let pts = [
            (P2::new(0.0, 0.0), 10.0),
            (P2::new(300.0, 40.0), 30.0),
            (P2::new(600.0, 0.0), 10.0),
        ];
        let class = RoadClass::Rural;
        let auto = design_spline(
            &pts,
            RoadSpec::new(class, ProfileMode::Auto),
            0.0,
2.0,
10.0,
ground,
        )
        .unwrap();
        // Auto: near the ground at both ends, never steeper than the class allows.
        let last = auto.stations.last().unwrap();
        assert!((auto.profile.elevation(0.0) - ground(auto.stations[0].pos)).abs() < 1.0);
        assert!((auto.profile.elevation(last.s) - ground(last.pos)).abs() < 1.0);
        assert_eq!(auto.profile.vpis.last().unwrap().s, last.s);
        let g_max = auto.standard.max_grade;
        assert!(auto
            .stations
            .iter()
            .all(|st| auto.profile.grade(st.s).abs() <= g_max + 0.005));
        assert_eq!(auto.sections.len(), auto.stations.len());
        // Explicit: the road passes through each point's height.
        let exp = design_spline(
            &pts,
            RoadSpec::new(class, ProfileMode::Explicit),
            0.0,
2.0,
10.0,
ground,
        )
        .unwrap();
        for (p, y) in pts {
            let st = exp
                .stations
                .iter()
                .min_by(|a, b| {
                    let da = (a.pos.x - p.x).powi(2) + (a.pos.z - p.z).powi(2);
                    let db = (b.pos.x - p.x).powi(2) + (b.pos.z - p.z).powi(2);
                    da.total_cmp(&db)
                })
                .unwrap();
            assert!((exp.profile.elevation(st.s) - y).abs() < 0.05, "{y}");
        }
        // Ground: within a metre of the ground everywhere.
        let gr = design_spline(
            &pts,
            RoadSpec::new(class, ProfileMode::Ground),
            0.0,
2.0,
10.0,
ground,
        )
        .unwrap();
        assert!(gr
            .stations
            .iter()
            .zip(&gr.ground)
            .all(|(st, g)| (gr.profile.elevation(st.s) - g).abs() < 1.0));
        // A width sets the lanes; too few points design nothing.
        let wide = RoadSpec::new(class, ProfileMode::Auto)
        .standard(10.0);
        assert!((wide.lane_width - 5.0f64.min(4.5)).abs() < 1e-9);
        assert!(design_spline(&pts[..1], RoadSpec::new(class, ProfileMode::Auto), 0.0,
2.0,
10.0,
ground).is_none());
    }

    #[test]
    fn collinear_points_design_one_straight_road() {
        // Three points on a line: the middle one turns nothing.
        let pts = [
            (P2::new(-28.0, 10.0), 23.0),
            (P2::new(16.0, 10.0), 23.0),
            (P2::new(60.0, 10.0), 23.0),
        ];
        for profile in ProfileMode::ALL {
            let d = design_spline(
                &pts,
                RoadSpec::new(RoadClass::Collector, profile),
                8.0,
                1.0,
                10.0,
                |_| 20.0,
            )
            .unwrap_or_else(|| panic!("{} designs nothing", profile.name()));
            let last = d.stations.last().unwrap();
            assert!((last.s - 88.0).abs() < 1e-6, "{}: {}", profile.name(), last.s);
            assert!(d.stations.iter().all(|st| (st.pos.z - 10.0).abs() < 1e-9 && st.heading.abs() < 1e-9));
            let want = if profile == ProfileMode::Explicit { 23.0 } else { 20.0 };
            assert!(d
                .stations
                .iter()
                .all(|st| (d.profile.elevation(st.s) - want).abs() < 1e-6));
            assert!(d.sections.iter().all(|x| x.cross_slope == 0.0));
            assert!((d.standard.formation_half_width() - 5.5).abs() < 1e-9);
        }
    }
}
