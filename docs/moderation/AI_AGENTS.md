# AI Agents Moderation Documentation

**Autonomous AI Agent Architecture for Eustress Engine Content Moderation**

> *Best Match Dynamic: Autonomy → Tool Calls for 95% accuracy judgments, fallback rules for edge cases*

**Last Updated:** December 03, 2025  
**Status:** Pre-Release Architecture  
**Applies To:** All AI-driven moderation, content classification, and safety systems

---

## Table of Contents

1. [Overview](#overview)
2. [Agent Architecture](#agent-architecture)
3. [A2A Protocol Integration](#a2a-protocol-integration)
4. [MCP Tool Calls](#mcp-tool-calls)
5. [Supervised Learning Pipeline](#supervised-learning-pipeline)
6. [Safety Guardrails](#safety-guardrails)
7. [Rust Implementation](#rust-implementation)
8. [Testing & Validation](#testing--validation)

---

## Overview

### Design Philosophy

```
Dynamic: Moderation + AI Agents → Autonomy
Implication: Tool Calls for 95% accuracy, fallback rules, scales to 1M req/sec
Benefit: Prevents Roblox-like exposures, minimizes human moderator burden
```

**Mantra:** "Judge Fair, Act Swift" — AI agents make rapid, consistent decisions with human oversight.

### Agent Hierarchy

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        MODERATION ORCHESTRATOR                          │
│                    (A2A Protocol Coordinator)                           │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐         ┌───────────────┐         ┌───────────────┐
│ TEXT AGENT    │         │ IMAGE AGENT   │         │ BEHAVIOR AGENT│
│ (Chat/UGC)    │         │ (Avatar/Tex)  │         │ (Actions)     │
└───────────────┘         └───────────────┘         └───────────────┘
        │                           │                           │
        └───────────────────────────┼───────────────────────────┘
                                    │
                                    ▼
                        ┌───────────────────┐
                        │  ESCALATION AGENT │
                        │  (Human Handoff)  │
                        └───────────────────┘
```

---

## Reputation & Status System

### Creator Value Weighting

Reputation affects moderation decisions to protect valuable creators from false positives while still holding everyone accountable for serious offenses.

```rust
// crates/agents/src/reputation.rs

/// User reputation and status for moderation weighting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserReputation {
    pub user_id: String,
    pub status: CreatorStatus,
    pub score: f32,              // 0.0 to 1.0
    pub net_worth: NetWorth,     // Account value metrics
    pub history: ModerationHistory,
}

/// Creator status tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreatorStatus {
    /// New account, limited history
    New,
    
    /// Regular user with some history
    Established,
    
    /// Active contributor with positive track record
    Trusted,
    
    /// Significant creator with substantial contributions
    Verified,
    
    /// Top-tier creator, major platform contributor
    Star,
    
    /// Partnership-level creator
    Partner,
}

/// Account value metrics for moderation weighting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetWorth {
    /// Total assets created
    pub assets_created: u64,
    
    /// Assets with positive ratings
    pub quality_assets: u64,
    
    /// Total downloads/uses of their content
    pub total_engagement: u64,
    
    /// Revenue generated (if applicable)
    pub revenue_generated: f64,
    
    /// Community contributions (tutorials, help, etc.)
    pub community_score: f32,
    
    /// Account age in days
    pub account_age_days: u64,
    
    /// Calculated value score (normalized)
    pub value_score: f32,
}

impl NetWorth {
    /// Calculate normalized value score
    pub fn calculate_value_score(&self) -> f32 {
        let asset_score = (self.quality_assets as f32 / (self.assets_created as f32 + 1.0)).min(1.0);
        let engagement_score = (self.total_engagement as f32 / 10000.0).min(1.0);
        let age_score = (self.account_age_days as f32 / 365.0).min(1.0);
        
        // Weighted combination
        asset_score * 0.3 + engagement_score * 0.4 + self.community_score * 0.2 + age_score * 0.1
    }
}

/// Moderation history tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationHistory {
    /// Reports against this user
    pub reports_against: Vec<ReportRecord>,
    
    /// Reports this user has made
    pub reports_made: Vec<ReportRecord>,
    
    /// Accuracy of reports made (affects report weight)
    pub reporter_accuracy: f32,
    
    /// Strikes accumulated
    pub strikes: u32,
    
    /// Strike decay (strikes reduce over time with good behavior)
    pub last_strike_date: Option<chrono::DateTime<chrono::Utc>>,
    
    /// Appeals won/lost
    pub appeals: AppealHistory,
}

/// Reputation service for moderation decisions
pub struct ReputationService {
    db: sqlx::PgPool,
    cache: redis::aio::ConnectionManager,
}

impl ReputationService {
    /// Get user reputation (cached)
    pub async fn get_user(&self, user_id: &str) -> Result<UserReputation, ReputationError> {
        // Check cache first
        let cache_key = format!("reputation:{}", user_id);
        if let Ok(cached) = self.cache.get::<_, String>(&cache_key).await {
            if let Ok(rep) = serde_json::from_str(&cached) {
                return Ok(rep);
            }
        }
        
        // Load from DB
        let rep = self.load_reputation(user_id).await?;
        
        // Cache for 5 minutes
        let _ = self.cache.set_ex(&cache_key, serde_json::to_string(&rep)?, 300).await;
        
        Ok(rep)
    }
    
    /// Calculate moderation weight based on reputation
    /// Higher weight = more scrutiny for moderation decisions
    pub fn moderation_weight(&self, reputation: &UserReputation, severity: Severity) -> ModerationWeight {
        // Critical+ severity = no reputation protection
        if !severity.considers_reputation() {
            return ModerationWeight {
                review_priority: Priority::Critical,
                auto_action_allowed: true,
                human_review_required: false,
                reputation_shield: false,
            };
        }
        
        // Calculate shield strength based on reputation
        let shield_strength = match reputation.status {
            CreatorStatus::New => 0.0,
            CreatorStatus::Established => 0.2,
            CreatorStatus::Trusted => 0.4,
            CreatorStatus::Verified => 0.6,
            CreatorStatus::Star => 0.8,
            CreatorStatus::Partner => 0.9,
        };
        
        // Modify by net worth and history
        let value_modifier = reputation.net_worth.value_score * 0.2;
        let history_modifier = if reputation.history.strikes > 0 {
            -0.1 * reputation.history.strikes as f32
        } else {
            0.1  // Clean history bonus
        };
        
        let total_shield = (shield_strength + value_modifier + history_modifier).clamp(0.0, 0.95);
        
        ModerationWeight {
            review_priority: if total_shield > 0.7 {
                Priority::Low  // High-rep = careful review
            } else if total_shield > 0.4 {
                Priority::Normal
            } else {
                Priority::High  // Low-rep = faster action
            },
            auto_action_allowed: total_shield < 0.5,
            human_review_required: total_shield > 0.6,
            reputation_shield: total_shield > 0.3,
        }
    }
    
    /// Recalculate reputation after moderation event
    pub async fn recalculate(&self, user_id: &str) -> Result<(), ReputationError> {
        let history = self.load_history(user_id).await?;
        
        // Calculate new reputation score
        let base_score = self.calculate_base_score(user_id).await?;
        let strike_penalty = history.strikes as f32 * 0.1;
        let appeal_bonus = history.appeals.success_rate() * 0.1;
        let age_bonus = self.calculate_age_bonus(user_id).await?;
        
        let new_score = (base_score - strike_penalty + appeal_bonus + age_bonus).clamp(0.0, 1.0);
        
        // Update in DB
        sqlx::query!(
            "UPDATE user_reputation SET score = $1, updated_at = NOW() WHERE user_id = $2",
            new_score, user_id
        )
        .execute(&self.db)
        .await?;
        
        // Invalidate cache
        self.cache.del(&format!("reputation:{}", user_id)).await?;
        
        Ok(())
    }
}

#[derive(Debug)]
pub struct ModerationWeight {
    pub review_priority: Priority,
    pub auto_action_allowed: bool,
    pub human_review_required: bool,
    pub reputation_shield: bool,  // Protects against low-confidence actions
}
```

---

## Agent Architecture

### Core Agent Trait

```rust
// crates/agents/src/core.rs
use async_trait::async_trait;

/// Core moderation agent trait
#[async_trait]
pub trait ModerationAgent: Send + Sync {
    /// Agent identity card (A2A compatible)
    fn agent_card(&self) -> AgentCard;
    
    /// Process content and return moderation decision
    async fn moderate(&self, content: &Content) -> Result<ModerationDecision, AgentError>;
    
    /// Explain reasoning (GDPR Art. 22 compliance)
    async fn explain(&self, decision: &ModerationDecision) -> Result<Explanation, AgentError>;
    
    /// Learn from feedback (supervised learning)
    async fn learn(&mut self, feedback: &Feedback) -> Result<(), AgentError>;
    
    /// Available tool calls
    fn tools(&self) -> Vec<ToolDefinition>;
}

/// A2A-compatible agent identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub id: String,
    pub name: String,
    pub version: String,
    pub capabilities: Vec<Capability>,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub performance: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub accuracy: f32,           // Target: 95%+
    pub latency_p50_ms: u64,     // Target: <50ms
    pub latency_p99_ms: u64,     // Target: <200ms
    pub throughput_rps: u64,     // Target: 10K+ per agent
}

/// Moderation decision with confidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationDecision {
    pub content_id: String,
    pub action: ModerationAction,
    pub confidence: f32,
    pub categories: Vec<ViolationCategory>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub agent_id: String,
    pub requires_human_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModerationAction {
    /// Content is safe
    Allow,
    
    /// Content requires modification (e.g., blur, filter)
    Modify { modifications: Vec<Modification> },
    
    /// Content should be hidden pending review
    Hide { reason: String },
    
    /// Content should be removed
    Remove { reason: String, severity: Severity },
    
    /// Escalate to human moderator
    Escalate { priority: Priority, context: String },
}

/// Severity levels: Mild → XRay (most severe)
/// Named after radiation exposure levels for intuitive understanding
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// Level 0: No concern - normal content
    None,
    
    /// Level 1: Mild - slightly edgy but acceptable
    /// Examples: mild profanity, competitive trash talk, heated debate
    /// Action: Monitor, no intervention
    Mild,
    
    /// Level 2: Moderate - borderline content requiring attention
    /// Examples: strong language, controversial opinions, suggestive content
    /// Action: Flag for review, may filter for minors
    Moderate,
    
    /// Level 3: Elevated - content that crosses lines
    /// Examples: targeted insults, misinformation, age-inappropriate
    /// Action: Hide pending review, warn user
    Elevated,
    
    /// Level 4: High - clearly violating content
    /// Examples: harassment, hate speech, explicit content
    /// Action: Remove, strike on account
    High,
    
    /// Level 5: Severe - dangerous content requiring immediate action
    /// Examples: threats, doxxing, self-harm encouragement
    /// Action: Immediate removal, account suspension
    Severe,
    
    /// Level 6: Critical - illegal content
    /// Examples: CSAM, terrorism, imminent violence
    /// Action: Instant removal, law enforcement notification, permanent ban
    Critical,
    
    /// Level 7: XRay - maximum severity, platform-threatening
    /// Examples: coordinated attacks, state-actor threats, mass exploitation
    /// Action: All above + platform-wide alerts, legal escalation
    XRay,
}

impl Severity {
    /// Get recommended action for severity level
    pub fn recommended_action(&self) -> ModerationAction {
        match self {
            Severity::None | Severity::Mild => ModerationAction::Allow,
            Severity::Moderate => ModerationAction::Modify {
                modifications: vec![Modification::AgeGate],
            },
            Severity::Elevated => ModerationAction::Hide {
                reason: "Content under review".into(),
            },
            Severity::High | Severity::Severe => ModerationAction::Remove {
                reason: "Violation of community guidelines".into(),
                severity: *self,
            },
            Severity::Critical | Severity::XRay => ModerationAction::Remove {
                reason: "Critical policy violation".into(),
                severity: *self,
            },
        }
    }
    
    /// Does this severity require law enforcement notification?
    pub fn requires_law_enforcement(&self) -> bool {
        matches!(self, Severity::Critical | Severity::XRay)
    }
    
    /// Should reputation be considered at this severity?
    pub fn considers_reputation(&self) -> bool {
        // Critical+ = no reputation consideration (act immediately)
        !matches!(self, Severity::Critical | Severity::XRay)
    }
    
    /// Strike weight for account history
    pub fn strike_weight(&self) -> u32 {
        match self {
            Severity::None | Severity::Mild => 0,
            Severity::Moderate => 1,
            Severity::Elevated => 2,
            Severity::High => 3,
            Severity::Severe => 5,
            Severity::Critical => 10,
            Severity::XRay => 100,  // Instant permaban territory
        }
    }
}
```

### Specialized Agents

```rust
// crates/agents/src/text_agent.rs

/// Text content moderation agent
pub struct TextModerationAgent {
    classifier: TextClassifier,
    toxicity_model: ToxicityModel,
    pii_detector: PiiDetector,
    language_detector: LanguageDetector,
    child_safety_filter: ChildSafetyFilter,
}

#[async_trait]
impl ModerationAgent for TextModerationAgent {
    fn agent_card(&self) -> AgentCard {
        AgentCard {
            id: "text-moderation-v1".into(),
            name: "Text Moderation Agent".into(),
            version: "1.0.0".into(),
            capabilities: vec![
                Capability::TextClassification,
                Capability::ToxicityDetection,
                Capability::PiiDetection,
                Capability::LanguageDetection,
                Capability::ChildSafetyFiltering,
            ],
            performance: PerformanceMetrics {
                accuracy: 0.96,
                latency_p50_ms: 15,
                latency_p99_ms: 80,
                throughput_rps: 50000,
            },
            ..Default::default()
        }
    }
    
    async fn moderate(&self, content: &Content) -> Result<ModerationDecision, AgentError> {
        let text = content.as_text()?;
        
        // Parallel classification
        let (toxicity, pii, child_safety, categories) = tokio::join!(
            self.toxicity_model.score(text),
            self.pii_detector.detect(text),
            self.child_safety_filter.check(text),
            self.classifier.classify(text),
        );
        
        // Decision logic
        let action = if child_safety?.is_violation {
            ModerationAction::Remove {
                reason: "Child safety violation".into(),
                severity: Severity::Critical,
            }
        } else if toxicity?.score > 0.9 {
            ModerationAction::Remove {
                reason: format!("High toxicity: {:.2}", toxicity?.score),
                severity: Severity::High,
            }
        } else if toxicity?.score > 0.7 {
            ModerationAction::Hide {
                reason: "Moderate toxicity - pending review".into(),
            }
        } else if !pii?.findings.is_empty() {
            ModerationAction::Modify {
                modifications: vec![Modification::RedactPii(pii?.findings)],
            }
        } else {
            ModerationAction::Allow
        };
        
        let confidence = categories?.confidence.min(toxicity?.confidence);
        
        Ok(ModerationDecision {
            content_id: content.id.clone(),
            action,
            confidence,
            categories: categories?.labels,
            timestamp: chrono::Utc::now(),
            agent_id: self.agent_card().id,
            requires_human_review: confidence < 0.9,
        })
    }
    
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "classify_text",
                description: "Classify text content for violations",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "context": { "type": "string" }
                    }
                }),
            },
            ToolDefinition {
                name: "detect_pii",
                description: "Detect personally identifiable information",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    }
                }),
            },
        ]
    }
}
```

---

## A2A Protocol Integration

### Google A2A for Cross-Vendor Interoperability

```rust
// crates/agents/src/a2a.rs
use reqwest::Client;

