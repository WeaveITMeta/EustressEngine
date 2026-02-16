//! # Eustress Scenarios — Core Data Structures
//!
//! Table of Contents:
//! 1. GeoPoint — WGS84 geographic coordinate
//! 2. DataSourceRef — Reference to a data source (file, API, live feed)
//! 3. ScenarioParameter / ParameterValue — Input parameters driving the scenario
//! 4. ScenarioEntity / EntityRole — People, places, vehicles, items in the scenario
//! 5. Evidence / EvidenceType / EvidenceLink / AttachmentMode — Evidence items and branch attachments
//! 6. BranchNode / BranchStatus / BranchLogic — Hypothesis branches in the decision tree
//! 7. Outcome / OutcomeData — Terminal outcomes of branch paths
//! 8. Scenario / ScenarioScale / ScenarioStatus — Top-level scenario container

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─────────────────────────────────────────────
// 1. GeoPoint — WGS84 geographic coordinate
// ─────────────────────────────────────────────

/// A WGS84 geographic coordinate with optional altitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    /// Latitude in decimal degrees (-90.0 to 90.0)
    pub lat: f64,
    /// Longitude in decimal degrees (-180.0 to 180.0)
    pub lon: f64,
    /// Altitude in meters above sea level (optional)
    pub alt: Option<f64>,
}

impl GeoPoint {
    /// Create a new GeoPoint from latitude and longitude.
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon, alt: None }
    }

    /// Create a new GeoPoint with altitude.
    pub fn with_alt(lat: f64, lon: f64, alt: f64) -> Self {
        Self { lat, lon, alt: Some(alt) }
    }

    /// Haversine distance to another point in meters.
    pub fn distance_to(&self, other: &GeoPoint) -> f64 {
        const R: f64 = 6_371_000.0; // Earth radius in meters
        let d_lat = (other.lat - self.lat).to_radians();
        let d_lon = (other.lon - self.lon).to_radians();
        let lat1 = self.lat.to_radians();
        let lat2 = other.lat.to_radians();
        let a = (d_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();
        R * c
    }
}

// ─────────────────────────────────────────────
// 2. DataSourceRef — Reference to a data source
// ─────────────────────────────────────────────

/// Reference to the origin of a piece of data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataSourceRef {
    /// Local file (JSON, CSV, RON)
    LocalFile {
        path: String,
        format: FileFormat,
    },
    /// REST API endpoint
    RestApi {
        url: String,
        method: String,
        headers: HashMap<String, String>,
    },
    /// Live feed via Eustress Parameters
    LiveFeed {
        parameter_name: String,
        stream_id: String,
    },
    /// Manual entry by an analyst
    ManualEntry {
        analyst_id: Uuid,
        timestamp: DateTime<Utc>,
    },
    /// Imported from external system
    ExternalImport {
        system_name: String,
        import_id: String,
    },
}

/// Supported local file formats for data ingestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileFormat {
    Json,
    Csv,
    Ron,
}

// ─────────────────────────────────────────────
// 3. ScenarioParameter / ParameterValue
// ─────────────────────────────────────────────

/// An input parameter that drives scenario behavior.
/// Parameters are the knobs analysts turn — location, time window,
/// entity attributes, thresholds, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioParameter {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable name (e.g., "Crime Scene Location")
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Current value
    pub value: ParameterValue,
    /// Where this parameter's data came from
    pub source: Option<DataSourceRef>,
    /// When this parameter was last updated
    pub updated_at: DateTime<Utc>,
    /// Whether this parameter auto-updates from a live feed
    pub live: bool,
}

/// Typed parameter values supporting the full range of scenario inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParameterValue {
    /// Scalar float (e.g., temperature, distance, probability)
    Float(f64),
    /// Integer (e.g., count, age)
    Integer(i64),
    /// Boolean flag
    Bool(bool),
    /// Text string (e.g., name, description, SKU)
    Text(String),
    /// Geographic point
    Location(GeoPoint),
    /// Geographic region (bounding box: southwest, northeast)
    Region { sw: GeoPoint, ne: GeoPoint },
    /// Time instant
    Timestamp(DateTime<Utc>),
    /// Time range
    TimeRange {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
    /// Duration in seconds
    Duration(f64),
    /// Probability (0.0 to 1.0)
    Probability(f64),
    /// List of values (heterogeneous)
    List(Vec<ParameterValue>),
    /// Key-value map
    Map(HashMap<String, ParameterValue>),
    /// Reference to an entity in the scenario
    EntityRef(Uuid),
    /// Reference to evidence
    EvidenceRef(Uuid),
}

