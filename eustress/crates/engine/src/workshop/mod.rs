//! # Workshop Module — System 0: Ideation
//!
//! Conversational chat interface for product ideation. Takes a natural language
//! idea and guides the user through patent, SOTA validation, requirements,
//! mesh generation, part files, and catalog registration — step by step.
//!
//! ## Table of Contents
//!
//! 1. Data Structures — IdeationBrief, ChatMessage, PipelineStep, conversation types
//! 2. IdeationPipeline — state machine resource driving the generation flow
//! 3. Conversation Persistence — Windsurf-style entries.json per session
//! 4. Claude Bridge — routes chat messages through the BYOK API key
//! 5. Brief Normalizer — freeform text → ideation_brief.toml via Claude
//! 6. WorkshopPlugin — Bevy plugin registration, systems, events
//!
//! ## Architecture
//!
//! - All AI interactions use the BYOK API key from Soul Settings
//! - Conversation history persisted to ~/.eustress_engine/workshop/history/{session_id}/entries.json
//! - Each pipeline step requires explicit user approval before spending credits
//! - Generated .glb meshes loaded once to GPU; .part.toml files clone with unique properties

pub mod persistence;
pub mod normalizer;
pub mod claude_bridge;
pub mod artifact_gen;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::manufacturing::{AllocationDecision, AllocationStatus};
use std::path::PathBuf;
use uuid::Uuid;

// ============================================================================
// 1. Data Structures
// ============================================================================

/// A single message in the ideation conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Unique message identifier
    pub id: u32,
    /// Who sent this message
    pub role: MessageRole,
    /// Message text content
    pub content: String,
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// For MCP commands: the endpoint being called
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_endpoint: Option<String>,
    /// For MCP commands: GET/POST
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_method: Option<String>,
    /// For MCP commands: current status
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_status: Option<McpCommandStatus>,
    /// For artifacts: file path of generated artifact
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<PathBuf>,
    /// For artifacts: type classification
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_type: Option<ArtifactType>,
    /// Estimated cost of this message's API call (in USD)
    #[serde(default)]
    pub estimated_cost: f64,
    /// Actual cost after completion (in USD, if known)
    #[serde(default)]
    pub actual_cost: Option<f64>,
}

/// Who sent a message in the conversation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// User typed this message
    User,
    /// System (Workshop AI) response
    System,
    /// MCP command card (approve/edit/skip)
    Mcp,
    /// Approval gate requiring user decision
    Approval,
    /// Generated artifact notification
    Artifact,
    /// Error message
    Error,
}

impl MessageRole {
    /// Convert to the string format Slint expects
    pub fn to_slint_string(&self) -> &str {
        match self {
            MessageRole::User => "user",
            MessageRole::System => "system",
            MessageRole::Mcp => "mcp",
            MessageRole::Approval => "approval",
            MessageRole::Artifact => "artifact",
            MessageRole::Error => "error",
        }
    }
}

/// Status of an MCP command in the conversation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpCommandStatus {
    /// Waiting for user to approve
    Pending,
    /// User approved, queued for execution
    Approved,
    /// Currently executing
    Running,
    /// Completed successfully
    Done,
    /// User chose to skip
    Skipped,
    /// Failed with error
    Error,
}

impl McpCommandStatus {
    /// Convert to the string format Slint expects
    pub fn to_slint_string(&self) -> &str {
        match self {
            McpCommandStatus::Pending => "pending",
            McpCommandStatus::Approved => "approved",
            McpCommandStatus::Running => "running",
            McpCommandStatus::Done => "done",
            McpCommandStatus::Skipped => "skipped",
            McpCommandStatus::Error => "error",
        }
    }
}

/// Type of generated artifact
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    Patent,
    Sota,
    Requirements,
    Mesh,
    Toml,
    Readme,
    Catalog,
    Brief,
    RuneSimScript,
    RuneUiScript,
    UiToml,
    /// Manufacturing deal term sheet — equity split + royalty structure
    DealStructure,
    /// Pilot program and warehousing logistics plan
    LogisticsPlan,
}