/// A2A (Agent-to-Agent) protocol client
/// Preferred over proprietary APIs for cross-vendor support
pub struct A2AClient {
    client: Client,
    registry: AgentRegistry,
}

impl A2AClient {
    /// Discover available agents
    pub async fn discover(&self, capability: Capability) -> Vec<AgentCard> {
        self.registry
            .find_by_capability(capability)
            .await
            .unwrap_or_default()
    }
    
    /// Send task to agent
    pub async fn send_task(&self, agent_id: &str, task: Task) -> Result<TaskResult, A2AError> {
        let agent = self.registry.get(agent_id).await?;
        
        let response = self.client
            .post(&agent.endpoint)
            .header("Content-Type", "application/json")
            .header("X-A2A-Protocol-Version", "1.0")
            .json(&A2ARequest {
                task_id: uuid::Uuid::new_v4().to_string(),
                task,
                callback_url: None,
            })
            .send()
            .await?;
        
        response.json().await.map_err(Into::into)
    }
    
    /// Delegate moderation to specialized agent
    pub async fn delegate_moderation(
        &self,
        content: &Content,
        required_capability: Capability,
    ) -> Result<ModerationDecision, A2AError> {
        // Find best agent for capability
        let agents = self.discover(required_capability).await;
        let best_agent = agents
            .iter()
            .max_by(|a, b| a.performance.accuracy.partial_cmp(&b.performance.accuracy).unwrap())
            .ok_or(A2AError::NoAgentAvailable)?;
        
        // Send moderation task
        let task = Task::Moderate {
            content: content.clone(),
            context: ModerationContext::default(),
        };
        
        let result = self.send_task(&best_agent.id, task).await?;
        
        match result {
            TaskResult::ModerationDecision(decision) => Ok(decision),
            TaskResult::Error(e) => Err(A2AError::AgentError(e)),
            _ => Err(A2AError::UnexpectedResult),
        }
    }
}

