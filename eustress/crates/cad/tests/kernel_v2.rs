//! Kernel v2: placement, profiles, bodies, persistent names, real blends,
//! shell, draft, loft, extrude-to, countersink, split, STEP.
//!
//! Every check is a number that can come out wrong. Blends are measured by
//! the volume they REMOVE, because a 5 mm fillet changes a 100x60x10 plate's
//! volume by 0.36%: a tolerance on the total would pass with no fillet at
//! all. Planar geometry is asserted tightly (tessellation of a plane is
//! exact); anything with a circle uses a fine `mesh_tolerance` and a looser
//! bound, since a chordal polygon always encloses a little less.

use std::f64::consts::PI;

use eustress_cad::{edges_of, evaluate_tree, faces_of, mass_properties, parse_tree, step_string, EvalOutput};

fn eval(src: &str) -> EvalOutput {
    let tree = parse_tree(src).unwrap_or_else(|e| panic!("parse: {e}\n{src}"));
    evaluate_tree(&tree).expect("tree evaluates")
}

fn all_ok(out: &EvalOutput) {
    for s in &out.entry_status {
        assert!(s.ok && !s.degraded, "entry '{}' failed or degraded: {}", s.name, s.message);
    }
}

fn status<'a>(out: &'a EvalOutput, name: &str) -> &'a eustress_cad::EntryStatus {
    out.entry_status.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no entry {name}"))
}

fn volume(out: &EvalOutput) -> f64 {
    mass_properties(out.mesh.as_ref().expect("mesh")).volume()
}

fn bounds(out: &EvalOutput) -> ([f64; 3], [f64; 3]) {
    let mp = mass_properties(out.mesh.as_ref().expect("mesh"));
    (mp.min, mp.max)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-30)
}

/// 100 x 60 x 10 mm plate from a rectangle, z from 0 to 10 mm.
const PLATE: &str = r#"
[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.05, -0.03]
p2 = [0.05, 0.03]

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "10 mm"
"#;

const PLATE_V: f64 = 0.1 * 0.06 * 0.01;

// ── Placement ───────────────────────────────────────────────────────

#[test]
fn a_sketch_on_xz_extrudes_up_the_y_axis() {
    let out = eval(
        r#"
[[entry]]
name = "S"
kind = "sketch"
plane = "xz"

[[entry.entities]]
type = "rectangle"
p1 = [0.0, 0.0]
p2 = [0.02, 0.01]

[[entry]]
name = "E"
kind = "feature"
op = "extrude"
sketch = "S"
depth = "30 mm"
"#,
    );
    all_ok(&out);
    let (lo, hi) = bounds(&out);
    // u runs along +X, v along -Z, and the extrusion along +Y.
    assert!((lo[1] - 0.0).abs() < 1e-9 && (hi[1] - 0.03).abs() < 1e-9, "y {lo:?} {hi:?}");
    assert!((lo[0] - 0.0).abs() < 1e-9 && (hi[0] - 0.02).abs() < 1e-9, "x {lo:?} {hi:?}");
    assert!((lo[2] + 0.01).abs() < 1e-9 && hi[2].abs() < 1e-9, "z {lo:?} {hi:?}");
}

#[test]
fn a_sketch_on_a_face_sits_on_that_face() {
    let out = eval(&format!(
        r#"{PLATE}
[[entry]]
name = "Top"
kind = "sketch"
plane = "Extrude1.cap_end"

[[entry.entities]]
type = "rectangle"
p1 = [-0.01, -0.01]
p2 = [0.01, 0.01]

[[entry]]
name = "Boss"
kind = "feature"
op = "extrude"
sketch = "Top"
depth = "5 mm"
"#
    ));
    all_ok(&out);
    assert_eq!(out.bodies.len(), 1, "the boss joins the plate");
    let (_, hi) = bounds(&out);
    assert!((hi[2] - 0.015).abs() < 1e-9, "boss top at {}", hi[2]);
    assert!(rel(volume(&out), PLATE_V + 0.02 * 0.02 * 0.005) < 1e-6);
}

