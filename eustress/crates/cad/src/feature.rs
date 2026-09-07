//! Feature — a parametric operation in the tree. Tagged enum so TOML
//! stays compact while the types stay typed.

use serde::{Deserialize, Serialize};

use crate::error::{CadError, CadResult};

/// A feature operation. Each variant serializes with `op = "<name>"`
/// so TOML stays compact.
///
/// ```toml
/// [[entry]]
/// name = "Extrude1"
/// kind = "feature"
/// op = "extrude"
/// sketch = "Sketch1"
/// depth = "20 mm"
/// end_condition = "blind"
/// # implicit: combine = "new_body"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Feature {
    /// Linear extrusion of a sketch profile.
    Extrude {
        sketch: String,
        /// `"50 mm"` or a variable name (`"length"`).
        depth: String,
        #[serde(default)]
        end_condition: EndCondition,
        #[serde(default)]
        combine: FeatureOp,
        /// Draft angle (per-side). `"0 deg"` = straight.
        #[serde(default = "default_zero_deg")]
        draft_angle: String,
        /// Both-sides extrusion — mirrors the depth to the negative
        /// side of the sketch plane.
        #[serde(default)]
        both_sides: bool,
    },

    /// Revolve a sketch profile around an axis.
    Revolve {
        sketch: String,
        /// Axis reference — e.g. `"xy/x"` (plane/edge) or an entity
        /// edge like `"Extrude1/edge-4"`.
        axis: String,
        /// `"360 deg"` or a variable.
        angle: String,
        #[serde(default)]
        combine: FeatureOp,
    },

    /// Fillet rounded edges — one radius per edge list.
    Fillet {
        /// List of edge references (e.g. `["Extrude1/edge-0", "Extrude1/edge-2"]`).
        edges: Vec<String>,
        radius: String,
        /// Propagate to tangent-connected edges automatically.
        #[serde(default = "default_true")]
        propagate_tangent: bool,
    },

    /// Chamfer edges — distance or distance-angle.
    Chamfer {
        edges: Vec<String>,
        /// Distance from the edge along each adjacent face.
        /// When `distance2` is present, chamfer is asymmetric.
        distance: String,
        #[serde(default)]
        distance2: Option<String>,
        /// Alternate authoring: distance + angle (45° default).
        #[serde(default)]
        angle: Option<String>,
    },

    /// Hollow the part; `open_faces` stay open.
    Shell {
        open_faces: Vec<String>,
        wall_thickness: String,
    },

    /// Sweep — sketch profile along a path (another sketch or 3D curve).
    Sweep {
        profile: String,
        path: String,
        #[serde(default)]
        combine: FeatureOp,
    },

    /// Loft between two or more profile sketches.
    Loft {
        profiles: Vec<String>,
        /// Optional guide curves to constrain the loft path.
        #[serde(default)]
        guide_curves: Vec<String>,
        #[serde(default)]
        combine: FeatureOp,
    },

    /// Parametric hole — drill, counterbore, countersink, tapped.
    Hole {
        sketch_point: String, // "Sketch1/point-0"
        diameter: String,
        depth: String,
        #[serde(default)]
        counterbore_diameter: Option<String>,
        #[serde(default)]
        counterbore_depth: Option<String>,
        #[serde(default)]
        countersink_diameter: Option<String>,
        #[serde(default)]
        countersink_angle: Option<String>,
        #[serde(default)]
        tap_class: Option<String>, // "M6" etc.
    },

    /// Mirror bodies across a plane — feature-tree level (parametric).
    /// The Part/Model-level `ModelReflect` in `tools_smart` is a
    /// sibling; this is the BRep-kernel variant.
    Mirror {
        plane: String,
        /// If present, only these features' output bodies are mirrored;
        /// else the entire current-result shell is mirrored.
        #[serde(default)]
        features: Vec<String>,
        #[serde(default)]
        combine: FeatureOp,
    },

    /// Linear / circular / path pattern.
    Pattern {
        #[serde(rename = "pattern_kind")]
        kind: PatternKind,
        features: Vec<String>,
        count: CountSpec,
        /// For linear: direction + spacing. For circular: axis + angle.
        /// Stored as strings so variable refs work (`"length"`, `"180 deg"`).
        #[serde(default)]
        spacing: Option<String>,
        #[serde(default)]
        direction: Option<[f64; 3]>,
        #[serde(default)]
        axis: Option<String>,
        #[serde(default)]
        angle: Option<String>,
        /// How the instances combine with the running body.
        ///
        /// Defaults to `Add`, which is what every pre-existing tree
        /// assumed, so nothing changes meaning. Set it to `Subtract`
        /// to pattern a cut: "four of this hole" needs the hole's
        /// TOOL replicated and removed, which no combination of the
        /// other fields could previously express.
        #[serde(default)]
        combine: FeatureOp,
    },

    /// Reference plane — offset from another plane, 3-point, tangent.
    /// Not a body-producing feature but participates in the tree.
    ReferencePlane {
        #[serde(flatten)]
        plane: ReferencePlane,
    },

    /// Body-level boolean — union / subtract / intersect between
    /// current result + the body output of another feature subtree.
    ///
    /// **Note**: the operand field is `boolean_op` because the enum
    /// is tagged with `#[serde(tag = "op")]` — a variant field
    /// called `op` would collide with the tag discriminator and
    /// break the derive. The field is renamed both at the Rust
    /// level and on the wire so no callsite is tempted to write
    /// `Boolean { op: ... }` and re-introduce the clash.
    Boolean {
        target: String,
        boolean_op: BooleanOp,
    },

    /// Split-body along a plane — produces two output bodies the user
    /// can later reference individually in a Pattern or Mirror.
    Split {
        plane: String,
    },
}