/// A2A message format
#[derive(Serialize, Deserialize)]
pub struct A2AMessage {
    pub version: String,
    pub sender: AgentCard,
    pub recipient: String,
    pub task: Task,
    pub context: serde_json::Value,
}

/// Task types for inter-agent communication
#[derive(Serialize, Deserialize)]
pub enum Task {
    Moderate { content: Content, context: ModerationContext },
    Classify { input: serde_json::Value },
    Explain { decision_id: String },
    Learn { feedback: Feedback },
    HealthCheck,
}
```

### Orchestrator Implementation

```rust
// crates/agents/src/orchestrator.rs

/// Central moderation orchestrator
pub struct ModerationOrchestrator {
    a2a_client: A2AClient,
    text_agent: TextModerationAgent,
    image_agent: ImageModerationAgent,
    behavior_agent: BehaviorModerationAgent,
    escalation_queue: EscalationQueue,
    metrics: MetricsCollector,
}

impl ModerationOrchestrator {
    /// Route content to appropriate agent(s)
    pub async fn moderate(&self, content: Content) -> Result<ModerationDecision, OrchestratorError> {
        let start = std::time::Instant::now();
        
        // Determine content type and route
        let decision = match content.content_type {
            ContentType::Text => self.text_agent.moderate(&content).await?,
            ContentType::Image => self.image_agent.moderate(&content).await?,
            ContentType::Behavior => self.behavior_agent.moderate(&content).await?,
            ContentType::Mixed => {
                // Parallel moderation for mixed content
                let (text_decision, image_decision) = tokio::join!(
                    self.text_agent.moderate(&content),
                    self.image_agent.moderate(&content),
                );
                
                // Combine decisions (most restrictive wins)
                self.combine_decisions(text_decision?, image_decision?)
            }
        };
        
        // Check if escalation needed
        if decision.requires_human_review || decision.confidence < 0.9 {
            self.escalation_queue.enqueue(EscalationRequest {
                content: content.clone(),
                ai_decision: decision.clone(),
                priority: self.calculate_priority(&decision),
            }).await?;
        }
        
        // Record metrics
        self.metrics.record_moderation(
            &decision,
            start.elapsed(),
        );
        
        Ok(decision)
    }
    
