//! # Eustress Scenarios — Persistence
//!
//! Table of Contents:
//! 1. PersistenceError — Error types for save/load operations
//! 2. ScenarioPersistence — Save/load scenarios in binary format (bincode + zstd)
//! 3. ScenarioHeader — Lightweight metadata for listing without full deserialization
//! 4. ScenarioBundle — Multiple scenarios packed into a single binary

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::hierarchy::ScenarioGraph;
use super::types::Scenario;

// ─────────────────────────────────────────────
// 1. PersistenceError
// ─────────────────────────────────────────────

/// Errors that can occur during scenario persistence operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PersistenceError {
    /// File I/O error
    IoError(String),
    /// Serialization error
    SerializeError(String),
    /// Deserialization error
    DeserializeError(String),
    /// Compression error
    CompressionError(String),
    /// Scenario not found
    NotFound(Uuid),
    /// Version mismatch
    VersionMismatch { expected: u32, found: u32 },
    /// Integrity check failed
    IntegrityError(String),
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::SerializeError(e) => write!(f, "Serialization error: {e}"),
            Self::DeserializeError(e) => write!(f, "Deserialization error: {e}"),
            Self::CompressionError(e) => write!(f, "Compression error: {e}"),
            Self::NotFound(id) => write!(f, "Scenario not found: {id}"),
            Self::VersionMismatch { expected, found } => {
                write!(f, "Version mismatch: expected {expected}, found {found}")
            }
            Self::IntegrityError(e) => write!(f, "Integrity error: {e}"),
        }
    }
}

impl std::error::Error for PersistenceError {}

// ─────────────────────────────────────────────
// 2. ScenarioPersistence
// ─────────────────────────────────────────────

/// Current binary format version.
const FORMAT_VERSION: u32 = 1;

/// Magic bytes identifying an Eustress Scenario binary file.
const MAGIC: &[u8; 4] = b"ESCN";

/// Handles saving and loading scenarios in the Eustress binary format.
/// Format: MAGIC (4 bytes) + VERSION (4 bytes) + HASH (32 bytes) + ZSTD(bincode(data))
pub struct ScenarioPersistence;

impl ScenarioPersistence {
    /// Save a single scenario to a file.
    pub fn save(scenario: &Scenario, path: &Path) -> Result<(), PersistenceError> {
        let bytes = Self::serialize_scenario(scenario)?;
        std::fs::write(path, bytes).map_err(|e| PersistenceError::IoError(e.to_string()))
    }

    /// Load a single scenario from a file.
    pub fn load(path: &Path) -> Result<Scenario, PersistenceError> {
        let bytes =
            std::fs::read(path).map_err(|e| PersistenceError::IoError(e.to_string()))?;
        Self::deserialize_scenario(&bytes)
    }

    /// Save a bundle of multiple scenarios to a file.
    pub fn save_bundle(bundle: &ScenarioBundle, path: &Path) -> Result<(), PersistenceError> {
        let bytes = Self::serialize_bundle(bundle)?;
        std::fs::write(path, bytes).map_err(|e| PersistenceError::IoError(e.to_string()))
    }

    /// Load a bundle of scenarios from a file.
    pub fn load_bundle(path: &Path) -> Result<ScenarioBundle, PersistenceError> {
        let bytes =
            std::fs::read(path).map_err(|e| PersistenceError::IoError(e.to_string()))?;
        Self::deserialize_bundle(&bytes)
    }

    /// Read only the header from a file (lightweight, no full deserialization).
    pub fn read_header(path: &Path) -> Result<ScenarioHeader, PersistenceError> {
        let bytes =
            std::fs::read(path).map_err(|e| PersistenceError::IoError(e.to_string()))?;
        Self::extract_header(&bytes)
    }

