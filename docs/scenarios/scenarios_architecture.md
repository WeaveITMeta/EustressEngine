# Eustress Scenarios — Finalized Architecture

> **Module Location:** `eustress/crates/engine/src/scenarios/`  
> **Registered in:** `lib.rs` as `pub mod scenarios;`  
> **Status:** Design finalized, implementation pending

---

## Table of Contents

1. [Overview](#overview)
2. [Critical Thinking Structure](#critical-thinking-structure)
3. [Module Structure](#module-structure)
4. [Core Data Model](#core-data-model)
5. [Simulation Engine](#simulation-engine)
6. [Data Agglomeration Pipeline](#data-agglomeration-pipeline)
7. [Branch Logic Authoring](#branch-logic-authoring)
8. [Evidence Attachment](#evidence-attachment)
9. [Micro/Macro Composable Hierarchy](#micromacro-composable-hierarchy)
10. [Soft Pruning](#soft-pruning)
11. [Persistence](#persistence)
12. [Bevy Integration](#bevy-integration)
13. [Visualization Phases](#visualization-phases)
14. [Real-Case Investigative Features](#real-case-investigative-features)
    - 14.1 [Temporal Decay & Evidence Freshness](#temporal-decay--evidence-freshness)
    - 14.2 [Contradictory Evidence Handling](#contradictory-evidence-handling)
    - 14.3 [Chain of Custody / Evidence Provenance](#chain-of-custody--evidence-provenance)
    - 14.4 [Cognitive Bias Detection](#cognitive-bias-detection)
    - 14.5 [Witness Reliability Scoring](#witness-reliability-scoring)
    - 14.6 [Geographic Profiling](#geographic-profiling)
    - 14.7 [Timeline Gap Analysis](#timeline-gap-analysis)
    - 14.8 [Multi-Analyst Collaboration](#multi-analyst-collaboration)
    - 14.9 [Victimology Profiling](#victimology-profiling)
    - 14.10 [Scenario Comparison / Diff Tool](#scenario-comparison--diff-tool)
15. [SOTA Intelligence Features](#sota-intelligence-features)
    - 15.1 [NLP Entity Extraction Pipeline](#nlp-entity-extraction-pipeline)
    - 15.2 [Link Analysis / Social Network Graph](#link-analysis--social-network-graph)
    - 15.3 [ML Predictive Modeling](#ml-predictive-modeling)
    - 15.4 [Digital Forensics Adapters](#digital-forensics-adapters)
    - 15.5 [Immutable Audit Trail](#immutable-audit-trail)
    - 15.6 [Devil's Advocate / Red Team Mode](#devils-advocate--red-team-mode)
    - 15.7 [Retail / Product Purchase Tracking](#retail--product-purchase-tracking)
16. [Dependencies](#dependencies)
17. [Implementation Checklist](#implementation-checklist)

---

## Overview

> **Think of Eustress Scenarios as something the FBI would use.**  
> Think of Eustress Circumstances as something Costco would use.  
> Same engine. Different questions. One platform.

Eustress Scenarios is a probabilistic scenario simulation engine embedded in the Eustress Engine. It provides a rigorous, iterative framework for modeling branching outcomes with Monte Carlo simulations, Bayesian updates, and 4D visualization (X/Y/Z spatial + T time/probability). It is designed for **investigative, backward-looking analysis** — law enforcement, intelligence agencies, forensic analysts, and anyone asking **"What happened?"**

The system supports both **micro** (tactical, single-event) and **macro** (strategic, trend-level) scenarios in a **composable hierarchy** — macros contain micros as sub-scenarios. Multiple scenarios can coexist in the same Space.

---

## Critical Thinking Structure

### Phase 1: Define the Case Core (Initialization)
- User sets **Eustress Parameters**: Location, Time, Entities, Initial Evidence
- Natural language query compiled to scenario structure
- Parameters embed into vector space for semantic linking

### Phase 2: Data Agglomeration (Ingestion)
- Pull from local files, REST APIs, and live feeds filtered by Parameters
- AI clusters data into "parameter bundles" with confidence scores
- Rune APIs chunk data into probabilistic nodes

### Phase 3: Scenario Building (Modeling)
- Construct base scenarios from agglomerated data as root BranchNodes
- Visualize in 4D field (spatial + time/probability branching)
- Apply Bayesian updates: priors → posteriors as evidence arrives

### Phase 4: Dynamic Resolution (Iteration)
- Micro-steps: Timeline Reconstruction → Entity Profiling → Motive Inference → Evidence Correlation
- New data triggers re-runs; parameters auto-adjust for real-time updates

### Phase 5: Branching What-Ifs (Exploration)
- Scenarios fork realistically from base nodes
- Each branch pulls agglomerated evidence
- Low-probability branches soft-collapse (never hard-pruned)
- 4D field visualization: holographic decision tree with time scrubbing

### Phase 6: Outcome Synthesis (Resolution)
- Aggregate branches into top probabilistic paths
- Dashboards with heatmaps, 3D decision trees, timeline ribbons
- Feedback loop: user rates outcomes, system refines parameters

---

## Module Structure

```
scenarios/
├── mod.rs              // ScenariosPlugin, re-exports
├── types.rs            // Core data structures
├── engine.rs           // Monte Carlo runner, Bayesian updater
├── agglomeration.rs    // Data ingestion pipeline, source adapters
│   ├── local.rs        // Local file adapters (JSON/CSV/RON)
│   ├── rest.rs         // REST API adapters
│   └── live.rs         // Live feed adapters via Eustress Parameters
├── hierarchy.rs        // Micro/Macro composable nesting
├── evidence.rs         // Evidence attachment (manual + automatic)
├── pruning.rs          // Soft pruning / visual collapse logic
├── persistence.rs      // Binary serialization (embedded in Eustress format)
├── visualization.rs    // Bevy systems for 4D field rendering
└── rune_api.rs         // Rune bindings for user-scriptable branch logic
```

---

## Core Data Model

```rust
// === Parameters ===

enum ParameterValue {
    Text(String),
    Number(f64),
    Position(Vec3),           // Spatial coordinate
    Timestamp(DateTime<Utc>), // Temporal coordinate
    EntityRef(Uuid),          // Reference to ScenarioEntity
    Boolean(bool),
    List(Vec<ParameterValue>),
}

struct ScenarioParameter {
    key: String,
    value: ParameterValue,
    confidence: f64,          // 0.0–1.0
    source: DataSourceRef,    // Where this parameter came from
}

// === Entities ===

enum EntityRole {
    Victim,
    Suspect,
    Witness,
    Location,
    Evidence,
    Vehicle,
    Object,
    Custom(String),
}

struct ScenarioEntity {
    id: Uuid,
    name: String,
    role: EntityRole,
    attributes: HashMap<String, ParameterValue>,
    linked_evidence: Vec<Uuid>,  // Evidence IDs attached to this entity
}

// === Evidence ===

enum EvidenceType {
    Physical,       // DNA, fingerprints, blood
    Digital,        // Surveillance, logs, metadata
    Testimonial,    // Witness statements
    Circumstantial, // Behavioral patterns, timelines
    Geospatial,     // Location data, tracks
    Custom(String),
}

struct Evidence {
    id: Uuid,
    label: String,
    evidence_type: EvidenceType,
    data: HashMap<String, ParameterValue>,
    confidence: f64,
    source: DataSourceRef,
    timestamp: Option<DateTime<Utc>>,
}

struct EvidenceLink {
    evidence_id: Uuid,
    attachment_mode: AttachmentMode, // Manual | Automatic
    relevance_score: f64,            // How strongly this evidence supports the branch
    likelihood_ratio: f64,           // P(evidence|hypothesis) / P(evidence|¬hypothesis)
}

enum AttachmentMode {
    Manual,                          // User explicitly linked
    Automatic { embedding_score: f64 }, // System inferred via embeddings
}

// === Scenarios ===

enum ScenarioScale {
    Micro,  // Single event/element, fine-grained
    Macro,  // Broad patterns, aggregated
}

struct Scenario {
    id: Uuid,
    name: String,
    description: String,
    scale: ScenarioScale,
    parameters: Vec<ScenarioParameter>,
    entities: Vec<ScenarioEntity>,
    evidence_pool: Vec<Evidence>,
    root_branch: BranchNode,
    sub_scenarios: Vec<Uuid>,        // Composable: macro contains micro IDs
    parent_scenario: Option<Uuid>,   // If this is a micro inside a macro
    created: DateTime<Utc>,
    modified: DateTime<Utc>,
    simulation_config: SimulationConfig,
}

// === Branches ===

struct BranchNode {
    id: Uuid,
    label: String,
    description: String,
    prior_probability: f64,
    posterior_probability: Option<f64>,  // After Bayesian update
    evidence_links: Vec<EvidenceLink>,
    children: Vec<BranchNode>,          // What-if forks
    outcome: Option<OutcomeData>,
    soft_collapsed: bool,               // Visual collapse state
    collapse_threshold: f64,            // User-configurable per branch
    branch_logic: BranchLogicSource,    // How this branch was defined
    metadata: HashMap<String, String>,
}

enum BranchLogicSource {
    Template,                           // Built-in scenario template
    VisualEditor,                       // Defined via Slint node editor
    RuneScript(String),                 // Custom Rune script path/content
    NaturalLanguage(String),            // AI-compiled from NL input
}

// === Outcomes ===

struct OutcomeData {
    description: String,
    confidence: f64,
    monte_carlo_samples: usize,
    distribution: HashMap<String, f64>, // outcome_label -> probability
    recommended_actions: Vec<String>,
}

// === Simulation Config ===

struct SimulationConfig {
    num_iterations: usize,              // Monte Carlo sample count (default: 1000)
    collapse_threshold: f64,            // Default soft-collapse threshold (e.g., 0.05)
    enable_auto_evidence: bool,         // Enable automatic evidence attachment
    parallel_threads: Option<usize>,    // Rayon thread count (None = auto)
}
```

---

## Simulation Engine

**Execution model:** Hybrid — rayon for Monte Carlo compute, tokio for I/O.

```
┌─────────────────────────────────────────────────┐
│                  Bevy Main Loop                  │
│                                                  │
│  ScenarioEvent ──► handle_scenario_events        │
│                         │                        │
│         ┌───────────────┼───────────────┐        │
│         ▼               ▼               ▼        │
│   ┌──────────┐   ┌───────────┐   ┌──────────┐   │
│   │  rayon   │   │   tokio   │   │  Bevy    │   │
│   │ threadpool│   │  runtime  │   │ systems  │   │
│   │          │   │           │   │          │   │
│   │ Monte    │   │ REST API  │   │ Viz      │   │
│   │ Carlo    │   │ fetches   │   │ updates  │   │
│   │ sims     │   │           │   │          │   │
│   │          │   │ Live feed │   │ UI sync  │   │
│   │ Bayesian │   │ streams   │   │          │   │
│   │ updates  │   │           │   │          │   │
│   └────┬─────┘   └─────┬─────┘   └──────────┘   │
│        │               │                        │
│        └───────┬───────┘                        │
│                ▼                                │
│        crossbeam channel                        │
│                ▼                                │
│        SimulationComplete event                 │
│                ▼                                │
│        update_visualizations system             │
└─────────────────────────────────────────────────┘
```

### Bayesian Update Flow
1. User attaches evidence to branch (manual) or system auto-attaches
2. Compute likelihood ratio: `P(evidence | branch_hypothesis) / P(evidence | ¬hypothesis)`
3. Update posterior: `P(H|E) = P(E|H) * P(H) / P(E)`
4. Propagate updates up/down the tree (parent/child probabilities re-normalize)
5. Trigger Monte Carlo re-run on affected subtree

---

## Data Agglomeration Pipeline

### Source Adapters

| Adapter | Transport | Description |
|---------|-----------|-------------|
| **Local Files** | Filesystem | JSON, CSV, RON scenario definitions. Bulk import of evidence sets, entity databases. |
| **REST APIs** | tokio + reqwest | Public databases (FBI, weather, geospatial). Configurable endpoints with auth. |
| **Live Feeds** | tokio async streams | Real-time data via Eustress Parameters. Subscribes to parameter change events, auto-triggers re-agglomeration. |

### Pipeline Flow
```
Sources → Ingest → Normalize → Cluster → Parameterize → Attach to Scenario
                                  │
                                  ▼
                        Confidence scoring
                        (per data point)
```

---

## Branch Logic Authoring

**Hybrid approach** with three tiers:

1. **Visual Node Editor** (primary) — Slint UI with drag-and-drop conditions, outcomes, and probability assignments. Accessible to all users.
2. **Rune Script Overrides** (advanced) — Users write custom branch logic in Rune for complex probabilistic models, custom distributions, or domain-specific algorithms.
3. **Natural Language → AI-compiled to Rune** (future) — User describes branch logic in plain English; AI compiles to Rune script for execution.

Template scenarios provide pre-built branch structures that users can customize via any tier.

---

## Evidence Attachment

Two modes, both always available:

- **Manual:** User explicitly links evidence to a branch. Sets relevance score and optionally a likelihood ratio. Full control.
- **Automatic:** System infers which branches evidence supports via semantic embeddings. Computes embedding similarity between evidence attributes and branch hypothesis descriptions. User can review/override automatic attachments.

---

## Micro/Macro Composable Hierarchy

```
MacroScenario (National kidnapping trends)
├── MicroScenario (Guthrie case: timeline reconstruction)
│   ├── Branch: Intruder entry via back door
│   └── Branch: Intruder entry via garage
├── MicroScenario (Guthrie case: suspect profiling)
│   ├── Branch: Known associate
│   └── Branch: Stranger
└── MicroScenario (Regional pattern analysis)
    ├── Branch: Serial pattern match
    └── Branch: Isolated incident
```

- **Micro feeds macro:** Gait match from micro agglomerates into suspect profiles for macro branching
- **Macro informs micro:** National trends prioritize certain micro evidence checks
- **Transitions are automatic:** When a micro scenario's outcome crosses a confidence threshold, it propagates to the parent macro

---

## Soft Pruning

**No hard pruning. Ever.** All branches persist in the scenario tree.

- Branches below a configurable threshold (default: 5%) are **visually collapsed** in all visualization modes
- Collapsed branches show as a single compressed node with aggregate probability
- User can expand any collapsed branch at any time
- Threshold is configurable globally (SimulationConfig) and per-branch (BranchNode.collapse_threshold)
- Collapsed branches still participate in Monte Carlo simulations — they are never excluded from computation

---

## Persistence

Scenarios are **embedded in the Eustress binary format** (the project save file):

- Serialized via `bincode` + `zstd` compression for space efficiency
- Support for **very large scenarios** (100k+ branches) via streaming serialization
- **Multiple scenarios per Space** — each scenario is an independent entity in the ECS, stored in a `ScenarioWorkspace` resource
- Scenario snapshots for versioning (save/restore points during iterative analysis)
- Export options: JSON (interop), RON (human-readable), CSV (outcome tables)

---

## Bevy Integration

```rust
pub struct ScenariosPlugin;

impl Plugin for ScenariosPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<ScenarioWorkspace>()      // All active scenarios
            .init_resource::<SimulationChannels>()      // rayon/tokio → Bevy bridges
            .init_resource::<AgglomerationConfig>()     // Source adapter configs
            
            // Events
            .add_event::<ScenarioEvent>()               // Create, Fork, Update, Delete
            .add_event::<SimulationRequest>()            // Trigger Monte Carlo run
            .add_event::<SimulationComplete>()           // Results ready
            .add_event::<EvidenceEvent>()                // Attach, Detach, Auto-link
            .add_event::<AgglomerationEvent>()           // Data source updates
            
            // Systems
            .add_systems(Update, (
                handle_scenario_events,
                handle_evidence_events,
                dispatch_simulations,                    // Send to rayon
                poll_simulation_results,                 // Receive from channel
                poll_agglomeration_results,              // Receive from tokio
                propagate_bayesian_updates,
                update_soft_collapse_states,
                sync_scenario_to_ui,                     // Push to Slint
                update_3d_visualizations,                // Bevy gizmos/meshes
            ).chain());
    }
}
```

---

## Visualization Phases

### Phase 1: Decision Tree Graph (3D Node-Link)
- Scenarios as spheres in 3D space
- Node size = simulation sample count
- Node color = probability (green → yellow → red gradient)
- Edge thickness = branch weight
- Interactive: click node to inspect, hover for tooltip
- Camera auto-frames the tree; pan/zoom/orbit
- Soft-collapsed branches render as small grey dots with expand affordance

### Phase 2: Geospatial Heatmap Overlay
- WGS84 coordinate mapping to Bevy world space
- Probability density as colored mesh overlay on terrain
- Entity markers (suspects, evidence, locations) as 3D pins
- Temporal scrubbing: slide time to see heatmap evolve
- Layer toggles: show/hide evidence types, entity roles
- Integration with `terrain_plugin` for ground-truth surface

### Phase 3: Timeline Ribbon (4D)
- Horizontal time axis with vertical probability ribbons per branch
- Ribbons split at fork points, width = probability mass
- Color-coded by scenario type
- Scrub handle: drag through time to see probabilities shift
- Bayesian update markers on timeline (evidence arrival points)
- Collapse/expand sub-branches interactively

### Phase 4: Slint Text Dashboard
- Scenario tree view (collapsible, like Explorer panel)
- Per-node: name, prior/posterior probability, evidence count, sample count
- Outcome summary table: top N paths ranked by confidence
- Parameter editor: modify inputs and trigger re-simulation
- Simulation progress bar (for long-running Monte Carlo jobs)
- Export: JSON/RON scenario snapshots, CSV outcome tables

---

## Real-Case Investigative Features

These 10 features are derived from patterns observed in real criminal investigations, intelligence analysis, and disaster response. Each addresses a specific failure mode or capability gap that has impacted real cases.

### Temporal Decay & Evidence Freshness

**Problem:** In real cases (e.g., missing persons, cold cases), evidence degrades over time. A witness statement from hour 1 is more reliable than one from week 3. Static confidence scores don't capture this.

**Solution:** A `FreshnessDecay` function on Evidence — confidence auto-decreases based on `(now - timestamp)` with configurable decay curves.

```rust
enum DecayCurve {
    Linear { half_life: Duration },        // Steady decline
    Exponential { lambda: f64 },           // Rapid initial drop, long tail
    Step { thresholds: Vec<(Duration, f64)> }, // Discrete drops at time boundaries
    None,                                  // No decay (physical evidence, DNA)
}

struct TemporalDecay {
    curve: DecayCurve,
    base_confidence: f64,                  // Original confidence at collection time
    collected_at: DateTime<Utc>,
}

// Evidence.effective_confidence(now) = base_confidence * decay_factor(now - collected_at)
```

**Real-case reference:** Delphi murders — witness descriptions became less reliable over months. Madeleine McCann — early witness sightings weighted higher than later ones.

### Contradictory Evidence Handling

**Problem:** Real cases almost always have conflicting data. Physical evidence may point in multiple directions simultaneously. The base model doesn't explicitly handle contradictions.

**Solution:** An `EvidenceConflict` struct that links contradicting evidence items with resolution strategies.

```rust
struct EvidenceConflict {
    id: Uuid,
    evidence_a: Uuid,
    evidence_b: Uuid,
    conflict_type: ConflictType,
    resolution: ConflictResolution,
    notes: String,
}

enum ConflictType {
    DirectContradiction,    // A says X, B says not-X
    TemporalImpossibility,  // A and B can't both be true given timeline
    PhysicalIncompatibility, // Forensic evidence conflicts
    TestimonialDisagreement, // Witnesses disagree
}

enum ConflictResolution {
    WeightedAverage,        // Split probability mass proportionally
    PreferA { reason: String },
    PreferB { reason: String },
    HoldBoth,               // Maintain both as valid, split branches
    Unresolved,             // Flag for analyst review
}
```

**Real-case reference:** JonBenét Ramsey — physical evidence pointed in multiple directions. Steven Avery / Making a Murderer — contested forensic evidence.

### Chain of Custody / Evidence Provenance

**Problem:** In legal contexts, evidence without provenance is inadmissible. Tainted evidence chains can corrupt entire scenario trees.

**Solution:** A `ProvenanceChain` on each Evidence — who collected it, when, how it was stored, chain of transfers. Generates an admissibility score separate from confidence.

```rust
struct ProvenanceChain {
    entries: Vec<ProvenanceEntry>,
    admissibility_score: f64,  // 0.0–1.0, computed from chain integrity
}

struct ProvenanceEntry {
    timestamp: DateTime<Utc>,
    custodian: String,          // Person/agency responsible
    action: CustodyAction,
    location: Option<String>,
    notes: String,
    verified: bool,             // Independently confirmed
}

enum CustodyAction {
    Collected,
    Transferred,
    Stored,
    Analyzed,
    Duplicated,
    Sealed,
    Unsealed,
    Contaminated,  // Flags a break in chain
    Destroyed,
}
```

Admissibility auto-degrades if:
- Gaps exist in the chain (missing entries between actions)
- `Contaminated` action appears
- `verified: false` on critical entries
- Chain crosses jurisdictions without transfer documentation

**Real-case reference:** Whitey Bulger — FBI evidence handling corruption. O.J. Simpson — chain of custody challenges on blood evidence.

### Cognitive Bias Detection

**Problem:** Tunnel vision is the #1 cause of wrongful convictions (Innocence Project data). Investigators fixate on one scenario and unconsciously discount contradicting evidence.

**Solution:** A **bias detector** system that runs as a Bevy system, analyzing the current scenario state and emitting `BiasWarning` events.

```rust
struct BiasWarning {
    bias_type: BiasType,
    severity: f64,           // 0.0–1.0
    description: String,
    affected_branches: Vec<Uuid>,
    suggested_action: String,
}

enum BiasType {
    ConfirmationBias,   // >80% of evidence manually attached to leading branch
    NeglectBias,        // Contradicting evidence exists but isn't attached to any branch
    TunnelVision,       // User hasn't explored branches below threshold in N sessions
    AnchoringBias,      // First-entered scenario has disproportionate evidence
    AvailabilityBias,   // Recent evidence weighted higher than warranted
}
```

Detection rules:
- **Confirmation bias:** Leading branch has >80% of all manually-attached evidence
- **Neglect bias:** Evidence items exist in the pool that contradict the leading hypothesis but aren't linked to any branch
- **Tunnel vision:** User hasn't expanded or interacted with branches below 20% probability in the last N sessions
- **Anchoring:** The first-created scenario branch has >2x the evidence of the second
- **Availability:** Evidence attached in the last 24h has >3x the average relevance score

**Real-case reference:** Cameron Todd Willingham — arson investigators fixated on "pour patterns" that were later debunked. Central Park Five — tunnel vision on initial suspects despite contradicting DNA.

### Witness Reliability Scoring

**Problem:** Not all testimonial evidence is equal. Eyewitness descriptions can be wildly inaccurate. Real investigators use structured reliability assessments.

**Solution:** A `WitnessReliability` component on testimonial evidence that auto-adjusts the evidence's likelihood ratio.

```rust
struct WitnessReliability {
    // Environmental factors (0.0–1.0 each)
    proximity: f64,            // How close was the witness
    lighting_conditions: f64,  // Visibility at the time
    duration_of_observation: f64, // How long they observed
    
    // Cognitive factors
    stress_level: f64,         // High stress = lower reliability (weapon focus effect)
    time_elapsed: Duration,    // Since observation to statement
    prior_relationship: bool,  // Knew the subject = higher reliability for ID
    
    // Consistency
    statement_count: usize,    // Number of interviews
    consistency_score: f64,    // Cross-interview consistency (0.0–1.0)
    detail_specificity: f64,   // Vague vs. specific descriptions
    
    // Computed
    composite_reliability: f64, // Weighted aggregate
}

impl WitnessReliability {
    fn compute_composite(&self) -> f64 {
        // Weighted formula based on empirical research
        // (Loftus, Wells & Olson, etc.)
        let env = (self.proximity * 0.3 + self.lighting_conditions * 0.3 
                   + self.duration_of_observation * 0.4);
        let cog = (1.0 - self.stress_level * 0.4) 
                  * (1.0 - (self.time_elapsed.as_secs_f64() / 86400.0).min(1.0) * 0.3)
                  * if self.prior_relationship { 1.3 } else { 1.0 };
        let con = self.consistency_score * 0.6 + self.detail_specificity * 0.4;
        (env * 0.3 + cog.min(1.0) * 0.4 + con * 0.3).clamp(0.0, 1.0)
    }
}
```

Auto-adjusts the parent Evidence's `likelihood_ratio` by multiplying with `composite_reliability`.

**Real-case reference:** Elizabeth Smart — initial eyewitness descriptions were inaccurate. Ronald Cotton case — witness misidentification despite high confidence.

### Geographic Profiling

**Problem:** Serial cases are often cracked through geographic profiling — offenders operate in a "comfort zone." Passive heatmaps don't actively compute anchor points.

**Solution:** An **active geographic profiler** that computes a probability surface for the offender's anchor point (home/work) given a set of linked crime/evidence locations.

```rust
struct GeographicProfile {
    crime_sites: Vec<GeoPoint>,
    anchor_probability_surface: Vec<(GeoPoint, f64)>, // Grid of probabilities
    peak_anchor: Option<GeoPoint>,                     // Most likely anchor point
    buffer_zone_radius: f64,                           // Estimated comfort zone (meters)
    algorithm: GeoProfileAlgorithm,
}

enum GeoProfileAlgorithm {
    Rossmo {                    // Rossmo's CGT formula
        f: f64,                 // Empirical exponent (typically 1.2)
        g: f64,                 // Empirical exponent (typically 1.2)
        buffer: f64,            // Buffer zone radius
    },
    BayesianDistanceDecay {     // Bayesian approach with distance decay
        prior: DistanceDecayPrior,
        kernel_bandwidth: f64,
    },
    CenterOfMinimumDistance,    // Simple centroid-based
}

struct GeoPoint {
    lat: f64,   // WGS84
    lon: f64,
    alt: Option<f64>,
}
```

Integrates with Viz Phase 2 (Geospatial Heatmap) — the anchor probability surface renders as a colored overlay on the terrain.

**Real-case reference:** BTK killer — geographic profiling narrowed search area. Golden State Killer — geographic patterns across decades of crimes.

### Timeline Gap Analysis

**Problem:** Unaccounted time windows are critical in investigations. The current model has timestamps but doesn't actively identify gaps.

**Solution:** A **timeline gap detector** that analyzes all entity timelines and flags unaccounted periods.

```rust
struct TimelineGap {
    entity_id: Uuid,
    gap_start: DateTime<Utc>,
    gap_end: DateTime<Utc>,
    duration: Duration,
    severity: GapSeverity,
    last_known_location: Option<GeoPoint>,
    next_known_location: Option<GeoPoint>,
    possible_activities: Vec<GapHypothesis>,
}

enum GapSeverity {
    Critical,   // Gap during key event window
    Significant, // >1 hour gap with no coverage
    Minor,      // <1 hour or outside event window
}

struct GapHypothesis {
    description: String,
    probability: f64,
    supporting_evidence: Vec<Uuid>,
}
```

The system:
1. Collects all timestamped evidence per entity
2. Sorts chronologically
3. Identifies gaps exceeding a configurable threshold
4. Cross-references gaps with the scenario's event window
5. Generates hypotheses for what could have occurred during gaps (based on branch logic)

**Real-case reference:** Madeleine McCann — the "missing 45 minutes" was central to the investigation. Maura Murray — timeline gaps in the hours before disappearance.

### Multi-Analyst Collaboration

**Problem:** Major cases involve multiple agencies with different data, hypotheses, and jurisdictional friction. No single analyst has the full picture.

**Solution:** **Analyst roles** — each analyst maintains independent branch weightings and evidence attachments. A merge view shows agreement/disagreement with consensus probability.

```rust
struct Analyst {
    id: Uuid,
    name: String,
    agency: String,
    role: AnalystRole,
    created: DateTime<Utc>,
}

enum AnalystRole {
    Lead,           // Full read/write, can merge
    Contributor,    // Can add evidence, adjust own weightings
    Reviewer,       // Read-only with annotation capability
    Observer,       // Read-only
}

struct AnalystView {
    analyst_id: Uuid,
    scenario_id: Uuid,
    branch_overrides: HashMap<Uuid, f64>,    // Branch ID → analyst's probability override
    evidence_annotations: HashMap<Uuid, String>, // Evidence ID → analyst's notes
    private_branches: Vec<BranchNode>,       // Branches only this analyst sees
}

struct ConsensusView {
    branch_consensus: HashMap<Uuid, ConsensusEntry>,
}

struct ConsensusEntry {
    branch_id: Uuid,
    mean_probability: f64,
    std_deviation: f64,
    analyst_count: usize,
    agreement_level: AgreementLevel,  // High (σ<0.1), Medium, Low (σ>0.3), Conflict
}
```

Features:
- Each analyst can fork their own view without affecting others
- **Merge view** highlights where analysts agree (green) and disagree (red)
- **Consensus probability** = weighted average across analysts (Lead weighted higher)
- **Conflict alerts** when analysts diverge by >30% on the same branch
- Git-like history: who changed what, when

**Real-case reference:** Zodiac killer — multiple agencies with fragmented data. DC Sniper — multi-jurisdictional coordination challenges.

### Victimology Profiling

**Problem:** FBI behavioral analysis always starts with the victim. Generic entity attributes don't capture structured victimology.

**Solution:** A `VictimologyProfile` struct with structured fields that auto-generates "opportunity windows" for the scenario timeline.

```rust
struct VictimologyProfile {
    entity_id: Uuid,
    
    // Risk assessment
    risk_level: VictimRiskLevel,
    risk_factors: Vec<String>,
    
    // Routine activities
    daily_routine: Vec<RoutineActivity>,
    known_locations: Vec<(GeoPoint, String)>,  // Location + description
    transportation: Vec<String>,
    
    // Social network
    relationships: Vec<Relationship>,
    social_media_presence: SocialMediaLevel,
    community_visibility: f64,  // 0.0 (reclusive) – 1.0 (public figure)
    
    // Digital footprint
    last_digital_activity: Option<DateTime<Utc>>,
    device_locations: Vec<(DateTime<Utc>, GeoPoint)>,
    
    // Computed
    opportunity_windows: Vec<OpportunityWindow>,
}

enum VictimRiskLevel {
    Low,     // Stable routine, low exposure
    Medium,  // Some risk factors
    High,    // High exposure, risky behavior patterns
}

struct RoutineActivity {
    description: String,
    time_range: (NaiveTime, NaiveTime),
    days: Vec<Weekday>,
    location: Option<GeoPoint>,
}

struct Relationship {
    entity_id: Uuid,
    relationship_type: String,  // "spouse", "coworker", "neighbor", etc.
    closeness: f64,             // 0.0–1.0
    conflict_history: bool,
    last_contact: Option<DateTime<Utc>>,
}

struct OpportunityWindow {
    time_range: (DateTime<Utc>, DateTime<Utc>),
    location: GeoPoint,
    vulnerability_score: f64,   // How exposed the victim was
    routine_deviation: bool,    // Was this outside normal routine?
}
```

Auto-generates opportunity windows by:
1. Mapping the victim's routine onto the scenario timeline
2. Identifying deviations from routine (higher risk)
3. Cross-referencing with suspect entity timelines for overlap
4. Feeding windows into branch probability calculations

**Real-case reference:** FBI's Behavioral Analysis Unit methodology. Ted Bundy cases — victimology patterns revealed targeting criteria. Israel Keyes — victim selection based on opportunity windows.

### Scenario Comparison / Diff Tool

**Problem:** Investigators often maintain parallel theories. The current model supports multiple scenarios per Space but lacks structured comparison.

**Solution:** A **scenario diff** tool — side-by-side comparison showing divergent branches, shared evidence, and probability deltas.

```rust
struct ScenarioDiff {
    scenario_a: Uuid,
    scenario_b: Uuid,
    shared_evidence: Vec<Uuid>,
    unique_to_a: Vec<Uuid>,
    unique_to_b: Vec<Uuid>,
    branch_comparisons: Vec<BranchComparison>,
    overall_divergence: f64,  // 0.0 (identical) – 1.0 (completely different)
}

struct BranchComparison {
    label: String,
    prob_a: Option<f64>,       // None if branch doesn't exist in A
    prob_b: Option<f64>,
    delta: f64,                // |prob_a - prob_b|
    shared_evidence_count: usize,
    divergence_reason: String,
}
```

Features:
- **Side-by-side view** in the Slint dashboard
- **Shared evidence highlighting** — evidence used by both scenarios shown in blue
- **Exclusive evidence** — evidence unique to each scenario shown in orange/purple
- **Probability delta bars** — visual bars showing where scenarios agree/disagree
- **Merge tool** — combine the best branches from two scenarios into a new one
- **A/B simulation** — run Monte Carlo on both simultaneously, compare outcome distributions

**Real-case reference:** Zodiac killer — one killer vs. copycat theories. Jack the Ripper — dozens of competing suspect theories maintained in parallel.

---

## SOTA Intelligence Features

These 6 features close the gap between Eustress Scenarios and the state of the art in intelligence analysis platforms (Palantir Gotham, i2 Analyst's Notebook, Babel Street). Together with the Real-Case Investigative Features, they make the system genuinely revolutionary — no existing tool combines all of these in a unified real-time 3D environment.

### NLP Entity Extraction Pipeline

**Problem:** Real investigations generate massive volumes of unstructured text — police reports, witness statements, news articles, social media posts, court documents. Manual data entry is the bottleneck.

**Solution:** An NLP pipeline that ingests raw text and auto-extracts structured data into the scenario model.

```rust
struct ExtractionResult {
    source_text: String,
    source_metadata: DataSourceRef,
    extracted_entities: Vec<ExtractedEntity>,
    extracted_relationships: Vec<ExtractedRelationship>,
    extracted_events: Vec<ExtractedEvent>,
    confidence: f64,
}

struct ExtractedEntity {
    name: String,
    entity_type: EntityRole,        // Person, Location, Vehicle, etc.
    attributes: HashMap<String, String>,
    text_span: (usize, usize),      // Character offsets in source
    confidence: f64,
}

struct ExtractedRelationship {
    entity_a: String,
    entity_b: String,
    relationship_type: String,       // "knows", "employed_by", "located_at", etc.
    confidence: f64,
}

struct ExtractedEvent {
    description: String,
    timestamp: Option<DateTime<Utc>>,
    location: Option<GeoPoint>,
    participants: Vec<String>,       // Entity names
    event_type: String,              // "sighting", "transaction", "communication", etc.
    confidence: f64,
}
```

Pipeline stages:
1. **Ingest** — Accept raw text from files, clipboard, or REST API responses
2. **NER (Named Entity Recognition)** — Extract persons, locations, organizations, dates, amounts
3. **Relation Extraction** — Identify relationships between entities
4. **Temporal Parsing** — Normalize date/time expressions ("last Tuesday", "around 10 PM")
5. **Geocoding** — Resolve location names to WGS84 coordinates
6. **Deduplication** — Match extracted entities against existing scenario entities
7. **Review** — Present extractions to analyst for confirmation before committing

**Implementation note:** Can use local models (e.g., ONNX-exported NER models via `ort` crate) or API-based (OpenAI, local LLM via Ollama). Configurable per deployment.

### Link Analysis / Social Network Graph

**Problem:** Relationships between entities are often the key to solving cases. Who knows who, who communicated with whom, who was where when. Flat entity lists miss these patterns.

**Solution:** A graph analysis engine operating on the scenario's entity relationship data.

```rust
struct EntityGraph {
    nodes: HashMap<Uuid, GraphNode>,
    edges: Vec<GraphEdge>,
}

struct GraphNode {
    entity_id: Uuid,
    centrality_score: f64,          // How connected this entity is
    community_id: Option<usize>,    // Cluster membership
    betweenness: f64,               // Bridge between communities
}

struct GraphEdge {
    source: Uuid,
    target: Uuid,
    edge_type: EdgeType,
    weight: f64,                    // Strength of connection
    evidence_ids: Vec<Uuid>,        // Evidence supporting this link
    timestamps: Vec<DateTime<Utc>>, // When interactions occurred
}

enum EdgeType {
    Social,         // Personal relationship
    Communication,  // Phone, email, message
    Financial,      // Money transfer, shared accounts
    Proximity,      // Co-located at same time
    Organizational, // Same employer, group membership
    Familial,       // Family relationship
    Digital,        // Shared IP, device, account
    Custom(String),
}
```

Analysis capabilities:
- **Degree centrality** — Who has the most connections (potential organizer)
- **Betweenness centrality** — Who bridges separate groups (potential intermediary)
- **Community detection** — Identify clusters/cells within the network
- **Shortest path** — How are two entities connected (degrees of separation)
- **Temporal patterns** — Communication frequency changes over time (spike before event?)
- **Financial flow** — Follow the money through the network

Visualization: Renders as a force-directed 3D graph in the Bevy viewport, integrated with the decision tree and geospatial views.

### ML Predictive Modeling

**Problem:** Monte Carlo simulations are powerful but purely generative — they don't learn from historical data. Real SOTA systems use resolved cases to improve predictions.

**Solution:** An optional ML layer that trains on historical case outcomes to provide informed priors and pattern-based predictions.

```rust
struct PredictiveModel {
    model_id: Uuid,
    name: String,
    training_cases: usize,
    feature_set: Vec<String>,       // Which evidence/entity features the model uses
    accuracy: f64,                  // Cross-validated accuracy
    last_trained: DateTime<Utc>,
}

struct Prediction {
    model_id: Uuid,
    scenario_id: Uuid,
    predicted_outcome: String,
    confidence: f64,
    feature_importance: HashMap<String, f64>,  // Which features drove the prediction
    similar_cases: Vec<SimilarCase>,
}

struct SimilarCase {
    case_id: String,
    similarity_score: f64,
    outcome: String,
    key_differences: Vec<String>,
}
```

Capabilities:
- **Prior improvement** — "Cases with these evidence patterns resolved as kidnapping-for-ransom 73% of the time"
- **Pattern matching** — Find historically similar cases and their outcomes
- **Feature importance** — Which evidence features are most predictive for this scenario type
- **Anomaly detection** — Flag when current case deviates significantly from historical patterns

**Implementation note:** Use `ort` crate for ONNX model inference (runs locally, no API dependency). Training can happen offline on anonymized case databases. Models are optional — system works fully without them.

### Digital Forensics Adapters

**Problem:** Modern investigations are heavily digital — phone records, GPS logs, financial transactions, social media metadata. These come in specialized formats that need parsing.

**Solution:** Source adapters for common digital forensics export formats.

```rust
enum ForensicsFormat {
    // Mobile forensics
    CellebriteUFED,     // .ufed / .xml exports
    GrayKeyReport,      // PDF/CSV exports
    
    // Communications
    CallDetailRecords,  // CDR CSV (tower, duration, parties)
    SMSExport,          // Message logs
    EmailHeaders,       // MIME header analysis
    
    // Financial
    BankStatement,      // CSV/OFX transaction logs
    CryptoLedger,       // Blockchain transaction exports
    
    // Location
    GoogleTimeline,     // Google Takeout location history JSON
    AppleLocationData,  // Apple device location exports
    CellTowerDump,      // Tower dump CSV (all devices at tower)
    
    // Social media
    FacebookExport,     // Facebook data download
    TwitterArchive,     // Twitter/X data export
    InstagramExport,    // Instagram data download
    
    // Generic
    CustomCSV { mapping: ColumnMapping },
}

struct ColumnMapping {
    timestamp_col: Option<String>,
    entity_col: Option<String>,
    location_lat_col: Option<String>,
    location_lon_col: Option<String>,
    value_col: Option<String>,
    description_col: Option<String>,
}
```

Each adapter:
1. Parses the format-specific structure
2. Normalizes to `Evidence` and `ScenarioEntity` objects
3. Auto-generates timeline entries
4. Geocodes locations to WGS84
5. Links to existing entities via name/phone/email matching

### Immutable Audit Trail

**Problem:** For courtroom use, every analytical step must be reproducible and explainable. "How did you arrive at this conclusion?" needs a documented answer.

**Solution:** An append-only audit log that records every action taken in the scenario workspace.

```rust
struct AuditLog {
    scenario_id: Uuid,
    entries: Vec<AuditEntry>,       // Append-only, never modified
    hash_chain: Vec<[u8; 32]>,      // Blake3 hash chain for tamper detection
}

struct AuditEntry {
    id: Uuid,
    timestamp: DateTime<Utc>,
    analyst_id: Uuid,
    action: AuditAction,
    previous_state: Option<String>, // JSON snapshot of affected data before change
    new_state: Option<String>,      // JSON snapshot after change
    rationale: Option<String>,      // Analyst's stated reason (optional but encouraged)
    hash: [u8; 32],                 // Blake3 hash of this entry + previous hash
}

enum AuditAction {
    // Scenario lifecycle
    ScenarioCreated,
    ScenarioModified { field: String },
    
    // Evidence
    EvidenceAdded { evidence_id: Uuid },
    EvidenceAttached { evidence_id: Uuid, branch_id: Uuid, mode: AttachmentMode },
    EvidenceDetached { evidence_id: Uuid, branch_id: Uuid },
    EvidenceConfidenceChanged { evidence_id: Uuid, old: f64, new: f64 },
    
    // Branches
    BranchCreated { branch_id: Uuid, parent_id: Option<Uuid> },
    BranchProbabilityOverridden { branch_id: Uuid, old: f64, new: f64 },
    BranchCollapsed { branch_id: Uuid },
    BranchExpanded { branch_id: Uuid },
    
    // Simulation
    SimulationRun { config: SimulationConfig, duration_ms: u64 },
    BayesianUpdateApplied { branch_id: Uuid, evidence_id: Uuid },
    
    // Analyst
    AnalystJoined { analyst_id: Uuid },
    AnalystAnnotation { target_id: Uuid, note: String },
    
    // Bias
    BiasWarningGenerated { warning: BiasWarning },
    BiasWarningAcknowledged { warning_id: Uuid, response: String },
    
    // Devil's Advocate
    CounterArgumentGenerated { branch_id: Uuid },
    CounterArgumentAddressed { branch_id: Uuid, response: String },
}
```

Properties:
- **Append-only** — entries are never modified or deleted
- **Hash-chained** — each entry includes a Blake3 hash of itself + the previous entry's hash (tamper detection)
- **Exportable** — full audit trail exports to JSON/PDF for court submission
- **Queryable** — "Show me all actions by Analyst X on Branch Y between dates A and B"
- **Rationale capture** — system prompts analyst to explain significant changes (probability overrides, evidence detachments)

Uses `blake3` crate (already in Cargo.toml) for hash chain integrity.

### Devil's Advocate / Red Team Mode

**Problem:** CIA tradecraft and intelligence analysis best practices (per Richards Heuer's "Psychology of Intelligence Analysis") require structured techniques to counter cognitive biases. Simply detecting bias isn't enough — the system must actively challenge the analyst.

**Solution:** A "Devil's Advocate" mode that auto-generates counter-arguments to the leading hypothesis and forces the analyst to engage with them.

```rust
struct DevilsAdvocateSession {
    id: Uuid,
    scenario_id: Uuid,
    target_branch: Uuid,            // The leading hypothesis being challenged
    counter_arguments: Vec<CounterArgument>,
    status: DASessionStatus,
    started: DateTime<Utc>,
    completed: Option<DateTime<Utc>>,
}

struct CounterArgument {
    id: Uuid,
    argument: String,
    argument_type: CounterArgumentType,
    supporting_evidence: Vec<Uuid>,  // Evidence that supports the counter-argument
    neglected_evidence: Vec<Uuid>,   // Evidence not considered by leading hypothesis
    alternative_branch: Option<Uuid>, // Which alternative branch this supports
    analyst_response: Option<String>, // How the analyst addressed this
    addressed: bool,
}

enum CounterArgumentType {
    AlternativeExplanation,   // "This evidence could also mean..."
    NeglectedEvidence,        // "You haven't considered this evidence..."
    AssumptionChallenge,      // "Your scenario assumes X, but what if..."
    HistoricalPrecedent,      // "In similar cases, the outcome was different..."
    LogicalFlaw,              // "The reasoning from A to B has a gap..."
    MissingData,              // "There's no evidence for this critical link..."
}

enum DASessionStatus {
    Active,                   // Counter-arguments being generated/presented
    PendingResponse,          // Waiting for analyst to address arguments
    Completed,                // All arguments addressed
    Dismissed { reason: String }, // Analyst dismissed session with reason (logged)
}
```

Activation modes:
- **Manual** — Analyst triggers Devil's Advocate on any branch
- **Automatic** — System triggers when a branch exceeds 70% probability without the analyst having explored alternatives
- **Pre-finalization** — Required before marking any scenario as "concluded" (configurable policy)

Generation logic:
1. Identify the leading branch (highest posterior probability)
2. Find all evidence NOT attached to this branch
3. Find all alternative branches with evidence that contradicts the leading hypothesis
4. Generate counter-arguments from: neglected evidence, alternative explanations, assumption challenges
5. Present to analyst as a structured checklist — each must be addressed (responded to or explicitly dismissed with rationale)
6. All responses logged to the immutable audit trail

### Retail / Product Purchase Tracking

**Problem:** Investigators frequently need to trace a specific product (by SKU, model number, or serial number) back to the person who purchased it. This is a **multi-source join problem** — no single data source links product to person. The FBI, ATF, or local PD goes to a retailer and asks for cooperation; the store provides fragmented data that must be correlated across systems.

**Solution:** A dedicated Item entity type, Transaction evidence type, and retail-specific source adapters that perform the multi-source join automatically.

```rust
// === Item Tracking ===

struct ItemEntity {
    id: Uuid,
    sku: Option<String>,              // Store SKU (e.g., "WMT-4521-RADIO-T800")
    upc: Option<String>,              // Universal Product Code / barcode
    serial_number: Option<String>,    // Manufacturer serial (unique per unit)
    model_number: Option<String>,     // Manufacturer model (e.g., "T800")
    product_name: String,             // Human-readable name
    manufacturer: Option<String>,
    category: Option<String>,         // "Electronics", "Firearms", "Chemicals", etc.
    purchase_history: Vec<Uuid>,      // Transaction evidence IDs
}

// === Transaction Evidence ===

struct TransactionEvidence {
    id: Uuid,
    transaction_type: TransactionType,
    timestamp: DateTime<Utc>,
    store: StoreInfo,
    items: Vec<TransactionItem>,
    payment: PaymentInfo,
    customer: CustomerIdentification,
    register_id: Option<String>,
    receipt_number: Option<String>,
    surveillance_window: Option<(DateTime<Utc>, DateTime<Utc>)>, // Footage time range
}

enum TransactionType {
    InStorePurchase,
    OnlineOrder,
    OnlinePickup,       // Buy online, pick up in store
    Return,
    Exchange,
    WarrantyRegistration,
    LayawayPayment,
}

struct StoreInfo {
    name: String,               // "Walmart #4521"
    address: String,
    location: Option<GeoPoint>, // WGS84
    chain: Option<String>,      // "Walmart", "Home Depot", etc.
}

struct TransactionItem {
    sku: Option<String>,
    serial_number: Option<String>,
    product_name: String,
    quantity: u32,
    unit_price: f64,
}

struct PaymentInfo {
    method: PaymentMethod,
    amount: f64,
    card_last_four: Option<String>,
    card_type: Option<String>,      // "Visa", "Mastercard", etc.
    gift_card_number: Option<String>,
}

enum PaymentMethod {
    CreditCard,
    DebitCard,
    Cash,                           // Weakest link to identity
    GiftCard,                       // Weak — unless purchased with card
    MobilePayment,                  // Apple Pay, Google Pay — linked to device
    Check,                          // Name on check
    StoreCredit,                    // Linked to loyalty account
    Financing,                      // Store credit application — full identity
    Online { account_email: String }, // Direct identity
}

enum CustomerIdentification {
    /// Direct identification — high confidence
    Direct {
        name: String,
        method: DirectIdMethod,
    },
    /// Indirect — requires correlation with other sources
    Indirect {
        clues: Vec<IndirectClue>,
    },
    /// Unknown — cash, no loyalty, no ID
    Unknown,
}

enum DirectIdMethod {
    LoyaltyCard { member_id: String },
    CreditCard { last_four: String, bank_subpoena_needed: bool },
    OnlineAccount { email: String },
    WarrantyRegistration { name: String, address: String },
    Financing { application_id: String },
    CheckPayment { name_on_check: String },
    ReturnWithId { id_type: String },  // Driver's license for returns
}

enum IndirectClue {
    SurveillanceFootage { camera_id: String, timestamp: DateTime<Utc> },
    CellTowerPresence { tower_id: String, timestamp: DateTime<Utc>, device_id: Option<String> },
    StoreWifiLog { mac_address: String, timestamp: DateTime<Utc> },
    ParkingLotCamera { plate_number: Option<String>, vehicle_description: Option<String> },
    CompanionIdentified { entity_id: Uuid }, // Person they were with was ID'd
    ReceiptFound { location_found: String }, // Physical receipt recovered elsewhere
}
```

#### Retail Data Sources & Adapters

| Source | Format | Identity Strength | Adapter |
|--------|--------|-------------------|---------|
| **POS Transaction Logs** | CSV/XML export from store's system | Weak (timestamp + payment only) | `RetailPosAdapter` |
| **Loyalty/Rewards DB** | CSV/JSON export or API query | **Strong** (name, email, phone, full history) | `LoyaltyDbAdapter` |
| **Credit Card Records** | Bank CSV (via subpoena) | **Strong** (card holder) | `FinancialAdapter` (404) |
| **Online Order Records** | E-commerce DB export | **Strong** (full identity + shipping address) | `EcommerceAdapter` |
| **Warranty Registrations** | Manufacturer DB query | **Strong** (name, address, serial) | `WarrantyAdapter` |
| **Surveillance Footage** | Timestamps + camera IDs | Visual only (needs facial match or witness) | Manual + Timeline (307) |
| **Cell Tower Dumps** | Carrier CSV (via subpoena) | **Medium** (device owner at location) | `CellTowerAdapter` (404) |
| **Store WiFi Logs** | Router logs | **Weak** (MAC → device, not person) | `WifiLogAdapter` |
| **Parking Lot Cameras** | ALPR (Automatic License Plate Reader) | **Medium** (plate → registered owner) | `AlprAdapter` |
| **Return Records** | Store DB (often requires ID scan) | **Strong** (driver's license captured) | `RetailPosAdapter` |

#### Multi-Source Join Logic

The system performs a **probabilistic join** across data sources to link Item → Transaction → Person:

```
Step 1: Item Lookup
  Input: SKU "WMT-RADIO-T800" OR Serial "SN-789012"
  → Query POS logs for all transactions containing this item
  → Result: N transactions with timestamps, register IDs, payment methods

Step 2: Payment Resolution (per transaction)
  IF card payment → card_last_four → bank subpoena → card holder identity (95% confidence)
  IF loyalty card used → loyalty DB → member identity (99% confidence)
  IF online order → order DB → account holder (99% confidence)
  IF cash + no loyalty → fall through to Step 3

Step 3: Indirect Correlation (for cash/anonymous purchases)
  Timestamp + Register → surveillance footage window
  Timestamp + Store location → cell tower dump → devices present
  Timestamp + Store → WiFi logs → MAC addresses
  Parking lot cameras → license plates during window
  → Each source adds probabilistic weight to candidate identities

Step 4: Candidate Ranking
  Aggregate all evidence per candidate person
  Compute posterior probability: P(person | all evidence)
  Rank candidates by confidence
  Flag if multiple strong candidates (possible accomplice scenario)
```

This join logic runs as a **Rune-scriptable template scenario** — analysts can customize the join steps, add/remove data sources, and adjust confidence weights per source type.

#### Real-World Applicability

| Retail Chain | Cooperation Level (typical) | Data Available |
|---|---|---|
| **Walmart** | Cooperative with LE subpoena | POS, loyalty (Walmart+), surveillance (extensive), online orders |
| **Home Depot** | Cooperative | POS, Pro Xtra loyalty, surveillance, online orders |
| **Amazon** | Subpoena required | Full order history, payment, shipping, device data |
| **Target** | Cooperative | POS, Circle loyalty, RedCard (direct ID), surveillance |
| **Best Buy** | Cooperative | POS, My Best Buy loyalty, Geek Squad records (serial numbers) |
| **Gun stores (FFL)** | ATF Form 4473 required | **Full identity** — federal requirement for firearms |
| **Pharmacies** | HIPAA constraints | Prescription records require specific warrant |
| **Gas stations** | Limited | Card transactions, some surveillance, no loyalty for most |

**Real-case reference:** Unabomber — traced via specific hardware store purchases. Boston Marathon bombing — traced pressure cooker purchase via store records. BTK — traced floppy disk purchase to specific store. Many serial cases broken by tracing unusual product purchases (duct tape, zip ties, chemicals) back to buyers.

---

## Dependencies

### Already in `Cargo.toml`:

| Crate | Use |
|-------|-----|
| `rand = "0.8"` | Monte Carlo sampling, weighted choices |
| `rayon = "1.10"` | Parallel batch simulations |
| `tokio` | Async I/O for REST APIs and live feeds |
| `reqwest = "0.12"` | HTTP client for REST adapters |
| `serde` / `serde_json` / `ron` | Serialization |
| `bincode` | Binary scenario persistence |
| `zstd = "0.13"` | Compression for large scenarios |
| `uuid` | Unique IDs for all scenario entities |
| `chrono` | Temporal parameters and timestamps |
| `rune = "0.14"` | User-scriptable branch logic |
| `blake3 = "1"` | Hash chain for immutable audit trail |
| `bevy` | Plugin/Resource/Event/System integration |

### New dependencies needed for Phase 6:

| Crate | Use | Phase |
|-------|-----|-------|
| `ort` | ONNX Runtime — local ML inference for NLP extraction and predictive modeling | 401, 403 |
| `petgraph` | Graph data structures and algorithms for link analysis (centrality, shortest path, community detection) | 402 |
| `csv` | CSV parsing for digital forensics adapters (CDR, financial, tower dumps) | 404 |
| `calamine` | Excel/ODS parsing for forensics exports | 404 |

---

## Implementation Checklist

### Phase 0: Core Engine (Foundation)
- [ ] **201** Core data structures: Parameter, Entity, Evidence, Scenario, BranchNode, Outcome, ScenarioScale
- [ ] **202** Monte Carlo simulation engine with Bayesian updates (rayon threadpool)
- [ ] **203a** Data agglomeration: Local file adapters (JSON/CSV/RON)
- [ ] **203b** Data agglomeration: REST API adapters (public databases, weather, geospatial)
- [ ] **203c** Data agglomeration: Live feeds via Eustress Parameters (tokio async streams)
- [ ] **204** Micro/Macro composable hierarchy (macros contain micros as sub-scenarios)
- [ ] **205** Bevy ScenariosPlugin (resources, events, systems, rayon+tokio channel bridge)
- [ ] **207** Persistence: Scenarios embedded in Eustress binary format (bincode+zstd, large scenario support, multiple per Space)

### Phase 0.5: Branch Logic & Evidence
- [ ] **206a** Branch logic: Visual node editor in Slint UI (drag-and-drop)
- [ ] **206b** Branch logic: Rune script overrides for advanced users
- [ ] **206c** Branch logic: Natural language → AI-compiled to Rune
- [ ] **208a** Evidence attachment: Per-branch manual linking
- [ ] **208b** Evidence attachment: Automatic option (embedding-based inference)
- [ ] **209** Soft pruning: Visual collapse of low-probability branches (configurable threshold, no hard prune)

### Phase 1: Decision Tree Graph
- [ ] **210** 3D node-link diagram (spheres, edges, color=probability, interactive, soft-collapse rendering)

### Phase 2: Geospatial Heatmap
- [ ] **211** WGS84 mapping, entity pins, probability density overlay, temporal scrub, terrain integration

### Phase 3: Timeline Ribbon
- [ ] **212** Time axis, probability ribbons, fork points, Bayesian markers, scrub handle

### Phase 4: Slint Dashboard
- [ ] **213** Scenario tree, per-node stats, outcome table, param editor, progress bar, export

### Phase 5: Real-Case Investigative Features
- [ ] **301** Temporal decay & evidence freshness — configurable decay curves (linear, exponential, step) on Evidence confidence
- [ ] **302** Contradictory evidence handling — EvidenceConflict struct, ConflictType enum, resolution strategies (weighted avg, prefer A/B, hold both, unresolved)
- [ ] **303** Chain of custody / evidence provenance — ProvenanceChain with CustodyAction log, auto-computed admissibility score
- [ ] **304** Cognitive bias detection — Bevy system emitting BiasWarning events (confirmation, neglect, tunnel vision, anchoring, availability)
- [ ] **305** Witness reliability scoring — WitnessReliability struct (proximity, stress, consistency), composite score auto-adjusts likelihood ratio
- [ ] **306** Geographic profiling — Rossmo's formula / Bayesian distance decay, anchor point probability surface, buffer zone estimation
- [ ] **307** Timeline gap analysis — per-entity gap detection, GapSeverity classification, auto-generated GapHypothesis with probability estimates
- [ ] **308** Multi-analyst collaboration — Analyst roles (Lead/Contributor/Reviewer/Observer), independent AnalystViews, ConsensusView with agreement levels, conflict alerts
- [ ] **309** Victimology profiling — VictimologyProfile (routine activities, risk level, social graph, digital footprint), auto-generated OpportunityWindows
- [ ] **310** Scenario comparison / diff tool — ScenarioDiff (shared/unique evidence, BranchComparison, divergence score), side-by-side view, merge tool, A/B simulation

### Phase 6: SOTA Intelligence Features
- [ ] **401** NLP entity extraction pipeline — raw text (police reports, news, social media) → auto-extract entities, relationships, timestamps, locations → create Evidence/Entity objects
- [ ] **402** Link analysis / social network graph — centrality scoring, community detection, shortest path between entities, communication pattern analysis, financial flow mapping
- [ ] **403** ML predictive modeling — train on resolved cases to improve priors, pattern-based outcome prediction ("cases with these evidence patterns resolved as X 73% of the time")
- [ ] **404** Digital forensics adapters — Cellebrite exports, CDR (call detail records), financial transaction CSVs, GPS logs, social media metadata, phone tower triangulation
- [ ] **405** Immutable audit trail — every parameter change, evidence attachment, probability update, analyst action logged with timestamps for court-admissible reproducibility
- [ ] **406** Devil's Advocate / Red Team mode — auto-generate counter-arguments to leading hypothesis, surface neglected evidence, force analyst to address counter-evidence before finalizing
- [ ] **407** Retail / Product purchase tracking — Item entity type (SKU, UPC, serial, model), Transaction evidence type, retail source adapters (POS, loyalty, e-commerce, warranty, ALPR), multi-source probabilistic join (Item → Transaction → Person), Rune-scriptable template scenario
