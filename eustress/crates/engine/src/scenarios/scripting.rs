//! # Eustress Scenarios — Rune Script Integration
//!
//! Table of Contents:
//! 1. ScenarioScriptEngine — Compile and execute Rune scripts for branch logic
//! 2. ScriptContext — Data passed into Rune scripts from the scenario
//! 3. ScriptResult — Output from a Rune script execution
//! 4. BranchScriptApi — Functions exposed to Rune for branch manipulation
//! 5. Claude conditioning — System prompt for Claude→Rune code generation
//!
//! This module serves two purposes:
//! - **Phase 0.5 task 206b**: Rune script overrides for advanced branch logic
//! - **Eustress Rune API extension**: Scenario/Circumstance functions exposed to all Rune scripts
//!
//! The Claude conditioning prompt (section 5) teaches Claude how to generate
//! Rune scripts that use the Scenario API, enabling the NL→Rune pipeline (task 206c).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{
    BranchLogic, BranchNode, BranchStatus, EvidencePolarity,
    Scenario, ScenarioScale,
};

// ─────────────────────────────────────────────
// 1. ScenarioScriptEngine — Compile and execute
// ─────────────────────────────────────────────

/// Errors from script compilation or execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScriptError {
    /// Rune compilation failed
    CompileError(String),
    /// Runtime execution error
    RuntimeError(String),
    /// Script returned invalid output
    InvalidOutput(String),
    /// Script timed out
    Timeout { limit_ms: u64 },
    /// Script referenced a branch that doesn't exist
    BranchNotFound(Uuid),
    /// Script tried to perform a disallowed operation
    PermissionDenied(String),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::CompileError(e) => write!(f, "Compile error: {}", e),
            ScriptError::RuntimeError(e) => write!(f, "Runtime error: {}", e),
            ScriptError::InvalidOutput(e) => write!(f, "Invalid output: {}", e),
            ScriptError::Timeout { limit_ms } => write!(f, "Script timed out after {}ms", limit_ms),
            ScriptError::BranchNotFound(id) => write!(f, "Branch not found: {}", id),
            ScriptError::PermissionDenied(e) => write!(f, "Permission denied: {}", e),
        }
    }
}

impl std::error::Error for ScriptError {}

/// Configuration for the scenario script engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptEngineConfig {
    /// Maximum execution time per script in milliseconds
    pub timeout_ms: u64,
    /// Whether scripts can create new branches
    pub allow_branch_creation: bool,
    /// Whether scripts can modify evidence links
    pub allow_evidence_modification: bool,
    /// Whether scripts can change branch probabilities directly
    pub allow_probability_override: bool,
    /// Maximum number of branches a script can create per execution
    pub max_branches_per_execution: usize,
    /// Whether to cache compiled scripts by hash
    pub cache_compiled: bool,
}

impl Default for ScriptEngineConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            allow_branch_creation: true,
            allow_evidence_modification: false,
            allow_probability_override: true,
            max_branches_per_execution: 50,
            cache_compiled: true,
        }
    }
}

/// The scenario script engine compiles and executes Rune scripts
/// that control branch logic in the hypothesis tree.
///
/// Scripts receive a `ScriptContext` with read access to the scenario
/// and return a `ScriptResult` with branch modifications.
pub struct ScenarioScriptEngine {
    /// Engine configuration
    pub config: ScriptEngineConfig,
    /// Cache of compiled script hashes → compiled units
    compiled_cache: HashMap<String, CompiledScript>,
}

/// A compiled Rune script ready for execution.
#[derive(Debug, Clone)]
struct CompiledScript {
    /// Source code hash for cache invalidation
    source_hash: String,
    /// The original source code
    source: String,
    /// When this was compiled
    compiled_at: DateTime<Utc>,
}

impl ScenarioScriptEngine {
    /// Create a new script engine with the given config.
    pub fn new(config: ScriptEngineConfig) -> Self {
        Self {
            config,
            compiled_cache: HashMap::new(),
        }
    }