impl ArtifactType {
    /// Convert to the string format Slint expects
    pub fn to_slint_string(&self) -> &str {
        match self {
            ArtifactType::Patent => "patent",
            ArtifactType::Sota => "sota",
            ArtifactType::Requirements => "requirements",
            ArtifactType::Mesh => "mesh",
            ArtifactType::Toml => "toml",
            ArtifactType::Readme => "readme",
            ArtifactType::Catalog => "catalog",
            ArtifactType::Brief => "brief",
            ArtifactType::RuneSimScript => "rune_sim_script",
            ArtifactType::RuneUiScript => "rune_ui_script",
            ArtifactType::UiToml => "ui_toml",
            ArtifactType::DealStructure => "deal_structure",
            ArtifactType::LogisticsPlan => "logistics_plan",
        }
    }
}

/// A pipeline step with status tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    /// Step index (0-based)
    pub index: u32,
    /// Human-readable label
    pub label: String,
    /// Current status
    pub status: StepStatus,
    /// Number of artifacts generated in this step
    pub artifact_count: u32,
    /// Associated MCP endpoint
    pub mcp_endpoint: String,
    /// Estimated cost for this step
    pub estimated_cost: f64,
}

/// Status of a pipeline step
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Not yet reached
    Waiting,
    /// Currently executing
    Active,
    /// Completed successfully
    Done,
    /// Failed with error
    Error,
    /// User chose to skip
    Skipped,
}

impl StepStatus {
    /// Convert to the string format Slint expects
    pub fn to_slint_string(&self) -> &str {
        match self {
            StepStatus::Waiting => "waiting",
            StepStatus::Active => "active",
            StepStatus::Done => "done",
            StepStatus::Error => "error",
            StepStatus::Skipped => "skipped",
        }
    }
}

// ============================================================================
// 2. IdeationPipeline — state machine resource
// ============================================================================

/// The ideation pipeline state machine
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdeationState {
    /// No active ideation session
    Idle,
    /// Gathering information from user via chat
    Conversing,
    /// Normalizing freeform input into ideation_brief.toml
    Normalizing,
    /// Generating PATENT.md
    GeneratingPatent,
    /// Generating SOTA_VALIDATION.md
    GeneratingSotaValidation,
    /// Generating EustressEngine_Requirements.md
    GeneratingRequirements,
    /// Running Blender headless for .glb meshes
    GeneratingMeshes,
    /// Generating .part.toml files placed in Workspace
    GeneratingParts,
    /// Generating Rune simulation scripts placed in SoulService
    GeneratingSimScripts,
    /// Generating ScreenGui UI TOML + Rune UI scripts placed in StarterGui
    GeneratingUI,
    /// Registering in Products.md catalog
    FinalizingCatalog,
    /// Generating DEAL_STRUCTURE.md — equity split, royalty terms, manufacturing program stake
    GeneratingDealStructure,
    /// Generating LOGISTICS_PLAN.md — pilot program, warehousing, fulfillment partners
    GeneratingLogisticsPlan,
    /// All steps complete, ready for Systems 1-8 handoff
    Complete,
    /// Pipeline paused waiting for user input
    Paused,
    /// Pipeline encountered an error
    Failed { error: String },
}

impl Default for IdeationState {
    fn default() -> Self {
        IdeationState::Idle
    }
}

/// The ideation brief — structured product definition normalized from any input
/// This is the TOML-serializable schema that gets written to ideation_brief.toml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdeationBrief {
    pub product: ProductDefinition,
    #[serde(default)]
    pub innovations: Vec<Innovation>,
    #[serde(default)]
    pub target_specs: Vec<TargetSpec>,
    #[serde(default)]
    pub bill_of_materials: Vec<BomEntry>,
    #[serde(default)]
    pub physics_model: Option<PhysicsModel>,
    /// Manufacturing deal structure — equity split and royalty terms
    #[serde(default)]
    pub deal_structure: Option<DealStructure>,
    /// AI allocation decision — selected manufacturer + investors for this product
    #[serde(default)]
    pub allocation: Option<AllocationDecision>,
    pub ideation_metadata: IdeationMetadata,
}

// ============================================================================
// 1b. Deal Structure — equity distribution and manufacturing deal terms
// ============================================================================

