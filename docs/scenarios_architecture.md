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
14. [Dependencies](#dependencies)
15. [Implementation Checklist](#implementation-checklist)

---

## Overview

Eustress Scenarios is a probabilistic scenario simulation engine embedded in the Eustress Engine. It provides a rigorous, iterative framework for modeling branching outcomes with Monte Carlo simulations, Bayesian updates, and 4D visualization (X/Y/Z spatial + T time/probability).

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

## Dependencies

All already present in `Cargo.toml`:

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
| `bevy` | Plugin/Resource/Event/System integration |

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