    /// Compile a Rune script source into a cached unit.
    /// Returns the hash key for later execution.
    pub fn compile(&mut self, source: &str) -> Result<String, ScriptError> {
        let hash = Self::hash_source(source);

        // Check cache
        if self.compiled_cache.contains_key(&hash) {
            return Ok(hash);
        }

        // Validate basic structure
        Self::validate_source(source)?;

        // Store compiled unit
        let compiled = CompiledScript {
            source_hash: hash.clone(),
            source: source.to_string(),
            compiled_at: Utc::now(),
        };

        if self.config.cache_compiled {
            self.compiled_cache.insert(hash.clone(), compiled);
        }

        Ok(hash)
    }

    /// Execute a compiled script against a scenario context.
    /// Returns a ScriptResult with branch modifications to apply.
    pub fn execute(
        &self,
        source: &str,
        context: &ScriptContext,
    ) -> Result<ScriptResult, ScriptError> {
        // Validate source
        Self::validate_source(source)?;

        // Build the execution environment
        let mut result = ScriptResult::new();

        // Execute the script logic by interpreting the source
        // In production, this would use rune::Vm to execute the compiled unit.
        // For now, we provide a structured interpreter that handles common patterns.
        self.interpret(source, context, &mut result)?;

        // Validate result against permissions
        self.validate_result(&result)?;

        Ok(result)
    }

    /// Execute a script and apply its results to a scenario.
    pub fn execute_and_apply(
        &self,
        source: &str,
        scenario: &mut Scenario,
        branch_id: Uuid,
    ) -> Result<ScriptResult, ScriptError> {
        let context = ScriptContext::from_scenario(scenario, branch_id)?;
        let result = self.execute(source, &context)?;
        Self::apply_result(scenario, &result);
        Ok(result)
    }

    /// Apply a script result to a scenario.
    pub fn apply_result(scenario: &mut Scenario, result: &ScriptResult) {
        // Apply probability overrides
        for (&branch_id, &new_prob) in &result.probability_overrides {
            if let Some(branch) = scenario.branches.get_mut(&branch_id) {
                branch.posterior = new_prob.clamp(0.001, 0.999);
            }
        }

        // Apply status changes
        for (&branch_id, &new_status) in &result.status_changes {
            if let Some(branch) = scenario.branches.get_mut(&branch_id) {
                branch.status = new_status;
            }
        }

        // Create new branches
        for new_branch in &result.new_branches {
            if let Some(parent) = scenario.branches.get(&new_branch.parent_id) {
                let depth = parent.depth + 1;
                let child = BranchNode {
                    id: Uuid::new_v4(),
                    label: new_branch.label.clone(),
                    description: None,
                    parent_id: Some(new_branch.parent_id),
                    children: Vec::new(),
                    depth,
                    prior: new_branch.prior,
                    posterior: new_branch.prior,
                    mc_hits: 0,
                    mc_total: 0,
                    status: BranchStatus::Active,
                    logic: BranchLogic::Weighted,
                    evidence_ids: Vec::new(),
                    entity_ids: Vec::new(),
                    outcome: None,
                    created_at: Utc::now(),
                };
                let child_id = child.id;
                scenario.branches.insert(child_id, child);
                if let Some(parent) = scenario.branches.get_mut(&new_branch.parent_id) {
                    parent.children.push(child_id);
                }
            }
        }

        // Apply log messages (store in scenario metadata or emit as events)
        // For now, log messages are captured in the result for the caller to handle.

        scenario.updated_at = Utc::now();
    }

    /// Invalidate the compiled cache for a specific source hash.
    pub fn invalidate(&mut self, hash: &str) {
        self.compiled_cache.remove(hash);
    }

    /// Clear the entire compiled cache.
    pub fn clear_cache(&mut self) {
        self.compiled_cache.clear();
    }

    /// Hash source code for cache keying.
    fn hash_source(source: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Validate script source for basic structural correctness.
    fn validate_source(source: &str) -> Result<(), ScriptError> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err(ScriptError::CompileError("Empty script source".into()));
        }