#[test]
fn a_cut_sketched_on_a_face_cuts_into_the_part() {
    // The face's normal points out of the plate. Given no direction, the
    // pocket must still go down into it rather than into the air above.
    let out = eval(&format!(
        r#"{PLATE}
[[entry]]
name = "Top"
kind = "sketch"
plane = "Extrude1.cap_end"

[[entry.entities]]
type = "rectangle"
p1 = [-0.01, -0.01]
p2 = [0.01, 0.01]

[[entry]]
name = "Pocket"
kind = "feature"
op = "extrude"
sketch = "Top"
depth = "5 mm"
combine = "subtract"
"#
    ));
    all_ok(&out);
    let msg = &status(&out, "Pocket").message;
    assert!(msg.contains("into the material"), "{msg}");
    assert_eq!(out.bodies.len(), 1);
    assert!(rel(volume(&out), PLATE_V - 0.02 * 0.02 * 0.005) < 1e-6, "volume {}", volume(&out));
    let (_, hi) = bounds(&out);
    assert!((hi[2] - 0.01).abs() < 1e-9, "nothing may be added above the plate: top at {}", hi[2]);
}

// ── Profiles ────────────────────────────────────────────────────────

#[test]
fn a_circle_inside_a_rectangle_is_a_hole_in_the_extrusion() {
    let out = eval(
        r#"
[metadata]
mesh_tolerance = 0.00005

[[entry]]
name = "S"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.05, -0.03]
p2 = [0.05, 0.03]

[[entry.entities]]
type = "circle"
center = [0.0, 0.0]
radius = 0.01

[[entry]]
name = "E"
kind = "feature"
op = "extrude"
sketch = "S"
depth = "10 mm"
"#,
    );
    all_ok(&out);
    let expect = (0.1 * 0.06 - PI * 0.01 * 0.01) * 0.01;
    assert!(rel(volume(&out), expect) < 5e-3, "volume {} vs {expect}", volume(&out));
}

#[test]
fn two_separate_outlines_become_two_bodies() {
    let out = eval(
        r#"
[[entry]]
name = "S"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [0.0, 0.0]
p2 = [0.01, 0.01]

[[entry.entities]]
type = "rectangle"
p1 = [0.02, 0.0]
p2 = [0.03, 0.01]

[[entry]]
name = "E"
kind = "feature"
op = "extrude"
sketch = "S"
depth = "10 mm"
"#,
    );
    all_ok(&out);
    assert_eq!(out.bodies.len(), 2);
    assert!(rel(volume(&out), 2.0 * 0.01 * 0.01 * 0.01) < 1e-6);
}

// ── Bodies ──────────────────────────────────────────────────────────

const BLOCK: &str = r#"
[[entry]]
name = "S"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [0.01, -0.01]
p2 = [0.03, 0.01]

[[entry]]
name = "E"
kind = "feature"
op = "extrude"
sketch = "S"
depth = "10 mm"
"#;

const BLOCK_V: f64 = 0.02 * 0.02 * 0.01;

#[test]
fn a_mirror_that_does_not_touch_adds_a_body_the_right_way_out() {
    let out = eval(&format!(
        r#"{BLOCK}
[[entry]]
name = "M"
kind = "feature"
op = "mirror"
plane = "yz"
"#
    ));
    all_ok(&out);
    assert_eq!(out.bodies.len(), 2);
    // Positive total volume: an inside-out mirror would cancel it.
    assert!(rel(volume(&out), 2.0 * BLOCK_V) < 1e-6, "{}", volume(&out));
    let (lo, _) = bounds(&out);
    assert!((lo[0] + 0.03).abs() < 1e-9);
}

#[test]
fn an_additive_pattern_of_separate_copies_keeps_every_copy() {
    let out = eval(&format!(
        r#"{BLOCK}
[[entry]]
name = "P"
kind = "feature"
op = "pattern"
pattern_kind = "linear"
count = 3
spacing = "50 mm"
direction = [1.0, 0.0, 0.0]
"#
    ));
    all_ok(&out);
    assert_eq!(out.bodies.len(), 3);
    assert!(rel(volume(&out), 3.0 * BLOCK_V) < 1e-6);
}

#[test]
fn split_makes_two_bodies_that_add_up() {
    let out = eval(&format!(
        r#"{PLATE}
[[entry]]
name = "Cut"
kind = "feature"
op = "split"
plane = "yz"
"#
    ));
    all_ok(&out);
    assert_eq!(out.bodies.len(), 2);
    assert!(rel(volume(&out), PLATE_V) < 1e-6);
}

#[test]
fn a_failed_feature_leaves_the_part_as_it_was() {
    let out = eval(&format!(
        r#"{PLATE}
[[entry]]
name = "Far"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "circle"
center = [1.0, 1.0]
radius = 0.005

[[entry]]
name = "Miss"
kind = "feature"
op = "extrude"
sketch = "Far"
depth = "10 mm"
combine = "subtract"
"#
    ));
    let s = status(&out, "Miss");
    assert!(!s.ok && s.message.contains("does not intersect"), "{}", s.message);
    assert_eq!(out.bodies.len(), 1);
    assert!(rel(volume(&out), PLATE_V) < 1e-6);
}

