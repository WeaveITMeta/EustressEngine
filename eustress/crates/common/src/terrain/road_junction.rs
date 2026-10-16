//! At-grade junctions: where road ends meet, the junction box, its kerb
//! returns, sidewalk corners, crosswalks, stop bars, its traffic control and
//! the lane connections through it.
//!
//! Pure math over [`road_design`](super::road_design) standards:
//! [`design_junction`] takes the junction's centre and the roads that leave
//! it (their outward headings and standards) and decides everything a
//! civil engineer would from those alone:
//! - **Kerb returns:** each corner between two roads is a circular return
//!   tangent to both kerb lines, its radius chosen for the design vehicle of
//!   the lesser road (a truck on arterials, a car on local streets). A side
//!   with no corner (the far side of a T) is a straight kerb.
//! - **Setbacks:** each road stops where its corners let it, so the junction
//!   box is exactly the area the turning paths need.
//! - **Control:** signals where two through roads cross, stop signs on the
//!   minor road where a local street meets a larger one, an all-way stop where
//!   local streets cross.
//! - **Markings:** a continental crosswalk across every urban approach and a
//!   stop bar across the incoming lanes of every approach that must stop.
//! - **Connections:** a lane path for every legal movement (through, left,
//!   right), for the road graph and for vehicles.
//!
//! Traffic drives on the right; offsets are positive to the right of the
//! road's outward heading (see `road_design`'s frame).

use super::road_design::{CrossSection, EdgeTreatment, RoadClass, RoadStandard, Station, P2};
use super::road_mesh::{DesignedRoad, MeshData, RoadEnds, RoadMeshes, Surface};
use std::f64::consts::PI;

/// Length of a crosswalk along the road, metres.
pub const CROSSWALK_LENGTH: f64 = 3.0;
/// Gap between a crosswalk and the stop bar behind it, metres.
pub const STOP_BAR_GAP: f64 = 1.2;
/// Width of a stop bar along the road, metres.
pub const STOP_BAR_WIDTH: f64 = 0.45;
/// Height of paint over the surface it is laid on, metres.
pub const MARKING_RISE: f64 = 0.012;
/// Width of a kerb's face across the road, as the road's own section has it.
const KERB_FACE: f64 = 0.15;
/// How far a corner sidewalk's back and kerb faces reach below its top, so
/// no gap shows under it wherever the ground sits lower, metres.
const SIDEWALK_SKIRT: f64 = 0.6;
/// Spacing of the rows a draped junction surface is built from, metres.
const DRAPE_STEP: f64 = 0.5;

/// One leg's road surface near a junction: the elevation of the surface the
/// road's meshes are laid on (before their lift) on a grid `step` metres
/// apart along the leg from the junction's centre, at `offsets` across it
/// (positive to the right of the leg's outward heading, ascending).
#[derive(Clone, Debug, PartialEq)]
pub struct LegSurface {
    pub step: f64,
    pub offsets: Vec<f64>,
    /// `heights[k][m]`: `k * step` along the leg, `offsets[m]` across it.
    pub heights: Vec<Vec<f64>>,
}

impl LegSurface {
    /// A level surface at `z` everywhere.
    pub fn flat(z: f64) -> Self {
        Self { step: 1.0, offsets: vec![-1.0, 1.0], heights: vec![vec![z, z]] }
    }

    /// The surface `along` metres out from the centre, `offset` across:
    /// linear between grid rows and between offsets, level past the ends.
    pub fn height(&self, along: f64, offset: f64) -> f64 {
        let n = self.heights.len();
        if n == 0 {
            return 0.0;
        }
        let row = |r: &[f64]| interp(&self.offsets, r, offset);
        if n == 1 {
            return row(&self.heights[0]);
        }
        let f = (along / self.step).clamp(0.0, (n - 1) as f64);
        let k = (f.floor() as usize).min(n - 2);
        let t = f - k as f64;
        let (a, b) = (row(&self.heights[k]), row(&self.heights[k + 1]));
        a + (b - a) * t
    }
}

/// `ys` over ascending `xs` at `x`, linear between them and level past the ends.
fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return 0.0;
    }
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let i = xs[..n].partition_point(|&v| v < x).clamp(1, n - 1);
    let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]).max(1e-12);
    ys[i - 1] + (ys[i] - ys[i - 1]) * t
}

/// A cross-section's elevation at `offset` (the road's frame), over the
/// profile: linear between its points, level past its last.
fn section_rel(sec: &CrossSection, offset: f64) -> f64 {
    sec.elevation_at(offset)
}

/// The elevation of road `d`'s surface `s` metres along it, `offset` metres
/// to its right: its profile plus its cross-section there, linear between
/// stations as its meshes are.
pub fn surface_at(d: &DesignedRoad, s: f64, offset: f64) -> f64 {
    let n = d.stations.len().min(d.sections.len());
    if n == 0 {
        return d.profile.elevation(s);
    }
    if n == 1 {
        return d.profile.elevation(s) + section_rel(&d.sections[0], offset);
    }
    let i = d.stations[..n].partition_point(|st| st.s < s).clamp(1, n - 1);
    let (a, b) = (&d.stations[i - 1], &d.stations[i]);
    let t = ((s - a.s) / (b.s - a.s).max(1e-9)).clamp(0.0, 1.0);
    let rel = section_rel(&d.sections[i - 1], offset) * (1.0 - t) + section_rel(&d.sections[i], offset) * t;
    d.profile.elevation(s) + rel
}

/// Leg `leg` of road `d`'s surface out to `reach` metres from the junction,
/// sampled at the road's own section breaks so its crown, kerbs and
/// sidewalks are kept exactly.
fn leg_surface(d: &DesignedRoad, leg: &Leg, reach: f64) -> LegSurface {
    let total = d.stations.last().map_or(0.0, |st| st.s);
    let sign = if leg.outward { 1.0 } else { -1.0 };
    let sec = &d.sections[station_at(d, leg.s).min(d.sections.len().saturating_sub(1))];
    // Offsets in the leg's frame: the road's right is the leg's right only
    // when the leg leaves toward increasing s.
    let mut offsets: Vec<f64> = sec.positive.iter().chain(&sec.negative).map(|p| p.offset * sign).collect();
    offsets.push(0.0);
    offsets.sort_by(f64::total_cmp);
    offsets.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let rows = (reach / DRAPE_STEP).ceil() as usize + 1;
    let heights = (0..rows)
        .map(|k| {
            let s = (leg.s + sign * k as f64 * DRAPE_STEP).clamp(0.0, total);
            offsets.iter().map(|&o| surface_at(d, s, o * sign)).collect()
        })
        .collect();
    LegSurface { step: DRAPE_STEP, offsets, heights }
}

/// Which way traffic on an approach's road runs, seen from the junction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flow {
    /// Both ways: lanes in on the left of the outward heading, out on the right.
    #[default]
    Both,
    /// A one-way road toward the junction: every lane comes in.
    Inbound,
    /// A one-way road away from it: every lane goes out.
    Outbound,
}

/// One road leaving the junction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Approach {
    /// Heading pointing away from the junction, radians.
    pub heading: f64,
    pub standard: RoadStandard,
    pub flow: Flow,
}

impl Approach {
    /// A two-way road leaving at `heading`.
    pub fn new(heading: f64, standard: RoadStandard) -> Self {
        Self { heading, standard, flow: Flow::Both }
    }

    /// Lanes that come into the junction.
    pub fn lanes_in(&self) -> u32 {
        match self.flow {
            Flow::Both => self.standard.lanes_per_direction,
            Flow::Inbound => self.standard.lanes_total(),
            Flow::Outbound => 0,
        }
    }

    /// Lanes that leave it.
    pub fn lanes_out(&self) -> u32 {
        match self.flow {
            Flow::Both => self.standard.lanes_per_direction,
            Flow::Outbound => self.standard.lanes_total(),
            Flow::Inbound => 0,
        }
    }

    /// Offset across the approach (positive right of its outward heading)
    /// of incoming lane `k`, counted from the driver's left.
    pub fn lane_in_offset(&self, k: u32) -> f64 {
        let s = &self.standard;
        match self.flow {
            Flow::Inbound => s.half_lanes_width() - (k as f64 + 0.5) * s.lane_width,
            _ => -(s.median_width / 2.0 + (k as f64 + 0.5) * s.lane_width),
        }
    }

    /// Offset of outgoing lane `k`, counted from the driver's left.
    pub fn lane_out_offset(&self, k: u32) -> f64 {
        let s = &self.standard;
        match self.flow {
            Flow::Outbound => -s.half_lanes_width() + (k as f64 + 0.5) * s.lane_width,
            _ => s.median_width / 2.0 + (k as f64 + 0.5) * s.lane_width,
        }
    }

    /// The span across the approach its stop bar covers: every incoming lane.
    pub fn stop_bar_span(&self) -> Option<(f64, f64)> {
        let s = &self.standard;
        let half = s.carriageway_width() / 2.0;
        match self.flow {
            Flow::Outbound => None,
            Flow::Inbound => Some((-half + 0.2, half - 0.2)),
            Flow::Both => Some((-half + 0.2, -(s.median_width / 2.0) - 0.2)),
        }
    }
}

/// How traffic through the junction is controlled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Signals,
    AllWayStop,
    /// Stop signs on the minor approaches only.
    MinorStop,
}

/// A movement through the junction, as the driver sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Movement {
    Left,
    Through,
    Right,
}

/// One lane path through the junction.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub from: usize,
    pub from_lane: u32,
    pub to: usize,
    pub to_lane: u32,
    pub movement: Movement,
    pub path: Vec<P2>,
}

/// One corner of the junction, between approach `a` and the next approach
/// counter-clockwise (by heading), `b`.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    pub a: usize,
    pub b: usize,
    /// The kerb return's radius and centre (`None` for a straight kerb).
    pub ret: Option<(f64, P2)>,
    /// The kerb line from approach `a`'s side to approach `b`'s, through the return.
    pub kerb: Vec<P2>,
}

