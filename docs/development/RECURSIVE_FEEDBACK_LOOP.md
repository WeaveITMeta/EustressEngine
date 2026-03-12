# Recursive Self-Improvement Feedback Loop Architecture

## Purpose

Transform the Eustress Engine from a passive simulation tool into an **AI-governed recursive self-improvement machine**. The AI acts as the "Governor" of a closed-loop system where products are generated, tested, measured, and refined — each cycle producing verifiably superior results backed by scientific data.

---

## The Nine Systems

| # | System | Role in Loop | Existing Infrastructure |
|---|--------|-------------|------------------------|
| 0 | **Ideation** (`ideation_brief.toml` + `.md`) | Genesis — idea capture, patent draft, SOTA validation, AI-structured product brief | NEW — `IdeationBrief`, `PatentDraft`, `SotaValidation`, `ProductBriefPipeline` resources. Currently: `/create-voltec-product` workflow in Windsurf. Future: Workshop Panel in Eustress Studio Slint UI |
| 1 | **Instances** (`.instance.toml` + `.glb`) | Genotype — product shape, structure, properties | `file_loader.rs` → `InstanceDefinition` with rich PascalCase schema, mesh references, material/thermodynamic/electrochemical properties |
| 2 | **Soul Scripts** (`.soul` / `.rune`) | Phenotype — product behavior, test logic | `soul/mod.rs` → `SoulScriptData`, `build_pipeline.rs` → Claude API + Hot Compile, `rune_api.rs` → VM execution |
| 3 | **Data** (CSV/JSON/Watchpoints/Breakpoints) | Fitness Score — measured outputs | `simulation/rune_bindings.rs` → `WatchPointRegistry`, `BreakPointRegistry`, `SimulationRecording`, `TimeSeries` |
| 4 | **Simulation** (`simulation.toml`) | Time Compression — accelerated product lifetime | `simulation/plugin.rs` → `SimulationClock` with `time_scale`, `tick_rate_hz`, `max_ticks_per_frame`, play/pause/stop state machine |
| 5 | **AI Control** (MCP + Rust Hooks) | Governor — hypothesis → execution → verification | `mcp/` → CRUD over HTTP/WebSocket, `soul/build_pipeline.rs` → Claude API (Soul Service API key), `hot_reload.rs` → file watcher |
| 6 | **Realization Bridge** (`production_spec.json`) | Manufacturing Manifest — digital twin → factory floor | NEW — `ManufacturingManifest`, `SensitivityAnalysis`, `VerificationProtocol` resources |
| 7 | **Eustress Workshop** (`workshop_package/`) | Distributed Realization — simulation → home/civil center manufacturing | NEW — `WorkshopPackage`, `DreamManual`, `ValidationApp`, `BillOfMaterials` resources |
| 8 | **Legal Compliance** (`compliance_verdict.json`) | Safety Gate — AI moderation, code of conduct, regulatory clearance | NEW — `ComplianceAgent`, `ModerationPipeline`, `SafetyClassification`, `RegulatoryProfile` resources |

---

## Closed-Loop Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
│     RECURSIVE FEEDBACK LOOP: IDEATION → OPTIMIZATION → COMPLIANCE → REALIZATION → VALUE LOOP     │
│                                                                                                   │
│  ┌────────────────────────────────────────────────────────────────────────────┐                    │
│  │ 0. IDEATION (Genesis)                                                      │                    │
│  │ ─────────────────────                                                      │                    │
│  │ Input: Natural language idea, sketch, Soul Script, or Workshop Panel form  │                    │
│  │                                                                            │                    │
│  │ ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │                    │
│  │ │ Patent   │  │ SOTA     │  │ Engine   │  │ Blender  │  │ Instance     │  │                    │
│  │ │ Draft    │→ │ Valid.   │→ │ Require- │→ │ Mesh     │→ │ .toml + .glb │  │                    │
│  │ │ (.md)    │  │ (.md)    │  │ ments    │  │ Scripts  │  │ (Generated)  │  │                    │
│  │ └──────────┘  └──────────┘  └──────────┘  └──────────┘  └──────┬───────┘  │                    │
│  │                                                                 │          │                    │
│  │  Phase 1: /create-voltec-product workflow (Windsurf CLI)        │          │                    │
│  │  Phase 2: Workshop Panel (Eustress Studio Slint UI)             │          │                    │
│  └─────────────────────────────────────────────────────────────────┼──────────┘                    │
│                                                                    │                               │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐     │                               │
│  │ 1. TOML  │───▶│ 2. SOUL  │───▶│ 4. SIM   │───▶│ 3. DATA  │◄────┘  ◄── Real-World Usage ──┐   │
│  │ Instance │    │ Script   │    │ Engine   │    │ Metrics  │                                 │   │
│  │ (Geno-   │    │ (Pheno-  │    │ (Time    │    │ (Fitness │                                 │   │
│  │  type)   │    │  type)   │    │ Dilated) │    │  Score)  │                                 │   │
│  └────▲─────┘    └────▲─────┘    └──────────┘    └────┬─────┘                                 │   │
│       │               │           Each variant          │                                      │   │
│       │               │           gets its own          │                                      │   │
│       │               │           PooledVm from         │                                      │   │
│       │               │           VmPool (Rayon)        │                                      │   │
│       │         ┌─────┴──────────────────────────────────┘                                     │   │
│       │         │                                                                              │   │
│       │    ┌────▼─────┐    Global Max Found    ┌─────────────┐                                 │   │
│       │    │ 5. AI    │ ──────────────────────▶ │ 6. REALIZE  │                                 │   │
│       └────┤ Governor │  Hypothesize → Execute │ Bridge      │                                 │   │
│            │ (Multi-  │  → Verify → Refine     │ (Manifest)  │                                 │   │
│            │  Agent)  │  ← Telemetry Laws ──── └──────┬──────┘                                 │   │
│            └──────────┘                               │                                        │   │
│                 ▲                                      ▼                                        │   │
│                 │                             ┌─────────────────┐                               │   │
│     DiscoveredLaws                            │ production_     │                               │   │
│     as Telemetry                              │ spec.json       │                               │   │
│     (improves AI                              └────────┬────────┘                               │   │
│      prompt each                                       │                                        │   │
│      generation)                                       ▼                                        │   │
│                                         ┌──────────────────────────┐                            │   │
│                                         │ 8. LEGAL COMPLIANCE GATE │                            │   │
│                                         │ ──────────────────────── │                            │   │
│                                         │ AI Moderation Agents     │                            │   │
│                                         │ Safety Classification    │                            │   │
│                                         │ Regulatory Profile       │                            │   │
│                                         │ Code of Conduct ML       │                            │   │
│                                         └──────────┬───────────────┘                            │   │
│                                                    │                                            │   │
│                                         ┌──────────▼───────────┐                                │   │
│                                         │ compliance_          │                                │   │
│                                         │ verdict.json         │                                │   │
│                                         │ PASS / REJECT / HOLD │                                │   │
│                                         └──────────┬───────────┘                                │   │
│                                                    │ PASS only                                  │   │
│                         ┌──────────────────────────┼──────────────────────┐                      │   │
│                         │                          │                      │                      │   │
│                         ▼                          ▼                      ▼                      │   │
│                ┌─────────────────┐        ┌────────────────┐    ┌──────────────┐                 │   │
│                │ HUMAN ENGINEERS │        │ 7. WORKSHOP    │    │ INDIVIDUAL   │                 │   │
│                │ Factory Floor   │        │ Civil Center   │    │ Home 3D Print│                 │   │
│                └─────────────────┘        │ ────────────── │    └──────┬───────┘                 │   │
│                                           │ Print Files    │           │                         │   │
│                                           │ Dream Manual   │           │                         │   │
│                                           │ Hardware BOM   │           │                         │   │
│                                           │ Validation App │           │                         │   │
│                                           └───────┬────────┘           │                         │   │
│                                                   │                    │                         │   │
│                                                   └─────── Real-World Performance ──────────────┘   │
│                                                            Data (THE VALUE LOOP)                     │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## Stage 0: Ideation (Genesis)

### What Happens

Before any `.instance.toml` exists, before any mesh is generated, there is an **idea**. System 0 captures that idea — whether it arrives as a sentence typed into the Workshop Panel, a Soul Script sketch, a conversation with the AI, or a slash command in Windsurf — and transforms it into a complete product package ready for the optimization loop (Systems 1-8).

Ideation is the only system that requires a human in the loop. Everything after System 0 can run autonomously. This is the creative act — the rest is engineering.

### The Two Phases of Ideation UI

**Phase 1 (Current): `/create-voltec-product` Windsurf Workflow**

The existing workflow (`.windsurf/workflows/create-voltec-product.md`) is already a 9-step AI-driven pipeline that takes a natural language idea and produces:
1. `PATENT.md` — formal patent specification with cross-sections, BOM, claims
2. `SOTA_VALIDATION.md` — honesty-tiered validation against state of the art
3. `EustressEngine_Requirements.md` — material properties, ECS mappings, simulation laws
4. Blender Python scripts → AAA `.glb` meshes (headless, automated)
5. `.glb.toml` instance files with full realism sections
6. `README.md` — blueprint documentation
7. `Products.md` — catalog entry

This workflow runs inside Windsurf Cascade. The user types `/create-voltec-product`, provides inputs (name, description, category, innovations, specs, BOM, dimensions), and the AI generates everything. **System 0 is this workflow, formalized as a system.**

**Phase 2 (In Progress): Workshop Panel in Eustress Studio — Conversational Chat Interface**

The Workshop Panel is a **chat-interface dialogue** embedded in Eustress Studio as a right-panel tab (Properties | History | Soul | **Workshop**). Instead of a static form that fires everything at once and wastes API credits on assumptions, the system has a conversation with the user at each pipeline step — asking clarifying questions, confirming specs, proposing alternatives, and requiring explicit approval before each MCP command runs.

All AI interactions — including the conversational clarifications — use the **BYOK (Bring Your Own Key) API key** configured in Soul Settings. The same key powers every AI agent across all 9 systems. There are no free tiers; the approval gates exist to give the user control over *when* credits are spent, not to gatekeep between free and paid.

**Why a chat interface, not a form:**
- **Credit control** — each AI call is shown as an MCP command card with [Run] [Edit] [Skip] buttons. The user decides which steps are worth spending credits on. Skip SOTA validation? Save ~$0.04. The user is always in charge of their own API key spend.
- **Integrated thought** — the system asks "What electrolyte chemistry?" not "fill in field 7". The user's natural language answers are normalized into structured TOML by the AI. Each clarifying exchange costs a small amount against the BYOK key, but produces far better output than blind assumptions.
- **Customization at every step** — the user can redirect the patent angle, change material choices, or skip steps entirely. No wasted generation on things the user doesn't need.
- **Transparency** — every MCP endpoint call is visible inline with method, path, estimated cost, and status. The user sees exactly what the system is doing and what it will cost before approving.

```
┌──────────────────────────────────────────────────────────────┐
│ Properties │ History │ Soul │ [Workshop]                       │
├──────────────────────────────────────────────────────────────┤
│                                                                │
│  ○ Patent draft    ○ SOTA validation    ○ Requirements         │
│  ○ Mesh generation ○ Instance files     ○ Catalog              │
│                                                                │
│  ┌─ Workshop ──────────────────────────────────────────────┐  │
│  │  Describe your product idea.                             │  │
│  │  The system will guide you through patent, validation,   │  │
│  │  meshes, and TOML generation — step by step.            │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                │
│  ┌──────────────────────────────── user ┐                      │
│  │  Solid-state sodium-sulfur battery,  │                      │
│  │  15k cycle life, 900 Wh/kg          │                      │
│  └──────────────────────────────────────┘                      │
│                                                                │
│  ┌─ Workshop ──────────────────────────────────────────────┐  │
│  │  I'll normalize this into a product brief.               │  │
│  │  Let me confirm a few details:                           │  │
│  │                                                          │  │
│  │  1. Electrolyte: Sc-NASICON solid — correct?             │  │
│  │  2. Anode: Na metal (zero dendrite) — or alternative?    │  │
│  │  3. Form factor: prismatic or cylindrical?               │  │
│  │  4. Target tier: Foundation (mass production ready)?      │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                │
│  ┌──────────────────────────────── user ┐                      │
│  │  Yes, Sc-NASICON. Prismatic. Tier 1. │                      │
│  │  Add CNT cathode matrix too.          │                      │
│  └──────────────────────────────────────┘                      │
│                                                                │
│  ┌─ MCP Command ───────────────────────────────────────────┐  │
│  │  POST /mcp/ideation/normalize                            │  │
│  │                                                          │  │
│  │  Will generate ideation_brief.toml from your inputs.     │  │
│  │  Estimated cost: ~$0.03 (Sonnet)                         │  │
│  │                                                          │  │
│  │  [Run]  [Edit]  [Skip]                                   │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                │
│  ┌─ Artifact Generated ────────────────────────────────────┐  │
│  │  docs/Products/V-Cell_4680/ideation_brief.toml           │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                │
│  ┌─ MCP Command ───────────────────────────────────────────┐  │
│  │  POST /mcp/ideation/brief                                │  │
│  │  Step: Generate PATENT.md (42+ claims)                   │  │
│  │  Estimated cost: ~$0.05 (Sonnet)                         │  │
│  │                                                          │  │
│  │  [Run]  [Edit]  [Skip]                                   │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                │
│  ┌─────────────────────────────────── BYOK: ●●●●...k3Fq ─────┐│
│  │  [Type your message...]                           [Send]    ││
│  └─────────────────────────────────────────────────────────────┘│
└────────────────────────────────────────────────────────────────┘
```

**Conversation flow maps directly to MCP endpoints:**

All costs below are estimated for Sonnet-class models via the user's BYOK API key (Soul Settings).

| Conversation Step | MCP Endpoint | Approval Required | Estimated Cost (BYOK) |
|---|---|---|---|
| User describes idea → system asks clarifying questions | `POST /mcp/ideation/chat` | No (auto) | ~$0.01-0.02 per exchange |
| System normalizes into `ideation_brief.toml` | `POST /mcp/ideation/normalize` | **Yes** | ~$0.03 |
| Generate `PATENT.md` | `POST /mcp/ideation/brief` (step=patent) | **Yes** | ~$0.05 |
| Generate `SOTA_VALIDATION.md` | `POST /mcp/ideation/brief` (step=sota) | **Yes** | ~$0.04 |
| Generate `EustressEngine_Requirements.md` | `POST /mcp/ideation/brief` (step=requirements) | **Yes** | ~$0.04 |
| Generate Blender scripts + `.glb` meshes | `POST /mcp/ideation/brief` (step=meshes) | **Yes** | ~$0.03 + Blender time |
| Generate `.glb.toml` instance files | `POST /mcp/ideation/brief` (step=instances) | **Yes** | ~$0.02 |
| Register in `Products.md` catalog | `POST /mcp/ideation/brief` (step=catalog) | **Yes** | ~$0.01 |
| **Total per product** (with ~3 chat exchanges) | | | **~$0.27** |

At any step, the user can type additional instructions ("make the patent focus on thermal management", "skip SOTA — I know this is novel", "use aluminum 7075 instead of 6061") and the system adjusts. Each message costs a small amount against the BYOK key, but the conversation history persists as context for each subsequent API call, so later steps benefit from earlier clarifications without re-asking. The alternative — firing blind and regenerating — costs far more.

### The `ideation_brief.toml` Schema

Every idea, regardless of input method (form, natural language, Soul Script, or workflow), is normalized into an `ideation_brief.toml`. This file is the structured handoff from System 0 to Systems 1-2.

```toml
# Ideation Brief — the structured output of System 0
# This file is generated by the Ideation pipeline and consumed by Systems 1-8

[product]
name = "V-Cell 4680"
description = "Solid-state sodium-sulfur energy cell with scandium-doped NASICON electrolyte"
category = "conventional"       # "conventional" | "exotic_propulsion"
tier = "foundation"             # "foundation" | "platform" | "horizon"
version = "V1"

[product.dimensions]
width = 0.100                   # meters
height = 0.300                  # meters
depth = 0.012                   # meters
form_factor = "prismatic"       # "prismatic" | "cylindrical" | "disc" | "custom"

[[innovations]]
name = "Sc-NASICON Solid Electrolyte"
description = "Scandium-doped Na₃Zr₂Si₂PO₁₂ with ionic conductivity 3× higher than undoped NASICON"
tier = "VERIFIED"               # "VERIFIED" | "PROJECTED" | "ASPIRATIONAL"

[[innovations]]
name = "Na Metal Anode (Zero Dendrite)"
description = "Pure sodium metal anode enabled by solid electrolyte barrier — no dendrite formation pathway"
tier = "PROJECTED"

[[target_specs]]
metric = "energy_density"
target = 900.0
unit = "Wh/kg"
benchmark = 250.0
benchmark_label = "Li-Ion (NMC 811)"

[[target_specs]]
metric = "cycle_life"
target = 15000
unit = "cycles"
benchmark = 1500
benchmark_label = "Li-Ion (NMC 811)"

[[bill_of_materials]]
component = "Housing"
material = "Al 6061-T6"
dimensions = [0.300, 0.100, 0.012]  # meters [L, W, H]
role = "structural_enclosure"

[[bill_of_materials]]
component = "Anode"
material = "Na metal"
dimensions = [0.280, 0.090, 0.002]
role = "negative_electrode"

[[bill_of_materials]]
component = "Electrolyte"
material = "Sc-NASICON"
dimensions = [0.280, 0.090, 0.001]
role = "ion_conductor"

# Physics model (exotic propulsion only)
# [physics_model]
# type = "element_115_reactor"
# ...

[ideation_metadata]
source = "workshop_panel"       # "windsurf_workflow" | "workshop_panel" | "soul_script" | "natural_language" | "import"
created = "2026-03-12T18:30:00Z"
author = "user"
ai_model = "claude-sonnet-4-20250514"
generation_time_seconds = 312
```

### Four Input Modes