impl Feature {
    /// Short label for feature-tree UI rows.
    pub fn op_label(&self) -> &'static str {
        match self {
            Feature::Extrude { .. } => "extrude",
            Feature::Revolve { .. } => "revolve",
            Feature::Fillet { .. } => "fillet",
            Feature::Chamfer { .. } => "chamfer",
            Feature::Shell { .. } => "shell",
            Feature::Sweep { .. } => "sweep",
            Feature::Loft { .. } => "loft",
            Feature::Hole { .. } => "hole",
            Feature::Mirror { .. } => "mirror",
            Feature::Pattern { .. } => "pattern",
            Feature::ReferencePlane { .. } => "plane",
            Feature::Boolean { .. } => "boolean",
            Feature::Split { .. } => "split",
        }
    }
}

/// How a feature's output combines with the running part body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureOp {
    /// Append — produce a new solid body.
    NewBody,
    /// Add to the existing body (union).
    #[default]
    Add,
    /// Subtract from the existing body.
    Subtract,
    /// Intersect with the existing body.
    Intersect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndCondition {
    #[default]
    Blind,        // fixed depth
    ThroughAll,   // extrude until out of the bounding box
    ToPlane,      // up to a named plane
    ToSurface,    // up to a named face reference
    MidPlane,     // extrude half each direction from sketch plane
    UpToNext,     // stop at the next body encountered
}

/// How many instances a pattern makes: a literal, or a rule.
///
/// Every other algorithmic parameter on a feature is a string resolved
/// through the variable/expression path (`spacing = "pitch"`,
/// `angle = "sweep/2"`, `depth = "height"`). `count` alone was a raw
/// `u32`, so the one quantity that decides how much repetition happens
/// could not be driven by a rule: you could parameterise the SPACING
/// of a hole array but not the NUMBER of holes, which is the parameter
/// anyone actually wants to vary.
///
/// Untagged, so `count = 4` and `count = "hole_count"` both parse and
/// every existing tree keeps working unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CountSpec {
    /// A literal, e.g. `count = 4`.
    Fixed(u32),
    /// A variable name or expression, e.g. `count = "rows * cols"`.
    Expr(String),
}

impl Default for CountSpec {
    fn default() -> Self {
        CountSpec::Fixed(1)
    }
}

impl CountSpec {
    /// Resolve to an instance count.
    ///
    /// A count is dimensionless by nature, so a value carrying a unit
    /// (`"10 mm"`) is a mistake worth naming rather than truncating.
    pub fn resolve(&self, vars: &std::collections::HashMap<String, String>) -> CadResult<u32> {
        match self {
            CountSpec::Fixed(n) => Ok(*n),
            CountSpec::Expr(src) => {
                let q = crate::feature_tree::resolve_quantity_explained(src, vars).map_err(|e| {
                    CadError::EvalFailed {
                        feature: "Pattern".into(),
                        reason: format!("count '{src}' does not resolve: {e}"),
                    }
                })?;
                if !matches!(q.unit, crate::Unit::Scalar) {
                    return Err(CadError::EvalFailed {
                        feature: "Pattern".into(),
                        reason: format!(
                            "count '{src}' resolved to {:?}, but a count is a plain number. \
                             Drop the unit",
                            q.unit
                        ),
                    });
                }
                let v = q.value;
                if !v.is_finite() || v < 0.0 {
                    return Err(CadError::EvalFailed {
                        feature: "Pattern".into(),
                        reason: format!("count '{src}' resolved to {v}, which is not a count"),
                    });
                }
                // Tolerate the float arithmetic that produced it: an
                // expression like "12/4" lands on 2.9999999999999996.
                let rounded = v.round();
                if (v - rounded).abs() > 1.0e-6 {
                    return Err(CadError::EvalFailed {
                        feature: "Pattern".into(),
                        reason: format!(
                            "count '{src}' resolved to {v}, which is not a whole number of \
                             instances"
                        ),
                    });
                }
                Ok(rounded as u32)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    #[default]
    Linear,
    Circular,
    Path,
    /// Instance at each point of a referenced sketch.
    Sketch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp { Union, Difference, Intersect }

/// Reference plane definition — offset, 3-point, tangent-to-face,
/// or normal-to-curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "plane_kind", rename_all = "snake_case")]
pub enum ReferencePlane {
    Offset { base: String, distance: String },
    ThreePoint { p1: [f64; 3], p2: [f64; 3], p3: [f64; 3] },
    TangentFace { face: String },
    NormalToCurve { curve: String, t: f64 },
}

fn default_true() -> bool { true }
fn default_zero_deg() -> String { "0 deg".to_string() }