/// A designed junction.
#[derive(Clone, Debug, PartialEq)]
pub struct Junction {
    pub centre: P2,
    /// The approaches in order of heading.
    pub approaches: Vec<Approach>,
    /// Distance from the centre at which each approach's road begins.
    pub setbacks: Vec<f64>,
    pub corners: Vec<Corner>,
    /// The junction box, counter-clockwise by heading, closed implicitly.
    pub boundary: Vec<P2>,
    pub control: Control,
    /// Per approach: whether its traffic stops (a stop bar is painted).
    pub stops: Vec<bool>,
    /// Per approach: whether a crosswalk crosses it.
    pub crosswalks: Vec<bool>,
    pub connections: Vec<Connection>,
    /// Where signal poles stand (at the corners), for the furniture pass.
    pub signal_poles: Vec<P2>,
}

/// A road's importance for junction decisions.
fn rank(class: RoadClass) -> u8 {
    match class {
        RoadClass::Motorway => 5,
        RoadClass::Arterial => 4,
        RoadClass::Collector | RoadClass::Rural => 3,
        RoadClass::Mountain | RoadClass::Local => 2,
        RoadClass::Unpaved => 1,
        RoadClass::Driveway => 0,
    }
}

/// Offset of a road's kerb (or shoulder edge) from its centreline.
pub fn kerb_offset(s: &RoadStandard) -> f64 {
    s.carriageway_width() / 2.0 + s.shoulder_width
}

/// Kerb return radius between two roads, from the design vehicle the lesser
/// road must turn: a semi-trailer where both carry trucks, a car on local
/// streets (tighter still where kerbs and sidewalks slow turning traffic).
pub fn corner_radius(a: &RoadStandard, b: &RoadStandard) -> f64 {
    let lesser = if rank(a.class) <= rank(b.class) { a } else { b };
    let urban =
        a.edge == EdgeTreatment::KerbAndSidewalk && b.edge == EdgeTreatment::KerbAndSidewalk;
    match lesser.class {
        RoadClass::Motorway | RoadClass::Arterial => 12.0,
        RoadClass::Collector | RoadClass::Rural | RoadClass::Mountain => 9.0,
        RoadClass::Local => {
            if urban {
                6.0
            } else {
                7.5
            }
        }
        RoadClass::Unpaved | RoadClass::Driveway => 4.5,
    }
}

fn dir(h: f64) -> P2 {
    P2::new(h.cos(), h.sin())
}
fn add(a: P2, b: P2) -> P2 {
    P2::new(a.x + b.x, a.z + b.z)
}
fn sub(a: P2, b: P2) -> P2 {
    P2::new(a.x - b.x, a.z - b.z)
}
fn mul(a: P2, k: f64) -> P2 {
    P2::new(a.x * k, a.z * k)
}
fn dot(a: P2, b: P2) -> f64 {
    a.x * b.x + a.z * b.z
}
fn len(a: P2) -> f64 {
    a.x.hypot(a.z)
}
fn wrap(h: f64) -> f64 {
    h.rem_euclid(2.0 * PI)
}
fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(2.0 * PI);
    if d > PI {
        d - 2.0 * PI
    } else {
        d
    }
}

/// Where lines `p + t u` and `q + s v` meet.
fn intersect(p: P2, u: P2, q: P2, v: P2) -> Option<P2> {
    let den = u.x * v.z - u.z * v.x;
    if den.abs() < 1e-9 {
        return None;
    }
    let w = sub(q, p);
    let t = (w.x * v.z - w.z * v.x) / den;
    Some(add(p, mul(u, t)))
}

