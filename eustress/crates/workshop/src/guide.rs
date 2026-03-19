//! # guide
//!
//! AI-generated build guides for workshop products.
//! A `BuildGuide` is an ordered list of `BuildStep` entries. Each step references
//! one or more tools from the registry by UUID, specifies required materials,
//! and includes live location data resolved from `LiveStatusStore` at render time.
//!
//! Build guides are stored as `.guide.toml` files alongside `.tool.toml` files,
//! matching the file-system-first philosophy of Eustress.
//!
//! ## Table of Contents
//!
//! | Section              | Purpose                                                     |
//! |----------------------|-------------------------------------------------------------|
//! | `MaterialRequirement`| A material needed for a build step (quantity + unit)        |
//! | `StepRequirement`    | Union of tool or material requirement for a step            |
//! | `BuildStep`          | A single ordered step in a build guide                      |
//! | `MissingRequirement` | A tool or material that could not be resolved from registry |
//! | `BuildGuide`         | Full ordered guide — serialises to `.guide.toml`            |
//! | `GuideResolution`    | Output of resolving a guide against registry + live status  |

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::registry::ToolIndex;
use crate::status::LiveStatusStore;

// ============================================================================
// 1. Requirements
// ============================================================================

/// A material or consumable required for a build step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialRequirement {
    /// Name of the material (e.g. "M6 hex bolt", "3mm aluminium sheet", "PLA filament")
    pub name: String,
    /// Numeric quantity required
    pub quantity: f32,
    /// Unit of measure (e.g. "pcs", "mm", "kg", "m", "sheets")
    pub unit: String,
    /// Optional supplier hint or ASIN for procurement
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supplier_hint: Option<String>,
    /// Whether this material is confirmed available in the workshop inventory
    #[serde(default)]
    pub in_stock: bool,
}

/// A single requirement for a build step — either a registered tool or a material
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepRequirement {
    /// A registered tool from the workshop registry
    Tool {
        /// UUID of the registered tool
        tool_id: Uuid,
        /// Human-readable fallback name (used if tool is not in registry)
        tool_name: String,
        /// Whether this tool is strictly required or can be substituted
        #[serde(default = "default_true")]
        required: bool,
    },
    /// A raw material or consumable
    Material(MaterialRequirement),
}

fn default_true() -> bool {
    true
}

// ============================================================================
// 2. BuildStep
// ============================================================================

/// A single ordered step in a build guide.
/// References tools by UUID so the live location can be resolved at render time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStep {
    /// 1-based step index
    pub index: u32,
    /// Short title for the step (e.g. "Drill pilot holes")
    pub title: String,
    /// Full instruction text — written by the AI with tool-specific how-to detail
    pub instruction: String,
    /// Safety notes specific to this step
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub safety_notes: Vec<String>,
    /// Tools and materials required for this step
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<StepRequirement>,
    /// Estimated time to complete in minutes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_minutes: Option<u32>,
    /// Optional image or diagram path (relative to workspace)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagram_path: Option<String>,
    /// Whether this step has been marked complete by the user
    #[serde(default)]
    pub completed: bool,
}

impl BuildStep {
    /// Returns all tool UUIDs referenced in this step's requirements
    pub fn tool_ids(&self) -> Vec<Uuid> {
        self.requirements
            .iter()
            .filter_map(|r| match r {
                StepRequirement::Tool { tool_id, .. } => Some(*tool_id),
                StepRequirement::Material(_) => None,
            })
            .collect()
    }

    /// Returns all material requirements in this step
    pub fn materials(&self) -> Vec<&MaterialRequirement> {
        self.requirements
            .iter()
            .filter_map(|r| match r {
                StepRequirement::Material(m) => Some(m),
                _ => None,
            })
            .collect()
    }
}

// ============================================================================
// 3. MissingRequirement
// ============================================================================

/// A tool or material that is required by a build step but cannot be fulfilled
/// from the current workshop registry or inventory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingRequirement {
    /// Step index where this requirement is needed
    pub step_index: u32,
    /// Step title for display context
    pub step_title: String,
    /// Human-readable name of the missing item
    pub item_name: String,
    /// Why it is considered missing
    pub reason: MissingReason,
    /// Suggested search terms for procurement
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_terms: Vec<String>,
}

/// The reason a required item is considered missing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingReason {
    /// The tool UUID references a tool that is not in the registry
    ToolNotRegistered,
    /// The tool is registered but currently checked out or in use
    ToolUnavailable,
    /// The tool is registered but its IoT chip reports it as missing
    ToolMissing,
    /// A material is needed but marked as not in stock
    MaterialNotInStock,
    /// A material quantity exceeds what is listed as in-stock
    InsufficientMaterial,
}

// ============================================================================
// 4. BuildGuide
// ============================================================================

