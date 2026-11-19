//! What `Road:Audit` reports: where a designed road and the junctions it
//! meets break the rules they were built to, measured on the very meshes
//! the engine draws, so a network can be worked against numbers instead
//! of captures.
//!
//! Per road:
//! - **Violations:** curves under the class's minimum radius, grades over its
//!   maximum, and fills and cuts past their limits where no bridge or tunnel
//!   carries the road ([`road_design::audit`](super::road_design::audit)).
//! - **Structures:** each bridge and tunnel, where it starts and ends.
//!
//! Per junction the road meets, and per leg of it:
//! - **Mouth step:** the height between the leg's own surface just past the
//!   box's mouth and the box's just inside it, worst across the leg from kerb
//!   to kerb. The box is draped to its roads, so anything over a few
//!   millimetres is a defect.
//! - **Lanes with no exit:** incoming lanes no lane path leaves the box from.
//! - **Close junctions:** other junctions whose centres stand within
//!   [`MIN_JUNCTION_SPACING`] of this one, less than a usable block apart
//!   (the junction finder merges two a road joins, so these share no road).
//!
//! Pure math over the designs and junctions the bake prepared, no ECS.

use serde::Serialize;

use super::road_design::{Violation, P2};
use super::road_junction::{kerb_offset, Control, NetworkJunction, MIN_JUNCTION_SPACING};
use super::road_mesh::{build_designed_zoned, DesignedRoad, MeshData, StructureKind, Surface};
use super::road_surface::junction_zones;

/// How far either side of a mouth its two surfaces are read, metres.
const MOUTH_PROBE: f64 = 0.05;
/// Spacing of the readings across a leg at its mouth, metres.
const MOUTH_PROBE_STEP: f64 = 0.25;
/// Reach around a mouth whose road triangles are kept for its readings, metres.
const MOUTH_REACH: f64 = 30.0;

/// One road's audit.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RoadAudit {
    /// The road's layer id.
    pub road: u64,
    pub class: &'static str,
    /// Length along its alignment, metres.
    pub length: f64,
    pub violations: Vec<ViolationReport>,
    pub structures: Vec<StructureReport>,
    /// Every junction one of its legs meets, in the order the bake found them.
    pub junctions: Vec<JunctionAudit>,
}

/// One way the road breaks its standard.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ViolationReport {
    /// `Radius`, `Grade`, `Fill` or `Cut`.
    pub kind: &'static str,
    /// The control point whose curve it is (`Radius` only).
    pub point: Option<usize>,
    /// The stretch it runs over, metres along the road (all but `Radius`).
    pub from: Option<f64>,
    pub to: Option<f64>,
    /// The worst value found and the limit it breaks: metres for `Radius`,
    /// `Fill` and `Cut`, a fraction (rise over run) for `Grade`.
    pub value: f64,
    pub limit: f64,
}

/// One bridge or tunnel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StructureReport {
    pub kind: &'static str,
    /// Where it starts and ends, metres along the road.
    pub from: f64,
    pub to: f64,
}

/// One junction the road meets.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JunctionAudit {
    /// World X and Z of its centre, metres.
    pub centre: [f64; 2],
    /// Its surface's height at the centre, metres.
    pub elevation: f64,
    /// `Signals`, `AllWayStop` or `MinorStop`.
    pub control: &'static str,
    pub legs: Vec<LegAudit>,
    /// Other junctions within [`MIN_JUNCTION_SPACING`] of it.
    pub close: Vec<CloseJunction>,
    /// The worst mouth step of any of its legs, metres (absolute).
    pub worst_mouth_step: f64,
    /// Incoming lanes over all its legs that have no exit.
    pub lanes_without_exit: u32,
}

