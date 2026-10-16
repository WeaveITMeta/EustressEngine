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
pub mod mention;
pub mod mention_scanner;
pub mod mention_persistence;
pub mod mention_ui;
pub mod mention_resolver;
/// Vortex-backed semantic searcher. Compiled only under the
/// `workshop-vortex-embeddings` feature; the plugin installs it in place
/// of the default [`mention::SubstringSearcher`] when the feature is on.
#[cfg(feature = "workshop-vortex-embeddings")]
pub mod mention_searcher_vortex;
pub mod claude_bridge;
/// Runs tool calls on a worker thread, in order, and validates any image a
/// tool returns before the model sees it.
pub mod tool_dispatch;
/// When Workshop must stop and show an approval card, whatever the Auto
/// switch says.
pub mod approval_policy;
/// What an approval card shows for a call, and the check that what runs is
/// what was shown.
pub mod card_review;
pub mod artifact_gen;
/// Where a Workshop product's meshes live, and the one-time move of those
/// written to the wrong folder by earlier builds.
pub mod product_meshes;
/// Splits a reply into prose and tables so the chat bubble can draw a grid
/// instead of raw `|---|---|` rows.
pub mod markdown;
pub mod modes;
/// Opt-in AAA quality loop — build → observe → critique → fix before "done".
pub mod gauntlet;
pub mod tools;
pub mod context;
pub mod streams;
pub mod api_reference;
pub mod watchman;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::manufacturing::AllocationDecision;
use std::path::PathBuf;
use uuid::Uuid;

/// System set for workshop core systems (handle_send_message, handle_approve_mcp, etc.)
/// Claude bridge and artifact generation systems run AFTER this set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct WorkshopCoreSystems;

// ============================================================================
// 1. Data Structures
// ============================================================================

/// A single message in the ideation conversation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    /// Which model produced this turn (API id, e.g. `"claude-fable-5"`) —
    /// provenance for the model picker, not itself a setting. `None` for
    /// non-assistant rows (user/artifact/error/etc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
    /// Image attached to this message, stored beside the session on disk.
    /// A path rather than inline base64 so session transcripts stay readable
    /// and reloading a conversation can render the picture again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<PathBuf>,

    // ── Agentic tool-use fields (new) ──────────────────────────────────
    // Populated when the message represents a Claude `tool_use` block or
    // the user-facing approval/execution card for one.

    /// Anthropic `tool_use.id` — links this message to its `tool_result`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// JSON input object Claude sent for this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    /// Result string produced by `ToolRegistry.dispatch()`, fed back to
    /// Claude as the matching `tool_result` block on the next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<String>,
    /// True when this message represents the assistant's `tool_use` block
    /// that spawned an Mcp card, so history reconstruction groups them.
    #[serde(default)]
    pub is_assistant_turn: bool,
    /// An image the tool returned (a camera capture), held in memory only.
    ///
    /// Session files do not store tool results, so nothing is lost on reload,
    /// and keeping it out of the Space keeps captures out of its git history.
    /// Only the newest few stay attached; see `tool_dispatch::KEEP_RECENT_IMAGES`.
    #[serde(skip)]
    pub tool_result_image: Option<tool_dispatch::ToolImage>,
    /// A fingerprint of `tool_input`, taken when the approval card was made.
    /// The approve handler refuses a call whose input no longer matches it, so
    /// what runs is what the card showed (`card_review::approval_check`). In
    /// memory only: a card restored from a session file is never waiting.
    #[serde(skip)]
    pub input_digest: Option<String>,
}