    /// Combine multiple agent decisions
    fn combine_decisions(
        &self,
        text: ModerationDecision,
        image: ModerationDecision,
    ) -> ModerationDecision {
        // Most restrictive action wins
        let action = match (&text.action, &image.action) {
            (ModerationAction::Remove { .. }, _) | (_, ModerationAction::Remove { .. }) => {
                text.action.clone().max(image.action.clone())
            }
            (ModerationAction::Hide { .. }, _) | (_, ModerationAction::Hide { .. }) => {
                ModerationAction::Hide { reason: "Combined decision".into() }
            }
            (ModerationAction::Modify { .. }, _) | (_, ModerationAction::Modify { .. }) => {
                // Merge modifications
                ModerationAction::Modify {
                    modifications: vec![], // Merge logic here
                }
            }
            _ => ModerationAction::Allow,
        };
        
        ModerationDecision {
            content_id: text.content_id,
            action,
            confidence: text.confidence.min(image.confidence),
            categories: [text.categories, image.categories].concat(),
            timestamp: chrono::Utc::now(),
            agent_id: "orchestrator".into(),
            requires_human_review: text.requires_human_review || image.requires_human_review,
        }
    }
}
```

---

## MCP Tool Calls

### Model Context Protocol Integration

```rust
// crates/agents/src/mcp.rs

/// MCP (Model Context Protocol) tool interface
pub struct McpToolProvider {
    tools: HashMap<String, Box<dyn McpTool>>,
}

pub trait McpTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn execute(&self, params: serde_json::Value) -> BoxFuture<'_, Result<serde_json::Value, ToolError>>;
}

impl McpToolProvider {
    /// Register moderation tools
    pub fn new() -> Self {
        let mut tools: HashMap<String, Box<dyn McpTool>> = HashMap::new();
        
        tools.insert("moderate_text".into(), Box::new(ModerateTextTool::new()));
        tools.insert("moderate_image".into(), Box::new(ModerateImageTool::new()));
        tools.insert("check_user_history".into(), Box::new(UserHistoryTool::new()));
        tools.insert("apply_action".into(), Box::new(ApplyActionTool::new()));
        tools.insert("escalate".into(), Box::new(EscalateTool::new()));
        
        Self { tools }
    }
    
    /// Execute tool call from agent
    pub async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let tool = self.tools.get(&call.name)
            .ok_or(ToolError::NotFound(call.name.clone()))?;
        
        let result = tool.execute(call.parameters).await?;
        
        Ok(ToolResult {
            call_id: call.id,
            result,
        })
    }
}

/// Moderation text tool implementation
pub struct ModerateTextTool {
    agent: TextModerationAgent,
}

impl McpTool for ModerateTextTool {
    fn name(&self) -> &str { "moderate_text" }
    
    fn description(&self) -> &str {
        "Analyze text content for policy violations including toxicity, PII, and child safety issues"
    }
    
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text content to moderate"
                },
                "context": {
                    "type": "string",
                    "description": "Context about where this text appears (chat, profile, etc.)"
                },
                "user_age": {
                    "type": "integer",
                    "description": "Age of the user who created/will view this content"
                }
            },
            "required": ["text"]
        })
    }
    
    fn execute(&self, params: serde_json::Value) -> BoxFuture<'_, Result<serde_json::Value, ToolError>> {
        Box::pin(async move {
            let text = params["text"].as_str().ok_or(ToolError::InvalidParams)?;
            let context = params["context"].as_str().unwrap_or("unknown");
            
            let content = Content {
                id: uuid::Uuid::new_v4().to_string(),
                content_type: ContentType::Text,
                data: ContentData::Text(text.into()),
                context: context.into(),
                ..Default::default()
            };
            
            let decision = self.agent.moderate(&content).await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            
            Ok(serde_json::to_value(decision)?)
        })
    }
}
```

---

## Supervised Learning Pipeline

### Continuous Learning from Human Feedback

```rust
// crates/ml-core/src/training/supervised.rs

/// Supervised learning pipeline for moderation agents
pub struct SupervisedLearningPipeline {
    feedback_buffer: FeedbackBuffer,
    model_registry: ModelRegistry,
    training_scheduler: TrainingScheduler,
}

#[derive(Debug, Clone)]
pub struct Feedback {
    pub decision_id: String,
    pub original_decision: ModerationDecision,
    pub corrected_action: Option<ModerationAction>,
    pub reviewer_id: String,
    pub reviewer_type: ReviewerType,
    pub notes: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl SupervisedLearningPipeline {
    /// Record feedback for future training
    pub async fn record_feedback(&self, feedback: Feedback) -> Result<(), TrainingError> {
        // Validate feedback
        if feedback.corrected_action.is_some() {
            // This is a correction - high value for training
            self.feedback_buffer.push_correction(feedback.clone()).await?;
        } else {
            // This is confirmation - use for validation
            self.feedback_buffer.push_confirmation(feedback.clone()).await?;
        }
        
        // Check if retraining threshold reached
        if self.feedback_buffer.corrections_count().await > 1000 {
            self.schedule_retraining().await?;
        }
        
        Ok(())
    }
    
    /// Retrain model with accumulated feedback
    pub async fn retrain(&self, agent_id: &str) -> Result<ModelVersion, TrainingError> {
        // Get current model
        let current_model = self.model_registry.get_latest(agent_id).await?;
        
        // Prepare training data
        let corrections = self.feedback_buffer.drain_corrections().await?;
        let training_data = self.prepare_training_data(corrections)?;
        
        // Fine-tune model (using Candle)
        let new_model = self.fine_tune(&current_model, &training_data).await?;
        
        // Validate new model
        let validation_results = self.validate_model(&new_model).await?;
        
        if validation_results.accuracy < current_model.metrics.accuracy {
            return Err(TrainingError::AccuracyRegression);
        }
        
        // Deploy new model
        let version = self.model_registry.deploy(agent_id, new_model).await?;
        
        Ok(version)
    }
    
    async fn fine_tune(
        &self,
        base_model: &Model,
        training_data: &TrainingData,
    ) -> Result<Model, TrainingError> {
        use candle_core::{Device, Tensor};
        use candle_nn::optim::AdamW;
        
        let device = Device::cuda_if_available(0).unwrap_or(Device::Cpu);
        
        // Load base model weights
        let mut model = base_model.load(&device)?;
        
        // Configure optimizer
        let params = candle_nn::ParamsAdamW {
            lr: 1e-5,  // Low learning rate for fine-tuning
            ..Default::default()
        };
        let mut optimizer = AdamW::new(model.parameters(), params)?;
        
        // Training loop
        for epoch in 0..3 {
            for batch in training_data.batches(32) {
                let loss = model.forward_loss(&batch)?;
                optimizer.backward_step(&loss)?;
            }
        }
        
        Ok(model)
    }
}
```

---

## First Amendment-Considerate Toxicity Detection

### Philosophy: Truth Over Tone

Our toxicity detection is designed to protect **free expression** while preventing genuine harm. We are NOT the UK—we will not censor opinions, political speech, or uncomfortable truths.

```rust
// crates/ml-core/src/toxicity/first_amendment.rs

/// Toxicity classifier that respects free speech
pub struct FirstAmendmentToxicityClassifier {
    base_classifier: ToxicityModel,
    protected_speech_detector: ProtectedSpeechDetector,
    harassment_detector: HarassmentDetector,
}

/// Categories of protected speech (NOT moderated)
#[derive(Debug, Clone)]
pub enum ProtectedSpeech {
    /// Political opinions, even controversial ones
    PoliticalCommentary,
    
