//! # eustress-cad — parametric CAD for Eustress
//!
//! Pure-Rust parametric BRep modeler built on `truck`. Provides:
//!
//! - [`Quantity`] — unit-tagged scalar. Prevents accidental inches-
//!   as-meters and makes feature-tree TOML self-describing.
//! - [`FeatureTree`] — ordered list of sketches + features per part.
//!   TOML-backed so git diffs are meaningful + hot-reload works.
//! - [`Sketch`] — 2D primitives + dimensions + constraints on a plane.
//! - [`Feature`] — Extrude / Revolve / Fillet / Chamfer / Shell / etc.
//!   Each re-evaluates deterministically into BRep bodies.
//! - [`eval::evaluate_tree`] — deterministic eval of a tree into a
//!   final [`Body`] (and mesh for rendering/physics).
//!
//! ## Architecture
//!
//! The on-disk TOML is the source of truth. Loaders parse it into
//! typed feature structs, the evaluator walks the tree producing BRep
//! shells via truck, and meshalgo tessellates for display.
//!
//! ```text
//! features.toml  ──parse──▶  FeatureTree  ──evaluate──▶  Body (truck)
//!                                                          │
//!                                                          ▼
//!                                                        Mesh
//!                                                     (for Bevy display
//!                                                      + Avian collider)
//! ```
//!
//! ## Shipped
//!
//! - `Quantity` + unit registry (length, angle, mass, force)
//! - TOML schemas for `FeatureTree`, `Sketch`, `Feature::*`
//! - Working evaluators: Extrude, Revolve, Mirror, Pattern
//!   (linear/circular), Boolean, Split, Hole
//! - Real tessellation: [`tessellate_solid`] — truck Solid →
//!   flat triangle arrays ([`EvalMesh`]) via truck-meshalgo, with
//!   robust-retry for boolean output and per-tree
//!   `metadata.mesh_tolerance` override
//!
//! ## What lands next
//!
//! Sweep / Loft / Shell evaluators; Fillet / Chamfer once
//! truck-shapeops stabilizes upstream. Sketch solver lands as
//! in-house Levenberg-Marquardt over constraint residuals (see
//! docs/architecture/CAD_PLATFORM_PLAN.md Phase C).

pub mod quantity;
pub mod feature_tree;
pub mod sketch;
pub mod feature;
pub mod eval;
pub mod error;

pub use quantity::{Quantity, Unit, LengthUnit, AngleUnit};
pub use feature_tree::{FeatureTree, FeatureEntry};
pub use sketch::{Sketch, SketchEntity, SketchDimension, SketchConstraint, ConstraintKind};
pub use feature::{
    Feature, FeatureOp, ReferencePlane,
    // Enum types the evaluator addresses as `crate::PatternKind`,
    // `crate::BooleanOp`, etc. These have to be at the crate root
    // or the `eval.rs` match arms fail to resolve them.
    PatternKind, BooleanOp, EndCondition,
};
pub use error::{CadError, CadResult};
pub use eval::{
    evaluate_tree, tessellate_solid,
    EvalOutput, EvalMesh, EntryStatus, DEFAULT_MESH_TOLERANCE,
};

/// Parse a feature tree from a TOML string. This is the primary entry
/// point — feature trees live as TOML documents in the WorldDb tree
/// partition, so they usually arrive as strings, not files.
pub fn parse_tree(s: &str) -> CadResult<FeatureTree> {
    toml::from_str(s).map_err(|e| CadError::Parse(e.to_string()))
}

/// Serialize a feature tree to a TOML string (the inverse of
/// [`parse_tree`] — what gets written to the tree partition).
pub fn tree_to_toml(tree: &FeatureTree) -> CadResult<String> {
    toml::to_string_pretty(tree).map_err(|e| CadError::Serialize(e.to_string()))
}

/// Load a feature tree from TOML on disk. Callers pass the path to
/// `<part>/features.toml`.
pub fn load_tree(path: &std::path::Path) -> CadResult<FeatureTree> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| CadError::Io(format!("read {:?}: {e}", path)))?;
    parse_tree(&s)
}

/// Write a feature tree to TOML on disk.
pub fn save_tree(path: &std::path::Path, tree: &FeatureTree) -> CadResult<()> {
    let s = tree_to_toml(tree)?;
    std::fs::write(path, s).map_err(|e| CadError::Io(format!("write {:?}: {e}", path)))
}