impl ScenarioParameter {
    /// Create a new parameter with a name and value.
    pub fn new(name: impl Into<String>, value: ParameterValue) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            value,
            source: None,
            updated_at: Utc::now(),
            live: false,
        }
    }
}

// ─────────────────────────────────────────────
// 4. ScenarioEntity / EntityRole
// ─────────────────────────────────────────────

/// An entity involved in the scenario — a person, place, vehicle,
/// organization, item, or abstract concept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioEntity {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable name
    pub name: String,
    /// Role/type of this entity
    pub role: EntityRole,
    /// Known attributes (flexible key-value)
    pub attributes: HashMap<String, ParameterValue>,
    /// Known locations over time
    pub known_locations: Vec<(DateTime<Utc>, GeoPoint)>,
    /// Relationships to other entities
    pub relationships: Vec<EntityRelationship>,
    /// When this entity was added to the scenario
    pub created_at: DateTime<Utc>,
    /// Data source that introduced this entity
    pub source: Option<DataSourceRef>,
}

/// The role/type of an entity in the scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityRole {
    /// Person of interest (suspect, witness, victim, etc.)
    Person,
    /// Physical location (crime scene, store, residence)
    Location,
    /// Vehicle (car, boat, aircraft)
    Vehicle,
    /// Organization (company, agency, gang)
    Organization,
    /// Physical item (weapon, tool, product, evidence item)
    Item,
    /// Digital artifact (phone, computer, account, IP address)
    Digital,
    /// Event (meeting, transaction, communication)
    Event,
    /// Abstract concept (motive, opportunity, means)
    Concept,
}

/// A relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelationship {
    /// The other entity in this relationship
    pub target_id: Uuid,
    /// Type of relationship
    pub relationship_type: String,
    /// Strength/confidence of this relationship (0.0 to 1.0)
    pub confidence: f64,
    /// Evidence supporting this relationship
    pub evidence_ids: Vec<Uuid>,
    /// Time range when this relationship was active (if known)
    pub active_period: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

impl ScenarioEntity {
    /// Create a new entity with a name and role.
    pub fn new(name: impl Into<String>, role: EntityRole) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            role,
            attributes: HashMap::new(),
            known_locations: Vec::new(),
            relationships: Vec::new(),
            created_at: Utc::now(),
            source: None,
        }
    }

    /// Add a key-value attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: ParameterValue) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    /// Add a known location at a specific time.
    pub fn with_location(mut self, time: DateTime<Utc>, point: GeoPoint) -> Self {
        self.known_locations.push((time, point));
        self
    }
}

// ─────────────────────────────────────────────
// 5. Evidence / EvidenceType / EvidenceLink / AttachmentMode
// ─────────────────────────────────────────────

/// A piece of evidence that informs hypothesis probabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable label
    pub label: String,
    /// Type classification
    pub evidence_type: EvidenceType,
    /// Confidence in this evidence's reliability (0.0 to 1.0)
    pub confidence: f64,
    /// Likelihood ratio: P(evidence | hypothesis_true) / P(evidence | hypothesis_false)
    /// Values > 1.0 support the hypothesis, < 1.0 weaken it
    pub likelihood_ratio: f64,
    /// When this evidence was collected/observed
    pub timestamp: Option<DateTime<Utc>>,
    /// Where this evidence was collected/observed
    pub location: Option<GeoPoint>,
    /// Entities referenced by this evidence
    pub entity_refs: Vec<Uuid>,
    /// Source of this evidence
    pub source: DataSourceRef,
    /// When this evidence was added to the scenario
    pub added_at: DateTime<Utc>,
    /// Branches this evidence is attached to
    pub links: Vec<EvidenceLink>,
    /// Free-form notes
    pub notes: Option<String>,
}