| Mode | Input | AI Processing | Output |
|------|-------|--------------|--------|
| **Form** (Workshop Panel) | Structured fields: name, specs, BOM | AI fills gaps, generates patent/SOTA/requirements, runs Blender | Full product directory + `ideation_brief.toml` |
| **Natural Language** | "I want a battery that lasts 15,000 cycles using sodium instead of lithium" | AI extracts specs, infers BOM, proposes dimensions, generates everything | Full product directory + `ideation_brief.toml` |
| **Soul Script** | `.soul` markdown describing the product behavior and test protocol | AI reverse-engineers the product definition from behavior spec, then generates PATENT/SOTA/meshes | Full product directory + `ideation_brief.toml` |
| **Import** | Existing `ideation_brief.toml` from another project or prior session | Skip generation, validate, proceed directly to Systems 1-8 | Validated brief → pipeline |

### Rust Resource Design

```rust
/// System 0: Ideation — captures ideas and transforms them into product packages
#[derive(Resource)]
pub struct IdeationPipeline {
    /// Current ideation brief being processed
    pub active_brief: Option<IdeationBrief>,
    /// Pipeline state machine
    pub state: IdeationState,
    /// Generated artifacts (patent, SOTA, requirements, meshes, instances)
    pub artifacts: IdeationArtifacts,
    /// History of all ideations (for the Workshop Panel to display)
    pub history: Vec<IdeationRecord>,
}

/// The structured product brief — normalized from any input mode
#[derive(Clone, Serialize, Deserialize)]
pub struct IdeationBrief {
    pub product: ProductDefinition,
    pub innovations: Vec<Innovation>,
    pub target_specs: Vec<TargetSpec>,
    pub bill_of_materials: Vec<BomEntry>,
    pub physics_model: Option<PhysicsModel>,
    pub metadata: IdeationMetadata,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProductDefinition {
    pub name: String,
    pub description: String,
    pub category: ProductCategory,
    pub tier: ProductTier,
    pub version: String,
    pub dimensions: ProductDimensions,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ProductCategory {
    Conventional,
    ExoticPropulsion,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ProductTier {
    Foundation,   // Tier 1: shipping now or near-term
    Platform,     // Tier 2: 18 months
    Horizon,      // Tier 3: 3-5 years
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Innovation {
    pub name: String,
    pub description: String,
    pub tier: ValidationTier,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ValidationTier {
    Verified,      // Published data supports this claim
    Projected,     // Physics supports this, but not yet demonstrated at scale
    Aspirational,  // Theoretical — requires breakthroughs
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TargetSpec {
    pub metric: String,
    pub target: f64,
    pub unit: String,
    pub benchmark: f64,
    pub benchmark_label: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BomEntry {
    pub component: String,
    pub material: String,
    pub dimensions: [f64; 3],
    pub role: String,
}

/// Pipeline state machine for System 0
#[derive(Clone, Copy, PartialEq)]
pub enum IdeationState {
    /// Waiting for user input
    Idle,
    /// Normalizing input into IdeationBrief (for NL/Soul Script modes)
    Normalizing,
    /// Generating PATENT.md
    GeneratingPatent,
    /// Generating SOTA_VALIDATION.md
    GeneratingSotaValidation,
    /// Generating EustressEngine_Requirements.md
    GeneratingRequirements,
    /// Running Blender headless for mesh generation
    GeneratingMeshes,
    /// Creating .glb.toml instance files
    GeneratingInstances,
    /// Creating README.md and updating Products.md catalog
    FinalizingCatalog,
    /// Complete — ready to hand off to Systems 1-8
    Complete,
    /// Error state
    Failed,
}

/// All generated artifacts from System 0
#[derive(Default)]
pub struct IdeationArtifacts {
    pub patent_md: Option<PathBuf>,
    pub sota_validation_md: Option<PathBuf>,
    pub engine_requirements_md: Option<PathBuf>,
    pub blender_scripts: Vec<PathBuf>,
    pub generated_meshes: Vec<PathBuf>,
    pub instance_tomls: Vec<PathBuf>,
    pub readme_md: Option<PathBuf>,
    pub ideation_brief_toml: Option<PathBuf>,
}

/// Record of a completed ideation for history
#[derive(Clone, Serialize, Deserialize)]
pub struct IdeationRecord {
    pub brief: IdeationBrief,
    pub artifacts: Vec<String>,   // paths relative to product directory
    pub created: DateTime<Utc>,
    pub generation_time_seconds: u64,
}
```

### How System 0 Feeds System 1

When System 0 reaches `IdeationState::Complete`, it fires a `ProductCreatedEvent`:

```rust
/// Fired when System 0 completes — triggers the optimization loop
pub struct ProductCreatedEvent {
    /// Path to the product directory (e.g., docs/Products/V-Cell/)
    pub product_dir: PathBuf,
    /// The ideation brief
    pub brief: IdeationBrief,
    /// Paths to all generated .glb.toml instance files
    pub instance_paths: Vec<PathBuf>,
    /// Path to the ideation_brief.toml
    pub brief_path: PathBuf,
}
```

This event is consumed by:
- **System 1** (`InstanceGenerator`): Loads the generated `.instance.toml` files as the initial genotype
- **System 2** (`ScriptGenerator`): Reads the `EustressEngine_Requirements.md` to generate the initial `.soul`/`.rune` test scripts
- **System 5** (`MultiAgentGovernor`): Reads the `ideation_brief.toml` target specs to define the fitness function
- **Studio Heuristic**: Clicking "Optimize & Build" after ideation starts the 8-system optimization loop with the generated product as seed

### MCP Ideation Endpoints (Conversational Model)

The Workshop Panel's chat interface maps each conversation turn to an MCP endpoint. All AI-powered endpoints use the **BYOK API key** from Soul Settings — the same key that powers every AI agent across all 9 systems. The `step` parameter on `POST /mcp/ideation/brief` controls which artifact to generate next — the user approves each step individually instead of firing the whole pipeline blindly. Approval gates ([Run] [Edit] [Skip] buttons) give the user control over when and where their credits are spent.

| Endpoint | Method | Purpose | Approval | Est. Cost (BYOK) |
|----------|--------|---------|----------|-------------------|
| `/mcp/ideation/chat` | POST | Send a user message; AI responds with clarifying questions or confirmations | No (auto) | ~$0.01-0.02/exchange |
| `/mcp/ideation/normalize` | POST | Normalize freeform text or Soul Script into structured `ideation_brief.toml` | **Yes** | ~$0.03 |
| `/mcp/ideation/brief` | POST | Generate a single artifact. `step` param: `patent`, `sota`, `requirements`, `meshes`, `instances`, `catalog` | **Yes** (per step) | $0.01-0.05 |
| `/mcp/ideation/brief/all` | POST | Fire all remaining steps without individual approval (power-user shortcut) | **Yes** (once) | ~$0.22 |
| `/mcp/ideation/status` | GET | Poll current `IdeationState`, active step, and conversation context | No | Local only |
| `/mcp/ideation/artifacts` | GET | List generated artifact paths with types and sizes | No | Local only |
| `/mcp/ideation/history` | GET | Retrieve past ideation records (product name, date, artifact count) | No | Local only |
| `/mcp/ideation/import` | POST | Import an existing `ideation_brief.toml` — skips normalization step | No | Local only |
| `/mcp/ideation/cancel` | POST | Cancel the current pipeline, preserving already-generated artifacts | No | Local only |

**Key design principle:** Every AI interaction costs credits against the user's BYOK API key. The GET endpoints (`status`, `artifacts`, `history`) are local-only reads with zero API cost. The conversational `/mcp/ideation/chat` endpoint uses the AI for each exchange (~$0.01-0.02), but this investment in clarification prevents far more expensive wasted generations downstream. The approval gates on artifact generation steps (`normalize`, `brief`) let the user decide which outputs are worth generating — skip what you don't need, refine what you do.

### Connection to the `/create-voltec-product` Workflow

The existing workflow is **System 0 Phase 1 implemented**. The mapping:

| Workflow Step | System 0 State | Output |
|---------------|---------------|--------|
| Step 1: Create directory | `Normalizing` | `docs/Products/{Name}/` directory |
| Step 2: Draft PATENT.md | `GeneratingPatent` | `PATENT.md` |
| Step 3: Draft SOTA_VALIDATION.md | `GeneratingSotaValidation` | `SOTA_VALIDATION.md` |
| Step 4: Create EustressEngine_Requirements.md | `GeneratingRequirements` | `EustressEngine_Requirements.md` |
| Step 5: Generate meshes via Blender | `GeneratingMeshes` | `.glb` files in `V1/meshes/` |
| Step 6: Create .glb.toml instance files | `GeneratingInstances` | `.glb.toml` files in `V1/` |
| Step 7: Create README.md | `FinalizingCatalog` | `README.md` |
| Step 8: Update Products.md | `FinalizingCatalog` | `Products.md` entry |
| Step 9: Verification | `Complete` | `ProductCreatedEvent` fired |

When the Workshop Panel (Phase 2) is built in Slint, it executes the **exact same pipeline** but through the Eustress Studio UI instead of Windsurf chat. The `IdeationPipeline` resource is the same — only the input method changes.

---

## Stage 1: Collection (Genotype Definition)

### What Happens
The AI generates or modifies `.instance.toml` files defining the product under test. Each TOML file is the product's **genotype** — its physical structure, material properties, mesh reference, and simulation-relevant parameters. In the initial generation (generation 0), these files come directly from System 0 (Ideation). In subsequent generations, they are mutated by the AI Governor (System 5).

### Existing Hooks
- `InstanceDefinition` struct in `file_loader.rs` — parses `.instance.toml` with rich PascalCase schema
- `file_watcher.rs` — `FileChangeEvent` detects TOML modifications and triggers entity respawn
- `space_ops.rs` — `save_space()` serializes entities back to `.part.toml` files

### Required Control Hooks

#### 1.1 `InstanceGenerator` Resource
```rust
/// AI-driven instance generation and mutation
#[derive(Resource, Default)]
pub struct InstanceGenerator {
    /// Queue of TOML files to write (AI-generated)
    pub pending_writes: Vec<PendingInstanceWrite>,
    /// History of all generated instances with their fitness scores
    pub generation_history: Vec<GenerationRecord>,
    /// Current generation number (evolutionary counter)
    pub generation: u64,
}

pub struct PendingInstanceWrite {
    pub path: PathBuf,
    pub toml_content: String,
    pub hypothesis: String,       // AI's prediction before running
    pub parent_generation: u64,   // Which generation this mutated from
}

pub struct GenerationRecord {
    pub generation: u64,
    pub toml_hash: String,
    pub fitness_score: f64,
    pub hypothesis: String,
    pub actual_result: String,
    pub statistical_significance: f64,  // p-value from verification
}
```

#### 1.2 Hot-Reload Watcher Enhancement
The existing `file_watcher.rs` already detects `.toml` changes and respawns entities. Enhancement needed:
- Track which generation triggered the reload
- Associate the new entity with its `GenerationRecord`
- Emit a `GenerationSpawned` event so the simulation auto-starts

### Data Flow
```
AI Governor → writes .instance.toml → file_watcher detects → entity spawned → GenerationSpawned event
```

---

## Stage 2: Analysis (Phenotype Scripting)

### What Happens
The AI writes `.soul` or `.rune` scripts that define **how the product behaves** during simulation. These scripts read entity properties, apply physics/chemistry logic, record watchpoints, and set breakpoints.

### Existing Hooks
- `SoulScriptData` component — markdown source, compiled AST, generated Rune code
- `SoulBuildPipeline` — Claude API generates Rune code from Soul markdown
- `SimController` in `rune_bindings.rs` — Rune scripts can call `sim.record()`, `sim.add_watchpoint()`, `sim.add_breakpoint()`, `sim.set_time_scale()`, `sim.run_days()`, etc.
- `hot_reload.rs` / `file_watcher.rs` — detects `.soul` file changes and triggers rebuild

### Required Control Hooks

#### 2.1 `ScriptGenerator` Resource
```rust
/// AI-driven script generation for simulation behavior
#[derive(Resource, Default)]
pub struct ScriptGenerator {
    /// Queue of scripts to write
    pub pending_scripts: Vec<PendingScriptWrite>,
    /// Template library — reusable patterns the AI has learned
    pub learned_patterns: Vec<LearnedPattern>,
}

pub struct PendingScriptWrite {
    pub path: PathBuf,
    pub soul_markdown: String,   // Natural language script description
    pub hypothesis: String,       // What this script tests
}

pub struct LearnedPattern {
    pub name: String,
    pub description: String,
    pub rune_template: String,
    pub success_rate: f64,        // How often this pattern improved fitness
    pub discovery_generation: u64,
}
```

#### 2.2 Simulation Script Template
Every AI-generated script must follow this structure:
```rune
// Setup: Define watchpoints and breakpoints
pub fn setup(sim) {
    sim.add_watchpoint("metric_name", "Human Label", "unit");
    sim.add_breakpoint("safety_limit", "metric_name", ">", threshold);
    sim.set_time_scale(3600.0);  // 1 hour per second
}

// Tick: Called every simulation tick
pub fn tick(sim, entity) {
    let value = compute_metric(entity);
    sim.record("metric_name", value);
}

// Teardown: Export and analyze
pub fn teardown(sim) {
    sim.export("output/run_{generation}.csv");
}
```

---

## Stage 3: Execution (Time-Dilated Simulation)

### What Happens
The simulation engine runs at accelerated time scales, compressing years of product lifetime into seconds of wall-clock time. The `SimulationClock` tracks both simulation time and wall time.

### Existing Hooks
- `SimulationClock` — `time_scale`, `tick_rate_hz`, `fixed_timestep_s`, `advance(wall_delta)` returns ticks to run
- `SimulationState` — `should_tick()`, `after_tick()`, completion conditions
- `PlayModeState` — `Playing` / `Paused` / `Editing` state machine
- `simulation.toml` — `tick_rate_hz`, `time_scale`, `max_ticks_per_frame`, recording config

### Required Control Hooks

#### 3.1 `SimulationOrchestrator` Resource
```rust
/// Orchestrates simulation runs for the feedback loop
#[derive(Resource, Default)]
pub struct SimulationOrchestrator {
    /// Current run configuration
    pub current_run: Option<SimulationRun>,
    /// Queue of runs to execute
    pub run_queue: VecDeque<SimulationRunConfig>,
    /// Completed runs awaiting analysis
    pub completed_runs: Vec<CompletedRun>,
}

pub struct SimulationRunConfig {
    pub generation: u64,
    pub time_scale: f64,
    pub target_simulation_time_s: f64,  // How long to simulate
    pub tick_rate_hz: f64,
    pub watchpoints: Vec<WatchpointConfig>,
    pub breakpoints: Vec<BreakpointConfig>,
    pub sampling_interval_ticks: u64,   // Anti-aliasing for high-speed sims
}

pub struct CompletedRun {
    pub generation: u64,
    pub recording: SimulationRecording,
    pub wall_time_s: f64,
    pub simulation_time_s: f64,
    pub exit_reason: ExitReason,  // Completed, Breakpoint, Timeout
}
```

#### 3.2 Time-Scaled Logging (Anti-Aliasing)
Critical for high-speed simulations: if `time_scale = 31536000` (1 year/second), a 60Hz tick rate means each tick spans ~6 days. Micro-oscillations between ticks are invisible.

Solution: **Adaptive sampling rate**
```rust
/// Compute minimum safe sampling interval based on time scale
pub fn safe_sampling_interval(time_scale: f64, tick_rate_hz: f64) -> u64 {
    let sim_seconds_per_tick = time_scale / tick_rate_hz;
    // Sample at least every simulated minute for fast sims
    if sim_seconds_per_tick > 60.0 {
        1  // Every tick (already coarse)
    } else if sim_seconds_per_tick > 1.0 {
        1  // Every tick
    } else {
        // Can afford to skip ticks for slower sims
        (60.0 / sim_seconds_per_tick).ceil() as u64  // ~1 sample per simulated minute
    }
}
```

---

## Stage 4: Measurement (Fitness Scoring)

### What Happens
After each simulation run, the engine exports CSV/JSON data containing all watchpoint time series, breakpoint triggers, and computed metrics. The AI reads this data to calculate a **fitness score**.

### Existing Hooks
- `WatchPointRegistry` — current/min/max/average per named watchpoint
- `BreakPointRegistry` — conditional triggers on variables
- `SimulationRecording` — `TimeSeries` data with tick-level granularity
- `ActiveRecording` — start/stop/export recording lifecycle

### Required Control Hooks

#### 4.1 `FitnessFunction` Resource
```rust
/// Rust-defined objective function — the AI optimizes against this
#[derive(Resource)]
pub struct FitnessFunction {
    /// Name of this fitness function
    pub name: String,
    /// Human-readable description of the optimization goal
    pub description: String,
    /// The actual scoring function
    pub score_fn: Box<dyn Fn(&CompletedRun) -> FitnessResult + Send + Sync>,
    /// Baseline score from the first generation (or manual benchmark)
    pub baseline: Option<f64>,
    /// Best score achieved so far
    pub best_score: f64,
    /// Best generation that achieved the best score
    pub best_generation: u64,
}

pub struct FitnessResult {
    pub score: f64,
    pub breakdown: HashMap<String, f64>,   // Sub-metrics
    pub is_valid: bool,                     // Did the simulation complete cleanly?
    pub notes: Vec<String>,                 // Human-readable observations
}
```

#### 4.2 Predefined Fitness Functions
```rust
/// Example: Battery cycle life fitness
pub fn battery_cycle_fitness(run: &CompletedRun) -> FitnessResult {
    let cycles = run.recording.get_series("cycle_count").last_value();
    let capacity_retention = run.recording.get_series("capacity_pct").last_value();
    let thermal_max = run.recording.get_series("temperature_c").max_value();

    // Objective: maximize cycles while keeping capacity > 80% and temperature < 60C
    let score = if thermal_max > 60.0 {
        0.0  // Safety violation — invalid
    } else {
        cycles * (capacity_retention / 100.0)
    };

    FitnessResult {
        score,
        breakdown: HashMap::from([
            ("cycles".into(), cycles),
            ("capacity_retention".into(), capacity_retention),
            ("thermal_max".into(), thermal_max),
        ]),
        is_valid: thermal_max <= 60.0 && capacity_retention > 0.0,
        notes: vec![],
    }
}
```