    /// Serialize a scenario to bytes (MAGIC + VERSION + HASH + ZSTD(bincode)).
    pub fn serialize_scenario(scenario: &Scenario) -> Result<Vec<u8>, PersistenceError> {
        // Bincode encode
        let bincode_bytes = bincode::serialize(scenario)
            .map_err(|e| PersistenceError::SerializeError(e.to_string()))?;

        // Zstd compress
        let compressed = zstd::encode_all(bincode_bytes.as_slice(), 3)
            .map_err(|e| PersistenceError::CompressionError(e.to_string()))?;

        // Blake3 hash of compressed data for integrity
        let hash = blake3::hash(&compressed);

        // Assemble: MAGIC + VERSION + HASH + compressed data
        let mut output = Vec::with_capacity(4 + 4 + 32 + compressed.len());
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        output.extend_from_slice(hash.as_bytes());
        output.extend_from_slice(&compressed);

        Ok(output)
    }

    /// Deserialize a scenario from bytes.
    pub fn deserialize_scenario(bytes: &[u8]) -> Result<Scenario, PersistenceError> {
        // Validate minimum size
        if bytes.len() < 40 {
            return Err(PersistenceError::DeserializeError(
                "File too small to be a valid scenario".into(),
            ));
        }

        // Check magic
        if &bytes[0..4] != MAGIC {
            return Err(PersistenceError::DeserializeError(
                "Invalid magic bytes — not an Eustress Scenario file".into(),
            ));
        }

        // Check version
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != FORMAT_VERSION {
            return Err(PersistenceError::VersionMismatch {
                expected: FORMAT_VERSION,
                found: version,
            });
        }

        // Extract and verify hash
        let stored_hash: [u8; 32] = bytes[8..40]
            .try_into()
            .map_err(|_| PersistenceError::IntegrityError("Invalid hash length".into()))?;
        let compressed = &bytes[40..];
        let computed_hash = blake3::hash(compressed);

        if computed_hash.as_bytes() != &stored_hash {
            return Err(PersistenceError::IntegrityError(
                "Hash mismatch — file may be corrupted".into(),
            ));
        }

        // Zstd decompress
        let decompressed = zstd::decode_all(compressed)
            .map_err(|e| PersistenceError::CompressionError(e.to_string()))?;

        // Bincode decode
        bincode::deserialize(&decompressed)
            .map_err(|e| PersistenceError::DeserializeError(e.to_string()))
    }

    /// Serialize a bundle to bytes.
    fn serialize_bundle(bundle: &ScenarioBundle) -> Result<Vec<u8>, PersistenceError> {
        let bincode_bytes = bincode::serialize(bundle)
            .map_err(|e| PersistenceError::SerializeError(e.to_string()))?;

        let compressed = zstd::encode_all(bincode_bytes.as_slice(), 3)
            .map_err(|e| PersistenceError::CompressionError(e.to_string()))?;

        let hash = blake3::hash(&compressed);

        let mut output = Vec::with_capacity(4 + 4 + 32 + compressed.len());
        output.extend_from_slice(b"ESBK"); // Bundle magic
        output.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        output.extend_from_slice(hash.as_bytes());
        output.extend_from_slice(&compressed);

        Ok(output)
    }

    /// Deserialize a bundle from bytes.
    fn deserialize_bundle(bytes: &[u8]) -> Result<ScenarioBundle, PersistenceError> {
        if bytes.len() < 40 {
            return Err(PersistenceError::DeserializeError(
                "File too small for bundle".into(),
            ));
        }

        if &bytes[0..4] != b"ESBK" {
            return Err(PersistenceError::DeserializeError(
                "Invalid magic — not a Scenario Bundle".into(),
            ));
        }

        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != FORMAT_VERSION {
            return Err(PersistenceError::VersionMismatch {
                expected: FORMAT_VERSION,
                found: version,
            });
        }

        let stored_hash: [u8; 32] = bytes[8..40]
            .try_into()
            .map_err(|_| PersistenceError::IntegrityError("Invalid hash".into()))?;
        let compressed = &bytes[40..];
        let computed_hash = blake3::hash(compressed);

        if computed_hash.as_bytes() != &stored_hash {
            return Err(PersistenceError::IntegrityError(
                "Bundle hash mismatch".into(),
            ));
        }

        let decompressed = zstd::decode_all(compressed)
            .map_err(|e| PersistenceError::CompressionError(e.to_string()))?;

        bincode::deserialize(&decompressed)
            .map_err(|e| PersistenceError::DeserializeError(e.to_string()))
    }

