//! Built-in parametric part templates — the single source of truth
//! shared by Studio's Insert ribbon (`engine::cad_plugin`) and the
//! MCP `cad_create_part` tool (`eustress-tools::cad_tools`).
//!
//! Every variable declared in a template MUST drive geometry — a
//! Properties field that silently does nothing is worse than no field.
//! `tests/templates.rs` enforces this by re-evaluating each template
//! with every variable perturbed and asserting the mesh changes.
//!
//! Rectangles can't be dimension-driven (their corners are literal
//! coordinates), so parametric profiles use line loops: Horizontal /
//! Vertical constraints keep them square, Linear dimensions drive the
//! side lengths, and the solver's implicit endpoint welding keeps the
//! loop closed while it stretches.

/// 100 × 60 × 10 mm plate. `length`/`width` drive the profile via
/// solver dimensions; `height` drives the extrude.
pub const PLATE_TOML: &str = r#"
[variables]
length = "0.1 m"
width = "0.06 m"
height = "0.01 m"

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "line"
p1 = [-0.05, -0.03]
p2 = [0.05, -0.03]

[[entry.entities]]
type = "line"
p1 = [0.05, -0.03]
p2 = [0.05, 0.03]

[[entry.entities]]
type = "line"
p1 = [0.05, 0.03]
p2 = [-0.05, 0.03]

[[entry.entities]]
type = "line"
p1 = [-0.05, 0.03]
p2 = [-0.05, -0.03]

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

[[entry.dimensions]]
type = "linear"
e1 = 0
value = "length"

[[entry.dimensions]]
type = "linear"
e1 = 1
value = "width"

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "height"
both_sides = true
"#;

/// 50 mm cube. `size` drives all three dimensions.
pub const BOX_TOML: &str = r#"
[variables]
size = "0.05 m"

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "line"
p1 = [-0.025, -0.025]
p2 = [0.025, -0.025]

[[entry.entities]]
type = "line"
p1 = [0.025, -0.025]
p2 = [0.025, 0.025]

[[entry.entities]]
type = "line"
p1 = [0.025, 0.025]
p2 = [-0.025, 0.025]

[[entry.entities]]
type = "line"
p1 = [-0.025, 0.025]
p2 = [-0.025, -0.025]

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

[[entry.dimensions]]
type = "linear"
e1 = 0
value = "size"

[[entry.dimensions]]
type = "linear"
e1 = 1
value = "size"

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "size"
both_sides = true
"#;

/// Ø40 mm × H60 mm cylinder. `radius` drives the profile circle via a
/// radial dimension; `height` drives the extrude.
pub const CYLINDER_TOML: &str = r#"
[variables]
radius = "0.02 m"
height = "0.06 m"

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "circle"
center = [0.0, 0.0]
radius = 0.02

[[entry.dimensions]]
type = "radial"
e1 = 0
value = "radius"

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "Sketch1"
depth = "height"
both_sides = true
"#;

/// 100×60 mm plate with a centered through-hole. `hole_depth` ≥ the
/// plate height keeps it a through hole; shrink it below the height
/// for a blind hole.
pub const PLATE_HOLE_TOML: &str = r#"
[variables]
length = "0.1 m"
width = "0.06 m"
height = "0.01 m"
hole_dia = "0.012 m"
hole_depth = "0.015 m"

[[entry]]
name = "BaseSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "line"
p1 = [-0.05, -0.03]
p2 = [0.05, -0.03]

[[entry.entities]]
type = "line"
p1 = [0.05, -0.03]
p2 = [0.05, 0.03]

[[entry.entities]]
type = "line"
p1 = [0.05, 0.03]
p2 = [-0.05, 0.03]

[[entry.entities]]
type = "line"
p1 = [-0.05, 0.03]
p2 = [-0.05, -0.03]

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

[[entry.dimensions]]
type = "linear"
e1 = 0
value = "length"

[[entry.dimensions]]
type = "linear"
e1 = 1
value = "width"

[[entry]]
name = "Base"
kind = "feature"
op = "extrude"
sketch = "BaseSketch"
depth = "height"
both_sides = true

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
diameter = "hole_dia"
depth = "hole_depth"
"#;

/// L-bracket profile extruded to `thickness` (closed polyline).
pub const L_BRACKET_TOML: &str = r#"
[variables]
thickness = "0.008 m"

[[entry]]
name = "LSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "line"
p1 = [0.0, 0.0]
p2 = [0.06, 0.0]

[[entry.entities]]
type = "line"
p1 = [0.06, 0.0]
p2 = [0.06, 0.012]

[[entry.entities]]
type = "line"
p1 = [0.06, 0.012]
p2 = [0.012, 0.012]

[[entry.entities]]
type = "line"
p1 = [0.012, 0.012]
p2 = [0.012, 0.05]

[[entry.entities]]
type = "line"
p1 = [0.012, 0.05]
p2 = [0.0, 0.05]

[[entry.entities]]
type = "line"
p1 = [0.0, 0.05]
p2 = [0.0, 0.0]

[[entry]]
name = "Extrude1"
kind = "feature"
op = "extrude"
sketch = "LSketch"
depth = "thickness"
both_sides = true
"#;

/// Four deliberately-skewed lines — Solve snaps them square via the
/// Horizontal/Vertical constraints (endpoint welding keeps the loop
/// closed; explicit coincident constraints are NOT used because the
/// anchor-based Coincident compares both entities' p1, which would
/// pull chain corners onto each other).
pub const CONSTRAINED_FRAME_TOML: &str = r#"
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

/// Hollow box: `size` drives the cube, `wall` the shell thickness
/// (scale-shell approximation until offset surfaces land).
pub const SHELLED_BOX_TOML: &str = r#"
[variables]
size = "0.05 m"
wall = "0.004 m"

[[entry]]
name = "Sketch1"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "line"
p1 = [-0.025, -0.025]
p2 = [0.025, -0.025]

[[entry.entities]]
type = "line"
p1 = [0.025, -0.025]
p2 = [0.025, 0.025]

[[entry.entities]]
type = "line"
p1 = [0.025, 0.025]
p2 = [-0.025, 0.025]

[[entry.entities]]
type = "line"
p1 = [-0.025, 0.025]
p2 = [-0.025, -0.025]

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

[[entry.dimensions]]
type = "linear"
e1 = 0
value = "size"

[[entry.dimensions]]
type = "linear"
e1 = 1
value = "size"

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

/// All templates with stable names — the test harness iterates this,
/// so adding a template here automatically enrolls it in the
/// every-variable-drives-geometry check.
pub fn all() -> &'static [(&'static str, &'static str)] {
    &[
        ("plate", PLATE_TOML),
        ("box", BOX_TOML),
        ("cylinder", CYLINDER_TOML),
        ("plate_hole", PLATE_HOLE_TOML),
        ("l_bracket", L_BRACKET_TOML),
        ("constrained_frame", CONSTRAINED_FRAME_TOML),
        ("shelled_box", SHELLED_BOX_TOML),
    ]
}
