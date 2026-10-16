//! # Artifact Generation — Per-Step System Prompts and Dispatch
//!
//! Each pipeline step after normalization generates a specific artifact type.
//! This module defines the system prompts, output file paths, and dispatch logic
//! for each of the 8 artifact generation steps.
//!
//! ## Table of Contents
//!
//! 1. ArtifactStep enum — maps pipeline step indices to generation logic
//! 2. Per-step system prompts — patent, SOTA, requirements, meshes, parts, sim scripts, UI, catalog
//! 3. dispatch_artifact_request — spawns background thread for approved artifact steps
//! 4. handle_artifact_response — processes completed artifact generation (write to disk)
//!
//! ## Architecture
//!
//! All generated artifacts live in the Space/Universe filesystem hierarchy:
//!
//! - **Workspace/{product}/** — product directory: part.toml files, patent, SOTA,
//!   requirements, brief, catalog README, Blender script
//! - **Universe/assets/meshes/{product}/** — shared .glb mesh files (referenced by part.toml)
//! - **SoulService/{product}/** — Rune simulation scripts (physics, fitness)
//! - **StarterGui/{product}/** — ScreenGui UI TOML files
//! - **StarterGui/{product}/scripts/** — Rune UI data-feedback scripts
//!
//! No docs/Products/ directory is used in Eustress. Each step requires an approved
//! MCP command before dispatching. The brief TOML is included as context in every prompt.
//! Each step fires a ClaudeResponseEvent with the step_index set.

use bevy::prelude::*;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use eustress_common::soul::ClaudeConfig;

use super::{
    IdeationPipeline, IdeationState, ClaudeResponseEvent,
    McpCommandStatus, ArtifactType,
    claude_bridge::WorkshopClaudeTasks,
};

// ============================================================================
// 1. ArtifactStep — pipeline step to generation logic mapping
// ============================================================================

/// Maps pipeline step index to artifact generation parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStep {
    /// Step 1: Generate PATENT.md (42+ claims, cross-sections, BOM)
    Patent,
    /// Step 2: Generate SOTA_VALIDATION.md (honesty-tiered validation)
    SotaValidation,
    /// Step 3: Generate EustressEngine_Requirements.md (material properties, ECS mappings)
    Requirements,
    /// Step 4: Generate Blender Python scripts for mesh creation
    MeshGeneration,
    /// Step 5: Generate .part.toml files placed in Workspace
    PartFiles,
    /// Step 6: Generate Rune simulation scripts placed in SoulService
    SimScripts,
    /// Step 7: Generate ScreenGui UI TOML + Rune UI scripts placed in StarterGui
    UiGeneration,
    /// Step 8: Generate README.md and update Products.md catalog
    CatalogEntry,
    /// Step 9: Generate DEAL_STRUCTURE.md — equity split, royalty terms, manufacturing program stake
    DealStructure,
    /// Step 10: Generate LOGISTICS_PLAN.md — pilot program, warehousing, fulfillment partners
    LogisticsPlan,
}

impl ArtifactStep {
    /// Convert pipeline step index (1-6) to ArtifactStep
    /// Step 0 is normalization (handled by claude_bridge::dispatch_normalize_request)
    pub fn from_step_index(index: u32) -> Option<Self> {
        match index {
            1 => Some(Self::Patent),
            2 => Some(Self::SotaValidation),
            3 => Some(Self::Requirements),
            4 => Some(Self::MeshGeneration),
            5 => Some(Self::PartFiles),
            6 => Some(Self::SimScripts),
            7 => Some(Self::UiGeneration),
            8 => Some(Self::CatalogEntry),
            9 => Some(Self::DealStructure),
            10 => Some(Self::LogisticsPlan),
            _ => None,
        }
    }

    /// Pipeline step index for this artifact step
    pub fn step_index(&self) -> u32 {
        match self {
            Self::Patent => 1,
            Self::SotaValidation => 2,
            Self::Requirements => 3,
            Self::MeshGeneration => 4,
            Self::PartFiles => 5,
            Self::SimScripts => 6,
            Self::UiGeneration => 7,
            Self::CatalogEntry => 8,
            Self::DealStructure => 9,
            Self::LogisticsPlan => 10,
        }
    }

    /// The IdeationState that corresponds to this step being active
    pub fn pipeline_state(&self) -> IdeationState {
        match self {
            Self::Patent => IdeationState::GeneratingPatent,
            Self::SotaValidation => IdeationState::GeneratingSotaValidation,
            Self::Requirements => IdeationState::GeneratingRequirements,
            Self::MeshGeneration => IdeationState::GeneratingMeshes,
            Self::PartFiles => IdeationState::GeneratingParts,
            Self::SimScripts => IdeationState::GeneratingSimScripts,
            Self::UiGeneration => IdeationState::GeneratingUI,
            Self::CatalogEntry => IdeationState::FinalizingCatalog,
            Self::DealStructure => IdeationState::GeneratingDealStructure,
            Self::LogisticsPlan => IdeationState::GeneratingLogisticsPlan,
        }
    }

    /// MCP endpoint step parameter value
    pub fn step_param(&self) -> &'static str {
        match self {
            Self::Patent => "patent",
            Self::SotaValidation => "sota",
            Self::Requirements => "requirements",
            Self::MeshGeneration => "meshes",
            Self::PartFiles => "parts",
            Self::SimScripts => "sim_scripts",
            Self::UiGeneration => "ui",
            Self::CatalogEntry => "catalog",
            Self::DealStructure => "deal_structure",
            Self::LogisticsPlan => "logistics_plan",
        }
    }

    /// Output filename relative to the product docs directory (for doc artifacts)
    /// or a marker for runtime artifacts that go into Space/Universe directories.
    pub fn output_filename(&self) -> &'static str {
        match self {
            Self::Patent => "PATENT.md",
            Self::SotaValidation => "SOTA_VALIDATION.md",
            Self::Requirements => "EustressEngine_Requirements.md",
            Self::MeshGeneration => "generate_meshes.py",
            Self::PartFiles => "__WORKSPACE__",  // Written to Space/Workspace/{product}/
            Self::SimScripts => "__SOULSERVICE__",   // Written to Space/SoulService/{product}/
            Self::UiGeneration => "__STARTERGUI__",   // Written to Space/StarterGui/{product}/
            Self::CatalogEntry => "README.md",
            Self::DealStructure => "DEAL_STRUCTURE.md",
            Self::LogisticsPlan => "LOGISTICS_PLAN.md",
        }
    }

    /// ArtifactType for chat messages
    pub fn artifact_type(&self) -> ArtifactType {
        match self {
            Self::Patent => ArtifactType::Patent,
            Self::SotaValidation => ArtifactType::Sota,
            Self::Requirements => ArtifactType::Requirements,
            Self::MeshGeneration => ArtifactType::Mesh,
            Self::PartFiles => ArtifactType::Toml,
            Self::SimScripts => ArtifactType::RuneSimScript,
            Self::UiGeneration => ArtifactType::UiToml,
            Self::CatalogEntry => ArtifactType::Catalog,
            Self::DealStructure => ArtifactType::DealStructure,
            Self::LogisticsPlan => ArtifactType::LogisticsPlan,
        }
    }

    /// Estimated BYOK cost for this step
    pub fn estimated_cost(&self) -> f64 {
        match self {
            Self::Patent => 0.05,
            Self::SotaValidation => 0.04,
            Self::Requirements => 0.04,
            Self::MeshGeneration => 0.03,
            Self::PartFiles => 0.02,
            Self::SimScripts => 0.04,
            Self::UiGeneration => 0.04,
            Self::CatalogEntry => 0.01,
            Self::DealStructure => 0.04,
            Self::LogisticsPlan => 0.04,
        }
    }

    /// Get the system prompt for this artifact step
    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::Patent => PATENT_SYSTEM_PROMPT,
            Self::SotaValidation => SOTA_SYSTEM_PROMPT,
            Self::Requirements => REQUIREMENTS_SYSTEM_PROMPT,
            Self::MeshGeneration => MESH_SYSTEM_PROMPT,
            Self::PartFiles => PART_SYSTEM_PROMPT,
            Self::SimScripts => SIM_SCRIPTS_SYSTEM_PROMPT,
            Self::UiGeneration => UI_GENERATION_SYSTEM_PROMPT,
            Self::CatalogEntry => CATALOG_SYSTEM_PROMPT,
            Self::DealStructure => DEAL_STRUCTURE_SYSTEM_PROMPT,
            Self::LogisticsPlan => LOGISTICS_PLAN_SYSTEM_PROMPT,
        }
    }
}

