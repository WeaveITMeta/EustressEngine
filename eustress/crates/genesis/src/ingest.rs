//! Ingest-and-surpass contracts (Phase 6 / Way 41, 42, 43). A vendor-agnostic
//! generation backend + a provenance-tagged asset envelope, so output from
//! Marble / a World API / a local generator enters ONE re-derivation pipeline
//! (ingest -> re-derive editable, simulatable state -> score fidelity).

use serde::{Deserialize, Serialize};

/// Where a generated / ingested asset came from — provenance for the
/// re-derivation ladder + the synthetic-data flywheel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum IngestSource {
    /// Output of an external vendor backend (name).
    Vendor(String),
    /// Produced by the in-house generative loop.
    Synthetic,
    /// Captured from the real world (scan / photogrammetry).
    Captured,
}

/// The kind of payload an ingested asset carries.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    GaussianSplats,
    PointCloud,
    Mesh,
    Voxels,
    /// A structured candidate already in our schema.
    Candidate,
}

/// A provenance-tagged generated / ingested asset — the envelope every backend
/// returns and the re-derivation pipeline consumes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeneratedAsset {
    pub kind: AssetKind,
    pub source: IngestSource,
    /// Opaque payload (e.g. .ply/.spz bytes, a serialized candidate).
    pub data: Vec<u8>,
    /// Free-form generation parameters / prompt, kept for the flywheel.
    pub meta: String,
}

/// A pluggable generation backend (vendor or in-house). The world model ingests
/// its output and re-derives editable, simulatable state.
pub trait GenerationBackend {
    /// Backend identity (for provenance).
    fn name(&self) -> &str;
    /// Generate an asset from a text prompt.
    fn generate(&self, prompt: &str) -> Result<GeneratedAsset, GenerationError>;
}

/// Generation / ingest failure.
#[derive(Debug)]
pub enum GenerationError {
    Unsupported,
    Backend(String),
}
impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerationError::Unsupported => write!(f, "generation backend unsupported"),
            GenerationError::Backend(s) => write!(f, "generation backend error: {s}"),
        }
    }
}
impl std::error::Error for GenerationError {}

/// A trivial in-house backend that emits a `Candidate`-kind asset. Proves the
/// contract round-trips end-to-end; the real generative-loop wiring replaces it.
pub struct NullBackend;
impl GenerationBackend for NullBackend {
    fn name(&self) -> &str {
        "null"
    }
    fn generate(&self, prompt: &str) -> Result<GeneratedAsset, GenerationError> {
        Ok(GeneratedAsset {
            kind: AssetKind::Candidate,
            source: IngestSource::Synthetic,
            data: Vec::new(),
            meta: prompt.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_round_trips() {
        let b = NullBackend;
        let a = b.generate("a slender steel tower").unwrap();
        assert_eq!(a.kind, AssetKind::Candidate);
        assert_eq!(a.source, IngestSource::Synthetic);
        assert_eq!(a.meta, "a slender steel tower");
        // envelope serdes
        let json = serde_json::to_string(&a).unwrap();
        let back: GeneratedAsset = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, a.kind);
    }
}