/// A complete AI-generated build guide for a product.
/// Serialises to a `.guide.toml` file in the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildGuide {
    /// Stable unique identifier for this guide
    pub id: Uuid,
    /// Human-readable title (e.g. "Aluminium Bracket Assembly Guide")
    pub title: String,
    /// Short description of what is being built
    pub description: String,
    /// Link back to the product's `IdeationBrief` file path (relative to workspace)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_path: Option<String>,
    /// Ordered list of build steps
    pub steps: Vec<BuildStep>,
    /// Total estimated time in minutes (sum of step estimates)
    #[serde(default)]
    pub total_estimated_minutes: u32,
    /// Skill level required: "beginner", "intermediate", "advanced"
    #[serde(default = "default_skill_level")]
    pub skill_level: String,
    /// Notes about the workshop setup needed before starting
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setup_notes: Vec<String>,
    /// ISO 8601 timestamp when this guide was generated
    pub generated_at: DateTime<Utc>,
    /// ISO 8601 timestamp of last modification
    pub updated_at: DateTime<Utc>,
}

fn default_skill_level() -> String {
    "intermediate".into()
}

impl BuildGuide {
    /// Create a new empty guide
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            description: description.into(),
            brief_path: None,
            steps: Vec::new(),
            total_estimated_minutes: 0,
            skill_level: default_skill_level(),
            setup_notes: Vec::new(),
            generated_at: now,
            updated_at: now,
        }
    }

    /// Derive the canonical `.guide.toml` filename from the guide title
    pub fn canonical_filename(&self) -> String {
        let slug = self
            .title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        format!("{}.guide.toml", slug)
    }

    /// Recalculate and update `total_estimated_minutes` from step estimates
    pub fn recalculate_total_time(&mut self) {
        self.total_estimated_minutes = self
            .steps
            .iter()
            .filter_map(|s| s.estimated_minutes)
            .sum();
    }

    /// Returns every unique tool UUID referenced across all steps
    pub fn all_tool_ids(&self) -> Vec<Uuid> {
        let mut ids = Vec::new();
        for step in &self.steps {
            for id in step.tool_ids() {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids
    }

    /// Returns every material requirement across all steps
    pub fn all_materials(&self) -> Vec<(u32, &MaterialRequirement)> {
        self.steps
            .iter()
            .flat_map(|step| {
                step.materials()
                    .into_iter()
                    .map(|m| (step.index, m))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Resolve this guide against the tool registry and live status store.
    /// Returns a `GuideResolution` containing enriched steps and a missing items list.
    pub fn resolve<'a>(
        &'a self,
        index: &'a ToolIndex,
        live: &LiveStatusStore,
    ) -> GuideResolution<'a> {
        let mut resolved_steps = Vec::new();
        let mut missing = Vec::new();

        for step in &self.steps {
            let mut step_tools = Vec::new();

            for req in &step.requirements {
                match req {
                    StepRequirement::Tool { tool_id, tool_name, required } => {
                        match index.get(tool_id) {
                            Some(tool) => {
                                let state = live.state_of(tool_id);
                                let location = live.location_of(tool_id);
                                let assignable = state.is_assignable();

                                if !assignable && *required {
                                    missing.push(MissingRequirement {
                                        step_index: step.index,
                                        step_title: step.title.clone(),
                                        item_name: tool.name.clone(),
                                        reason: MissingReason::ToolUnavailable,
                                        search_terms: vec![tool.name.clone()],
                                    });
                                }

                                step_tools.push(ResolvedTool {
                                    tool_id: *tool_id,
                                    name: tool.name.clone(),
                                    home_location: tool.home_location.clone(),
                                    current_location: location,
                                    how_to_use: tool.how_to_use.clone(),
                                    safety_notes: tool.safety_notes.clone(),
                                    state_label: state.display_label().to_string(),
                                    assignable,
                                });
                            }
                            None => {
                                if *required {
                                    missing.push(MissingRequirement {
                                        step_index: step.index,
                                        step_title: step.title.clone(),
                                        item_name: tool_name.clone(),
                                        reason: MissingReason::ToolNotRegistered,
                                        search_terms: vec![tool_name.clone()],
                                    });
                                }
                            }
                        }
                    }
                    StepRequirement::Material(material) => {
                        if !material.in_stock {
                            missing.push(MissingRequirement {
                                step_index: step.index,
                                step_title: step.title.clone(),
                                item_name: material.name.clone(),
                                reason: MissingReason::MaterialNotInStock,
                                search_terms: vec![
                                    material.name.clone(),
                                    format!("{} {}", material.quantity, material.unit),
                                ],
                            });
                        }
                    }
                }
            }

            resolved_steps.push(ResolvedStep {
                step,
                resolved_tools: step_tools,
            });
        }

        GuideResolution {
            guide: self,
            resolved_steps,
            missing,
        }
    }
}

// ============================================================================
// 5. GuideResolution — output of resolving against live registry
// ============================================================================

/// A resolved tool reference — combines registry definition + live status
#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub tool_id: Uuid,
    pub name: String,
    pub home_location: String,
    pub current_location: String,
    pub how_to_use: String,
    pub safety_notes: Vec<String>,
    pub state_label: String,
    pub assignable: bool,
}

/// A build step with its tool references fully resolved
pub struct ResolvedStep<'a> {
    pub step: &'a BuildStep,
    pub resolved_tools: Vec<ResolvedTool>,
}

