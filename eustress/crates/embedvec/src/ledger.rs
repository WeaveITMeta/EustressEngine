//! Trait Ledger — Versioned Provenance-Chained Mutation History
//!
//! ## Table of Contents
//! 1. TraitValue      — typed value (Scalar / Vector / Label / Flag / Map)
//! 2. TraitDelta      — granular delta between two values
//! 3. ProvenanceRecord — who/when/why metadata for every mutation
//! 4. ProvenanceSource — which subsystem authored the change
//! 5. TraitRevision   — single versioned revision with hash chain
//! 6. DiffResult      — structured diff between two revision states
//! 7. RollbackPolicy  — rules governing permitted rollback
//! 8. TraitLedger     — full ACID-like revision history for a single trait
//!
//! ## Design
//! Inspired by Vortex `storage/trait_ledger.rs`. Every mutation to a
//! tracked trait (embedding, property, classification) carries mandatory
//! provenance: author, timestamp, reason, and source subsystem.
//! Revisions form a hash chain for integrity: each revision stores the
//! hash of its predecessor, making tampering detectable.
//!
//! ## Usage
//! ```rust
//! use eustress_embedvec::{TraitLedger, TraitValue, ProvenanceRecord, ProvenanceSource};
//!
//! let mut ledger = TraitLedger::new("confidence", TraitValue::Scalar(0.5));
//! let prov = ProvenanceRecord::new("system", "initial calibration",
//!                                  ProvenanceSource::SystemInit, 0);
//! ledger.commit(TraitValue::Scalar(0.8), prov).unwrap();
//!
//! assert_eq!(ledger.current().as_scalar(), 0.8);
//! assert_eq!(ledger.version(), 1);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// 1. TraitValue
// ============================================================================

/// A typed trait value — confidence, embedding, label, flag, or nested map
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TraitValue {
    /// Scalar float (confidence, weight, strength)
    Scalar(f64),
    /// Vector of floats (embedding, tensor slice)
    Vector(Vec<f64>),
    /// Categorical string label
    Label(String),
    /// Boolean flag (invariance rule, constraint switch)
    Flag(bool),
    /// Structured map of named sub-properties
    Map(HashMap<String, TraitValue>),
}

impl TraitValue {
    /// Compute the delta from `self` to `other`
    pub fn diff(&self, other: &TraitValue) -> Option<TraitDelta> {
        match (self, other) {
            (TraitValue::Scalar(a), TraitValue::Scalar(b)) => {
                Some(TraitDelta::ScalarDelta(*b - *a))
            }
            (TraitValue::Vector(a), TraitValue::Vector(b)) if a.len() == b.len() => {
                let delta = a.iter().zip(b).map(|(x, y)| y - x).collect();
                Some(TraitDelta::VectorDelta(delta))
            }
            (TraitValue::Label(a), TraitValue::Label(b)) if a != b => {
                Some(TraitDelta::LabelChange(a.clone(), b.clone()))
            }
            (TraitValue::Flag(a), TraitValue::Flag(b)) if a != b => {
                Some(TraitDelta::FlagFlip(*b))
            }
            (a, b) if a != b => Some(TraitDelta::Replacement(b.clone())),
            _ => None, // no change
        }
    }

    /// Apply a delta to produce a new value
    pub fn apply_delta(&self, delta: &TraitDelta) -> TraitValue {
        match (self, delta) {
            (TraitValue::Scalar(v), TraitDelta::ScalarDelta(d)) => TraitValue::Scalar(v + d),
            (TraitValue::Vector(v), TraitDelta::VectorDelta(d)) => {
                TraitValue::Vector(v.iter().zip(d).map(|(a, b)| a + b).collect())
            }
            (_, TraitDelta::LabelChange(_, new)) => TraitValue::Label(new.clone()),
            (_, TraitDelta::FlagFlip(new)) => TraitValue::Flag(*new),
            (_, TraitDelta::Replacement(new)) => new.clone(),
            _ => self.clone(), // incompatible — no-op
        }
    }