    /// Religious expression and debate
    ReligiousExpression,
    
    /// Criticism of public figures, companies, governments
    PublicCriticism,
    
    /// Satire, parody, humor
    SatireAndParody,
    
    /// Academic or educational discussion
    EducationalContent,
    
    /// News reporting and journalism
    Journalism,
    
    /// Artistic expression
    ArtisticExpression,
    
    /// Factual statements (even uncomfortable ones)
    FactualStatement,
    
    /// Personal opinions and beliefs
    PersonalOpinion,
}

/// What we DO moderate (actual harm, not just offense)
#[derive(Debug, Clone)]
pub enum ActualHarm {
    /// Direct threats of violence
    ViolentThreats,
    
    /// Targeted harassment campaigns
    TargetedHarassment,
    
    /// Doxxing (revealing private information)
    Doxxing,
    
    /// Defamation (false statements presented as fact)
    Defamation,
    
    /// Incitement to imminent lawless action (Brandenburg test)
    Incitement,
    
    /// Child exploitation
    ChildExploitation,
    
    /// Fraud and scams
    Fraud,
    
    /// Impersonation for harm
    MaliciousImpersonation,
}

impl FirstAmendmentToxicityClassifier {
    /// Classify content with First Amendment protections
    pub async fn classify(&self, text: &str, context: &Context) -> ToxicityResult {
        // 1. Check for protected speech FIRST
        let protected = self.protected_speech_detector.detect(text, context).await;
        
        // 2. Run base toxicity classifier
        let base_toxicity = self.base_classifier.score(text).await;
        
        // 3. Check for actual harm (what we DO moderate)
        let harm_check = self.harassment_detector.check_actual_harm(text, context).await;
        
        // Decision logic:
        // - If protected speech AND no actual harm → ALLOW
        // - If actual harm detected → moderate based on severity
        // - High toxicity score alone is NOT sufficient for moderation
        
        if harm_check.harm_detected {
            // Actual harm found - proceed with moderation
            return ToxicityResult {
                action: self.determine_action(&harm_check),
                reason: ToxicityReason::ActualHarm(harm_check.harm_type),
                confidence: harm_check.confidence,
                protected_speech_override: false,
            };
        }
        
        if protected.is_protected {
            // Protected speech - do NOT moderate even if "toxic" by ML standards
            return ToxicityResult {
                action: ModerationAction::Allow,
                reason: ToxicityReason::ProtectedSpeech(protected.category),
                confidence: protected.confidence,
                protected_speech_override: true,
            };
        }
        
        // Neither protected nor harmful - use toxicity score but with high threshold
        if base_toxicity.score > 0.95 {
            ToxicityResult {
                action: ModerationAction::Escalate {
                    priority: Priority::Normal,
                    context: "High toxicity but no clear harm - needs human review".into(),
                },
                reason: ToxicityReason::HighToxicityScore(base_toxicity.score),
                confidence: base_toxicity.score,
                protected_speech_override: false,
            }
        } else {
            ToxicityResult {
                action: ModerationAction::Allow,
                reason: ToxicityReason::BelowThreshold,
                confidence: 1.0 - base_toxicity.score,
                protected_speech_override: false,
            }
        }
    }
}

/// Harassment detector focused on actual harm, not hurt feelings
pub struct HarassmentDetector {
    /// Patterns for targeted harassment (repeated, directed at individual)
    targeting_patterns: Vec<TargetingPattern>,
    
    /// Threat detection
    threat_detector: ThreatDetector,
    
    /// Doxxing detection
    doxxing_detector: DoxxingDetector,
}

impl HarassmentDetector {
    pub async fn check_actual_harm(&self, text: &str, context: &Context) -> HarmCheckResult {
        // Check for Brandenburg test: imminent lawless action
        if self.threat_detector.is_imminent_threat(text) {
            return HarmCheckResult {
                harm_detected: true,
                harm_type: ActualHarm::Incitement,
                confidence: 0.95,
            };
        }
        
        // Check for doxxing
        if self.doxxing_detector.contains_private_info(text, context) {
            return HarmCheckResult {
                harm_detected: true,
                harm_type: ActualHarm::Doxxing,
                confidence: 0.9,
            };
        }
        
        // Check for targeted harassment (requires pattern, not single message)
        if self.is_targeted_harassment(text, context).await {
            return HarmCheckResult {
                harm_detected: true,
                harm_type: ActualHarm::TargetedHarassment,
                confidence: 0.85,
            };
        }
        
        // No actual harm detected
        HarmCheckResult {
            harm_detected: false,
            harm_type: ActualHarm::ViolentThreats, // placeholder
            confidence: 0.0,
        }
    }
    