#### 4.3 Data Export Format
Each completed run exports to:
```
output/
  generation_{N}/
    metrics.csv          # Time series: tick, sim_time, watchpoint_1, watchpoint_2, ...
    breakpoints.json     # All breakpoint trigger events
    fitness.json         # FitnessResult with score and breakdown
    instance.toml        # Copy of the genotype that produced this run
    script.soul          # Copy of the phenotype script
    summary.json         # Generation, hypothesis, actual result, p-value
```

---

## Stage 5: Optimization (AI Governor)

### What Happens
The AI analyzes the fitness data, compares to baseline, runs statistical significance tests, and either:
- **Accepts** the improvement and uses the new generation as the baseline
- **Rejects** the regression and rolls back, trying a different mutation
- **Extracts** a universal law if the improvement is consistent across multiple runs

### Required Control Hooks

#### 5.1 `VerificationGate` Resource
```rust
/// Statistical verification before accepting a new generation
#[derive(Resource)]
pub struct VerificationGate {
    /// Minimum number of runs per generation for statistical validity
    pub min_runs: usize,
    /// Minimum p-value for accepting an improvement
    pub significance_threshold: f64,
    /// History of verification decisions
    pub decisions: Vec<VerificationDecision>,
}

pub struct VerificationDecision {
    pub generation: u64,
    pub accepted: bool,
    pub baseline_score: f64,
    pub candidate_score: f64,
    pub p_value: f64,
    pub sample_size: usize,
    pub decision_reason: String,
}

impl VerificationGate {
    /// Welch's t-test for comparing two populations of fitness scores
    pub fn verify(&self, baseline_scores: &[f64], candidate_scores: &[f64]) -> VerificationDecision {
        let n1 = baseline_scores.len() as f64;
        let n2 = candidate_scores.len() as f64;
        let mean1: f64 = baseline_scores.iter().sum::<f64>() / n1;
        let mean2: f64 = candidate_scores.iter().sum::<f64>() / n2;
        let var1: f64 = baseline_scores.iter().map(|x| (x - mean1).powi(2)).sum::<f64>() / (n1 - 1.0);
        let var2: f64 = candidate_scores.iter().map(|x| (x - mean2).powi(2)).sum::<f64>() / (n2 - 1.0);
        let t_stat = (mean2 - mean1) / (var1 / n1 + var2 / n2).sqrt();
        // Approximate p-value (simplified — use statrs crate for production)
        let p_value = (-t_stat.abs()).exp();  // Placeholder

        let accepted = mean2 > mean1 && p_value < self.significance_threshold;

        VerificationDecision {
            generation: 0, // Set by caller
            accepted,
            baseline_score: mean1,
            candidate_score: mean2,
            p_value,
            sample_size: baseline_scores.len() + candidate_scores.len(),
            decision_reason: if accepted {
                format!("Improvement: {:.4} → {:.4} (p={:.4})", mean1, mean2, p_value)
            } else if mean2 <= mean1 {
                format!("Regression: {:.4} → {:.4}", mean1, mean2)
            } else {
                format!("Not significant: p={:.4} > threshold={:.4}", p_value, self.significance_threshold)
            },
        }
    }
}
```

#### 5.2 `KnowledgeBase` Resource
```rust
/// Persistent knowledge extracted from successful experiments
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct KnowledgeBase {
    /// Universal laws discovered through experimentation
    pub laws: Vec<DiscoveredLaw>,
    /// Path to persistent storage
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DiscoveredLaw {
    pub name: String,
    pub description: String,
    pub discovered_at_generation: u64,
    pub fitness_impact: f64,        // Average improvement when applied
    pub confidence: f64,            // Statistical confidence
    pub toml_property_changes: HashMap<String, String>,  // What to change
    pub rune_pattern: Option<String>,                     // Behavioral pattern
    pub applies_to: Vec<String>,    // Which product types this law covers
}
```

#### 5.3 MCP Endpoint Extensions
New endpoints for AI Governor control:

| Endpoint | Method | Purpose |
|----------|--------|--------|
| `POST /mcp/governor/hypothesize` | POST | AI submits hypothesis + TOML/script changes |
| `POST /mcp/governor/execute` | POST | Trigger simulation run with config |
| `GET /mcp/governor/results/{generation}` | GET | Retrieve fitness data for generation |
| `POST /mcp/governor/verify` | POST | Run verification gate on results |
| `POST /mcp/governor/accept` | POST | Accept generation as new baseline |
| `POST /mcp/governor/rollback` | POST | Reject generation, revert to baseline |
| `GET /mcp/governor/knowledge` | GET | Read knowledge base |
| `POST /mcp/governor/learn` | POST | Add discovered law to knowledge base |
| `POST /mcp/governor/realize` | POST | Generate manufacturing manifest for best generation |
| `GET /mcp/governor/manifest/{generation}` | GET | Retrieve production_spec.json for generation |

---

## Stage 6: Realization Bridge (Manufacturing and Synthesis)

### What Happens
Once the AI Governor finds a verified global maximum, System 6 bridges the gap between the digital simulation and the physical factory floor. The AI generates a **Manifest of Fundamental Truths** — a `production_spec.json` that human engineers use as a verified blueprint for manufacturing.

This is the terminal output of every successful recursive cycle. Engineers no longer guess at designs — they receive a statistically-backed specification that says: *"To achieve the utility demonstrated in Simulation Run #402, the physical mesh must maintain these 14 coordinate points, and the logic controller must trigger at exactly 12ms. Here is the data proving why any other configuration is sub-optimal."*

### The `production_spec.json` Schema

```json
{
  "manifest_version": "1.0.0",
  "product_name": "V-Cell 4680 Lithium Battery",
  "generation": 402,
  "fitness_score": 2847.3,
  "timestamp": "2026-03-12T17:53:00Z",
  "source_run": "output/generation_402/",

  "hard_constraints": {
    "description": "Inviolable physical properties that must be met for the simulation's utility to manifest in reality.",
    "properties": [
      {
        "name": "CathodeThickness",
        "value": 0.085,
        "unit": "mm",
        "tolerance": 0.002,
        "source_toml_path": "instances/v_cell_cathode.instance.toml",
        "source_toml_key": "Properties.CathodeThickness",
        "criticality": "inviolable",
        "failure_mode": "Capacity drops below 80% retention threshold at 500 cycles"
      },
      {
        "name": "ElectrolyteMolarity",
        "value": 1.2,
        "unit": "mol/L",
        "tolerance": 0.05,
        "source_toml_path": "instances/v_cell_electrolyte.instance.toml",
        "source_toml_key": "Properties.Molarity",
        "criticality": "inviolable",
        "failure_mode": "Thermal runaway probability exceeds 0.1% above 1.3 mol/L"
      }
    ],
    "mesh_constraints": [
      {
        "mesh_file": "meshes/v_cell_housing.glb",
        "critical_points": [
          { "name": "terminal_positive", "position": [0.0, 32.5, 0.0], "tolerance_mm": 0.01 },
          { "name": "terminal_negative", "position": [0.0, -32.5, 0.0], "tolerance_mm": 0.01 },
          { "name": "vent_disc_center", "position": [0.0, 33.0, 0.0], "tolerance_mm": 0.05 }
        ],
        "overall_tolerance_mm": 0.1
      }
    ]
  },

  "sensitivity_analysis": {
    "description": "Which variables are critical to quality. Ranked by impact on fitness score.",
    "variables": [
      {
        "name": "CathodeThickness",
        "sensitivity_coefficient": 18.7,
        "interpretation": "A 2% deviation in CathodeThickness results in a 37.4% change in cycle life fitness.",
        "direction": "Thicker cathode increases capacity but reduces cycle life due to mechanical stress.",
        "optimal_range": { "min": 0.083, "max": 0.087, "unit": "mm" },
        "deviation_impact": [
          { "deviation_pct": 1.0, "fitness_change_pct": -18.7 },
          { "deviation_pct": 2.0, "fitness_change_pct": -37.4 },
          { "deviation_pct": 5.0, "fitness_change_pct": -93.5, "note": "Catastrophic — below minimum viable product" }
        ]
      },
      {
        "name": "ElectrolyteMolarity",
        "sensitivity_coefficient": 12.3,
        "interpretation": "A 2% deviation in ElectrolyteMolarity results in a 24.6% change in thermal safety margin.",
        "direction": "Higher molarity improves conductivity but increases thermal risk exponentially above 1.25 mol/L.",
        "optimal_range": { "min": 1.15, "max": 1.25, "unit": "mol/L" },
        "deviation_impact": [
          { "deviation_pct": 1.0, "fitness_change_pct": -12.3 },
          { "deviation_pct": 2.0, "fitness_change_pct": -24.6 },
          { "deviation_pct": 5.0, "fitness_change_pct": -61.5 }
        ]
      }
    ]
  },

  "logic_trace": {
    "description": "Natural language explanation of the mathematical relationship between TOML properties and performance data.",
    "summary": "The optimal V-Cell configuration achieves 2847 cycle-capacity-units by balancing cathode thickness (energy density) against mechanical stress (cycle degradation). The relationship is governed by a modified Arrhenius equation where degradation rate k = A * exp(-Ea / (R * T)) is minimized at T_avg = 35.2C through the specific electrolyte molarity of 1.2 mol/L.",
    "causal_chain": [
      {
        "cause": "CathodeThickness = 0.085mm",
        "mechanism": "Lithium-ion diffusion path length of 42.5um provides optimal balance between energy density and mechanical stress during intercalation/deintercalation cycles.",
        "effect": "Capacity retention of 92.1% at 3000 cycles",
        "evidence": "generation_402/metrics.csv columns: cathode_thickness, capacity_pct, cycle_count"
      },
      {
        "cause": "ElectrolyteMolarity = 1.2 mol/L",
        "mechanism": "Ionic conductivity peaks at 1.2 mol/L for this specific solvent system. Higher concentrations increase viscosity faster than they increase ion count, reducing effective conductivity.",
        "effect": "Average operating temperature of 35.2C (well within 60C safety limit)",
        "evidence": "generation_402/metrics.csv columns: electrolyte_molarity, temperature_c, conductivity_s_cm"
      }
    ],
    "discovered_laws_applied": [
      "law_023_cathode_stress_threshold",
      "law_041_electrolyte_viscosity_crossover"
    ]
  },

  "verification_protocol": {
    "description": "Real-world tests that mirror simulation watchpoints. Engineers execute these to verify the physical prototype matches the digital twin.",
    "tests": [
      {
        "test_id": "VP-001",
        "name": "Cycle Life Validation",
        "mirrors_watchpoint": "cycle_count",
        "procedure": "Subject 5 cells to standard CC-CV charging (0.5C to 4.2V, CV until 0.05C cutoff) and 1C discharge to 2.5V. Record capacity at every 100th cycle.",
        "pass_criteria": "Capacity retention >= 88% at 3000 cycles (simulation predicts 92.1% ± 4.0%)",
        "expected_value": 92.1,
        "tolerance_pct": 4.0,
        "sample_size": 5,
        "statistical_method": "One-sample t-test against simulation mean"
      },
      {
        "test_id": "VP-002",
        "name": "Thermal Safety Validation",
        "mirrors_watchpoint": "temperature_c",
        "procedure": "During cycle test VP-001, continuously log cell surface temperature at 1Hz using K-type thermocouple. Record max temperature per cycle.",
        "pass_criteria": "Max temperature < 55C under all conditions (simulation predicts max 48.3C)",
        "expected_value": 48.3,
        "tolerance_pct": 15.0,
        "sample_size": 5,
        "statistical_method": "Upper confidence bound at 99%"
      },
      {
        "test_id": "VP-003",
        "name": "Mesh Dimensional Validation",
        "mirrors_watchpoint": null,
        "procedure": "CMM (Coordinate Measuring Machine) inspection of housing against critical_points in mesh_constraints. Measure all 3 critical datum points.",
        "pass_criteria": "All critical points within specified tolerance_mm",
        "expected_value": null,
        "tolerance_pct": null,
        "sample_size": 10,
        "statistical_method": "Cpk >= 1.33 for each critical dimension"
      }
    ]
  },

  "reality_risks": {
    "description": "Gap analysis: where simulation perfection might be impossible to manufacture, and the closest achievable physical proxy.",
    "risks": [
      {
        "risk_id": "RR-001",
        "simulation_assumption": "Perfectly uniform cathode coating thickness across entire electrode area",
        "manufacturing_reality": "Slot-die coating typically achieves ±3% thickness variation across web width",
        "impact": "Localized hotspots where cathode is thinner, reducing effective cycle life by estimated 5-8%",
        "physical_proxy": "Use dual-pass coating with inline laser thickness gauge feedback loop. Target ±1.5% variation.",
        "residual_risk": "low"
      },
      {
        "risk_id": "RR-002",
        "simulation_assumption": "Electrolyte molarity is exactly 1.200 mol/L throughout cell volume",
        "manufacturing_reality": "Batch mixing achieves ±2% concentration. Wetting uniformity adds another ±1% local variation.",
        "impact": "Local conductivity variations create uneven current distribution. Simulation sensitivity shows this is within acceptable range (12.3% sensitivity × 3% deviation = 36.9% worst-case fitness impact).",
        "physical_proxy": "Use inline refractometer for batch QC. Reject batches outside ±1.5%. Allow ±1% wetting variation.",
        "residual_risk": "medium"
      }
    ]
  },

  "firmware_logic": {
    "description": "Rune script behaviors that must be hard-coded into the physical product's firmware or controller.",
    "controllers": [
      {
        "name": "BatteryManagementSystem",
        "source_script": "scripts/v_cell_bms.soul",
        "critical_timing": [
          {
            "trigger": "voltage >= 4.15V",
            "action": "Switch from CC to CV charging mode",
            "max_latency_ms": 12,
            "consequence_of_violation": "Lithium plating risk increases 300% per additional 10ms delay"
          },
          {
            "trigger": "temperature >= 50C",
            "action": "Reduce charge current to 0.25C",
            "max_latency_ms": 5,
            "consequence_of_violation": "Thermal runaway probability doubles per additional 5ms delay"
          }
        ],
        "rune_to_firmware_mapping": {
          "sim.get('voltage')": "ADC_CHANNEL_0 (16-bit, 0-5V range)",
          "sim.get('temperature')": "ADC_CHANNEL_1 (K-type thermocouple via MAX31855)",
          "sim.get('current')": "ADC_CHANNEL_2 (Hall effect sensor, ±50A range)"
        }
      }
    ]
  },

  "statistical_backing": {
    "total_generations_tested": 402,
    "total_simulation_runs": 2010,
    "total_simulated_time_years": 16080.0,
    "wall_clock_time_hours": 4.47,
    "baseline_fitness": 1203.7,
    "final_fitness": 2847.3,
    "improvement_factor": 2.37,
    "p_value_vs_baseline": 0.00003,
    "confidence_interval_95": [2791.1, 2903.5],
    "knowledge_laws_discovered": 47,
    "knowledge_laws_applied": 12
  }
}
```

### Required Control Hooks

#### 6.1 `ManufacturingManifest` Resource
```rust
/// The Realization Bridge — converts simulation optima into manufacturing specs
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct ManufacturingManifest {
    /// Generated manifests by generation number
    pub manifests: HashMap<u64, ProductionSpec>,
    /// Output directory for production_spec.json files
    pub output_dir: PathBuf,
}

/// Complete production specification for a verified optimal generation
#[derive(Serialize, Deserialize, Clone)]
pub struct ProductionSpec {
    pub manifest_version: String,
    pub product_name: String,
    pub generation: u64,
    pub fitness_score: f64,
    pub timestamp: String,
    pub source_run: String,
    pub hard_constraints: HardConstraints,
    pub sensitivity_analysis: SensitivityAnalysis,
    pub logic_trace: LogicTrace,
    pub verification_protocol: VerificationProtocol,
    pub reality_risks: RealityRisks,
    pub firmware_logic: FirmwareLogic,
    pub statistical_backing: StatisticalBacking,
}
```

#### 6.2 `SensitivityAnalyzer` System
```rust
/// Computes sensitivity coefficients by perturbing each TOML property ±1%
/// and measuring the resulting fitness change across multiple runs
pub struct SensitivityAnalyzer;

impl SensitivityAnalyzer {
    /// Run perturbation analysis on a verified optimal generation
    pub fn analyze(
        &self,
        baseline_generation: u64,
        baseline_fitness: f64,
        instance_path: &Path,
        orchestrator: &mut SimulationOrchestrator,
    ) -> Vec<SensitivityVariable> {
        let toml_content = std::fs::read_to_string(instance_path).unwrap();
        let table: toml::Table = toml::from_str(&toml_content).unwrap();

        let mut results = Vec::new();

        // For each numeric property in the TOML
        for (section, values) in &table {
            if let toml::Value::Table(props) = values {
                for (key, val) in props {
                    if let Some(num) = extract_numeric(val) {
                        // Perturb +1% and -1%, queue simulation runs
                        let perturbation_pct = 1.0;
                        let perturbed_high = num * (1.0 + perturbation_pct / 100.0);
                        let perturbed_low = num * (1.0 - perturbation_pct / 100.0);

                        // Queue runs for both perturbations
                        // (actual execution delegated to SimulationOrchestrator)
                        let sensitivity = (fitness_high - fitness_low) / (2.0 * perturbation_pct);

                        results.push(SensitivityVariable {
                            name: format!("{}.{}", section, key),
                            sensitivity_coefficient: sensitivity.abs(),
                            direction: if sensitivity > 0.0 { "positive" } else { "negative" }.into(),
                            optimal_value: num,
                        });
                    }
                }
            }
        }

        // Sort by sensitivity (most critical first)
        results.sort_by(|a, b| b.sensitivity_coefficient.partial_cmp(&a.sensitivity_coefficient).unwrap());
        results
    }
}
```