        // Check for balanced braces
        let mut depth: i32 = 0;
        for ch in trimmed.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return Err(ScriptError::CompileError("Unbalanced braces".into()));
            }
        }
        if depth != 0 {
            return Err(ScriptError::CompileError(format!(
                "Unbalanced braces: {} unclosed",
                depth
            )));
        }

        Ok(())
    }

    /// Interpret a script source against a context.
    /// This is a simplified interpreter for common branch logic patterns.
    /// In production, this delegates to rune::Vm.
    fn interpret(
        &self,
        source: &str,
        context: &ScriptContext,
        result: &mut ScriptResult,
    ) -> Result<(), ScriptError> {
        // Parse directives from the script
        for line in source.lines() {
            let trimmed = line.trim();

            // Skip comments and empty lines
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            // Directive: set_probability(branch_label, value)
            if let Some(rest) = trimmed.strip_prefix("set_probability(") {
                if let Some(args) = rest.strip_suffix(')') {
                    self.parse_set_probability(args, context, result)?;
                }
                continue;
            }

            // Directive: collapse(branch_label)
            if let Some(rest) = trimmed.strip_prefix("collapse(") {
                if let Some(label) = rest.strip_suffix(')') {
                    let label = label.trim().trim_matches('"');
                    if let Some((&id, _)) = context.branches.iter().find(|(_, b)| b.label == label) {
                        result.status_changes.insert(id, BranchStatus::Collapsed);
                    }
                }
                continue;
            }

            // Directive: restore(branch_label)
            if let Some(rest) = trimmed.strip_prefix("restore(") {
                if let Some(label) = rest.strip_suffix(')') {
                    let label = label.trim().trim_matches('"');
                    if let Some((&id, _)) = context.branches.iter().find(|(_, b)| b.label == label) {
                        result.status_changes.insert(id, BranchStatus::Active);
                    }
                }
                continue;
            }

            // Directive: add_branch(parent_label, new_label, prior)
            if let Some(rest) = trimmed.strip_prefix("add_branch(") {
                if let Some(args) = rest.strip_suffix(')') {
                    self.parse_add_branch(args, context, result)?;
                }
                continue;
            }

            // Directive: log(message)
            if let Some(rest) = trimmed.strip_prefix("log(") {
                if let Some(msg) = rest.strip_suffix(')') {
                    result.log_messages.push(msg.trim().trim_matches('"').to_string());
                }
                continue;
            }

            // Directive: if evidence_count(branch_label) > N then ...
            // (Simplified conditional — full Rune VM handles complex logic)
            if trimmed.starts_with("if ") {
                // Skip conditionals in the simple interpreter
                result.log_messages.push(format!("Skipped conditional: {}", trimmed));
                continue;
            }
        }

        Ok(())
    }

    /// Parse set_probability(label, value) directive.
    fn parse_set_probability(
        &self,
        args: &str,
        context: &ScriptContext,
        result: &mut ScriptResult,
    ) -> Result<(), ScriptError> {
        let parts: Vec<&str> = args.splitn(2, ',').collect();
        if parts.len() != 2 {
            return Err(ScriptError::InvalidOutput(
                "set_probability requires (label, value)".into(),
            ));
        }

        let label = parts[0].trim().trim_matches('"');
        let value: f64 = parts[1]
            .trim()
            .parse()
            .map_err(|_| ScriptError::InvalidOutput("Invalid probability value".into()))?;

        if let Some((&id, _)) = context.branches.iter().find(|(_, b)| b.label == label) {
            if !self.config.allow_probability_override {
                return Err(ScriptError::PermissionDenied(
                    "Probability override not allowed".into(),
                ));
            }
            result.probability_overrides.insert(id, value.clamp(0.001, 0.999));
        }

        Ok(())
    }

    /// Parse add_branch(parent_label, new_label, prior) directive.
    fn parse_add_branch(
        &self,
        args: &str,
        context: &ScriptContext,
        result: &mut ScriptResult,
    ) -> Result<(), ScriptError> {
        let parts: Vec<&str> = args.splitn(3, ',').collect();
        if parts.len() != 3 {
            return Err(ScriptError::InvalidOutput(
                "add_branch requires (parent_label, new_label, prior)".into(),
            ));
        }

        let parent_label = parts[0].trim().trim_matches('"');
        let new_label = parts[1].trim().trim_matches('"');
        let prior: f64 = parts[2]
            .trim()
            .parse()
            .map_err(|_| ScriptError::InvalidOutput("Invalid prior value".into()))?;

        if !self.config.allow_branch_creation {
            return Err(ScriptError::PermissionDenied(
                "Branch creation not allowed".into(),
            ));
        }

        if result.new_branches.len() >= self.config.max_branches_per_execution {
            return Err(ScriptError::PermissionDenied(format!(
                "Max branches per execution ({}) exceeded",
                self.config.max_branches_per_execution
            )));
        }

        if let Some((&parent_id, _)) = context.branches.iter().find(|(_, b)| b.label == parent_label) {
            result.new_branches.push(NewBranch {
                parent_id,
                label: new_label.to_string(),
                prior: prior.clamp(0.001, 0.999),
            });
        }

        Ok(())
    }

    /// Validate a script result against engine permissions.
    fn validate_result(&self, result: &ScriptResult) -> Result<(), ScriptError> {
        if !self.config.allow_probability_override && !result.probability_overrides.is_empty() {
            return Err(ScriptError::PermissionDenied(
                "Probability overrides not allowed".into(),
            ));
        }

        if !self.config.allow_branch_creation && !result.new_branches.is_empty() {
            return Err(ScriptError::PermissionDenied(
                "Branch creation not allowed".into(),
            ));
        }

        if result.new_branches.len() > self.config.max_branches_per_execution {
            return Err(ScriptError::PermissionDenied(format!(
                "Too many new branches: {} > {}",
                result.new_branches.len(),
                self.config.max_branches_per_execution
            )));
        }

        Ok(())
    }
}

