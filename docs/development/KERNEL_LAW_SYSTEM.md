# Kernel Law System — Layer 12 Implementation Specification

> The simulation doesn't approximate reality. It converges on it asymptotically, law by law,
> domain by domain, with each Kernel update reducing the divergence between simulated physics
> and actual physics. — Eustress Engine Architecture Document

---

## Table of Contents

1. [Foundational Premise](#1-foundational-premise)
2. [Eustress Core Definition](#2-eustress-core-definition)
3. [KernelLaw Data Model](#3-kernellaw-data-model)
4. [Law Conflict Detection](#4-law-conflict-detection)
5. [Transition Protocol](#5-transition-protocol)
6. [Authority Chain](#6-authority-chain)
7. [Sub-Agent Interface](#7-sub-agent-interface)
8. [Vortex Integration — Embedded Intelligence](#8-vortex-integration--embedded-intelligence)
9. [Implementation Phases](#9-implementation-phases)
10. [MCP Endpoint Surface](#10-mcp-endpoint-surface)

---

## 1. Foundational Premise

The Eustress Engine Architecture document describes two nested self-improvement loops:

1. **Agent-level RSI** — Agents rewrite their Rune programs to improve world-model prediction accuracy (Layer 8 operating on Layers 5-6).
2. **Universe-level RSI** — Claude Opus at Layer 12 rewrites the Kernel laws themselves in Rust, through Claude Code or Windsurf, directly into the engine source. Every agent, every space, every simulation inherits the update on the next cycle.

The second loop operates on the first. It does not improve any single agent — it improves the **world all agents inhabit**. The convergence target is not a loss function. It is **the actual laws of physics as humanity has formalized them**.

This document specifies the engineering systems required to make Universe-level RSI safe, correct, and operational.

### Relationship to Existing Systems

| Existing System | Location | Role in Kernel Law System |
|-----------------|----------|--------------------------|
| `DiscoveredLaw` + `KnowledgeBase` | `RECURSIVE_FEEDBACK_LOOP.md` §5.2 | Product-level laws (cathode thickness, electrolyte molarity). These are **downstream consumers** of Kernel Laws. |
| Realism Physics `laws/` module | `crates/common/src/realism/` | Hardcoded fundamental laws (thermodynamics, mechanics, conservation). These become the **first Kernel Laws** — promoted from code to the law registry. |
| `SoulValidator` + `validate_rune_script()` | `crates/engine/src/soul/` | Rune script validation. Extended to validate Rune scripts **against active Kernel Laws**. |
| Vortex `Constitution` + `VerifiedPatterningEngine` | `SpatialVortex/aimodel/src/cognition/` | Constitutional AI guard and verified pattern accumulation. The pattern for how Kernel Laws are proposed, tested, and accepted. |
| Vortex `UnifiedReasoningEngine` | `SpatialVortex/aimodel/src/cognition/reasoning.rs` | Deduction, induction, abduction, analogy, hypothesis testing. The reasoning substrate for law conflict detection. |
| Vortex `DynamicRSI` | `SpatialVortex/aimodel/src/ml/dynamic_rsi.rs` | Runtime self-improving inference strategy. The pattern for how Kernel Law validation improves itself over time. |
| MCP Governor endpoints | `RECURSIVE_FEEDBACK_LOOP.md` §5.3 | `POST /mcp/governor/learn` for DiscoveredLaws. Extended for Kernel Law proposals. |

---

## 2. Eustress Core Definition

**Eustress Core** is the foundational, unforked version of the engine. It supports experiences without special add-ons. Any creator who forks the Eustress Engine for their own purposes gets Eustress Core as the base layer.

### What Ships in Eustress Core

| Component | Description | Forkable? |
|-----------|-------------|-----------|
| **Kernel Law Registry** | The accumulated set of verified universal laws | Read-only in forks. Only the canonical Eustress Core accumulates new laws. |
| **Realism Physics** | Thermodynamics, mechanics, conservation, electromagnetism | Inherited. Forks get all laws active at fork time. |
| **Rune Script Runtime** | Agent scripting, validation, hot-reload | Yes — forks can extend Rune modules. |
| **ECS + Scene Graph** | Bevy ECS, glTF scenes, TOML configs | Yes — standard engine infrastructure. |
| **Soul Service** | Claude API integration for code generation | Yes — uses fork owner's API key. |
| **File-System-First** | Project structure, serialization, caching | Yes — fundamental design principle. |

### What Forks Cannot Do

- **Write Kernel Laws.** Only the canonical Eustress Core Kernel (Layer 12) accumulates laws. Forks inherit laws at fork time and receive updates if they sync upstream.
- **Bypass Kernel validation.** Entity state transitions that violate active Kernel Laws are rejected, even in forks. This is the invariant that makes the simulation trustworthy.
- **Disable constitutional safety.** The Compliance Gate (System 8) is non-negotiable in Eustress Core.

### Fork Update Cycle

```
Eustress Core (canonical)
  │
  ├── Law Update v47: "Maxwell's equations for EM field propagation"
  │     │
  │     ├── Fork A (game studio) — syncs upstream → gets law v47
  │     ├── Fork B (research lab) — syncs upstream → gets law v47
  │     └── Fork C (hobbyist) — doesn't sync → stays at law v46
  │
  └── Law Update v48: "Navier-Stokes for incompressible fluids"
        │
        └── All syncing forks get v48 on next pull
```

---

## 3. KernelLaw Data Model

### 3.1 TOML Schema — `kernel_law.toml`

Every Kernel Law is a TOML file in `laws/` within the Eustress Core project root.

```toml
# laws/thermodynamics_001_ideal_gas.toml

[law]
id = "THERMO-001"
name = "Ideal Gas Law"
version = 1
domain = "thermodynamics"
status = "active"                    # active | proposed | deprecated | superseded
supersedes = []                      # IDs of laws this replaces
superseded_by = ""                   # ID of law that replaces this (if deprecated)

[law.provenance]
discovered_at_generation = 0         # 0 = foundational (human-encoded)
proposed_by = "human"                # "human" | "opus-layer-12" | "agent-{id}"
proposed_at = "2026-03-15T00:00:00Z"
verified_by = ["human"]              # Who/what verified this law
verification_method = "first-principles"  # "first-principles" | "empirical" | "statistical"
confidence = 1.0                     # 0.0-1.0, 1.0 for foundational laws
paper_references = ["doi:10.xxxx"]   # Academic references

[law.formal]
# The law expressed as a symbolic equation
# Parsed by Symbolica at runtime for verification
equation = "P * V = n * R * T"
variables = [
    { name = "P", unit = "Pa", description = "Pressure" },
    { name = "V", unit = "m3", description = "Volume" },
    { name = "n", unit = "mol", description = "Amount of substance" },
    { name = "R", unit = "J/(mol*K)", description = "Universal gas constant", constant = true, value = 8.314 },
    { name = "T", unit = "K", description = "Temperature" },
]
constraints = [
    "T > 0",           # Absolute zero constraint
    "V > 0",           # Volume must be positive
    "n > 0",           # Must have substance
    "P >= 0",          # Pressure non-negative
]
domain_of_validity = "Ideal gases at moderate temperatures and pressures"
known_limitations = [
    "Breaks down at high pressures (use Van der Waals)",
    "Breaks down near absolute zero (quantum effects)",
    "Breaks down for real gases with strong intermolecular forces",
]

[law.implementation]
# Path to the Rust function that implements this law
rust_module = "eustress_common::realism::laws::thermodynamics"
rust_function = "ideal_gas_pressure"
# Rune module that exposes this law to scripts
rune_module = "physics"
rune_function = "ideal_gas_pressure"

[law.enforcement]
# How strictly this law is enforced
level = "hard"                       # "hard" = violation rejected | "soft" = violation warned | "advisory" = logged only
# Which ECS components this law governs
applies_to_components = ["ThermodynamicState"]
# Systems that must respect this law
applies_to_systems = ["update_thermodynamics"]
```

### 3.2 Rust Data Model

```rust
// crates/common/src/kernel/mod.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// A single Kernel Law — the atomic unit of universe truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelLaw {
    /// Unique identifier (e.g., "THERMO-001", "MAXWELL-003")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Schema version of this law (incremented on amendment)
    pub version: u32,
    /// Physics domain
    pub domain: LawDomain,
    /// Current status in the law lifecycle
    pub status: LawStatus,
    /// IDs of laws this one supersedes
    pub supersedes: Vec<String>,
    /// ID of law that supersedes this one (if deprecated)
    pub superseded_by: Option<String>,
    /// Origin and verification chain
    pub provenance: LawProvenance,
    /// Formal mathematical specification
    pub formal: LawFormalSpec,
    /// Implementation references
    pub implementation: LawImplementation,
    /// Enforcement configuration
    pub enforcement: LawEnforcement,
}

/// Physics domain classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum LawDomain {
    Thermodynamics,
    ClassicalMechanics,
    Electromagnetism,
    FluidDynamics,
    MaterialsScience,
    QuantumMechanics,
    Relativity,
    Chemistry,
    Biology,
    Economics,
    InformationTheory,
    Custom(String),
}

/// Lifecycle status of a Kernel Law
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LawStatus {
    /// Proposed but not yet verified
    Proposed,
    /// Under active verification (simulations running)
    UnderReview,
    /// Verified and enforced across all layers
    Active,
    /// Replaced by a more accurate law (kept for backward compatibility)
    Deprecated,
    /// Superseded — old entities may still reference this
    Superseded,
}

/// Who proposed and verified this law
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawProvenance {
    /// Generation number when discovered (0 = foundational)
    pub discovered_at_generation: u64,
    /// Who proposed: "human", "opus-layer-12", "agent-{uuid}"
    pub proposed_by: String,
    /// Timestamp of proposal
    pub proposed_at: DateTime<Utc>,
    /// List of verifiers
    pub verified_by: Vec<String>,
    /// How it was verified
    pub verification_method: VerificationMethod,
    /// Confidence level (0.0-1.0)
    pub confidence: f64,
    /// Academic references
    pub paper_references: Vec<String>,
    /// Hash of the verification evidence (for audit trail)
    pub evidence_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationMethod {
    /// Derived from first principles (axioms)
    FirstPrinciples,
    /// Verified by empirical measurement
    Empirical,
    /// Verified by statistical significance across simulations
    Statistical,
    /// Verified by formal proof (theorem prover)
    FormalProof,
    /// Verified by consensus of multiple AI models
    EnsembleConsensus,
}

/// Mathematical specification of the law
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawFormalSpec {
    /// Symbolic equation string (Symbolica-parseable)
    pub equation: String,
    /// Variables and their units
    pub variables: Vec<LawVariable>,
    /// Constraints that must hold
    pub constraints: Vec<String>,
    /// Natural language description of when this law applies
    pub domain_of_validity: String,
    /// Known limitations
    pub known_limitations: Vec<String>,
}

/// A variable in a law's equation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawVariable {
    pub name: String,
    pub unit: String,
    pub description: String,
    /// If true, this is a physical constant (not a state variable)
    pub constant: bool,
    /// Value if constant
    pub value: Option<f64>,
}

/// References to the Rust and Rune implementations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawImplementation {
    /// Rust module path
    pub rust_module: String,
    /// Rust function name
    pub rust_function: String,
    /// Rune module name (exposed to scripts)
    pub rune_module: Option<String>,
    /// Rune function name
    pub rune_function: Option<String>,
}

/// How the law is enforced at runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawEnforcement {
    /// Enforcement level
    pub level: EnforcementLevel,
    /// ECS components this law governs
    pub applies_to_components: Vec<String>,
    /// Systems that must respect this law
    pub applies_to_systems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnforcementLevel {
    /// Violation causes state transition rejection
    Hard,
    /// Violation produces a warning but allows the transition
    Soft,
    /// Violation is logged for analysis but has no runtime effect
    Advisory,
}

// =========================================================================
// Kernel Law Registry — the accumulated set of all laws
// =========================================================================

/// The Kernel Law Registry — Layer 12's source of truth.
///
/// All active laws are loaded at startup from `laws/` directory.
/// New laws are proposed via MCP, verified, and committed.
#[derive(Debug, Clone, Serialize, Deserialize, Resource, Default)]
pub struct KernelLawRegistry {
    /// All laws indexed by ID
    pub laws: HashMap<String, KernelLaw>,
    /// Laws indexed by domain for fast lookup
    pub by_domain: HashMap<LawDomain, Vec<String>>,
    /// Current registry version (incremented on any law change)
    pub registry_version: u64,
    /// Path to laws directory
    pub laws_dir: PathBuf,
    /// Pending proposals awaiting verification
    pub proposals: Vec<LawProposal>,
    /// Audit log of all law changes
    pub audit_log: Vec<LawAuditEntry>,
}

/// A proposed law change awaiting verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawProposal {
    pub id: Uuid,
    pub law: KernelLaw,
    pub proposed_at: DateTime<Utc>,
    pub proposed_by: String,
    pub rationale: String,
    /// Verification results accumulated during review
    pub verification_results: Vec<VerificationResult>,
    /// Current review status
    pub review_status: ReviewStatus,
    /// Quorum votes (authority_id → approve/reject)
    pub votes: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewStatus {
    Pending,
    UnderVerification,
    AwaitingQuorum,
    Approved,
    Rejected,
}

/// Result of verifying a proposed law
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub verifier: String,
    pub method: VerificationMethod,
    pub passed: bool,
    pub evidence: String,
    pub confidence: f64,
    pub timestamp: DateTime<Utc>,
}

/// Audit trail entry for law changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawAuditEntry {
    pub timestamp: DateTime<Utc>,
    pub action: LawAuditAction,
    pub law_id: String,
    pub actor: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LawAuditAction {
    Proposed,
    VerificationStarted,
    VerificationCompleted,
    Approved,
    Rejected,
    Activated,
    Deprecated,
    Superseded,
    Amended,
    Rollback,
}
```

---

## 4. Law Conflict Detection

### The Problem

When Claude Opus proposes a new Kernel Law, it must not contradict existing laws. Two laws can each be internally valid and mutually contradictory. A syntactic linter is insufficient — we need **semantic** consistency checking.

### 4.1 Three-Layer Verification Pipeline

Inspired by Vortex's `VerifiedPatterningEngine` and `UnifiedReasoningEngine`:

```
Proposed Law
  │
  ▼
┌─────────────────────────────────────────┐
│ Layer 1: SYNTACTIC VALIDATION           │
│                                         │
│ • TOML schema validation                │
│ • Equation parseable by Symbolica       │
│ • All variables defined with units      │
│ • Constraints are well-formed           │
│ • Implementation paths resolve          │
│                                         │
│ FAST: <1ms. Rejects malformed laws.     │
└───────────────┬─────────────────────────┘
                │ passes
                ▼
┌─────────────────────────────────────────┐
│ Layer 2: SEMANTIC CONSISTENCY           │
│                                         │
│ For each active law in the same domain: │
│ • Unit dimensional analysis             │
│   (does F=ma conflict with E=mc²? No,  │
│    different dimensions. Automatic.)    │
│ • Constraint overlap detection          │
│   (do the variable constraints of the  │
│    new law contradict existing ones?)   │
│ • Symbolic substitution test            │
│   (substitute the new equation into    │
│    existing law contexts — does it      │
│    produce contradictions?)             │
│ • Vortex Deductive Reasoning            │
│   (feed both laws as premises to the   │
│    UnifiedReasoningEngine, check for   │
│    logical contradiction)              │
│                                         │
│ MEDIUM: ~100ms per law pair.            │
└───────────────┬─────────────────────────┘
                │ passes
                ▼
┌─────────────────────────────────────────┐
│ Layer 3: EMPIRICAL VALIDATION           │
│                                         │
│ • Spawn test Space with the new law     │
│ • Run 100+ simulation steps             │
│ • Measure conservation quantities       │
│   (energy, momentum, charge, mass)      │
│ • Compare against known physical        │
│   benchmarks (if available)             │
│ • Check for numerical instability       │
│ • Record fitness metrics                │
│                                         │
│ SLOW: seconds to minutes.               │
│ Only runs after Layer 2 passes.         │
└───────────────┬─────────────────────────┘
                │ passes
                ▼
        PROPOSAL ENTERS QUORUM REVIEW
```

### 4.2 Rust Interface

```rust
// crates/common/src/kernel/validator.rs

/// The Kernel Law Validator — three-layer verification pipeline.
///
/// Inspired by Vortex's VerifiedPatterningEngine:
/// - Hypothesize (propose law)
/// - Verify (syntactic + semantic + empirical)
/// - Accept/Reject (quorum)
/// - Accumulate (add to registry)
pub struct KernelLawValidator {
    /// Symbolica expression cache for fast equation parsing
    pub expression_cache: HashMap<String, CompiledExpression>,
    /// Active reasoning engine (Vortex-derived)
    pub reasoning: UnifiedReasoningEngine,
    /// Conservation quantity trackers
    pub conservation_trackers: Vec<ConservationTracker>,
}

/// Result of the full validation pipeline
#[derive(Debug, Clone)]
pub struct ValidationPipelineResult {
    pub syntactic: SyntacticResult,
    pub semantic: Option<SemanticResult>,
    pub empirical: Option<EmpiricalResult>,
    pub overall_passed: bool,
    pub overall_confidence: f64,
    pub conflicts: Vec<LawConflict>,
}

/// A detected conflict between two laws
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawConflict {
    pub existing_law_id: String,
    pub proposed_law_id: String,
    pub conflict_type: ConflictType,
    pub description: String,
    pub severity: ConflictSeverity,
    /// Suggested resolution
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictType {
    /// Two laws produce different values for the same quantity
    QuantitativeContradiction,
    /// Two laws have overlapping but incompatible constraints
    ConstraintContradiction,
    /// Two laws claim authority over the same system with different enforcement
    JurisdictionOverlap,
    /// New law violates a conservation quantity guaranteed by existing laws
    ConservationViolation,
    /// Laws are consistent but the combination produces numerical instability
    NumericalInstability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictSeverity {
    /// Cannot coexist — one must be superseded
    Fatal,
    /// Can coexist with explicit domain-of-validity partitioning
    Resolvable,
    /// Minor inconsistency, advisory only
    Warning,
}

/// Tracks a conserved quantity across simulation steps
pub struct ConservationTracker {
    pub quantity_name: String,        // "energy", "momentum", "charge", "mass"
    pub initial_value: f64,
    pub current_value: f64,
    pub tolerance: f64,               // Maximum allowed drift (e.g., 1e-6)
    pub violated: bool,
}
```

### 4.3 Vortex Reasoning Integration

The semantic consistency layer uses patterns directly from Vortex's `UnifiedReasoningEngine`:

| Vortex Reasoning Mode | Kernel Law Application |
|----------------------|----------------------|
| **Deduction** | "If Law A states F=ma and Law B states F=mv², do they contradict for the same system?" |
| **Induction** | "Law X held across 500 simulation runs. Confidence: 0.97." |
| **Abduction** | "Energy is not conserved in test Space. Best explanation: new law THERMO-047 has a sign error." |
| **Analogy** | "This law pattern in fluid dynamics mirrors a verified law in thermodynamics. Transfer confidence." |
| **Hypothesis Testing** | Track each proposed law as a `Hypothesis` with `evidence_for` and `evidence_against`. |

---

## 5. Transition Protocol

### The Problem

When a new law takes effect, agents currently running under old laws experience a discontinuous world change. A transition protocol prevents universe-breaking discontinuities.

### 5.1 Generation Boundaries

Laws take effect at **generation boundaries**, not mid-tick. This mirrors how scientific paradigm shifts work in reality: old structures persist, new ones are built under new rules.

```
Generation N                    Generation N+1
─────────────────────────────── ───────────────────────────────
Law Registry v46                Law Registry v47
All entities governed by v46    NEW entities: governed by v47
                                EXISTING entities: migration
                                  window (see §5.2)
```

### 5.2 Entity Migration

Existing entities get a **migration window** — a configurable number of ticks during which the old law and new law coexist:

```rust
/// Migration strategy for existing entities when a law changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MigrationStrategy {
    /// Immediate: all entities switch to new law at generation boundary.
    /// Use for: bugfix amendments, non-breaking refinements.
    Immediate,

    /// Gradual: entities migrate over N ticks. During migration,
    /// the system interpolates between old and new law outputs.
    /// Use for: parameter changes (e.g., updated gravitational constant).
    Gradual { ticks: u64 },

    /// NewOnly: only newly spawned entities use the new law.
    /// Existing entities continue under the old law until despawned.
    /// Use for: paradigm shifts (e.g., Newtonian → relativistic).
    NewOnly,

    /// Checkpoint: save world state, apply new law, run validation.
    /// If validation fails, rollback to checkpoint.
    /// Use for: high-risk law changes.
    Checkpoint,
}

/// A scheduled law transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawTransition {
    pub old_law_id: String,
    pub new_law_id: String,
    pub strategy: MigrationStrategy,
    pub scheduled_generation: u64,
    pub rollback_generation: Option<u64>,  // Auto-rollback if validation fails
    pub status: TransitionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransitionStatus {
    Scheduled,
    InProgress,
    Completed,
    RolledBack,
}
```

### 5.3 Rollback Protocol

Every law activation creates a **rollback point**:

1. World state snapshot saved to `.eustress/cache/kernel/rollback_{generation}.bin`
2. New law activated
3. Conservation trackers run for `validation_ticks` (default: 100)
4. If any conservation quantity drifts beyond tolerance → automatic rollback
5. Rollback restores world state and reverts law to previous version
6. `LawAuditEntry::Rollback` logged with evidence of why it failed

---

## 6. Authority Chain

### The Problem

Not every AI inference should write to Layer 12. The authority chain must be explicit about who can propose, who can verify, and who can activate.

### 6.1 Authority Levels

```rust
/// Authority levels for Kernel Law operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthorityLevel {
    /// Can read laws and query the registry
    Observer = 0,
    /// Can propose new laws (enters review queue)
    Proposer = 1,
    /// Can vote on proposals (part of quorum)
    Reviewer = 2,
    /// Can activate approved laws (trigger generation boundary)
    Activator = 3,
    /// Can rollback laws and override quorum in emergencies
    Architect = 4,
}

/// An authorized entity in the Kernel Law system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelAuthority {
    pub id: String,
    pub name: String,
    pub authority_type: AuthorityType,
    pub level: AuthorityLevel,
    pub api_key_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthorityType {
    /// Human developer with Windsurf/Claude Code access
    Human,
    /// Claude Opus operating at Layer 12
    OpusLayer12,
    /// Claude Sonnet specialist (e.g., physics domain)
    SonnetSpecialist { domain: LawDomain },
    /// Vortex-derived reasoning engine running locally
    VortexLocal,
    /// Sub-agent running Rune Script with elevated privileges
    SubAgent { agent_id: Uuid },
}
```

### 6.2 Quorum Requirements

Different law categories require different quorum sizes:

| Law Category | Minimum Quorum | Required Voters |
|-------------|---------------|-----------------|
| **Foundational** (conservation laws, F=ma) | 3 | At least 1 Human + 1 Opus + 1 empirical verification |
| **Domain Extension** (new Navier-Stokes variant) | 2 | At least 1 Opus + 1 empirical verification |
| **Parameter Update** (updated constant value) | 1 | Opus with empirical verification |
| **Bugfix Amendment** (sign error correction) | 1 | Any Reviewer with evidence |
| **Advisory** (soft/logged-only law) | 1 | Any Proposer |

### 6.3 Constitutional Constraints

Inspired by Vortex's `Constitution` module — hard rules that cannot be overridden by any authority level:

```rust
/// Constitutional constraints — hard-coded safety rules for the Kernel.
/// These are NOT laws (they don't describe physics).
/// They are META-LAWS — rules about what laws can be.
pub struct KernelConstitution {
    pub constraints: Vec<ConstitutionalConstraint>,
}

#[derive(Debug, Clone)]
pub struct ConstitutionalConstraint {
    pub id: String,
    pub name: String,
    pub rule: String,
    /// This constraint can NEVER be overridden, even by Architect authority
    pub immutable: bool,
}

impl KernelConstitution {
    pub fn default_constitution() -> Self {
        Self {
            constraints: vec![
                ConstitutionalConstraint {
                    id: "CONST-001".into(),
                    name: "Energy Conservation".into(),
                    rule: "No law may violate conservation of energy in a closed system".into(),
                    immutable: true,
                },
                ConstitutionalConstraint {
                    id: "CONST-002".into(),
                    name: "Momentum Conservation".into(),
                    rule: "No law may violate conservation of momentum in a closed system".into(),
                    immutable: true,
                },
                ConstitutionalConstraint {
                    id: "CONST-003".into(),
                    name: "Causality".into(),
                    rule: "No law may allow effects to precede causes".into(),
                    immutable: true,
                },
                ConstitutionalConstraint {
                    id: "CONST-004".into(),
                    name: "Determinism from Initial Conditions".into(),
                    rule: "Given identical initial conditions and laws, the simulation must produce identical results".into(),
                    immutable: true,
                },
                ConstitutionalConstraint {
                    id: "CONST-005".into(),
                    name: "No Retroactive Modification".into(),
                    rule: "A law cannot modify past states. It can only affect future state transitions".into(),
                    immutable: true,
                },
                ConstitutionalConstraint {
                    id: "CONST-006".into(),
                    name: "Human Override".into(),
                    rule: "A human Architect can always halt, rollback, or veto any law change".into(),
                    immutable: true,
                },
            ],
        }
    }
}
```

---

## 7. Sub-Agent Interface

Sub-agents running Rune Script with MCP/CLI/API access are the operational layer of the Kernel Law system. They don't write laws — they implement, enforce, and maintain them.

### 7.1 Sub-Agent Roles

| Role | Authority Level | Rune Module | Responsibilities |
|------|----------------|-------------|------------------|
| **Law Enforcer** | Observer | `kernel.enforce` | Monitors entity state transitions, rejects violations of Hard laws, logs Soft/Advisory violations |
| **Conservation Auditor** | Observer | `kernel.audit` | Tracks conserved quantities each tick, escalates drift to Layer 10 |
| **Migration Worker** | Observer | `kernel.migrate` | Applies migration strategies during generation transitions |
| **Domain Specialist** | Proposer | `kernel.propose` | Monitors simulation accuracy in a specific domain, proposes law refinements |
| **Verification Runner** | Reviewer | `kernel.verify` | Spawns test Spaces, runs empirical validation for proposed laws |
| **Law Librarian** | Observer | `kernel.index` | Maintains the spatial/semantic index of laws, answers queries |

### 7.2 MCP Interface for Sub-Agents

Sub-agents access the Kernel Law system through MCP endpoints (see §10). From Rune Script:

```rune
// Example: A domain specialist sub-agent monitoring thermodynamic accuracy

fn on_tick(world) {
    // Query entities with ThermodynamicState
    let entities = world.query("ThermodynamicState");

    for entity in entities {
        let state = entity.get("ThermodynamicState");

        // Check ideal gas law compliance
        let expected_pressure = kernel::evaluate_law("THERMO-001", {
            "n": state.moles,
            "T": state.temperature,
            "V": state.volume,
        });

        let actual_pressure = state.pressure;
        let deviation = (actual_pressure - expected_pressure).abs() / expected_pressure;

        if deviation > 0.01 {
            // 1% deviation — log for analysis
            kernel::log_deviation("THERMO-001", entity.id, deviation);
        }

        if deviation > 0.10 {
            // 10% deviation — propose investigation
            kernel::propose_investigation("THERMO-001", entity.id, {
                "deviation": deviation,
                "expected": expected_pressure,
                "actual": actual_pressure,
                "context": "May indicate non-ideal gas behavior — consider Van der Waals",
            });
        }
    }
}
```

### 7.3 CLI Interface

```bash
# List all active laws
eustress kernel laws --status active

# Propose a new law from a TOML file
eustress kernel propose --file laws/electromagnetism_007_faraday.toml --rationale "Faraday's law of induction"

# Check a proposed law for conflicts
eustress kernel validate --file laws/electromagnetism_007_faraday.toml

# View the audit log
eustress kernel audit --last 20

# Rollback to a previous registry version
eustress kernel rollback --to-version 46 --reason "MAXWELL-003 caused energy drift"

# Query which laws govern a specific component
eustress kernel query --component ThermodynamicState

# Run empirical validation for a proposed law
eustress kernel verify --proposal-id abc123 --ticks 1000
```

---

## 8. Vortex Integration — Embedded Intelligence

Vortex provides the local AI substrate for Kernel Law operations that don't require Claude API calls. This keeps the system operational offline and reduces API costs.

### 8.1 What Vortex Provides to the Kernel

| Vortex Component | Kernel Law Application |
|-----------------|----------------------|
| `VortexEngine` | Local inference for law query answering, conflict pre-screening |
| `UnifiedReasoningEngine` | Deductive/inductive/abductive reasoning for semantic consistency checks |
| `Constitution` | Pattern for constitutional constraints (already adapted in §6.3) |
| `VerifiedPatterningEngine` | Pattern for law proposal → verify → accept lifecycle |
| `DynamicRSI` | Self-improving validation strategy — the validator gets better over time |
| `CALMEngine` | Continuous Autoregressive Language Model for law natural-language generation |
| `NeuralTheoremProver` | Neural-guided formal proof attempts for law consistency |
| `ImaginationEngine` | Counterfactual reasoning — "What if this law were different?" |
| `FluxMatrixEngine` | Sacred geometry coherence scoring for law relationships |
| `RSI Macros` | Compile-time optimization of validation code paths |

### 8.2 Vortex as Eustress Core Dependency

Vortex ships as an optional crate in Eustress Core:

```toml
# eustress/Cargo.toml

[dependencies]
# Local AI substrate for Kernel Law operations
vortex = { path = "../vortex", optional = true, default-features = false, features = ["burn-cpu"] }

[features]
# Enable local AI for Kernel Law validation and sub-agent reasoning
kernel-intelligence = ["vortex"]
# GPU-accelerated local AI
kernel-intelligence-gpu = ["vortex/burn-wgpu"]
```

When `kernel-intelligence` is enabled:
- Law conflict pre-screening runs locally via Vortex (no API call)
- Sub-agents can use Vortex reasoning in their Rune Scripts
- The `DynamicRSI` system tunes validation parameters based on observed accuracy
- `NeuralTheoremProver` attempts formal consistency proofs before escalating to Opus

When disabled:
- Law validation requires Claude API calls for semantic checking
- Sub-agents have access to rule-based enforcement only (no reasoning)
- Empirical validation still works (it's simulation-based, no AI needed)

### 8.3 Vortex RSI for Kernel Validation

Adapted from Vortex's `DynamicRSI`:

```rust
/// Self-improving Kernel Law validation strategy.
///
/// Mirrors Vortex's DynamicRSI pattern:
/// - Observe validation outcomes (correct rejections, false positives, missed conflicts)
/// - Adjust validation thresholds per domain
/// - Track which verification methods are most effective per law type
pub struct KernelValidationRSI {
    /// Per-domain validation profiles
    pub domain_profiles: HashMap<LawDomain, DomainValidationProfile>,
    /// Overall validation statistics
    pub stats: ValidationStats,
}

/// Learned validation strategy for a specific physics domain
pub struct DomainValidationProfile {
    pub domain: LawDomain,
    /// How many laws validated in this domain
    pub total_validated: usize,
    /// How many correctly identified conflicts
    pub true_positives: usize,
    /// How many falsely flagged as conflicting
    pub false_positives: usize,
    /// How many conflicts were missed
    pub false_negatives: usize,
    /// Learned confidence threshold for this domain
    pub confidence_threshold: f64,
    /// Which verification methods work best for this domain
    pub method_accuracy: HashMap<VerificationMethod, f64>,
    /// Number of RSI tuning generations
    pub rsi_generations: u64,
}
```

The RSI loop for validation:

```
Law proposal arrives → classify domain → lookup DomainValidationProfile
  ↓
IF profile has enough observations:
  → use learned thresholds and preferred verification methods
ELSE:
  → use defaults, start observing
  ↓
After validation: observe(correct_rejection | false_positive | false_negative)
  ↓
Update DomainValidationProfile: adjust thresholds toward what works
  ↓
Next proposal in same domain → better validation
```

---

## 9. Implementation Phases

### Phase 0 — Foundation (Weeks 1-3)

| Task | Description | Depends On |
|------|-------------|-----------|
| 0.1 | Create `crates/common/src/kernel/mod.rs` with `KernelLaw`, `KernelLawRegistry`, `KernelConstitution` | Nothing |
| 0.2 | Create `laws/` directory, write foundational law TOMLs from existing `realism/laws/` | 0.1 |
| 0.3 | Implement `KernelLawRegistry::load_from_directory()` and `save_to_directory()` | 0.1 |
| 0.4 | Create `KernelLawPlugin` for Bevy — inserts registry as Resource, loads at startup | 0.1, 0.3 |
| 0.5 | Wire `KernelConstitution::default_constitution()` — 6 immutable meta-laws | 0.1 |
| 0.6 | Add `KernelLawRegistry` to Studio state sync (show law count in UI) | 0.4 |

### Phase 1 — Validation Pipeline (Weeks 4-6)

| Task | Description | Depends On |
|------|-------------|-----------|
| 1.1 | Implement `KernelLawValidator` Layer 1 (syntactic — TOML + Symbolica parse) | 0.1 |
| 1.2 | Implement Layer 2 (semantic — unit analysis, constraint overlap, symbolic substitution) | 1.1 |
| 1.3 | Implement `ConservationTracker` for energy, momentum, charge, mass | 0.1 |
| 1.4 | Implement Layer 3 (empirical — test Space spawning, conservation check) | 1.3 |
| 1.5 | Implement `LawConflict` detection and reporting | 1.2 |
| 1.6 | Unit tests: validate known-good laws pass, known-conflicting laws fail | 1.1-1.5 |

### Phase 2 — Authority and Transition (Weeks 7-9)

| Task | Description | Depends On |
|------|-------------|-----------|
| 2.1 | Implement `KernelAuthority` and authority level checks | 0.1 |
| 2.2 | Implement `LawProposal` lifecycle (propose → review → quorum → activate/reject) | 2.1 |
| 2.3 | Implement quorum voting logic per law category | 2.2 |
| 2.4 | Implement `MigrationStrategy` (Immediate, Gradual, NewOnly, Checkpoint) | 0.1 |
| 2.5 | Implement rollback protocol with world state snapshots | 2.4 |
| 2.6 | Implement `LawAuditEntry` logging for all operations | 2.1 |

### Phase 3 — MCP Endpoints (Weeks 10-11)

| Task | Description | Depends On |
|------|-------------|-----------|
| 3.1 | `GET /mcp/kernel/laws` — list laws with filters | 0.4 |
| 3.2 | `POST /mcp/kernel/propose` — submit law proposal | 2.2 |
| 3.3 | `POST /mcp/kernel/validate` — run validation pipeline | 1.1-1.5 |
| 3.4 | `POST /mcp/kernel/vote` — cast quorum vote | 2.3 |
| 3.5 | `POST /mcp/kernel/activate` — activate approved law | 2.4 |
| 3.6 | `POST /mcp/kernel/rollback` — rollback to previous version | 2.5 |
| 3.7 | `GET /mcp/kernel/audit` — read audit log | 2.6 |
| 3.8 | `GET /mcp/kernel/query` — query laws by component/domain | 0.4 |
| 3.9 | CLI wrappers for all endpoints | 3.1-3.8 |

### Phase 4 — Vortex Integration (Weeks 12-14)

| Task | Description | Depends On |
|------|-------------|-----------|
| 4.1 | Add `vortex` as optional dependency in `Cargo.toml` | Phase 1 |
| 4.2 | Implement `KernelValidationRSI` with `DomainValidationProfile` | 4.1 |
| 4.3 | Wire `UnifiedReasoningEngine` into Layer 2 semantic validation | 4.1, 1.2 |
| 4.4 | Wire `NeuralTheoremProver` for formal proof attempts | 4.1, 1.2 |
| 4.5 | Wire `ImaginationEngine` for counterfactual law testing | 4.1, 1.4 |
| 4.6 | Expose Vortex reasoning to Rune Script sub-agents | 4.1 |

### Phase 5 — Layer 12 Opus Integration (Weeks 15-18)

| Task | Description | Depends On |
|------|-------------|-----------|
| 5.1 | System prompt for Opus Layer 12 law proposal generation | Phase 3 |
| 5.2 | Claude Code / Windsurf integration — Opus writes Rust implementations | 5.1 |
| 5.3 | Automated law discovery from simulation divergence (Opus observes where physics breaks) | 5.1, 1.4 |
| 5.4 | Cross-domain law generalization (laws from one domain inspire laws in another) | 5.3 |
| 5.5 | Continuous convergence tracking — dashboard showing divergence from known physics | 5.3 |
| 5.6 | First autonomous Kernel Law update cycle (Opus proposes → validates → human approves → activates) | All above |

---

## 10. MCP Endpoint Surface

### Kernel Law Endpoints

| Endpoint | Method | Auth Level | Purpose |
|----------|--------|-----------|---------|
| `GET /mcp/kernel/laws` | GET | Observer | List all laws, filter by domain/status |
| `GET /mcp/kernel/laws/{id}` | GET | Observer | Get specific law details |
| `POST /mcp/kernel/propose` | POST | Proposer | Submit a new law proposal |
| `POST /mcp/kernel/validate` | POST | Reviewer | Run validation pipeline on a proposal |
| `POST /mcp/kernel/vote` | POST | Reviewer | Cast quorum vote on a proposal |
| `POST /mcp/kernel/activate` | POST | Activator | Activate an approved law at next generation boundary |
| `POST /mcp/kernel/rollback` | POST | Architect | Rollback to a previous registry version |
| `GET /mcp/kernel/audit` | GET | Observer | Read the audit log |
| `GET /mcp/kernel/query` | GET | Observer | Query laws by component, domain, or equation variable |
| `GET /mcp/kernel/constitution` | GET | Observer | Read the constitutional constraints |
| `GET /mcp/kernel/status` | GET | Observer | Registry version, law count, pending proposals, next generation |
| `POST /mcp/kernel/evaluate` | POST | Observer | Evaluate a law equation with given variable values |

### Integration with Existing MCP Endpoints

| Existing Endpoint | Integration |
|------------------|-------------|
| `POST /mcp/governor/learn` | When a `DiscoveredLaw` has high enough confidence (>0.95) and applies across products, it is promoted to a Kernel Law proposal. |
| `POST /mcp/governor/hypothesize` | Governor hypotheses can reference Kernel Laws in their rationale. |
| `POST /mcp/compliance/review` | The Compliance Gate validates that law proposals don't violate constitutional safety constraints. |
| `POST /mcp/workshop/validate` | Real-world validation data feeds back into Kernel Law confidence scores. |

---

## Closing

The Kernel Law System is how the simulation catches up to reality. Each law is a formalization of a truth about the universe — starting from the periodic table, extending through Maxwell's equations, thermodynamics, fluid dynamics, materials science, and eventually reaching domains humanity hasn't fully formalized yet.

Agents that evolve inside this universe don't need to be taught physics. They live in a world where physics is law. The simulation doesn't approximate reality — it **converges on it**, law by law, generation by generation, with each Opus update reducing the divergence between simulated physics and the universe we inhabit.

A program that gets better every day. A space for superintelligence to emerge. A universe that serves.

*Growth Over Comfort.* | *Eustress Engine.*