/// Who sent a message in the conversation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// User typed this message
    #[default]
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
    /// A note from the Workshop about the conversation rather than a turn in
    /// it: "Stopped", or which approvals a reply skipped. Rendered as a muted
    /// centred line, never sent to the model, and terminal for dispatch, so a
    /// Stop cannot be undone by the loop picking itself straight back up.
    Notice,
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
            MessageRole::Notice => "notice",
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
    /// Active domain modes inferred from conversation context. General is
    /// always implicit. Multiple modes stack additively.
    pub active_modes: modes::ActiveModes,
    /// True while a Claude tool-use round-trip has unresolved tool calls:
    /// waiting on user approval for one or more MCP cards, or running on the
    /// worker. Dispatch is suppressed until every card has a result (approved
    /// and executed, ran on its own, or skipped), so the next model request
    /// carries all of the turn's results together.
    pub awaiting_tool_approval: bool,
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
            active_modes: modes::ActiveModes::default(),
            awaiting_tool_approval: false,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        });
        self.dirty = true;
        id
    }

    /// Add a notice: a muted line about the conversation itself, such as
    /// "Stopped". See [`MessageRole::Notice`] for why it is terminal for
    /// dispatch and never reaches the model.
    pub fn add_notice_message(&mut self, content: String) -> u32 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        self.messages.push(ChatMessage {
            id,
            role: MessageRole::Notice,
            content,
            timestamp: chrono::Utc::now().to_rfc3339(),
            ..Default::default()
        });
        self.dirty = true;
        id
    }

    /// Close out every agentic tool call that still has no result and is in
    /// one of `statuses`: mark it Skipped and give it `note` as its result.
    /// Returns how many it touched.
    ///
    /// Every `tool_use` the model emitted needs a matching `tool_result` on the
    /// next turn or the provider rejects the whole conversation. Anything that
    /// abandons tool calls, Stop or a reply that redirects instead of
    /// approving, therefore has to answer them explicitly rather than leave
    /// them hanging.
    pub fn skip_unanswered_tool_calls(&mut self, statuses: &[McpCommandStatus], note: &str) -> usize {
        let mut touched = 0;
        for msg in self.messages.iter_mut() {
            let is_open_call = msg.mcp_method.as_deref() == Some("tool_use")
                && msg.tool_use_id.is_some()
                && msg.tool_result.is_none()
                && msg.mcp_status.as_ref().is_some_and(|s| statuses.contains(s));
            if is_open_call {
                msg.tool_result = Some(note.to_string());
                msg.mcp_status = Some(McpCommandStatus::Skipped);
                touched += 1;
            }
        }
        if touched > 0 {
            self.dirty = true;
        }
        touched
    }

    /// Stamp which model produced a message — provenance for cost accounting
    /// and multi-model sessions. Call with the id `add_system_message`
    /// returned, right after adding an assistant/advisor turn.
    pub fn set_message_model(&mut self, message_id: u32, model_api_id: &str) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == message_id) {
            msg.model_used = Some(model_api_id.to_string());
        }
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

    /// Load a conversation from a saved SessionManifest
    pub fn load_from_manifest(&mut self, manifest: &persistence::SessionManifest, session_id: &str) {
        self.reset();
        self.session_id = session_id.to_string();
        self.product_name = manifest.product_name.clone();
        self.total_cost = manifest.total_cost;
        self.state = IdeationState::Conversing;

        // Rebuild messages from entries
        for entry in &manifest.entries {
            let role = match entry.source.as_str() {
                "user" => MessageRole::User,
                "system" => MessageRole::System,
                "mcp" => MessageRole::Mcp,
                "artifact" => MessageRole::Artifact,
                "error" => MessageRole::Error,
                _ => MessageRole::System,
            };

            let id = self.next_message_id;
            self.next_message_id += 1;

            // Rebuild conversation context
            match role {
                MessageRole::User => self.conversation_context.push_str(&format!("\nUser: {}", entry.content)),
                MessageRole::System => self.conversation_context.push_str(&format!("\nWorkshop: {}", entry.content)),
                _ => {}
            }

            self.messages.push(ChatMessage {
                id,
                role: role.clone(),
                content: entry.content.clone(),
                timestamp: chrono::DateTime::from_timestamp_millis(entry.timestamp as i64)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
                mcp_endpoint: entry.mcp_endpoint.clone(),
                mcp_method: None,
                mcp_status: {
                    let parsed = entry.mcp_status.as_deref().and_then(|s| {
                        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
                    });
                    // Historical entries with no saved status were completed calls —
                    // default to Done so they don't show pending approval buttons.
                    if role == MessageRole::Mcp { Some(parsed.unwrap_or(McpCommandStatus::Done)) } else { parsed }
                },
                artifact_path: entry.artifact_path.as_ref().map(std::path::PathBuf::from),
                artifact_type: entry.artifact_type.as_deref().and_then(|s| {
                    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
                }),
                estimated_cost: entry.cost,
                actual_cost: None,
                ..Default::default()
            });
        }

        self.dirty = false;
    }

    /// Check if the pipeline has an active session (not idle)
    pub fn is_active(&self) -> bool {
        self.state != IdeationState::Idle
    }

    /// Get the Slint-compatible pipeline state string
    ///
    /// `conversing` is split out from `running` so the UI can hide the
    /// 13-step pipeline panel while the user is just chatting with Claude.
    /// The huge step list at the top of the Workshop panel should only
    /// appear once real artifact-generation kicks off — until then it
    /// looks like the pipeline is "doing something" when it isn't.
    pub fn state_string(&self) -> &str {
        match &self.state {
            IdeationState::Idle => "idle",
            IdeationState::Conversing => "conversing",
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
    /// PNG bytes of an image attached to this message, if the user pasted
    /// one into the attach strip before sending. Carried on the event
    /// because the staging lives in the Slint action layer while the
    /// pipeline that records the message lives here.
    #[allow(dead_code)]
    pub image_png: Option<Vec<u8>>,
}

/// Messages the user sent while the agent was mid-turn, oldest first.
///
/// A message sent during a turn used to go straight into the conversation,
/// ahead of the reply it was waiting on. The reply then landed after it, the
/// dispatch guard saw the assistant as the last to speak, and the message was
/// never answered. Holding it here until the turn ends keeps the conversation
/// in order and gets every message a reply.
///
/// A `static` rather than a resource for the same reason as the attach strip's
/// staging in `slint_ui.rs`: the × on a queued chip removes entries from inside
/// the Slint action match, which does not take this module's resources.
static QUEUED_MESSAGES: std::sync::Mutex<std::collections::VecDeque<WorkshopSendMessageEvent>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

fn queued_messages() -> std::sync::MutexGuard<'static, std::collections::VecDeque<WorkshopSendMessageEvent>> {
    // A poisoned lock still holds a valid queue; losing the user's messages
    // to an unrelated panic would be worse than reading it.
    QUEUED_MESSAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One-line previews of the queued messages, for the chips above the input.
pub fn queued_message_previews() -> Vec<String> {
    queued_messages()
        .iter()
        .map(|m| {
            let line = m.content.split_whitespace().collect::<Vec<_>>().join(" ");
            match (m.image_png.is_some(), line.is_empty()) {
                (true, true) => "(image)".to_string(),
                (true, false) => format!("(image) {line}"),
                (false, _) => line,
            }
        })
        .collect()
}

/// Drop the queued message at `index`, if there is one.
pub fn remove_queued_message(index: usize) {
    let mut queue = queued_messages();
    if index < queue.len() {
        queue.remove(index);
    }
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

/// Fired when the user picks a different mode from the Workshop dropdown.
#[derive(Message, Debug, Clone)]
pub struct WorkshopSetModeEvent {
    /// Display name (e.g. "General", "Simulation") from the dropdown.
    pub mode_name: String,
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
    space_root: Option<Res<crate::space::SpaceRoot>>,
    // Optional so a missing resource degrades to "never busy" instead of
    // Bevy skipping this system, which would silently kill every send.
    claude_tasks: Option<Res<claude_bridge::WorkshopClaudeTasks>>,
) {
    // A turn is running while a model call is in flight or an approved tool
    // is still executing. Messages that arrive then wait in the queue; see
    // `QUEUED_MESSAGES` for what went wrong when they did not.
    let turn_running = claude_tasks.as_ref().is_some_and(|t| t.is_busy())
        || pipeline.messages.iter().any(|m| {
            m.mcp_method.as_deref() == Some("tool_use")
                && m.tool_result.is_none()
                && matches!(
                    m.mcp_status,
                    Some(McpCommandStatus::Approved) | Some(McpCommandStatus::Running)
                )
        });

    let incoming: Vec<WorkshopSendMessageEvent> = events.read().cloned().collect();
    let mut batch: Vec<WorkshopSendMessageEvent> = {
        let mut queue = queued_messages();
        if turn_running {
            queue.extend(incoming);
            return;
        }
        // Queued messages also wait out an approval. They were written before
        // the user saw what Workshop wants to do next, so letting them release
        // now would answer a request the user has not read.
        if pipeline.awaiting_tool_approval && incoming.is_empty() {
            return;
        }
        queue.drain(..).collect()
    };
    batch.extend(incoming);
    if batch.is_empty() {
        return;
    }

    // A reply while tool calls wait for approval is a redirect: skip them and
    // tell the model so, which lets it read the reply as the answer to what it
    // asked instead of treating its own calls as silently dropped. One step
    // for the user, where approving-then-correcting used to take two.
    if pipeline.awaiting_tool_approval {
        let skipped = pipeline.skip_unanswered_tool_calls(
            &[McpCommandStatus::Pending],
            "Not run. The user replied instead of approving; their message follows.",
        );
        pipeline.awaiting_tool_approval = false;
        if skipped > 0 {
            pipeline.add_notice_message(if skipped == 1 {
                "Skipped the waiting action to follow your reply.".to_string()
            } else {
                format!("Skipped {skipped} waiting actions to follow your reply.")
            });
        }
    }

    for event in batch {
        let content = event.content.trim().to_string();
        // An image on its own is a legitimate message ("what is wrong with
        // this?"). Bailing on empty text alone silently swallowed those.
        if content.is_empty() && event.image_png.is_none() {
            continue;
        }

        // Mode detection runs FIRST (keyword scan against the incoming
        // text). Any newly-activated mode gets a system badge appended
        // *before* the user message so that after we push the user msg,
        // the pipeline still ends with a User role — `dispatch_chat_request`'s
        // `last_is_user` guard needs that invariant.
        let newly_activated = pipeline.active_modes.detect_from_message(&content);
        for mode in &newly_activated {
            pipeline.add_system_message(
                format!("{} — mode activated. {}", mode.badge(), mode.greeting()),
                0.0,
            );
            info!("Workshop: Activated {} mode from user message", mode.display_name());
        }

        // Add user message to conversation (must be last so dispatch guard passes)
        let user_msg_id = pipeline.add_user_message(content.clone());

        // Persist any attachment beside the session and record it on the
        // message, so it renders in the transcript and can be inlined for
        // the model on dispatch.
        if let Some(png) = &event.image_png {
            match space_root.as_ref() {
                Some(sr) => {
                    let dir = sr
                        .0
                        .join("SoulService")
                        .join("Workshop")
                        .join(&pipeline.session_id)
                        .join("attachments");
                    let path = dir.join(format!("msg_{}.png", user_msg_id));
                    let written = std::fs::create_dir_all(&dir)
                        .and_then(|_| std::fs::write(&path, png));
                    match written {
                        Ok(()) => {
                            if let Some(msg) =
                                pipeline.messages.iter_mut().find(|m| m.id == user_msg_id)
                            {
                                msg.image_path = Some(path.clone());
                            }
                            info!(
                                "Workshop: attached image saved to {} ({} bytes)",
                                path.display(),
                                png.len()
                            );
                        }
                        Err(e) => {
                            pipeline.add_error_message(format!(
                                "Workshop: could not save the attached image: {e}"
                            ));
                        }
                    }
                }
                None => {
                    pipeline.add_error_message(
                        "Workshop: an image was attached but no Space is loaded, so it \
                         could not be saved."
                            .to_string(),
                    );
                }
            }
        }

        // The key for the model that will answer, not always the Anthropic
        // one. This checked Anthropic only, which predates the multi-provider
        // picker: someone with just an xAI or OpenAI key who picked Grok or
        // Astra was told "no API key" on every message and could never chat,
        // even though the dispatch path resolves their key correctly.
        let model = global_settings
            .as_ref()
            .map(|g| g.effective_workshop_model())
            .unwrap_or_default();
        // A key still being looked up counts: the message waits in the queue
        // until the store answers (see `dispatch_chat_request`).
        let key_pending = global_settings.as_ref().is_some_and(|g| g.key_pending(model.provider()));
        let has_key = key_pending || match model.provider() {
            crate::soul::Provider::Anthropic => match (&global_settings, &space_settings) {
                (Some(global), Some(space)) => !space.effective_api_key(global).is_empty(),
                _ => false,
            },
            provider => global_settings
                .as_ref()
                .and_then(|g| g.key_for_provider(provider))
                .is_some(),
        };

        if !has_key {
            pipeline.add_error_message(format!(
                "{} needs your {} API key. Add it in Settings > Soul, then send again.",
                model.display_name(),
                model.provider().key_label()
            ));
            continue;
        }

        // If pipeline is idle, start a new session with the user's idea.
        // State MUST be Conversing for `dispatch_chat_request` to pick this
        // up on the next tick.
        if pipeline.state == IdeationState::Idle {
            pipeline.state = IdeationState::Conversing;
            info!("Workshop: Started new ideation session {}", pipeline.session_id);
        }

        info!("Workshop: User message queued for Claude: {} chars", content.len());
    }
}

/// Resource holding all in-flight (background-threaded) tool dispatches: the
/// calls the user approved and the ones that ran without asking. See
/// `tool_dispatch` for how they are run and what each handle carries.
#[derive(Resource, Default)]
pub struct ToolDispatchTasks {
    in_flight: Vec<tool_dispatch::InFlight>,
}

impl ToolDispatchTasks {
    /// Tell every in-flight call to stop and forget them. Used by Stop: a call
    /// that has not started never starts, and a tool that polls sees its
    /// `ToolContext::cancelled` flag and returns early. Returns how many there
    /// were.
    pub(crate) fn cancel_all(&mut self) -> usize {
        for task in &self.in_flight {
            task.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let count = self.in_flight.len();
        self.in_flight.clear();
        count
    }
}

/// The tools that act on the running session rather than on the Space: the
/// game's input, the simulation's values and breakpoints, and the run itself
/// (run or resume, step, pause, stop, and `control_simulation`'s playback).
/// Each is a Write tool, so without this list it would wait for Stop, and an
/// agent could not even end a run. They stay live during Play.
const PLAY_SESSION_TOOLS: [&str; 8] = [
    "play_input",
    "set_sim_value",
    "sim_step",
    "control_simulation",
    "set_breakpoint",
    "run_simulation",
    "pause_simulation",
    "stop_simulation",
];

/// Why a tool call waits for Stop, or `None` to run it. During Play only a
/// read-only tool runs, and the tools that drive the running session
/// ([`PLAY_SESSION_TOOLS`]); any other would change the Space while Stop
/// puts the authored scene back. Asked where a call is dispatched: the
/// Workshop's auto-run and approved cards, and the bridge's `tools.call`.
pub(crate) fn tool_play_refusal(state: Option<&State<crate::play_mode::PlayModeState>>, tool_name: &str, class: Option<eustress_tools::Capability>) -> Option<String> {
    let playing = state.is_some_and(|s| *s.get() != crate::play_mode::PlayModeState::Editing);
    let live = class == Some(eustress_tools::Capability::Read) || PLAY_SESSION_TOOLS.contains(&tool_name);
    (playing && !live).then(|| format!("Stop Play to run {tool_name}: it changes the Space, and Stop puts the scene back."))
}

/// Close a card the person clicked Run on but that must not run. It ends as an
/// Error with the reason as its result, so the conversation is not left waiting
/// for an answer that will never come, and a line in the chat tells the person why.
fn refuse_approved_card(pipeline: &mut IdeationPipeline, message_id: u32, reason: &str) {
    pipeline.update_mcp_status(message_id, McpCommandStatus::Error);
    if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == message_id) {
        msg.tool_result = Some(reason.to_string());
    }
    pipeline.add_notice_message(reason.to_string());
    warn!("Workshop: refused to run approved card {message_id}: {reason}");
}

/// Process MCP command approvals — mark as Running and spawn the tool
/// dispatch on a background thread so long-running tools (e.g. run_bash
/// with curl) don't block the Bevy frame loop.
fn handle_approve_mcp(
    mut events: MessageReader<WorkshopApproveMcpEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
    tool_registry: Option<Res<tools::ToolRegistry>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    auth: Option<Res<crate::auth::AuthState>>,
    display_unit: Option<Res<eustress_common::units::DisplayUnit>>,
    mut tasks: ResMut<ToolDispatchTasks>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
) {
    for event in events.read() {
        // An agentic tool_use card is dispatched right here, so it goes
        // straight to Running. A pipeline step card (normalize, artifact)
        // stays Approved: `dispatch_normalize_request` and
        // `dispatch_artifact_requests` run exactly the cards in that state.
        let is_tool_use = pipeline.messages.iter()
            .find(|m| m.id == event.message_id)
            .is_some_and(|m| m.mcp_method.as_deref() == Some("tool_use"));
        pipeline.update_mcp_status(
            event.message_id,
            if is_tool_use { McpCommandStatus::Running } else { McpCommandStatus::Approved },
        );
        info!("Workshop: MCP command {} approved → dispatching async", event.message_id);

        // Extract the tool-use metadata from the approved card.
        let (tool_name, tool_use_id, tool_input, input_digest) = {
            let Some(msg) = pipeline.messages.iter().find(|m| m.id == event.message_id) else {
                continue;
            };
            match (msg.mcp_endpoint.clone(), msg.tool_use_id.clone(), msg.tool_input.clone()) {
                (Some(n), Some(id), Some(input)) if msg.mcp_method.as_deref() == Some("tool_use") => {
                    (n, id, input, msg.input_digest.clone())
                }
                _ => {
                    // Not an agentic tool_use card (e.g. legacy normalize/artifact
                    // step). Dispatch stays on the old approval flow via
                    // dispatch_normalize_request / dispatch_artifact_request.
                    continue;
                }
            }
        };

        // The rule the card applies to its Run button, applied again here: a click
        // the card would have disabled, or on an input that is no longer the one
        // that was shown, is refused. A call that runs code, deletes or goes online
        // is shown whole or not run (`card_review`).
        let class = eustress_tools::capability::capability_of_call(&tool_name, &tool_input);
        if let Err(refusal) =
            card_review::approval_check(&tool_input, class, input_digest.as_deref())
        {
            refuse_approved_card(&mut pipeline, event.message_id, refusal.message());
            continue;
        }
        // Approved or not, during Play a tool that changes the Space waits
        // for Stop (`tool_play_refusal`).
        if let Some(reason) = tool_play_refusal(play_state.as_deref(), &tool_name, class) {
            refuse_approved_card(&mut pipeline, event.message_id, &reason);
            continue;
        }

        // Build the ToolContext for dispatch.
        let ctx = match (&space_root, &auth) {
            (Some(sr), auth_opt) => {
                let universe_root = crate::space::universe_root_for_path(&sr.0)
                    .unwrap_or_else(|| sr.0.clone());
                let (user_id, username) = auth_opt.as_ref()
                    .and_then(|a| a.user.as_ref())
                    .map(|u| (Some(u.id.clone()), Some(u.username.clone())))
                    .unwrap_or((None, None));
                tools::ToolContext {
                    space_root: sr.0.clone(),
                    universe_root,
                    user_id,
                    username,
                    luau_executor: None,
                    display_unit: display_unit.as_deref().map(|d| d.0.symbol().to_string()),
                    cancelled: None,
                    // This path runs only after the user clicked Approve on
                    // the tool card, so the human has authorized this
                    // specific call. Full capability with a per-call human
                    // gate is the intended posture for the local session.
                    permissions: eustress_tools::Permissions::full()
                        .for_principal("workshop-user-approved"),
                }
            }
            _ => {
                pipeline.add_error_message(
                    "Workshop: tool approval received but no Space is loaded.".to_string()
                );
                continue;
            }
        };

        // Sanity check only: the dispatch thread builds its own registry from
        // `build_tool_registry()`, the same list the resource was built from.
        if tool_registry.is_none() {
            pipeline.add_error_message(
                "Workshop: ToolRegistry missing — cannot dispatch approved tool.".to_string()
            );
            continue;
        }

        // Run it on a worker so a long tool (run_bash with a multi-minute curl,
        // a camera capture waiting for frames) does not block the frame. One
        // call, so a batch of one; `tool_dispatch` has the guarantees.
        let (call, in_flight) = tool_dispatch::PendingCall::new(
            event.message_id,
            tool_name,
            tool_use_id,
            tool_input,
        );
        tasks.in_flight.push(in_flight);
        tool_dispatch::spawn_batch(vec![call], move || {
            let registry = build_tool_registry();
            move |pending: &tool_dispatch::PendingCall| {
                // Each call carries its own cancel flag, so Stop reaches it.
                let mut call_ctx = ctx.clone();
                call_ctx.cancelled = Some(pending.cancel.clone());
                registry.dispatch(
                    &pending.tool_name,
                    &pending.tool_use_id,
                    pending.input.clone(),
                    &call_ctx,
                )
            }
        });
    }
}

/// Poll background tool dispatch threads for completion. When a result
/// arrives, update the pipeline message and clear the approval gate.
fn poll_tool_dispatches(
    mut pipeline: ResMut<IdeationPipeline>,
    mut tasks: ResMut<ToolDispatchTasks>,
) {
    let mut completed_indices = Vec::new();
    let mut image_arrived = false;

    for (i, task) in tasks.in_flight.iter().enumerate() {
        let Some(outcome) = task.take_outcome() else { continue };
        let success = outcome.result.success;
        let has_image = outcome.image.is_some();

        // A card that Stop or Skip already closed keeps what it was closed
        // with: a late answer must not rewrite what the conversation has
        // already said about it. (Only reaching `iter_mut` once a result has
        // actually arrived keeps an idle frame from marking the pipeline
        // changed.)
        let applied = pipeline
            .messages
            .iter_mut()
            .find(|m| m.id == task.message_id)
            .is_some_and(|msg| tool_dispatch::apply_outcome(msg, outcome));

        if applied {
            pipeline.dirty = true;
            image_arrived |= has_image;
            info!("Workshop: tool '{}' (msg {}) completed → success={}",
                task.tool_name, task.message_id, success);
        } else {
            info!("Workshop: tool '{}' (msg {}) finished after its card was closed; result discarded",
                task.tool_name, task.message_id);
        }

        completed_indices.push(i);
    }

    // Remove completed tasks (reverse order to preserve indices)
    for i in completed_indices.into_iter().rev() {
        tasks.in_flight.swap_remove(i);
    }

    // A new picture pushes the oldest out: they cost tokens on every turn.
    if image_arrived {
        tool_dispatch::evict_old_images(
            &mut pipeline.messages,
            tool_dispatch::KEEP_RECENT_IMAGES,
        );
    }

    // Clear the approval gate if every agentic tool_use card now has a
    // result. Dispatch will re-fire on the next tick and Claude will see
    // the tool_result in the next turn.
    let all_resolved = pipeline.messages.iter().all(|m| {
        m.mcp_method.as_deref() != Some("tool_use")
            || m.tool_result.is_some()
            || m.mcp_status == Some(McpCommandStatus::Skipped)
    });
    if all_resolved && pipeline.awaiting_tool_approval {
        pipeline.awaiting_tool_approval = false;
    }
}

/// Process MCP command skips — mark step as skipped and advance
fn handle_skip_mcp(
    mut events: MessageReader<WorkshopSkipMcpEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    for event in events.read() {
        pipeline.update_mcp_status(event.message_id, McpCommandStatus::Skipped);

        // For agentic tool_use cards, Claude still expects a tool_result
        // block paired with the tool_use — otherwise the conversation is
        // malformed on the next turn. Synthesize a "skipped by user" result
        // and mark the approval gate as resolved if all cards are settled.
        if let Some(msg) = pipeline.messages.iter_mut().find(|m| m.id == event.message_id) {
            if msg.mcp_method.as_deref() == Some("tool_use") && msg.tool_result.is_none() {
                msg.tool_result = Some("User skipped this tool call.".to_string());
            }
        }
        let all_resolved = pipeline.messages.iter().all(|m| {
            m.mcp_method.as_deref() != Some("tool_use")
                || m.tool_result.is_some()
                || m.mcp_status == Some(McpCommandStatus::Skipped)
        });
        if all_resolved {
            pipeline.awaiting_tool_approval = false;
        }

        // Find which step this MCP command belongs to by checking content for "step=" param
        let step_idx = pipeline.messages.iter()
            .find(|m| m.id == event.message_id)
            .and_then(|msg| {
                // Normalization MCP uses /mcp/ideation/normalize endpoint
                if msg.mcp_endpoint.as_deref() == Some("/mcp/ideation/normalize") {
                    return Some(0u32);
                }
                // Artifact steps embed "step={param}" in content
                for idx in 1u32..=10 {
                    if let Some(step) = artifact_gen::ArtifactStep::from_step_index(idx) {
                        if msg.content.contains(&format!("step={}", step.step_param())) {
                            return Some(idx);
                        }
                    }
                }
                None
            });

        if let Some(idx) = step_idx {
            // Mark the step as skipped and clone the label before mutating
            let label = pipeline.steps.get(idx as usize)
                .map(|s| s.label.clone())
                .unwrap_or_else(|| "step".to_string());
            if let Some(step) = pipeline.steps.get_mut(idx as usize) {
                step.status = StepStatus::Skipped;
            }
            pipeline.add_system_message(format!("{} skipped.", label), 0.0);

            // Propose the next artifact step if there is one
            let next_idx = idx + 1;
            if let Some(next_step) = artifact_gen::ArtifactStep::from_step_index(next_idx) {
                let next_label = pipeline.steps.get(next_idx as usize)
                    .map(|s| s.label.clone())
                    .unwrap_or_else(|| "next artifact".to_string());
                let description = format!(
                    "Generate {} (step={})\nEstimated cost: ~${:.2} (Sonnet)",
                    next_label,
                    next_step.step_param(),
                    next_step.estimated_cost()
                );
                pipeline.add_mcp_command(
                    description,
                    "/mcp/ideation/brief".to_string(),
                    "POST".to_string(),
                    next_step.estimated_cost(),
                );
            } else if idx == 10 {
                // Last step skipped — pipeline complete
                pipeline.state = IdeationState::Complete;
                pipeline.add_system_message(
                    "All steps processed. Click \"Optimize & Build\" to hand off to Systems 1-8.".to_string(),
                    0.0,
                );
            }
        } else {
            pipeline.add_system_message("Step skipped.".to_string(), 0.0);
        }

        info!("Workshop: MCP command {} skipped", event.message_id);
    }
}

/// Process MCP command edits — acknowledge the request (full edit UI is future work)
fn handle_edit_mcp(
    mut events: MessageReader<WorkshopEditMcpEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    for event in events.read() {
        pipeline.add_system_message(
            "MCP command editing is not yet supported. You can skip and re-run with different parameters.".to_string(),
            0.0,
        );
        info!("Workshop: MCP command {} edit requested (not yet implemented)", event.message_id);
    }
}

/// Open an artifact file or its containing directory
fn handle_open_artifact(
    mut events: MessageReader<WorkshopOpenArtifactEvent>,
) {
    for event in events.read() {
        let path = std::path::Path::new(&event.path);
        if path.exists() {
            if let Err(e) = open::that(&event.path) {
                warn!("Workshop: Failed to open artifact {:?}: {}", event.path, e);
            }
        } else {
            // Try opening the parent directory
            if let Some(parent) = path.parent() {
                if parent.exists() {
                    if let Err(e) = open::that(parent) {
                        warn!("Workshop: Failed to open directory {:?}: {}", parent, e);
                    }
                }
            }
            warn!("Workshop: Artifact path does not exist: {:?}", event.path);
        }
    }
}

/// User picked a mode from the Workshop dropdown. Map display name →
/// WorkshopMode and replace the pipeline's active modes with the new single
/// selection (General is always kept as the base).
fn handle_set_mode(
    mut events: MessageReader<WorkshopSetModeEvent>,
    mut pipeline: ResMut<IdeationPipeline>,
) {
    for event in events.read() {
        let mode = match event.mode_name.as_str() {
            "Simulation"    => modes::WorkshopMode::Simulation,
            "Manufacturing" => modes::WorkshopMode::Manufacturing,
            "Fabrication"   => modes::WorkshopMode::Fabrication,
            "Finance"       => modes::WorkshopMode::Finance,
            "Shopping"      => modes::WorkshopMode::Shopping,
            "Travel"        => modes::WorkshopMode::Travel,
            "Supply Chain"  => modes::WorkshopMode::SupplyChain,
            "Warehousing"   => modes::WorkshopMode::Warehousing,
            "Universe"      => modes::WorkshopMode::UniverseBrowsing,
            _               => modes::WorkshopMode::General,
        };
        let mut new_modes = modes::ActiveModes::default();
        if mode != modes::WorkshopMode::General {
            new_modes.activate(mode);
        }
        pipeline.active_modes = new_modes;
        info!("Workshop: mode switched to {:?}", mode);
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
                // Parse the brief from Claude's TOML response
                match normalizer::parse_brief_from_toml(&event.content) {
                    Ok(brief) => {
                        // Validate the brief
                        if let Err(validation_errors) = normalizer::validate_brief(&brief) {
                            pipeline.add_error_message(format!(
                                "Brief validation warnings: {}",
                                validation_errors.join(", ")
                            ));
                            // Validation failed — don't store invalid brief, revert to conversing
                            if let Some(step) = pipeline.steps.get_mut(0) {
                                step.status = StepStatus::Error;
                            }
                            pipeline.state = IdeationState::Conversing;
                            continue;
                        }

                        // Step 0 succeeded
                        if let Some(step) = pipeline.steps.get_mut(0) {
                            step.status = StepStatus::Done;
                            step.artifact_count += 1;
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
                        // Mark step as error and revert to conversing so user can retry
                        if let Some(step) = pipeline.steps.get_mut(0) {
                            step.status = StepStatus::Error;
                        }
                        pipeline.state = IdeationState::Conversing;
                    }
                }
            }
            
            // Artifact steps 1-6: file writing, artifact messages, and next-step
            // proposals are handled entirely by artifact_gen::handle_artifact_completion.
            // We intentionally do nothing here to avoid double-counting artifacts.
            Some(step_idx) if step_idx >= 1 && step_idx <= 10 => {}
            
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
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    if !pipeline.dirty || !pipeline.is_active() {
        return;
    }

    let space = space_root.as_ref().map(|sr| sr.0.as_path());
    if let Err(e) = persistence::save_session_to_space(&pipeline, space) {
        warn!("Workshop: Failed to autosave conversation: {}", e);
    } else {
        pipeline.dirty = false;
    }
}

// ============================================================================
// 4b. Vortex searcher installer (feature-gated)
// ============================================================================

/// Swap the MentionIndex's active searcher to the Vortex-backed one as
/// soon as the Universe root becomes available. The swap is one-shot per
/// Universe; tracking happens in a local `Option<PathBuf>` resource-
/// like closure state.
#[cfg(feature = "workshop-vortex-embeddings")]
fn install_vortex_searcher_on_universe_load(
    mut last_installed: Local<Option<std::path::PathBuf>>,
    mut index: ResMut<mention::MentionIndex>,
) {
    let Some(universe) = index.universe_root().map(|p| p.to_path_buf()) else { return };
    if last_installed.as_deref() == Some(&universe) { return; }

    let knowledge_dir = mention_persistence::knowledge_dir(&universe);
    match mention_searcher_vortex::VortexSearcher::try_open(&knowledge_dir) {
        Ok(searcher) => {
            index.swap_searcher(Box::new(searcher));
            info!("Workshop: Vortex mention searcher installed for {}", universe.display());
        }
        Err(e) => {
            warn!("Workshop: Vortex searcher unavailable ({}); falling back to substring", e);
        }
    }
    *last_installed = Some(universe);
}

// ============================================================================
// 5. WorkshopPlugin
// ============================================================================

/// The one list of tools the Workshop offers: every tool shipped by
/// `eustress-tools`, the mode-specific tools and the AI camera.
///
/// Everything that dispatches tools builds its registry here: the plugin's
/// resource, the worker threads that run tool calls, and the capability test
/// below. This used to be three hand-copied lists (the plugin, the approved-tool
/// thread, the test), which is exactly how the shared crate's tools once went
/// missing from the Workshop. A tool added here is offered, dispatched and
/// checked for a capability classification everywhere at once.
///
/// Cheap enough to call per dispatch: every handler is a stateless unit struct.
pub(crate) fn build_tool_registry() -> tools::ToolRegistry {
    let mut registry = tools::ToolRegistry::default();
    // The baseline (every tool shipped by `eustress-tools`). A manual
    // enumeration drifted before: tools added to the shared crate never made
    // it into the Workshop because the list was kept by hand in two places.
    tools::register_all_tools(&mut registry);
    // Manufacturing mode tools
    registry.register(modes::manufacturing::NormalizeBriefTool);
    registry.register(modes::manufacturing::QueryManufacturersTool);
    registry.register(modes::manufacturing::QueryInvestorsTool);
    registry.register(modes::manufacturing::AllocateProductTool);
    // Simulation mode tools
    registry.register(modes::simulation::ControlSimulationTool);
    registry.register(modes::simulation::SetBreakpointTool);
    registry.register(modes::simulation::ExportRecordingTool);
    // Supply Chain mode tools
    registry.register(modes::supply_chain::RunScenarioTool);
    registry.register(modes::supply_chain::ForecastDemandTool);
    registry.register(modes::supply_chain::ScoreSupplierRiskTool);
    // Warehousing mode tools
    registry.register(modes::warehousing::InventoryCheckTool);
    registry.register(modes::warehousing::StorageOptimizeTool);
    // Finance mode tools
    registry.register(modes::finance::CalculateCostTool);
    registry.register(modes::finance::EstimateTaxTool);
    // Fabrication mode tools
    registry.register(modes::fabrication::SelectProcessTool);
    // Shopping mode tools
    registry.register(modes::shopping::PriceProductTool);
    // Travel mode tools
    registry.register(modes::travel::EstimateShippingTool);
    // The AI camera: move, aim, frame and capture with the camera the AI
    // controls. Hosts opt in; the Workshop does, so it can see what it built.
    eustress_tools::ai_camera_tools::register(&mut registry);
    registry
}

/// Bevy plugin for the Workshop (System 0: Ideation) module
pub struct WorkshopPlugin;

impl Plugin for WorkshopPlugin {
    fn build(&self, app: &mut App) {
        // The tool registry: General + domain-specific tools.
        let registry = build_tool_registry();
        tracing::info!("Workshop: registered {} MCP tools", registry.tool_count());

        app
            // Stage 2: Tool registry + context + streams
            .insert_resource(registry)
            .init_resource::<streams::StreamAwareContext>()
            // Stage 3: Staged diffs
            .init_resource::<tools::diff_tools::StagedChanges>()
            // Legacy resources (kept for Manufacturing mode compatibility)
            .init_resource::<IdeationPipeline>()
            .init_resource::<claude_bridge::WorkshopClaudeTasks>()
            // @-mention index — backs the Workshop chat autocomplete.
            // The default searcher is a substring scanner; when the
            // `workshop-vortex-embeddings` feature is on, the plugin
            // attempts to install the Vortex-backed semantic searcher
            // below (post-init, after the Universe root is known).
            .init_resource::<mention::MentionIndex>()
            .init_resource::<mention_scanner::UniverseScanTask>()
            .init_resource::<ToolDispatchTasks>()
            // Watchman — proactive simulation monitor
            .init_resource::<watchman::WatchmanConfig>()
            .init_resource::<watchman::WatchmanState>()
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
            .add_message::<WorkshopSetModeEvent>()
            .add_message::<gauntlet::WorkshopSetGauntletEvent>()
            .init_resource::<gauntlet::GauntletMode>()
            // Core systems: handle user actions → update pipeline state
            // Must run AFTER SlintSystems::Drain so WorkshopSendMessageEvent etc. are available
            .add_systems(Update, (
                handle_send_message,
                handle_approve_mcp,
                handle_skip_mcp,
                handle_edit_mcp,
                handle_open_artifact,
                handle_claude_response,
                handle_claude_error,
                handle_set_mode,
                gauntlet::handle_set_gauntlet,
                // Last in the chain so a Stop lands before `dispatch_chat_request`
                // (ordered after this set) looks at the conversation this frame.
                claude_bridge::apply_workshop_stop,
            ).chain().after(crate::ui::SlintSystems::Drain).in_set(WorkshopCoreSystems))
            // Poll background tool dispatch threads (non-blocking approve)
            .add_systems(Update, poll_tool_dispatches.after(WorkshopCoreSystems))
            // One-shot: restore the persisted Gauntlet toggle on launch.
            .add_systems(Update, gauntlet::seed_gauntlet_from_settings)
            // Claude bridge: dispatch async requests + poll responses
            // Must run AFTER WorkshopCoreSystems so pipeline.state is updated before dispatch checks it
            .add_systems(Update, (
                claude_bridge::dispatch_chat_request,
                claude_bridge::dispatch_normalize_request,
                claude_bridge::poll_claude_responses,
                claude_bridge::poll_agentic_responses,
                claude_bridge::poll_advisor_responses,
            ).after(WorkshopCoreSystems))
            // Artifact generation: dispatch per-step requests + handle completions
            .add_systems(Update, (
                artifact_gen::dispatch_artifact_requests,
                artifact_gen::handle_artifact_completion,
            ).after(WorkshopCoreSystems))
            // One-time per Space open: move product meshes that earlier builds wrote
            // to <Universe>/.eustress/assets/meshes into <Universe>/assets/meshes.
            .add_systems(Update, artifact_gen::migrate_product_meshes_on_space_open)
            // Autosave: check dirty flag each frame (cheap when not dirty)
            .add_systems(Update, autosave_conversation)
            // Watchman: proactive simulation monitor — injects alert
            // messages into the pipeline when sim values breach thresholds.
            // Runs only during Playing state, after the Claude bridge so
            // alerts don't collide with in-flight dispatches.
            .add_systems(
                Update,
                watchman::watchman_monitor
                    .run_if(in_state(crate::play_mode::PlayModeState::Playing))
                    .after(WorkshopCoreSystems),
            )
            // Reset Watchman cooldowns when entering Play mode
            .add_systems(
                OnEnter(crate::play_mode::PlayModeState::Playing),
                watchman::watchman_reset_on_play,
            )
            // Mention index systems — see workshop/mention.rs and
            // workshop/mention_scanner.rs for architecture notes.
            //
            // GATING: the live-mirror, rescan-trigger, and autosave systems
            // carry the `mention::mention_indexing_enabled` run-condition. It is
            // `false` only for a LARGE / streaming binary-ECS Space
            // (`active_db::streaming_active()` — the same "huge scene" gate
            // `instance_loader` and the Explorer use). There the chat
            // @-autocomplete is non-essential and the live mirror's per-change
            // O(N) upsert + the multi-second universe walk + periodic
            // full-catalogue JSON autosave dominate the frame, so the scheduler
            // skips them entirely. For every normal-sized Space the condition is
            // `true` and the @-mention system runs unchanged.
            //
            // `poll_universe_scan` is intentionally LEFT UNGATED: if a universe
            // scan was spawned on an early frame before `streaming_active()`
            // flipped true, the in-flight task must still be drained/merged
            // rather than left un-polled. It early-returns when the task is None,
            // so leaving it ungated costs effectively nothing per frame.
            //
            // Live ECS mirror: reactive enumeration of the currently-loaded
            // Space's entities/services/scripts. Runs every frame but only
            // touches `Added`/`Changed`/`Removed` entities.
            .add_systems(
                Update,
                mention::update_mention_index_live
                    .run_if(mention::mention_indexing_enabled),
            )
            // Pull AuthState → MentionIndex.active_user so MRU persists
            // under the correct bucket across sessions.
            .add_systems(
                Update,
                mention::sync_mention_active_user
                    .run_if(mention::mention_indexing_enabled),
            )
            // Static scanner: walks the entire Universe for TOMLs + media
            // on Universe switch. Runs on IoTaskPool so large scans don't
            // stall the main thread. (poll left ungated — see note above.)
            .add_systems(
                Update,
                (
                    mention_scanner::trigger_rescan_on_universe_change,
                    mention_persistence::autosave_index,
                )
                    .run_if(mention::mention_indexing_enabled),
            )
            .add_systems(Update, mention_scanner::poll_universe_scan);

        // Feature-gated: swap in the Vortex semantic searcher once the
        // Universe root is known. Runs at most once per Universe load;
        // falls back silently to the substring searcher on init failure.
        #[cfg(feature = "workshop-vortex-embeddings")]
        app.add_systems(Update, install_vortex_searcher_on_universe_load);
        
        info!("WorkshopPlugin initialized — System 0: Ideation ready");
    }
}

#[cfg(test)]
mod capability_guard_tests {
    use super::*;

    /// Every tool the ENGINE registers must carry a capability
    /// classification, or it is advertised to callers and then refused
    /// at dispatch.
    ///
    /// `eustress-tools` already has an equivalent guard, but it can
    /// only see its own `register_all_tools` baseline — it is blind by
    /// construction to the mode tools registered here. That blind spot
    /// let all seventeen of them ship advertised-but-uncallable: the
    /// tool appeared in the list, the model called it, and dispatch
    /// answered "has no capability classification". This test closes
    /// the gap on the side that actually owns the registrations.
    #[test]
    fn every_engine_registered_tool_is_classified() {
        // The same builder the plugin and the dispatch threads use, so this
        // checks exactly the set that ships, not a copy of it.
        let registry = build_tool_registry();

        let unclassified: Vec<&str> = registry
            .all_tools()
            .into_iter()
            .map(|d| d.name)
            .filter(|name| eustress_tools::capability::capability_of(name).is_none())
            .collect();

        assert!(
            unclassified.is_empty(),
            "these tools are registered and advertised but have no capability \
             classification, so every call is denied at dispatch: {unclassified:?}. \
             Add them to capability_of() in tools/src/capability.rs."
        );
    }
}

#[cfg(test)]
mod tool_dispatch_wiring_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// Stop must reach calls that are running or queued on the worker, not only
    /// close their cards: a long-polling tool watches this flag.
    #[test]
    fn stop_tells_every_in_flight_call_and_forgets_them() {
        let mut tasks = ToolDispatchTasks::default();
        let mut flags = Vec::new();
        for id in 1..=3u32 {
            let (_call, in_flight) = tool_dispatch::PendingCall::new(
                id,
                "await_simulation".to_string(),
                format!("toolu_{id}"),
                serde_json::json!({}),
            );
            flags.push(in_flight.cancel.clone());
            tasks.in_flight.push(in_flight);
        }

        assert_eq!(tasks.cancel_all(), 3);
        assert!(flags.iter().all(|f| f.load(Ordering::Relaxed)), "every call must be told to stop");
        assert!(tasks.in_flight.is_empty(), "stopped calls are forgotten, so a late answer is never applied");
        assert_eq!(tasks.cancel_all(), 0, "a second Stop finds nothing left to cancel");
    }

    /// A card the person approved but that must not run closes with the reason,
    /// so the conversation is not left waiting on a result that never comes.
    #[test]
    fn a_refused_approval_closes_the_card_with_the_reason() {
        let mut pipeline = IdeationPipeline::default();
        let id = pipeline.add_mcp_command(
            "run_bash({})".to_string(),
            "run_bash".to_string(),
            "tool_use".to_string(),
            0.0,
        );
        pipeline.awaiting_tool_approval = true;

        let reason = card_review::Refusal::TooLong.message();
        refuse_approved_card(&mut pipeline, id, reason);

        let card = pipeline.messages.iter().find(|m| m.id == id).expect("the card stays");
        assert_eq!(card.mcp_status, Some(McpCommandStatus::Error));
        assert_eq!(card.tool_result.as_deref(), Some(reason), "the model reads this as the tool's result");
        assert!(
            pipeline.messages.iter().any(|m| m.role == MessageRole::Notice && m.content == reason),
            "the person is told why"
        );
    }
}

/// During Play a Workshop or bridge tool call that changes the Space waits
/// for Stop; a read-only tool and the tools that drive the running session
/// still run.
#[cfg(test)]
mod play_gate_tests {
    use super::*;
    use crate::play_mode::PlayModeState;

    /// The tools that drive the running session, named here rather than read
    /// from [`PLAY_SESSION_TOOLS`], so a tool dropped from the list fails.
    const SESSION_TOOLS: [&str; 8] = [
        "play_input",
        "set_sim_value",
        "sim_step",
        "control_simulation",
        "set_breakpoint",
        "run_simulation",
        "pause_simulation",
        "stop_simulation",
    ];

    #[test]
    fn play_lets_only_reads_and_session_tools_run() {
        use eustress_tools::Capability;
        assert_eq!(PLAY_SESSION_TOOLS.as_slice(), SESSION_TOOLS.as_slice());
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            let state = State::new(state);
            let refusal = tool_play_refusal(Some(&state), "create_entity", Some(Capability::Write));
            assert_eq!(refusal.as_deref(), Some("Stop Play to run create_entity: it changes the Space, and Stop puts the scene back."));
            assert!(tool_play_refusal(Some(&state), "run_experiment", Some(Capability::Write)).is_some());
            assert!(tool_play_refusal(Some(&state), "delete_entity", Some(Capability::Destructive)).is_some());
            assert!(tool_play_refusal(Some(&state), "list_directory", Some(Capability::Read)).is_none());
            for tool in SESSION_TOOLS {
                assert!(tool_play_refusal(Some(&state), tool, Some(Capability::Write)).is_none(), "{tool}");
            }
        }
        let editing = State::new(PlayModeState::Editing);
        assert!(tool_play_refusal(Some(&editing), "create_entity", Some(Capability::Write)).is_none());
        assert!(tool_play_refusal(None, "create_entity", Some(Capability::Write)).is_none());
    }

    /// Approve a card for `tool` in `state` and run the Workshop's approval
    /// handler once, with no Space open: a call the Play gate lets through
    /// stops at "no Space is loaded", and a refused one closes with its
    /// reason. Returns the card's result and whether the handler got as far
    /// as the Space.
    fn approve(state: PlayModeState, tool: &str) -> (Option<String>, bool) {
        let mut app = App::new();
        app.add_message::<WorkshopApproveMcpEvent>();
        app.insert_resource(State::new(state));
        app.init_resource::<ToolDispatchTasks>();
        let mut pipeline = IdeationPipeline::default();
        let id = pipeline.add_mcp_command(format!("{tool}({{}})"), tool.to_string(), "tool_use".to_string(), 0.0);
        let card = pipeline.messages.iter_mut().find(|m| m.id == id).expect("the card");
        card.tool_use_id = Some(format!("toolu_{tool}"));
        card.tool_input = Some(serde_json::json!({}));
        app.insert_resource(pipeline);
        app.add_systems(Update, handle_approve_mcp);
        app.world_mut().write_message(WorkshopApproveMcpEvent { message_id: id });
        app.update();
        let pipeline = app.world().resource::<IdeationPipeline>();
        let result = pipeline.messages.iter().find(|m| m.id == id).and_then(|m| m.tool_result.clone());
        let no_space = "Workshop: tool approval received but no Space is loaded.";
        (result, pipeline.messages.iter().any(|m| m.role == MessageRole::Error && m.content == no_space))
    }

    /// Through an approved card each tool that drives the running session
    /// passes the Play gate, though each is a Write tool; a Write tool that
    /// is not one of them is refused with its reason, and while editing the
    /// same tool passes.
    #[test]
    fn the_session_tools_run_in_play_through_an_approved_card() {
        use eustress_tools::Capability;
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            for tool in SESSION_TOOLS {
                let class = eustress_tools::capability::capability_of_call(tool, &serde_json::json!({}));
                assert_eq!(class, Some(Capability::Write), "{tool}");
                assert_eq!(approve(state, tool), (None, true), "{state:?} {tool}");
            }
            let reason = "Stop Play to run run_experiment: it changes the Space, and Stop puts the scene back.".to_string();
            assert_eq!(approve(state, "run_experiment"), (Some(reason), false), "{state:?}");
        }
        assert_eq!(approve(PlayModeState::Editing, "run_experiment"), (None, true));
    }
}