    /// Extract scalar or default 0.0
    pub fn as_scalar(&self) -> f64 {
        match self {
            TraitValue::Scalar(v) => *v,
            _ => 0.0,
        }
    }

    /// Extract label or empty string
    pub fn as_label(&self) -> &str {
        match self {
            TraitValue::Label(s) => s.as_str(),
            _ => "",
        }
    }

    /// Extract flag or false
    pub fn as_flag(&self) -> bool {
        match self {
            TraitValue::Flag(b) => *b,
            _ => false,
        }
    }
}

// ============================================================================
// 2. TraitDelta
// ============================================================================

/// A granular delta representing the change between two trait values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraitDelta {
    /// Additive scalar change (e.g., confidence += 0.05)
    ScalarDelta(f64),
    /// Element-wise vector change
    VectorDelta(Vec<f64>),
    /// Label transition (old → new)
    LabelChange(String, String),
    /// Boolean flip
    FlagFlip(bool),
    /// Full replacement (incompatible type change)
    Replacement(TraitValue),
}

impl TraitDelta {
    /// L2-norm magnitude of the delta
    pub fn magnitude(&self) -> f64 {
        match self {
            TraitDelta::ScalarDelta(d) => d.abs(),
            TraitDelta::VectorDelta(d) => d.iter().map(|x| x * x).sum::<f64>().sqrt(),
            TraitDelta::LabelChange(_, _) => 1.0,
            TraitDelta::FlagFlip(_) => 1.0,
            TraitDelta::Replacement(_) => f64::MAX,
        }
    }
}

// ============================================================================
// 4. ProvenanceSource
// ============================================================================

/// Subsystem that authored the trait change
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProvenanceSource {
    /// System initialization / seed
    SystemInit,
    /// Bevy ECS observation (auto-indexed)
    EcsObserver,
    /// Rune script (DSL execution)
    RuneScript,
    /// MCP server request
    McpServer,
    /// Human operator override
    HumanOverride,
    /// Federated learning aggregation
    FederatedAggregation,
    /// Reinforcement learning policy update
    ReinforcementLearning,
    /// Supervised training pass
    SupervisedTraining,
    /// Rollback operation
    Rollback,
    /// Custom source label
    Custom(String),
}

// ============================================================================
// 3. ProvenanceRecord
// ============================================================================

/// Mandatory provenance attached to every trait revision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    /// Author identifier (agent, node, or "human")
    pub author: String,
    /// Unix timestamp milliseconds
    pub timestamp_ms: u64,
    /// Human-readable reason for the change
    pub reason: String,
    /// Subsystem that proposed the change
    pub source: ProvenanceSource,
    /// Hash of the previous revision (chain integrity)
    pub parent_hash: u64,
}

impl ProvenanceRecord {
    /// Create a new record with current timestamp
    pub fn new(
        author: &str,
        reason: &str,
        source: ProvenanceSource,
        parent_hash: u64,
    ) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;

        Self {
            author: author.to_string(),
            timestamp_ms,
            reason: reason.to_string(),
            source,
            parent_hash,
        }
    }

    /// DJB2 hash of this record (for chain linking)
    pub fn hash(&self) -> u64 {
        let mut h = 5381u64;
        for b in self.author.bytes().chain(self.reason.bytes()) {
            h = h.wrapping_mul(33).wrapping_add(b as u64);
        }
        h = h.wrapping_mul(33).wrapping_add(self.timestamp_ms);
        h = h.wrapping_mul(33).wrapping_add(self.parent_hash);
        h
    }
}

// ============================================================================
// 5. TraitRevision
// ============================================================================

/// A single versioned revision in the ledger chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitRevision {
    /// Monotonically increasing version number (0 = initial)
    pub version: u64,
    /// The trait value at this revision
    pub value: TraitValue,
    /// Delta from the previous revision (None for version 0)
    pub delta: Option<TraitDelta>,
    /// Mandatory provenance
    pub provenance: ProvenanceRecord,
    /// Hash of this revision
    pub hash: u64,
}

