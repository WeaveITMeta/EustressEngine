//! # Artifact Generation — Per-Step System Prompts and Dispatch
//!
//! Each pipeline step after normalization generates a specific artifact type.
//! This module defines the system prompts, output file paths, and dispatch logic
//! for each of the 6 artifact generation steps.
//!
//! ## Table of Contents
//!
//! 1. ArtifactStep enum — maps pipeline step indices to generation logic
//! 2. Per-step system prompts — patent, SOTA, requirements, meshes, instances, catalog
//! 3. dispatch_artifact_request — spawns background thread for approved artifact steps
//! 4. handle_artifact_response — processes completed artifact generation (write to disk)
//!
//! ## Architecture
//!
//! - Each step requires an approved MCP command before dispatching
//! - The brief TOML (from normalization) is included as context in every prompt
//! - Generated artifacts are written to docs/Products/{ProductName}/
//! - Each step fires a ClaudeResponseEvent with the step_index set
//! - Mesh generation (step 4) is special: it generates Blender Python scripts, not markdown

use bevy::prelude::*;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use eustress_common::soul::ClaudeConfig;

use super::{
    IdeationPipeline, IdeationState, ClaudeResponseEvent,
    McpCommandStatus, ArtifactType, normalizer,
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
    /// Step 5: Generate .glb.toml instance files
    InstanceFiles,
    /// Step 6: Generate README.md and update Products.md catalog
    CatalogEntry,
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
            5 => Some(Self::InstanceFiles),
            6 => Some(Self::CatalogEntry),
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
            Self::InstanceFiles => 5,
            Self::CatalogEntry => 6,
        }
    }

    /// The IdeationState that corresponds to this step being active
    pub fn pipeline_state(&self) -> IdeationState {
        match self {
            Self::Patent => IdeationState::GeneratingPatent,
            Self::SotaValidation => IdeationState::GeneratingSotaValidation,
            Self::Requirements => IdeationState::GeneratingRequirements,
            Self::MeshGeneration => IdeationState::GeneratingMeshes,
            Self::InstanceFiles => IdeationState::GeneratingInstances,
            Self::CatalogEntry => IdeationState::FinalizingCatalog,
        }
    }

    /// MCP endpoint step parameter value
    pub fn step_param(&self) -> &'static str {
        match self {
            Self::Patent => "patent",
            Self::SotaValidation => "sota",
            Self::Requirements => "requirements",
            Self::MeshGeneration => "meshes",
            Self::InstanceFiles => "instances",
            Self::CatalogEntry => "catalog",
        }
    }

    /// Output filename relative to the product directory
    pub fn output_filename(&self) -> &'static str {
        match self {
            Self::Patent => "PATENT.md",
            Self::SotaValidation => "SOTA_VALIDATION.md",
            Self::Requirements => "EustressEngine_Requirements.md",
            Self::MeshGeneration => "V1/meshes/generate_meshes.py",
            Self::InstanceFiles => "V1/",  // Directory — multiple .glb.toml files
            Self::CatalogEntry => "README.md",
        }
    }

    /// ArtifactType for chat messages
    pub fn artifact_type(&self) -> ArtifactType {
        match self {
            Self::Patent => ArtifactType::Patent,
            Self::SotaValidation => ArtifactType::Sota,
            Self::Requirements => ArtifactType::Requirements,
            Self::MeshGeneration => ArtifactType::Mesh,
            Self::InstanceFiles => ArtifactType::Toml,
            Self::CatalogEntry => ArtifactType::Catalog,
        }
    }

    /// Estimated BYOK cost for this step
    pub fn estimated_cost(&self) -> f64 {
        match self {
            Self::Patent => 0.05,
            Self::SotaValidation => 0.04,
            Self::Requirements => 0.04,
            Self::MeshGeneration => 0.03,
            Self::InstanceFiles => 0.02,
            Self::CatalogEntry => 0.01,
        }
    }

    /// Get the system prompt for this artifact step
    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::Patent => PATENT_SYSTEM_PROMPT,
            Self::SotaValidation => SOTA_SYSTEM_PROMPT,
            Self::Requirements => REQUIREMENTS_SYSTEM_PROMPT,
            Self::MeshGeneration => MESH_SYSTEM_PROMPT,
            Self::InstanceFiles => INSTANCE_SYSTEM_PROMPT,
            Self::CatalogEntry => CATALOG_SYSTEM_PROMPT,
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

