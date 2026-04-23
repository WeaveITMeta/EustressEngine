//! Error types for the CAD crate.

use thiserror::Error;

pub type CadResult<T> = Result<T, CadError>;

#[derive(Debug, Error)]
pub enum CadError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("TOML parse error: {0}")]
    Parse(String),

    #[error("TOML serialize error: {0}")]
    Serialize(String),

    #[error("unit mismatch: expected {expected}, got {got}")]
    UnitMismatch { expected: String, got: String },

    #[error("feature evaluation failed at '{feature}': {reason}")]
    EvalFailed { feature: String, reason: String },

    #[error("sketch reference '{0}' not found in feature tree")]
    SketchNotFound(String),

    #[error("sketch is under-constrained ({dof} remaining DOF)")]
    UnderConstrained { dof: u32 },

    #[error("sketch is over-constrained ({redundant} redundant)")]
    OverConstrained { redundant: u32 },

    #[error("feature '{0}' is not yet implemented in this build")]
    NotImplemented(String),

    #[error("BRep kernel (truck) error: {0}")]
    Kernel(String),
}