/// Classification of evidence types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EvidenceType {
    /// Physical evidence (DNA, fingerprints, fibers, weapons)
    Physical,
    /// Digital evidence (logs, metadata, communications)
    Digital,
    /// Testimonial (witness statement, confession, interview)
    Testimonial,
    /// Documentary (records, receipts, contracts, reports)
    Documentary,
    /// Forensic analysis result (lab report, autopsy, ballistics)
    Forensic,
    /// Surveillance (CCTV, audio recording, GPS tracking)
    Surveillance,
    /// Circumstantial (behavioral patterns, motive indicators)
    Circumstantial,
    /// Statistical (population data, frequency analysis)
    Statistical,
    /// Transaction (purchase, financial transfer, trade)
    Transaction,
    /// Custom type
    Custom(String),
}

/// A link between evidence and a specific branch, with metadata
/// about how and why it was attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceLink {
    /// The branch this evidence is linked to
    pub branch_id: Uuid,
    /// How this link was created
    pub mode: AttachmentMode,
    /// Whether this evidence supports or contradicts the branch hypothesis
    pub polarity: EvidencePolarity,
    /// Weight of this evidence for this specific branch (0.0 to 1.0)
    pub weight: f64,
    /// When this link was created
    pub linked_at: DateTime<Utc>,
    /// Who/what created this link
    pub linked_by: Option<Uuid>,
}

/// How an evidence-to-branch link was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentMode {
    /// Manually attached by an analyst
    Manual,
    /// Automatically inferred by embedding similarity
    Automatic,
    /// Suggested by the system, confirmed by analyst
    SuggestedConfirmed,
    /// Suggested by the system, not yet reviewed
    SuggestedPending,
}

/// Whether evidence supports or contradicts a hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidencePolarity {
    /// Evidence supports this hypothesis
    Supporting,
    /// Evidence contradicts this hypothesis
    Contradicting,
    /// Evidence is neutral / ambiguous for this hypothesis
    Neutral,
}

impl Evidence {
    /// Create a new evidence item.
    pub fn new(
        label: impl Into<String>,
        evidence_type: EvidenceType,
        confidence: f64,
        likelihood_ratio: f64,
        source: DataSourceRef,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            evidence_type,
            confidence: confidence.clamp(0.0, 1.0),
            likelihood_ratio,
            timestamp: None,
            location: None,
            entity_refs: Vec::new(),
            source,
            added_at: Utc::now(),
            links: Vec::new(),
            notes: None,
        }
    }

    /// Attach this evidence to a branch.
    pub fn attach_to_branch(
        &mut self,
        branch_id: Uuid,
        mode: AttachmentMode,
        polarity: EvidencePolarity,
        weight: f64,
    ) {
        self.links.push(EvidenceLink {
            branch_id,
            mode,
            polarity,
            weight: weight.clamp(0.0, 1.0),
            linked_at: Utc::now(),
            linked_by: None,
        });
    }

    /// Compute the effective likelihood ratio for a specific branch,
    /// accounting for confidence, weight, and polarity.
    pub fn effective_lr_for_branch(&self, branch_id: Uuid) -> Option<f64> {
        self.links
            .iter()
            .find(|l| l.branch_id == branch_id)
            .map(|link| {
                let base_lr = match link.polarity {
                    EvidencePolarity::Supporting => self.likelihood_ratio,
                    EvidencePolarity::Contradicting => 1.0 / self.likelihood_ratio,
                    EvidencePolarity::Neutral => 1.0,
                };
                // Scale by confidence and weight
                // LR of 1.0 = no effect, so we interpolate toward 1.0
                let effective = 1.0 + (base_lr - 1.0) * self.confidence * link.weight;
                effective.max(0.001) // Prevent division by zero downstream
            })
    }
}

// ─────────────────────────────────────────────
// 6. BranchNode / BranchStatus / BranchLogic
// ─────────────────────────────────────────────