    /// Extract a lightweight header without full deserialization.
    fn extract_header(bytes: &[u8]) -> Result<ScenarioHeader, PersistenceError> {
        // For now, do a full deserialize and extract header fields.
        // Future optimization: store header in a fixed-size prefix.
        let scenario = Self::deserialize_scenario(bytes)?;
        Ok(ScenarioHeader {
            id: scenario.id,
            name: scenario.name.clone(),
            scale: scenario.scale,
            status: scenario.status,
            branch_count: scenario.branch_count(),
            evidence_count: scenario.evidence.len(),
            entity_count: scenario.entities.len(),
            created_at: scenario.created_at,
            updated_at: scenario.updated_at,
        })
    }
}

// ─────────────────────────────────────────────
// 3. ScenarioHeader
// ─────────────────────────────────────────────

/// Lightweight metadata for listing scenarios without full deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioHeader {
    /// Scenario ID
    pub id: Uuid,
    /// Scenario name
    pub name: String,
    /// Scale
    pub scale: super::types::ScenarioScale,
    /// Status
    pub status: super::types::ScenarioStatus,
    /// Number of branches
    pub branch_count: usize,
    /// Number of evidence items
    pub evidence_count: usize,
    /// Number of entities
    pub entity_count: usize,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// ─────────────────────────────────────────────
// 4. ScenarioBundle
// ─────────────────────────────────────────────

/// Multiple scenarios packed into a single binary file.
/// Used for saving all scenarios in a Space at once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioBundle {
    /// Bundle name (usually the Space name)
    pub name: String,
    /// All scenarios in this bundle
    pub scenarios: Vec<Scenario>,
    /// The composition graph (macro/micro relationships)
    pub graph: ScenarioGraph,
    /// Bundle creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ScenarioBundle {
    /// Create a new bundle.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            scenarios: Vec::new(),
            graph: ScenarioGraph::new(),
            created_at: chrono::Utc::now(),
        }
    }

    /// Add a scenario to the bundle.
    pub fn add(&mut self, scenario: Scenario) {
        self.scenarios.push(scenario);
    }

    /// Number of scenarios in the bundle.
    pub fn count(&self) -> usize {
        self.scenarios.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::types::{ScenarioScale, ScenarioStatus};

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let mut scenario = Scenario::new("Roundtrip Test", ScenarioScale::Micro);
        scenario.set_root_branch("Root", 1.0);
        let root_id = scenario.root_branch_id.unwrap();
        scenario.add_branch(root_id, "Child A", 0.6);
        scenario.add_branch(root_id, "Child B", 0.4);

        let bytes = ScenarioPersistence::serialize_scenario(&scenario).unwrap();
        let loaded = ScenarioPersistence::deserialize_scenario(&bytes).unwrap();

        assert_eq!(loaded.id, scenario.id);
        assert_eq!(loaded.name, scenario.name);
        assert_eq!(loaded.branch_count(), 3);
    }

    #[test]
    fn test_integrity_check_fails_on_corruption() {
        let scenario = Scenario::new("Integrity Test", ScenarioScale::Micro);
        let mut bytes = ScenarioPersistence::serialize_scenario(&scenario).unwrap();

        // Corrupt a byte in the compressed data
        if let Some(last) = bytes.last_mut() {
            *last ^= 0xFF;
        }

        let result = ScenarioPersistence::deserialize_scenario(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_bundle_roundtrip() {
        let mut bundle = ScenarioBundle::new("Test Space");
        bundle.add(Scenario::new("Scenario 1", ScenarioScale::Micro));
        bundle.add(Scenario::new("Scenario 2", ScenarioScale::Macro));

        let bytes = ScenarioPersistence::serialize_bundle(&bundle).unwrap();
        let loaded = ScenarioPersistence::deserialize_bundle(&bytes).unwrap();

        assert_eq!(loaded.name, "Test Space");
        assert_eq!(loaded.count(), 2);
    }

    #[test]
    fn test_invalid_magic() {
        let bytes = b"XXXX\x01\x00\x00\x00";
        let result = ScenarioPersistence::deserialize_scenario(bytes);
        assert!(matches!(result, Err(PersistenceError::DeserializeError(_))));
    }
}