impl Default for ScenarioScriptEngine {
    fn default() -> Self {
        Self::new(ScriptEngineConfig::default())
    }
}

// ─────────────────────────────────────────────
// 2. ScriptContext — Read-only scenario snapshot
// ─────────────────────────────────────────────

/// Read-only snapshot of scenario data passed into a Rune script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptContext {
    /// Scenario ID
    pub scenario_id: Uuid,
    /// Scenario name
    pub scenario_name: String,
    /// Scenario scale
    pub scale: ScenarioScale,
    /// The branch this script is attached to
    pub current_branch_id: Uuid,
    /// All branches (read-only snapshot)
    pub branches: HashMap<Uuid, BranchSnapshot>,
    /// Evidence summary per branch
    pub evidence_summary: HashMap<Uuid, EvidenceSummary>,
    /// Scenario-level parameters as key-value pairs
    pub parameters: HashMap<String, String>,
    /// Current simulation iteration (if running inside a simulation)
    pub simulation_iteration: Option<u64>,
}

/// Lightweight branch snapshot for script context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchSnapshot {
    pub label: String,
    pub parent_id: Option<Uuid>,
    pub children: Vec<Uuid>,
    pub depth: u32,
    pub prior: f64,
    pub posterior: f64,
    pub status: BranchStatus,
    pub evidence_count: usize,
    pub entity_count: usize,
}

/// Summary of evidence attached to a branch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceSummary {
    /// Total evidence items linked to this branch
    pub total: usize,
    /// Supporting evidence count
    pub supporting: usize,
    /// Contradicting evidence count
    pub contradicting: usize,
    /// Neutral evidence count
    pub neutral: usize,
    /// Average confidence of linked evidence
    pub avg_confidence: f64,
    /// Average likelihood ratio of linked evidence
    pub avg_lr: f64,
}