/// Manufacturing deal structure written to DEAL_STRUCTURE.md and ideation_brief.toml.
///
/// Encodes the equity split between the inventor, the Eustress Manufacturing Program,
/// logistics partners, and any co-investors, plus the royalty percentage that flows
/// back into the manufacturing fund on each unit sold.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DealStructure {
    /// Human-readable title for this deal (e.g. "The Cube — V1 Manufacturing Deal")
    pub title: String,
    /// All equity stakeholders that share in the product revenue
    pub equity_splits: Vec<EquityStake>,
    /// Royalty percentage of net sales that flows to the Manufacturing Program fund
    /// (funds future pilot programs and warehousing capacity)
    pub manufacturing_program_royalty_pct: f64,
    /// Royalty percentage of net sales retained by the inventor
    pub inventor_royalty_pct: f64,
    /// Suggested retail unit price in USD
    pub unit_price_usd: f64,
    /// Estimated unit cost (BOM + assembly + logistics) in USD
    pub unit_cost_usd: f64,
    /// Minimum pilot batch size (units) before full production is unlocked
    pub pilot_minimum_units: u32,
    /// Target geography for the pilot program
    pub pilot_geography: String,
    /// Deal expiry — how many months this term sheet is valid
    pub term_validity_months: u32,
    /// Optional notes or negotiation terms
    #[serde(default)]
    pub notes: String,
    /// Logistics plan for the pilot — warehousing, fulfillment, 3PL partners
    #[serde(default)]
    pub logistics: Option<LogisticsPlan>,
}

impl DealStructure {
    /// Gross margin per unit after BOM + assembly + logistics
    pub fn gross_margin_usd(&self) -> f64 {
        self.unit_price_usd - self.unit_cost_usd
    }

    /// Gross margin as a percentage of retail price
    pub fn gross_margin_pct(&self) -> f64 {
        if self.unit_price_usd > 0.0 {
            (self.gross_margin_usd() / self.unit_price_usd) * 100.0
        } else {
            0.0
        }
    }

    /// Total royalty outflow as a percentage of net sales
    pub fn total_royalty_pct(&self) -> f64 {
        self.manufacturing_program_royalty_pct + self.inventor_royalty_pct
    }

    /// Validate that all equity stakes sum to 100.0% (within float tolerance)
    pub fn validate_equity_sum(&self) -> Result<(), String> {
        let total: f64 = self.equity_splits.iter().map(|s| s.percentage).sum();
        if (total - 100.0).abs() > 0.01 {
            Err(format!("Equity stakes sum to {:.2}% — must equal 100%", total))
        } else {
            Ok(())
        }
    }

    /// Royalty dollars per unit flowing to the Manufacturing Program
    pub fn manufacturing_royalty_per_unit(&self) -> f64 {
        self.unit_price_usd * (self.manufacturing_program_royalty_pct / 100.0)
    }

    /// Estimated Manufacturing Program fund contribution after a full pilot batch
    pub fn pilot_fund_contribution_usd(&self) -> f64 {
        self.manufacturing_royalty_per_unit() * self.pilot_minimum_units as f64
    }
}

/// A single equity stakeholder in the manufacturing deal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityStake {
    /// Stakeholder name (e.g. "Inventor", "Eustress Manufacturing Program", "3PL Partner")
    pub stakeholder: String,
    /// Role description (e.g. "IP owner", "manufacturing fund", "logistics partner")
    pub role: String,
    /// Equity percentage (0.0–100.0)
    pub percentage: f64,
    /// Optional vesting cliff in months (None = immediate)
    pub vesting_cliff_months: Option<u32>,
    /// Optional vesting period in months (None = no vesting schedule)
    pub vesting_period_months: Option<u32>,
}

/// Pilot program and warehousing logistics plan written to LOGISTICS_PLAN.md
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogisticsPlan {
    /// Phase 1: Pilot program details
    pub pilot: PilotProgram,
    /// Phase 2: Warehousing configuration
    pub warehousing: WarehousingConfig,
    /// Phase 3: Fulfillment and shipping partners
    pub fulfillment: FulfillmentConfig,
    /// Regulatory and customs requirements
    #[serde(default)]
    pub regulatory_notes: String,
}

/// Pilot program configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PilotProgram {
    /// Number of units in the initial pilot batch
    pub batch_size: u32,
    /// Target market segment for pilot (e.g. "Professional workshops, US Pacific Northwest")
    pub target_segment: String,
    /// Pilot duration in weeks
    pub duration_weeks: u32,
    /// Success criteria — what metrics must be hit to unlock full production
    pub success_criteria: Vec<String>,
    /// List of pilot distribution channels
    pub channels: Vec<String>,
    /// Planned pilot launch date (ISO 8601, approximate)
    pub launch_date_approx: String,
    /// Feedback collection method (survey, telemetry, interviews)
    pub feedback_method: String,
}