// ── Names ───────────────────────────────────────────────────────────

#[test]
fn a_plate_names_its_faces_and_edges_after_the_sketch() {
    let out = eval(PLATE);
    let b = &out.bodies[0];
    let faces: Vec<String> = faces_of(b).into_iter().map(|f| f.name).collect();
    for n in [
        "Extrude1.cap_start",
        "Extrude1.cap_end",
        "Extrude1.side.e0.bottom",
        "Extrude1.side.e0.right",
        "Extrude1.side.e0.top",
        "Extrude1.side.e0.left",
    ] {
        assert!(faces.iter().any(|f| f == n), "missing face {n}: {faces:?}");
    }
    let edges = edges_of(b);
    assert_eq!(edges.len(), 12);
    assert!(edges.iter().any(|e| e.name == "Extrude1.cap_end | Extrude1.side.e0.top"), "{edges:?}");
}

#[test]
fn names_survive_a_hole() {
    let out = eval(&format!(
        r#"{PLATE}
[[entry]]
name = "HS"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "point"
p = [0.0, 0.0]

[[entry]]
name = "Hole1"
kind = "feature"
op = "hole"
sketch_point = "HS/point-0"
diameter = "10 mm"
depth = "10 mm"
"#
    ));
    all_ok(&out);
    let b = &out.bodies[0];
    let faces: Vec<String> = faces_of(b).into_iter().map(|f| f.name).collect();
    for n in ["Extrude1.cap_end", "Extrude1.cap_start", "Hole1.wall.0", "Hole1.wall.1"] {
        assert!(faces.iter().any(|f| f == n), "missing face {n}: {faces:?}");
    }
    let edges = edges_of(b);
    assert!(edges.iter().any(|e| e.name == "Extrude1.cap_end | Hole1.wall.0"), "{:?}", edges.iter().map(|e| &e.name).collect::<Vec<_>>());
}

// ── Blends ──────────────────────────────────────────────────────────

fn blend(op: &str, size_field: &str, edges: &[&str]) -> EvalOutput {
    let list = edges.iter().map(|e| format!("\"{e}\"")).collect::<Vec<_>>().join(", ");
    eval(&format!(
        r#"
[metadata]
mesh_tolerance = 0.00002
{PLATE}
[[entry]]
name = "B"
kind = "feature"
op = "{op}"
edges = [{list}]
{size_field}
"#
    ))
}

const LATERAL: [&str; 4] = [
    "Extrude1.side.e0.bottom | Extrude1.side.e0.right",
    "Extrude1.side.e0.right | Extrude1.side.e0.top",
    "Extrude1.side.e0.left | Extrude1.side.e0.top",
    "Extrude1.side.e0.bottom | Extrude1.side.e0.left",
];