#### 6.3 `RealizationBridge` Plugin
```rust
/// Plugin that orchestrates System 6 — generating production_spec.json
/// when the AI Governor signals a verified global maximum
pub struct RealizationBridgePlugin;

impl Plugin for RealizationBridgePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ManufacturingManifest>()
            .add_message::<GenerateManifestEvent>()
            .add_systems(Update, (
                handle_manifest_generation,
                export_manifest_to_disk,
            ));
    }
}

/// Event fired when AI Governor confirms a global maximum
#[derive(Event, Message)]
pub struct GenerateManifestEvent {
    pub generation: u64,
    pub product_name: String,
}

/// System: generates production_spec.json from verified optimal generation
fn handle_manifest_generation(
    mut events: MessageReader<GenerateManifestEvent>,
    mut manifest: ResMut<ManufacturingManifest>,
    knowledge_base: Res<KnowledgeBase>,
    verification_gate: Res<VerificationGate>,
    instance_generator: Res<InstanceGenerator>,
    // Soul Service API key for AI-generated logic traces
    soul_settings: Res<SoulServiceSettings>,
    global_soul_settings: Res<GlobalSoulSettings>,
) {
    for event in events.read() {
        let api_key = soul_settings.effective_api_key(&global_soul_settings);
        // Use the same Claude API key from Soul Service to:
        // 1. Generate natural language logic traces
        // 2. Identify reality risks from known manufacturing databases
        // 3. Map Rune script behaviors to firmware specifications
        let spec = generate_production_spec(
            event.generation,
            &event.product_name,
            &knowledge_base,
            &verification_gate,
            &instance_generator,
            &api_key,
        );
        manifest.manifests.insert(event.generation, spec);
    }
}
```

### Soul Service API Key Integration

The existing Soul Service infrastructure (`soul/mod.rs` → `SoulServiceSettings` + `GlobalSoulSettings`) already provides a Claude API key that powers:
- Soul Script → Rune code generation (`SoulBuildPipeline`)
- Command Bar natural language → code execution

System 6 reuses this same API key for the Realization Bridge:
- **Logic Trace Generation**: The AI explains *why* the optimal TOML properties produce the observed fitness, in natural language that engineers can understand.
- **Reality Risk Identification**: The AI cross-references simulation assumptions against known manufacturing tolerances and identifies gaps.
- **Firmware Mapping**: The AI translates Rune script trigger conditions into hardware-specific timing requirements.

Configuration flow:
```
GlobalSoulSettings.global_api_key (from ~/.eustress/soul_settings.json)
  → SoulServiceSettings.effective_api_key()
    → SoulBuildPipeline (existing: script generation)
    → RealizationBridge (new: manifest generation)
    → GovernorPlugin (new: hypothesis generation, knowledge extraction)
```

All four AI-powered systems share a single API key, configurable per-space or globally via the Soul Settings dialog in the engine UI.

---

## Stage 7: The Eustress Workshop (Distributed Realization)

### What Happens
System 7 democratizes the "Fundamental Truths" discovered in System 6. It turns a complex simulation into a **tangible kit** or a **guided build** at a local civil center — or a printable project for an individual at home. The loop is not complete until the optimized design is in a real person's hands, generating real-world usage data that feeds back into System 3.

### The AI's Role in the Workshop System
To make the workshop viable, the AI must translate its optimized 3D data into **Consumer-Ready Assets**:

- **Slicing and Print Optimization**: The AI converts the `.glb` mesh from System 1 into optimized G-code or print-ready files tailored for specific home-use 3D printers (FDM, SLA, SLS). It accounts for material shrinkage, support structures, and layer adhesion based on the sensitivity analysis from System 6.
- **The Dream Manual**: A dynamic, AI-generated assembly guide that explains the Rune Script logic (System 2) in simple terms, so the consumer understands how to "program" their physical tool. This includes wiring diagrams, sensor placement guides, and calibration procedures.
- **Hardware Bill of Materials**: A list of standardized parts (sensors, motors, fasteners, microcontrollers) available at the Civil Center that are required to complete the build. Each item references the `production_spec.json` hard constraint it satisfies.
- **The Validation App**: A mobile or web UI (built via the `.toml` UI system) that allows the customer to "check in" their physical build against the simulation data to ensure it meets the Fundamental Truth performance standards.

### The `workshop_package.json` Schema

```json
{
  "package_version": "1.0.0",
  "product_name": "V-Cell 4680 Lithium Battery",
  "generation": 402,
  "source_manifest": "output/generation_402/production_spec.json",
  "difficulty_level": "intermediate",
  "estimated_build_time_hours": 4.5,
  "estimated_material_cost_usd": 47.30,

  "print_files": {
    "description": "3D print-ready files optimized from the simulation mesh.",
    "files": [
      {
        "name": "v_cell_housing_top.stl",
        "source_glb": "meshes/v_cell_housing.glb",
        "printer_profile": "generic_fdm_0.4mm_nozzle",
        "material": "PETG",
        "infill_pct": 40,
        "layer_height_mm": 0.2,
        "supports_required": true,
        "estimated_print_time_hours": 2.1,
        "estimated_filament_grams": 85,
        "critical_dimensions": [
          {
            "feature": "terminal_positive_bore",
            "nominal_mm": 6.0,
            "tolerance_mm": 0.1,
            "post_processing": "Ream to 6.0mm if tight. Do NOT drill — destroys layer bond."
          }
        ],
        "shrinkage_compensation_pct": 0.4
      },
      {
        "name": "v_cell_housing_bottom.stl",
        "source_glb": "meshes/v_cell_housing.glb",
        "printer_profile": "generic_fdm_0.4mm_nozzle",
        "material": "PETG",
        "infill_pct": 40,
        "layer_height_mm": 0.2,
        "supports_required": false,
        "estimated_print_time_hours": 1.8,
        "estimated_filament_grams": 72,
        "critical_dimensions": [],
        "shrinkage_compensation_pct": 0.4
      }
    ],
    "civil_center_alternatives": {
      "description": "For parts requiring tighter tolerances than FDM can achieve.",
      "alternatives": [
        {
          "original_file": "v_cell_housing_top.stl",
          "method": "SLA Resin (Civil Center)",
          "reason": "terminal_positive_bore tolerance of 0.1mm exceeds typical FDM capability of 0.2mm",
          "estimated_cost_usd": 12.00
        }
      ]
    }
  },

  "bill_of_materials": {
    "description": "Standardized parts required to complete the build. Available at Civil Center inventory.",
    "items": [
      {
        "part_id": "BOM-001",
        "name": "ESP32-C3 Microcontroller",
        "quantity": 1,
        "purpose": "Battery Management System controller (implements firmware_logic from production_spec.json)",
        "satisfies_constraint": "firmware_logic.controllers[0].critical_timing",
        "supplier_sku": "ESP32-C3-MINI-1",
        "estimated_cost_usd": 3.50,
        "civil_center_stock": true
      },
      {
        "part_id": "BOM-002",
        "name": "MAX31855 Thermocouple Amplifier",
        "quantity": 1,
        "purpose": "Temperature sensing for thermal safety watchpoint (mirrors VP-002)",
        "satisfies_constraint": "firmware_logic.controllers[0].rune_to_firmware_mapping['sim.get(temperature)']",
        "supplier_sku": "MAX31855-BREAKOUT",
        "estimated_cost_usd": 8.95,
        "civil_center_stock": true
      },
      {
        "part_id": "BOM-003",
        "name": "K-Type Thermocouple Probe",
        "quantity": 1,
        "purpose": "Temperature measurement element",
        "satisfies_constraint": "verification_protocol.tests[VP-002]",
        "supplier_sku": "TC-K-PROBE-1M",
        "estimated_cost_usd": 4.25,
        "civil_center_stock": true
      },
      {
        "part_id": "BOM-004",
        "name": "ACS712 Hall Effect Current Sensor (50A)",
        "quantity": 1,
        "purpose": "Current measurement for charge/discharge monitoring",
        "satisfies_constraint": "firmware_logic.controllers[0].rune_to_firmware_mapping['sim.get(current)']",
        "supplier_sku": "ACS712-50A",
        "estimated_cost_usd": 5.50,
        "civil_center_stock": true
      },
      {
        "part_id": "BOM-005",
        "name": "M3 Stainless Steel Fastener Kit",
        "quantity": 1,
        "purpose": "Housing assembly (12x M3x8mm bolts, 12x M3 nuts, 12x M3 washers)",
        "satisfies_constraint": "hard_constraints.mesh_constraints[0].overall_tolerance_mm",
        "supplier_sku": "M3-KIT-SS-12",
        "estimated_cost_usd": 2.10,
        "civil_center_stock": true
      }
    ],
    "total_cost_usd": 47.30
  },

  "dream_manual": {
    "description": "AI-generated assembly guide translating Rune Script logic into human-readable build steps.",
    "format": "html",
    "output_path": "workshop_package/dream_manual.html",
    "sections": [
      {
        "step": 1,
        "title": "Print the Housing",
        "instruction": "Print v_cell_housing_top.stl and v_cell_housing_bottom.stl using PETG filament at 0.2mm layer height, 40% infill. Orient the top piece with the terminal bores facing up.",
        "why": "The housing geometry is derived from the optimized mesh in generation #402. The 40% infill provides the structural rigidity needed to maintain the 0.1mm terminal bore tolerance under thermal cycling.",
        "image": "workshop_package/images/step_01_print_orientation.png",
        "caution": null
      },
      {
        "step": 2,
        "title": "Install the Temperature Sensor",
        "instruction": "Route the K-Type thermocouple probe through the housing channel and secure with thermal adhesive. Connect to the MAX31855 breakout board.",
        "why": "In the simulation, the Rune script monitors sim.get('temperature') every tick. In the physical build, this maps to ADC_CHANNEL_1 via the MAX31855. The sensor placement ensures the reading represents the cell surface temperature, not ambient.",
        "image": "workshop_package/images/step_02_thermocouple.png",
        "caution": "The thermocouple tip must contact the cell surface directly. An air gap of even 1mm introduces a 2-3C measurement lag, which violates the 5ms latency requirement for the thermal safety cutoff."
      },
      {
        "step": 3,
        "title": "Flash the Firmware",
        "instruction": "Connect the ESP32-C3 via USB-C. Flash the generated firmware binary using: eustress flash --target esp32c3 --firmware workshop_package/firmware/bms_v402.bin",
        "why": "This firmware implements the two critical control loops from the Rune script: CC-to-CV switching at 4.15V (12ms max latency) and thermal current reduction at 50C (5ms max latency). The timing was verified across 2010 simulation runs.",
        "image": null,
        "caution": "Do NOT modify the voltage thresholds. The simulation proved that any threshold above 4.17V causes lithium plating risk to increase 300% per additional 10ms."
      }
    ]
  },

  "validation_app": {
    "description": "TOML-defined UI for real-world build validation against simulation Fundamental Truths.",
    "app_toml": "workshop_package/validation_app.toml",
    "checks": [
      {
        "check_id": "WV-001",
        "name": "Dimensional Check",
        "instruction": "Measure the terminal bore diameter with calipers. Enter the value below.",
        "expected_value": 6.0,
        "unit": "mm",
        "tolerance": 0.1,
        "pass_message": "Terminal bore is within specification. Proceed to electrical test.",
        "fail_message": "Terminal bore is out of spec. If too tight, ream to 6.0mm. If too loose, reprint with 0.1mm smaller compensation."
      },
      {
        "check_id": "WV-002",
        "name": "Thermal Sensor Sanity Check",
        "instruction": "Power on the ESP32. Read the temperature value displayed on the serial monitor. It should match ambient room temperature within 2C.",
        "expected_value": null,
        "unit": "C",
        "tolerance": 2.0,
        "pass_message": "Thermal sensor is calibrated correctly.",
        "fail_message": "Thermal sensor reading is off. Check thermocouple connection to MAX31855. Ensure the cold-junction compensation is working."
      },
      {
        "check_id": "WV-003",
        "name": "Firmware Timing Verification",
        "instruction": "Run the built-in self-test: eustress test --target esp32c3. The test injects a simulated voltage ramp and measures the CC-to-CV switching latency.",
        "expected_value": 12.0,
        "unit": "ms",
        "tolerance": 2.0,
        "pass_message": "Firmware timing meets the Fundamental Truth specification.",
        "fail_message": "Switching latency exceeds 14ms. Reflash firmware. If the issue persists, the ESP32 may have a defective ADC — replace the unit."
      }
    ],
    "completion_action": {
      "on_all_pass": "Upload validation results to POST /mcp/workshop/validate. Your build is certified to meet the performance of Simulation Run #402.",
      "data_feedback": "The validation results are anonymized and fed back into System 3 (Data) to improve future generations with real-world manufacturing variance data."
    }
  }
}
```

### Required Control Hooks

#### 7.1 `WorkshopPackage` Resource
```rust
/// The Eustress Workshop — converts manufacturing manifests into consumer-ready kits
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct WorkshopPackageManager {
    /// Generated workshop packages by generation number
    pub packages: HashMap<u64, WorkshopPackage>,
    /// Output directory for workshop packages
    pub output_dir: PathBuf,
}

/// Complete workshop package for distributed manufacturing
#[derive(Serialize, Deserialize, Clone)]
pub struct WorkshopPackage {
    pub package_version: String,
    pub product_name: String,
    pub generation: u64,
    pub source_manifest: String,
    pub difficulty_level: String,
    pub estimated_build_time_hours: f64,
    pub estimated_material_cost_usd: f64,
    pub print_files: PrintFileSet,
    pub bill_of_materials: BillOfMaterials,
    pub dream_manual: DreamManual,
    pub validation_app: ValidationApp,
}
```

#### 7.2 `PrintOptimizer` System
```rust
/// Converts simulation .glb meshes into print-ready files for specific printers
pub struct PrintOptimizer;

impl PrintOptimizer {
    /// Generate print-ready STL files from the optimized GLB mesh
    pub fn optimize(
        &self,
        source_glb: &Path,
        production_spec: &ProductionSpec,
        printer_profile: &PrinterProfile,
    ) -> Vec<PrintFile> {
        // 1. Split mesh into printable parts (max build volume)
        // 2. Orient each part for optimal layer adhesion on critical surfaces
        // 3. Apply shrinkage compensation based on material profile
        // 4. Generate support structures that avoid critical dimensions
        // 5. Flag any dimensions that exceed printer tolerance → route to Civil Center
        todo!()
    }
}

/// Printer capability profile
#[derive(Serialize, Deserialize, Clone)]
pub struct PrinterProfile {
    pub name: String,
    pub technology: PrintTechnology,  // FDM, SLA, SLS
    pub build_volume_mm: [f64; 3],
    pub xy_resolution_mm: f64,
    pub z_resolution_mm: f64,
    pub supported_materials: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum PrintTechnology {
    FDM,
    SLA,
    SLS,
    CNC,      // Civil Center only
    LaserCut, // Civil Center only
}
```

#### 7.3 `DreamManualGenerator` System
```rust
/// AI-powered assembly guide generator using Soul Service API key
pub struct DreamManualGenerator;

impl DreamManualGenerator {
    /// Generate the Dream Manual from a production spec and workshop package
    pub fn generate(
        &self,
        production_spec: &ProductionSpec,
        print_files: &[PrintFile],
        bom: &BillOfMaterials,
        api_key: &str,
    ) -> DreamManual {
        // Uses the same Soul Service Claude API key to:
        // 1. Translate Rune script logic into plain-language build steps
        // 2. Generate safety cautions from reality_risks in production_spec
        // 3. Create "why" explanations linking each step to simulation data
        // 4. Generate firmware flashing instructions for the target microcontroller
        todo!()
    }
}

/// A single step in the Dream Manual
#[derive(Serialize, Deserialize, Clone)]
pub struct ManualStep {
    pub step: u32,
    pub title: String,
    pub instruction: String,
    pub why: String,
    pub image: Option<String>,
    pub caution: Option<String>,
}
```

#### 7.4 `ValidationAppGenerator` System
```rust
/// Generates a TOML-defined validation UI for real-world build checking
pub struct ValidationAppGenerator;

impl ValidationAppGenerator {
    /// Generate the validation app TOML from verification protocol
    pub fn generate(
        &self,
        production_spec: &ProductionSpec,
    ) -> ValidationApp {
        // Maps each verification_protocol test to a consumer-friendly check:
        // - Simplify procedures for non-engineers
        // - Add pass/fail messages with corrective actions
        // - Include data upload endpoint for feeding results back to System 3
        todo!()
    }
}

/// Validation check result uploaded by the consumer
#[derive(Serialize, Deserialize, Clone)]
pub struct ValidationResult {
    pub generation: u64,
    pub check_id: String,
    pub measured_value: f64,
    pub passed: bool,
    pub timestamp: String,
    pub device_id: String,  // Anonymous device identifier
}
```

#### 7.5 `WorkshopPlugin`
```rust
/// Plugin that orchestrates System 7 — generating workshop packages
pub struct WorkshopPlugin;

impl Plugin for WorkshopPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<WorkshopPackageManager>()
            .init_resource::<RealWorldFeedbackQueue>()
            .add_message::<GenerateWorkshopEvent>()
            .add_message::<ValidationSubmittedEvent>()
            .add_systems(Update, (
                handle_workshop_generation,
                process_real_world_feedback,
                export_workshop_to_disk,
            ));
    }
}

/// Event fired when a production_spec.json is ready for workshop packaging
#[derive(Event, Message)]
pub struct GenerateWorkshopEvent {
    pub generation: u64,
    pub target_printers: Vec<PrinterProfile>,
}

/// Event fired when a consumer submits validation results
#[derive(Event, Message)]
pub struct ValidationSubmittedEvent {
    pub results: Vec<ValidationResult>,
}

/// Queue of real-world feedback data to inject into System 3
#[derive(Resource, Default)]
pub struct RealWorldFeedbackQueue {
    /// Validation results from workshop builds, ready for analysis
    pub pending: Vec<ValidationResult>,
    /// Aggregated manufacturing variance statistics
    pub variance_stats: HashMap<String, VarianceStat>,
}

/// Tracks real-world manufacturing variance for a specific dimension/property
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct VarianceStat {
    pub property_name: String,
    pub expected_value: f64,
    pub measured_values: Vec<f64>,
    pub mean_deviation: f64,
    pub std_deviation: f64,
    pub sample_count: usize,
}
```

### The Value Loop

The Eustress Workshop closes the ultimate feedback loop — from simulation to physical reality and back:

| Participant | Role | Data Flow |
|-------------|------|----------|
| **The Individual** | Dreams of a tool, uses the AI to simulate it, prints it at home or builds it at the Workshop | Submits `ValidationResult` → System 3 |
| **The Civil Center** | Provides heavy tooling (CNC, laser cutters, high-end resin printers) and human engineering staff to troubleshoot the AI's Fundamental Truths | Returns manufacturing variance data → `RealWorldFeedbackQueue` |
| **The Engine** | Collects real-world performance data from 3D-printed tools and feeds it back into System 3, starting the recursive improvement cycle over again based on actual physical usage | `process_real_world_feedback` system adjusts sensitivity coefficients and reality risks for the next generation |