impl ScriptContext {
    /// Build a script context from a scenario, focused on a specific branch.
    pub fn from_scenario(
        scenario: &Scenario,
        branch_id: Uuid,
    ) -> Result<Self, ScriptError> {
        if !scenario.branches.contains_key(&branch_id) {
            return Err(ScriptError::BranchNotFound(branch_id));
        }

        let branches: HashMap<Uuid, BranchSnapshot> = scenario
            .branches
            .iter()
            .map(|(&id, b)| {
                (
                    id,
                    BranchSnapshot {
                        label: b.label.clone(),
                        parent_id: b.parent_id,
                        children: b.children.clone(),
                        depth: b.depth,
                        prior: b.prior,
                        posterior: b.posterior,
                        status: b.status,
                        evidence_count: b.evidence_ids.len(),
                        entity_count: b.entity_ids.len(),
                    },
                )
            })
            .collect();

        // Build evidence summaries
        let mut evidence_summary: HashMap<Uuid, EvidenceSummary> = HashMap::new();
        for evidence in &scenario.evidence {
            for link in &evidence.links {
                let summary = evidence_summary
                    .entry(link.branch_id)
                    .or_default();
                summary.total += 1;
                match link.polarity {
                    EvidencePolarity::Supporting => summary.supporting += 1,
                    EvidencePolarity::Contradicting => summary.contradicting += 1,
                    EvidencePolarity::Neutral => summary.neutral += 1,
                }
                summary.avg_confidence += evidence.confidence;
                summary.avg_lr += evidence.likelihood_ratio;
            }
        }
        // Finalize averages
        for summary in evidence_summary.values_mut() {
            if summary.total > 0 {
                summary.avg_confidence /= summary.total as f64;
                summary.avg_lr /= summary.total as f64;
            }
        }

        let parameters: HashMap<String, String> = scenario
            .parameters
            .iter()
            .map(|p| (p.name.clone(), format!("{:?}", p.value)))
            .collect();

        Ok(Self {
            scenario_id: scenario.id,
            scenario_name: scenario.name.clone(),
            scale: scenario.scale,
            current_branch_id: branch_id,
            branches,
            evidence_summary,
            parameters,
            simulation_iteration: None,
        })
    }
}

// ─────────────────────────────────────────────
// 3. ScriptResult — Output from execution
// ─────────────────────────────────────────────

/// Output from a Rune script execution.
/// Contains branch modifications to apply to the scenario.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptResult {
    /// Branch probability overrides (branch_id → new posterior)
    pub probability_overrides: HashMap<Uuid, f64>,
    /// Branch status changes (branch_id → new status)
    pub status_changes: HashMap<Uuid, BranchStatus>,
    /// New branches to create
    pub new_branches: Vec<NewBranch>,
    /// Log messages from the script
    pub log_messages: Vec<String>,
    /// Whether the script completed successfully
    pub success: bool,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
}

impl ScriptResult {
    pub fn new() -> Self {
        Self {
            success: true,
            ..Default::default()
        }
    }
}

/// A new branch to be created by a script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBranch {
    /// Parent branch ID
    pub parent_id: Uuid,
    /// Label for the new branch
    pub label: String,
    /// Prior probability
    pub prior: f64,
}

// ─────────────────────────────────────────────
// 4. BranchScriptApi — Functions exposed to Rune
// ─────────────────────────────────────────────

/// Functions that Rune scripts can call to query and manipulate branches.
/// These are registered as Rune module functions when the full Rune VM is active.
///
/// In the simplified interpreter, these are handled as directive parsing.
/// When the full rune::Vm integration is complete, these become:
///
/// ```rune
/// // In a Rune script:
/// let branch = scenario::get_branch("Suspect A");
/// let evidence = scenario::evidence_for(branch);
///
/// if evidence.supporting > 3 && evidence.avg_confidence > 0.8 {
///     scenario::set_probability(branch, 0.85);
/// }
///
/// if scenario::get_probability("Suspect B") < 0.05 {
///     scenario::collapse("Suspect B");
/// }
///
/// scenario::add_branch("Root", "New Lead", 0.3);
/// scenario::log("Adjusted probabilities based on evidence count");
/// ```
pub struct BranchScriptApi;