// ============================================================================
// 2. Per-step system prompts
// ============================================================================

/// System prompt for PATENT.md generation
const PATENT_SYSTEM_PROMPT: &str = r#"You are a patent specification writer for the Eustress Engine product pipeline.

Given an ideation brief (TOML), generate a comprehensive PATENT.md with:

1. **Title and Abstract** — formal patent language
2. **Background of the Invention** — prior art and problems solved
3. **Detailed Description** — complete technical specification with:
   - Cross-sectional diagrams (described in text for later illustration)
   - Material specifications with exact compositions
   - Manufacturing process steps
   - Assembly sequence
4. **Claims** — minimum 42 independent and dependent claims covering:
   - Product claims (apparatus/device)
   - Method claims (manufacturing process)
   - System claims (integration with larger systems)
   - Composition of matter claims (novel materials)
5. **Bill of Materials** — every component with material, dimensions, and role
6. **Figures Description** — text descriptions of all cross-sections and exploded views

Output ONLY the markdown content for PATENT.md. Do not wrap in code fences.
Be technically precise. Reference specific alloy grades, chemical formulas, and tolerances."#;

/// System prompt for SOTA_VALIDATION.md generation
const SOTA_SYSTEM_PROMPT: &str = r#"You are a state-of-the-art validation analyst for the Eustress Engine product pipeline.

Given an ideation brief (TOML), generate SOTA_VALIDATION.md with honesty-tiered validation:

1. **Executive Summary** — one paragraph assessment of novelty
2. **Prior Art Analysis** — for each innovation in the brief:
   - Known prior art (published papers, patents, commercial products)
   - Closest competing technology with specific performance numbers
   - Gap analysis: what the proposed product claims vs what exists
3. **Validation Tier Assessment** — for each target spec:
   - VERIFIED: Published peer-reviewed data supports this claim (cite sources)
   - PROJECTED: Physics supports this but not demonstrated at scale (explain reasoning)
   - ASPIRATIONAL: Theoretical only, requires breakthroughs (identify which ones)
4. **Risk Matrix** — technical risks ranked by likelihood and impact
5. **Recommendations** — specific changes to improve feasibility

Be brutally honest. Flag any claim that lacks scientific backing.
Output ONLY the markdown content. Do not wrap in code fences."#;

/// System prompt for EustressEngine_Requirements.md generation
const REQUIREMENTS_SYSTEM_PROMPT: &str = r#"You are a simulation requirements engineer for the Eustress Engine.

Given an ideation brief (TOML), generate EustressEngine_Requirements.md with:

1. **Material Property Tables** — for each BOM component:
   - Density, Young's modulus, yield strength, thermal conductivity
   - Specific heat capacity, melting point, CTE
   - Electrochemical properties (if applicable): ionic conductivity, voltage window
2. **ECS Component Mapping** — which Bevy/Eustress components each part needs:
   - Transform, Mesh, Material, Physics body type
   - Custom properties (thermodynamic state, electrochemical state)
   - Script bindings (which watchpoints to expose)
3. **Simulation Laws** — the physics/chemistry equations to implement:
   - Governing equations with variable names matching TOML property keys
   - Time integration method (explicit Euler, Verlet, RK4, or adaptive RK45)
   - Stability constraints (max timestep for numerical accuracy)
4. **Fitness Function Definition** — how to score simulation results:
   - Primary metric, secondary metrics, safety constraints
   - Benchmark values from SOTA validation
5. **Mesh Requirements** — geometry specs for Blender generation:
   - Part dimensions, tolerances, critical features
   - UV mapping requirements, material slot assignments

Output ONLY the markdown content. Do not wrap in code fences."#;

/// System prompt for Blender mesh generation scripts
const MESH_SYSTEM_PROMPT: &str = r#"You are a Blender Python script generator for the Eustress Engine product pipeline.

Given an ideation brief (TOML), generate a Blender Python script that:

1. Creates all mesh parts listed in the bill_of_materials
2. Uses exact dimensions from the brief (in meters)
3. Applies appropriate materials with PBR properties
4. Names each object to match the BOM component name
5. Creates proper UV mappings for each part
6. Exports each part as a separate .glb file
7. Also exports an assembled version as {product_name}_assembled.glb

Script requirements:
- Import bpy at the top
- Clear the default scene
- Use bpy.ops.mesh.primitive_* or bmesh for geometry
- Set proper origins and transforms
- Export with glTF 2.0 settings (draco compression disabled)
- Output directory: same as the script location + "/meshes/"

The script must be runnable headless via: blender --background --python generate_meshes.py

Output ONLY the Python script. Do not wrap in markdown code fences."#;

/// System prompt for .part.toml file generation
/// Part files go in Workspace/{product}/ and reference meshes via assets/meshes/{product}/
const PART_SYSTEM_PROMPT: &str = r#"You are a part file generator for the Eustress Engine.

Given an ideation brief (TOML), generate .part.toml files for each mesh part.
These files will be placed in the Space's Workspace/{product_name}/ directory.
Meshes live in the Universe's assets/meshes/{product_name}/ directory.

Each part file uses this schema (lowercase keys matching InstanceDefinition):

```toml
[asset]
mesh = "assets/meshes/{product_name}/{component_name}.glb"
scene = "Scene0"

[transform]
position = [0.0, 1.0, -5.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [1.0, 1.0, 1.0]

[properties]
color = [0.8, 0.8, 0.8, 1.0]
transparency = 0.0
anchored = true
can_collide = true
cast_shadow = true

[metadata]
class_name = "Part"
archivable = true

[material]
name = "Al 6061-T6"
density = 2700.0
thermal_conductivity = 167.0
specific_heat = 896.0
young_modulus = 68900.0

[thermodynamic]
temperature = 293.15
pressure = 101325.0

[electrochemical]
# Only for electrochemically active components
```

IMPORTANT: Position the first/main component at [0.0, 1.0, -5.0] so it spawns in front of the camera.
Offset other components relative to the main one based on the assembly layout.

Generate one TOML block per mesh part. Separate each file with a comment line:
# --- FILE: {component_name}.part.toml ---

Use real material property values from the brief. Do not invent numbers.
Output ONLY the TOML content. Do not wrap in markdown code fences."#;