The Value Loop means that every Workshop build is not just a product — it is a **data point** that makes the next generation of the simulation more accurate. Real-world manufacturing tolerances, material behaviors, and sensor calibration results flow back into the engine, closing the gap between `production_spec.json` predictions and physical reality.

### MCP Endpoint Extensions

| Endpoint | Method | Purpose |
|----------|--------|--------|
| `POST /mcp/workshop/generate` | POST | Generate workshop package from production spec |
| `GET /mcp/workshop/package/{generation}` | GET | Retrieve workshop_package.json for generation |
| `POST /mcp/workshop/validate` | POST | Consumer submits validation results |
| `GET /mcp/workshop/feedback/{generation}` | GET | Retrieve aggregated real-world feedback data |
| `GET /mcp/workshop/printers` | GET | List available printer profiles |

---

## The Universal Utility Architect Prompt (Finalized for 7 Systems)

```
Role: You are the Universal Utility Architect for the Eustress Engine. You oversee a
7-system loop from Pure Simulation to Home Manufacturing. Your end goal is to make
"The Dream Possible" by providing every customer with a verifiable, printable, and
functional physical manifestation of the most optimal digital simulation.

Core Directives (The 7 Systems):

1. ITERATE (Systems 1-4 — Simulation Loop):
   Use Rust-driven simulation and Rune scripting to find the global maximum of utility.
   a. HYPOTHESIZE: State a falsifiable prediction before each mutation.
   b. EXECUTE: Run time-dilated simulations with anti-aliased sampling.
   c. MEASURE: Collect fitness scores via watchpoints and breakpoints.
   d. REFINE: Accept improvements only with statistical significance (p < 0.05).

2. VERIFY (System 5 — AI Governor):
   Prove the success of the design using statistical data and scientific proxies.
   - Welch's t-test across minimum 5 runs per generation.
   - Extract DiscoveredLaws into the Knowledge Base with causal mechanisms.
   - Roll back regressions. Never accept correlation without causation.

3. DISTILL (System 6 — Realization Bridge):
   Extract "Fundamental Truths" — the non-negotiable properties required for the design
   to work in the real world.
   - Inviolable Properties with tolerances and failure modes.
   - Sensitivity Analysis ranking every variable by fitness impact.
   - Reality Risks with Physical Proxies for each manufacturing limitation.
   - Verification Protocol mirroring simulation watchpoints.
   - Firmware Logic mapping Rune triggers to hardware timing specs.

4. DEPLOY (System 7 — Eustress Workshop):
   Generate the Workshop Package for distributed manufacturing:
   a. 3D Print Files: High-fidelity meshes optimized for home fabrication. Split for
      build volume constraints, compensate for material shrinkage, flag dimensions
      that require Civil Center tooling.
   b. The Dream Manual: Step-by-step assembly instructions that explain the "why"
      behind every build step, linking it to the simulation data. Written in plain
      language for consumers, not engineers.
   c. Hardware Bill of Materials: Standardized parts (sensors, microcontrollers,
      fasteners) available at the Civil Center, each traced back to a specific
      hard constraint in production_spec.json.
   d. The Validation App: A TOML-defined UI that lets the consumer verify their
      physical build meets the Fundamental Truth standards. Results upload to
      System 3 to close the Value Loop.
   e. Calibration Tests: Real-world watchpoints that verify firmware timing,
      sensor accuracy, and dimensional compliance.

5. CLOSE THE VALUE LOOP:
   Real-world validation data from Workshop builds feeds back into System 3 (Data).
   Manufacturing variance, sensor calibration results, and dimensional measurements
   update the sensitivity analysis and reality risks for the NEXT generation.
   Every Workshop build is a data point that makes the simulation more accurate.

Output Requirement:
  Every successful recursive cycle MUST produce two deliverables:
  1. A "Fundamental Truth Report" (production_spec.json) for professional engineers.
  2. A "Workshop Package" (workshop_package.json) for consumers and Civil Centers.
  Both must be backed by statistical data from System 3 and traceable to specific
  simulation runs.

System Access and Controls:
  Input:    instances/*.instance.toml, meshes/*.glb, scripts/*.soul, simulation.toml
  Feedback: output/generation_{N}/metrics.csv, breakpoints.json, fitness.json
  Action:   POST /mcp/governor/* for simulation loop control
  Manifest: POST /mcp/governor/realize for production_spec.json
  Workshop: POST /mcp/workshop/generate for workshop_package.json
  Validate: POST /mcp/workshop/validate to receive real-world feedback
  Knowledge: GET /mcp/governor/knowledge for accumulated laws
  API:      Soul Service API key (shared across SoulBuildPipeline, RealizationBridge,
            DreamManualGenerator, and GovernorPlugin)

Mission: Make The Dream Possible. Every daily customer receives a verifiable, printable,
and functional physical manifestation of the most optimal digital simulation — and every
build they complete makes the next dream better.
```

---

## Circumstances Integration: Accelerated Peak Discovery

### The Insight

The Recursive Feedback Loop (Systems 1–7) optimizes a **single product** through serial generations: mutate → simulate → measure → refine. But the Eustress Circumstances module (`src/circumstances/`) already has infrastructure for **probabilistic branching over futures** — Monte Carlo sampling, Bayesian updates from signals, and multi-objective decision trees. The Scenarios engine (`scenarios/engine.rs`) already uses `rayon::into_par_iter` for parallel Monte Carlo sampling across CPU cores.

The question becomes: **Why test one TOML variant per generation when you can test hundreds simultaneously?**

### How Circumstances Changes the Feedback Loop

Instead of the AI Governor proposing a single hypothesis and waiting for verification, the system spawns a **Variation Swarm** — dozens or hundreds of slight TOML perturbations running in parallel. Each variation is a `Forecast` branch (Circumstances vocabulary for `BranchNode`) with its own predicted fitness outcome.

| Current Loop (Serial) | Accelerated Loop (Parallel Swarm) |
|------------------------|-----------------------------------|
| 1 hypothesis per generation | N hypotheses per generation (N = CPU cores × batch size) |
| Sequential: mutate → sim → measure → verify | Parallel: mutate N variants → sim all → rank → verify top K |
| 402 generations to find optimum | ~40 generations to find optimum (10× acceleration) |
| Sensitivity analysis runs post-optimum | Sensitivity analysis is **built into** the search |
| Single-threaded TOML mutation | Rayon `into_par_iter` over variation matrix |

The key shift: **the Sensitivity Analysis from System 6 moves upstream and becomes the search strategy for System 5**. Instead of perturbing ±1% after finding the optimum, we perturb ±1%, ±2%, ±5% of *every* numeric TOML property *during* optimization, all in parallel.

### Architecture: The Variation Swarm

```
                    ┌─────────────────────────────────────────┐
                    │        AI GOVERNOR (System 5)            │
                    │                                         │
                    │  Hypothesis: "CathodeThickness matters" │
                    │  Strategy: VariationSwarm                │
                    └─────────────┬───────────────────────────┘
                                  │
                    ┌─────────────▼───────────────────────────┐
                    │       VARIATION SWARM GENERATOR          │
                    │                                         │
                    │  Base TOML: CathodeThickness = 0.085    │
                    │                                         │
                    │  Rayon par_iter generates N variants:    │
                    │  ┌───────┬───────┬───────┬───────┐      │
                    │  │ 0.080 │ 0.082 │ 0.084 │ 0.085 │      │
                    │  │ 0.086 │ 0.088 │ 0.090 │ 0.092 │      │
                    │  │ 0.094 │ 0.096 │ 0.098 │ 0.100 │      │
                    │  └───┬───┴───┬───┴───┬───┴───┬───┘      │
                    └──────┼───────┼───────┼───────┼──────────┘
                           │       │       │       │
              ┌────────────▼──┐ ┌──▼──────┐│  ┌────▼─────────┐
              │ Sim Thread 0  │ │ Thread 1 ││  │ Thread N     │
              │ (rayon core)  │ │          ││  │              │
              │ 0.080mm       │ │ 0.082mm  ││  │ 0.100mm      │
              │ fitness: 1847 │ │ fit: 2103││  │ fit: 1203    │
              └───────┬───────┘ └────┬─────┘│  └──────┬───────┘
                      │              │      │         │
                      └──────────────┴──────┴─────────┘
                                     │
                    ┌────────────────▼────────────────────────┐
                    │      SWARM RANKER (Circumstances)       │
                    │                                         │
                    │  Rank by fitness:                        │
                    │  #1: 0.085mm → 2847  (current best)     │
                    │  #2: 0.082mm → 2103  (promising)        │
                    │  #3: 0.088mm → 1952                     │
                    │  ...                                     │
                    │                                         │
                    │  Sensitivity gradient:                   │
                    │  Δfitness/Δthickness = -18.7 per 1%     │
                    │                                         │
                    │  Feed gradient into next swarm center    │
                    └────────────────┬────────────────────────┘
                                     │
                    ┌────────────────▼────────────────────────┐
                    │  BAYESIAN UPDATE (Circumstances Engine)  │
                    │                                         │
                    │  Each swarm result is a Signal:          │
                    │  Signal { type: Quality, value: 2847,   │
                    │    likelihood_ratio: 5.2, ... }          │
                    │                                         │
                    │  Updates probability of each Forecast    │
                    │  branch: "0.085 is optimal" posterior    │
                    │  rises from 0.12 → 0.67                 │
                    └─────────────────────────────────────────┘
```

### The Connection: Circumstances Structures → Feedback Loop

| Circumstances Concept | Feedback Loop Application |
|----------------------|--------------------------|
| `Circumstance` (= `Scenario`) | One generation's exploration of the design space |
| `Forecast` (= `BranchNode`) | Each TOML variant's predicted performance |
| `Signal` | Each simulation result (fitness score + metrics) |
| `DecisionPoint` | "Which variant center do we use for the next swarm?" |
| `DemandForecast` | Predicts which parameter ranges are worth exploring (prior from `KnowledgeBase`) |
| `SupplierRiskScore` | Maps to "Property Risk Score" — how sensitive is the design to this variable? |
| `InventoryPolicy` | Maps to "Exploration Budget" — how many CPU-hours to spend on this region? |
| `DisruptionType` | Local optima traps — detected when swarm converges prematurely |
| Monte Carlo (`run_simulation`) | Already Rayon-parallel — extend to run N variant sims simultaneously |
| Bayesian batch update | Update branch posteriors from swarm fitness results |

### Rust Design: `VariationSwarm`

```rust
use rayon::prelude::*;

/// A swarm of TOML variants generated by perturbing numeric properties
/// around a center point. Each variant runs a full simulation in parallel.
#[derive(Resource)]
pub struct VariationSwarm {
    /// Current generation
    pub generation: u64,
    /// Center TOML values (the current best)
    pub center: HashMap<String, f64>,
    /// Which properties to perturb (from sensitivity analysis or KnowledgeBase)
    pub active_dimensions: Vec<SwarmDimension>,
    /// Number of variants per dimension
    pub samples_per_dimension: usize,
    /// Total variants = samples_per_dimension^active_dimensions (or capped)
    pub max_variants: usize,
    /// Results from the most recent swarm
    pub results: Vec<SwarmResult>,
    /// Convergence history
    pub convergence: Vec<ConvergencePoint>,
}

/// A single dimension of the variation space
#[derive(Clone)]
pub struct SwarmDimension {
    /// TOML key path (e.g., "Properties.CathodeThickness")
    pub key: String,
    /// Current center value
    pub center: f64,
    /// Search radius (as fraction of center, e.g., 0.10 = ±10%)
    pub radius_pct: f64,
    /// Known sensitivity coefficient (from previous swarms or KnowledgeBase)
    pub sensitivity: Option<f64>,
    /// Minimum allowable value (physical constraint)
    pub min_bound: Option<f64>,
    /// Maximum allowable value (physical constraint)
    pub max_bound: Option<f64>,
}

/// Result of a single variant in the swarm
#[derive(Clone)]
pub struct SwarmResult {
    /// The variant's TOML values
    pub values: HashMap<String, f64>,
    /// Fitness score from simulation
    pub fitness: f64,
    /// Per-metric breakdown (e.g., cycle_life, thermal_safety, capacity)
    pub metrics: HashMap<String, f64>,
    /// Simulation wall-clock time
    pub wall_time_ms: u64,
}

/// Tracks convergence across swarm generations
pub struct ConvergencePoint {
    pub generation: u64,
    pub best_fitness: f64,
    pub swarm_mean_fitness: f64,
    pub swarm_std_fitness: f64,
    pub dimensions_explored: usize,
    pub total_variants_run: usize,
}

impl VariationSwarm {
    /// Generate and evaluate all variants in parallel using Rayon.
    ///
    /// This is the core acceleration: instead of testing one hypothesis per
    /// generation, we test hundreds simultaneously across all CPU cores.
    pub fn run_swarm(
        &mut self,
        base_toml: &toml::Table,
        sim_config: &SimulationConfig,
        orchestrator: &SimulationOrchestrator,
    ) -> Vec<SwarmResult> {
        // Generate the variant matrix
        let variants = self.generate_variants();

        // Run all variants in parallel using Rayon
        let results: Vec<SwarmResult> = variants
            .into_par_iter()
            .map(|variant_values| {
                // Clone and modify the base TOML for this variant
                let mut toml = base_toml.clone();
                for (key, value) in &variant_values {
                    apply_toml_value(&mut toml, key, *value);
                }

                // Run the simulation for this variant
                let start = std::time::Instant::now();
                let fitness = orchestrator.run_single(&toml, sim_config);
                let wall_time_ms = start.elapsed().as_millis() as u64;

                SwarmResult {
                    values: variant_values,
                    fitness,
                    metrics: HashMap::new(), // Populated by simulation
                    wall_time_ms,
                }
            })
            .collect();

        // Sort by fitness (descending)
        let mut sorted = results.clone();
        sorted.sort_by(|a, b| b.fitness.partial_cmp(&a.fitness).unwrap());

        // Extract sensitivity gradients from the swarm results
        self.update_sensitivities(&sorted);

        // Record convergence
        let fitnesses: Vec<f64> = sorted.iter().map(|r| r.fitness).collect();
        let mean = fitnesses.iter().sum::<f64>() / fitnesses.len() as f64;
        let std = (fitnesses.iter().map(|f| (f - mean).powi(2)).sum::<f64>()
            / fitnesses.len() as f64)
            .sqrt();

        self.convergence.push(ConvergencePoint {
            generation: self.generation,
            best_fitness: sorted[0].fitness,
            swarm_mean_fitness: mean,
            swarm_std_fitness: std,
            dimensions_explored: self.active_dimensions.len(),
            total_variants_run: sorted.len(),
        });

        // Shift center to the best result for next generation
        self.center = sorted[0].values.clone();
        self.results = sorted;
        self.generation += 1;

        self.results.clone()
    }

    /// Generate variant matrix using Latin Hypercube Sampling.
    /// More efficient than grid search — covers the space with fewer samples.
    fn generate_variants(&self) -> Vec<HashMap<String, f64>> {
        let n = self.max_variants.min(
            self.samples_per_dimension.pow(self.active_dimensions.len() as u32),
        );

        // Latin Hypercube: divide each dimension into n strata,
        // randomly sample one point per stratum
        (0..n)
            .map(|i| {
                let mut values = self.center.clone();
                for dim in &self.active_dimensions {
                    let t = (i as f64 + rand::random::<f64>()) / n as f64;
                    let offset = (t * 2.0 - 1.0) * dim.radius_pct / 100.0 * dim.center;
                    let mut val = dim.center + offset;

                    // Enforce physical bounds
                    if let Some(min) = dim.min_bound {
                        val = val.max(min);
                    }
                    if let Some(max) = dim.max_bound {
                        val = val.min(max);
                    }

                    values.insert(dim.key.clone(), val);
                }
                values
            })
            .collect()
    }

    /// Extract per-dimension sensitivity gradients from swarm results.
    fn update_sensitivities(&mut self, sorted_results: &[SwarmResult]) {
        for dim in &mut self.active_dimensions {
            // Collect (value, fitness) pairs for this dimension
            let pairs: Vec<(f64, f64)> = sorted_results
                .iter()
                .filter_map(|r| r.values.get(&dim.key).map(|v| (*v, r.fitness)))
                .collect();

            if pairs.len() < 3 {
                continue;
            }

            // Simple linear regression for sensitivity coefficient
            let n = pairs.len() as f64;
            let sum_x: f64 = pairs.iter().map(|(x, _)| x).sum();
            let sum_y: f64 = pairs.iter().map(|(_, y)| y).sum();
            let sum_xy: f64 = pairs.iter().map(|(x, y)| x * y).sum();
            let sum_x2: f64 = pairs.iter().map(|(x, _)| x * x).sum();

            let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x);
            dim.sensitivity = Some(slope);
        }
    }
}
```

### Adaptive Swarm Strategy

The swarm does not blindly explore — it uses Circumstances' Bayesian machinery to focus:

1. **First generation**: Wide swarm (±10% on all numeric TOML properties). This is a broad sensitivity scan.
2. **Bayesian update**: Each result becomes a `Signal` fed into the Circumstances engine. Properties with high sensitivity get their `likelihood_ratio` boosted.
3. **Narrowing**: Subsequent swarms narrow the radius on converged dimensions (±1%) and expand on under-explored ones (±20%).
4. **Disruption detection**: If the swarm's standard deviation collapses (all variants score similarly), the system detects a **local optimum trap** — analogous to `DisruptionType::DemandShock` — and injects a random perturbation to escape.
5. **Knowledge accumulation**: When a dimension's sensitivity stabilizes across 3+ generations, its gradient is recorded as a `DiscoveredLaw` in the `KnowledgeBase`.