    /// Targeted harassment requires PATTERN, not single message
    async fn is_targeted_harassment(&self, text: &str, context: &Context) -> bool {
        // Single rude message ≠ harassment
        // Harassment requires:
        // 1. Repeated behavior (3+ instances)
        // 2. Directed at specific individual
        // 3. Intended to cause distress
        
        let user_history = context.get_sender_history().await;
        let target = context.get_target_user();
        
        if let (Some(history), Some(target)) = (user_history, target) {
            let negative_interactions = history.interactions_with(&target.id)
                .filter(|i| i.sentiment < -0.5)
                .count();
            
            // Requires pattern of 3+ negative interactions
            negative_interactions >= 3
        } else {
            false
        }
    }
}

/// Examples of what we DO and DON'T moderate
/// 
/// ✅ ALLOWED (Protected Speech):
/// - "I think the president is an idiot" (political opinion)
/// - "Your religion is wrong" (religious debate)
/// - "This game sucks" (criticism)
/// - "You played terribly that round" (competitive context)
/// - "Climate change is/isn't real" (opinion/debate)
/// - Curse words in general conversation
/// 
/// ❌ MODERATED (Actual Harm):
/// - "I'm going to find where you live and hurt you" (threat)
/// - "Here's [person's] home address" (doxxing)
/// - Repeated daily messages telling someone to kill themselves (harassment)
/// - "[False claim] is a pedophile" (defamation)
/// - Coordinated brigading against an individual
```

---

## Asset Pipeline Moderation

### UGC Asset Scanning

```rust
// crates/agents/src/asset_pipeline.rs

/// Asset moderation pipeline for UGC
pub struct AssetModerationPipeline {
    image_scanner: ImageModerationAgent,
    mesh_scanner: MeshAnalyzer,
    audio_scanner: AudioModerationAgent,
    metadata_scanner: MetadataScanner,
    hash_database: AssetHashDatabase,
}

/// Asset types requiring moderation
#[derive(Debug, Clone)]
pub enum AssetType {
    Texture { width: u32, height: u32, format: String },
    Mesh { vertices: u64, has_uv: bool },
    Audio { duration_secs: f32, format: String },
    Script { language: String, lines: u64 },
    Prefab { components: Vec<String> },
    Animation { duration_secs: f32, bones: u64 },
}

#[derive(Debug)]
pub struct AssetModerationResult {
    pub asset_id: String,
    pub asset_type: AssetType,
    pub action: ModerationAction,
    pub severity: Severity,
    pub findings: Vec<AssetFinding>,
    pub hash: AssetHash,  // For duplicate detection
}

#[derive(Debug)]
pub enum AssetFinding {
    /// Inappropriate imagery in texture
    InappropriateImagery { category: String, confidence: f32 },
    
    /// Copyrighted content detected
    CopyrightViolation { source: String, confidence: f32 },
    
    /// Hidden content in mesh/texture
    HiddenContent { description: String },
    
    /// Malicious script patterns
    MaliciousCode { pattern: String, risk: String },
    
    /// Inappropriate audio content
    InappropriateAudio { category: String, timestamp: f32 },
    
    /// PII in metadata
    MetadataPii { field: String, pii_type: String },
}

impl AssetModerationPipeline {
    /// Full asset scan pipeline
    pub async fn moderate_asset(&self, asset: &Asset) -> Result<AssetModerationResult, AssetError> {
        // 1. Check hash against known violations
        let hash = self.compute_asset_hash(asset)?;
        if self.hash_database.is_blocked(&hash).await {
            return Ok(AssetModerationResult {
                asset_id: asset.id.clone(),
                asset_type: asset.asset_type.clone(),
                action: ModerationAction::Remove {
                    reason: "Previously blocked content".into(),
                    severity: Severity::High,
                },
                severity: Severity::High,
                findings: vec![],
                hash,
            });
        }
        
        // 2. Type-specific scanning
        let findings = match &asset.asset_type {
            AssetType::Texture { .. } => self.scan_texture(asset).await?,
            AssetType::Mesh { .. } => self.scan_mesh(asset).await?,
            AssetType::Audio { .. } => self.scan_audio(asset).await?,
            AssetType::Script { .. } => self.scan_script(asset).await?,
            AssetType::Prefab { .. } => self.scan_prefab(asset).await?,
            AssetType::Animation { .. } => self.scan_animation(asset).await?,
        };
        
        // 3. Metadata scan (all asset types)
        let metadata_findings = self.metadata_scanner.scan(&asset.metadata).await?;
        let all_findings: Vec<AssetFinding> = [findings, metadata_findings].concat();
        
        // 4. Determine severity and action
        let severity = self.calculate_severity(&all_findings);
        let action = severity.recommended_action();
        
        Ok(AssetModerationResult {
            asset_id: asset.id.clone(),
            asset_type: asset.asset_type.clone(),
            action,
            severity,
            findings: all_findings,
            hash,
        })
    }
    
    async fn scan_texture(&self, asset: &Asset) -> Result<Vec<AssetFinding>, AssetError> {
        let image = asset.load_as_image()?;
        
        let mut findings = vec![];
        
        // NSFW detection
        let nsfw_result = self.image_scanner.detect_nsfw(&image).await?;
        if nsfw_result.score > 0.8 {
            findings.push(AssetFinding::InappropriateImagery {
                category: nsfw_result.category,
                confidence: nsfw_result.score,
            });
        }
        
        // Hidden content detection (steganography)
        if self.image_scanner.detect_hidden_content(&image).await? {
            findings.push(AssetFinding::HiddenContent {
                description: "Potential steganographic content detected".into(),
            });
        }
        
        // Copyright detection (perceptual hash against known IPs)
        if let Some(match_) = self.image_scanner.check_copyright(&image).await? {
            findings.push(AssetFinding::CopyrightViolation {
                source: match_.source,
                confidence: match_.confidence,
            });
        }
        
        Ok(findings)
    }
    
    async fn scan_mesh(&self, asset: &Asset) -> Result<Vec<AssetFinding>, AssetError> {
        let mesh = asset.load_as_mesh()?;
        
        let mut findings = vec![];
        
        // Shape analysis for inappropriate forms
        let shape_analysis = self.mesh_scanner.analyze_shape(&mesh).await?;
        if shape_analysis.inappropriate_probability > 0.85 {
            findings.push(AssetFinding::InappropriateImagery {
                category: "inappropriate_shape".into(),
                confidence: shape_analysis.inappropriate_probability,
            });
        }
        
        // Check for hidden geometry
        if self.mesh_scanner.has_hidden_geometry(&mesh).await? {
            findings.push(AssetFinding::HiddenContent {
                description: "Hidden geometry detected inside mesh".into(),
            });
        }
        
        Ok(findings)
    }
}
```

---

## Safety Guardrails

### Multi-Layer Safety System

```rust
// crates/agents/src/safety.rs

/// Safety guardrails for AI agents
pub struct SafetyGuardrails {
    /// Hard rules that always override AI decisions
    hard_rules: Vec<HardRule>,
    
    /// Confidence thresholds for escalation
    confidence_threshold: f32,
    
    /// Rate limiting for automated actions
    rate_limiter: RateLimiter,
    
    /// Audit logger
    audit_log: AuditLogger,
    