/// A node in the scenario's hypothesis tree.
/// Each branch represents a possible explanation or outcome path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchNode {
    /// Unique identifier
    pub id: Uuid,
    /// Parent branch (None for root)
    pub parent_id: Option<Uuid>,
    /// Child branches
    pub children: Vec<Uuid>,
    /// Human-readable hypothesis label
    pub label: String,
    /// Detailed description of this hypothesis
    pub description: Option<String>,
    /// Prior probability before evidence (0.0 to 1.0)
    pub prior: f64,
    /// Posterior probability after Bayesian updates (0.0 to 1.0)
    pub posterior: f64,
    /// Monte Carlo sample count that landed on this branch
    pub mc_hits: u64,
    /// Total Monte Carlo samples run
    pub mc_total: u64,
    /// Current status
    pub status: BranchStatus,
    /// Logic that determines branching behavior
    pub logic: BranchLogic,
    /// Evidence IDs attached to this branch
    pub evidence_ids: Vec<Uuid>,
    /// Entity IDs relevant to this branch
    pub entity_ids: Vec<Uuid>,
    /// Outcome data if this is a terminal branch
    pub outcome: Option<OutcomeData>,
    /// When this branch was created
    pub created_at: DateTime<Utc>,
    /// Depth in the tree (root = 0)
    pub depth: u32,
}

/// Status of a branch in the hypothesis tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchStatus {
    /// Actively being explored
    Active,
    /// Visually collapsed (soft-pruned) due to low probability
    Collapsed,
    /// Marked as resolved / concluded
    Resolved,
    /// Manually frozen by analyst (no further updates)
    Frozen,
}

/// Logic that controls how a branch evaluates and spawns children.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BranchLogic {
    /// Simple probability-weighted branch (default)
    Weighted,
    /// Conditional on a parameter value
    Conditional {
        parameter_id: Uuid,
        condition: String,
    },
    /// Rune script that computes probability and child branches
    Script {
        source: String,
        compiled_hash: Option<String>,
    },
    /// AI-generated from natural language description
    AiGenerated {
        prompt: String,
        generated_script: Option<String>,
    },
}

impl BranchNode {
    /// Create a new root branch.
    pub fn root(label: impl Into<String>, prior: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            parent_id: None,
            children: Vec::new(),
            label: label.into(),
            description: None,
            prior: prior.clamp(0.0, 1.0),
            posterior: prior.clamp(0.0, 1.0),
            mc_hits: 0,
            mc_total: 0,
            status: BranchStatus::Active,
            logic: BranchLogic::Weighted,
            evidence_ids: Vec::new(),
            entity_ids: Vec::new(),
            outcome: None,
            created_at: Utc::now(),
            depth: 0,
        }
    }

    /// Create a child branch under a parent.
    pub fn child(label: impl Into<String>, parent_id: Uuid, prior: f64, depth: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            parent_id: Some(parent_id),
            children: Vec::new(),
            label: label.into(),
            description: None,
            prior: prior.clamp(0.0, 1.0),
            posterior: prior.clamp(0.0, 1.0),
            mc_hits: 0,
            mc_total: 0,
            status: BranchStatus::Active,
            logic: BranchLogic::Weighted,
            evidence_ids: Vec::new(),
            entity_ids: Vec::new(),
            outcome: None,
            created_at: Utc::now(),
            depth,
        }
    }

    /// Whether this branch is a leaf (no children).
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Whether this branch is the root (no parent).
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    /// Monte Carlo probability estimate (hits / total).
    pub fn mc_probability(&self) -> f64 {
        if self.mc_total == 0 {
            self.prior
        } else {
            self.mc_hits as f64 / self.mc_total as f64
        }
    }

    /// Whether this branch should be soft-pruned at the given threshold.
    pub fn should_collapse(&self, threshold: f64) -> bool {
        self.posterior < threshold && self.status == BranchStatus::Active
    }
}

// ─────────────────────────────────────────────
// 7. Outcome / OutcomeData
// ─────────────────────────────────────────────

