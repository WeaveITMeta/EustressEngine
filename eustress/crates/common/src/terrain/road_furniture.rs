//! Road furniture: what stands beside and over a road because of how it is
//! designed and how its junctions are controlled. Pure geometry over
//! [`road_junction`](super::road_junction) and [`road_mesh`](super::road_mesh):
//! - **Signals:** at a signalled junction, a mast-arm pole on the far-right
//!   corner of every approach traffic comes in by, its arm reaching over the
//!   lanes that traffic drives into, one signal head over each, facing it.
//! - **Stop signs:** at every other approach that must stop, a stop sign on
//!   a post on the incoming traffic's right, level with its stop bar.
//! - **Streetlights:** along every kerbed road, staggered from side to side,
//!   on the sidewalk, an arm reaching over the road with a lamp shining down;
//!   kept clear of junction boxes.
//!
//! Everything is merged into one mesh per finish, so a road's furniture is a
//! handful of draws however long it is.

use super::road_design::{EdgeTreatment, P2};
use super::road_junction::{kerb_offset, Control, Flow, Junction, LegSurface, CROSSWALK_LENGTH, STOP_BAR_GAP};
use super::road_mesh::{DesignedRoad, MeshData, RoadZones};
use std::f64::consts::PI;

/// What a piece of furniture is made of, for its material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Finish {
    /// Galvanised steel: poles, arms, posts.
    Steel,
    /// A signal head's housing.
    SignalHousing,
    LensRed,
    LensAmber,
    LensGreen,
    /// A stop sign's face.
    SignRed,
    /// A sign's border.
    SignWhite,
    /// A streetlight's luminaire.
    Luminaire,
    /// A street tree's trunk.
    Bark,
    /// A street tree's canopy.
    Foliage,
}

/// A light the furniture carries: where it is and which way it shines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    pub position: [f64; 3],
    /// Unit direction it points.
    pub direction: [f64; 3],
}

/// A road's or a junction's furniture: meshes by finish, and its lamps.
#[derive(Clone, Debug, Default)]
pub struct FurnitureMeshes {
    pub parts: Vec<(Finish, MeshData)>,
    pub lamps: Vec<Lamp>,
}

impl FurnitureMeshes {
    pub fn part(&mut self, finish: Finish) -> &mut MeshData {
        if let Some(i) = self.parts.iter().position(|(f, _)| *f == finish) {
            return &mut self.parts[i].1;
        }
        self.parts.push((finish, MeshData::default()));
        &mut self.parts.last_mut().expect("just pushed").1
    }

    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|(_, m)| m.triangle_count()).sum()
    }
}

/// Height of a signal mast arm, metres.
pub const SIGNAL_ARM_HEIGHT: f64 = 6.4;
/// Lowest point of a signal head over the road (clearance), metres.
pub const SIGNAL_CLEARANCE: f64 = 5.0;
/// Height of a stop sign's lower edge over the ground beside the road, metres.
pub const STOP_SIGN_HEIGHT: f64 = 2.1;
/// Across-corners size of a stop sign, metres.
pub const STOP_SIGN_SIZE: f64 = 0.75;
/// Streetlight pole height, arm reach and spacing along each side, metres.
pub const STREETLIGHT_HEIGHT: f64 = 9.0;
pub const STREETLIGHT_ARM: f64 = 2.2;
pub const STREETLIGHT_SPACING: f64 = 35.0;
/// How far a pole stands back from a kerb's face onto the sidewalk, metres.
const SET_BACK: f64 = 0.6;
/// Clear length kept from a junction's mouth before the first streetlight.
const JUNCTION_CLEAR: f64 = 6.0;

fn rot(yaw: f64, x: f64, z: f64) -> (f64, f64) {
    // Local +x across, +z forward, turned by `yaw` about +y (counter-clockwise from above, x toward z).
    let (c, s) = (yaw.cos(), yaw.sin());
    (x * c - z * s, x * s + z * c)
}