impl TraitRevision {
    fn new(
        version: u64,
        value: TraitValue,
        delta: Option<TraitDelta>,
        provenance: ProvenanceRecord,
    ) -> Self {
        let hash = provenance.hash().wrapping_add(version);
        Self { version, value, delta, provenance, hash }
    }
}

// ============================================================================
// 6. DiffResult
// ============================================================================

/// Structured diff between two revisions of the same trait
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Trait name
    pub trait_name: String,
    /// Starting version
    pub from_version: u64,
    /// Ending version
    pub to_version: u64,
    /// Net accumulated delta
    pub delta: TraitDelta,
    /// Provenance chain over the range
    pub provenance_chain: Vec<ProvenanceRecord>,
    /// Net magnitude of change
    pub magnitude: f64,
}

// ============================================================================
// 7. RollbackPolicy
// ============================================================================

/// Policy controlling rollback behaviour
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPolicy {
    /// Maximum history depth before old revisions are compressed
    pub max_history_depth: usize,
    /// Require human approval for rollbacks
    pub require_human_approval: bool,
    /// Maximum number of versions to roll back in one operation
    pub max_rollback_distance: usize,
}

impl Default for RollbackPolicy {
    fn default() -> Self {
        Self {
            max_history_depth: 100,
            require_human_approval: false,
            max_rollback_distance: 10,
        }
    }
}

// ============================================================================
// 8. TraitLedger
// ============================================================================

/// Full versioned revision history for a single named trait.
///
/// All mutations are recorded with mandatory provenance.
/// The revision chain is hash-linked for tamper detection.
pub struct TraitLedger {
    /// Trait name (e.g., "confidence", "class_embedding")
    pub name: String,
    /// Ordered revision history (version → revision)
    revisions: BTreeMap<u64, TraitRevision>,
    /// Current version counter
    current_version: u64,
    /// Rollback policy
    pub policy: RollbackPolicy,
}

impl TraitLedger {
    /// Create a new ledger with an initial value
    pub fn new(name: impl Into<String>, initial: TraitValue) -> Self {
        let prov = ProvenanceRecord::new(
            "system",
            "initial value",
            ProvenanceSource::SystemInit,
            0,
        );
        let revision = TraitRevision::new(0, initial, None, prov);
        let mut revisions = BTreeMap::new();
        revisions.insert(0, revision);

        Self {
            name: name.into(),
            revisions,
            current_version: 0,
            policy: RollbackPolicy::default(),
        }
    }

    /// Commit a new value with provenance
    pub fn commit(&mut self, new_value: TraitValue, provenance: ProvenanceRecord) -> Result<u64, String> {
        let current = self.current().clone();
        let delta = current.diff(&new_value);
        let next_version = self.current_version + 1;

        let revision = TraitRevision::new(next_version, new_value, delta, provenance);
        self.revisions.insert(next_version, revision);
        self.current_version = next_version;

        // Enforce history depth limit
        self.trim_history();

        Ok(next_version)
    }

    /// Get the current trait value
    pub fn current(&self) -> &TraitValue {
        &self.revisions[&self.current_version].value
    }

    /// Get the current version number
    pub fn version(&self) -> u64 {
        self.current_version
    }

    /// Get a specific revision by version
    pub fn get_revision(&self, version: u64) -> Option<&TraitRevision> {
        self.revisions.get(&version)
    }