/// One leg of a junction.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LegAudit {
    /// The leg's road (its layer id) and class.
    pub road: u64,
    pub class: &'static str,
    /// Where the leg meets the junction, metres along its road, and whether
    /// it leaves toward increasing distance.
    pub station: f64,
    pub outward: bool,
    /// Its heading away from the junction, degrees clockwise from +X toward +Z.
    pub heading_deg: f64,
    /// Distance from the junction's centre to its mouth, metres.
    pub setback: f64,
    pub lanes_in: u32,
    pub lanes_out: u32,
    /// Incoming lanes (counted from the driver's left) no lane path leaves from.
    pub lanes_without_exit: Vec<u32>,
    /// Whether its traffic stops (a stop bar), and whether a crosswalk crosses it.
    pub stops: bool,
    pub crosswalk: bool,
    /// Its surface just past the mouth less the box's just inside it, at the
    /// offset across the leg (positive to the right of its outward heading)
    /// where that is worst, metres. `None` where either surface could not be
    /// read across the whole leg.
    pub mouth_step: Option<f64>,
    pub mouth_step_offset: Option<f64>,
}

/// A junction standing close to another.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CloseJunction {
    pub centre: [f64; 2],
    /// Centre to centre, metres.
    pub distance: f64,
}

fn control_name(c: Control) -> &'static str {
    match c {
        Control::Signals => "Signals",
        Control::AllWayStop => "AllWayStop",
        Control::MinorStop => "MinorStop",
    }
}

/// Triangles of `m` (world positions), only those with a corner within
/// `reach` of `near` when given.
fn triangles(m: &MeshData, near: Option<(P2, f64)>) -> Vec<[[f64; 3]; 3]> {
    m.indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]].map(|i| m.positions[i as usize].map(f64::from)))
        .filter(|t| {
            near.is_none_or(|(c, r)| t.iter().any(|p| (p[0] - c.x).hypot(p[2] - c.z) <= r))
        })
        .collect()
}

/// The highest of `tris` over world point `p`, if any covers it.
fn height_on(tris: &[[[f64; 3]; 3]], p: P2) -> Option<f64> {
    let mut best: Option<f64> = None;
    for t in tris {
        let (a, b, c) = (t[0], t[1], t[2]);
        let den = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if den.abs() < 1e-12 {
            continue;
        }
        let w0 = ((b[2] - c[2]) * (p.x - c[0]) + (c[0] - b[0]) * (p.z - c[2])) / den;
        let w1 = ((c[2] - a[2]) * (p.x - c[0]) + (a[0] - c[0]) * (p.z - c[2])) / den;
        let w2 = 1.0 - w0 - w1;
        if w0 >= -1e-9 && w1 >= -1e-9 && w2 >= -1e-9 {
            let h = w0 * a[1] + w1 * b[1] + w2 * c[1];
            best = Some(best.map_or(h, |b: f64| b.max(h)));
        }
    }
    best
}

fn violation_report(v: &Violation) -> ViolationReport {
    match *v {
        Violation::Radius { pi, radius, minimum } => {
            ViolationReport { kind: "Radius", point: Some(pi), from: None, to: None, value: radius, limit: minimum }
        }
        Violation::Grade { from, to, grade, maximum } => {
            ViolationReport { kind: "Grade", point: None, from: Some(from), to: Some(to), value: grade, limit: maximum }
        }
        Violation::Fill { from, to, depth, maximum } => {
            ViolationReport { kind: "Fill", point: None, from: Some(from), to: Some(to), value: depth, limit: maximum }
        }
        Violation::Cut { from, to, depth, maximum } => {
            ViolationReport { kind: "Cut", point: None, from: Some(from), to: Some(to), value: depth, limit: maximum }
        }
    }
}

/// The paved surfaces of `d` as the engine draws them beside `junctions`
/// (its boxes left out), at lift 0.
fn road_paving(id: u64, d: &DesignedRoad, junctions: &[&NetworkJunction]) -> MeshData {
    let zones = junction_zones(id, junctions.iter().copied());
    let built = build_designed_zoned(d, 0.0, &zones);
    let mut out = MeshData::default();
    for (kind, m) in &built.parts {
        if matches!(kind, Surface::Carriageway | Surface::Shoulder) {
            let base = out.positions.len() as u32;
            out.positions.extend_from_slice(&m.positions);
            out.indices.extend(m.indices.iter().map(|i| base + i));
        }
    }
    out
}