/// Warehousing configuration for pilot and production
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WarehousingConfig {
    /// Preferred warehouse model — "own" | "3pl" | "dropship" | "consignment"
    pub model: String,
    /// Geographic regions for warehouse nodes
    pub regions: Vec<String>,
    /// Minimum stock level before reorder is triggered
    pub reorder_point_units: u32,
    /// Standard order quantity when reorder triggers
    pub reorder_quantity_units: u32,
    /// Storage temperature requirements (None = ambient)
    pub temperature_requirements: Option<String>,
    /// Hazmat classification (None = standard goods)
    pub hazmat_class: Option<String>,
    /// Estimated monthly warehousing cost per SKU in USD
    pub estimated_monthly_cost_usd: f64,
}

/// Fulfillment and shipping configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FulfillmentConfig {
    /// Primary 3PL or fulfillment partner name (e.g. "ShipBob", "Amazon FBA", "own fleet")
    pub primary_partner: String,
    /// Backup fulfillment partner
    pub backup_partner: Option<String>,
    /// Supported shipping speeds (e.g. ["Standard 5-7d", "Express 2d", "Overnight"])
    pub shipping_speeds: Vec<String>,
    /// Target countries for shipping
    pub ship_to_countries: Vec<String>,
    /// Average fulfillment cost per order in USD
    pub avg_fulfillment_cost_usd: f64,
    /// Returns/RMA policy summary
    pub returns_policy: String,
}

/// Core product identity
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductDefinition {
    pub name: String,
    pub description: String,
    /// "conventional" | "exotic_propulsion"
    #[serde(default = "default_category")]
    pub category: String,
    /// "foundation" | "platform" | "horizon"
    #[serde(default = "default_tier")]
    pub tier: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub dimensions: ProductDimensions,
}

fn default_category() -> String { "conventional".to_string() }
fn default_tier() -> String { "foundation".to_string() }
fn default_version() -> String { "V1".to_string() }

/// Physical dimensions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductDimensions {
    /// Width in meters
    pub width: f64,
    /// Height in meters
    pub height: f64,
    /// Depth in meters
    pub depth: f64,
    /// "prismatic" | "cylindrical" | "disc" | "custom"
    #[serde(default = "default_form_factor")]
    pub form_factor: String,
}

fn default_form_factor() -> String { "prismatic".to_string() }

/// A key innovation claim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Innovation {
    pub name: String,
    pub description: String,
    /// "VERIFIED" | "PROJECTED" | "ASPIRATIONAL"
    pub tier: String,
}

/// A target specification with benchmark comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSpec {
    pub metric: String,
    pub target: f64,
    pub unit: String,
    pub benchmark: f64,
    pub benchmark_label: String,
}

/// A bill of materials entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BomEntry {
    pub component: String,
    pub material: String,
    /// Dimensions in meters [L, W, H]
    pub dimensions: [f64; 3],
    pub role: String,
}

/// Physics model for exotic propulsion products
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsModel {
    #[serde(rename = "type")]
    pub model_type: String,
    /// Additional physics parameters as key-value pairs
    #[serde(flatten)]
    pub parameters: HashMap<String, toml::Value>,
}

/// Metadata about the ideation session
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdeationMetadata {
    /// "windsurf_workflow" | "workshop_panel" | "soul_script" | "natural_language" | "import"
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub session_id: String,
    /// Total BYOK API cost for this ideation session
    #[serde(default)]
    pub total_cost: f64,
}

fn default_source() -> String { "workshop_panel".to_string() }

/// Main pipeline resource — holds all state for the current ideation session
#[derive(Resource, Debug, Clone)]
pub struct IdeationPipeline {
    /// Current state machine position
    pub state: IdeationState,
    /// Unique session identifier
    pub session_id: String,
    /// Conversation history
    pub messages: Vec<ChatMessage>,
    /// Next message ID counter
    pub next_message_id: u32,
    /// Pipeline steps with status
    pub steps: Vec<PipelineStep>,
    /// The normalized brief (populated after normalization step)
    pub brief: Option<IdeationBrief>,
    /// Product name (extracted early from conversation)
    pub product_name: String,
    /// Output directory for generated artifacts
    pub output_dir: Option<PathBuf>,
    /// Generated artifact paths
    pub artifacts: Vec<(ArtifactType, PathBuf)>,
    /// Running total of BYOK API costs this session
    pub total_cost: f64,
    /// Conversation context for Claude (accumulated user messages for richer prompts)
    pub conversation_context: String,
    /// Whether the pipeline has unsaved changes
    pub dirty: bool,
}

