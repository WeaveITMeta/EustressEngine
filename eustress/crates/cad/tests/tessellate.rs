//! End-to-end tessellation tests: TOML feature tree → evaluate →
//! truck Solid → EvalMesh triangle arrays. This is the Phase A
//! "kernel half" of the CAD vertical slice exit test (the engine
//! half lifts EvalMesh into a Bevy Mesh).

use eustress_cad::{evaluate_tree, parse_tree, tessellate_solid, EvalMesh, EvalOutput};

fn eval(src: &str) -> EvalOutput {
    let tree = parse_tree(src).expect("TOML should parse into a FeatureTree");
    evaluate_tree(&tree).expect("tree should evaluate")
}

fn bounds(mesh: &EvalMesh) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for a in 0..3 {
            min[a] = min[a].min(p[a]);
            max[a] = max[a].max(p[a]);
        }
    }
    (min, max)
}

fn assert_mesh_well_formed(mesh: &EvalMesh) {
    assert!(!mesh.positions.is_empty(), "mesh has no vertices");
    assert_eq!(mesh.positions.len(), mesh.normals.len(), "one normal per vertex");
    assert_eq!(mesh.positions.len(), mesh.uvs.len(), "one uv per vertex");
    assert_eq!(mesh.indices.len() % 3, 0, "index count is whole triangles");
    assert!(!mesh.indices.is_empty(), "mesh has no triangles");
    let n_verts = mesh.positions.len() as u32;
    assert!(
        mesh.indices.iter().all(|&i| i < n_verts),
        "all indices in range"
    );
    for n in &mesh.normals {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!(
            (len - 1.0).abs() < 1e-3,
            "normal should be unit length, got {len}"
        );
    }
}

const BOX_TOML: &str = r#"
[variables]
depth = "20 mm"

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [0.0, 0.0]
p2 = [0.05, 0.03]

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "depth"
"#;

#[test]
fn extruded_box_tessellates_to_exact_bounds() {
    let out = eval(BOX_TOML);
    assert!(out.body.is_some(), "extrude should produce a body");
    let mesh = out.mesh.expect("evaluation should produce a mesh");
    assert_mesh_well_formed(&mesh);

    // A box is 6 planar faces — at least 2 triangles each.
    assert!(
        mesh.indices.len() >= 36,
        "box should have >= 12 triangles, got {}",
        mesh.indices.len() / 3
    );

    let (min, max) = bounds(&mesh);
    let eps = 1e-5;
    for (axis, (lo, hi)) in [(0usize, (0.0, 0.05)), (1, (0.0, 0.03)), (2, (0.0, 0.02))] {
        assert!(
            (min[axis] - lo as f32).abs() < eps && (max[axis] - hi as f32).abs() < eps,
            "axis {axis}: expected [{lo}, {hi}], got [{}, {}]",
            min[axis],
            max[axis]
        );
    }
}

fn cylinder_toml(metadata: &str) -> String {
    format!(
        r#"
{metadata}

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "circle"
center = [0.0, 0.0]
radius = 0.01

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "30 mm"
"#
    )
}

#[test]
fn extruded_cylinder_respects_radius_and_height() {
    let out = eval(&cylinder_toml(""));
    let mesh = out.mesh.expect("cylinder should produce a mesh");
    assert_mesh_well_formed(&mesh);

    let (min, max) = bounds(&mesh);
    let eps = 1e-4;
    assert!((min[2]).abs() < eps && (max[2] - 0.03).abs() < eps, "height 30 mm");

    // No vertex may sit outside the analytic cylinder by more than
    // the tessellation tolerance; the widest must come close to it.
    let max_r = mesh
        .positions
        .iter()
        .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
        .fold(0.0f32, f32::max);
    assert!(max_r <= 0.01 + 1e-4, "radius overshoot: {max_r}");
    assert!(max_r >= 0.0095, "radius undershoot: {max_r}");
}

#[test]
fn mesh_tolerance_metadata_controls_triangle_density() {
    let coarse = eval(&cylinder_toml("[metadata]\nmesh_tolerance = 0.005"))
        .mesh
        .expect("coarse mesh");
    let fine = eval(&cylinder_toml("[metadata]\nmesh_tolerance = 0.0001"))
        .mesh
        .expect("fine mesh");
    assert_mesh_well_formed(&coarse);
    assert_mesh_well_formed(&fine);
    assert!(
        fine.indices.len() > coarse.indices.len(),
        "finer tolerance should yield more triangles ({} vs {})",
        fine.indices.len(),
        coarse.indices.len()
    );
}