/// System prompt for README.md and Products.md catalog entry
const CATALOG_SYSTEM_PROMPT: &str = r#"You are a product catalog writer for the Eustress Engine.

Given an ideation brief (TOML), generate TWO outputs separated by the marker "---CATALOG_SEPARATOR---":

FIRST: README.md for the product directory containing:
1. Product name and one-line description
2. Innovation highlights with validation tiers
3. Target specifications table
4. Bill of materials table
5. Directory structure listing all generated files
6. How to load in Eustress Engine (load the .glb.toml files)
7. How to run the simulation (reference the Soul Script)
8. Version history

SECOND: A Products.md catalog entry (single row to append) in this format:
| {Name} | {Category} | {Tier} | {Key Spec} | {Innovation Count} | {Date} |

Output both sections. Do not wrap in code fences."#;

/// System prompt for Rune simulation script generation
/// Scripts go in SoulService/{product}/ for physics/chemistry simulation logic
const SIM_SCRIPTS_SYSTEM_PROMPT: &str = r#"You are a Rune simulation script generator for the Eustress Engine.

Given an ideation brief (TOML) and the EustressEngine_Requirements.md context, generate Rune scripts
that implement the simulation physics and chemistry for this product.

These scripts will be placed in the Space's SoulService/{product_name}/ directory.

The Rune scripting environment provides these APIs:
- `sim.time()` — current simulation time in seconds
- `sim.dt()` — timestep in simulation seconds
- `sim.is_running()` — check if simulation is active
- `sim.get("watchpoint_name")` — get current watchpoint value
- `sim.record("name", value)` — record a value to a watchpoint
- `sim.add_watchpoint("name", "label", "unit")` — register a new watchpoint
- `sim.add_breakpoint("name", "variable", ">", threshold)` — add simulation breakpoint
- `sim.set_time_scale(scale)` — adjust time compression
- `ecs.get_voltage("entity_name")` — get entity voltage
- `ecs.get_soc("entity_name")` — get state of charge
- `ecs.get_temperature("entity_name")` — get entity temperature
- `ecs.get_dendrite_risk("entity_name")` — get dendrite risk
- `ecs.get_sim("key")` — get simulation value by key
- `ecs.set_sim("key", value)` — set simulation value
- `log_info("message")`, `log_warn("message")`, `log_error("message")`

Generate the following scripts:

1. **{product_name}_simulation.rune** — Main simulation loop implementing the governing equations
   from the requirements. Register watchpoints on startup, compute physics each tick.

2. **{product_name}_fitness.rune** — Fitness scoring function that reads watchpoints and
   computes the fitness score per the requirements. Records "fitness_score" watchpoint.

Separate each file with:
# --- FILE: {filename}.rune ---

Use the real equations and constants from the brief. Match watchpoint names to the
brief's target_specs metrics. Output ONLY the Rune script content."#;

/// System prompt for ScreenGui UI TOML + Rune UI scripts generation
/// UI TOML goes in StarterGui/{product}/, scripts go in StarterGui/{product}/scripts/
const UI_GENERATION_SYSTEM_PROMPT: &str = r#"You are a UI generator for the Eustress Engine.

Given an ideation brief (TOML), generate:
1. A ScreenGui TOML file that defines the data feedback dashboard
2. A Rune UI script that wires simulation data to the UI elements

These files will be placed in the Space's StarterGui/{product_name}/ directory.

The ScreenGui TOML format uses the GuiElement .screengui.toml pattern:

```toml
# {product_name}_dashboard.screengui.toml
[metadata]
class_name = "ScreenGui"
archivable = true

[ui]
text = ""
background_color = [0.1, 0.1, 0.12, 0.85]
background_transparency = 0.15
border_color = [0.3, 0.3, 0.35, 1.0]
border_size = 1.0
position_x = 0.7
position_y = 0.02
size_x = 0.28
size_y = 0.45
anchor_point_x = 0.0
anchor_point_y = 0.0
z_index = 10
```

For child UI elements, generate .textlabel.toml and .frame.toml files:

```toml
# Title.textlabel.toml
[metadata]
class_name = "TextLabel"

[ui]
text = "Product Dashboard"
font_size = 18.0
text_color = [1.0, 1.0, 1.0, 1.0]
background_transparency = 1.0
size_x = 1.0
size_y = 0.08
```

```toml
# Voltage.textlabel.toml — updates via Rune script
[metadata]
class_name = "TextLabel"

[ui]
text = "Voltage: --"
font_size = 14.0
text_color = [0.7, 0.9, 1.0, 1.0]
background_transparency = 1.0
```

For the Rune UI script (placed in scripts/ subdirectory), use the ECS bindings API:
- `ecs.get_sim("watchpoint_name")` — read simulation values
- `ecs.get_voltage("entity")`, `ecs.get_temperature("entity")`, `ecs.get_pressure("entity")`, `ecs.get_property("entity", "key")`
- `log_info("message")` — debug logging
- The script runs each frame and updates UI text labels with current sim data

Generate these outputs separated by file markers:

1. **{product_name}_dashboard.screengui.toml** — Main ScreenGui container
2. **Title.textlabel.toml** — Dashboard title
3. One **.textlabel.toml** per key metric from the brief's target_specs
4. **scripts/{product_name}_ui.rune** — Script that reads sim data and updates labels

Separate each file with:
# --- FILE: {filename} ---

Output ONLY the file contents. Do not wrap in markdown code fences."#;

// ============================================================================
// 3. dispatch_artifact_request — spawn background thread for approved steps
// ============================================================================

/// Checks for approved artifact generation MCP commands and dispatches them
pub fn dispatch_artifact_requests(
    mut pipeline: ResMut<IdeationPipeline>,
    mut tasks: ResMut<WorkshopClaudeTasks>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
) {
    // Only dispatch artifact steps when we have a brief (post-normalization)
    if pipeline.brief.is_none() {
        return;
    }

    // Find the next approved MCP command for an artifact generation step
    let approved_step = pipeline.messages.iter().find_map(|m| {
        if m.role != super::MessageRole::Mcp
            || m.mcp_status != Some(McpCommandStatus::Approved)
        {
            return None;
        }

        // Match against artifact step endpoints
        // The endpoint is /mcp/ideation/brief with a step parameter embedded in content
        let endpoint = m.mcp_endpoint.as_deref()?;
        if endpoint != "/mcp/ideation/brief" {
            return None;
        }

        // Determine which step this is from the content
        let content = &m.content;
        for step_idx in 1u32..=8 {
            if let Some(step) = ArtifactStep::from_step_index(step_idx) {
                if content.contains(step.step_param()) {
                    return Some((m.id, step));
                }
            }
        }
        None
    });

    let (msg_id, step) = match approved_step {
        Some(pair) => pair,
        None => return,
    };

    // Mark as running
    pipeline.update_mcp_status(msg_id, McpCommandStatus::Running);
    pipeline.state = step.pipeline_state();

    // Update step status
    if let Some(pipeline_step) = pipeline.steps.get_mut(step.step_index() as usize) {
        pipeline_step.status = super::StepStatus::Active;
    }

    // Get API key
    let api_key = match (&global_settings, &space_settings) {
        (Some(global), Some(space)) => {
            let key = space.effective_api_key(global);
            if key.is_empty() { return; }
            key
        }
        _ => return,
    };

    // Build the prompt with the brief as context
    let brief_toml = pipeline.brief.as_ref()
        .and_then(|b| toml::to_string_pretty(b).ok())
        .unwrap_or_default();

    let prompt = format!(
        "Product ideation brief:\n\n```toml\n{}\n```\n\n\
         Conversation context:\n{}\n\n\
         Generate the {} artifact now.",
        brief_toml,
        pipeline.conversation_context,
        step.step_param()
    );

    // Create shared result container
    let result_container: Arc<Mutex<Option<Result<String, String>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = result_container.clone();

    let config = ClaudeConfig {
        api_key: Some(api_key),
        ..ClaudeConfig::default()
    };

    let system_prompt = step.system_prompt().to_string();

    // Spawn background thread
    std::thread::spawn(move || {
        let client = crate::soul::ClaudeClient::new(config);
        let result = client.call_api_for_workshop(&prompt, &system_prompt);

        match result_clone.lock() {
            Ok(mut lock) => *lock = Some(result),
            Err(poisoned) => {
                tracing::error!("Workshop: Mutex poisoned in artifact thread, recovering");
                *poisoned.into_inner() = Some(Err("Internal error: thread lock poisoned".to_string()));
            }
        }
    });

    // Track the in-flight request
    tasks.in_flight.push(super::claude_bridge::InFlightRequest::new(
        result_container,
        Some(step.step_index()),
        Some(msg_id),
        false,
    ));

    info!(
        "Workshop: Dispatched {} artifact generation (step {}, est. ${:.2})",
        step.step_param(),
        step.step_index(),
        step.estimated_cost()
    );
}