impl<'a> ResolvedStep<'a> {
    /// Format this step as a rich instruction card string for the AI or UI
    pub fn render_card(&self) -> String {
        let mut lines = vec![
            format!("## Step {}: {}", self.step.index, self.step.title),
            String::new(),
            self.step.instruction.clone(),
        ];

        if !self.resolved_tools.is_empty() {
            lines.push(String::new());
            lines.push("**Tools:**".into());
            for tool in &self.resolved_tools {
                lines.push(format!(
                    "  - {} — {} ({})",
                    tool.name, tool.current_location, tool.state_label
                ));
                lines.push(format!("    How to use: {}", tool.how_to_use));
            }
        }

        let materials: Vec<&MaterialRequirement> = self.step.materials();
        if !materials.is_empty() {
            lines.push(String::new());
            lines.push("**Materials:**".into());
            for mat in materials {
                let stock = if mat.in_stock { "in stock" } else { "MISSING" };
                lines.push(format!(
                    "  - {} × {} {} ({})",
                    mat.quantity, mat.unit, mat.name, stock
                ));
            }
        }

        if !self.step.safety_notes.is_empty() {
            lines.push(String::new());
            lines.push("**Safety:**".into());
            for note in &self.step.safety_notes {
                lines.push(format!("  ⚠ {}", note));
            }
        }

        if let Some(mins) = self.step.estimated_minutes {
            lines.push(String::new());
            lines.push(format!("*Estimated time: {} min*", mins));
        }

        lines.join("\n")
    }
}

/// The output of resolving a `BuildGuide` against the tool registry and live status.
/// Passed to the UI for rendering and to the procurement module for shopping list generation.
pub struct GuideResolution<'a> {
    pub guide: &'a BuildGuide,
    pub resolved_steps: Vec<ResolvedStep<'a>>,
    pub missing: Vec<MissingRequirement>,
}

impl<'a> GuideResolution<'a> {
    /// Returns true if every required tool and material is available
    pub fn is_fully_ready(&self) -> bool {
        self.missing.is_empty()
    }

    /// Render the complete guide as a Markdown document
    pub fn render_markdown(&self) -> String {
        let mut lines = vec![
            format!("# {}", self.guide.title),
            String::new(),
            self.guide.description.clone(),
            String::new(),
            format!(
                "*Estimated total time: {} min | Skill level: {}*",
                self.guide.total_estimated_minutes, self.guide.skill_level
            ),
        ];

        if !self.guide.setup_notes.is_empty() {
            lines.push(String::new());
            lines.push("## Setup".into());
            for note in &self.guide.setup_notes {
                lines.push(format!("- {}", note));
            }
        }

        lines.push(String::new());
        lines.push("## Steps".into());

        for resolved_step in &self.resolved_steps {
            lines.push(String::new());
            lines.push(resolved_step.render_card());
        }

        if !self.missing.is_empty() {
            lines.push(String::new());
            lines.push("## ⚠ Missing Items".into());
            for item in &self.missing {
                lines.push(format!(
                    "- **{}** (Step {}: {}) — {:?}",
                    item.item_name, item.step_index, item.step_title, item.reason
                ));
            }
        }

        lines.join("\n")
    }
}

// ============================================================================
// 6. Guide System Prompt Builder
// ============================================================================

/// Build the AI system prompt section that instructs Claude how to generate
/// build guide TOML from an ideation brief + registered workshop tools context.
pub fn build_guide_system_prompt(tool_context: &str) -> String {
    format!(
        r#"You are a master workshop technician and technical writer generating precise build guides.

You have access to the following registered workshop tools. For EVERY step that requires a tool,
you MUST reference the tool's exact UUID from the registry below. Do not invent tool IDs.
If a required tool is not in the registry, include it in the step's requirements with a generated
UUID and set "required": true so it appears in the missing items list.

Output a valid TOML document matching the BuildGuide schema. Every step must have:
- A clear, actionable instruction (2-5 sentences)
- At least one StepRequirement (tool or material)
- Safety notes for any step involving cutting, drilling, welding, or power tools
- A realistic estimated_minutes value

{}

Generate the BuildGuide TOML now:"#,
        tool_context
    )
}