/// System prompt for .glb.toml instance file generation
const INSTANCE_SYSTEM_PROMPT: &str = r#"You are an instance file generator for the Eustress Engine.

Given an ideation brief (TOML) and the list of generated mesh files, generate .glb.toml instance files for each mesh part. Each instance file uses this PascalCase schema:

```toml
[Asset]
GlbPath = "meshes/{component_name}.glb"

[Transform]
Position = [0.0, 0.0, 0.0]
Rotation = [0.0, 0.0, 0.0, 1.0]
Scale = [1.0, 1.0, 1.0]

[Properties]
ClassName = "{ComponentName}"
DisplayName = "{Human Readable Name}"
# Add component-specific properties from the brief

[Material]
BaseColor = [0.8, 0.8, 0.8, 1.0]
Metallic = 0.0
Roughness = 0.5

[Thermodynamic]
Temperature = 293.15
SpecificHeat = 897.0
ThermalConductivity = 237.0
Density = 2700.0

[Electrochemical]
# Only for electrochemically active components
IonicConductivity = 0.003
VoltageWindow = [0.0, 5.0]
```

Generate one TOML block per mesh part. Separate each file with a comment line:
# --- FILE: {component_name}.glb.toml ---

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
        for step_idx in 1u32..=6 {
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
/// - Writes the generated content to the product directory
/// - Adds an artifact message to the conversation
/// - Advances the pipeline to the next step (or proposes it as an MCP command)
pub fn handle_artifact_completion(
    mut events: MessageReader<ClaudeResponseEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    for event in events.read() {
        // Only handle artifact step responses (step_index 1-6)
        let step_idx = match event.step_index {
            Some(idx) if idx >= 1 && idx <= 6 => idx,
            _ => continue,
        };

        let step = match ArtifactStep::from_step_index(step_idx) {
            Some(s) => s,
            None => continue,
        };

        // Determine output path
        let product_name = pipeline.product_name.clone();
        let output_dir = normalizer::product_output_dir(std::path::Path::new("."), &product_name);

        let output_path = match step {
            ArtifactStep::InstanceFiles => {
                // Instance files: split by file separator and write each one
                write_instance_files(&output_dir, &event.content);
                output_dir.join("V1")
            }
            ArtifactStep::CatalogEntry => {
                // Split README.md and Products.md entry
                let parts: Vec<&str> = event.content.splitn(2, "---CATALOG_SEPARATOR---").collect();
                if let Some(readme) = parts.first() {
                    let readme_path = output_dir.join("README.md");
                    write_artifact_file(&readme_path, readme.trim());
                }
                if let Some(catalog_entry) = parts.get(1) {
                    // Append to Products.md at project root
                    append_catalog_entry(catalog_entry.trim());
                }
                output_dir.join("README.md")
            }
            _ => {
                // Standard single-file artifacts (patent, SOTA, requirements, mesh script)
                let filename = step.output_filename();
                let path = output_dir.join(filename);
                // Create parent directories if needed
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
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

/// Write multiple instance files from a single response using file separators
fn write_instance_files(output_dir: &PathBuf, content: &str) {
    let v1_dir = output_dir.join("V1");
    let _ = std::fs::create_dir_all(&v1_dir);

    // Split on "# --- FILE: {name} ---" markers
    let mut current_filename: Option<String> = None;
    let mut current_content = String::new();

    for line in content.lines() {
        if line.starts_with("# --- FILE:") && line.ends_with("---") {
            // Write previous file if any
            if let Some(ref filename) = current_filename {
                let path = v1_dir.join(filename);
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
        let path = v1_dir.join(filename);
        write_artifact_file(&path, current_content.trim());
    }
}

/// Append a catalog entry to docs/Products.md (create if not exists)
fn append_catalog_entry(entry: &str) {
    let catalog_path = PathBuf::from("docs/Products.md");

    if !catalog_path.exists() {
        // Create with header
        let header = "# Product Catalog\n\n\
                      | Name | Category | Tier | Key Spec | Innovations | Date |\n\
                      |------|----------|------|----------|-------------|------|\n";
        let content = format!("{}{}\n", header, entry);
        write_artifact_file(&catalog_path, &content);
    } else {
        // Append entry
        match std::fs::read_to_string(&catalog_path) {
            Ok(existing) => {
                let updated = format!("{}\n{}\n", existing.trim_end(), entry);
                write_artifact_file(&catalog_path, &updated);
            }
            Err(e) => warn!("Workshop: Failed to read Products.md: {}", e),
        }
    }
}