// ============================================================================
// 4. handle_artifact_response — write generated artifacts to disk
// ============================================================================

/// Processes a completed artifact generation response:
/// - Writes the generated content to the correct directory based on artifact type:
///   - Product docs (patent, SOTA, requirements, brief, catalog, Blender script)
///     → Space/Workspace/{product}/  (child of the product directory)
///   - Part TOML files → Space/Workspace/{product}/
///   - .glb mesh files → Universe/assets/meshes/{product}/ (via Blender script output dir)
///   - Rune sim scripts → Space/SoulService/{product}/
///   - UI TOML + UI scripts → Space/StarterGui/{product}/ (scripts in scripts/ subfolder)
/// - Adds an artifact message to the conversation
/// - Advances the pipeline to the next step (or proposes it as an MCP command)
pub fn handle_artifact_completion(
    mut events: MessageReader<ClaudeResponseEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
    space_root: Res<crate::space::SpaceRoot>,
) {
    for event in events.read() {
        // Only handle artifact step responses (step_index 1-8)
        let step_idx = match event.step_index {
            Some(idx) if idx >= 1 && idx <= 10 => idx,
            _ => continue,
        };

        let step = match ArtifactStep::from_step_index(step_idx) {
            Some(s) => s,
            None => continue,
        };

        let product_name = pipeline.product_name.clone();
        let safe_name = safe_product_name(&product_name);

        // Space directory — all product artifacts live in the Space filesystem
        let space_path = &space_root.0;

        // Product directory in Workspace — the primary product container
        // All doc artifacts (patent, SOTA, requirements, brief, README) are children of this
        let workspace_product_dir = space_path.join("Workspace").join(&safe_name);

        // Universe assets directory for shared mesh files
        let universe_assets_dir = resolve_universe_assets_dir(space_path, &safe_name);

        let output_path = match step {
            ArtifactStep::PartFiles => {
                // Part folder instances → Space/Workspace/{product}/{name}/_instance.toml
                write_split_part_folders(&workspace_product_dir, &event.content);
                workspace_product_dir.clone()
            }
            ArtifactStep::SimScripts => {
                // Rune simulation scripts → Space/SoulService/{name}/
                //
                // Each `# --- FILE: name ---` block lands as a folder
                // with `_instance.toml` + `{name}.rune` + `{name}.md`,
                // matching the MCP `eustress_create_script` tool's
                // output and the hand-authored `cycle_life_test`
                // reference. Previously these were flat `.rune` files
                // under a single product folder, which orphaned them
                // from the SoulService Explorer + rename/delete UI.
                let soul_dir = space_path.join("SoulService");
                write_split_rune_script_folders(&soul_dir, &event.content);
                soul_dir
            }
            ArtifactStep::UiGeneration => {
                // UI TOML + UI scripts → Space/StarterGui/{product}/
                // Scripts go in a scripts/ subfolder (handled by write_split_files path logic)
                let gui_product_dir = space_path.join("StarterGui").join(&safe_name);
                write_split_files(&gui_product_dir, &event.content, "");
                gui_product_dir
            }
            ArtifactStep::MeshGeneration => {
                // Blender script → Workspace/{product}/generate_meshes.py
                // Ensure Universe assets/meshes/{product}/ directory exists for script output
                let _ = std::fs::create_dir_all(&universe_assets_dir);
                let path = workspace_product_dir.join("generate_meshes.py");
                let _ = std::fs::create_dir_all(&workspace_product_dir);
                // Fence removed and the output directory injected (see `mesh_script_for`)
                let final_content = mesh_script_for(&event.content, &universe_assets_dir);
                write_artifact_file(&path, &final_content);
                path
            }
            ArtifactStep::CatalogEntry => {
                // README.md → Workspace/{product}/README.md
                let (readme, catalog_entry) = catalog_parts(&event.content);
                if let Some(readme) = readme {
                    let readme_path = workspace_product_dir.join("README.md");
                    write_artifact_file(&readme_path, &readme);
                }
                if let Some(catalog_entry) = catalog_entry {
                    // Catalog entry appended to Space-level Products.md
                    let catalog_path = space_path.join("Products.md");
                    append_catalog_entry_to(&catalog_path, &catalog_entry);
                }
                workspace_product_dir.join("README.md")
            }
            _ => {
                // Single-file artifacts in the product directory (patent, SOTA, requirements)
                let filename = step.output_filename();
                let path = workspace_product_dir.join(filename);
                let _ = std::fs::create_dir_all(&workspace_product_dir);
                write_artifact_file(&path, &event.content);
                path
            }
        };

        // Mark step as done and update artifact count
        if let Some(pipeline_step) = pipeline.steps.get_mut(step_idx as usize) {
            pipeline_step.status = super::StepStatus::Done;
            pipeline_step.artifact_count += 1;
        }

        // Add artifact message to the conversation
        pipeline.add_artifact_message(
            output_path.clone(),
            step.artifact_type(),
        );

        // Propose the next step as an MCP command (if there is one)
        let next_step_idx = step_idx + 1;
        if let Some(next_step) = ArtifactStep::from_step_index(next_step_idx) {
            let description = format!(
                "Generate {} (step={})\nEstimated cost: ~${:.2} (Sonnet)",
                pipeline.steps.get(next_step_idx as usize)
                    .map(|s| s.label.as_str())
                    .unwrap_or("next artifact"),
                next_step.step_param(),
                next_step.estimated_cost()
            );
            pipeline.add_mcp_command(
                description,
                "/mcp/ideation/brief".to_string(),
                "POST".to_string(),
                next_step.estimated_cost(),
            );
        } else {
            // All steps complete
            pipeline.state = IdeationState::Complete;
            pipeline.add_system_message(
                "All artifacts generated! Click \"Optimize & Build\" to hand off to the simulation loop (Systems 1-8).".to_string(),
                0.0,
            );
            info!("Workshop: Ideation pipeline complete for '{}'", pipeline.product_name);
        }
    }
}