/// Audit junction `j` among `all` (for its close neighbours), reading each
/// leg's road from `roads`; `paving` caches each road's built surface.
fn audit_junction(
    j: &NetworkJunction,
    roads: &[(u64, &DesignedRoad)],
    all: &[&NetworkJunction],
    paving: &mut Vec<(u64, MeshData)>,
) -> JunctionAudit {
    let jn = &j.junction;
    let box_mesh = j.meshes(0.0);
    let box_tris: Vec<[[f64; 3]; 3]> = box_mesh
        .parts
        .iter()
        .filter(|(k, _)| *k == Surface::Carriageway)
        .flat_map(|(_, m)| triangles(m, None))
        .collect();
    let mut legs = Vec::with_capacity(j.legs.len());
    for (i, leg) in j.legs.iter().enumerate() {
        let a = &jn.approaches[i];
        let setback = jn.setbacks[i];
        let u = P2::new(a.heading.cos(), a.heading.sin());
        let n = P2::normal(a.heading);
        let at = |d: f64, o: f64| P2::new(jn.centre.x + u.x * d + n.x * o, jn.centre.z + u.z * d + n.z * o);
        // The leg's own surface, built once per road.
        let mouth_step = roads.iter().find(|(id, _)| *id == leg.road).and_then(|(id, d)| {
            if !paving.iter().any(|(r, _)| r == id) {
                paving.push((*id, road_paving(*id, d, all)));
            }
            let mesh = &paving.iter().find(|(r, _)| r == id)?.1;
            let tris = triangles(mesh, Some((at(setback, 0.0), MOUTH_REACH)));
            let half = kerb_offset(&a.standard) - 0.1;
            let mut worst: Option<(f64, f64)> = None;
            let mut o = -half;
            while o <= half + 1e-9 {
                let road = height_on(&tris, at(setback + MOUTH_PROBE, o))?;
                let paved = height_on(&box_tris, at(setback - MOUTH_PROBE, o))?;
                let step = road - paved;
                #[cfg(test)]
                let step = if super::mutant("g05_step") { 0.0 } else { step };
                if worst.is_none_or(|(w, _)| step.abs() > w.abs()) {
                    worst = Some((step, o));
                }
                o += MOUTH_PROBE_STEP;
            }
            worst
        });
        let lanes_without_exit = (0..a.lanes_in())
            .filter(|&k| !jn.connections.iter().any(|c| c.from == i && c.from_lane == k))
            .collect();
        #[cfg(test)]
        let lanes_without_exit: Vec<u32> = if super::mutant("g05_lanes") { Vec::new() } else { lanes_without_exit };
        legs.push(LegAudit {
            road: leg.road,
            class: a.standard.class.name(),
            station: leg.s,
            outward: leg.outward,
            heading_deg: a.heading.to_degrees().rem_euclid(360.0),
            setback,
            lanes_in: a.lanes_in(),
            lanes_out: a.lanes_out(),
            lanes_without_exit,
            stops: jn.stops[i],
            crosswalk: jn.crosswalks[i],
            mouth_step: mouth_step.map(|(s, _)| s),
            mouth_step_offset: mouth_step.map(|(_, o)| o),
        });
    }
    let close = all
        .iter()
        .filter(|o| !std::ptr::eq(**o, j))
        .map(|o| (o, (o.centre.x - j.centre.x).hypot(o.centre.z - j.centre.z)))
        .filter(|(_, d)| *d < MIN_JUNCTION_SPACING)
        .map(|(o, d)| CloseJunction { centre: [o.centre.x, o.centre.z], distance: d })
        .collect();
    #[cfg(test)]
    let close: Vec<CloseJunction> = if super::mutant("g05_close") { Vec::new() } else { close };
    JunctionAudit {
        centre: [j.centre.x, j.centre.z],
        elevation: j.elevation,
        control: control_name(jn.control),
        worst_mouth_step: legs.iter().filter_map(|l| l.mouth_step).fold(0.0, |w: f64, s| w.max(s.abs())),
        lanes_without_exit: legs.iter().map(|l| l.lanes_without_exit.len() as u32).sum(),
        legs,
        close,
    }
}

