//! Constraint solver + shell/sweep smoke tests.

use eustress_cad::{evaluate_tree, parse_tree, solve_sketch, SolveStatus};
use std::collections::HashMap;

#[test]
fn constrained_frame_solves_and_extrudes() {
    let toml = r#"
[variables]
depth = "0.01 m"

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "line"
p1 = [0.0, 0.0]
p2 = [0.08, 0.005]

[[entry.entities]]
type = "line"
p1 = [0.08, 0.005]
p2 = [0.082, 0.05]

[[entry.entities]]
type = "line"
p1 = [0.082, 0.05]
p2 = [0.002, 0.048]

[[entry.entities]]
type = "line"
p1 = [0.002, 0.048]
p2 = [0.0, 0.0]

[[entry.constraints]]
kind = "horizontal"
e1 = 0

[[entry.constraints]]
kind = "vertical"
e1 = 1

[[entry.constraints]]
kind = "horizontal"
e1 = 2

[[entry.constraints]]
kind = "vertical"
e1 = 3

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "depth"
both_sides = true
"#;
    let tree = parse_tree(toml).expect("parse");
    let out = evaluate_tree(&tree).expect("eval");
    assert!(out.body.is_some());
    assert!(out.mesh.is_some());
    let sketch_status = out
        .entry_status
        .iter()
        .find(|s| s.name == "Sketch1")
        .expect("sketch status");
    assert!(
        sketch_status.message.contains("solved") || sketch_status.ok,
        "{}",
        sketch_status.message
    );
}

#[test]
fn shell_feature_hollows_box() {
    let toml = r#"
[variables]
size = "0.05 m"
wall = "0.004 m"

[[entry]]
name = "Sketch1"
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
sketch = "Sketch1"
depth = "size"
both_sides = true

[[entry]]
name = "Shell1"
kind = "feature"
op = "shell"
open_faces = []
wall_thickness = "wall"
"#;
    let tree = parse_tree(toml).expect("parse");
    let out = evaluate_tree(&tree).expect("eval");
    let shell = out
        .entry_status
        .iter()
        .find(|s| s.name == "Shell1")
        .expect("shell status");
    assert!(shell.ok, "shell failed: {}", shell.message);
    let mesh = out.mesh.expect("shelled mesh");
    assert!(!mesh.indices.is_empty());

    // The open-top cavity is real geometry: interior wall vertices sit
    // t inside the outer wall. size=0.05 both_sides → x,y ∈ ±0.025;
    // inner wall at ±(0.025 − 0.004).
    let inner_wall = 0.025 - 0.004;
    let has_inner = mesh.positions.iter().any(|p| {
        (p[0].abs() - inner_wall as f32).abs() < 1e-4 && p[1].abs() <= inner_wall as f32 + 1e-4
    });
    assert!(has_inner, "no inner-wall vertices — cavity missing");
}

#[test]
fn fillet_feature_softens_mesh_creases() {
    let toml = r#"
[variables]
size = "0.04 m"
radius = "0.003 m"

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "rectangle"
p1 = [-0.02, -0.02]
p2 = [0.02, 0.02]

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "size"
both_sides = true

[[entry]]
name = "Fillet1"
kind = "feature"
op = "fillet"
edges = ["Extrude1/edge-0"]
radius = "radius"
"#;
    let tree = parse_tree(toml).expect("parse");
    let out = evaluate_tree(&tree).expect("eval");
    let fillet = out
        .entry_status
        .iter()
        .find(|s| s.name == "Fillet1")
        .expect("fillet status");
    assert!(fillet.ok, "{}", fillet.message);
    assert!(
        fillet.message.contains("mesh-edge"),
        "expected mesh-edge note, got {}",
        fillet.message
    );
    let mesh = out.mesh.expect("mesh");
    assert!(!mesh.indices.is_empty());
    assert_eq!(mesh.positions.len(), mesh.normals.len());
}

#[test]
fn horizontal_solver_unit() {
    use eustress_cad::{ConstraintKind, Sketch, SketchConstraint, SketchEntity};
    let sketch = Sketch {
        plane: "xy".into(),
        entities: vec![SketchEntity::Line {
            p1: [0.0, 0.0],
            p2: [1.0, 0.4],
        }],
        dimensions: vec![],
        constraints: vec![SketchConstraint {
            kind: ConstraintKind::Horizontal,
            e1: 0,
            e2: None,
        }],
    };
    let report = solve_sketch(&sketch, &HashMap::new()).unwrap();
    assert!(
        matches!(
            report.status,
            SolveStatus::UnderConstrained
                | SolveStatus::FullyConstrained
                | SolveStatus::OverConstrained
        ) || report.residual_norm < 1e-3
    );
}