// ============================================================================
// 5. File writing helpers
// ============================================================================

/// The Blender script as it is saved: the model's reply without a wrapping code fence
/// (a fence line in a `.py` file is a syntax error), with the Universe's mesh output
/// directory injected, or a header naming it when the script sets none.
fn mesh_script_for(reply: &str, universe_assets_dir: &std::path::Path) -> String {
    let script = super::normalizer::unwrap_fenced_document(reply);
    let patched = script.replace(
        "OUTPUT_DIR = ",
        &format!("OUTPUT_DIR = r\"{}\"  # ", universe_assets_dir.display()),
    );
    if patched == script {
        format!(
            "# Mesh output directory: {}\n# Run: blender --background --python generate_meshes.py\n\n{}",
            universe_assets_dir.display(),
            script
        )
    } else {
        patched
    }
}

/// The catalog reply split into its README and its catalog row, each without a wrapping
/// code fence: the whole reply may be fenced, or each part on its own.
fn catalog_parts(reply: &str) -> (Option<String>, Option<String>) {
    use super::normalizer::unwrap_fenced_document;
    let reply = unwrap_fenced_document(reply);
    let mut parts = reply.splitn(2, "---CATALOG_SEPARATOR---");
    let mut next = || parts.next().map(|p| unwrap_fenced_document(p.trim()).trim().to_string());
    let readme = next();
    let entry = next();
    (readme, entry)
}

/// A model told to write raw text sometimes wraps the whole reply in a code fence anyway.
/// Each artifact step that writes a reply to disk has to save the content, not the fence:
/// a fence line in a `.py` or `.toml` file is a syntax error, and in a document it is noise.
#[cfg(test)]
mod fence_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A folder in the temp directory, removed when dropped.
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "eustress_artifact_fences_{}_{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }
        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.0.join(rel)).unwrap_or_else(|e| format!("<{rel}: {e}>"))
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_fenced_patent_is_saved_without_its_fence() {
        let dir = Dir::new();
        let path = dir.0.join("PATENT.md");
        write_artifact_file(&path, "```markdown\n# Patent\n\nClaim 1: a cell.\n```");
        assert_eq!(dir.read("PATENT.md"), "# Patent\n\nClaim 1: a cell.");

        // A document that merely contains a code block is saved exactly as written.
        let doc = "# Patent\n\nFigure 1:\n```text\nfig\n```\n\nClaim 2.\n";
        write_artifact_file(&dir.0.join("SOTA.md"), doc);
        assert_eq!(dir.read("SOTA.md"), doc);
    }

    #[test]
    fn a_fenced_blender_script_is_saved_as_python() {
        let assets = std::path::Path::new("Universe").join("assets").join("meshes").join("Car");
        let shown = assets.display().to_string();

        // The script names its output directory: it is injected, and the fence is gone.
        let with_dir = mesh_script_for("```python\nimport bpy\nOUTPUT_DIR = \"out\"\nprint(1)\n```", &assets);
        assert!(!with_dir.contains("```"), "{with_dir}");
        assert!(with_dir.contains(&format!("OUTPUT_DIR = r\"{shown}\"  # \"out\"")), "{with_dir}");
        assert!(with_dir.starts_with("import bpy") && with_dir.ends_with("print(1)"), "{with_dir}");

        // It does not: a header names the directory, and the fence is still gone.
        let without = mesh_script_for("```python\nimport bpy\nprint(1)\n```", &assets);
        assert!(without.starts_with(&format!("# Mesh output directory: {shown}\n")), "{without}");
        assert!(without.ends_with("import bpy\nprint(1)") && !without.contains("```"), "{without}");

        // An unfenced script is unchanged by it.
        assert_eq!(
            mesh_script_for("import bpy\nprint(1)", &assets),
            format!("# Mesh output directory: {shown}\n# Run: blender --background --python generate_meshes.py\n\nimport bpy\nprint(1)")
        );
    }

    #[test]
    fn fenced_part_files_are_saved_as_toml() {
        let part = |name: &str| format!("[asset]\nmesh = \"assets/meshes/Car/{name}.glb\"");

        // The whole reply fenced, the file markers inside it.
        let dir = Dir::new();
        let whole = format!("```toml\n# --- FILE: Wheel ---\n{}\n# --- FILE: Body ---\n{}\n```", part("wheel"), part("body"));
        write_split_part_folders(&dir.0, &whole);
        assert_eq!(dir.read("Wheel/_instance.toml"), part("wheel"));
        assert_eq!(dir.read("Body/_instance.toml"), part("body"));

        // Each file fenced on its own.
        let dir = Dir::new();
        let each = format!(
            "# --- FILE: Wheel ---\n```toml\n{}\n```\n# --- FILE: Body ---\n```toml\n{}\n```",
            part("wheel"),
            part("body")
        );
        write_split_part_folders(&dir.0, &each);
        assert_eq!(dir.read("Wheel/_instance.toml"), part("wheel"));
        assert_eq!(dir.read("Body/_instance.toml"), part("body"));
    }

    #[test]
    fn fenced_rune_scripts_are_saved_as_source() {
        let source = "fn main() {\n    1 + 1\n}";
        let whole = format!("```rune\n# --- FILE: cycle ---\n{source}\n```");
        let dir = Dir::new();
        write_split_rune_script_folders(&dir.0, &whole);
        assert_eq!(dir.read("cycle/cycle.rune"), source);

        let each = format!("# --- FILE: cycle ---\n```rune\n{source}\n```\n# --- FILE: cycle.md ---\n```markdown\n# Cycle\n```");
        let dir = Dir::new();
        write_split_rune_script_folders(&dir.0, &each);
        assert_eq!(dir.read("cycle/cycle.rune"), source);
        assert_eq!(dir.read("cycle/cycle.md"), "# Cycle");
    }

    #[test]
    fn fenced_ui_files_are_saved_as_toml() {
        let ui = "[ScreenGui]\nname = \"Hud\"";
        let dir = Dir::new();
        write_split_files(&dir.0, &format!("```toml\n# --- FILE: hud.toml ---\n{ui}\n```"), "");
        assert_eq!(dir.read("hud.toml"), ui);
        let dir = Dir::new();
        write_split_files(&dir.0, &format!("# --- FILE: hud.toml ---\n```toml\n{ui}\n```"), "");
        assert_eq!(dir.read("hud.toml"), ui);
    }

    #[test]
    fn a_fenced_catalog_reply_is_split_without_fences() {
        let (readme, entry) = catalog_parts("```markdown\n# README\n\nA cell.\n---CATALOG_SEPARATOR---\n| Cell | conventional |\n```");
        assert_eq!(readme.as_deref(), Some("# README\n\nA cell."));
        assert_eq!(entry.as_deref(), Some("| Cell | conventional |"));

        let (readme, entry) = catalog_parts("```markdown\n# README\n```\n---CATALOG_SEPARATOR---\n```markdown\n| Cell |\n```");
        assert_eq!(readme.as_deref(), Some("# README"));
        assert_eq!(entry.as_deref(), Some("| Cell |"));

        // No separator: only a README. No fences: as before.
        assert_eq!(catalog_parts("# README only"), (Some("# README only".to_string()), None));
        let (readme, entry) = catalog_parts("# README\n---CATALOG_SEPARATOR---\n| Cell |");
        assert_eq!((readme.as_deref(), entry.as_deref()), (Some("# README"), Some("| Cell |")));
    }
}