/// A box `size` (x across, y up, z forward) with its base centred at
/// `base`, turned `yaw` about the vertical. Faces outward.
pub fn add_box(m: &mut MeshData, base: [f64; 3], size: [f64; 3], yaw: f64) {
    let (hx, hz) = (size[0] / 2.0, size[2] / 2.0);
    let corner = |sx: f64, y: f64, sz: f64| {
        let (dx, dz) = rot(yaw, sx * hx, sz * hz);
        [base[0] + dx, base[1] + y, base[2] + dz]
    };
    let (b, t) = (0.0, size[1]);
    let quad = |m: &mut MeshData, q: [[f64; 3]; 4]| m.quad(q, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    // Sides (counter-clockwise seen from outside), top, bottom.
    quad(m, [corner(-1.0, b, 1.0), corner(1.0, b, 1.0), corner(1.0, t, 1.0), corner(-1.0, t, 1.0)]);
    quad(m, [corner(1.0, b, -1.0), corner(-1.0, b, -1.0), corner(-1.0, t, -1.0), corner(1.0, t, -1.0)]);
    quad(m, [corner(1.0, b, 1.0), corner(1.0, b, -1.0), corner(1.0, t, -1.0), corner(1.0, t, 1.0)]);
    quad(m, [corner(-1.0, b, -1.0), corner(-1.0, b, 1.0), corner(-1.0, t, 1.0), corner(-1.0, t, -1.0)]);
    quad(m, [corner(-1.0, t, 1.0), corner(1.0, t, 1.0), corner(1.0, t, -1.0), corner(-1.0, t, -1.0)]);
    quad(m, [corner(-1.0, b, -1.0), corner(1.0, b, -1.0), corner(1.0, b, 1.0), corner(-1.0, b, 1.0)]);
}

/// A round pole: an octagonal prism `radius` thick from `base` up `height`.
pub fn add_pole(m: &mut MeshData, base: [f64; 3], radius: f64, height: f64) {
    let k = 8;
    let at = |i: usize, y: f64| {
        let a = 2.0 * PI * i as f64 / k as f64;
        [base[0] + radius * a.cos(), base[1] + y, base[2] + radius * a.sin()]
    };
    for i in 0..k {
        let j = (i + 1) % k;
        // Outward: counter-clockwise seen from outside.
        m.quad([at(j, 0.0), at(i, 0.0), at(i, height), at(j, height)], [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    }
}

/// The yaw that turns a part's local +z (its front) toward world direction `v`.
pub fn yaw_toward(v: P2) -> f64 {
    (-v.x).atan2(v.z)
}

/// A flat octagon `size` across corners, centred `dz` in front of
/// `centre` along the direction `yaw` faces, seen from that side (its
/// front faces local +z).
fn add_octagon(m: &mut MeshData, centre: [f64; 3], size: f64, yaw: f64, dz: f64) {
    let r = size / 2.0;
    let k = 8;
    let place = |x: f64, y: f64| {
        let (dx, ddz) = rot(yaw, x, dz);
        [centre[0] + dx, centre[1] + y, centre[2] + ddz]
    };
    let c = place(0.0, 0.0);
    let base = m.positions.len() as u32;
    let (fx, fz) = rot(yaw, 0.0, 1.0);
    let normal = [fx as f32, 0.0, fz as f32];
    for p in std::iter::once(c).chain((0..k).map(|i| {
        let a = 2.0 * PI * (i as f64 + 0.5) / k as f64;
        // Counter-clockwise seen from in front (local +z): x right, y up.
        place(r * a.cos(), r * a.sin())
    })) {
        m.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        m.uvs.push([0.0, 0.0]);
        m.normals.push(normal);
    }
    for i in 0..k as u32 {
        m.indices.extend([base, base + 1 + i, base + 1 + (i + 1) % k as u32]);
    }
}

/// A signal head hanging with its top at `top`, facing `yaw`: housing and
/// red, amber and green lenses top to bottom.
fn add_signal_head(f: &mut FurnitureMeshes, top: [f64; 3], yaw: f64) {
    let (w, h, d) = (0.36, 1.05, 0.28);
    add_box(f.part(Finish::SignalHousing), [top[0], top[1] - h, top[2]], [w, h, d], yaw);
    for (k, finish) in [Finish::LensRed, Finish::LensAmber, Finish::LensGreen].into_iter().enumerate() {
        let y = top[1] - 0.18 - 0.34 * k as f64;
        add_octagon(f.part(finish), [top[0], y, top[2]], 0.24, yaw, d / 2.0 + 0.004);
    }
}

/// The furniture of junction `j` over its roads' `surfaces`
/// ([`LegSurface`]): signals when signalled, stop signs at the stop bars
/// otherwise.
pub fn junction_furniture(j: &Junction, surfaces: &[LegSurface]) -> FurnitureMeshes {
    let mut f = FurnitureMeshes::default();
    let n = j.approaches.len();
    let hgt = |i: usize, d: f64, o: f64| surfaces.get(i).map_or(0.0, |s| s.height(d, o));
    let dir = |h: f64| P2::new(h.cos(), h.sin());
    let at = |p: P2, y: f64| [p.x, y, p.z];
    for i in 0..n {
        let a = &j.approaches[i];
        if a.flow == Flow::Outbound || a.lanes_in() == 0 || !j.stops[i] && j.control != Control::Signals {
            continue;
        }
        let (u, nrm) = (dir(a.heading), P2::normal(a.heading));
        // Traffic comes in along -u; signs and heads face it (+u).
        // Its right is the approach's negative side; beyond the kerb's face
        // onto the sidewalk, or beyond the shoulder's edge.
        let kerbed = a.standard.edge == EdgeTreatment::KerbAndSidewalk;
        let side = kerb_offset(&a.standard) + if kerbed { 0.15 + SET_BACK } else { 1.0 };
        match j.control {
            Control::Signals => {
                // The road traffic drives on into: the approach nearest the opposite heading.
                let opposite = (0..n)
                    .filter(|&k| k != i)
                    .max_by(|&p, &q| {
                        let c = |k: usize| (j.approaches[k].heading - (a.heading + PI)).cos();
                        c(p).total_cmp(&c(q))
                    })
                    .filter(|&k| (j.approaches[k].heading - (a.heading + PI)).cos() > 0.7);
                // The pole, the way its arm reaches, and how far out along the arm each head hangs.
                let (pole, arm, heads): (P2, P2, Vec<f64>) = match opposite {
                    Some(k) => {
                        // Far side, on the right of travel into it (its positive side).
                        let b = &j.approaches[k];
                        let (uk, nk) = (dir(b.heading), P2::normal(b.heading));
                        let kerbed_k = b.standard.edge == EdgeTreatment::KerbAndSidewalk;
                        let off = kerb_offset(&b.standard) + if kerbed_k { 0.15 + SET_BACK } else { 1.0 };
                        let d = j.setbacks[k] + 1.0;
                        let p = P2::new(j.centre.x + uk.x * d + nk.x * off, j.centre.z + uk.z * d + nk.z * off);
                        let count = a.lanes_in().min(b.lanes_out()).max(1);
                        let heads = (0..b.lanes_out()).rev().take(count as usize).map(|q| off - b.lane_out_offset(q)).collect();
                        (p, P2::new(-nk.x, -nk.z), heads)
                    }
                    None => {
                        // The stem of a T: the near-right corner, its arm over the lanes coming in.
                        let d = j.setbacks[i] + CROSSWALK_LENGTH + STOP_BAR_GAP;
                        let p = P2::new(j.centre.x + u.x * d - nrm.x * side, j.centre.z + u.z * d - nrm.z * side);
                        let heads = (0..a.lanes_in()).map(|q| side + a.lane_in_offset(q)).collect();
                        (p, P2::new(nrm.x, nrm.z), heads)
                    }
                };
                let leg = opposite.unwrap_or(i);
                let lp = j.approaches[leg].heading;
                let along = (pole.x - j.centre.x) * lp.cos() + (pole.z - j.centre.z) * lp.sin();
                let across = -(pole.x - j.centre.x) * lp.sin() + (pole.z - j.centre.z) * lp.cos();
                let ground = hgt(leg, along, across.signum() * (kerb_offset(&j.approaches[leg].standard) + 0.15));
                let base = at(pole, ground);
                add_pole(f.part(Finish::Steel), base, 0.16, SIGNAL_ARM_HEIGHT + 0.4);
                let reach = heads.iter().copied().fold(2.0, f64::max) + 0.6;
                let mid = [base[0] + arm.x * reach / 2.0, ground + SIGNAL_ARM_HEIGHT, base[2] + arm.z * reach / 2.0];
                add_box(f.part(Finish::Steel), mid, [0.14, 0.18, reach], yaw_toward(arm));
                for h in heads {
                    let top = [base[0] + arm.x * h, ground + SIGNAL_ARM_HEIGHT, base[2] + arm.z * h];
                    add_signal_head(&mut f, top, yaw_toward(u));
                }
            }
            Control::AllWayStop | Control::MinorStop => {
                let d = j.setbacks[i] + CROSSWALK_LENGTH + STOP_BAR_GAP;
                let p = P2::new(j.centre.x + u.x * d - nrm.x * side, j.centre.z + u.z * d - nrm.z * side);
                let base = hgt(i, d, -(kerb_offset(&a.standard) + if kerbed { 0.15 } else { 0.0 }));
                add_pole(f.part(Finish::Steel), at(p, base), 0.04, STOP_SIGN_HEIGHT + STOP_SIGN_SIZE);
                let centre = [p.x, base + STOP_SIGN_HEIGHT + STOP_SIGN_SIZE / 2.0, p.z];
                // Red face inside a white border toward the traffic, a steel back away from it.
                add_octagon(f.part(Finish::SignWhite), centre, STOP_SIGN_SIZE, yaw_toward(u), 0.05);
                add_octagon(f.part(Finish::SignRed), centre, STOP_SIGN_SIZE * 0.9, yaw_toward(u), 0.056);
                add_octagon(f.part(Finish::Steel), centre, STOP_SIGN_SIZE, yaw_toward(P2::new(-u.x, -u.z)), -0.044);
            }
        }
    }
    for (_, m) in &mut f.parts {
        finish_outward(m);
    }
    f
}

/// Streetlights along kerbed road `d`, its meshes lifted `lift`, kept out of
/// `zones`' gaps (junction boxes) and clear of them by [`JUNCTION_CLEAR`]:
/// one every [`STREETLIGHT_SPACING`] on each side, the sides staggered.
pub fn road_streetlights(d: &DesignedRoad, zones: &RoadZones, lift: f64) -> FurnitureMeshes {
    let mut f = FurnitureMeshes::default();
    let s = &d.standard;
    if s.edge != EdgeTreatment::KerbAndSidewalk || d.stations.len() < 2 {
        return f;
    }
    let total = d.stations[d.stations.len() - 1].s;
    let clear = |at: f64| zones.gaps.iter().all(|&(a, b)| at < a.min(b) - JUNCTION_CLEAR || at > a.max(b) + JUNCTION_CLEAR);
    let off = kerb_offset(s) + 0.15 + SET_BACK;
    for (side, start) in [(1.0, STREETLIGHT_SPACING / 4.0), (-1.0, STREETLIGHT_SPACING * 3.0 / 4.0)] {
        let mut at = start;
        while at < total - 2.0 {
            if clear(at) {
                let i = d.stations.partition_point(|st| st.s < at).clamp(1, d.stations.len() - 1);
                let (a, b) = (&d.stations[i - 1], &d.stations[i]);
                let t = ((at - a.s) / (b.s - a.s).max(1e-9)).clamp(0.0, 1.0);
                let pos = P2::new(a.pos.x + (b.pos.x - a.pos.x) * t, a.pos.z + (b.pos.z - a.pos.z) * t);
                let heading = a.heading + (b.heading - a.heading) * t;
                let nrm = P2::normal(heading);
                let base_p = P2::new(pos.x + nrm.x * side * off, pos.z + nrm.z * side * off);
                let ground = super::road_junction::surface_at(d, at, side * (kerb_offset(s) + 0.15 + SET_BACK)) + lift;
                let base = [base_p.x, ground, base_p.z];
                add_pole(f.part(Finish::Steel), base, 0.11, STREETLIGHT_HEIGHT);
                // The arm reaches back over the road.
                let inward = P2::new(-nrm.x * side, -nrm.z * side);
                let arm_mid = [base[0] + inward.x * STREETLIGHT_ARM / 2.0, ground + STREETLIGHT_HEIGHT - 0.1, base[2] + inward.z * STREETLIGHT_ARM / 2.0];
                let yaw = yaw_toward(inward);
                add_box(f.part(Finish::Steel), arm_mid, [0.08, 0.1, STREETLIGHT_ARM], yaw);
                let head = [base[0] + inward.x * STREETLIGHT_ARM, ground + STREETLIGHT_HEIGHT - 0.25, base[2] + inward.z * STREETLIGHT_ARM];
                add_box(f.part(Finish::Luminaire), head, [0.32, 0.16, 0.7], yaw);
                f.lamps.push(Lamp { position: [head[0], head[1] - 0.02, head[2]], direction: [inward.x * 0.25, -0.968, inward.z * 0.25] });
            }
            at += STREETLIGHT_SPACING;
        }
    }
    for (_, m) in &mut f.parts {
        finish_outward(m);
    }
    f
}

/// Street trees along kerbed road `d` whose sidewalks are at least
/// [`TREE_MIN_SIDEWALK`] wide: one every [`TREE_SPACING`] on each side, in a pit
/// a metre in from the kerb, clear of junction boxes (`zones`' gaps) and of
/// the streetlights' own places.
pub fn road_trees(d: &DesignedRoad, zones: &RoadZones, lift: f64) -> FurnitureMeshes {
    let mut f = FurnitureMeshes::default();
    let s = &d.standard;
    if s.edge != EdgeTreatment::KerbAndSidewalk || s.sidewalk_width < TREE_MIN_SIDEWALK || d.stations.len() < 2 {
        return f;
    }
    let total = d.stations[d.stations.len() - 1].s;
    let clear = |at: f64| zones.gaps.iter().all(|&(a, b)| at < a.min(b) - JUNCTION_CLEAR - 4.0 || at > a.max(b) + JUNCTION_CLEAR + 4.0);
    let off = kerb_offset(s) + 0.15 + 1.0;
    for (side, light_start) in [(1.0, STREETLIGHT_SPACING / 4.0), (-1.0, STREETLIGHT_SPACING * 3.0 / 4.0)] {
        let mut at = TREE_SPACING / 2.0;
        while at < total - 2.0 {
            // Clear of this side's streetlights.
            let from_light = ((at - light_start).rem_euclid(STREETLIGHT_SPACING)).min(STREETLIGHT_SPACING - (at - light_start).rem_euclid(STREETLIGHT_SPACING));
            if clear(at) && from_light > 3.0 {
                let i = d.stations.partition_point(|st| st.s < at).clamp(1, d.stations.len() - 1);
                let (a, b) = (&d.stations[i - 1], &d.stations[i]);
                let t = ((at - a.s) / (b.s - a.s).max(1e-9)).clamp(0.0, 1.0);
                let pos = P2::new(a.pos.x + (b.pos.x - a.pos.x) * t, a.pos.z + (b.pos.z - a.pos.z) * t);
                let heading = a.heading + (b.heading - a.heading) * t;
                let nrm = P2::normal(heading);
                let p = P2::new(pos.x + nrm.x * side * off, pos.z + nrm.z * side * off);
                let ground = super::road_junction::surface_at(d, at, side * off) + lift;
                add_tree(&mut f, [p.x, ground, p.z], (at * 7.3).sin());
            }
            at += TREE_SPACING;
        }
    }
    for (_, m) in &mut f.parts {
        finish_outward(m);
    }
    f
}

/// Spacing of street trees along each side, metres.
pub const TREE_SPACING: f64 = 12.0;
/// Narrowest sidewalk that takes a row of trees, metres.
pub const TREE_MIN_SIDEWALK: f64 = 3.0;

/// A low-poly street tree standing at `base`: a trunk and a canopy of two
/// stacked octagonal crowns, its size varied by `vary` (-1 to 1).
fn add_tree(f: &mut FurnitureMeshes, base: [f64; 3], vary: f64) {
    let scale = 1.0 + 0.15 * vary;
    add_pole(f.part(Finish::Bark), base, 0.16 * scale, 2.6 * scale);
    let crown = |m: &mut MeshData, y0: f64, r0: f64, y1: f64, r1: f64| {
        let k = 8;
        let ring = |y: f64, r: f64, i: usize| {
            let a = 2.0 * PI * (i as f64 + 0.5) / k as f64;
            [base[0] + r * a.cos(), base[1] + y, base[2] + r * a.sin()]
        };
        for i in 0..k {
            let j = (i + 1) % k;
            m.quad([ring(y0, r0, j), ring(y0, r0, i), ring(y1, r1, i), ring(y1, r1, j)], [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        }
        // Caps: a fan top and bottom.
        let (top, bottom) = ([base[0], base[1] + y1, base[2]], [base[0], base[1] + y0, base[2]]);
        for i in 0..k {
            let j = (i + 1) % k;
            if r1 > 1e-6 {
                m.quad([top, ring(y1, r1, j), ring(y1, r1, i), ring(y1, r1, i)], [[0.5, 0.5], [0.0, 0.0], [1.0, 0.0], [1.0, 0.0]]);
            }
            m.quad([bottom, ring(y0, r0, i), ring(y0, r0, j), ring(y0, r0, j)], [[0.5, 0.5], [0.0, 0.0], [1.0, 0.0], [1.0, 0.0]]);
        }
    };
    let m = f.part(Finish::Foliage);
    crown(m, 2.4 * scale, 1.6 * scale, 4.4 * scale, 2.5 * scale);
    crown(m, 4.4 * scale, 2.5 * scale, 7.0 * scale, 0.6 * scale);
}

/// Average each vertex's normal from its triangles as wound (furniture faces
/// every way, unlike a road surface, so nothing is turned up).
fn finish_outward(m: &mut MeshData) {
    let mut acc = vec![[0.0f64; 3]; m.positions.len()];
    for t in m.indices.chunks_exact(3) {
        let p = |i: u32| {
            let v = m.positions[i as usize];
            [v[0] as f64, v[1] as f64, v[2] as f64]
        };
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for k in 0..3 {
                acc[i as usize][k] += n[k];
            }
        }
    }
    for (dst, n) in m.normals.iter_mut().zip(acc) {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l > 1e-12 {
            *dst = [(n[0] / l) as f32, (n[1] / l) as f32, (n[2] / l) as f32];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::road_design::{EdgeChoice, ProfileMode, RoadClass, RoadSpec};
    use super::super::road_junction::{find_junctions, NetworkJunction};
    use super::super::road_mesh::design_spline;
    use super::*;

    fn road(spec: RoadSpec, a: (f64, f64), b: (f64, f64)) -> DesignedRoad {
        design_spline(&[(P2::new(a.0, a.1), 10.0), (P2::new(b.0, b.1), 10.0)], spec, 0.0, 2.0, 4.0, |_| 10.0).unwrap()
    }

    fn urban(class: RoadClass) -> RoadSpec {
        RoadSpec { edge: EdgeChoice::Sidewalk, ..RoadSpec::new(class, ProfileMode::Explicit) }
    }

    /// The facing directions (xz) of every fan in `m` (each lens or sign face).
    fn faces(m: &MeshData) -> Vec<(P2, [f32; 3])> {
        m.indices
            .chunks_exact(3)
            .step_by(8)
            .map(|t| {
                let n = m.normals[t[0] as usize];
                let c = m.positions[t[0] as usize];
                (P2::new(n[0] as f64, n[2] as f64), c)
            })
            .collect()
    }

    fn crossing(a: RoadSpec, b: RoadSpec) -> NetworkJunction {
        let ra = road(a, (0.0, 0.0), (300.0, 0.0));
        let rb = road(b, (150.0, -150.0), (150.0, 150.0));
        find_junctions(&[(1, &ra), (2, &rb)]).into_iter().next().expect("a crossroads")
    }

    #[test]
    fn signal_heads_hang_over_the_far_lanes_and_face_their_traffic() {
        let j = crossing(urban(RoadClass::Arterial), urban(RoadClass::Collector));
        assert_eq!(j.junction.control, Control::Signals);
        let f = junction_furniture(&j.junction, &j.surfaces);
        let part = |k: Finish| f.parts.iter().find(|(p, _)| *p == k).map(|(_, m)| m.clone()).unwrap_or_default();
        let lenses = faces(&part(Finish::LensRed));
        let lanes_in: u32 = j.junction.approaches.iter().map(|a| a.lanes_in()).sum();
        assert_eq!(lenses.len() as u32, lanes_in, "a head over every lane coming in");
        for a in &j.junction.approaches {
            let u = P2::new(a.heading.cos(), a.heading.sin());
            let facing = lenses.iter().filter(|(n, _)| n.x * u.x + n.z * u.z > 0.99).count() as u32;
            assert_eq!(facing, a.lanes_in(), "heads face the traffic coming in along heading {:.2}", a.heading);
            // Each of those heads hangs over the far road's carriageway.
            let k = j.junction.approaches.iter().position(|b| (b.heading - (a.heading + PI)).cos() > 0.99).unwrap();
            let b = &j.junction.approaches[k];
            for (_, c) in lenses.iter().filter(|(n, _)| n.x * u.x + n.z * u.z > 0.99) {
                let rel = P2::new(c[0] as f64 - j.junction.centre.x, c[2] as f64 - j.junction.centre.z);
                let across = -rel.x * b.heading.sin() + rel.z * b.heading.cos();
                assert!(across > 0.0 && across < b.standard.carriageway_width() / 2.0, "over a lane traffic drives into: {across:.2}");
            }
        }
        // Clearance: no housing lower than 5 m over the road.
        let low = part(Finish::SignalHousing).positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min) as f64;
        assert!(low - j.elevation >= SIGNAL_CLEARANCE - 0.05, "heads {:.2} m over the road", low - j.elevation);
        assert!(part(Finish::Steel).triangle_count() > 0);
    }

    #[test]
    fn stop_signs_stand_on_the_right_level_with_the_stop_bars() {
        let j = crossing(urban(RoadClass::Local), urban(RoadClass::Local));
        assert_eq!(j.junction.control, Control::AllWayStop);
        let f = junction_furniture(&j.junction, &j.surfaces);
        let red = f.parts.iter().find(|(p, _)| *p == Finish::SignRed).map(|(_, m)| m.clone()).unwrap();
        let signs = faces(&red);
        assert_eq!(signs.len(), 4);
        for (i, a) in j.junction.approaches.iter().enumerate() {
            let u = P2::new(a.heading.cos(), a.heading.sin());
            let (_, c) = signs.iter().find(|(n, _)| n.x * u.x + n.z * u.z > 0.99).expect("a sign facing each approach");
            let rel = P2::new(c[0] as f64 - j.junction.centre.x, c[2] as f64 - j.junction.centre.z);
            let along = rel.x * u.x + rel.z * u.z;
            let across = -rel.x * a.heading.sin() + rel.z * a.heading.cos();
            assert!((along - (j.junction.setbacks[i] + CROSSWALK_LENGTH + STOP_BAR_GAP)).abs() < 0.2, "level with the stop bar");
            assert!(across < -kerb_offset(&a.standard), "on the incoming traffic's right, off the road: {across:.2}");
        }
    }

    #[test]
    fn street_trees_line_wide_sidewalks_between_the_lights() {
        let d = road(urban(RoadClass::Arterial), (0.0, 0.0), (300.0, 0.0));
        let zones = RoadZones { gaps: vec![(140.0, 160.0)], ..RoadZones::default() };
        let trees = road_trees(&d, &zones, 0.02);
        let lights = road_streetlights(&d, &zones, 0.02);
        let trunk = trees.parts.iter().find(|(f, _)| *f == Finish::Bark).map(|(_, m)| m.clone()).expect("trunks");
        // Each trunk is 8 quads of 4 vertices.
        let count = trunk.positions.len() / (8 * 4);
        assert!((30..=48).contains(&count), "about one every 12 m each side: {count}");
        for chunk in trunk.positions.chunks(8 * 4) {
            let x = chunk.iter().map(|p| p[0] as f64).sum::<f64>() / chunk.len() as f64;
            assert!(!(130.0..=170.0).contains(&x), "a tree in the junction at x {x:.1}");
            for l in &lights.lamps {
                let same_side = (l.position[2] > 0.0) == (chunk[0][2] > 0.0);
                assert!(!same_side || (l.position[0] - x).abs() > 2.9, "a tree on a streetlight at x {x:.1}");
            }
        }
        // A Local street's 2 m sidewalks take none.
        let local = road(urban(RoadClass::Local), (0.0, 0.0), (300.0, 0.0));
        assert_eq!(road_trees(&local, &RoadZones::default(), 0.02).triangle_count(), 0);
    }

    #[test]
    fn streetlights_stagger_along_kerbed_roads_clear_of_junctions() {
        let d = road(urban(RoadClass::Local), (0.0, 0.0), (300.0, 0.0));
        let zones = RoadZones { gaps: vec![(140.0, 160.0)], ..RoadZones::default() };
        let f = road_streetlights(&d, &zones, 0.02);
        let n = f.lamps.len();
        assert!((14..=18).contains(&n), "about one every 35 m each side: {n}");
        let half = kerb_offset(&d.standard);
        let mut sides = (0, 0);
        for l in &f.lamps {
            let (x, z) = (l.position[0], l.position[2]);
            assert!(!(140.0 - 6.0..=160.0 + 6.0).contains(&x), "a lamp in the junction at x {x:.1}");
            assert!(z.abs() < half, "the arm reaches over the road: {z:.2}");
            assert!(l.direction[1] < -0.9, "it shines down");
            if z > 0.0 { sides.0 += 1 } else { sides.1 += 1 }
        }
        assert!(sides.0 > 4 && sides.1 > 4 && (sides.0 as i32 - sides.1 as i32).abs() <= 1, "both sides: {sides:?}");
        // A road without kerbs has none.
        let rural = road(RoadSpec::new(RoadClass::Rural, ProfileMode::Explicit), (0.0, 0.0), (300.0, 0.0));
        assert!(road_streetlights(&rural, &RoadZones::default(), 0.02).lamps.is_empty());
    }
}