impl BranchScriptApi {
    /// Get the list of function signatures exposed to Rune.
    /// Used to build the Rune module and the Claude conditioning prompt.
    pub fn api_signatures() -> Vec<ApiFunctionSignature> {
        vec![
            ApiFunctionSignature {
                name: "scenario::get_branch".into(),
                params: vec![("label".into(), "String".into())],
                returns: "Branch".into(),
                description: "Get a branch by its label".into(),
            },
            ApiFunctionSignature {
                name: "scenario::get_probability".into(),
                params: vec![("label".into(), "String".into())],
                returns: "f64".into(),
                description: "Get the posterior probability of a branch".into(),
            },
            ApiFunctionSignature {
                name: "scenario::set_probability".into(),
                params: vec![
                    ("label".into(), "String".into()),
                    ("value".into(), "f64".into()),
                ],
                returns: "()".into(),
                description: "Override the posterior probability of a branch (0.001 to 0.999)".into(),
            },
            ApiFunctionSignature {
                name: "scenario::evidence_for".into(),
                params: vec![("label".into(), "String".into())],
                returns: "EvidenceSummary".into(),
                description: "Get evidence summary for a branch (total, supporting, contradicting, avg_confidence, avg_lr)".into(),
            },
            ApiFunctionSignature {
                name: "scenario::collapse".into(),
                params: vec![("label".into(), "String".into())],
                returns: "()".into(),
                description: "Soft-collapse a branch (hide from visualization, never delete)".into(),
            },
            ApiFunctionSignature {
                name: "scenario::restore".into(),
                params: vec![("label".into(), "String".into())],
                returns: "()".into(),
                description: "Restore a collapsed branch back to active".into(),
            },
            ApiFunctionSignature {
                name: "scenario::add_branch".into(),
                params: vec![
                    ("parent_label".into(), "String".into()),
                    ("new_label".into(), "String".into()),
                    ("prior".into(), "f64".into()),
                ],
                returns: "()".into(),
                description: "Create a new child branch under the given parent".into(),
            },
            ApiFunctionSignature {
                name: "scenario::children".into(),
                params: vec![("label".into(), "String".into())],
                returns: "Vec<String>".into(),
                description: "Get labels of all child branches".into(),
            },
            ApiFunctionSignature {
                name: "scenario::param".into(),
                params: vec![("key".into(), "String".into())],
                returns: "String".into(),
                description: "Get a scenario parameter value by key".into(),
            },
            ApiFunctionSignature {
                name: "scenario::log".into(),
                params: vec![("message".into(), "String".into())],
                returns: "()".into(),
                description: "Log a message (visible in script output panel)".into(),
            },
        ]
    }
}

/// Signature of a function exposed to Rune scripts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFunctionSignature {
    /// Full function name (e.g., "scenario::get_branch")
    pub name: String,
    /// Parameter names and types
    pub params: Vec<(String, String)>,
    /// Return type
    pub returns: String,
    /// Human-readable description
    pub description: String,
}

// ─────────────────────────────────────────────
// 5. Claude conditioning — System prompt
// ─────────────────────────────────────────────