#[test]
fn filleting_the_corners_of_a_plate_is_exact() {
    let out = blend("fillet", r#"radius = "5 mm""#, &LATERAL);
    all_ok(&out);
    assert!(status(&out, "B").message.contains("exactly"), "{}", status(&out, "B").message);
    let r: f64 = 0.005;
    let removed = PLATE_V - volume(&out);
    let expect = 4.0 * (1.0 - PI / 4.0) * r * r * 0.01;
    assert!(rel(removed, expect) < 0.05, "removed {removed} vs {expect}");
    let faces: Vec<String> = faces_of(&out.bodies[0]).into_iter().map(|f| f.name).collect();
    assert!(faces.iter().any(|f| f == "B.face.0"), "{faces:?}");
}

#[test]
fn chamfering_the_corners_of_a_plate_is_exact() {
    let out = blend("chamfer", r#"distance = "4 mm""#, &LATERAL);
    all_ok(&out);
    let d: f64 = 0.004;
    let removed = PLATE_V - volume(&out);
    let expect = 4.0 * 0.5 * d * d * 0.01;
    // Planar faces only: exact to rounding.
    assert!(rel(removed, expect) < 1e-6, "removed {removed} vs {expect}");
}

#[test]
fn chamfering_a_top_edge_cuts_the_solid() {
    let out = blend("chamfer", r#"distance = "3 mm""#, &["Extrude1.cap_end | Extrude1.side.e0.top"]);
    all_ok(&out);
    let d: f64 = 0.003;
    let removed = PLATE_V - volume(&out);
    // A triangular prism the length of the 100 mm edge.
    let expect = 0.5 * d * d * 0.1;
    assert!(rel(removed, expect) < 1e-3, "removed {removed} vs {expect}");
}

#[test]
fn an_unknown_edge_is_refused_by_name() {
    let out = blend("fillet", r#"radius = "1 mm""#, &["Extrude1.cap_end | Nope.face"]);
    let s = status(&out, "B");
    assert!(!s.ok && s.message.contains("no edge named"), "{}", s.message);
}

// ── Shell ───────────────────────────────────────────────────────────

const CUBE50: &str = r#"
[[entry]]
name = "S"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.025, -0.025]
p2 = [0.025, 0.025]

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "S"
depth = "50 mm"
both_sides = true
"#;

#[test]
fn an_open_top_shell_is_exact() {
    let out = eval(&format!(
        r#"{CUBE50}
[[entry]]
name = "Sh"
kind = "feature"
op = "shell"
open_faces = ["Extrude1.cap_end"]
wall_thickness = "4 mm"
"#
    ));
    all_ok(&out);
    // 50^3 minus a 42 x 42 x 46 cavity open at the top.
    let expect = (125_000.0 - 42.0 * 42.0 * 46.0) * 1e-9;
    assert!(rel(volume(&out), expect) < 1e-6, "{} vs {expect}", volume(&out));
}

#[test]
fn a_shell_with_no_open_face_is_a_closed_cavity() {
    let out = eval(&format!(
        r#"{CUBE50}
[[entry]]
name = "Sh"
kind = "feature"
op = "shell"
open_faces = []
wall_thickness = "4 mm"
"#
    ));
    all_ok(&out);
    let expect = (125_000.0 - 42.0 * 42.0 * 42.0) * 1e-9;
    assert!(rel(volume(&out), expect) < 1e-6, "{} vs {expect}", volume(&out));
}

// ── Draft, loft, extrude-to ─────────────────────────────────────────

fn frustum(a1: f64, a2: f64, h: f64) -> f64 {
    h / 3.0 * (a1 + a2 + (a1 * a2).sqrt())
}

#[test]
fn a_drafted_extrusion_is_a_frustum() {
    let out = eval(
        r#"
[[entry]]
name = "S"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.01, -0.01]
p2 = [0.01, 0.01]

[[entry]]
name = "E"
kind = "feature"
op = "extrude"
sketch = "S"
depth = "10 mm"
draft_angle = "10 deg"
"#,
    );
    all_ok(&out);
    let top = 0.02 - 2.0 * 0.01 * (10.0_f64).to_radians().tan();
    let expect = frustum(0.02 * 0.02, top * top, 0.01);
    assert!(rel(volume(&out), expect) < 1e-6, "{} vs {expect}", volume(&out));
}

#[test]
fn a_loft_between_two_squares_is_a_frustum() {
    let out = eval(
        r#"
[[entry]]
name = "S1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.01, -0.01]
p2 = [0.01, 0.01]

[[entry]]
name = "P"
kind = "feature"
op = "reference_plane"
plane_kind = "offset"
base = "xy"
distance = "10 mm"

[[entry]]
name = "S2"
kind = "sketch"
plane = "P"

[[entry.entities]]
type = "rectangle"
p1 = [-0.005, -0.005]
p2 = [0.005, 0.005]

[[entry]]
name = "L"
kind = "feature"
op = "loft"
profiles = ["S1", "S2"]
"#,
    );
    all_ok(&out);
    let expect = frustum(0.02 * 0.02, 0.01 * 0.01, 0.01);
    assert!(rel(volume(&out), expect) < 1e-6, "{} vs {expect}", volume(&out));
}

#[test]
fn extrude_to_a_plane_stops_at_it() {
    let out = eval(&format!(
        r#"{PLATE}
[[entry]]
name = "P"
kind = "feature"
op = "reference_plane"
plane_kind = "offset"
base = "xy"
distance = "25 mm"

[[entry]]
name = "Mid"
kind = "feature"
op = "reference_plane"
plane_kind = "offset"
base = "xy"
distance = "5 mm"

[[entry]]
name = "BS"
kind = "sketch"
plane = "Mid"

[[entry.entities]]
type = "rectangle"
p1 = [-0.005, -0.005]
p2 = [0.005, 0.005]

[[entry]]
name = "Boss"
kind = "feature"
op = "extrude"
sketch = "BS"
depth = "1 mm"
end_condition = "to_plane"
to = "P"
"#
    ));
    all_ok(&out);
    let (_, hi) = bounds(&out);
    assert!((hi[2] - 0.025).abs() < 1e-9, "top at {}", hi[2]);
    // The boss adds 10 x 10 mm from the plate top (10 mm) to 25 mm.
    assert!(rel(volume(&out), PLATE_V + 0.01 * 0.01 * 0.015) < 1e-6);
}

#[test]
fn a_through_all_cut_from_the_top_face_goes_through() {
    let out = eval(&format!(
        r#"
[metadata]
mesh_tolerance = 0.00002
{PLATE}
[[entry]]
name = "Top"
kind = "sketch"
plane = "Extrude1.cap_end"

[[entry.entities]]
type = "circle"
center = [0.0, 0.0]
radius = 0.005

[[entry]]
name = "Cut"
kind = "feature"
op = "extrude"
sketch = "Top"
depth = "1 mm"
end_condition = "through_all"
combine = "subtract"
"#
    ));
    all_ok(&out);
    let removed = PLATE_V - volume(&out);
    let expect = PI * 0.005 * 0.005 * 0.01;
    assert!(rel(removed, expect) < 0.02, "removed {removed} vs {expect}");
}

// ── Revolve, hole ───────────────────────────────────────────────────

#[test]
fn revolving_about_a_sketch_line_makes_a_ring() {
    let out = eval(
        r#"
[metadata]
mesh_tolerance = 0.00002

[[entry]]
name = "S"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [0.01, 0.0]
p2 = [0.02, 0.01]

[[entry.entities]]
type = "construction"
p1 = [0.0, 0.0]
p2 = [0.0, 1.0]

[[entry]]
name = "R"
kind = "feature"
op = "revolve"
sketch = "S"
axis = "e1"
angle = "360 deg"
"#,
    );
    all_ok(&out);
    let expect = PI * (0.02 * 0.02 - 0.01 * 0.01) * 0.01;
    assert!(rel(volume(&out), expect) < 0.01, "{} vs {expect}", volume(&out));
}

#[test]
fn a_countersink_is_a_cone_not_a_counterbore() {
    let out = eval(&format!(
        r#"
[metadata]
mesh_tolerance = 0.00002
{PLATE}
[[entry]]
name = "HS"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "point"
p = [0.0, 0.0]

[[entry]]
name = "H"
kind = "feature"
op = "hole"
sketch_point = "HS/point-0"
diameter = "6 mm"
depth = "10 mm"
countersink_diameter = "12 mm"
countersink_angle = "90 deg"
"#
    ));
    all_ok(&out);
    // Through bore r = 3 mm, plus a 90 degree cone from r = 6 mm at the
    // entry face down to the bore, 3 mm deep: a frustum minus the bore
    // inside it.
    let bore = PI * 0.003 * 0.003 * 0.01;
    let cone = PI * 0.003 / 3.0 * (0.006 * 0.006 + 0.003 * 0.003 + 0.006 * 0.003) - PI * 0.003 * 0.003 * 0.003;
    let removed = PLATE_V - volume(&out);
    assert!(rel(removed, bore + cone) < 0.03, "removed {removed} vs {}", bore + cone);
    // A straight counterbore to 6 mm at a hard-coded 5 mm depth would
    // remove far more than the cone.
    let counterbore = PI * (0.006 * 0.006 - 0.003 * 0.003) * 0.005;
    assert!(removed < bore + counterbore * 0.8);
}

// ── STEP ────────────────────────────────────────────────────────────

#[test]
fn step_export_of_a_drilled_plate_is_well_formed_and_in_millimetres() {
    let out = eval(&format!(
        r#"{PLATE}
[[entry]]
name = "HS"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "point"
p = [0.0, 0.0]

[[entry]]
name = "Hole1"
kind = "feature"
op = "hole"
sketch_point = "HS/point-0"
diameter = "10 mm"
depth = "10 mm"
"#
    ));
    all_ok(&out);
    let text = step_string(&out.bodies, "plate").expect("step");
    assert!(text.contains("ISO-10303-21"), "not a STEP file");
    assert!(text.contains("SI_UNIT(.MILLI.,.METRE.)"));
    // The plate spans +/- 50 mm; in metres it would read 0.05.
    assert!(text.contains("50."), "coordinates are not in millimetres");
    // truck-stepio 0.3 writes INTERSECTION_CURVE with a duplicated entity
    // id; the exporter must have replaced every one.
    assert!(!text.contains("INTERSECTION_CURVE"));
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix('#') {
            if let Some((n, _)) = rest.split_once('=') {
                assert!(seen.insert(n.trim().to_string()), "entity #{} appears twice", n.trim());
            }
        }
    }
}