/// Design the junction at `centre` from the roads that leave it.
pub fn design_junction(centre: P2, approaches: &[Approach]) -> Option<Junction> {
    if approaches.len() < 2 {
        return None;
    }
    let mut ap: Vec<Approach> = approaches
        .iter()
        .map(|a| Approach {
            heading: wrap(a.heading),
            ..*a
        })
        .collect();
    ap.sort_by(|a, b| a.heading.total_cmp(&b.heading));
    let n = ap.len();
    let u: Vec<P2> = ap.iter().map(|a| dir(a.heading)).collect();
    let nrm: Vec<P2> = ap.iter().map(|a| P2::normal(a.heading)).collect();
    let h: Vec<f64> = ap.iter().map(|a| kerb_offset(&a.standard)).collect();

    // Corners: approach i's positive-side kerb meets approach j's negative-side kerb.
    let mut corners = Vec::with_capacity(n);
    let mut reach = vec![0.0f64; n]; // how far along each approach its corners reach
    for i in 0..n {
        let j = (i + 1) % n;
        let phi = wrap(ap[j].heading - ap[i].heading + if n == 2 && j == 0 { 0.0 } else { 0.0 });
        let phi = if phi < 1e-6 { 2.0 * PI } else { phi };
        let pi_line = add(centre, mul(nrm[i], h[i]));
        let pj_line = sub(centre, mul(nrm[j], h[j]));
        if phi > MIN_RETURN_ANGLE && phi < PI - 0.15 {
            let r = corner_radius(&ap[i].standard, &ap[j].standard);
            let p = intersect(pi_line, u[i], pj_line, u[j])?;
            let d = r / (phi / 2.0).tan();
            let ti = add(p, mul(u[i], d));
            let tj = add(p, mul(u[j], d));
            let bis = add(u[i], u[j]);
            let bis = mul(bis, 1.0 / len(bis));
            let q = add(p, mul(bis, r / (phi / 2.0).sin()));
            // The return sweeps from ti to tj around q, the short way.
            let (a0, a1) = (
                sub(ti, q).z.atan2(sub(ti, q).x),
                sub(tj, q).z.atan2(sub(tj, q).x),
            );
            let sweep = angle_diff(a1, a0);
            let steps = ((sweep.abs() / (PI / 24.0)).ceil() as usize).max(2);
            let kerb: Vec<P2> = (0..=steps)
                .map(|k| add(q, mul(dir(a0 + sweep * k as f64 / steps as f64), r)))
                .collect();
            reach[i] = reach[i].max(dot(sub(ti, centre), u[i]));
            reach[j] = reach[j].max(dot(sub(tj, centre), u[j]));
            corners.push(Corner {
                a: i,
                b: j,
                ret: Some((r, q)),
                kerb,
            });
        } else {
            corners.push(Corner {
                a: i,
                b: j,
                ret: None,
                kerb: Vec::new(),
            });
        }
    }
    // Setbacks: past every corner, and past the kerbs of every road it crosses.
    let setbacks: Vec<f64> = (0..n)
        .map(|i| {
            let crossing = (0..n)
                .filter(|&k| k != i)
                .map(|k| {
                    let s = angle_diff(ap[k].heading, ap[i].heading).sin().abs();
                    if s > 0.2 {
                        h[k] / s
                    } else {
                        0.0
                    }
                })
                .fold(0.0, f64::max);
            (reach[i].max(crossing) + 0.5).min(MAX_JUNCTION_SETBACK)
        })
        .collect();
    // Straight corners run between the two setback points.
    for c in &mut corners {
        if c.ret.is_none() {
            let a = add(
                centre,
                add(mul(u[c.a], setbacks[c.a]), mul(nrm[c.a], h[c.a])),
            );
            let b = add(
                centre,
                sub(mul(u[c.b], setbacks[c.b]), mul(nrm[c.b], h[c.b])),
            );
            c.kerb = vec![a, b];
        }
    }
    // The box: each approach's mouth, then its corner's kerb to the next approach.
    let mut boundary = Vec::new();
    for i in 0..n {
        let mouth = add(centre, mul(u[i], setbacks[i]));
        boundary.push(sub(mouth, mul(nrm[i], h[i])));
        boundary.push(add(mouth, mul(nrm[i], h[i])));
        boundary.extend(corners[i].kerb.iter().copied());
    }
    // Control is decided by roads, not approaches: two opposite approaches of
    // one class are one road passing through.
    let mut road_of = vec![usize::MAX; n];
    let mut road_rank: Vec<u8> = Vec::new();
    for i in 0..n {
        if road_of[i] != usize::MAX {
            continue;
        }
        road_of[i] = road_rank.len();
        if let Some(k) = (0..n).find(|&k| {
            road_of[k] == usize::MAX
                && ap[k].standard.class == ap[i].standard.class
                && angle_diff(ap[k].heading, ap[i].heading + PI).abs() < 0.35
        }) {
            road_of[k] = road_rank.len();
        }
        road_rank.push(rank(ap[i].standard.class));
    }
    let through = |i: usize| road_of.iter().filter(|&&r| r == road_of[i]).count() == 2;
    let mut ranks = road_rank.clone();
    ranks.sort_unstable_by(|a, b| b.cmp(a));
    let top = ranks[0];
    let majors = ranks.iter().filter(|&&r| r >= 3).count();
    let control = if majors >= 2 {
        Control::Signals
    } else if n >= 4 && ranks.iter().all(|&r| r == top) {
        Control::AllWayStop
    } else {
        Control::MinorStop
    };
    let stops: Vec<bool> = (0..n)
        .map(|i| {
            ap[i].flow != Flow::Outbound
                && match control {
                    Control::Signals | Control::AllWayStop => true,
                    // The lesser roads stop; among equals at a T, the stem (the approach with no road through it).
                    Control::MinorStop => road_rank[road_of[i]] < top || !through(i),
                }
        })
        .collect();
    let crosswalks: Vec<bool> = ap
        .iter()
        .map(|a| a.standard.edge == EdgeTreatment::KerbAndSidewalk || control == Control::Signals)
        .collect();
    // Lane connections.
    let stop_line = CROSSWALK_LENGTH + STOP_BAR_GAP + STOP_BAR_WIDTH;
    let mut connections = Vec::new();
    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            let turn = angle_diff(ap[j].heading, ap[i].heading + PI);
            if turn.abs() > 150f64.to_radians() {
                continue; // no U-turns
            }
            let movement = if turn.abs() < 30f64.to_radians() {
                Movement::Through
            } else if turn > 0.0 {
                Movement::Right
            } else {
                Movement::Left
            };
            let (li, lj) = (ap[i].lanes_in(), ap[j].lanes_out());
            if li == 0 || lj == 0 {
                continue;
            }
            let pairs: Vec<(u32, u32)> = match movement {
                Movement::Through => (0..li.min(lj)).map(|k| (k, k)).collect(),
                Movement::Right => vec![(li - 1, lj - 1)],
                Movement::Left => vec![(0, 0)],
            };
            for (fl, tl) in pairs {
                let s0 = add(centre, add(mul(u[i], setbacks[i] + stop_line), mul(nrm[i], ap[i].lane_in_offset(fl))));
                let s3 = add(centre, add(mul(u[j], setbacks[j] + stop_line), mul(nrm[j], ap[j].lane_out_offset(tl))));
                let l = len(sub(s3, s0));
                let s1 = add(s0, mul(u[i], -l / 3.0));
                let s2 = sub(s3, mul(u[j], l / 3.0));
                let path = (0..=16)
                    .map(|k| {
                        let t = k as f64 / 16.0;
                        let m = 1.0 - t;
                        add(
                            add(mul(s0, m * m * m), mul(s1, 3.0 * m * m * t)),
                            add(mul(s2, 3.0 * m * t * t), mul(s3, t * t * t)),
                        )
                    })
                    .collect();
                connections.push(Connection {
                    from: i,
                    from_lane: fl,
                    to: j,
                    to_lane: tl,
                    movement,
                    path,
                });
            }
        }
    }
    let signal_poles = if control == Control::Signals {
        corners
            .iter()
            .filter_map(|c| {
                let (_, q) = c.ret?;
                let mid = c.kerb[c.kerb.len() / 2];
                let toward = sub(q, mid);
                Some(add(mid, mul(toward, 1.0 / len(toward).max(1e-9))))
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(Junction {
        centre,
        approaches: ap,
        setbacks,
        corners,
        boundary,
        control,
        stops,
        crosswalks,
        connections,
        signal_poles,
    })
}

impl Junction {
    /// Stations for approach `i`'s road: straight from its mouth outward, `length`
    /// metres at `step` spacing (the start of the road the approach belongs to).
    pub fn approach_stations(&self, i: usize, length: f64, step: f64) -> Vec<Station> {
        let a = &self.approaches[i];
        let u = dir(a.heading);
        let mouth = add(self.centre, mul(u, self.setbacks[i]));
        let n = (length / step).ceil() as usize;
        (0..=n)
            .map(|k| {
                let s = (k as f64 * step).min(length);
                Station {
                    s,
                    pos: add(mouth, mul(u, s)),
                    heading: a.heading,
                    curvature: 0.0,
                }
            })
            .collect()
    }

    /// How approach `i`'s road should paint its junction end: bare through the
    /// crosswalk and stop bar, then a solid centre line on the approach.
    pub fn approach_ends(&self, i: usize) -> RoadEnds {
        let bare = if self.crosswalks[i] || self.stops[i] {
            CROSSWALK_LENGTH + STOP_BAR_GAP + STOP_BAR_WIDTH + 0.3
        } else {
            0.5
        };
        RoadEnds {
            bare_start: bare,
            bare_end: 0.0,
            solid_start: 30.0,
            solid_end: 0.0,
        }
    }

    /// The junction's own meshes, level at elevation `z`: the box, sidewalk
    /// corners, crosswalks and stop bars. `lift` matches the roads'.
    pub fn meshes(&self, z: f64, lift: f64) -> RoadMeshes {
        let flat: Vec<LegSurface> = self.approaches.iter().map(|_| LegSurface::flat(z)).collect();
        self.meshes_on(&flat, lift)
    }

    /// The box's outline, each vertex with its surface height: each mouth
    /// across its road at the road's own section breaks (so its crown meets
    /// the box), then the corner's kerb, its height blended from one road's
    /// kerb line to the next's.
    pub fn draped_boundary(&self, surfaces: &[LegSurface]) -> Vec<[f64; 3]> {
        let n = self.approaches.len();
        let hgt = |i: usize, d: f64, o: f64| surfaces.get(i).map_or(0.0, |s| s.height(d, o));
        let u: Vec<P2> = self.approaches.iter().map(|a| dir(a.heading)).collect();
        let nrm: Vec<P2> = self.approaches.iter().map(|a| P2::normal(a.heading)).collect();
        let mut out = Vec::new();
        for i in 0..n {
            let h = kerb_offset(&self.approaches[i].standard);
            let mut offs: Vec<f64> = surfaces
                .get(i)
                .map(|s| s.offsets.iter().copied().filter(|o| o.abs() < h - 1e-6).collect())
                .unwrap_or_default();
            offs.push(-h);
            offs.push(h);
            offs.sort_by(f64::total_cmp);
            offs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
            let mouth = add(self.centre, mul(u[i], self.setbacks[i]));
            for o in offs {
                let p = add(mouth, mul(nrm[i], o));
                out.push([p.x, hgt(i, self.setbacks[i], o), p.z]);
            }
            let c = &self.corners[i];
            let j = c.b;
            let m = c.kerb.len();
            if m == 0 {
                continue;
            }
            let hj = kerb_offset(&self.approaches[j].standard);
            let ha = hgt(i, dot(sub(c.kerb[0], self.centre), u[i]), h);
            let hb = hgt(j, dot(sub(c.kerb[m - 1], self.centre), u[j]), -hj);
            for (k, p) in c.kerb.iter().enumerate() {
                let t = if m > 1 { k as f64 / (m - 1) as f64 } else { 0.0 };
                let s = t * t * (3.0 - 2.0 * t);
                out.push([p.x, ha + (hb - ha) * s, p.z]);
            }
        }
        out
    }

    /// The junction's own meshes over the roads that meet it: `surfaces[i]`
    /// is approach `i`'s road surface ([`LegSurface`]). The box is draped
    /// from each road's mouth to the next; crosswalks and stop bars lie on
    /// their roads' surfaces; each corner sidewalk is a solid (kerb face,
    /// top, and a back that reaches below the ground) whose ends match the
    /// roads' own kerbs and sidewalks. `lift` matches the roads'.
    pub fn meshes_on(&self, surfaces: &[LegSurface], lift: f64) -> RoadMeshes {
        let mut out = RoadMeshes::default();
        let n = self.approaches.len();
        let hgt = |i: usize, d: f64, o: f64| surfaces.get(i).map_or(0.0, |s| s.height(d, o));
        let u: Vec<P2> = self.approaches.iter().map(|a| dir(a.heading)).collect();
        let nrm: Vec<P2> = self.approaches.iter().map(|a| P2::normal(a.heading)).collect();
        // The box: a fan from the centre (it is star-shaped about the centre).
        {
            let boundary = self.draped_boundary(surfaces);
            let centre_h = (0..n).map(|i| hgt(i, 0.0, 0.0)).sum::<f64>() / n.max(1) as f64;
            let m = out.part(Surface::Carriageway);
            let base = m.positions.len() as u32;
            m.positions.push([self.centre.x as f32, (centre_h + lift) as f32, self.centre.z as f32]);
            m.uvs.push([self.centre.x as f32, self.centre.z as f32]);
            m.normals.push([0.0, 1.0, 0.0]);
            for p in &boundary {
                m.positions.push([p[0] as f32, (p[1] + lift) as f32, p[2] as f32]);
                m.uvs.push([p[0] as f32, p[2] as f32]);
                m.normals.push([0.0, 1.0, 0.0]);
            }
            let k = boundary.len() as u32;
            for i in 0..k {
                m.indices.extend([base, base + 1 + (i + 1) % k, base + 1 + i]);
            }
        }
        // A point `d` along approach `i` and `o` across it, on its surface raised `dz`.
        let on = |i: usize, d: f64, o: f64, dz: f64| -> ([f64; 3], [f64; 2]) {
            let p = add(self.centre, add(mul(u[i], d), mul(nrm[i], o)));
            ([p.x, hgt(i, d, o) + lift + dz, p.z], [p.x, p.z])
        };
        // A patch of approach `i`'s surface from `d0` to `d1` along it and
        // across the given offsets, raised `dz`: rows every DRAPE_STEP.
        let patch = |m: &mut MeshData, i: usize, d0: f64, d1: f64, offs: &[f64], dz: f64| {
            let rows = (((d1 - d0) / DRAPE_STEP).ceil() as usize).max(1);
            for w in offs.windows(2) {
                let a: Vec<_> = (0..=rows).map(|k| on(i, d0 + (d1 - d0) * k as f64 / rows as f64, w[0], dz)).collect();
                let b: Vec<_> = (0..=rows).map(|k| on(i, d0 + (d1 - d0) * k as f64 / rows as f64, w[1], dz)).collect();
                m.strip(&a, &b);
            }
        };
        // A vertical face, both sides, from each point of `top` down `depth`.
        let skirt = |m: &mut MeshData, top: &[[f64; 3]], depth: f64| {
            for w in top.windows(2) {
                let (a, b) = (w[0], w[1]);
                m.quad2([[a[0], a[1] - depth, a[2]], [b[0], b[1] - depth, b[2]], b, a]);
            }
        };
        // Offsets across approach `i` between `a` and `b` (exclusive) where its surface bends.
        let breaks = |i: usize, a: f64, b: f64| -> Vec<f64> {
            let (lo, hi) = (a.min(b), a.max(b));
            let mut v: Vec<f64> = surfaces.get(i).map(|s| s.offsets.iter().copied().filter(|&o| o > lo + 1e-6 && o < hi - 1e-6).collect()).unwrap_or_default();
            v.push(lo);
            v.push(hi);
            v.sort_by(f64::total_cmp);
            v
        };
        // Sidewalk corners: a solid behind each return, and along the kerb
        // from the return to the mouth, its section the roads' own.
        for c in &self.corners {
            let (sa, sb) = (&self.approaches[c.a].standard, &self.approaches[c.b].standard);
            if sa.edge != EdgeTreatment::KerbAndSidewalk || sb.edge != EdgeTreatment::KerbAndSidewalk {
                continue;
            }
            let sw = KERB_FACE + sa.sidewalk_width.min(sb.sidewalk_width);
            let (ha, hb) = (kerb_offset(sa), kerb_offset(sb));
            match c.ret {
                Some((r, q)) => {
                    let m = c.kerb.len();
                    let (ta, tb) = (c.kerb[0], c.kerb[m - 1]);
                    let (da, db) = (dot(sub(ta, self.centre), u[c.a]), dot(sub(tb, self.centre), u[c.b]));
                    // Heights across the kerb and sidewalk at each end of the return.
                    let ends = |w: f64| (hgt(c.a, da, ha + w), hgt(c.b, db, -(hb + w)));
                    let ring = |w: f64| -> Vec<[f64; 3]> {
                        let (h0, h1) = ends(w);
                        c.kerb
                            .iter()
                            .enumerate()
                            .map(|(k, p)| {
                                let t = if m > 1 { k as f64 / (m - 1) as f64 } else { 0.0 };
                                let s = t * t * (3.0 - 2.0 * t);
                                let d = sub(*p, q);
                                let at = add(q, mul(d, ((r - w).max(0.0)) / r));
                                [at.x, h0 + (h1 - h0) * s + lift, at.z]
                            })
                            .collect()
                    };
                    let with_uv = |v: &[[f64; 3]]| -> Vec<([f64; 3], [f64; 2])> { v.iter().map(|p| (*p, [p[0], p[2]])).collect() };
                    let (base, top, back) = (ring(0.0), ring(KERB_FACE), ring(sw));
                    out.part(Surface::Kerb).strip(&with_uv(&base), &with_uv(&top));
                    out.part(Surface::Sidewalk).strip(&with_uv(&top), &with_uv(&back));
                    skirt(out.part(Surface::Kerb), &back, SIDEWALK_SKIRT);
                    // Straight pieces from the tangent points out to the mouths.
                    for (idx, d0, side, h) in [(c.a, da, 1.0, ha), (c.b, db, -1.0, hb)] {
                        let d1 = self.setbacks[idx];
                        if d1 - d0 < 0.05 {
                            continue;
                        }
                        let kerb = breaks(idx, side * h, side * (h + KERB_FACE));
                        let walk = breaks(idx, side * (h + KERB_FACE), side * (h + sw));
                        patch(out.part(Surface::Kerb), idx, d0, d1, &kerb, 0.0);
                        patch(out.part(Surface::Sidewalk), idx, d0, d1, &walk, 0.0);
                        let rows = (((d1 - d0) / DRAPE_STEP).ceil() as usize).max(1);
                        let edge: Vec<[f64; 3]> =
                            (0..=rows).map(|k| on(idx, d0 + (d1 - d0) * k as f64 / rows as f64, side * (h + sw), 0.0).0).collect();
                        skirt(out.part(Surface::Kerb), &edge, SIDEWALK_SKIRT);
                    }
                }
                None if c.kerb.len() == 2 => {
                    // A straight kerb (the far side of a T): its sidewalk runs
                    // between the two roads' mouths.
                    let (a, b) = (c.kerb[0], c.kerb[1]);
                    let along = sub(b, a);
                    let back = mul(P2::normal(along.z.atan2(along.x)), -1.0);
                    let (h0, h1) = (|w: f64| hgt(c.a, self.setbacks[c.a], ha + w), |w: f64| hgt(c.b, self.setbacks[c.b], -(hb + w)));
                    let line = |w: f64| -> Vec<([f64; 3], [f64; 2])> {
                        let (pa, pb) = (add(a, mul(back, w)), add(b, mul(back, w)));
                        vec![([pa.x, h0(w) + lift, pa.z], [pa.x, pa.z]), ([pb.x, h1(w) + lift, pb.z], [pb.x, pb.z])]
                    };
                    out.part(Surface::Kerb).strip(&line(0.0), &line(KERB_FACE));
                    out.part(Surface::Sidewalk).strip(&line(KERB_FACE), &line(sw));
                    let edge: Vec<[f64; 3]> = line(sw).iter().map(|(p, _)| *p).collect();
                    skirt(out.part(Surface::Kerb), &edge, SIDEWALK_SKIRT);
                }
                None => {}
            }
        }
        // Crosswalks (continental bars along the road) and stop bars, on the
        // approach roads' own surfaces past each mouth.
        for i in 0..n {
            let s = &self.approaches[i].standard;
            // A crosswalk runs kerb to kerb, across any parking lane or shoulder too.
            let half = kerb_offset(s);
            let d = self.setbacks[i];
            if self.crosswalks[i] {
                let mut o = -half + 0.3;
                while o + 0.6 <= half - 0.3 {
                    let offs = breaks(i, o, o + 0.6);
                    patch(out.part(Surface::MarkingWhite), i, d, d + CROSSWALK_LENGTH, &offs, MARKING_RISE);
                    o += 1.2;
                }
            }
            if let (true, Some((o0, o1))) = (self.stops[i], self.approaches[i].stop_bar_span()) {
                let d0 = d + CROSSWALK_LENGTH + STOP_BAR_GAP;
                let offs = breaks(i, o0, o1);
                patch(out.part(Surface::MarkingWhite), i, d0, d0 + STOP_BAR_WIDTH, &offs, MARKING_RISE);
            }
        }
        for (_, m) in &mut out.parts {
            m.finish_normals();
        }
        out
    }
}

// ============================================================================
// Junctions of a road network
// ============================================================================

/// How close two road ends (or an end and another road's centreline) must
/// come to meet at a junction, metres beyond the other road's half width.
pub const JUNCTION_SNAP: f64 = 12.0;
/// Height apart over which two roads cross at different levels (one bridges
/// the other) rather than meeting, metres.
const JUNCTION_MAX_STEP: f64 = 3.0;
/// Junctions closer than this merge into one, metres.
const JUNCTION_MERGE: f64 = 15.0;
/// A road's own end is this close before a meeting near it counts as at its
/// end rather than along it, metres.
const END_CLEAR: f64 = 15.0;
/// Side of the grid cells the finder buckets segments in, metres.
const NET_CELL: f64 = 32.0;
/// Two roads closer than this to parallel where they meet (radians, either
/// way along) run side by side or merge: they cross nowhere and form no
/// junction, as a near-coincident pair would otherwise cross at every
/// wiggle and give junctions whose corners reach hundreds of metres.
const MIN_MEETING_ANGLE: f64 = 15.0 * PI / 180.0;
/// Two neighbouring legs closer than this (radians) meet with a straight
/// corner instead of a kerb return, whose setback grows as `r / tan(phi / 2)`.
const MIN_RETURN_ANGLE: f64 = 30.0 * PI / 180.0;
/// No junction's box reaches further along a leg than this, metres, whatever
/// its roads: a backstop the design rules above keep it well inside.
pub const MAX_JUNCTION_SETBACK: f64 = 80.0;

/// The point nearest the centrelines of the roads meeting at `met` (each
/// road's line through its station at the meeting, along its heading there),
/// in the least-squares sense: where two roads cross, their crossing. `None`
/// when fewer than two roads meet or their lines run parallel.
fn centre_of_lines(roads: &[(u64, &DesignedRoad)], met: &[(usize, f64)]) -> Option<P2> {
    let (mut axx, mut axz, mut azz, mut bx, mut bz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut seen: Vec<usize> = Vec::new();
    for &(r, s) in met {
        if seen.contains(&r) {
            continue;
        }
        seen.push(r);
        let d = roads[r].1;
        let st = &d.stations[station_at(d, s)];
        let u = dir(st.heading);
        // I - u u^T projects onto the line's normal.
        let (mxx, mxz, mzz) = (1.0 - u.x * u.x, -u.x * u.z, 1.0 - u.z * u.z);
        axx += mxx;
        axz += mxz;
        azz += mzz;
        bx += mxx * st.pos.x + mxz * st.pos.z;
        bz += mxz * st.pos.x + mzz * st.pos.z;
    }
    let det = axx * azz - axz * axz;
    (seen.len() >= 2 && det.abs() > 1e-6).then(|| P2::new((azz * bx - axz * bz) / det, (axx * bz - axz * bx) / det))
}

/// Whether headings `a` and `b` lie within [`MIN_MEETING_ANGLE`] of parallel,
/// either way along.
fn nearly_parallel(a: f64, b: f64) -> bool {
    let d = angle_diff(a, b).abs();
    d < MIN_MEETING_ANGLE || d > PI - MIN_MEETING_ANGLE
}

/// One road's leg into a network junction: road `road`, meeting the junction
/// `s` metres along itself, leaving it at `heading` (radians, outward);
/// `outward` when it leaves toward increasing `s`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    pub road: u64,
    pub s: f64,
    pub heading: f64,
    pub outward: bool,
}

/// A junction of a road network: where it is, its height, the legs that
/// meet there (in the order of [`Junction::approaches`]), its design, and
/// each leg's road surface near it (in the same order), which its own
/// surfaces are draped to.
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkJunction {
    pub centre: P2,
    /// The surface's height at the centre: the mean of the legs' there.
    pub elevation: f64,
    pub legs: Vec<Leg>,
    pub junction: Junction,
    pub surfaces: Vec<LegSurface>,
}

/// What the ground should do at a point near a junction ([`NetworkJunction::ground_target`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundTarget {
    /// The junction's surface there (or at the nearest point it paves).
    pub surface: f64,
    /// Under the junction's paving (sink the bed under it) or beside it
    /// (meet it flush, fading back to the ground).
    pub paved: bool,
    /// Distance beyond the paving, metres (0 under it).
    pub beyond: f64,
}

impl NetworkJunction {
    /// The junction's own surfaces, draped over its roads ([`Junction::meshes_on`]).
    pub fn meshes(&self, lift: f64) -> RoadMeshes {
        self.junction.meshes_on(&self.surfaces, lift)
    }

    /// The triangles the junction paves (box, kerbs and sidewalk tops, no
    /// paint, no vertical faces), at its surface heights.
    pub fn paved_triangles(&self) -> Vec<[[f64; 3]; 3]> {
        let built = self.junction.meshes_on(&self.surfaces, 0.0);
        let mut out = Vec::new();
        for (kind, m) in &built.parts {
            if !matches!(kind, Surface::Carriageway | Surface::Shoulder | Surface::Kerb | Surface::Sidewalk) {
                continue;
            }
            for t in m.indices.chunks_exact(3) {
                let p = |i: u32| {
                    let v = m.positions[i as usize];
                    [v[0] as f64, v[1] as f64, v[2] as f64]
                };
                let tri = [p(t[0]), p(t[1]), p(t[2])];
                let area = ((tri[1][0] - tri[0][0]) * (tri[2][2] - tri[0][2]) - (tri[2][0] - tri[0][0]) * (tri[1][2] - tri[0][2])).abs();
                if area > 1e-6 {
                    out.push(tri);
                }
            }
        }
        out
    }

    /// Whether `p` lies on one of the junction's roads past its mouth, where
    /// that road's own bake shapes the ground.
    pub fn on_a_leg(&self, p: P2) -> bool {
        let j = &self.junction;
        j.approaches.iter().enumerate().any(|(i, a)| {
            let along = dot(sub(p, j.centre), dir(a.heading));
            let across = dot(sub(p, j.centre), P2::normal(a.heading));
            along >= j.setbacks[i] - 1e-3 && across.abs() <= a.standard.formation_half_width() + 0.5
        })
    }
}

/// What the ground should do at `p` near junction `j`, from its paved
/// triangles `tris` (see [`NetworkJunction::paved_triangles`]): `None` on
/// one of its roads past its mouth, where the road's own bake rules.
pub fn ground_target(j: &NetworkJunction, tris: &[[[f64; 3]; 3]], p: P2) -> Option<GroundTarget> {
    let mut best: Option<(f64, f64)> = None; // (distance, height at the nearest point)
    for t in tris {
        // A triangle no nearer than the best found so far cannot change it.
        let (lo_x, hi_x) = (t[0][0].min(t[1][0]).min(t[2][0]), t[0][0].max(t[1][0]).max(t[2][0]));
        let (lo_z, hi_z) = (t[0][2].min(t[1][2]).min(t[2][2]), t[0][2].max(t[1][2]).max(t[2][2]));
        let gap = P2::new((lo_x - p.x).max(p.x - hi_x).max(0.0), (lo_z - p.z).max(p.z - hi_z).max(0.0));
        let lower = len(gap);
        if let Some((d, _)) = best {
            if lower > d || (d == 0.0 && lower > 0.0) {
                continue;
            }
        }
        let (a, b, c) = (P2::new(t[0][0], t[0][2]), P2::new(t[1][0], t[1][2]), P2::new(t[2][0], t[2][2]));
        // Inside: barycentric height.
        let den = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
        if den.abs() > 1e-12 {
            let w0 = ((b.z - c.z) * (p.x - c.x) + (c.x - b.x) * (p.z - c.z)) / den;
            let w1 = ((c.z - a.z) * (p.x - c.x) + (a.x - c.x) * (p.z - c.z)) / den;
            let w2 = 1.0 - w0 - w1;
            if w0 >= -1e-9 && w1 >= -1e-9 && w2 >= -1e-9 {
                let h = w0 * t[0][1] + w1 * t[1][1] + w2 * t[2][1];
                // The highest paving wins (a sidewalk over the box's edge).
                let higher = match best {
                    Some((d, bh)) => d > 0.0 || h > bh,
                    None => true,
                };
                if higher {
                    best = Some((0.0, h));
                }
                continue;
            }
        }
        if matches!(best, Some((d, _)) if d == 0.0) {
            continue;
        }
        for (u, v, hu, hv) in [(a, b, t[0][1], t[1][1]), (b, c, t[1][1], t[2][1]), (c, a, t[2][1], t[0][1])] {
            let uv = sub(v, u);
            let s = (dot(sub(p, u), uv) / dot(uv, uv).max(1e-12)).clamp(0.0, 1.0);
            let d = len(sub(p, add(u, mul(uv, s))));
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, hu + (hv - hu) * s));
            }
        }
    }
    let (beyond, surface) = best?;
    if beyond > 0.0 && j.on_a_leg(p) {
        return None;
    }
    Some(GroundTarget { surface, paved: beyond == 0.0, beyond })
}