/// Generate the Claude system prompt section for Scenario Rune API.
/// This is appended to the main Soul system prompt when generating
/// scripts that interact with scenarios.
pub fn scenario_rune_conditioning_prompt() -> String {
    let mut prompt = String::from(
        r#"## Eustress Scenario Scripting API (Rune)

Scripts can control branch logic in the hypothesis tree. Each script is attached
to a branch and runs during simulation or on-demand.

### Available Functions

"#,
    );

    // Generate API reference from signatures
    for sig in BranchScriptApi::api_signatures() {
        let params: Vec<String> = sig.params.iter().map(|(n, t)| format!("{}: {}", n, t)).collect();
        prompt.push_str(&format!(
            "- `{}({})` → `{}` — {}\n",
            sig.name,
            params.join(", "),
            sig.returns,
            sig.description,
        ));
    }

    prompt.push_str(
        r#"
### Data Types

```rune
struct Branch {
    label: String,
    posterior: f64,
    prior: f64,
    status: String,  // "Active", "Collapsed", "Resolved"
    depth: i64,
    evidence_count: i64,
}

struct EvidenceSummary {
    total: i64,
    supporting: i64,
    contradicting: i64,
    neutral: i64,
    avg_confidence: f64,
    avg_lr: f64,
}
```

### Example Scripts

```rune
// Adjust probability based on evidence strength
pub fn main() {
    let ev = scenario::evidence_for("Suspect A");
    if ev.supporting > 3 && ev.avg_confidence > 0.8 {
        scenario::set_probability("Suspect A", 0.85);
        scenario::log("Strong evidence supports Suspect A");
    }

    // Auto-collapse weak hypotheses
    let prob_b = scenario::get_probability("Suspect B");
    if prob_b < 0.03 {
        scenario::collapse("Suspect B");
        scenario::log("Collapsed Suspect B due to low probability");
    }
}
```

```rune
// Generate sub-hypotheses from evidence patterns
pub fn main() {
    let ev = scenario::evidence_for("Financial Fraud");
    if ev.total > 5 && ev.supporting > ev.contradicting * 2 {
        scenario::add_branch("Financial Fraud", "Wire Transfer Scheme", 0.4);
        scenario::add_branch("Financial Fraud", "Invoice Manipulation", 0.35);
        scenario::add_branch("Financial Fraud", "Shell Company Network", 0.25);
        scenario::log("Generated fraud sub-hypotheses from evidence patterns");
    }
}
```

```rune
// Circumstance-aware: supply chain disruption response
pub fn main() {
    let risk = scenario::param("supplier_risk_score");
    if risk.parse::<f64>().unwrap_or(0.0) > 0.7 {
        scenario::set_probability("Supply Disruption", 0.8);
        scenario::add_branch("Supply Disruption", "Switch to Backup Supplier", 0.5);
        scenario::add_branch("Supply Disruption", "Increase Safety Stock", 0.3);
        scenario::add_branch("Supply Disruption", "Accept Delay", 0.2);
    }
}
```

### Rules
1. Scripts MUST have a `pub fn main()` entry point
2. All probability values must be between 0.001 and 0.999
3. Scripts cannot delete branches — only collapse (soft-prune)
4. Use `scenario::log()` for debugging and audit trail
5. Keep scripts focused — one logical decision per script
6. Scripts run in a sandboxed environment with no filesystem or network access
"#,
    );

    prompt
}

/// Generate the full Claude conditioning prompt that combines
/// the base Soul prompt with the Scenario API extension.
pub fn full_scenario_conditioning_prompt(base_soul_prompt: &str) -> String {
    format!(
        "{}\n\n{}\n",
        base_soul_prompt,
        scenario_rune_conditioning_prompt()
    )
}