/// Audit road `id` among the designed `roads` (each with its layer id) and
/// the `junctions` found between them. `None` when `id` is not one of them.
pub fn audit_road(id: u64, roads: &[(u64, &DesignedRoad)], junctions: &[&NetworkJunction]) -> Option<RoadAudit> {
    let d = roads.iter().find(|(r, _)| *r == id)?.1;
    // A fill (cut) a bridge (tunnel) carries the road over is what the
    // structure is for, not a violation.
    let carried = |kind: StructureKind, from: f64, to: f64| {
        d.structures.iter().any(|s| s.kind == kind && from >= s.s0 - 1e-6 && to <= s.s1 + 1e-6)
    };
    let violations = d
        .violations
        .iter()
        .filter(|v| match **v {
            Violation::Fill { from, to, .. } => !carried(StructureKind::Bridge, from, to),
            Violation::Cut { from, to, .. } => !carried(StructureKind::Tunnel, from, to),
            _ => true,
        })
        .map(violation_report)
        .collect();
    let structures = d
        .structures
        .iter()
        .map(|s| StructureReport {
            kind: match s.kind {
                StructureKind::Bridge => "Bridge",
                StructureKind::Tunnel => "Tunnel",
            },
            from: s.s0,
            to: s.s1,
        })
        .collect();
    let mut paving = Vec::new();
    let junctions_of_road = junctions
        .iter()
        .filter(|j| j.legs.iter().any(|l| l.road == id))
        .map(|j| audit_junction(j, roads, junctions, &mut paving))
        .collect();
    Some(RoadAudit {
        road: id,
        class: d.standard.class.name(),
        length: d.stations.last().map_or(0.0, |s| s.s),
        violations,
        structures,
        junctions: junctions_of_road,
    })
}

#[cfg(test)]
mod tests {
    use super::super::road_design::{ProfileMode, RoadClass, RoadSpec};
    use super::super::road_junction::find_junctions;
    use super::super::road_mesh::design_spline;
    use super::*;

    fn road(class: RoadClass, pts: &[(f64, f64, f64)]) -> DesignedRoad {
        let spec = RoadSpec::new(class, ProfileMode::Explicit);
        let p: Vec<(P2, f64)> = pts.iter().map(|&(x, z, y)| (P2::new(x, z), y)).collect();
        design_spline(&p, spec, 0.0, 2.0, 0.0, |_| 0.0).unwrap()
    }

    #[test]
    fn a_crossroads_audits_flush_with_every_lane_leaving() {
        // Two level Local streets crossing at (200, 0): four legs, each
        // flush with the box at its mouth, every incoming lane with a way out.
        let a = road(RoadClass::Local, &[(0.0, 0.0, 5.0), (400.0, 0.0, 5.0)]);
        let b = road(RoadClass::Local, &[(200.0, -200.0, 5.0), (200.0, 200.0, 5.0)]);
        let roads = [(1u64, &a), (2, &b)];
        let js = find_junctions(&roads);
        let refs: Vec<&NetworkJunction> = js.iter().collect();
        let audit = audit_road(1, &roads, &refs).expect("road 1");
        assert_eq!(audit.class, "Local");
        assert!((audit.length - 400.0).abs() < 1.0, "length {}", audit.length);
        assert_eq!(audit.junctions.len(), 1);
        let j = &audit.junctions[0];
        assert!((j.centre[0] - 200.0).abs() < 0.1 && j.centre[1].abs() < 0.1, "{:?}", j.centre);
        assert_eq!(j.legs.len(), 4);
        for l in &j.legs {
            let step = l.mouth_step.unwrap_or_else(|| panic!("leg of road {} unread at its mouth", l.road));
            assert!(step.abs() < 0.005, "road {} steps {step:.3} m at its mouth", l.road);
            assert!(l.lanes_without_exit.is_empty(), "{l:?}");
        }
        assert_eq!(j.lanes_without_exit, 0);
        assert!(j.close.is_empty());
    }