#[test]
fn tessellate_solid_is_directly_callable() {
    let out = eval(BOX_TOML);
    let body = out.body.expect("body");
    let mesh = tessellate_solid(&body, 0.001);
    assert_mesh_well_formed(&mesh);
}

#[test]
fn empty_tree_produces_no_mesh() {
    let out = eval("");
    assert!(out.body.is_none());
    assert!(out.mesh.is_none());
}

#[test]
fn boolean_subtract_still_tessellates() {
    // Box with a cylindrical cut — exercises the shapeops output
    // path (and the robust-retry fallback if the fast path drops
    // faces on the intersection curves).
    let src = r#"
[[entry]]
name = "BaseSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.02, -0.02]
p2 = [0.02, 0.02]

[[entry]]
name = "Base"
kind = "feature"
op = "extrude"
sketch = "BaseSketch"
depth = "10 mm"

[[entry]]
name = "CutSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "circle"
center = [0.0, 0.0]
radius = 0.005

[[entry]]
name = "Cut"
kind = "feature"
op = "extrude"
sketch = "CutSketch"
depth = "30 mm"
both_sides = true
combine = "subtract"
"#;
    let out = eval(src);
    for s in &out.entry_status {
        assert!(s.ok, "entry '{}' failed: {}", s.name, s.message);
    }
    let mesh = out.mesh.expect("subtract result should tessellate");
    assert_mesh_well_formed(&mesh);

    // The hole is real geometry: the inner wall sits at the cut
    // radius, and nothing tessellates strictly inside it.
    let min_r = mesh
        .positions
        .iter()
        .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
        .fold(f32::INFINITY, f32::min);
    assert!(
        (min_r - 0.005).abs() < 2e-4,
        "innermost vertex should sit on the cut wall (r=0.005), got {min_r}"
    );

    // Outer bounds unchanged by the cut.
    let (min, max) = bounds(&mesh);
    assert!((min[0] + 0.02).abs() < 1e-5 && (max[0] - 0.02).abs() < 1e-5);
    assert!((min[2]).abs() < 1e-5 && (max[2] - 0.01).abs() < 1e-5);
}

fn hole_toml(hole_depth_mm: u32) -> String {
    format!(
        r#"
[[entry]]
name = "BaseSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.02, -0.02]
p2 = [0.02, 0.02]

[[entry]]
name = "Base"
kind = "feature"
op = "extrude"
sketch = "BaseSketch"
depth = "10 mm"

[[entry]]
name = "HoleSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "point"
p = [0.0, 0.0]

[[entry]]
name = "Hole1"
kind = "feature"
op = "hole"
sketch_point = "HoleSketch/point-0"
diameter = "6 mm"
depth = "{hole_depth_mm} mm"
"#
    )
}

#[test]
fn blind_hole_cuts_without_coplanar_failure() {
    let out = eval(&hole_toml(5));
    for s in &out.entry_status {
        assert!(s.ok, "entry '{}' failed: {}", s.name, s.message);
    }
    let mesh = out.mesh.expect("hole result should tessellate");
    assert_mesh_well_formed(&mesh);

    // Hole wall at r = 3 mm; part bounds unchanged.
    let min_r = mesh
        .positions
        .iter()
        .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
        .fold(f32::INFINITY, f32::min);
    assert!((min_r - 0.003).abs() < 2e-4, "hole wall at r=0.003, got {min_r}");
    let (min, max) = bounds(&mesh);
    assert!((min[2]).abs() < 1e-5 && (max[2] - 0.01).abs() < 1e-5);
}

#[test]
fn through_hole_cuts_clean_out_the_far_face() {
    let out = eval(&hole_toml(10)); // exactly the part thickness
    for s in &out.entry_status {
        assert!(s.ok, "entry '{}' failed: {}", s.name, s.message);
    }
    let mesh = out.mesh.expect("through hole should tessellate");
    assert_mesh_well_formed(&mesh);
    let (min, max) = bounds(&mesh);
    assert!(
        (min[2]).abs() < 1e-5 && (max[2] - 0.01).abs() < 1e-5,
        "through-cut must not change part height: [{}, {}]",
        min[2],
        max[2]
    );
}