impl Default for IdeationPipeline {
    fn default() -> Self {
        Self {
            state: IdeationState::Idle,
            session_id: Uuid::new_v4().to_string(),
            messages: Vec::new(),
            next_message_id: 0,
            steps: Self::default_steps(),
            brief: None,
            product_name: String::new(),
            output_dir: None,
            artifacts: Vec::new(),
            total_cost: 0.0,
            conversation_context: String::new(),
            dirty: false,
        }
    }
}

impl IdeationPipeline {
    /// Create default pipeline steps matching the /create-voltec-product workflow
    pub fn default_steps() -> Vec<PipelineStep> {
        vec![
            PipelineStep {
                index: 0,
                label: "Normalize brief".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/normalize".to_string(),
                estimated_cost: 0.03,
            },
            PipelineStep {
                index: 1,
                label: "Patent draft".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.05,
            },
            PipelineStep {
                index: 2,
                label: "SOTA validation".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.04,
            },
            PipelineStep {
                index: 3,
                label: "Requirements".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.04,
            },
            PipelineStep {
                index: 4,
                label: "Mesh generation".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.03,
            },
            PipelineStep {
                index: 5,
                label: "Part files".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.02,
            },
            PipelineStep {
                index: 6,
                label: "Rune sim scripts".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.04,
            },
            PipelineStep {
                index: 7,
                label: "UI + UI scripts".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.04,
            },
            PipelineStep {
                index: 8,
                label: "Catalog entry".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.01,
            },
            PipelineStep {
                index: 9,
                label: "Deal structure".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.04,
            },
            PipelineStep {
                index: 10,
                label: "Logistics plan".to_string(),
                status: StepStatus::Waiting,
                artifact_count: 0,
                mcp_endpoint: "/mcp/ideation/brief".to_string(),
                estimated_cost: 0.04,
            },
        ]
    }

    /// Add a user message to the conversation
    pub fn add_user_message(&mut self, content: String) -> u32 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        
        // Accumulate into conversation context for Claude
        self.conversation_context.push_str(&format!("\nUser: {}", &content));
        
        self.messages.push(ChatMessage {
            id,
            role: MessageRole::User,
            content,
            timestamp: chrono::Utc::now().to_rfc3339(),
            mcp_endpoint: None,
            mcp_method: None,
            mcp_status: None,
            artifact_path: None,
            artifact_type: None,
            estimated_cost: 0.0,
            actual_cost: None,
        });
        self.dirty = true;
        id
    }

    /// Add a system (Workshop AI) response to the conversation
    pub fn add_system_message(&mut self, content: String, cost: f64) -> u32 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        
        self.total_cost += cost;
        
        // Accumulate into conversation context
        self.conversation_context.push_str(&format!("\nWorkshop: {}", &content));
        
        self.messages.push(ChatMessage {
            id,
            role: MessageRole::System,
            content,
            timestamp: chrono::Utc::now().to_rfc3339(),
            mcp_endpoint: None,
            mcp_method: None,
            mcp_status: None,
            artifact_path: None,
            artifact_type: None,
            estimated_cost: cost,
            actual_cost: Some(cost),
        });
        self.dirty = true;
        id
    }

    /// Add an MCP command card to the conversation (pending approval)
    pub fn add_mcp_command(
        &mut self,
        content: String,
        endpoint: String,
        method: String,
        estimated_cost: f64,
    ) -> u32 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        self.messages.push(ChatMessage {
            id,
            role: MessageRole::Mcp,
            content,
            timestamp: chrono::Utc::now().to_rfc3339(),
            mcp_endpoint: Some(endpoint),
            mcp_method: Some(method),
            mcp_status: Some(McpCommandStatus::Pending),
            artifact_path: None,
            artifact_type: None,
            estimated_cost,
            actual_cost: None,
        });
        self.dirty = true;
        id
    }

    /// Add an artifact notification to the conversation
    pub fn add_artifact_message(&mut self, path: PathBuf, artifact_type: ArtifactType) -> u32 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        
        let display_path = path.display().to_string();
        self.artifacts.push((artifact_type.clone(), path.clone()));
        
        self.messages.push(ChatMessage {
            id,
            role: MessageRole::Artifact,
            content: display_path,
            timestamp: chrono::Utc::now().to_rfc3339(),
            mcp_endpoint: None,
            mcp_method: None,
            mcp_status: None,
            artifact_path: Some(path),
            artifact_type: Some(artifact_type),
            estimated_cost: 0.0,
            actual_cost: None,
        });
        self.dirty = true;
        id
    }

    /// Add an error message to the conversation
    pub fn add_error_message(&mut self, content: String) -> u32 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        self.messages.push(ChatMessage {
            id,
            role: MessageRole::Error,
            content,
            timestamp: chrono::Utc::now().to_rfc3339(),
            mcp_endpoint: None,
            mcp_method: None,
            mcp_status: None,
            artifact_path: None,
            artifact_type: None,
            estimated_cost: 0.0,
            actual_cost: None,
        });
        self.dirty = true;
        id
    }

    /// Update the status of an MCP command by message ID
    pub fn update_mcp_status(&mut self, message_id: u32, status: McpCommandStatus) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == message_id) {
            msg.mcp_status = Some(status);
            self.dirty = true;
        }
    }

    /// Get estimated cost for remaining unapproved steps
    pub fn estimated_remaining_cost(&self) -> f64 {
        self.steps.iter()
            .filter(|s| s.status == StepStatus::Waiting)
            .map(|s| s.estimated_cost)
            .sum()
    }

    /// Format total cost as USD string
    pub fn format_cost(&self) -> String {
        format!("${:.2}", self.total_cost)
    }

    /// Reset the pipeline for a new ideation session
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Check if the pipeline has an active session (not idle)
    pub fn is_active(&self) -> bool {
        self.state != IdeationState::Idle
    }

    /// Get the Slint-compatible pipeline state string
    pub fn state_string(&self) -> &str {
        match &self.state {
            IdeationState::Idle => "idle",
            IdeationState::Complete => "complete",
            IdeationState::Paused => "paused",
            IdeationState::Failed { .. } => "error",
            _ => "running",
        }
    }
}

