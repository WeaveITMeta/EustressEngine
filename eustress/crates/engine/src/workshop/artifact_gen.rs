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
   - Time integration method (explicit Euler, RK4, etc.)
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
- `ecs.get_voltage("entity")`, `ecs.get_temperature("entity")`, etc.
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

        if let Ok(mut lock) = result_clone.lock() {
            *lock = Some(result);
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
            Some(idx) if idx >= 1 && idx <= 8 => idx,
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
                // Part .toml files → Space/Workspace/{product}/
                write_split_files(&workspace_product_dir, &event.content, ".part.toml");
                workspace_product_dir.clone()
            }
            ArtifactStep::SimScripts => {
                // Rune simulation scripts → Space/SoulService/{product}/
                let soul_product_dir = space_path.join("SoulService").join(&safe_name);
                write_split_files(&soul_product_dir, &event.content, ".rune");
                soul_product_dir
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
                // Inject the correct output directory into the script content
                let patched_content = event.content.replace(
                    "OUTPUT_DIR = ",
                    &format!("OUTPUT_DIR = r\"{}\"  # ", universe_assets_dir.display()),
                );
                let final_content = if patched_content == event.content {
                    format!(
                        "# Mesh output directory: {}\n# Run: blender --background --python generate_meshes.py\n\n{}",
                        universe_assets_dir.display(),
                        event.content
                    )
                } else {
                    patched_content
                };
                write_artifact_file(&path, &final_content);
                path
            }
            ArtifactStep::CatalogEntry => {
                // README.md → Workspace/{product}/README.md
                let parts: Vec<&str> = event.content.splitn(2, "---CATALOG_SEPARATOR---").collect();
                if let Some(readme) = parts.first() {
                    let readme_path = workspace_product_dir.join("README.md");
                    write_artifact_file(&readme_path, readme.trim());
                }
                if let Some(catalog_entry) = parts.get(1) {
                    // Catalog entry appended to Space-level Products.md
                    let catalog_path = space_path.join("Products.md");
                    append_catalog_entry_to(&catalog_path, catalog_entry.trim());
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

/// Write a single artifact file to disk
fn write_artifact_file(path: &PathBuf, content: &str) {
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
/// Space layout: Universe/spaces/SpaceName/ → Universe is two levels up.
/// Mesh assets live at Universe/assets/meshes/{product}/.
fn resolve_universe_assets_dir(space_root: &std::path::Path, safe_product_name: &str) -> PathBuf {
    // Try standard layout: Universe/spaces/SpaceName/
    let universe_root = if let Some(spaces_dir) = space_root.parent() {
        if spaces_dir.file_name().map(|n| n == "spaces").unwrap_or(false) {
            // Standard: space_root.parent() = "spaces", one more up = Universe root
            spaces_dir.parent().unwrap_or(space_root).to_path_buf()
        } else {
            // Legacy: Space is directly inside Universe (no "spaces" intermediary)
            spaces_dir.to_path_buf()
        }
    } else {
        space_root.to_path_buf()
    };

    universe_root.join("assets").join("meshes").join(safe_product_name)
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
- Required certifications before shipping (FCC, CE, RoHS, etc. — infer from product type)
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
