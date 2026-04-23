use thiserror::Error;

pub type MeshEditResult<T> = Result<T, MeshEditError>;

#[derive(Debug, Error)]
pub enum MeshEditError {
    #[error("vertex id {0} out of range")]
    InvalidVertex(u32),
    #[error("edge id {0} out of range")]
    InvalidEdge(u32),
    #[error("face id {0} out of range")]
    InvalidFace(u32),
    #[error("half-edge id {0} out of range")]
    InvalidHalfEdge(u32),
    #[error("operation '{op}' not supported for non-manifold input: {reason}")]
    NonManifold { op: String, reason: String },
    #[error("operation '{0}' is scaffolded but not implemented yet")]
    NotImplemented(&'static str),
    #[error("mesh has no faces (expected triangulated input)")]
    Empty,
}