    #[test]
    fn a_lane_drop_is_reported() {
        // A three-lane one-way Collector running north into a two-way Local
        // street: lanes 0 and 2 turn, the middle lane has nowhere to go.
        use super::super::road_design::EdgeChoice;
        let spec = RoadSpec { lanes: 3, one_way: true, edge: EdgeChoice::None, ..RoadSpec::new(RoadClass::Collector, ProfileMode::Explicit) };
        let stem = design_spline(&[(P2::new(100.0, -200.0), 0.0), (P2::new(100.0, -6.0), 0.0)], spec, 0.0, 2.0, 0.0, |_| 0.0).unwrap();
        let through = road(RoadClass::Local, &[(0.0, 0.0, 0.0), (300.0, 0.0, 0.0)]);
        let roads = [(1u64, &through), (2, &stem)];
        let js = find_junctions(&roads);
        let refs: Vec<&NetworkJunction> = js.iter().collect();
        let audit = audit_road(2, &roads, &refs).expect("road 2");
        assert_eq!(audit.junctions.len(), 1);
        let j = &audit.junctions[0];
        let stem_leg = j.legs.iter().find(|l| l.road == 2).expect("the stem's leg");
        assert_eq!(stem_leg.lanes_in, 3);
        assert_eq!(stem_leg.lanes_without_exit, vec![1]);
        assert_eq!(j.lanes_without_exit, 1);
        assert!(j.close.is_empty());
    }

    #[test]
    fn junctions_close_but_on_no_common_road_are_reported() {
        // Two crossroads 36 m apart that share no road (the second pair runs
        // 10 m up, over the first pair's roads): not merged, so reported.
        let a = road(RoadClass::Local, &[(-200.0, 0.0, 0.0), (200.0, 0.0, 0.0)]);
        let b = road(RoadClass::Local, &[(0.0, -200.0, 0.0), (0.0, 200.0, 0.0)]);
        let c = road(RoadClass::Local, &[(-200.0, 30.0, 10.0), (200.0, 30.0, 10.0)]);
        let d = road(RoadClass::Local, &[(20.0, -200.0, 10.0), (20.0, 200.0, 10.0)]);
        let roads = [(1u64, &a), (2, &b), (3, &c), (4, &d)];
        let js = find_junctions(&roads);
        assert_eq!(js.len(), 2, "{:?}", js.iter().map(|j| j.centre).collect::<Vec<_>>());
        let refs: Vec<&NetworkJunction> = js.iter().collect();
        let audit = audit_road(1, &roads, &refs).unwrap();
        assert_eq!(audit.junctions.len(), 1);
        let close = &audit.junctions[0].close;
        assert_eq!(close.len(), 1, "{close:?}");
        assert!((close[0].distance - 36.06).abs() < 0.5, "{close:?}");
    }

    #[test]
    fn a_step_at_a_mouth_is_measured() {
        // Measured on the meshes, not assumed: lower one leg's surface 0.2 m
        // under its road and the audit must see it.
        let a = road(RoadClass::Local, &[(0.0, 0.0, 5.0), (400.0, 0.0, 5.0)]);
        let b = road(RoadClass::Local, &[(200.0, -200.0, 5.0), (200.0, 200.0, 5.0)]);
        let roads = [(1u64, &a), (2, &b)];
        let mut js = find_junctions(&roads);
        let leg = js[0].legs.iter().position(|l| l.road == 2).unwrap();
        for row in js[0].surfaces[leg].heights.iter_mut() {
            for h in row.iter_mut() {
                *h -= 0.2;
            }
        }
        let refs: Vec<&NetworkJunction> = js.iter().collect();
        let audit = audit_road(2, &roads, &refs).unwrap();
        let worst = audit.junctions[0].worst_mouth_step;
        assert!((worst - 0.2).abs() < 0.02, "the audit reads a {worst:.3} m step where the box sits 0.2 m low");
    }

    #[test]
    fn violations_come_from_the_design() {
        // A Local street up a 20% slope: over its 12% short-grade limit.
        let steep = road(RoadClass::Local, &[(0.0, 0.0, 0.0), (100.0, 0.0, 20.0)]);
        let roads = [(1u64, &steep)];
        let audit = audit_road(1, &roads, &[]).unwrap();
        let grade = audit.violations.iter().find(|v| v.kind == "Grade").expect("a grade violation");
        assert!(grade.value > grade.limit, "{grade:?}");
        assert!(audit.junctions.is_empty());
        assert!(audit_road(9, &roads, &[]).is_none());
    }
}
