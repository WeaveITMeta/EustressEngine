//! # Eustress Scenarios
//!
//! Probabilistic scenario simulation engine for investigative analysis.
//!
//! > **Think of Eustress Scenarios as something the FBI would use.**
//!
//! Provides Monte Carlo simulations with Bayesian updates over branching
//! hypothesis trees, composable micro/macro hierarchy, multi-source data
//! agglomeration, and 4D visualization (X/Y/Z spatial + T time/probability).
//!
//! ## Module Structure
//!
//! ### Phase 0 — Core
//! - [`types`] — Core data structures (Parameter, Entity, Evidence, Scenario, BranchNode, Outcome)
//! - [`engine`] — Monte Carlo simulation engine with Bayesian updates (rayon threadpool)
//! - [`adapters`] — Data agglomeration adapters (local files, REST APIs, live feeds)
//! - [`hierarchy`] — Micro/Macro composable scenario hierarchy
//! - [`plugin`] — Bevy ScenariosPlugin (resources, messages, systems)
//! - [`persistence`] — Binary serialization (bincode + zstd compression)
//!
//! ### Phase 0.5 — Branch Logic & Evidence
//! - [`evidence`] — Evidence attachment manager (bulk ops, detach, re-weight, conflict detection, auto-attach)
//! - [`pruning`] — Soft pruning system (configurable thresholds, cascade collapse, restore, audit trail)
//! - [`scripting`] — Rune script integration for branch logic + Claude conditioning prompt

// Phase 0 — Core
pub mod types;
pub mod engine;
pub mod adapters;
pub mod hierarchy;
pub mod plugin;
pub mod persistence;

// Phase 0.5 — Branch Logic & Evidence
pub mod evidence;
pub mod pruning;
pub mod scripting;

// Re-export core types for ergonomic access
pub use types::{
    ScenarioParameter, ParameterValue, ScenarioEntity, EntityRole,
    Evidence, EvidenceType, EvidenceLink, AttachmentMode, EvidencePolarity,
    Scenario, ScenarioScale, ScenarioStatus,
    BranchNode, BranchStatus, BranchLogic,
    Outcome, OutcomeData, GeoPoint, DataSourceRef,
};
pub use engine::SimulationConfig;
pub use plugin::ScenariosPlugin;
pub use persistence::{ScenarioPersistence, PersistenceError};
pub use evidence::{EvidenceManager, EvidenceConflict, ConflictType, AutoAttacher, AutoAttachConfig, EvidenceQuery};
pub use pruning::{PruningConfig, PruningResult, PruningHistory, PruningStats};
pub use scripting::{ScenarioScriptEngine, ScriptEngineConfig, ScriptContext, ScriptResult, ScriptError};