// ============================================================================
// 3. Events
// ============================================================================

/// Fired when a user sends a message in the Workshop Panel
#[derive(Message, Debug, Clone)]
pub struct WorkshopSendMessageEvent {
    pub content: String,
}

/// Fired when user approves an MCP command
#[derive(Message, Debug, Clone)]
pub struct WorkshopApproveMcpEvent {
    pub message_id: u32,
}

/// Fired when user skips an MCP command
#[derive(Message, Debug, Clone)]
pub struct WorkshopSkipMcpEvent {
    pub message_id: u32,
}

/// Fired when user wants to edit an MCP command before running
#[derive(Message, Debug, Clone)]
pub struct WorkshopEditMcpEvent {
    pub message_id: u32,
}

/// Fired when user clicks an artifact path to open it
#[derive(Message, Debug, Clone)]
pub struct WorkshopOpenArtifactEvent {
    pub path: String,
}

/// Fired when the ideation pipeline completes — consumed by Systems 1, 2, 5
#[derive(Message, Debug, Clone)]
pub struct ProductCreatedEvent {
    /// Product name
    pub product_name: String,
    /// Path to the generated ideation_brief.toml
    pub brief_path: PathBuf,
    /// Path to the product output directory
    pub output_dir: PathBuf,
    /// Session ID for traceability
    pub session_id: String,
}

/// Fired when the user clicks "Optimize & Build" to hand off to Systems 1-8
#[derive(Message, Debug, Clone)]
pub struct OptimizeAndBuildEvent {
    pub product_name: String,
    pub brief_path: PathBuf,
    pub output_dir: PathBuf,
}

/// Internal event: Claude response received (from async task)
#[derive(Message, Debug, Clone)]
pub struct ClaudeResponseEvent {
    /// The response text from Claude
    pub content: String,
    /// Cost of this API call
    pub cost: f64,
    /// Which pipeline step this was for (None = conversational chat)
    pub step_index: Option<u32>,
    /// If this was an MCP command response, the message ID
    pub mcp_message_id: Option<u32>,
}

/// Internal event: Claude request failed
#[derive(Message, Debug, Clone)]
pub struct ClaudeErrorEvent {
    pub error: String,
    pub step_index: Option<u32>,
    pub mcp_message_id: Option<u32>,
}

// ============================================================================
// 4. Systems
// ============================================================================