/// Station index of road `d` at distance `s` (the nearest).
fn station_at(d: &DesignedRoad, s: f64) -> usize {
    let i = d.stations.partition_point(|st| st.s < s);
    if i == 0 {
        0
    } else if i >= d.stations.len() {
        d.stations.len() - 1
    } else if (d.stations[i].s - s) < (s - d.stations[i - 1].s) {
        i
    } else {
        i - 1
    }
}

/// Where the segments `a0 a1` and `b0 b1` cross: the fractions along each.
fn segments_cross(a0: P2, a1: P2, b0: P2, b1: P2) -> Option<(f64, f64)> {
    let (r, q) = (sub(a1, a0), sub(b1, b0));
    let den = r.x * q.z - r.z * q.x;
    if den.abs() < 1e-12 {
        return None;
    }
    let w = sub(b0, a0);
    let t = (w.x * q.z - w.z * q.x) / den;
    let u = (w.x * r.z - w.z * r.x) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// The junctions of the designed roads `roads` (each with its layer id):
/// - wherever road ends gather within [`JUNCTION_SNAP`] of each other;
/// - wherever a road ends within that of another road's side (a tee);
/// - wherever two roads cross along their lengths (a crossroads).
///
/// Roads more than [`JUNCTION_MAX_STEP`] apart in height there cross at
/// different levels instead and meet nowhere. Two road ends meeting head to
/// head with nothing else are one road continuing and need no junction.
/// Meetings within [`JUNCTION_MERGE`] of each other are one junction.
pub fn find_junctions(roads: &[(u64, &DesignedRoad)]) -> Vec<NetworkJunction> {
    // Segment grid over every road's stations.
    let mut grid: std::collections::BTreeMap<(i64, i64), Vec<(usize, usize)>> = std::collections::BTreeMap::new();
    let cell = |p: P2| ((p.x / NET_CELL).floor() as i64, (p.z / NET_CELL).floor() as i64);
    for (r, (_, d)) in roads.iter().enumerate() {
        for k in 0..d.stations.len().saturating_sub(1) {
            let (a, b) = (d.stations[k].pos, d.stations[k + 1].pos);
            let (c0, c1) = (cell(a), cell(b));
            for x in c0.0.min(c1.0)..=c0.0.max(c1.0) {
                for z in c0.1.min(c1.1)..=c0.1.max(c1.1) {
                    grid.entry((x, z)).or_default().push((r, k));
                }
            }
        }
    }
    let z_at = |r: usize, s: f64| roads[r].1.profile.elevation(s);
    let total = |r: usize| roads[r].1.stations.last().map_or(0.0, |st| st.s);
    // Raw meetings: a point and the (road, s) pairs that meet there.
    let mut meetings: Vec<(P2, Vec<(usize, f64)>)> = Vec::new();
    // Ends near another road's side, and ends near ends.
    for (r, (_, d)) in roads.iter().enumerate() {
        if d.stations.len() < 2 {
            continue;
        }
        for s in [0.0, total(r)] {
            let p = d.stations[station_at(d, s)].pos;
            let (cx, cz) = cell(p);
            let mut met: Vec<(usize, f64)> = vec![(r, s)];
            // The nearest point of each other road within reach: (distance, s).
            let mut nearest: std::collections::BTreeMap<usize, (f64, f64)> = std::collections::BTreeMap::new();
            for x in cx - 1..=cx + 1 {
                for z in cz - 1..=cz + 1 {
                    for &(o, k) in grid.get(&(x, z)).into_iter().flatten() {
                        if o == r {
                            continue;
                        }
                        let od = roads[o].1;
                        let (a, b) = (od.stations[k].pos, od.stations[k + 1].pos);
                        let ab = sub(b, a);
                        let t = (dot(sub(p, a), ab) / dot(ab, ab).max(1e-12)).clamp(0.0, 1.0);
                        let foot = add(a, mul(ab, t));
                        let reach = od.standard.formation_half_width() + JUNCTION_SNAP;
                        if len(sub(p, foot)) > reach {
                            continue;
                        }
                        // Alongside, not into it: no tee.
                        if nearly_parallel(d.stations[station_at(d, s)].heading, ab.z.atan2(ab.x)) {
                            continue;
                        }
                        let os = od.stations[k].s + t * (od.stations[k + 1].s - od.stations[k].s);
                        if (z_at(o, os) - z_at(r, s)).abs() > JUNCTION_MAX_STEP {
                            continue;
                        }
                        let gap = len(sub(p, foot));
                        if nearest.get(&o).is_none_or(|&(best, _)| gap < best) {
                            nearest.insert(o, (gap, os));
                        }
                    }
                }
            }
            for (o, (_, os)) in nearest {
                // Near the other road's end: that end meets this one.
                let os = if os < END_CLEAR {
                    0.0
                } else if total(o) - os < END_CLEAR {
                    total(o)
                } else {
                    os
                };
                met.push((o, os));
            }
            if met.len() >= 2 {
                meetings.push((p, met));
            }
        }
    }
    // Crossings along both roads' lengths.
    for segs in grid.values() {
        for (i, &(r, k)) in segs.iter().enumerate() {
            for &(o, m) in &segs[i + 1..] {
                if o == r {
                    continue;
                }
                let (dr, od) = (roads[r].1, roads[o].1);
                let (ra, rb) = (dr.stations[k].pos, dr.stations[k + 1].pos);
                let (oa, ob) = (od.stations[m].pos, od.stations[m + 1].pos);
                if nearly_parallel((rb.z - ra.z).atan2(rb.x - ra.x), (ob.z - oa.z).atan2(ob.x - oa.x)) {
                    continue;
                }
                let Some((t, u)) =
                    segments_cross(dr.stations[k].pos, dr.stations[k + 1].pos, od.stations[m].pos, od.stations[m + 1].pos)
                else {
                    continue;
                };
                let rs = dr.stations[k].s + t * (dr.stations[k + 1].s - dr.stations[k].s);
                let os = od.stations[m].s + u * (od.stations[m + 1].s - od.stations[m].s);
                if rs < END_CLEAR || total(r) - rs < END_CLEAR || os < END_CLEAR || total(o) - os < END_CLEAR {
                    continue; // an end meeting: found above
                }
                if (z_at(r, rs) - z_at(o, os)).abs() > JUNCTION_MAX_STEP {
                    continue;
                }
                let p = add(dr.stations[k].pos, mul(sub(dr.stations[k + 1].pos, dr.stations[k].pos), t));
                meetings.push((p, vec![(r, rs), (o, os)]));
            }
        }
    }
    // Merge meetings that are one junction.
    let mut merged: Vec<(P2, Vec<(usize, f64)>, usize)> = Vec::new();
    for (p, met) in meetings {
        match merged.iter_mut().find(|(q, _, _)| len(sub(*q, p)) < JUNCTION_MERGE) {
            Some((q, all, n)) => {
                *q = mul(add(mul(*q, *n as f64), p), 1.0 / (*n as f64 + 1.0));
                *n += 1;
                for (r, s) in met {
                    if !all.iter().any(|&(m, ms)| m == r && (ms - s).abs() < END_CLEAR) {
                        all.push((r, s));
                    }
                }
            }
            None => merged.push((p, met, 1)),
        }
    }
    // Legs: an end gives one, a meeting along a road gives two.
    let mut out = Vec::new();
    for (centre, met, _) in merged {
        // The box builds every leg straight out from its centre, so the
        // centre stands where the roads' centrelines meet (the point nearest
        // all their lines, as an end's own position or an average of nearby
        // meetings need not be), and each leg's distance is measured from it
        // along its road: past an end that stops short of the centre, so the
        // road's surface begins exactly at the box's mouth.
        let centre = centre_of_lines(roads, &met).filter(|c| len(sub(*c, centre)) <= 2.0 * JUNCTION_MERGE).unwrap_or(centre);
        let mut legs: Vec<(Leg, RoadStandard)> = Vec::new();
        for (r, s) in met {
            let d = roads[r].1;
            let st = &d.stations[station_at(d, s)];
            let (id, standard) = (roads[r].0, d.standard);
            let end = total(r);
            let at = s + dot(sub(centre, st.pos), dir(st.heading));
            if s <= 1e-9 {
                legs.push((Leg { road: id, s: at.min(END_CLEAR), heading: st.heading, outward: true }, standard));
            } else if (end - s).abs() <= 1e-9 {
                legs.push((Leg { road: id, s: at.max(end - END_CLEAR), heading: st.heading + PI, outward: false }, standard));
            } else {
                legs.push((Leg { road: id, s: at, heading: st.heading, outward: true }, standard));
                legs.push((Leg { road: id, s: at, heading: st.heading + PI, outward: false }, standard));
            }
        }
        // Two roads' legs side by side (a merge left two near-parallel roads
        // in one junction): the higher class keeps its leg, then the road
        // listed first; the other runs on over it without meeting here.
        let rank_of = |leg: &Leg, st: &RoadStandard| (rank(st.class), std::cmp::Reverse(roads.iter().position(|(id, _)| *id == leg.road).unwrap_or(usize::MAX)));
        let mut keep = vec![true; legs.len()];
        for x in 0..legs.len() {
            for y in 0..legs.len() {
                if x == y || !keep[x] || !keep[y] || legs[x].0.road == legs[y].0.road {
                    continue;
                }
                if angle_diff(legs[x].0.heading, legs[y].0.heading).abs() < MIN_MEETING_ANGLE
                    && rank_of(&legs[x].0, &legs[x].1) >= rank_of(&legs[y].0, &legs[y].1)
                {
                    keep[y] = false;
                }
            }
        }
        let mut k = 0;
        legs.retain(|_| {
            k += 1;
            keep[k - 1]
        });
        if legs.len() < 2 {
            continue;
        }
        // Two ends head to head: one road continuing.
        if legs.len() == 2 && angle_diff(legs[0].0.heading, legs[1].0.heading).abs() > PI - 0.35 {
            continue;
        }
        legs.sort_by(|a, b| wrap(a.0.heading).total_cmp(&wrap(b.0.heading)));
        // A one-way road's leg runs out of the junction when it leaves toward
        // increasing distance (the road's own way), in otherwise.
        let approaches: Vec<Approach> = legs
            .iter()
            .map(|(l, st)| Approach {
                heading: l.heading,
                standard: *st,
                flow: match (st.one_way, l.outward) {
                    (false, _) => Flow::Both,
                    (true, true) => Flow::Outbound,
                    (true, false) => Flow::Inbound,
                },
            })
            .collect();
        let Some(junction) = design_junction(centre, &approaches) else { continue };
        // Each leg's surface out past its crosswalk and stop bar.
        let surfaces: Vec<LegSurface> = legs
            .iter()
            .enumerate()
            .map(|(i, (l, _))| {
                let r = roads.iter().position(|(id, _)| *id == l.road).expect("a leg's road");
                let reach = junction.setbacks[i] + CROSSWALK_LENGTH + STOP_BAR_GAP + STOP_BAR_WIDTH + 2.0;
                leg_surface(roads[r].1, l, reach)
            })
            .collect();
        let elevation = surfaces.iter().map(|s| s.height(0.0, 0.0)).sum::<f64>() / surfaces.len() as f64;
        out.push(NetworkJunction { centre, elevation, legs: legs.into_iter().map(|(l, _)| l).collect(), junction, surfaces });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::road_design::{ProfileMode, RoadSpec};
    use super::super::road_mesh::design_spline;

    /// A straight Local street from `a` to `b`, `y` metres up throughout.
    fn street(a: (f64, f64), b: (f64, f64), y: f64) -> DesignedRoad {
        let spec = RoadSpec::new(RoadClass::Local, ProfileMode::Explicit);
        design_spline(&[(P2::new(a.0, a.1), y), (P2::new(b.0, b.1), y)], spec, 0.0, 2.0, 0.0, |_| y).unwrap()
    }

    /// A straight Local street from `a` (at `ya`) to `b` (at `yb`), graded between.
    fn sloped(a: (f64, f64), ya: f64, b: (f64, f64), yb: f64) -> DesignedRoad {
        let spec = RoadSpec::new(RoadClass::Local, ProfileMode::Explicit);
        design_spline(&[(P2::new(a.0, a.1), ya), (P2::new(b.0, b.1), yb)], spec, 0.0, 2.0, 0.0, |_| 0.0).unwrap()
    }

    #[test]
    fn a_junction_centres_on_its_roads_and_each_mouth_sits_on_its_road() {
        // A tee whose stem stops 6 m short of the through street's centre
        // line (within reach of its side): the box must still centre on the
        // through street, and every mouth sit on its own road.
        let through = street((0.0, 0.0), (200.0, 0.0), 10.0);
        let stem = street((100.0, 60.0), (100.0, 6.0), 10.0);
        let roads = [(1u64, &through), (2, &stem)];
        let j = find_junctions(&roads).into_iter().next().expect("a tee");
        assert!(len(sub(j.centre, P2::new(100.0, 0.0))) < 0.1, "the tee centres at {:?}, off its streets' crossing", j.centre);
        for (i, l) in j.legs.iter().enumerate() {
            let d = if l.road == 1 { &through } else { &stem };
            let sb = j.junction.setbacks[i];
            let s_m = if l.outward { l.s + sb } else { l.s - sb };
            let st = &d.stations[station_at(d, s_m.clamp(0.0, d.stations.last().unwrap().s))];
            let a = &j.junction.approaches[i];
            let mouth = add(j.centre, mul(dir(a.heading), sb));
            let off = dot(sub(st.pos, mouth), P2::normal(a.heading));
            assert!(off.abs() < 0.1, "leg {i}'s mouth stands {off:+.2} m off road {}", l.road);
        }
        // The stem's leg starts at the centre, 6 m past its end.
        let stem_leg = j.legs.iter().find(|l| l.road == 2).unwrap();
        assert!((stem_leg.s - 60.0).abs() < 0.1, "the stem's leg starts at s {:.2}, not at the centre", stem_leg.s);
    }

    #[test]
    fn near_coincident_roads_give_no_runaway_junctions() {
        // A street, a second laid almost on it (4 m off at one end, 4 m the
        // other way at the other, crossing it at a shallow angle midway),
        // a third running alongside it and ending beside it, and a cross
        // street through all of them: what a stale copy of a street network
        // left over a new one gave, and froze the bake on.
        let a = street((0.0, 0.0), (600.0, 0.0), 10.0);
        let ghost = street((-20.0, 4.0), (620.0, -4.0), 10.0);
        let beside = street((100.0, 5.0), (500.0, 5.0), 10.0);
        let cross = street((300.0, -200.0), (300.0, 200.0), 10.0);
        let roads = [(1u64, &a), (2, &ghost), (3, &beside), (4, &cross)];
        let js = find_junctions(&roads);
        assert!(!js.is_empty(), "the cross street still meets the others");
        for j in &js {
            for (i, &s) in j.junction.setbacks.iter().enumerate() {
                assert!(s <= MAX_JUNCTION_SETBACK + 1e-9, "a junction at {:?} reaches {s:.0} m along leg {i}", j.centre);
            }
            let far = j.junction.boundary.iter().map(|p| len(sub(*p, j.centre))).fold(0.0, f64::max);
            assert!(far <= MAX_JUNCTION_SETBACK + 30.0, "a junction at {:?} spreads {far:.0} m from its centre", j.centre);
            // No two legs of different roads run side by side.
            for x in 0..j.legs.len() {
                for y in x + 1..j.legs.len() {
                    if j.legs[x].road != j.legs[y].road {
                        let d = angle_diff(j.legs[x].heading, j.legs[y].heading).abs();
                        assert!(d >= MIN_MEETING_ANGLE - 1e-9, "legs of roads {} and {} meet {:.1} degrees apart", j.legs[x].road, j.legs[y].road, d.to_degrees());
                    }
                }
            }
        }
        // The cross street meets the main street at a junction near (300, 0).
        assert!(js.iter().any(|j| len(sub(j.centre, P2::new(300.0, 0.0))) < 20.0 && j.legs.iter().any(|l| l.road == 4) && j.legs.iter().any(|l| l.road == 1)), "the crossroads at (300, 0) is missing");
    }

    /// Two graded streets crossing at (200, 0), both 8 m up there: one rising
    /// 4% east, one falling 3% south to north.
    fn graded_crossing() -> (DesignedRoad, DesignedRoad, NetworkJunction) {
        let a = sloped((0.0, 0.0), 0.0, (400.0, 0.0), 16.0);
        let b = sloped((200.0, -200.0), 14.0, (200.0, 200.0), 2.0);
        let j = find_junctions(&[(1u64, &a), (2, &b)]).into_iter().next().expect("a crossroads");
        (a, b, j)
    }

    /// Approach `i`'s road surface at world `p` (straight roads: s by projection).
    fn road_surface_at(j: &NetworkJunction, roads: [&DesignedRoad; 2], i: usize, p: P2) -> f64 {
        let leg = &j.legs[i];
        let d = roads[(leg.road - 1) as usize];
        let st0 = &d.stations[0];
        let u = dir(st0.heading);
        let s = dot(sub(p, st0.pos), u);
        let o = dot(sub(p, st0.pos), P2::normal(st0.heading));
        surface_at(d, s, o)
    }

    /// The approach `p` lies on past its mouth, if any, with its distance along and across.
    fn approach_of(j: &Junction, p: P2) -> Option<(usize, f64, f64)> {
        (0..j.approaches.len()).find_map(|i| {
            let a = &j.approaches[i];
            let along = dot(sub(p, j.centre), dir(a.heading));
            let across = dot(sub(p, j.centre), P2::normal(a.heading));
            (along >= j.setbacks[i] - 0.01 && across.abs() <= kerb_offset(&a.standard) + 0.01).then_some((i, along, across))
        })
    }

    #[test]
    fn a_one_way_street_flows_one_way_through_a_junction() {
        use super::super::road_design::EdgeChoice;
        // A two-way arterial along x (so the crossing is signalled and every
        // way in stops), a three-lane one-way street running north (increasing z) across it.
        let art = RoadSpec { edge: EdgeChoice::Sidewalk, ..RoadSpec::new(RoadClass::Arterial, ProfileMode::Explicit) };
        let a = design_spline(&[(P2::new(0.0, 0.0), 0.0), (P2::new(400.0, 0.0), 0.0)], art, 0.0, 2.0, 0.0, |_| 0.0).unwrap();
        let spec = RoadSpec { lanes: 3, one_way: true, edge: EdgeChoice::Sidewalk, ..RoadSpec::new(RoadClass::Collector, ProfileMode::Explicit) };
        let b = design_spline(&[(P2::new(200.0, -200.0), 0.0), (P2::new(200.0, 200.0), 0.0)], spec, 10.5, 2.0, 0.0, |_| 0.0).unwrap();
        let j = find_junctions(&[(1u64, &a), (2, &b)]).into_iter().next().expect("a crossroads");
        let jn = &j.junction;
        let of = |road: u64, outward: bool| j.legs.iter().position(|l| l.road == road && l.outward == outward).unwrap();
        let (b_out, b_in) = (of(2, true), of(2, false));
        assert_eq!(jn.approaches[b_out].flow, Flow::Outbound);
        assert_eq!(jn.approaches[b_in].flow, Flow::Inbound);
        // Nothing enters from the leg traffic leaves by; nothing leaves by the leg it comes in on.
        assert!(jn.connections.iter().all(|c| c.from != b_out && c.to != b_in));
        // Three lanes come in, and the through movement keeps all three.
        let through: Vec<_> = jn.connections.iter().filter(|c| c.from == b_in && c.movement == Movement::Through).collect();
        assert_eq!(through.len(), 3, "{:?}", through.iter().map(|c| (c.from_lane, c.to_lane)).collect::<Vec<_>>());
        // Each lane path starts in its own lane on the way in.
        let mut starts: Vec<f64> = through.iter().map(|c| dot(sub(c.path[0], jn.centre), P2::normal(jn.approaches[b_in].heading))).collect();
        starts.sort_by(f64::total_cmp);
        assert!((starts[0] + 3.5).abs() < 1e-6 && starts[1].abs() < 1e-6 && (starts[2] - 3.5).abs() < 1e-6, "{starts:?}");
        // Signalled: the way in stops across its whole width; the way out has no stop bar.
        assert_eq!(jn.control, Control::Signals);
        assert!(jn.stops[b_in] && !jn.stops[b_out]);
        let (o0, o1) = jn.approaches[b_in].stop_bar_span().unwrap();
        assert!(o0 < -5.0 && o1 > 5.0);
    }

    #[test]
    fn the_box_meets_each_graded_road_at_its_own_height_and_crown() {
        let (a, b, j) = graded_crossing();
        let tris = j.paved_triangles();
        let mut worst: f64 = 0.0;
        for i in 0..j.legs.len() {
            let ap = &j.junction.approaches[i];
            let h = kerb_offset(&ap.standard);
            let (u, nrm) = (dir(ap.heading), P2::normal(ap.heading));
            let mut o = -h + 0.05;
            while o <= h - 0.05 {
                // Just inside the box, at the mouth.
                let p = add(j.centre, add(mul(u, j.junction.setbacks[i] - 0.02), mul(nrm, o)));
                let box_h = ground_target(&j, &tris, p).expect("inside the box").surface;
                let road_h = road_surface_at(&j, [&a, &b], i, p);
                worst = worst.max((box_h - road_h).abs());
                o += 0.25;
            }
        }
        assert!(worst < 0.005, "the box steps {worst:.3} m from a road at its mouth");
    }

    #[test]
    fn crosswalks_and_stop_bars_lie_on_their_roads() {
        let (a, b, j) = graded_crossing();
        let built = j.meshes(0.0);
        let paint = built.parts.iter().find(|(k, _)| *k == Surface::MarkingWhite).map(|(_, m)| m).expect("paint");
        assert!(paint.triangle_count() > 0);
        let mut worst: f64 = 0.0;
        for v in &paint.positions {
            let p = P2::new(v[0] as f64, v[2] as f64);
            let (i, _, _) = approach_of(&j.junction, p).expect("paint lies on a road past its mouth");
            let want = road_surface_at(&j, [&a, &b], i, p) + MARKING_RISE;
            worst = worst.max((v[1] as f64 - want).abs());
        }
        assert!(worst < 0.003, "paint sits {worst:.3} m off its road");
        // Each crosswalk reaches from kerb to kerb.
        for (i, ap) in j.junction.approaches.iter().enumerate() {
            let reach = paint
                .positions
                .iter()
                .filter_map(|v| approach_of(&j.junction, P2::new(v[0] as f64, v[2] as f64)))
                .filter(|&(k, along, _)| k == i && along <= j.junction.setbacks[i] + CROSSWALK_LENGTH + 1e-6)
                .map(|(_, _, across)| across.abs())
                .fold(0.0, f64::max);
            assert!(reach >= kerb_offset(&ap.standard) - 0.6, "a crosswalk stops {:.2} m short of the kerb", kerb_offset(&ap.standard) - reach);
        }
        // Every approach of an all-way stop gets its crosswalk and stop bar.
        assert!(j.junction.crosswalks.iter().all(|&c| c) && j.junction.stops.iter().all(|&s| s));
    }

    #[test]
    fn corner_sidewalks_are_solid_and_meet_the_roads_sidewalks() {
        let (a, b, j) = graded_crossing();
        let built = j.meshes(0.0);
        let part = |k: Surface| built.parts.iter().find(|(s, _)| *s == k).map(|(_, m)| m).expect("part");
        let (kerb, walk) = (part(Surface::Kerb), part(Surface::Sidewalk));
        // Vertical faces: the back of every corner reaches below its top.
        let vertical = kerb.indices.chunks_exact(3).filter(|t| {
            let p = |i: u32| kerb.positions[i as usize];
            let (p0, p1, p2) = (p(t[0]), p(t[1]), p(t[2]));
            let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let ny = u[2] * v[0] - u[0] * v[2];
            let l = ((u[1] * v[2] - u[2] * v[1]).powi(2) + ny.powi(2) + (u[0] * v[1] - u[1] * v[0]).powi(2)).sqrt();
            l > 1e-9 && (ny / l).abs() < 1e-3
        });
        assert!(vertical.count() >= 8, "every corner has a back");
        let top = walk.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        let low = kerb.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(top - low > (SIDEWALK_SKIRT * 0.9) as f32, "the back reaches down: {top} to {low}");
        // At each mouth the corner's sidewalk is the road's own: same heights
        // at the kerb top and the sidewalk's back edge.
        let mut worst: f64 = 0.0;
        for i in 0..j.legs.len() {
            let ap = &j.junction.approaches[i];
            let h = kerb_offset(&ap.standard);
            let (u, nrm) = (dir(ap.heading), P2::normal(ap.heading));
            for side in [-1.0, 1.0] {
                for w in [KERB_FACE, KERB_FACE + ap.standard.sidewalk_width] {
                    let p = add(j.centre, add(mul(u, j.junction.setbacks[i]), mul(nrm, side * (h + w))));
                    let near = walk
                        .positions
                        .iter()
                        .filter(|v| ((v[0] as f64 - p.x).powi(2) + (v[2] as f64 - p.z).powi(2)).sqrt() < 1e-3)
                        .map(|v| v[1] as f64)
                        .next()
                        .expect("a sidewalk vertex at the mouth");
                    worst = worst.max((near - road_surface_at(&j, [&a, &b], i, p)).abs());
                }
            }
        }
        assert!(worst < 0.002, "a corner sidewalk steps {worst:.3} m from the road's at a mouth");
    }

    #[test]
    fn the_ground_meets_a_junction_flush_and_leaves_its_roads_alone() {
        let (_, _, j) = graded_crossing();
        let tris = j.paved_triangles();
        // Under the box: paved, at its surface.
        let c = ground_target(&j, &tris, j.centre).expect("the centre");
        assert!(c.paved && c.beyond == 0.0 && (c.surface - j.elevation).abs() < 0.05);
        // Diagonally behind a corner, past its sidewalk: the sidewalk's back, flush.
        let ap = &j.junction.approaches[0];
        let q = j.junction.corners[0].ret.expect("a return").1;
        let out = mul(sub(j.junction.centre, q), -1.0 / len(sub(j.junction.centre, q)));
        let edge_of_paving = (0..400)
            .map(|k| add(j.centre, mul(out, k as f64 * 0.1)))
            .find(|&p| ground_target(&j, &tris, p).is_some_and(|g| !g.paved))
            .expect("past the corner's paving");
        let g = ground_target(&j, &tris, add(edge_of_paving, mul(out, 1.0))).expect("behind the corner");
        assert!(!g.paved && (g.beyond - 1.0).abs() < 0.2, "{g:?}");
        let kerb_rise = ap.standard.kerb_height;
        assert!(g.surface > j.elevation + kerb_rise * 0.5, "flush with the sidewalk, not the road: {g:?}");
        // Past a mouth, on the road: the road's own bake rules.
        let p = add(j.centre, mul(dir(ap.heading), j.junction.setbacks[0] + 3.0));
        assert!(ground_target(&j, &tris, p).is_none());
    }

    #[test]
    fn a_network_meets_at_crossroads_and_tees_never_across_levels() {
        let a = street((0.0, 0.0), (400.0, 0.0), 0.0);
        let b = street((200.0, -200.0), (200.0, 200.0), 0.0); // crosses a at (200, 0)
        let c = street((300.0, 150.0), (300.0, 6.0), 0.0); // ends at a's side
        let d = street((100.0, -200.0), (100.0, 200.0), 20.0); // passes over a
        let e = street((400.0, 0.0), (700.0, 0.0), 0.0); // a continuing
        let roads = [(1u64, &a), (2, &b), (3, &c), (4, &d), (5, &e)];
        let j = find_junctions(&roads);
        let at = |x: f64| j.iter().find(|n| (n.centre.x - x).abs() < 10.0 && n.centre.z.abs() < 10.0);
        let cross = at(200.0).expect("a crossroads at x 200");
        assert_eq!(cross.legs.len(), 4);
        assert!(cross.legs.iter().filter(|l| l.road == 1).count() == 2 && cross.legs.iter().filter(|l| l.road == 2).count() == 2);
        let tee = at(300.0).expect("a tee at x 300");
        assert_eq!(tee.legs.len(), 3);
        // c ends 6 m short of a's centre line: its leg is measured from the
        // junction's centre on that line, 6 m past c's end.
        assert!(tee.legs.iter().any(|l| l.road == 3 && !l.outward && (l.s - 150.0).abs() < 1.0));
        assert!(at(100.0).is_none(), "20 m up, d bridges a and meets nothing");
        assert!(at(400.0).is_none(), "a and e are one road continuing");
        assert_eq!(j.len(), 2);
        // Legs line up with the design's approaches.
        for n in &j {
            assert_eq!(n.legs.len(), n.junction.approaches.len());
            for (l, ap) in n.legs.iter().zip(&n.junction.approaches) {
                assert!(angle_diff(l.heading, ap.heading).abs() < 1e-9);
            }
            assert!(n.elevation.abs() < 1e-6);
        }
    }

    fn urban(class: RoadClass) -> RoadStandard {
        RoadStandard {
            edge: EdgeTreatment::KerbAndSidewalk,
            sidewalk_width: 3.0,
            kerb_height: 0.15,
            shoulder_width: 0.3,
            ..RoadStandard::of(class)
        }
    }

    fn cross(a: RoadStandard, b: RoadStandard) -> Junction {
        let ap = [0.0, PI / 2.0, PI, 3.0 * PI / 2.0]
            .iter()
            .enumerate()
            .map(|(k, &h)| Approach::new(h, if k % 2 == 0 { a } else { b }))
            .collect::<Vec<_>>();
        design_junction(P2::new(0.0, 0.0), &ap).unwrap()
    }

    #[test]
    fn a_crossroads_has_four_returns_and_the_right_control() {
        let j = cross(urban(RoadClass::Arterial), urban(RoadClass::Collector));
        assert_eq!(j.control, Control::Signals);
        assert!(j.stops.iter().all(|&s| s));
        assert_eq!(j.corners.iter().filter(|c| c.ret.is_some()).count(), 4);
        assert!(
            j.corners.iter().all(|c| c.ret.unwrap().0 == 9.0),
            "collector design vehicle"
        );
        assert_eq!(j.signal_poles.len(), 4);
        // Each road stops clear of the other road's kerbs.
        for (i, a) in j.approaches.iter().enumerate() {
            let other = &j.approaches[(i + 1) % 4].standard;
            assert!(
                j.setbacks[i] > kerb_offset(other),
                "{} vs {}",
                j.setbacks[i],
                kerb_offset(other)
            );
            let _ = a;
        }
        let arterial_local = cross(urban(RoadClass::Arterial), urban(RoadClass::Local));
        assert_eq!(arterial_local.control, Control::MinorStop);
        let stopping: Vec<RoadClass> = arterial_local
            .approaches
            .iter()
            .zip(&arterial_local.stops)
            .filter(|(_, s)| **s)
            .map(|(a, _)| a.standard.class)
            .collect();
        assert!(stopping.iter().all(|c| *c == RoadClass::Local) && stopping.len() == 2);
        let locals = cross(urban(RoadClass::Local), urban(RoadClass::Local));
        assert_eq!(locals.control, Control::AllWayStop);
        assert!(locals.corners.iter().all(|c| c.ret.unwrap().0 == 6.0));
    }

    #[test]
    fn returns_are_tangent_to_both_kerbs() {
        let j = cross(urban(RoadClass::Collector), urban(RoadClass::Local));
        for c in &j.corners {
            let (r, q) = c.ret.unwrap();
            for p in &c.kerb {
                assert!((len(sub(*p, q)) - r).abs() < 1e-6);
            }
            // The first point lies on approach a's positive kerb line.
            let a = &j.approaches[c.a];
            let off = dot(sub(c.kerb[0], j.centre), P2::normal(a.heading));
            assert!((off - kerb_offset(&a.standard)).abs() < 1e-6, "{off}");
        }
    }

    #[test]
    fn a_tee_has_a_straight_far_side_and_the_stem_stops() {
        let main = urban(RoadClass::Local);
        let ap = [
            Approach::new(0.0, main),
            Approach::new(PI, main),
            Approach::new(PI / 2.0, main),
        ];
        let j = design_junction(P2::new(0.0, 0.0), &ap).unwrap();
        assert_eq!(j.corners.iter().filter(|c| c.ret.is_none()).count(), 1);
        assert_eq!(j.control, Control::MinorStop);
        let stem = j
            .approaches
            .iter()
            .position(|a| (a.heading - PI / 2.0).abs() < 1e-9)
            .unwrap();
        assert!(j.stops[stem]);
        assert_eq!(j.stops.iter().filter(|&&s| s).count(), 1);
    }

    #[test]
    fn every_movement_gets_a_lane_path() {
        let j = cross(urban(RoadClass::Arterial), urban(RoadClass::Collector));
        // Per approach: 2 through (two lanes each way on the arterial, one on the collector), a left, a right.
        let from0: Vec<&Connection> = j.connections.iter().filter(|c| c.from == 0).collect();
        assert!(from0.iter().any(|c| c.movement == Movement::Left));
        assert!(from0.iter().any(|c| c.movement == Movement::Right));
        assert!(from0.iter().any(|c| c.movement == Movement::Through));
        for c in &j.connections {
            assert_eq!(c.path.len(), 17);
        }
        // A right turn stays near the corner: it is the shortest path.
        let right = from0
            .iter()
            .find(|c| c.movement == Movement::Right)
            .unwrap();
        let left = from0.iter().find(|c| c.movement == Movement::Left).unwrap();
        let plen = |p: &Vec<P2>| p.windows(2).map(|w| len(sub(w[1], w[0]))).sum::<f64>();
        assert!(plen(&right.path) < plen(&left.path));
    }

    #[test]
    fn the_junction_meshes_are_well_formed() {
        let j = cross(urban(RoadClass::Arterial), urban(RoadClass::Local));
        let m = j.meshes(10.0, 0.04);
        for surface in [
            Surface::Carriageway,
            Surface::Sidewalk,
            Surface::MarkingWhite,
        ] {
            let part = &m.parts.iter().find(|(s, _)| *s == surface).unwrap().1;
            assert!(part.triangle_count() > 0, "{surface:?}");
            assert!(part
                .indices
                .iter()
                .all(|&i| (i as usize) < part.positions.len()));
            assert!(
                part.normals.iter().all(|n| n[1] > 0.99),
                "{surface:?} faces up"
            );
            for t in part.indices.chunks_exact(3) {
                let p = |i: u32| part.positions[i as usize];
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                let ny = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
                assert!(ny >= -1e-6, "{surface:?} has a triangle wound to face down");
            }
        }
    }
}