// ─────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::types::ScenarioScale;

    fn make_scenario() -> Scenario {
        let mut s = Scenario::new("Script Test", ScenarioScale::Micro);
        let root = s.set_root_branch("Root", 1.0);
        s.add_branch(root, "Suspect A", 0.6);
        s.add_branch(root, "Suspect B", 0.3);
        s.add_branch(root, "Unknown", 0.1);
        s
    }

    #[test]
    fn test_compile_and_cache() {
        let mut engine = ScenarioScriptEngine::default();
        let source = r#"
            set_probability("Suspect A", 0.8)
            log("Adjusted A")
        "#;

        let hash1 = engine.compile(source).unwrap();
        let hash2 = engine.compile(source).unwrap();
        assert_eq!(hash1, hash2); // Cache hit
    }

    #[test]
    fn test_set_probability() {
        let engine = ScenarioScriptEngine::default();
        let mut scenario = make_scenario();
        let branch_a_id = scenario.branches.values()
            .find(|b| b.label == "Suspect A")
            .unwrap().id;

        let source = r#"set_probability("Suspect A", 0.85)"#;
        let context = ScriptContext::from_scenario(&scenario, branch_a_id).unwrap();
        let result = engine.execute(source, &context).unwrap();

        assert_eq!(*result.probability_overrides.values().next().unwrap(), 0.85);

        // Apply
        ScenarioScriptEngine::apply_result(&mut scenario, &result);
        let branch_a = scenario.branches.values()
            .find(|b| b.label == "Suspect A")
            .unwrap();
        assert!((branch_a.posterior - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_collapse_directive() {
        let engine = ScenarioScriptEngine::default();
        let mut scenario = make_scenario();
        let root_id = scenario.root_branch_id.unwrap();

        let source = r#"collapse("Unknown")"#;
        let context = ScriptContext::from_scenario(&scenario, root_id).unwrap();
        let result = engine.execute(source, &context).unwrap();

        ScenarioScriptEngine::apply_result(&mut scenario, &result);
        let unknown = scenario.branches.values()
            .find(|b| b.label == "Unknown")
            .unwrap();
        assert_eq!(unknown.status, BranchStatus::Collapsed);
    }

    #[test]
    fn test_add_branch_directive() {
        let engine = ScenarioScriptEngine::default();
        let mut scenario = make_scenario();
        let root_id = scenario.root_branch_id.unwrap();
        let initial_count = scenario.branch_count();

        let source = r#"add_branch("Suspect A", "Sub-hypothesis X", 0.4)"#;
        let context = ScriptContext::from_scenario(&scenario, root_id).unwrap();
        let result = engine.execute(source, &context).unwrap();

        assert_eq!(result.new_branches.len(), 1);
        assert_eq!(result.new_branches[0].label, "Sub-hypothesis X");

        ScenarioScriptEngine::apply_result(&mut scenario, &result);
        assert_eq!(scenario.branch_count(), initial_count + 1);
    }

    #[test]
    fn test_permission_denied() {
        let config = ScriptEngineConfig {
            allow_branch_creation: false,
            ..Default::default()
        };
        let engine = ScenarioScriptEngine::new(config);
        let scenario = make_scenario();
        let root_id = scenario.root_branch_id.unwrap();

        let source = r#"add_branch("Root", "Forbidden Branch", 0.5)"#;
        let context = ScriptContext::from_scenario(&scenario, root_id).unwrap();
        let result = engine.execute(source, &context);

        assert!(result.is_err());
        match result.unwrap_err() {
            ScriptError::PermissionDenied(_) => {}
            other => panic!("Expected PermissionDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_and_apply() {
        let engine = ScenarioScriptEngine::default();
        let mut scenario = make_scenario();
        let root_id = scenario.root_branch_id.unwrap();

        let source = r#"
            set_probability("Suspect A", 0.9)
            collapse("Unknown")
            log("Script executed")
        "#;

        let result = engine.execute_and_apply(source, &mut scenario, root_id).unwrap();
        assert!(result.success);
        assert_eq!(result.log_messages.len(), 1);

        let a = scenario.branches.values().find(|b| b.label == "Suspect A").unwrap();
        assert!((a.posterior - 0.9).abs() < 0.001);

        let u = scenario.branches.values().find(|b| b.label == "Unknown").unwrap();
        assert_eq!(u.status, BranchStatus::Collapsed);
    }

    #[test]
    fn test_invalid_source() {
        let engine = ScenarioScriptEngine::default();
        let scenario = make_scenario();
        let root_id = scenario.root_branch_id.unwrap();

        // Unbalanced braces
        let source = "set_probability(\"A\", 0.5) {";
        let context = ScriptContext::from_scenario(&scenario, root_id).unwrap();
        let result = engine.execute(source, &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_conditioning_prompt() {
        let prompt = scenario_rune_conditioning_prompt();
        assert!(prompt.contains("scenario::get_branch"));
        assert!(prompt.contains("scenario::set_probability"));
        assert!(prompt.contains("scenario::collapse"));
        assert!(prompt.contains("scenario::add_branch"));
        assert!(prompt.contains("pub fn main()"));
    }

    #[test]
    fn test_api_signatures() {
        let sigs = BranchScriptApi::api_signatures();
        assert!(sigs.len() >= 8);

        // Verify key functions exist
        let names: Vec<&str> = sigs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"scenario::get_branch"));
        assert!(names.contains(&"scenario::set_probability"));
        assert!(names.contains(&"scenario::collapse"));
        assert!(names.contains(&"scenario::add_branch"));
        assert!(names.contains(&"scenario::log"));
    }
}