/// Write a single artifact file to disk.
///
/// A model told to write raw text sometimes wraps the whole reply in a code fence anyway, and a
/// fence line in a `.py` or `.toml` file is a syntax error, so a reply that is one fenced block is
/// saved without it. Every file this module writes goes through here, the files of a split
/// reply included.
fn write_artifact_file(path: &PathBuf, content: &str) {
    let content = super::normalizer::unwrap_fenced_document(content);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            warn!("Workshop: Failed to create directory {:?}: {}", parent, e);
            return;
        }
    }
    match std::fs::write(path, content) {
        Ok(_) => info!("Workshop: Wrote artifact {:?}", path),
        Err(e) => warn!("Workshop: Failed to write {:?}: {}", path, e),
    }
}

/// Write multiple files from a single Claude response using "# --- FILE: {name} ---" markers.
///
/// `output_dir` — base directory for all files (created if missing).
/// `default_extension` — appended to filenames that lack an extension (e.g. ".part.toml").
///   Pass "" to leave filenames as-is.
///
/// Files whose name starts with "scripts/" will be placed in an output_dir/scripts/ subdirectory.
fn write_split_files(output_dir: &PathBuf, content: &str, default_extension: &str) {
    let _ = std::fs::create_dir_all(output_dir);
    // The whole reply may be fenced, with the file markers inside; each file is unwrapped
    // again when it is written, for a reply that fences them one by one.
    let content = super::normalizer::unwrap_fenced_document(content);

    let mut current_filename: Option<String> = None;
    let mut current_content = String::new();

    for line in content.lines() {
        if line.starts_with("# --- FILE:") && line.ends_with("---") {
            // Write previous file if any
            if let Some(ref filename) = current_filename {
                let path = resolve_split_file_path(output_dir, filename, default_extension);
                write_artifact_file(&path, current_content.trim());
            }
            // Extract new filename
            let name = line
                .trim_start_matches("# --- FILE:")
                .trim_end_matches("---")
                .trim();
            current_filename = Some(name.to_string());
            current_content.clear();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Write last file
    if let Some(ref filename) = current_filename {
        let path = resolve_split_file_path(output_dir, filename, default_extension);
        write_artifact_file(&path, current_content.trim());
    }
}

/// Write split Rune scripts as folder-based instances, matching the
/// convention used by the MCP `eustress_create_script` tool and the
/// hand-authored `cycle_life_test` reference folder:
///
/// ```text
/// {output_dir}/{name}/
///   ├─ _instance.toml      # class_name = "Script", source = "{name}.rune"
///   ├─ {name}.rune         # Rune source code
///   └─ {name}.md           # Summary (placeholder if Claude didn't emit one)
/// ```
///
/// Previously this path called `write_split_files(…, ".rune")` which
/// flattened each block into a bare `{name}.rune` file — no
/// `_instance.toml`, no summary, no folder. That produced scripts the
/// SoulService loader saw as orphan files and the Explorer couldn't
/// route through the normal script-folder UI (rename / delete / tab
/// open all assumed the folder shape). Matches
/// [`crate::ui::center_tabs::script_source_path_canonical`] +
/// `script_summary_path_canonical` so subsequent reads round-trip.
///
/// Each `# --- FILE: name ---` block becomes one script folder. If a
/// companion `# --- FILE: name.md ---` block exists it becomes the
/// `.md` summary; otherwise a minimal "# {name}\n\n(no summary)\n"
/// placeholder is written so the file exists and the UI doesn't hit
/// the "missing file" branch.
fn write_split_rune_script_folders(output_dir: &PathBuf, content: &str) {
    let _ = std::fs::create_dir_all(output_dir);
    // The whole reply may be fenced, with the file markers inside; each file is unwrapped
    // again when it is written, for a reply that fences them one by one.
    let content = super::normalizer::unwrap_fenced_document(content);

    // First pass: collect every (filename, body) pair from the stream,
    // so we can pair `foo` with its companion `foo.md` regardless of
    // emit order.
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_content = String::new();
    for line in content.lines() {
        if line.starts_with("# --- FILE:") && line.ends_with("---") {
            if let Some(name) = current_name.take() {
                blocks.push((name, current_content.trim().to_string()));
            }
            let raw = line
                .trim_start_matches("# --- FILE:")
                .trim_end_matches("---")
                .trim();
            current_name = Some(raw.to_string());
            current_content.clear();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }
    if let Some(name) = current_name.take() {
        blocks.push((name, current_content.trim().to_string()));
    }

    // Bucket by script stem. Markdown blocks land on `.md`; everything
    // else (bare name or explicit `.rune`) is the source body. First
    // writer wins for each bucket — Claude has been known to repeat a
    // header, and we don't want a later empty block to clobber good
    // content.
    use std::collections::BTreeMap;
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    let mut summaries: BTreeMap<String, String> = BTreeMap::new();
    for (raw_name, body) in blocks {
        let n = raw_name.trim().trim_start_matches('/').to_string();
        let n = n.trim_start_matches("scripts/").to_string();
        if let Some(stem) = n.strip_suffix(".md") {
            summaries
                .entry(stem.to_string())
                .or_insert_with(|| body);
        } else {
            let stem = n
                .strip_suffix(".rune")
                .map(|s| s.to_string())
                .unwrap_or(n);
            sources.entry(stem).or_insert_with(|| body);
        }
    }

    for (stem, body) in sources {
        let Some(safe) = sanitize_script_folder_name(&stem) else {
            warn!(
                "Workshop: skipping unsafe rune script name {:?}",
                stem
            );
            continue;
        };
        let folder = output_dir.join(&safe);
        let _ = std::fs::create_dir_all(&folder);

        let rune_path = folder.join(format!("{safe}.rune"));
        let md_path = folder.join(format!("{safe}.md"));
        let instance_path = folder.join("_instance.toml");

        write_artifact_file(&rune_path, &body);

        let summary = summaries
            .get(&stem)
            .cloned()
            .unwrap_or_else(|| format!("# {safe}\n\n(no summary)\n"));
        write_artifact_file(&md_path, &summary);

        let instance_toml = format!(
            "[metadata]\nclass_name = \"Script\"\narchivable = true\n\n[script]\nsource = \"{safe}.rune\"\n"
        );
        write_artifact_file(&instance_path, &instance_toml);
    }
}

/// Accept-list sanitizer for Rune script folder names. Mirrors the
/// MCP tool's check (`create_script_handler` rejects names with
/// non-[A-Za-z0-9_-] bytes) so Workshop-generated folders match the
/// shape an MCP-authored one would produce.
fn sanitize_script_folder_name(raw: &str) -> Option<String> {
    let stem = raw.trim().trim_matches(|c: char| c == '/' || c == '\\');
    if stem.is_empty() {
        return None;
    }
    // Strip any trailing extension that snuck through.
    let stem = stem
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(stem);
    let cleaned: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim_matches('_').to_string();
    if cleaned.is_empty() {
        return None;
    }
    // Must start with letter or underscore, matching the MCP rule.
    if !cleaned
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_')
        .unwrap_or(false)
    {
        return Some(format!("_{cleaned}"));
    }
    Some(cleaned)
}

/// Write split Part files as folder-based instances.
/// Each "# --- FILE: name ---" block creates `output_dir/{name}/_instance.toml`.
///
/// Root-cause note (2026-04): Claude artifact output occasionally emitted
/// filename markers whose sanitized form collided with EEP reserved
/// names (`_instance.toml`, `_service.toml`) or left a bare `.toml`
/// suffix intact. That produced a directory literally named
/// `_instance.toml` (or `Block.toml`) under the Workspace, which the
/// file-loader then surfaced as a phantom child entity in the Explorer.
/// Guarded against both cases in `sanitize_part_folder_name` — names
/// that can't be represented as a folder are dropped with a warning
/// rather than written with a corrupted shape.
fn write_split_part_folders(output_dir: &PathBuf, content: &str) {
    let _ = std::fs::create_dir_all(output_dir);
    // The whole reply may be fenced, with the file markers inside; each file is unwrapped
    // again when it is written, for a reply that fences them one by one.
    let content = super::normalizer::unwrap_fenced_document(content);

    let mut current_name: Option<String> = None;
    let mut current_content = String::new();

    for line in content.lines() {
        if line.starts_with("# --- FILE:") && line.ends_with("---") {
            if let Some(ref name) = current_name {
                if let Some(safe) = sanitize_part_folder_name(name) {
                    let part_dir = output_dir.join(&safe);
                    let _ = std::fs::create_dir_all(&part_dir);
                    write_artifact_file(&part_dir.join("_instance.toml"), current_content.trim());
                }
            }
            let name = line.trim_start_matches("# --- FILE:").trim_end_matches("---").trim();
            current_name = Some(name.to_string());
            current_content.clear();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if let Some(ref name) = current_name {
        if let Some(safe) = sanitize_part_folder_name(name) {
            let part_dir = output_dir.join(&safe);
            let _ = std::fs::create_dir_all(&part_dir);
            write_artifact_file(&part_dir.join("_instance.toml"), current_content.trim());
        }
    }
}

/// Normalize a Claude-generated filename marker into a safe Part folder
/// name, or `None` if the marker can't be represented as one.
///
/// Rejects:
/// * Empty / whitespace-only names
/// * Reserved EEP markers (`_instance.toml`, `_service.toml`) — these
///   MUST be files inside a folder, never folder names themselves.
///   Writing one as a folder is what produced the "phantom
///   `_instance.toml` child entity" corruption.
/// * Anything that reduces to an empty string after stripping.
///
/// Accepts and strips every known TOML tail (`.part.toml`, `.glb.toml`,
/// and plain `.toml`) so `Block`, `Block.toml`, and `Block.part.toml`
/// all resolve to the same folder name `Block`.
fn sanitize_part_folder_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return None; }

    // Reject EEP reserved names BEFORE stripping so something like
    // `_instance.toml` (already the final form) can't slip through.
    let lower = trimmed.to_ascii_lowercase();
    if lower == "_instance.toml" || lower == "_service.toml"
        || lower == "_instance" || lower == "_service"
    {
        tracing::warn!(
            "🚫 Workshop artifact filename '{}' is an EEP reserved name — refusing to write as a folder.",
            raw
        );
        return None;
    }

    let safe = trimmed.replace(' ', "_").replace('/', "_").replace('\\', "_");
    // Strip TOML suffixes in decreasing-specificity order so `Block.glb.toml`
    // doesn't get chopped to `Block.glb` by an eager `.toml` match first.
    let stripped = safe
        .strip_suffix(".part.toml")
        .or_else(|| safe.strip_suffix(".glb.toml"))
        .or_else(|| safe.strip_suffix(".instance.toml"))
        .or_else(|| safe.strip_suffix(".toml"))
        .unwrap_or(&safe)
        .to_string();

    if stripped.is_empty() || stripped.starts_with('.') {
        tracing::warn!(
            "🚫 Workshop artifact filename '{}' sanitizes to an invalid folder name — skipping.",
            raw
        );
        return None;
    }
    Some(stripped)
}

/// Resolve the full path for a split file, handling scripts/ subdirectory and default extensions.
fn resolve_split_file_path(output_dir: &PathBuf, filename: &str, default_extension: &str) -> PathBuf {
    let mut name = filename.to_string();

    // Append default extension if the filename doesn't already have a recognized extension
    if !default_extension.is_empty() && !name.contains('.') {
        name.push_str(default_extension);
    }

    // Files with path separators (e.g. "scripts/foo.rune") are placed relative to output_dir
    let path = output_dir.join(&name);

    // Ensure parent directory exists (handles scripts/ subdirectory)
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    path
}

/// Resolve the Universe assets/meshes/{product}/ directory from a Space root path.
///
/// Mesh assets live at `<Universe>/assets/meshes/{product}/`: the folder export
/// ships, and the one the loader resolves an `assets/...` part path against.
/// Earlier builds wrote them under `.eustress/assets/meshes/`, where nothing
/// found them (see `product_meshes`).
fn resolve_universe_assets_dir(space_root: &std::path::Path, safe_product_name: &str) -> PathBuf {
    super::product_meshes::product_mesh_dir(space_root, safe_product_name)
}

/// Once per Space open: move the Workshop's product meshes out of
/// `<Universe>/.eustress/assets/meshes/`, where earlier builds wrote them and
/// nothing found them, to `<Universe>/assets/meshes/`. `product_meshes` says what
/// moves and what stays. A single directory lookup when there is nothing to move.
pub fn migrate_product_meshes_on_space_open(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut migrated_for: Local<Option<PathBuf>>,
) {
    let Some(space_root) = space_root else { return };
    if migrated_for.as_ref() == Some(&space_root.0) {
        return;
    }
    *migrated_for = Some(space_root.0.clone());

    let universe_root = super::product_meshes::universe_root_of(&space_root.0);
    let report = super::product_meshes::migrate_product_meshes(&universe_root);
    if report.moved_files > 0 {
        info!(
            "Workshop: moved {} mesh file(s) of {} to {}",
            report.moved_files,
            report.moved_products.join(", "),
            universe_root.join("assets").join("meshes").display()
        );
    }
    for product in &report.pinned_products {
        info!(
            "Workshop: left the meshes of '{product}' in .eustress/assets/meshes, its parts name that path"
        );
    }
    for path in &report.kept {
        warn!(
            "Workshop: could not move {} (a file is already at its destination, or it is a link)",
            path.display()
        );
    }
}

/// Sanitize a product name for use as a directory name
fn safe_product_name(product_name: &str) -> String {
    product_name
        .replace(' ', "_")
        .replace('/', "_")
        .replace('\\', "_")
        .replace(':', "_")
}

/// System prompt for DEAL_STRUCTURE.md generation
const DEAL_STRUCTURE_SYSTEM_PROMPT: &str = r#"You are a manufacturing deal structuring advisor for the Eustress Manufacturing Program.

Given the ideation brief (TOML), generate a DEAL_STRUCTURE.md that defines the equity distribution and royalty terms for bringing this product to market through the Eustress Manufacturing Program.

## What DEAL_STRUCTURE.md must contain:

### 1. Executive Summary
- Product name and version
- Deal type (Manufacturing Program partnership)
- Term sheet validity period
- One-paragraph summary of the deal

### 2. Equity Distribution Table
A precise table showing ALL stakeholders with their percentage. Must sum to exactly 100%.

Standard Eustress Manufacturing Program template:
| Stakeholder | Role | Equity % | Vesting |
|---|---|---|---|
| Inventor | IP owner and product creator | 60% | Immediate |
| Eustress Manufacturing Program | Manufacturing fund, infrastructure, distribution | 25% | Immediate |
| Logistics Partner | 3PL, warehousing, fulfillment operations | 10% | 12-month cliff, 24-month vest |
| Reserve Pool | Future co-investors, advisors, strategic partners | 5% | Board discretion |

Adjust percentages based on: product complexity, manufacturing capital required, IP strength, and market readiness indicated in the brief.

### 3. Royalty Structure
- **Manufacturing Program Royalty**: X% of net sales flows back to the Manufacturing Program fund. This funds future pilot programs, warehousing expansion, and new inventor onboarding. Default: 8% of net sales.
- **Inventor Royalty**: Y% of net sales retained by inventor above and beyond equity. Default: 5% of net sales.
- Clearly state what "net sales" means (gross revenue minus returns, chargebacks, and sales tax).

### 4. Unit Economics
| Metric | Value |
|---|---|
| Suggested Retail Price | $X.XX |
| Estimated Unit Cost (BOM + assembly + logistics) | $X.XX |
| Gross Margin per Unit | $X.XX (XX%) |
| Manufacturing Program royalty per unit | $X.XX |
| Inventor royalty per unit | $X.XX |
| Net to equity pool per unit | $X.XX |

Base unit cost on the BOM entries in the ideation brief. Add 35% for assembly labor, 15% for logistics/shipping, 10% for returns reserve.

### 5. Pilot Program Terms
- Minimum pilot batch: X units (minimum 500, recommend 1,000 for electronics)
- Pilot geography: target region based on product type
- Pilot duration: X weeks
- Go/no-go criteria for full production unlock

### 6. Intellectual Property Terms
- IP remains owned by the inventor
- Eustress Manufacturing Program receives an exclusive manufacturing license for the pilot period
- License converts to non-exclusive after pilot if production targets are met
- Patent filing costs split 50/50 between inventor and Manufacturing Program

### 7. Governance
- Product decisions during pilot: inventor has final say
- Manufacturing decisions: Eustress Manufacturing Program has final say
- Pricing decisions: joint approval required
- Dispute resolution: binding arbitration, jurisdiction: [TBD by parties]

### 8. Exit Terms
- Inventor buyout: inventor may buy out Manufacturing Program stake at 3× invested capital after 24 months
- Manufacturing Program exit: may sell stake to pre-approved third parties with inventor right-of-first-refusal

Output ONLY the DEAL_STRUCTURE.md content. No preamble. No explanation. Use realistic numbers from the BOM and product specs in the brief.
"#;

/// System prompt for LOGISTICS_PLAN.md generation
const LOGISTICS_PLAN_SYSTEM_PROMPT: &str = r#"You are a logistics and supply chain planner for the Eustress Manufacturing Program.

Given the ideation brief (TOML), generate a LOGISTICS_PLAN.md covering the full logistics pipeline from pilot through production: pilot program design, warehousing strategy, fulfillment operations, and regulatory requirements.

## What LOGISTICS_PLAN.md must contain:

### 1. Phase Overview
A three-phase timeline table:
| Phase | Name | Duration | Units | Goal |
|---|---|---|---|---|
| Phase 1 | Pilot | 12 weeks | 500–1,000 | Validate market fit, gather telemetry |
| Phase 2 | Limited Production | 6 months | 5,000–10,000 | Optimize fulfillment, reduce unit cost |
| Phase 3 | Full Production | Ongoing | 10,000+/mo | Scale with demand signals |

### 2. Pilot Program Design
- **Batch size**: Recommend based on product type and BOM cost
- **Target segment**: Specific customer profile (job title, industry, use case)
- **Distribution channels**: Direct DTC, Amazon, specialty retailers, B2B
- **Launch approach**: Pre-order campaign, waitlist, beta program, or direct sale
- **Feedback collection**: In-app telemetry (if IoT product), surveys, interviews, return analysis
- **Success criteria**: Specific measurable go/no-go gates (e.g., "<5% return rate", ">4.2 stars average", ">80% repurchase intent")
- **Kill conditions**: When to stop the pilot and redesign

### 3. Warehousing Strategy
Recommend a warehousing model based on product characteristics:
- **Model options**: 3PL (ShipBob, Flexport), Amazon FBA, own warehouse, consignment
- **Primary recommendation** with justification
- **Geographic nodes**: Which regions need stock (based on pilot geography)
- **Inventory parameters**:
  - Safety stock level
  - Reorder point (units)
  - Reorder quantity (economic order quantity calculation)
  - Lead time from manufacturer to warehouse
- **Storage requirements**: Temperature, humidity, hazmat if applicable
- **Estimated monthly warehousing cost** per SKU

### 4. Fulfillment Operations
- **Primary fulfillment partner** with justification
- **Backup partner** for redundancy
- **Shipping speeds offered**: Standard / Express / Overnight with carrier
- **Ship-to countries** for pilot vs full production
- **Average order fulfillment cost** (pick + pack + ship)
- **Returns/RMA process**: How returns are handled, restocked, or disposed

### 5. Supply Chain Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Component shortage | Medium | High | Dual-source critical BOM parts |
| Manufacturer delay | Low | High | 8-week buffer stock maintained |
| Customs delay | Low | Medium | Incoterms DDP for key markets |
| Demand spike | Medium | Medium | Pre-negotiated surge capacity with 3PL |

### 6. Regulatory and Customs
- Import classifications (HTS codes) for target markets
- Required certifications before shipping (FCC, CE, RoHS, UL, REACH — infer from product type and target market)
- Country-of-origin documentation requirements
- Any restricted goods considerations

### 7. Technology Stack
- Order management system recommendation
- Inventory tracking method (barcode, RFID, IoT serial — match to product)
- Integration with Eustress Workshop telemetry feed for IoT products
- Customer-facing tracking portal

### 8. Cost Summary Table
| Cost Category | Pilot (per unit) | Production (per unit) |
|---|---|---|
| Manufacturing | $X.XX | $X.XX |
| Inbound freight | $X.XX | $X.XX |
| Warehousing (allocated) | $X.XX | $X.XX |
| Pick + pack | $X.XX | $X.XX |
| Outbound shipping | $X.XX | $X.XX |
| Returns reserve | $X.XX | $X.XX |
| **Total landed cost** | **$X.XX** | **$X.XX** |

Base all costs on the BOM unit cost from the ideation brief. Inbound freight: 8% of unit cost. Warehousing: $0.50/unit/month. Pick+pack: $2.50 standard. Outbound shipping: $5–8 domestic.

Output ONLY the LOGISTICS_PLAN.md content. No preamble. No explanation. Use realistic numbers derived from the product BOM and specs.
"#;

/// Append a catalog entry to a Products.md file at the given path (create if not exists)
fn append_catalog_entry_to(catalog_path: &PathBuf, entry: &str) {
    if !catalog_path.exists() {
        let header = "# Product Catalog\n\n\
                      | Name | Category | Tier | Key Spec | Innovations | Date |\n\
                      |------|----------|------|----------|-------------|------|\n";
        let content = format!("{}{}\n", header, entry);
        write_artifact_file(catalog_path, &content);
    } else {
        match std::fs::read_to_string(catalog_path) {
            Ok(existing) => {
                let updated = format!("{}\n{}\n", existing.trim_end(), entry);
                write_artifact_file(catalog_path, &updated);
            }
            Err(e) => warn!("Workshop: Failed to read {:?}: {}", catalog_path, e),
        }
    }
}