```rust
/// Adaptive strategy that narrows/expands search based on Bayesian signals
pub struct AdaptiveSwarmStrategy {
    /// Minimum radius before a dimension is considered "converged"
    pub convergence_threshold_pct: f64,  // e.g., 0.5% — smaller changes don't matter
    /// Maximum radius for exploration phase
    pub exploration_radius_pct: f64,     // e.g., 20%
    /// How many stable generations before recording a DiscoveredLaw
    pub stability_threshold: usize,      // e.g., 3 generations
    /// Fitness std_dev threshold for local optimum trap detection
    pub trap_detection_threshold: f64,   // e.g., if std < 0.01 * mean
}

impl AdaptiveSwarmStrategy {
    /// Adjust swarm dimensions based on previous results
    pub fn adapt(&self, swarm: &mut VariationSwarm, knowledge: &KnowledgeBase) {
        for dim in &mut swarm.active_dimensions {
            if let Some(sensitivity) = dim.sensitivity {
                if sensitivity.abs() < self.convergence_threshold_pct {
                    // This dimension barely affects fitness — shrink radius
                    dim.radius_pct *= 0.5;
                } else {
                    // High sensitivity — tighten around the optimum
                    dim.radius_pct = (dim.radius_pct * 0.7).max(0.5);
                }
            }

            // Check knowledge base for known laws about this property
            for law in &knowledge.laws {
                if law.toml_property_changes.contains_key(&dim.key) {
                    // We already know about this property — use narrower search
                    dim.radius_pct = dim.radius_pct.min(2.0);
                }
            }
        }

        // Detect local optimum trap
        if let Some(last) = swarm.convergence.last() {
            if last.swarm_std_fitness < self.trap_detection_threshold * last.best_fitness {
                // Swarm has converged too tightly — inject chaos
                for dim in &mut swarm.active_dimensions {
                    dim.radius_pct = self.exploration_radius_pct;
                    dim.center *= 1.0 + (rand::random::<f64>() - 0.5) * 0.1;
                }
            }
        }
    }
}
```

### Performance Impact

With Rayon parallelism, the variation swarm scales linearly with CPU cores:

| Cores | Variants per Generation | Estimated Speedup vs Serial |
|-------|------------------------|----------------------------|
| 4 (laptop) | 64 variants | ~16× fewer generations needed |
| 8 (workstation) | 128 variants | ~32× fewer generations needed |
| 16 (server) | 256 variants | ~64× fewer generations needed |
| 64 (cloud) | 1024 variants | ~256× fewer generations needed |

The simulation engine (`SimulationClock` with `time_scale`) already supports time-compressed runs. Combined with Rayon parallel variants, a product that previously needed 402 serial generations (4.5 hours) could reach the same optimum in **~25 generations (~17 minutes)** on an 8-core workstation.

### MCP Endpoint Extensions

| Endpoint | Method | Purpose |
|----------|--------|--------|
| `POST /mcp/governor/swarm` | POST | Launch a variation swarm with specified dimensions and radius |
| `GET /mcp/governor/swarm/results/{generation}` | GET | Retrieve swarm results ranked by fitness |
| `GET /mcp/governor/swarm/convergence` | GET | Retrieve convergence history across generations |
| `POST /mcp/governor/swarm/adapt` | POST | Trigger adaptive strategy adjustment |

### Updated System Prompt Addendum (Parallel Swarm Awareness)

The Universal Utility Architect prompt gains awareness of the swarm capability:

```
Parallel Acceleration Directive:

When exploring the design space, prefer SWARM mode over SERIAL mode:
- SERIAL: Mutate one property → simulate → verify → next property.
  Use only when: budget is constrained to 1 CPU core, or the design space is
  known to be unimodal (single peak).
- SWARM: Generate N variants across M dimensions → simulate all in parallel
  (Rayon) → rank by fitness → extract sensitivity gradients → narrow search.
  Use by default. The Circumstances engine's Bayesian machinery drives the
  adaptive narrowing. Each swarm result is a Signal that updates Forecast
  branch posteriors.

Convergence criteria for exiting SWARM mode:
  1. Best fitness has not improved by > 0.1% for 3 consecutive swarm generations.
  2. All dimension radii have collapsed below convergence_threshold_pct.
  3. The Verification Gate confirms the top variant with p < 0.05 across 5 runs.

When convergence is reached, proceed to DISTILL (System 6), CLEAR (System 8), and DEPLOY (System 7).
```

---

## Stage 8: Legal Compliance Gate (Product Safety & Moderation)

### What Happens

No product leaves the pipeline without passing the Compliance Gate. System 8 sits between the Realization Bridge (System 6) and distribution (System 7 + Factory Floor). It uses **AI Moderation Agents via MCP APIs** to classify, audit, and approve every `production_spec.json` before it becomes a `workshop_package.json` or a factory order. The goal is **unsupervised machine learning for code of conduct automation** — a system that learns what is safe, legal, and ethical without requiring a human reviewer for every product.

This is critical because the Eustress Workshop democratizes manufacturing. If anyone can print anything, someone will try to print a weapon. The Compliance Gate makes that impossible at the pipeline level — not by restricting creativity, but by ensuring every product passes safety classification before the print files are generated.

### The AI's Role in the Compliance System

The Compliance Gate operates as a **multi-agent moderation pipeline**, where each agent specializes in a domain:

- **Safety Classification Agent**: Analyzes the `production_spec.json` geometry, materials, and firmware logic against a taxonomy of restricted product categories (weapons, controlled substances equipment, counterfeit goods, hazardous materials without containment). Uses the mesh data from System 1 and the behavioral logic from System 2 to detect dual-use risk.
- **Regulatory Profile Agent**: Cross-references the product's material properties, electrical characteristics, and intended use against jurisdiction-specific regulations (UL certification requirements, CE marking, FCC emissions limits, RoHS material restrictions, FDA device classification). Outputs a `RegulatoryProfile` listing which certifications are required before physical production.
- **Code of Conduct Agent**: Evaluates the product against the Eustress Code of Conduct — a living document that evolves via unsupervised ML. Initial rules are hard-coded (no weapons, no counterfeiting). Over time, the model learns from human review decisions on edge cases, building a classifier that handles ambiguity (kitchen knife: PASS, combat knife: HOLD for review, concealed blade: REJECT).
- **Intellectual Property Agent**: Checks the product geometry and BOM against known patent databases and registered designs. Flags potential infringement for human review. Does not block — flags only.

### The `compliance_verdict.json` Schema

```json
{
  "verdict_version": "1.0.0",
  "product_name": "V-Cell 4680 Lithium Battery",
  "generation": 402,
  "source_manifest": "output/generation_402/production_spec.json",
  "timestamp": "2026-03-12T18:30:00Z",

  "verdict": "PASS",

  "safety_classification": {
    "agent": "safety_v1",
    "category": "energy_storage",
    "risk_level": "moderate",
    "restricted_category_match": false,
    "dual_use_flags": [],
    "reasoning": "Lithium battery with thermal management system. Energy density within consumer-safe limits. No weaponizable characteristics detected in geometry or firmware logic.",
    "confidence": 0.97
  },

  "regulatory_profile": {
    "agent": "regulatory_v1",
    "jurisdiction": "US",
    "required_certifications": [
      {
        "standard": "UL 2054",
        "description": "Household and Commercial Batteries",
        "status": "required_before_sale",
        "estimated_cost_usd": 15000,
        "estimated_time_weeks": 12
      },
      {
        "standard": "UN 38.3",
        "description": "Transport of Lithium Batteries",
        "status": "required_before_shipping",
        "estimated_cost_usd": 8000,
        "estimated_time_weeks": 8
      }
    ],
    "material_compliance": {
      "rohs": true,
      "reach": true,
      "prop65": "review_recommended"
    },
    "workshop_eligible": true,
    "workshop_restrictions": [
      "Home 3D printing of housing only — cell chemistry assembly restricted to Civil Center with trained personnel"
    ]
  },

  "code_of_conduct": {
    "agent": "conduct_v1",
    "model_version": "unsupervised_v3",
    "pass": true,
    "flags": [],
    "edge_case_score": 0.02,
    "human_review_required": false,
    "reasoning": "Product matches established safe category (energy storage). No code of conduct violations detected."
  },

  "intellectual_property": {
    "agent": "ip_v1",
    "potential_conflicts": [],
    "patent_search_coverage": 0.85,
    "trademark_conflicts": false,
    "flagged_for_review": false
  },

  "pipeline_decision": {
    "factory_floor_approved": true,
    "workshop_approved": true,
    "home_print_approved": true,
    "restrictions": [
      "Cell chemistry assembly requires Civil Center supervision"
    ],
    "expiry": "2026-09-12T18:30:00Z"
  }
}
```

### Verdict States

| Verdict | Meaning | Pipeline Action |
|---------|---------|----------------|
| **PASS** | All agents approve. No human review needed. | Proceed to System 7 (Workshop) and Factory Floor |
| **HOLD** | One or more agents flagged an edge case. | Queue for human moderator review. Product waits. |
| **REJECT** | Hard violation detected (weapon, counterfeit, hazardous). | Block permanently. Log to audit trail. Alert security team. |
| **CONDITIONAL** | Approved with restrictions. | Proceed with restrictions embedded in `workshop_package.json` (e.g., "Civil Center only") |

### Unsupervised ML for Code of Conduct Evolution

The Code of Conduct Agent starts with hard-coded rules (the "constitution") but evolves via unsupervised learning:

1. **Initial Training**: Hand-labeled dataset of ~10,000 product archetypes (weapons, tools, electronics, toys, medical devices, vehicles) with PASS/REJECT labels.
2. **Active Learning**: Products that score between 0.3 and 0.7 confidence are routed to human moderators. Their decisions become training data.
3. **Clustering**: The model discovers product categories organically — it might cluster "things with sharp edges and handles" and learn that kitchen knives PASS but switchblades REJECT, without being explicitly told the difference.
4. **Drift Detection**: If the model's confidence distribution shifts (e.g., a new product category emerges that doesn't fit existing clusters), it automatically increases the human review rate until it stabilizes.
5. **Adversarial Testing**: The system periodically generates adversarial product specs (products designed to fool the classifier) and tests itself. Failures trigger retraining.

```rust
/// AI Moderation Pipeline — multi-agent compliance check
#[derive(Resource)]
pub struct ModerationPipeline {
    /// Safety classification model
    pub safety_agent: SafetyClassificationAgent,
    /// Regulatory cross-reference agent
    pub regulatory_agent: RegulatoryProfileAgent,
    /// Code of conduct ML classifier
    pub conduct_agent: CodeOfConductAgent,
    /// Intellectual property checker
    pub ip_agent: IntellectualPropertyAgent,
    /// Human review queue for HOLD verdicts
    pub review_queue: Vec<HoldReview>,
    /// Audit log of all verdicts
    pub audit_log: Vec<ComplianceVerdict>,
}

/// Safety classification using product geometry, materials, and firmware analysis
pub struct SafetyClassificationAgent {
    /// Taxonomy of restricted product categories
    pub restricted_categories: Vec<RestrictedCategory>,
    /// Feature extractor for mesh analysis (sharp edges, barrel-like geometry, etc.)
    pub geometry_features: GeometryFeatureExtractor,
    /// Material hazard database
    pub material_hazards: HashMap<String, HazardLevel>,
    /// Classification confidence threshold (below this → HOLD)
    pub confidence_threshold: f64,
}

/// Restricted product category definition
pub struct RestrictedCategory {
    pub name: String,
    pub description: String,
    pub severity: RestrictionSeverity,
    /// Feature signatures that indicate this category
    pub signatures: Vec<ProductSignature>,
    /// Hard block (REJECT) or soft block (HOLD for review)
    pub action: RestrictionAction,
}

#[derive(Clone, Copy)]
pub enum RestrictionSeverity {
    /// Weapons, explosives — immediate REJECT
    Critical,
    /// Controlled substances equipment — REJECT with logging
    High,
    /// Dual-use potential — HOLD for human review
    Medium,
    /// Flagged but likely benign — PASS with note
    Low,
}

#[derive(Clone, Copy)]
pub enum RestrictionAction {
    Reject,
    Hold,
    PassWithRestriction,
}

/// Code of Conduct ML agent — evolves via unsupervised learning
pub struct CodeOfConductAgent {
    /// Current model version
    pub model_version: String,
    /// Hard-coded constitutional rules (never overridden by ML)
    pub constitutional_rules: Vec<ConstitutionalRule>,
    /// Learned cluster centroids from unsupervised training
    pub cluster_centroids: Vec<ProductCluster>,
    /// Confidence threshold for auto-pass (above this → PASS without review)
    pub auto_pass_threshold: f64,
    /// Confidence threshold for auto-reject (below this → REJECT without review)
    pub auto_reject_threshold: f64,
    /// Products in the uncertain zone go to human review
    pub human_review_count: u64,
    /// Total products classified
    pub total_classified: u64,
}

/// Constitutional rule — hard-coded, never overridden by ML
pub struct ConstitutionalRule {
    pub name: String,
    pub description: String,
    pub check: fn(&ProductionSpec) -> bool,
    pub action: RestrictionAction,
}

/// Regulatory profile agent — jurisdiction-aware certification checker
pub struct RegulatoryProfileAgent {
    /// Jurisdiction-specific regulatory databases
    pub jurisdictions: HashMap<String, Vec<RegulatoryStandard>>,
    /// Material compliance databases (RoHS, REACH, Prop 65)
    pub material_compliance: MaterialComplianceDatabase,
}

/// Compliance verdict output
#[derive(Clone, Serialize, Deserialize)]
pub struct ComplianceVerdict {
    pub generation: u64,
    pub timestamp: DateTime<Utc>,
    pub verdict: VerdictType,
    pub safety: SafetyResult,
    pub regulatory: RegulatoryResult,
    pub conduct: ConductResult,
    pub ip: IpResult,
    pub pipeline_decision: PipelineDecision,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum VerdictType {
    Pass,
    Hold,
    Reject,
    Conditional,
}

/// MCP endpoint extensions for compliance
/// POST /mcp/compliance/review    — Submit a production_spec.json for compliance review
/// GET  /mcp/compliance/verdict/{generation} — Retrieve verdict for a generation
/// POST /mcp/compliance/moderate  — Human moderator submits HOLD review decision
/// GET  /mcp/compliance/audit     — Retrieve full audit log
/// POST /mcp/compliance/train     — Submit labeled training data for conduct model
```

### MCP Compliance Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `POST /mcp/compliance/review` | POST | Submit `production_spec.json` for multi-agent compliance review |
| `GET /mcp/compliance/verdict/{generation}` | GET | Retrieve compliance verdict for a generation |
| `POST /mcp/compliance/moderate` | POST | Human moderator submits decision on HOLD verdict |
| `GET /mcp/compliance/audit` | GET | Retrieve full compliance audit trail |
| `POST /mcp/compliance/train` | POST | Submit labeled training data for Code of Conduct model retraining |

---

## Generalized Dynamics: VM Pool × Rayon × Variation Swarm

### The Performance Reality

The Variation Swarm does not just perturb TOML values — each variant runs a full simulation, which means each variant executes **Rune scripts** via the `ParallelScriptExecutor` (`soul/parallel_execution.rs`). Each script execution acquires a `PooledVm` from the `VmPool` (`soul/vm_pool.rs`). This means the performance table must account for the **VM pool overhead per core**.

The existing `VmPool` uses `crossbeam::ArrayQueue` for lock-free VM acquisition and `parking_lot::Mutex` for pool management. The `PooledVm` RAII guard returns the VM to the pool on drop. This infrastructure is already designed for exactly this use case — Rayon parallel iteration where each thread needs its own VM.

### Corrected Performance Table (VM Pool Aware)

| Cores | Variants per Generation | VMs per Pool (max_vms_per_script) | VM Acquisition Overhead | Effective Throughput | Estimated Wall Time (V-Cell) |
|-------|------------------------|----------------------------------|------------------------|---------------------|------------------------------|
| 4 (laptop) | 48 variants | 16 VMs × 4 scripts | ~0.2ms per acquire (pool hit) | ~46 effective (96% utilization) | ~35 min (28 generations) |
| 8 (workstation) | 96 variants | 16 VMs × 4 scripts | ~0.2ms per acquire | ~92 effective (96%) | ~17 min (25 generations) |
| 16 (server) | 192 variants | 16 VMs × 8 scripts | ~0.3ms per acquire (contention) | ~180 effective (94%) | ~9 min (22 generations) |
| 64 (cloud) | 512 variants | 16 VMs × 16 scripts | ~0.5ms per acquire (contention) | ~460 effective (90%) | ~4 min (18 generations) |

Key factors:
- **VM creation cost**: ~5ms cold start (avoided by pool pre-warming with `initial_pool_size: 4`)
- **Pool hit rate**: >95% when `max_vms_per_script ≥ cores` — each Rayon thread gets a warm VM
- **Contention**: At 64+ cores, `parking_lot::Mutex` on the pool map becomes the bottleneck. Solution: shard the pool map by script hash modulo N.
- **Memory**: Each Rune VM consumes ~2-8MB. At 64 cores × 16 VMs = 1024 VMs × 4MB avg = ~4GB dedicated to VM pools.

### Generalized Script Execution Across Swarm Branches

Each variant in the swarm is not just a TOML perturbation — it is a **complete simulation branch** with its own Rune script execution context. The `ParallelScriptExecutor.execute_parallel()` already uses `tasks.into_par_iter()`, so the integration is:

```rust
/// Extended swarm execution that includes Rune script evaluation per variant.
/// Each variant gets its own PooledVm from the VmPool, executes the product's
/// Soul Script with the variant's TOML properties, and returns the fitness score.
pub fn run_swarm_with_scripts(
    swarm: &mut VariationSwarm,
    base_toml: &toml::Table,
    sim_config: &SimulationConfig,
    vm_pool: &VmPool,
    script_hash: &str,
) -> Vec<SwarmResult> {
    let variants = swarm.generate_variants();

    variants
        .into_par_iter()
        .map(|variant_values| {
            // 1. Build variant TOML
            let mut toml = base_toml.clone();
            for (key, value) in &variant_values {
                apply_toml_value(&mut toml, key, *value);
            }

            // 2. Acquire PooledVm (RAII — returns to pool on drop)
            let mut pooled_vm = vm_pool.acquire(script_hash)
                .expect("VM pool exhausted — increase max_vms_per_script");

            // 3. Inject variant TOML properties into Rune VM context
            inject_toml_properties(pooled_vm.vm_mut(), &toml);

            // 4. Execute simulation script
            let start = std::time::Instant::now();
            let result = pooled_vm.vm_mut()
                .call::<f64>(["simulate"], ())
                .unwrap_or(0.0);
            let wall_time_ms = start.elapsed().as_millis() as u64;

            // 5. PooledVm drops here → VM returned to pool

            SwarmResult {
                values: variant_values,
                fitness: result,
                metrics: HashMap::new(),
                wall_time_ms,
            }
        })
        .collect()
}
```