/// Process incoming user messages — route to Claude or handle locally
fn handle_send_message(
    mut events: MessageReader<WorkshopSendMessageEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
) {
    for event in events.read() {
        let content = event.content.trim().to_string();
        if content.is_empty() {
            continue;
        }
        
        // Add user message to conversation
        pipeline.add_user_message(content.clone());
        
        // Check if API key is available
        let has_key = match (&global_settings, &space_settings) {
            (Some(global), Some(space)) => {
                !space.effective_api_key(global).is_empty()
            }
            _ => false,
        };
        
        if !has_key {
            pipeline.add_error_message(
                "No API key configured. Open Soul Settings to add your BYOK key.".to_string()
            );
            continue;
        }
        
        // If pipeline is idle, start a new session with the user's idea
        if pipeline.state == IdeationState::Idle {
            pipeline.state = IdeationState::Conversing;
            info!("Workshop: Started new ideation session {}", pipeline.session_id);
        }
        
        // For now: acknowledge receipt and queue Claude call
        // The actual Claude bridge will be wired in the async task system
        // For the initial skeleton, we add a system message indicating the AI will respond
        info!("Workshop: User message queued for Claude: {} chars", content.len());
    }
}

/// Process MCP command approvals — advance the pipeline
fn handle_approve_mcp(
    mut events: MessageReader<WorkshopApproveMcpEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    for event in events.read() {
        pipeline.update_mcp_status(event.message_id, McpCommandStatus::Approved);
        info!("Workshop: MCP command {} approved", event.message_id);
        // The actual execution will be handled by the async task system
    }
}

/// Process MCP command skips — mark step as skipped and advance
fn handle_skip_mcp(
    mut events: MessageReader<WorkshopSkipMcpEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    for event in events.read() {
        pipeline.update_mcp_status(event.message_id, McpCommandStatus::Skipped);
        
        // Find which step this MCP command belongs to and mark it skipped
        // For now, advance conversation state
        pipeline.add_system_message("Step skipped. Moving to next.".to_string(), 0.0);
        info!("Workshop: MCP command {} skipped", event.message_id);
    }
}

/// Process Claude responses — route by step type:
/// - None (chat): add system message, stay in Conversing state
/// - Step 0 (normalize): parse TOML → write ideation_brief.toml → propose patent step
/// - Steps 1-8 (artifacts): handled by artifact_gen::handle_artifact_completion
fn handle_claude_response(
    mut events: MessageReader<ClaudeResponseEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
    space_root: Res<crate::space::SpaceRoot>,
) {
    for event in events.read() {
        // Mark MCP command as done if applicable
        if let Some(msg_id) = event.mcp_message_id {
            pipeline.update_mcp_status(msg_id, McpCommandStatus::Done);
        }
        
        match event.step_index {
            // Conversational chat response (no step)
            None => {
                pipeline.add_system_message(event.content.clone(), event.cost);
                
                // After a few exchanges, the AI should suggest normalization
                // Check if the response mentions "ready to normalize" or similar
                let content_lower = event.content.to_lowercase();
                if content_lower.contains("ready to normalize")
                    || content_lower.contains("normalize your idea")
                    || content_lower.contains("structured brief")
                    || content_lower.contains("ideation_brief")
                {
                    // Propose the normalization MCP command
                    pipeline.add_mcp_command(
                        "Generate ideation_brief.toml from your conversation.\nEstimated cost: ~$0.03 (Sonnet)".to_string(),
                        "/mcp/ideation/normalize".to_string(),
                        "POST".to_string(),
                        0.03,
                    );
                }
            }
            
            // Normalization response (step 0) — parse TOML, write to disk, propose patent
            Some(0) => {
                // Update step status
                if let Some(step) = pipeline.steps.get_mut(0) {
                    step.status = StepStatus::Done;
                    step.artifact_count += 1;
                }
                
                // Parse the brief from Claude's TOML response
                match normalizer::parse_brief_from_toml(&event.content) {
                    Ok(brief) => {
                        // Validate the brief
                        if let Err(validation_errors) = normalizer::validate_brief(&brief) {
                            pipeline.add_error_message(format!(
                                "Brief validation warnings: {}",
                                validation_errors.join(", ")
                            ));
                        }
                        
                        // Set product name from brief
                        pipeline.product_name = brief.product.name.clone();
                        
                        // Write to disk — brief goes to Space/Workspace/{product}/
                        let output_dir = normalizer::product_output_dir(&space_root.0, &pipeline.product_name);
                        match normalizer::write_brief_to_disk(&output_dir, &brief) {
                            Ok(path) => {
                                pipeline.add_artifact_message(
                                    path.clone(),
                                    ArtifactType::Brief,
                                );
                                pipeline.add_system_message(
                                    format!("Ideation brief generated: {}", path.display()),
                                    event.cost,
                                );
                            }
                            Err(e) => {
                                pipeline.add_error_message(format!(
                                    "Failed to write brief: {}", e
                                ));
                            }
                        }
                        
                        // Store the brief in the pipeline
                        pipeline.brief = Some(brief);
                        
                        // Advance state and propose the first artifact step (patent)
                        pipeline.state = IdeationState::GeneratingPatent;
                        pipeline.add_mcp_command(
                            "Generate PATENT.md (42+ claims, cross-sections, BOM)\nStep: patent\nEstimated cost: ~$0.05 (Sonnet)".to_string(),
                            "/mcp/ideation/brief".to_string(),
                            "POST".to_string(),
                            0.05,
                        );
                    }
                    Err(e) => {
                        pipeline.add_error_message(format!(
                            "Failed to parse brief TOML: {}. The AI response may need retry.", e
                        ));
                        // Revert to conversing so user can retry
                        pipeline.state = IdeationState::Conversing;
                    }
                }
            }
            
            // Artifact steps 1-6: file writing, artifact messages, and next-step
            // proposals are handled entirely by artifact_gen::handle_artifact_completion.
            // We intentionally do nothing here to avoid double-counting artifacts.
            Some(step_idx) if step_idx >= 1 && step_idx <= 6 => {}
            
            // Unknown step index
            Some(idx) => {
                warn!("Workshop: Unexpected step index {} in Claude response", idx);
                pipeline.add_system_message(event.content.clone(), event.cost);
            }
        }
        
        info!("Workshop: Claude response received (step={:?}, {} chars, ${:.4})", 
              event.step_index, event.content.len(), event.cost);
    }
}