/// Terminal outcome of a branch path — what happens if this
/// hypothesis is correct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeData {
    /// Human-readable outcome description
    pub description: String,
    /// Severity or impact level (domain-specific)
    pub severity: OutcomeSeverity,
    /// Recommended actions
    pub recommended_actions: Vec<String>,
    /// Confidence in this outcome given the branch is correct
    pub confidence: f64,
    /// Key-value metadata
    pub metadata: HashMap<String, ParameterValue>,
}

/// Severity classification for outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutcomeSeverity {
    /// Low impact / routine
    Low,
    /// Moderate impact / notable
    Medium,
    /// High impact / significant
    High,
    /// Critical impact / urgent
    Critical,
}

/// A named outcome that aggregates across multiple terminal branches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    /// Unique identifier
    pub id: Uuid,
    /// Outcome label
    pub label: String,
    /// Aggregate probability across all branches leading to this outcome
    pub aggregate_probability: f64,
    /// Branch IDs that lead to this outcome
    pub branch_ids: Vec<Uuid>,
    /// Outcome details
    pub data: OutcomeData,
}

// ─────────────────────────────────────────────
// 8. Scenario / ScenarioScale / ScenarioStatus
// ─────────────────────────────────────────────

/// Top-level scenario container — the root object that holds
/// the entire hypothesis tree, evidence pool, entities, and parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable scenario name
    pub name: String,
    /// Detailed description
    pub description: Option<String>,
    /// Scale: micro (tactical) or macro (strategic)
    pub scale: ScenarioScale,
    /// Current status
    pub status: ScenarioStatus,

    // === Core Data ===
    /// Input parameters
    pub parameters: Vec<ScenarioParameter>,
    /// Entities involved
    pub entities: Vec<ScenarioEntity>,
    /// Evidence pool
    pub evidence: Vec<Evidence>,
    /// Hypothesis tree (branches indexed by ID)
    pub branches: HashMap<Uuid, BranchNode>,
    /// Root branch ID
    pub root_branch_id: Option<Uuid>,
    /// Named outcomes
    pub outcomes: Vec<Outcome>,

    // === Configuration ===
    /// Soft-prune threshold (branches below this posterior are visually collapsed)
    pub collapse_threshold: f64,
    /// Sub-scenario IDs (for macro scenarios containing micros)
    pub sub_scenario_ids: Vec<Uuid>,
    /// Parent scenario ID (for micro scenarios inside a macro)
    pub parent_scenario_id: Option<Uuid>,

    // === Metadata ===
    /// When this scenario was created
    pub created_at: DateTime<Utc>,
    /// When this scenario was last modified
    pub updated_at: DateTime<Utc>,
    /// Creator analyst ID
    pub created_by: Option<Uuid>,
    /// Tags for organization
    pub tags: Vec<String>,
}

/// Scale of a scenario — determines composability behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioScale {
    /// Tactical, single-event scenario (e.g., "Who bought this item?")
    Micro,
    /// Strategic, trend-level scenario (e.g., "Serial offender pattern analysis")
    Macro,
}

/// Lifecycle status of a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioStatus {
    /// Initial setup, parameters being defined
    Initializing,
    /// Actively being analyzed
    Active,
    /// Simulation running
    Simulating,
    /// Analysis paused
    Paused,
    /// Concluded with outcome
    Concluded,
    /// Archived (read-only)
    Archived,
}