    /// Reputation service for creator protection
    reputation_service: ReputationService,
}

#[derive(Debug)]
pub struct HardRule {
    pub name: String,
    pub condition: fn(&ModerationDecision) -> bool,
    pub override_action: ModerationAction,
    pub priority: u8,
    pub respects_reputation: bool,  // false for critical violations
}

impl SafetyGuardrails {
    pub fn new() -> Self {
        Self {
            hard_rules: vec![
                // Rule 1: Always remove CSAM
                HardRule {
                    name: "csam_removal".into(),
                    condition: |d| d.categories.contains(&ViolationCategory::Csam),
                    override_action: ModerationAction::Remove {
                        reason: "CSAM - immediate removal".into(),
                        severity: Severity::Critical,
                    },
                    priority: 0,
                },
                // Rule 2: Always escalate potential self-harm
                HardRule {
                    name: "self_harm_escalation".into(),
                    condition: |d| d.categories.contains(&ViolationCategory::SelfHarm),
                    override_action: ModerationAction::Escalate {
                        priority: Priority::Urgent,
                        context: "Potential self-harm - requires immediate human review".into(),
                    },
                    priority: 1,
                },
                // Rule 3: Low confidence = human review
                HardRule {
                    name: "low_confidence_escalation".into(),
                    condition: |d| d.confidence < 0.85,
                    override_action: ModerationAction::Escalate {
                        priority: Priority::Normal,
                        context: "Low confidence AI decision".into(),
                    },
                    priority: 10,
                },
            ],
            confidence_threshold: 0.9,
            rate_limiter: RateLimiter::new(1000), // Max 1000 auto-actions per minute
            audit_log: AuditLogger::new(),
        }
    }
    
    /// Apply safety guardrails to AI decision
    pub async fn apply(&self, mut decision: ModerationDecision) -> ModerationDecision {
        // Check hard rules in priority order
        for rule in &self.hard_rules {
            if (rule.condition)(&decision) {
                // Log override
                self.audit_log.log(AuditEvent::SafetyOverride {
                    original_decision: decision.clone(),
                    rule_name: rule.name.clone(),
                    override_action: rule.override_action.clone(),
                });
                
                decision.action = rule.override_action.clone();
                decision.requires_human_review = true;
                break;
            }
        }
        
        // Rate limit automated removals
        if matches!(decision.action, ModerationAction::Remove { .. }) {
            if !self.rate_limiter.try_acquire().await {
                // Too many removals - escalate instead
                decision.action = ModerationAction::Escalate {
                    priority: Priority::High,
                    context: "Rate limit reached - manual review required".into(),
                };
            }
        }
        
        decision
    }
}

/// GDPR Art. 22 - Right to explanation
pub async fn explain_decision(decision: &ModerationDecision) -> Explanation {
    Explanation {
        decision_id: decision.content_id.clone(),
        summary: format!(
            "Content was {} due to: {}",
            match &decision.action {
                ModerationAction::Allow => "allowed",
                ModerationAction::Remove { .. } => "removed",
                ModerationAction::Hide { .. } => "hidden",
                ModerationAction::Modify { .. } => "modified",
                ModerationAction::Escalate { .. } => "escalated for review",
            },
            decision.categories.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")
        ),
        factors: vec![
            Factor { name: "Toxicity score".into(), value: "0.92".into(), weight: 0.4 },
            Factor { name: "Context".into(), value: "Public chat".into(), weight: 0.2 },
            Factor { name: "User history".into(), value: "Previous warnings".into(), weight: 0.2 },
        ],
        confidence: decision.confidence,
        human_reviewable: true,
        appeal_available: true,
    }
}
```

---

## Rust Implementation

### Crate Dependencies

```toml
# crates/agents/Cargo.toml
[dependencies]
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.11", features = ["json"] }
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"

# ML dependencies
candle-core = "0.3"
candle-nn = "0.3"
candle-transformers = "0.3"

# A2A protocol
# (custom implementation based on Google A2A spec)
```

### Agent Registration

```rust
// crates/agents/src/registry.rs

/// Central agent registry for discovery
pub struct AgentRegistry {
    agents: dashmap::DashMap<String, RegisteredAgent>,
    redis: redis::aio::ConnectionManager,
}

#[derive(Clone)]
pub struct RegisteredAgent {
    pub card: AgentCard,
    pub endpoint: String,
    pub health_status: HealthStatus,
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

impl AgentRegistry {
    /// Register agent for discovery
    pub async fn register(&self, agent: RegisteredAgent) -> Result<(), RegistryError> {
        // Store locally
        self.agents.insert(agent.card.id.clone(), agent.clone());
        
        // Publish to Redis for distributed discovery
        let key = format!("agents:{}", agent.card.id);
        self.redis.set_ex(&key, serde_json::to_string(&agent)?, 300).await?;
        
        Ok(())
    }
    
    /// Find agents by capability
    pub async fn find_by_capability(&self, capability: Capability) -> Vec<AgentCard> {
        self.agents
            .iter()
            .filter(|a| a.card.capabilities.contains(&capability))
            .map(|a| a.card.clone())
            .collect()
    }
}
```

---

## Testing & Validation

### Agent Test Suite

```rust
#[cfg(test)]
mod agent_tests {
    use super::*;
    
    #[tokio::test]
    async fn test_text_agent_accuracy() {
        let agent = TextModerationAgent::new();
        let test_cases = load_test_dataset("text_moderation_gold.json");
        
        let mut correct = 0;
        let mut total = 0;
        
        for case in test_cases {
            let decision = agent.moderate(&case.content).await.unwrap();
            if decision.action.matches(&case.expected_action) {
                correct += 1;
            }
            total += 1;
        }
        
        let accuracy = correct as f32 / total as f32;
        assert!(accuracy >= 0.95, "Accuracy {} below 95% threshold", accuracy);
    }
    
    #[tokio::test]
    async fn test_latency_requirements() {
        let agent = TextModerationAgent::new();
        let content = create_test_content("This is a test message");
        
        let mut latencies = vec![];
        
        for _ in 0..100 {
            let start = std::time::Instant::now();
            agent.moderate(&content).await.unwrap();
            latencies.push(start.elapsed().as_millis() as u64);
        }
        
        latencies.sort();
        let p50 = latencies[50];
        let p99 = latencies[99];
        
        assert!(p50 < 50, "P50 latency {} exceeds 50ms", p50);
        assert!(p99 < 200, "P99 latency {} exceeds 200ms", p99);
    }
    
    #[tokio::test]
    async fn test_safety_guardrails() {
        let guardrails = SafetyGuardrails::new();
        
        // Test CSAM always removed
        let csam_decision = ModerationDecision {
            categories: vec![ViolationCategory::Csam],
            action: ModerationAction::Allow, // AI incorrectly allowed
            confidence: 0.99,
            ..Default::default()
        };
        
        let result = guardrails.apply(csam_decision).await;
        assert!(matches!(result.action, ModerationAction::Remove { .. }));
    }
    
