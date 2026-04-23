//! 2D sketch — primitives + dimensions + constraints on a plane.

use serde::{Deserialize, Serialize};
use crate::Quantity;

/// A sketch is a named 2D drawing on a plane with primitive
/// entities, dimensions (driving), and constraints (geometric
/// relationships).
///
/// v0 shipped:
/// - Entities: `line`, `rectangle`, `circle`, `arc`, `point`
/// - Dimensions: `linear`, `radial`, `angular`
/// - Constraints: typed enum; solver lands in the evaluator module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sketch {
    /// Plane the sketch lives on. Built-ins: `"xy"`, `"xz"`, `"yz"`.
    /// When sketching on a face of a 3D part, this is the name of the
    /// feature reference (e.g. `"Extrude1/face-0"`).
    pub plane: String,

    /// The drawn entities — polyline vertices, circle centers, etc.
    #[serde(default)]
    pub entities: Vec<SketchEntity>,

    /// Driving dimensions — each references entity indexes and carries
    /// a `Quantity` or a variable name that resolves to one.
    #[serde(default)]
    pub dimensions: Vec<SketchDimension>,

    /// Geometric constraints — coincident, parallel, perpendicular,
    /// tangent, etc. Solver enforces these + dimensions together.
    #[serde(default)]
    pub constraints: Vec<SketchConstraint>,
}

impl Default for Sketch {
    fn default() -> Self {
        Self {
            plane: "xy".to_string(),
            entities: Vec::new(),
            dimensions: Vec::new(),
            constraints: Vec::new(),
        }
    }
}

/// One sketch entity — tagged enum over primitive kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SketchEntity {
    /// Straight segment from `p1` to `p2`.
    Line { p1: [f64; 2], p2: [f64; 2] },
    /// Axis-aligned (or with rotation separately applied via
    /// constraints) rectangle — two diagonal corners.
    Rectangle { p1: [f64; 2], p2: [f64; 2] },
    /// Center + radius.
    Circle { center: [f64; 2], radius: f64 },
    /// Arc — center + start angle + sweep angle (radians) + radius.
    Arc { center: [f64; 2], start_angle: f64, sweep: f64, radius: f64 },
    /// Single reference point.
    Point { p: [f64; 2] },
    /// Construction lines — drawn but non-generating (help solve
    /// constraints without producing geometry in the final body).
    Construction { p1: [f64; 2], p2: [f64; 2] },
}

/// Driving dimension — value may be a literal `Quantity` or a named
/// variable from `FeatureTree.variables`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SketchDimension {
    Linear  { e1: usize,          value: String }, // value = "50 mm" | "length"
    Radial  { e1: usize,          value: String },
    Angular { e1: usize, e2: usize, value: String }, // angle between two lines
}

impl SketchDimension {
    pub fn resolved_value(&self, vars: &std::collections::HashMap<String, String>) -> Option<Quantity> {
        let raw = match self {
            SketchDimension::Linear { value, .. }  => value,
            SketchDimension::Radial { value, .. }  => value,
            SketchDimension::Angular { value, .. } => value,
        };
        crate::feature_tree::resolve_quantity(raw, vars)
    }
}

/// Geometric constraint between sketch entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SketchConstraint {
    pub kind: ConstraintKind,
    /// First entity index into the sketch's `entities` vec.
    pub e1: usize,
    /// Second entity (optional — unary constraints like `Horizontal`
    /// only need one).
    #[serde(default)]
    pub e2: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Coincident,
    Concentric,
    Collinear,
    Parallel,
    Perpendicular,
    Tangent,
    Horizontal,
    Vertical,
    EqualLength,
    EqualRadius,
    Symmetric,
    Fix, // lock the entity in place — no DOF
}