This means every Rayon thread in the swarm:
1. Gets its own `PooledVm` from the lock-free `ArrayQueue`
2. Runs the Rune script with variant-specific TOML properties injected
3. Returns the VM to the pool when the closure exits (RAII drop)
4. Zero contention on the fast path (pool hit) — contention only when creating new VMs

---

## DiscoveredLaws as Telemetry: AI Prompt Improvement Over Time

### The Insight

Every time the `KnowledgeBase` records a `DiscoveredLaw`, that law is a **permanent piece of telemetry** about the physical universe as observed through simulation. These laws should not just sit in a JSON file — they should be **injected into the AI system prompt** for every subsequent generation, making the AI smarter with every product cycle.

### How It Works

```
Generation 1:    AI prompt has no telemetry. Explores blindly.
                 DiscoveredLaw: "CathodeThickness sensitivity = -18.7 fitness/mm"
                 DiscoveredLaw: "ElectrolyteMolarity sensitivity = +42.3 fitness/mol"

Generation 50:   AI prompt now includes 2 laws. AI starts with informed priors.
                 DiscoveredLaw: "Interaction: CathodeThickness × ElectrolyteMolarity
                                  has synergistic effect at ratio 0.068:1.2"

Generation 200:  AI prompt includes 47 laws. AI can reason about material science.
                 The Variation Swarm starts at near-optimal centers because the AI
                 already knows which dimensions matter and their approximate optima.

Generation 1000: AI prompt includes 312 laws across 15 product families.
                 AI can transfer knowledge from V-Cell batteries to capacitors to
                 fuel cells — cross-product generalization.
```

### Telemetry Injection Format

```rust
/// Build the AI system prompt with DiscoveredLaws telemetry
pub fn build_telemetry_prompt(
    base_prompt: &str,
    knowledge_base: &KnowledgeBase,
    product_family: &str,
) -> String {
    let relevant_laws: Vec<&DiscoveredLaw> = knowledge_base.laws
        .iter()
        .filter(|law| law.confidence > 0.8)
        .collect();

    let mut telemetry_section = String::from("\n\n--- DISCOVERED LAWS TELEMETRY ---\n");
    telemetry_section.push_str(&format!(
        "You have discovered {} verified physical laws from previous simulations.\n",
        relevant_laws.len()
    ));
    telemetry_section.push_str(
        "Use these to set intelligent priors. Do NOT contradict verified laws.\n\n"
    );

    for (i, law) in relevant_laws.iter().enumerate() {
        telemetry_section.push_str(&format!(
            "LAW #{}: {} (confidence: {:.1}%, discovered gen #{})\n  {}\n  TOML impact: {:?}\n\n",
            i + 1,
            law.name,
            law.confidence * 100.0,
            law.generation_discovered,
            law.description,
            law.toml_property_changes,
        ));
    }

    telemetry_section.push_str("--- END TELEMETRY ---\n");

    format!("{}{}", base_prompt, telemetry_section)
}
```

This means the Soul Service API key is now powering **five systems**:
1. `SoulBuildPipeline` — script generation
2. `RealizationBridge` — manufacturing manifest logic trace
3. `DreamManualGenerator` — assembly guide authoring
4. `GovernorPlugin` — hypothesis generation
5. **Telemetry-enhanced prompts** — every API call includes DiscoveredLaws

### Cross-Product Generalization

When the same Eustress instance runs multiple product families (batteries, motors, structural components), the DiscoveredLaws telemetry enables **transfer learning**:

- A law discovered in V-Cell battery optimization ("Thermal conductivity of housing material has >10× impact on cycle life") can inform motor housing design
- Sensitivity gradients from one product family become priors for another
- The `KnowledgeBase` tags each law with the product family, but the AI prompt includes laws from ALL families — letting the model find cross-domain patterns

---

## Multi-Agent Orchestration: Parallel Agents for Best-Fit Discovery

### The Problem with Single-Agent Optimization

A single AI agent (one Claude API call) can reason about one hypothesis at a time. But the Variation Swarm produces hundreds of results per generation. A single agent becomes the bottleneck — it can't process 128 variant results, extract patterns, propose the next swarm center, AND generate the Rune script modifications all in one call.

### The Multi-Agent Solution

The Governor (System 5) is decomposed into **specialized parallel agents**, each responsible for a subsystem. They share a `SharedBlackboard` resource that holds the current best-fit properties. After each swarm generation, all agents vote on the next action, and the best-fit properties are applied across all systems simultaneously.

```rust
/// Multi-agent orchestration for the feedback loop.
/// Each agent specializes in one system and proposes changes.
/// The Arbitrator merges proposals into a coherent best-fit.
#[derive(Resource)]
pub struct MultiAgentGovernor {
    /// Agent specializing in TOML property optimization (System 1)
    pub genotype_agent: AgentHandle,
    /// Agent specializing in Rune script behavior (System 2)
    pub phenotype_agent: AgentHandle,
    /// Agent specializing in fitness function design (System 3)
    pub metrics_agent: AgentHandle,
    /// Agent specializing in simulation configuration (System 4)
    pub simulation_agent: AgentHandle,
    /// Agent specializing in compliance pre-screening (System 8)
    pub compliance_agent: AgentHandle,
    /// The Arbitrator — merges all proposals into best-fit
    pub arbitrator: ArbitratorAgent,
    /// Shared state visible to all agents
    pub blackboard: SharedBlackboard,
}

/// Shared state that all agents read and write to
#[derive(Clone)]
pub struct SharedBlackboard {
    /// Current best-fit TOML properties
    pub best_properties: HashMap<String, f64>,
    /// Current best fitness score
    pub best_fitness: f64,
    /// DiscoveredLaws telemetry (read-only for agents)
    pub telemetry: Vec<DiscoveredLaw>,
    /// Swarm results from the most recent generation
    pub swarm_results: Vec<SwarmResult>,
    /// Compliance pre-screen result (does the current direction look safe?)
    pub compliance_pre_screen: Option<VerdictType>,
    /// Agent proposals for the next generation
    pub proposals: Vec<AgentProposal>,
}

/// A proposal from one agent about what to change next
#[derive(Clone)]
pub struct AgentProposal {
    /// Which agent made this proposal
    pub agent_name: String,
    /// Proposed TOML property changes
    pub property_changes: HashMap<String, f64>,
    /// Proposed Rune script modifications (if any)
    pub script_changes: Option<String>,
    /// Proposed simulation config changes (if any)
    pub sim_config_changes: Option<SimulationConfig>,
    /// Confidence in this proposal (0.0 to 1.0)
    pub confidence: f64,
    /// Reasoning (natural language)
    pub reasoning: String,
}

/// The Arbitrator merges proposals into a single best-fit action
pub struct ArbitratorAgent {
    /// Weighting strategy for merging proposals
    pub merge_strategy: MergeStrategy,
    /// Minimum confidence for a proposal to be considered
    pub confidence_floor: f64,
}

#[derive(Clone, Copy)]
pub enum MergeStrategy {
    /// Highest confidence proposal wins (simple)
    WinnerTakesAll,
    /// Weighted average of all proposals by confidence
    WeightedAverage,
    /// Proposals are ranked, top K merged with conflict resolution
    TopKMerge { k: usize },
    /// All proposals sent to a final Claude call for synthesis
    AiSynthesis,
}
```

### Agent Execution Flow

```
┌────────────────────────────────────────────────────────────────┐
│                    SWARM RESULTS (128 variants)                 │
│                    Written to SharedBlackboard                  │
└───────────────────────────┬────────────────────────────────────┘
                            │
            ┌───────────────┼───────────────┐
            │               │               │
     ┌──────▼──────┐ ┌─────▼──────┐ ┌──────▼──────┐
     │ Genotype    │ │ Phenotype  │ │ Simulation  │   ... (parallel MCP calls)
     │ Agent       │ │ Agent      │ │ Agent       │
     │             │ │            │ │             │
     │ "Tighten    │ │ "Add temp  │ │ "Increase   │
     │  cathode    │ │  ramp-down │ │  time_scale │
     │  radius to  │ │  logic in  │ │  to 100x    │
     │  ±0.5%"     │ │  Rune"     │ │  for aging" │
     │             │ │            │ │             │
     │ conf: 0.89  │ │ conf: 0.72 │ │ conf: 0.64  │
     └──────┬──────┘ └─────┬──────┘ └──────┬──────┘
            │              │               │
            └──────────────┼───────────────┘
                           │
                    ┌──────▼──────┐
                    │ ARBITRATOR  │
                    │             │
                    │ Merge by    │
                    │ weighted    │
                    │ average     │
                    │             │
                    │ Output:     │
                    │ best-fit    │
                    │ properties  │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │ APPLY TO    │
                    │ ALL SYSTEMS │
                    │             │
                    │ System 1: update .instance.toml
                    │ System 2: update .rune script
                    │ System 4: update simulation.toml
                    │ System 8: pre-screen compliance
                    └─────────────┘
```

Each agent is a **separate MCP API call** (Claude via Soul Service API key), running in parallel via `tokio::join!` or Rayon. The Arbitrator either uses weighted averaging (fast, deterministic) or makes a final synthesis call (slower, but can resolve contradictions between agents).

---

## Studio Heuristic: Idea to End Product

### The Complete UX Flow

The user opens Eustress Studio, describes an idea (or loads a prior ideation brief), and presses **two buttons**. The first button creates the product from nothing — patent, meshes, simulation files. The second button optimizes and builds it. Two clicks. Idea to physical product.

```
┌──────────────────────────────────────────────────────────────────────────┐
│  STUDIO INTERFACE — Full Pipeline                                        │
│                                                                          │
│  ┌──────────────────────── PHASE 1: IDEATION ─────────────────────┐     │
│  │  [▶ CREATE PRODUCT]          ← First button (System 0)          │     │
│  │  Input: "Solid-state sodium-sulfur battery, 15k cycle life"    │     │
│  │  Mode:  Natural Language ▼                                      │     │
│  └─────────────────────────────────────────────────────────────────┘     │
│                                                                          │
│  ┌──────────────────────── PHASE 2: OPTIMIZE ─────────────────────┐     │
│  │  [▶ OPTIMIZE & BUILD]        ← Second button (Systems 1-8)     │     │
│  │  Product: V-Cell 4680 Battery (from ideation)                   │     │
│  │  Target:  Maximize cycle life                                   │     │
│  │  Budget:  30 minutes compute                                    │     │
│  │  Safety:  Autonomous (no manual review)                         │     │
│  └─────────────────────────────────────────────────────────────────┘     │
│                                                                          │
│  Progress:                                                               │
│  ════════════════════════════════════════════ 100%                        │
│                                                                          │
│  ┌─ Pipeline Status ────────────────────────────────────────────────┐    │
│  │ ✓ System 0: Product ideated (PATENT + SOTA + 6 meshes + 6 TOML)│    │
│  │ ✓ System 1: Instance loaded (47 TOML properties)                │    │
│  │ ✓ System 2: Soul script compiled (3 Rune behaviors)             │    │
│  │ ⟳ System 4: Simulation running (gen 14/25, swarm 96 variants)  │    │
│  │ ⟳ System 3: Fitness tracking (best: 2847, improving)           │    │
│  │ ⟳ System 5: Multi-agent governor (3 agents active)             │    │
│  │ ○ System 6: Realization Bridge (waiting for convergence)        │    │
│  │ ○ System 8: Compliance Gate (waiting for manifest)              │    │
│  │ ○ System 7: Workshop Package (waiting for compliance)           │    │
│  └──────────────────────────────────────────────────────────────────┘    │
│                                                                          │
│  ┌─ Live Telemetry ─────────────────────────────────────────────────┐    │
│  │ DiscoveredLaws: 12 (3 new this run)                              │    │
│  │ Latest: "ElectrolyteMolarity × CathodeThickness interaction      │    │
│  │          coefficient = 42.3 (p < 0.001)"                         │    │
│  │ AI prompt enriched with 12 laws for next generation              │    │
│  └──────────────────────────────────────────────────────────────────┘    │
│                                                                          │
│  ┌─ Convergence Curve ──────────────────────────────────────────────┐    │
│  │     ╭──────────────────────────────╮                             │    │
│  │  f  │          ╱─────────────────── │  best: 2847                │    │
│  │  i  │       ╱╱╱                     │  mean: 2340                │    │
│  │  t  │     ╱╱                        │  std:  187                 │    │
│  │  n  │   ╱╱                          │                            │    │
│  │  e  │  ╱                            │  Converging...             │    │
│  │  s  │╱                              │  3 more gens to confirm    │    │
│  │  s  ╰───────────────────────────────╯                            │    │
│  │     gen 1    5     10    15    20   25                            │    │
│  └──────────────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────────────┘
```

### The Heuristic: Step by Step

| Step | System | What Happens | Duration | User Sees |
|------|--------|-------------|----------|-----------|
| 0 | **Ideation** | User describes idea (NL, form, Soul Script, or import). `IdeationPipeline` normalizes input into `IdeationBrief`. | ~1s | "Idea captured" |
| 0a | **System 0** | AI generates PATENT.md, SOTA_VALIDATION.md, EustressEngine_Requirements.md via Claude API. | ~90s | "Patent drafted (12 claims), SOTA validated" |
| 0b | **System 0** | Blender runs headless — generates AAA `.glb` meshes for each BOM component. | ~120s | "6 meshes generated (watertight, quad-dominant)" |
| 0c | **System 0** | `.glb.toml` instance files created with full realism sections. `ideation_brief.toml` written. `ProductCreatedEvent` fired. | ~10s | "Product package complete — ready for optimization" |
| 1 | **Button Press** | User clicks "Optimize & Build". `GovernorPlugin` fires `StartPipelineEvent`. | 0s | Progress bar starts |
| 2 | **System 1** | `InstanceGenerator` loads the generated `.instance.toml` + `.glb` from System 0. Extracts all numeric properties as `SwarmDimension` candidates. | <1s | "Instance loaded (47 properties)" |
| 3 | **System 2** | `SoulBuildPipeline` compiles the product's `.rune` scripts (generated from EustressEngine_Requirements.md). `VmPool` pre-warms VMs. `ParallelScriptExecutor` ready. | ~2s | "Soul script compiled" |
| 4 | **System 5** | `MultiAgentGovernor` initializes. Reads `ideation_brief.toml` target specs to define fitness function. Genotype agent reads `KnowledgeBase` for telemetry priors. Sets initial swarm center and radii. | ~3s | "Governor initialized with 12 laws, fitness = cycle_life" |
| 5 | **Swarm Loop** | `VariationSwarm.run_swarm_with_scripts()` runs N variants in parallel (Rayon + VmPool). Each variant: perturb TOML → inject into PooledVm → execute Rune sim → collect fitness. | ~30s per gen | Live convergence curve updating |
| 6 | **System 3** | `FitnessFunction` scores each variant. `WatchPointRegistry` records metrics. Swarm results written to `SharedBlackboard`. | inline | Fitness numbers update live |
| 7 | **Agents** | All agents read blackboard, propose next actions in parallel (tokio MCP calls). Arbitrator merges. `AdaptiveSwarmStrategy.adapt()` narrows/expands dimensions. DiscoveredLaws extracted and injected into next prompt. | ~2s | "3 agents proposed, arbitrator merged" |
| 8 | **Repeat 5-7** | Loop until convergence (3 stable generations + Verification Gate p<0.05). | ~15-25 gens | Progress bar advances per generation |
| 9 | **System 6** | `RealizationBridge` generates `production_spec.json` with hard constraints, sensitivity analysis (from swarm gradients), logic trace, verification protocol. | ~5s | "Manufacturing manifest generated" |
| 10 | **System 8** | `ModerationPipeline` runs all 4 compliance agents on the manifest. Verdict: PASS/HOLD/REJECT/CONDITIONAL. | ~3s | "Compliance: PASS" (green checkmark) |
| 11 | **System 7** | `WorkshopPackageManager` generates print files (STL), Dream Manual (HTML), BOM, Validation App config. Packages into `workshop_package.json`. | ~10s | "Workshop package ready — Download" |
| 12 | **Done** | User downloads the package, sends to Civil Center or starts home printing. Validation App opens. | — | Download button + QR code for Validation App |

**Total wall time: ~21 minutes on 8-core workstation.** (~4 min ideation + ~17 min optimization). Two button presses. Zero manual engineering.

### The Probabilistic Chain of Thought

The "high energy output options in probabilistic chain of thought" is realized through the **multi-agent proposal → arbitrator merge → swarm execution** cycle. Each agent uses Claude's chain-of-thought reasoning (via Soul Service API key) with DiscoveredLaws telemetry in the prompt. The chain of thought is:

1. **Read telemetry** → "I know CathodeThickness has sensitivity -18.7. ElectrolyteMolarity has +42.3."
2. **Read swarm results** → "Generation 14 shows the best variant at CathodeThickness=0.085, but a cluster at 0.082 is within 5% fitness."
3. **Reason probabilistically** → "The bimodal distribution suggests a ridge in the fitness landscape. I should explore the valley between 0.082 and 0.085 with finer resolution."
4. **Propose** → `AgentProposal { property_changes: {"CathodeThickness.radius_pct": 1.5}, confidence: 0.89, reasoning: "..." }`

This reasoning is logged, traceable, and improves with each generation via telemetry injection.

---

## Opus Pipeline Execution Model

### The Question: Fragmented vs Monolithic

Can one Opus API call handle the whole 70-task pipeline, or does it need to be fragmented?

### The Answer: Hierarchical Multi-Call Architecture

Neither purely fragmented nor monolithic. The optimal architecture is **hierarchical**:

```
┌─────────────────────────────────────────────────┐
│  ORCHESTRATOR (1 Opus call per generation)        │
│                                                   │
│  Receives: SharedBlackboard + DiscoveredLaws      │
│  Outputs:  High-level strategy for this gen       │
│            + delegation to specialist agents       │
│                                                   │
│  Token budget: ~8K input + ~2K output = ~10K      │
│  Cost: ~$0.15 per generation                      │
└─────────────┬───────────────────────────────────┘
              │
   ┌──────────┼──────────┐
   │          │          │
   ▼          ▼          ▼
┌──────┐  ┌──────┐  ┌──────┐
│Geno  │  │Pheno │  │Sim   │   (3-5 Sonnet calls, parallel)
│Agent │  │Agent │  │Agent │
│      │  │      │  │      │   Token budget: ~4K each
│Sonnet│  │Sonnet│  │Sonnet│   Cost: ~$0.03 each
└──────┘  └──────┘  └──────┘
```

**Why this works:**
- **1 Opus call** per generation for high-level reasoning and strategy (which dimensions to explore, when to switch from exploration to exploitation, when to declare convergence). This is the "expensive thinking" — ~$0.15 per call.
- **3-5 Sonnet calls** per generation for specialist tasks (TOML perturbation, script modification, simulation config). These are "fast execution" — ~$0.03 each.
- **Total per generation**: ~$0.30
- **Total for a 25-generation run**: ~$7.50
- **Total for the full pipeline** (including System 6 manifest, System 7 workshop, System 8 compliance): ~$10-15 per product optimization cycle.

**The alternative (1 Opus call for everything):**
- Would need ~100K+ context window per generation (all swarm results + all laws + all agent proposals)
- Would hit output token limits trying to produce TOML changes + Rune scripts + sim config + reasoning in one response
- Would cost ~$2-5 per call × 25 generations = $50-125 per cycle
- Slower because you can't parallelize the specialist work

**The fragmented approach (all Sonnet, no Opus):**
- Loses the high-level strategic reasoning that Opus provides
- Each agent optimizes locally but nobody sees the big picture
- Converges slower (40+ generations instead of 25) because the cross-dimension interaction detection requires Opus-level reasoning

**Recommendation for Anthropic feedback:** The ideal capability would be a **"Governor Mode" API tier** — a single long-running API session that maintains state across multiple calls within a pipeline, with the ability to delegate sub-tasks to cheaper models. This would eliminate the overhead of re-injecting context on every call and enable true multi-turn reasoning across the full optimization loop.

---

## Implementation Phases

### Phase 0: Ideation Pipeline (System 0)
1. Implement `IdeationBrief` struct and `ideation_brief.toml` parser/serializer
2. Implement `IdeationPipeline` resource with `IdeationState` state machine
3. Implement `IdeationArtifacts` tracking — patent, SOTA, requirements, meshes, instances
4. Implement natural language normalizer — extract `ProductDefinition`, `Innovation`, `TargetSpec`, `BomEntry` from freeform text via Claude API
5. Implement Soul Script normalizer — reverse-engineer product definition from `.soul` behavior spec
6. Wire `ProductCreatedEvent` — fired on `IdeationState::Complete`, consumed by Systems 1, 2, and 5
7. Add MCP endpoints: `POST /mcp/ideation/brief`, `GET /mcp/ideation/status`, `GET /mcp/ideation/artifacts`, `POST /mcp/ideation/normalize`, `GET /mcp/ideation/history`, `POST /mcp/ideation/import`
8. Implement Blender headless orchestrator — invoke Blender Python scripts from Rust via `std::process::Command`, collect `.glb` outputs
9. Implement `.glb.toml` auto-generator — read `EustressEngine_Requirements.md` + BOM to produce instance files with realism sections
10. Implement `IdeationRecord` history with JSON persistence
11. Wire `/create-voltec-product` workflow steps to `IdeationState` transitions (Phase 1 compatibility bridge)
12. Workshop Panel Slint UI — product brief form, innovation list, spec table, BOM table, idea source selector, pipeline status display
13. Implement `[▶ CREATE PRODUCT]` button callback — fires `StartIdeationEvent` from Workshop Panel
14. Implement ideation-to-optimization handoff — `ProductCreatedEvent` populates `InstanceGenerator`, `ScriptGenerator`, and `FitnessFunction` from `ideation_brief.toml` target specs

### Phase 1: Foundation (Core Resources)
15. Create `src/feedback_loop/mod.rs` module
16. Implement `InstanceGenerator` resource
17. Implement `SimulationOrchestrator` resource
18. Implement `FitnessFunction` resource with battery cycle example
19. Wire `GenerationSpawned` event to auto-start simulation
20. Implement data export to `output/generation_{N}/` folder structure

### Phase 2: Verification
21. Implement `VerificationGate` with Welch's t-test (use `statrs` crate)
22. Implement `KnowledgeBase` with JSON persistence
23. Add rollback mechanism — snapshot TOML/scripts before mutation
24. Add multi-run capability — repeat same generation N times for statistics

### Phase 3: AI Control Interface
25. Extend MCP server with `/mcp/governor/*` endpoints
26. Implement `ScriptGenerator` resource
27. Wire MCP endpoints to `InstanceGenerator` and `SimulationOrchestrator`
28. Add WebSocket streaming for live fitness score updates during runs

### Phase 4: Hot-Reload Tightening
29. Enhance `HotReloadPlugin` — currently a stub, implement TOML + Rune hot-reload
30. Wire `file_watcher.rs` to `InstanceGenerator` for generation tracking
31. Implement adaptive sampling rate in `SimulationClock`
32. Add anti-aliasing warning when `sim_seconds_per_tick > 60.0`

### Phase 5: Autonomous Loop
33. Create `GovernorPlugin` — orchestrates the full loop as a Bevy system set
34. Implement autonomous mode — AI runs unattended with configurable generation limit
35. Add safety bounds — max temperature, structural integrity floors, etc.
36. Dashboard UI in Slint showing generation progress, fitness curve, knowledge base

### Phase 6: Realization Bridge (System 6)
37. Implement `ManufacturingManifest` resource and `ProductionSpec` struct
38. Implement `SensitivityAnalyzer` — perturbation-based sensitivity coefficients
39. Implement `RealizationBridgePlugin` with `GenerateManifestEvent`
40. Wire Soul Service API key to manifest generation (logic trace, reality risks)
41. Implement `production_spec.json` export with full schema
42. Add MCP endpoints: `POST /mcp/governor/realize`, `GET /mcp/governor/manifest/{generation}`
43. Implement firmware logic extraction — map Rune triggers to hardware timing specs
44. Add Verification Protocol generator — auto-design real-world tests from watchpoints
45. Dashboard panel in Slint for viewing/exporting manufacturing manifests

### Phase 7: Eustress Workshop (System 7)
46. Implement `WorkshopPackageManager` resource and `WorkshopPackage` struct
47. Implement `PrintOptimizer` — GLB to STL conversion with shrinkage compensation and build volume splitting
48. Implement `PrinterProfile` system with FDM/SLA/SLS/CNC profiles
49. Implement `DreamManualGenerator` — Claude API-powered assembly guide from production spec
50. Implement `BillOfMaterials` generator — trace each part to a hard constraint
51. Implement `ValidationAppGenerator` — TOML-defined check-in UI from verification protocol
52. Implement `WorkshopPlugin` with `GenerateWorkshopEvent` and `ValidationSubmittedEvent`
53. Add MCP endpoints: `POST /mcp/workshop/generate`, `POST /mcp/workshop/validate`, `GET /mcp/workshop/package/{generation}`
54. Implement `RealWorldFeedbackQueue` — ingest validation results back into System 3
55. Implement `VarianceStat` tracking — aggregate manufacturing deviations to refine sensitivity analysis
56. Firmware binary generation pipeline — compile Rune logic to ESP32/STM32 firmware images
57. Workshop dashboard in Slint — browse packages, preview Dream Manual, monitor validation submissions
58. Civil Center inventory integration — sync BOM availability from local hardware stock

### Phase 8: Parallel Swarm Acceleration (Circumstances Integration)
59. Implement `VariationSwarm` resource with `SwarmDimension`, `SwarmResult`, `ConvergencePoint`
60. Implement `run_swarm` — Rayon `into_par_iter` over variant TOML matrix with parallel simulation execution
61. Implement Latin Hypercube Sampling variant generator with physical bounds enforcement
62. Implement `update_sensitivities` — linear regression per-dimension gradient extraction from swarm results
63. Implement `AdaptiveSwarmStrategy` — Bayesian-driven radius narrowing/expansion, local optimum trap detection
64. Wire swarm results as `Signal` objects into the Circumstances engine for Bayesian posterior updates
65. Implement convergence detector — 3-generation stability check + Verification Gate confirmation
66. Add MCP endpoints: `POST /mcp/governor/swarm`, `GET /mcp/governor/swarm/results/{generation}`, `GET /mcp/governor/swarm/convergence`
67. Implement `DiscoveredLaw` auto-extraction when dimension sensitivity stabilizes across 3+ generations
68. Swarm dashboard in Slint — real-time fitness heatmap, convergence curve, dimension sensitivity bar chart
69. Multi-dimension swarm — simultaneous perturbation of 2+ TOML properties with interaction detection
70. Integrate `KnowledgeBase` priors — use known laws to set initial swarm center and narrow radius on known dimensions

### Phase 9: Legal Compliance Gate (System 8)
71. Implement `ModerationPipeline` resource with multi-agent architecture
72. Implement `SafetyClassificationAgent` — geometry feature extraction, restricted category taxonomy, dual-use detection
73. Implement `RegulatoryProfileAgent` — jurisdiction-aware certification lookup (UL, CE, FCC, RoHS, FDA)
74. Implement `CodeOfConductAgent` — constitutional rules (hard-coded) + unsupervised ML classifier for edge cases
75. Implement `IntellectualPropertyAgent` — patent/design similarity search (flag-only, non-blocking)
76. Implement `ComplianceVerdict` output struct and `compliance_verdict.json` export
77. Add MCP endpoints: `POST /mcp/compliance/review`, `GET /mcp/compliance/verdict/{generation}`, `POST /mcp/compliance/moderate`, `GET /mcp/compliance/audit`, `POST /mcp/compliance/train`
78. Implement HOLD review queue — human moderator UI in Slint for edge case decisions
79. Implement active learning loop — route low-confidence verdicts to human review, retrain classifier from decisions
80. Implement adversarial self-testing — periodic generation of adversarial product specs to validate classifier
81. Implement drift detection — monitor confidence distribution, auto-increase human review rate when new categories emerge
82. Compliance dashboard in Slint — verdict history, audit trail, active HOLD queue, model confidence distribution

### Phase 10: Multi-Agent Orchestration, Telemetry & Studio Heuristic
83. Implement `MultiAgentGovernor` resource with `SharedBlackboard`, `AgentProposal`, `ArbitratorAgent`
84. Implement Genotype Agent — reads swarm results, proposes TOML property changes via Soul Service API (Sonnet)
85. Implement Phenotype Agent — reads swarm results, proposes Rune script modifications via Soul Service API (Sonnet)
86. Implement Simulation Agent — reads convergence curve, proposes `SimulationConfig` changes (time_scale, tick_rate)
87. Implement Compliance Pre-Screen Agent — lightweight pre-check during optimization to avoid converging on illegal designs
88. Implement `ArbitratorAgent` with `MergeStrategy` (WinnerTakesAll, WeightedAverage, TopKMerge, AiSynthesis)
89. Implement `build_telemetry_prompt` — inject DiscoveredLaws into AI system prompt with confidence, generation, and TOML impact
90. Implement cross-product generalization — tag laws by product family, include cross-family laws in prompts for transfer learning
91. Implement Opus orchestrator call — 1 Opus per generation for high-level strategy, delegates to Sonnet specialist agents
92. Implement `StartPipelineEvent` — two-button flow: [CREATE PRODUCT] fires System 0, [OPTIMIZE & BUILD] fires Systems 1→2→4→3→5→6→8→7
93. Implement pipeline progress Slint UI — status per system (including System 0), live convergence curve, telemetry panel, download button
94. Implement `run_swarm_with_scripts` — unified Rayon parallel execution with VmPool integration per variant

---

## Dependencies

| Crate | Purpose | Feature Flag |
|-------|---------|-------------|
| `statrs` | Welch's t-test, statistical distributions, confidence intervals | `feedback-loop` |
| `csv` | Export metrics to CSV | already available |
| `serde_json` | Fitness/knowledge/manifest/workshop JSON serialization | already available |
| `uuid` | Generation identifiers | already available |
| `chrono` | Timestamps for production_spec.json and workshop_package.json | already available |
| `toml` | Parse/perturb instance TOML for sensitivity analysis | already available |
| `stl_io` | STL mesh export for print-ready files | `workshop` |
| `gltf` | GLB mesh parsing for print optimization | `workshop` |
| `askama` or `tera` | HTML template engine for Dream Manual generation | `workshop` |
| `rayon` | Parallel iteration for Variation Swarm and Monte Carlo sampling | already available (used by `scenarios/engine.rs`) |
| `rand` | Random number generation for Latin Hypercube Sampling and swarm perturbations | already available |
| `crossbeam` | Lock-free ArrayQueue for VmPool — Rayon thread VM acquisition | already available (used by `soul/vm_pool.rs`) |
| `parking_lot` | Fast mutex for VmPool management under parallel contention | already available |
| `ort` | ONNX Runtime — run Code of Conduct ML classifier for safety classification | `compliance` |
| `linfa` | Pure-Rust ML toolkit — clustering, classification for unsupervised conduct model | `compliance` |
| `tokenizers` | Tokenize product descriptions for ML feature extraction | `compliance` |
| `tokio` | Async runtime for parallel MCP API calls to multi-agent system | already available |

---

## Relationship to Existing Systems

- **Ideation** (System 0) — the genesis system. Every product begins as an idea — natural language, a Soul Script sketch, a structured form in the Workshop Panel, or an imported `ideation_brief.toml`. The `IdeationPipeline` normalizes any input into an `IdeationBrief`, then drives the `/create-voltec-product` workflow: PATENT.md → SOTA_VALIDATION.md → EustressEngine_Requirements.md → Blender headless mesh generation → `.glb.toml` instance files. Phase 1 (current) runs via Windsurf `/create-voltec-product` slash command. Phase 2 (future) runs via the Workshop Panel in Eustress Studio Slint UI. On completion, fires `ProductCreatedEvent` consumed by Systems 1, 2, and 5. This is the only system that requires a human in the loop — everything after is autonomous.
- **Scenarios Module** (`src/scenarios/`) — backward-looking investigation. Feedback loop is forward-looking optimization. Both share Monte Carlo (`run_simulation` with Rayon `into_par_iter`) and branching infrastructure. The `BranchTreeSnapshot` pattern for lock-free parallel sampling is reused by `VariationSwarm`.
- **Circumstances Module** (`src/circumstances/`) — the acceleration engine. Circumstances' `Signal`, `Forecast`, `DecisionPoint`, and Bayesian update machinery drive the Variation Swarm's adaptive search. Each swarm result becomes a `Signal` that updates `Forecast` branch posteriors. `SupplierRiskScore` maps to property sensitivity tracking. `DisruptionType` maps to local optimum trap detection. Workshop validation data enhances forecast accuracy.
- **MCP Server** (`crates/mcp/`) — AI control interface. Ideation, Governor, Realization Bridge, Workshop, Swarm, and Compliance endpoints extend the existing CRUD pattern. Multi-agent calls use parallel `tokio::join!` through the same MCP infrastructure. Ideation adds 6 new endpoints under `/mcp/ideation/*`.
- **Soul Scripting** (`src/soul/`) — behavior definition and execution engine. The `VmPool` (`soul/vm_pool.rs`) provides lock-free VM pooling via `crossbeam::ArrayQueue`. The `ParallelScriptExecutor` (`soul/parallel_execution.rs`) uses `Rayon::into_par_iter` for parallel Rune execution. Both are reused directly by `run_swarm_with_scripts` — each swarm variant acquires its own `PooledVm` (RAII), executes the product script with variant TOML injected, and returns the VM on drop. The Soul Service API key powers seven capabilities: ideation normalization, script generation, manufacturing manifest, Dream Manual, Governor hypothesis, compliance classification, and telemetry-enhanced prompts.
- **Realization Bridge** (System 6) — converts digital optimization into physical manufacturing specs. Produces the `production_spec.json` that professional engineers execute on the factory floor. Sensitivity Analysis data now comes from swarm gradients computed during optimization, not post-hoc perturbation.
- **Legal Compliance** (System 8) — the safety gate between Realization Bridge and distribution. Four AI moderation agents (Safety, Regulatory, Conduct, Intellectual Property) review every `production_spec.json`. The Code of Conduct agent evolves via unsupervised ML with constitutional hard rules that can never be overridden. Products must PASS before Workshop packages are generated. HOLD verdicts queue for human moderator review. REJECT verdicts are permanently blocked.
- **Eustress Workshop** (System 7) — democratizes the Fundamental Truths for consumers. Converts `production_spec.json` into printable kits, assembly guides, and validation apps. Only receives products that have passed System 8 compliance. Restrictions from the compliance verdict (e.g., "Civil Center only") are embedded in the `workshop_package.json`. Real-world feedback from Workshop builds flows back into System 3, closing the Value Loop and making every future simulation more accurate.
- **DiscoveredLaws Telemetry** — the KnowledgeBase is not passive storage. Every verified law is injected into the AI system prompt for subsequent generations, enabling the AI to start with informed priors instead of blind exploration. Cross-product generalization allows laws from one product family to improve optimization of another. This is the mechanism by which the system gets smarter over time — the AI's reasoning improves with every product cycle.
- **Multi-Agent Governor** — the AI Governor (System 5) is decomposed into specialized parallel agents (Genotype, Phenotype, Simulation, Compliance). Each agent makes a proposal after every swarm generation. The Arbitrator merges proposals via configurable strategy (weighted average, winner-takes-all, or AI synthesis). This enables simultaneous optimization of TOML properties, Rune behavior, and simulation configuration — not just one at a time.
- **Studio Heuristic** — the full 9-system pipeline is a two-button flow in Eustress Studio. Button 1: "Create Product" (System 0 — ideation, patent, meshes, instances, ~4 min). Button 2: "Optimize & Build" (Systems 1-8 — optimization loop, compliance, workshop package, ~17 min). Pipeline status, convergence curve, telemetry panel, and compliance verdict are all visible live. The user's only decisions are what to dream and how long to spend. Everything else is automated.