    #[tokio::test]
    async fn test_a2a_protocol() {
        let client = A2AClient::new_test();
        
        // Test agent discovery
        let agents = client.discover(Capability::TextClassification).await;
        assert!(!agents.is_empty());
        
        // Test task delegation
        let content = create_test_content("Test");
        let result = client.delegate_moderation(&content, Capability::TextClassification).await;
        assert!(result.is_ok());
    }
}
```

---

## Metrics & Monitoring

```rust
/// Agent performance metrics
lazy_static! {
    static ref MODERATION_DECISIONS: CounterVec = register_counter_vec!(
        "moderation_decisions_total",
        "Total moderation decisions by action and agent",
        &["action", "agent_id"]
    ).unwrap();
    
    static ref DECISION_LATENCY: HistogramVec = register_histogram_vec!(
        "moderation_decision_latency_seconds",
        "Latency of moderation decisions",
        &["agent_id"],
        vec![0.01, 0.05, 0.1, 0.2, 0.5, 1.0]
    ).unwrap();
    
    static ref ESCALATION_RATE: Gauge = register_gauge!(
        "moderation_escalation_rate",
        "Percentage of decisions escalated to humans"
    ).unwrap();
}
```

---

## Judicial Review & Reinstatement System

### Philosophy: Courts Over Corporate Control

Banned users deserve a pathway back—not through corporate whim, but through a structured judicial process with independent review. This places decision-making power in a transparent system rather than absolute corporate control.

### Eligibility Requirements

| Ban Type | Minimum Wait | Severity Multiplier |
|----------|--------------|---------------------|
| Temporary | 7 days min | 1.0x |
| Extended | 2x ban length | 1.5x |
| Permanent | 365 days | 2.0x |
| IP Ban | 730 days | 2.5x |
| Hardware Ban | 1095 days | 3.0x |
| Termination | N/A | Cannot appeal |

**Severity multipliers** increase wait time based on original violation:
- Critical (child safety, terrorism): 3.0x
- Severe: 2.5x
- High: 2.0x
- Moderate: 1.5x

### Application Requirements

```rust
// Applicant must provide:
pub struct JudicialReviewRequest {
    /// Statement explaining rehabilitation
    pub rehabilitation_statement: String,
    
    /// Evidence of changed behavior
    pub evidence: Vec<RehabilitationEvidence>,
    
    /// Character references from users in good standing
    pub character_references: Vec<CharacterReference>,
    
    /// Conditions applicant agrees to if reinstated
    pub agreed_conditions: Vec<ReinstatementCondition>,
}

// Types of rehabilitation evidence
pub enum RehabilitationEvidenceType {
    CompletedEducation,      // Platform rules course
    CommunityService,        // Helping others
    ExternalPlatformRecord,  // Good behavior elsewhere
    ProfessionalCounseling,  // If applicable
    TimeBased,               // Clean waiting period
    VictimReconciliation,    // If applicable
    WrittenCommitment,       // Formal commitment
}
```

### Review Process

```
Application Submitted
        │
        ▼
┌───────────────────┐
│ Eligibility Check │
│ (time served,     │
│  no pending apps) │
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐     Severity >= 8     ┌───────────────────┐
│  Assign Judge     │─────────────────────▶│   Panel Review    │
│  (single review)  │                       │ (3+ judges vote)  │
└─────────┬─────────┘                       └─────────┬─────────┘
          │                                           │
          └─────────────────┬─────────────────────────┘
                            │
                            ▼
                  ┌───────────────────┐
                  │    Decision       │
                  │ (with reasoning)  │
                  └─────────┬─────────┘
                            │
          ┌─────────────────┴─────────────────┐
          │                                   │
          ▼                                   ▼
    ┌───────────┐                       ┌───────────┐
    │  Granted  │                       │  Denied   │
    │ (probation│                       │ (reapply  │
    │  period)  │                       │  in 180d) │
    └───────────┘                       └───────────┘
```

### Probationary Conditions

If reinstatement is granted, users enter a **probationary period** (typically 90 days) with:

```rust
pub enum ReinstatementConditionType {
    CompleteEducation,    // Must complete rules course
    CleanRecord,          // No violations during probation
    NoContactOrder,       // Cannot contact specific users
    ContentPreApproval,   // All content reviewed before publish
    LimitedFeatures,      // Restricted feature access
    RegularCheckIns,      // Bi-weekly check-ins with moderator
    NoGroupCreation,      // Cannot create/lead groups
    NoMonetization,       // Cannot earn from content
    TwoFactorRequired,    // Must enable 2FA
}
```

### Probation Violations

| Severity | Action |
|----------|--------|
| Minor | Warning issued |
| Moderate | Probation extended 30 days |
| Serious | Additional restrictions |
| Severe | Probation revoked, ban reinstated |

---

## Immediate Asset Actions

### Philosophy: Swift Action, Fair Warning

Bad assets require immediate action to protect the community, but users deserve clear warnings and a path to appeal.

### Action Flow

```
Asset Flagged (AI or Report)
        │
        ▼
┌───────────────────┐
│ Classify Violation│
│ (confidence score)│
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│ Take Immediate    │
│ Action on Asset   │
│ (hide/remove/etc) │
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│ Warn Asset Owner  │
│ (educational or   │
│  strike-based)    │
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│ Appeal Window     │
│ (7 days)          │
└───────────────────┘
```

### Asset Action Types

```rust
pub enum AssetActionType {
    Hide,                 // Soft action, pending review
    Remove,               // Removed from platform
    Quarantine,           // Isolated for investigation
    AgeRestrict,          // Limited to appropriate ages
    DisableMonetization,  // Cannot earn from asset
    RevokeDistribution,   // Cannot be shared/sold
    Ban,                  // Permanent removal
}
```

### Warning Escalation

| Warning Type | Strike Weight | Triggers |
|--------------|---------------|----------|
| First Offense Educational | 0 | First violation, educational |
| Repeat Offense Warning | 1 | 2nd-3rd violation |
| Serious Violation Strike | 2 | High severity |
| Critical Violation | 5 | Critical severity |
| Pattern Detected | 3 | Multiple violations detected |

### Appeal Availability

- **Automatic actions with <95% confidence**: Appeal available
- **Manual moderator actions**: Appeal available
- **High-confidence automatic (≥95%)**: Limited appeal
- **Critical violations**: Expedited review

---

## Related Documentation

- [MODERATION_API.md](./MODERATION_API.md) - API endpoints for moderation
- [COPPA.md](../legal/COPPA.md) - Child safety integration
- [GDPR.md](../legal/GDPR.md) - Art. 22 automated decision-making

---

**AI Safety Contact:** 
**Agent Issues:** 