impl Scenario {
    /// Create a new empty scenario.
    pub fn new(name: impl Into<String>, scale: ScenarioScale) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            scale,
            status: ScenarioStatus::Initializing,
            parameters: Vec::new(),
            entities: Vec::new(),
            evidence: Vec::new(),
            branches: HashMap::new(),
            root_branch_id: None,
            outcomes: Vec::new(),
            collapse_threshold: 0.05, // Default 5% soft-prune threshold
            sub_scenario_ids: Vec::new(),
            parent_scenario_id: None,
            created_at: now,
            updated_at: now,
            created_by: None,
            tags: Vec::new(),
        }
    }

    /// Add a root hypothesis branch to this scenario.
    pub fn set_root_branch(&mut self, label: impl Into<String>, prior: f64) -> Uuid {
        let branch = BranchNode::root(label, prior);
        let id = branch.id;
        self.root_branch_id = Some(id);
        self.branches.insert(id, branch);
        self.updated_at = Utc::now();
        id
    }

    /// Add a child branch under a parent.
    /// Returns the new branch ID, or None if parent not found.
    pub fn add_branch(
        &mut self,
        parent_id: Uuid,
        label: impl Into<String>,
        prior: f64,
    ) -> Option<Uuid> {
        let parent_depth = self.branches.get(&parent_id)?.depth;
        let child = BranchNode::child(label, parent_id, prior, parent_depth + 1);
        let child_id = child.id;
        self.branches.insert(child_id, child);
        // Register child in parent
        if let Some(parent) = self.branches.get_mut(&parent_id) {
            parent.children.push(child_id);
        }
        self.updated_at = Utc::now();
        Some(child_id)
    }

    /// Add an entity to this scenario.
    pub fn add_entity(&mut self, entity: ScenarioEntity) -> Uuid {
        let id = entity.id;
        self.entities.push(entity);
        self.updated_at = Utc::now();
        id
    }

    /// Add evidence to this scenario's pool.
    pub fn add_evidence(&mut self, evidence: Evidence) -> Uuid {
        let id = evidence.id;
        self.evidence.push(evidence);
        self.updated_at = Utc::now();
        id
    }

    /// Add a parameter to this scenario.
    pub fn add_parameter(&mut self, param: ScenarioParameter) -> Uuid {
        let id = param.id;
        self.parameters.push(param);
        self.updated_at = Utc::now();
        id
    }

    /// Get a branch by ID.
    pub fn branch(&self, id: Uuid) -> Option<&BranchNode> {
        self.branches.get(&id)
    }

    /// Get a mutable branch by ID.
    pub fn branch_mut(&mut self, id: Uuid) -> Option<&mut BranchNode> {
        self.branches.get_mut(&id)
    }

    /// Get evidence by ID.
    pub fn evidence_by_id(&self, id: Uuid) -> Option<&Evidence> {
        self.evidence.iter().find(|e| e.id == id)
    }

    /// Get entity by ID.
    pub fn entity_by_id(&self, id: Uuid) -> Option<&ScenarioEntity> {
        self.entities.iter().find(|e| e.id == id)
    }

    /// Count total branches in the tree.
    pub fn branch_count(&self) -> usize {
        self.branches.len()
    }

    /// Count active (non-collapsed) branches.
    pub fn active_branch_count(&self) -> usize {
        self.branches.values()
            .filter(|b| b.status == BranchStatus::Active)
            .count()
    }

    /// Apply soft-pruning: collapse branches below the threshold.
    pub fn apply_soft_pruning(&mut self) {
        let threshold = self.collapse_threshold;
        for branch in self.branches.values_mut() {
            if branch.should_collapse(threshold) {
                branch.status = BranchStatus::Collapsed;
            }
        }
        self.updated_at = Utc::now();
    }

    /// Perform a single Bayesian update on a branch using a piece of evidence.
    /// Updates the branch's posterior probability.
    pub fn bayesian_update(&mut self, branch_id: Uuid, evidence_id: Uuid) -> Option<f64> {
        let lr = self.evidence.iter()
            .find(|e| e.id == evidence_id)?
            .effective_lr_for_branch(branch_id)?;

        let branch = self.branches.get_mut(&branch_id)?;
        let prior = branch.posterior;

        // Bayes' theorem: P(H|E) = P(E|H) * P(H) / P(E)
        // Using odds form: posterior_odds = LR * prior_odds
        let prior_odds = prior / (1.0 - prior).max(1e-10);
        let posterior_odds = lr * prior_odds;
        let new_posterior = posterior_odds / (1.0 + posterior_odds);

        branch.posterior = new_posterior.clamp(0.001, 0.999);
        self.updated_at = Utc::now();
        Some(branch.posterior)
    }

    /// Normalize sibling branch posteriors so they sum to 1.0
    /// within each parent group.
    pub fn normalize_siblings(&mut self) {
        // Collect parent → children mapping
        let parent_children: HashMap<Option<Uuid>, Vec<Uuid>> = {
            let mut map: HashMap<Option<Uuid>, Vec<Uuid>> = HashMap::new();
            for branch in self.branches.values() {
                map.entry(branch.parent_id).or_default().push(branch.id);
            }
            map
        };

        // Normalize each sibling group
        for (_parent, children) in &parent_children {
            let sum: f64 = children.iter()
                .filter_map(|id| self.branches.get(id))
                .filter(|b| b.status == BranchStatus::Active)
                .map(|b| b.posterior)
                .sum();

            if sum > 0.0 && (sum - 1.0).abs() > 1e-6 {
                for id in children {
                    if let Some(branch) = self.branches.get_mut(id) {
                        if branch.status == BranchStatus::Active {
                            branch.posterior /= sum;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geopoint_distance() {
        // New York to Los Angeles ≈ 3,944 km
        let nyc = GeoPoint::new(40.7128, -74.0060);
        let lax = GeoPoint::new(34.0522, -118.2437);
        let dist = nyc.distance_to(&lax);
        assert!((dist - 3_944_000.0).abs() < 50_000.0, "Distance was {dist}");
    }

    #[test]
    fn test_scenario_branch_tree() {
        let mut scenario = Scenario::new("Test Case", ScenarioScale::Micro);
        let root = scenario.set_root_branch("Root Hypothesis", 1.0);
        let child_a = scenario.add_branch(root, "Hypothesis A", 0.6).unwrap();
        let child_b = scenario.add_branch(root, "Hypothesis B", 0.4).unwrap();

        assert_eq!(scenario.branch_count(), 3);
        assert!(scenario.branch(root).unwrap().children.contains(&child_a));
        assert!(scenario.branch(root).unwrap().children.contains(&child_b));
        assert_eq!(scenario.branch(child_a).unwrap().depth, 1);
    }

    #[test]
    fn test_bayesian_update() {
        let mut scenario = Scenario::new("Bayes Test", ScenarioScale::Micro);
        let root = scenario.set_root_branch("Root", 1.0);
        let branch = scenario.add_branch(root, "Suspect A", 0.5).unwrap();

        // Add strong supporting evidence (LR = 10)
        let mut evidence = Evidence::new(
            "DNA Match",
            EvidenceType::Forensic,
            0.95,
            10.0,
            DataSourceRef::ManualEntry {
                analyst_id: Uuid::new_v4(),
                timestamp: Utc::now(),
            },
        );
        evidence.attach_to_branch(branch, AttachmentMode::Manual, EvidencePolarity::Supporting, 1.0);
        let eid = scenario.add_evidence(evidence);

        // Update: prior 0.5 with LR ~10*0.95 should push posterior well above 0.5
        let new_posterior = scenario.bayesian_update(branch, eid).unwrap();
        assert!(new_posterior > 0.8, "Posterior was {new_posterior}");
    }

    #[test]
    fn test_soft_pruning() {
        let mut scenario = Scenario::new("Prune Test", ScenarioScale::Micro);
        scenario.collapse_threshold = 0.1;
        let root = scenario.set_root_branch("Root", 1.0);
        let high = scenario.add_branch(root, "High Prob", 0.9).unwrap();
        let low = scenario.add_branch(root, "Low Prob", 0.02).unwrap();

        scenario.apply_soft_pruning();

        assert_eq!(scenario.branch(high).unwrap().status, BranchStatus::Active);
        assert_eq!(scenario.branch(low).unwrap().status, BranchStatus::Collapsed);
    }

    #[test]
    fn test_normalize_siblings() {
        let mut scenario = Scenario::new("Normalize Test", ScenarioScale::Micro);
        let root = scenario.set_root_branch("Root", 1.0);
        let a = scenario.add_branch(root, "A", 0.3).unwrap();
        let b = scenario.add_branch(root, "B", 0.3).unwrap();
        let c = scenario.add_branch(root, "C", 0.3).unwrap();

        scenario.normalize_siblings();

        let sum: f64 = [a, b, c].iter()
            .map(|id| scenario.branch(*id).unwrap().posterior)
            .sum();
        assert!((sum - 1.0).abs() < 1e-6, "Sum was {sum}");
    }
}