/// Process Claude errors — add error to conversation
fn handle_claude_error(
    mut events: MessageReader<ClaudeErrorEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    for event in events.read() {
        pipeline.add_error_message(format!("AI error: {}", event.error));
        
        if let Some(msg_id) = event.mcp_message_id {
            pipeline.update_mcp_status(msg_id, McpCommandStatus::Error);
        }
        
        if let Some(step_idx) = event.step_index {
            if let Some(step) = pipeline.steps.get_mut(step_idx as usize) {
                step.status = StepStatus::Error;
            }
        }
        
        warn!("Workshop: Claude error: {}", event.error);
    }
}

/// Autosave conversation to disk periodically when dirty
fn autosave_conversation(
    mut pipeline: ResMut<IdeationPipeline>,
) {
    if !pipeline.dirty || !pipeline.is_active() {
        return;
    }
    
    if let Err(e) = persistence::save_session(&pipeline) {
        warn!("Workshop: Failed to autosave conversation: {}", e);
    } else {
        pipeline.dirty = false;
    }
}

// ============================================================================
// 5. WorkshopPlugin
// ============================================================================

/// Bevy plugin for the Workshop (System 0: Ideation) module
pub struct WorkshopPlugin;

impl Plugin for WorkshopPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<IdeationPipeline>()
            .init_resource::<claude_bridge::WorkshopClaudeTasks>()
            // Events
            .add_message::<WorkshopSendMessageEvent>()
            .add_message::<WorkshopApproveMcpEvent>()
            .add_message::<WorkshopSkipMcpEvent>()
            .add_message::<WorkshopEditMcpEvent>()
            .add_message::<WorkshopOpenArtifactEvent>()
            .add_message::<ProductCreatedEvent>()
            .add_message::<OptimizeAndBuildEvent>()
            .add_message::<ClaudeResponseEvent>()
            .add_message::<ClaudeErrorEvent>()
            // Core systems: handle user actions → update pipeline state
            .add_systems(Update, (
                handle_send_message,
                handle_approve_mcp,
                handle_skip_mcp,
                handle_claude_response,
                handle_claude_error,
            ))
            // Claude bridge: dispatch async requests + poll responses
            .add_systems(Update, (
                claude_bridge::dispatch_chat_request,
                claude_bridge::dispatch_normalize_request,
                claude_bridge::poll_claude_responses,
            ))
            // Artifact generation: dispatch per-step requests + handle completions
            .add_systems(Update, (
                artifact_gen::dispatch_artifact_requests,
                artifact_gen::handle_artifact_completion,
            ))
            // Autosave: check dirty flag each frame (cheap when not dirty)
            .add_systems(Update, autosave_conversation);
        
        info!("WorkshopPlugin initialized — System 0: Ideation ready");
    }
}