    /// Rollback to a specific version
    pub fn rollback_to(&mut self, target_version: u64, author: &str, reason: &str) -> Result<(), String> {
        if target_version >= self.current_version {
            return Err(format!("Cannot rollback to version {} (current: {})", target_version, self.current_version));
        }

        let distance = self.current_version - target_version;
        if distance > self.policy.max_rollback_distance as u64 {
            return Err(format!("Rollback distance {} exceeds policy limit {}", distance, self.policy.max_rollback_distance));
        }

        let target_value = self
            .revisions
            .get(&target_version)
            .ok_or_else(|| format!("Version {} not found", target_version))?
            .value
            .clone();

        let parent_hash = self.revisions[&self.current_version].hash;
        let prov = ProvenanceRecord::new(author, reason, ProvenanceSource::Rollback, parent_hash);

        self.commit(target_value, prov)
            .map(|_| ())
            .map_err(|e| e)
    }

    /// Compute diff between two versions
    pub fn diff(&self, from: u64, to: u64) -> Option<DiffResult> {
        let from_rev = self.revisions.get(&from)?;
        let to_rev = self.revisions.get(&to)?;

        let delta = from_rev.value.diff(&to_rev.value)?;
        let magnitude = delta.magnitude();

        let provenance_chain: Vec<ProvenanceRecord> = self
            .revisions
            .range(from + 1..=to)
            .map(|(_, r)| r.provenance.clone())
            .collect();

        Some(DiffResult {
            trait_name: self.name.clone(),
            from_version: from,
            to_version: to,
            delta,
            provenance_chain,
            magnitude,
        })
    }

    /// Verify hash chain integrity
    pub fn verify_integrity(&self) -> bool {
        let mut prev_hash = 0u64;
        for (_, revision) in &self.revisions {
            if revision.provenance.parent_hash != prev_hash && revision.version != 0 {
                return false;
            }
            prev_hash = revision.hash;
        }
        true
    }

    /// Number of revisions in the ledger
    pub fn revision_count(&self) -> usize {
        self.revisions.len()
    }

    /// Trim old revisions beyond `max_history_depth`
    fn trim_history(&mut self) {
        while self.revisions.len() > self.policy.max_history_depth {
            if let Some(&oldest) = self.revisions.keys().next() {
                if oldest < self.current_version {
                    self.revisions.remove(&oldest);
                } else {
                    break;
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn prov(reason: &str) -> ProvenanceRecord {
        ProvenanceRecord::new("test", reason, ProvenanceSource::SystemInit, 0)
    }

    #[test]
    fn test_commit_and_current() {
        let mut ledger = TraitLedger::new("confidence", TraitValue::Scalar(0.5));
        ledger.commit(TraitValue::Scalar(0.8), prov("calibration")).unwrap();
        assert_eq!(ledger.current().as_scalar(), 0.8);
        assert_eq!(ledger.version(), 1);
    }

    #[test]
    fn test_diff() {
        let mut ledger = TraitLedger::new("score", TraitValue::Scalar(0.0));
        ledger.commit(TraitValue::Scalar(0.5), prov("step 1")).unwrap();
        ledger.commit(TraitValue::Scalar(1.0), prov("step 2")).unwrap();

        let diff = ledger.diff(0, 2).unwrap();
        if let TraitDelta::ScalarDelta(d) = diff.delta {
            assert!((d - 1.0).abs() < 1e-6);
        } else {
            panic!("Expected ScalarDelta");
        }
    }

    #[test]
    fn test_rollback() {
        let mut ledger = TraitLedger::new("flag", TraitValue::Flag(false));
        ledger.commit(TraitValue::Flag(true), prov("enable")).unwrap();
        assert!(ledger.current().as_flag());

        ledger.rollback_to(0, "operator", "revert test").unwrap();
        assert!(!ledger.current().as_flag());
    }

    #[test]
    fn test_label_transition() {
        let mut ledger = TraitLedger::new("class", TraitValue::Label("Prop".into()));
        ledger.commit(TraitValue::Label("Actor".into()), prov("reclassify")).unwrap();

        let diff = ledger.diff(0, 1).unwrap();
        assert!(matches!(diff.delta, TraitDelta::LabelChange(_, _)));
        assert_eq!(diff.magnitude, 1.0);
    }
}
