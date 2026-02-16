//! # Eustress Scenarios — Evidence Attachment Manager
//!
//! Table of Contents:
//! 1. EvidenceManager — Bulk operations on evidence-branch links
//! 2. EvidenceConflict — Conflict detection between evidence items
//! 3. AutoAttacher — Embedding-based automatic evidence attachment
//! 4. AttachmentSuggestion — Suggested link from auto-attach
//! 5. EvidenceQuery — Filtered queries over evidence pools

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{
    AttachmentMode, BranchNode, BranchStatus, Evidence, EvidenceLink,
    EvidencePolarity, EvidenceType, Scenario,
};

// ─────────────────────────────────────────────
// 1. EvidenceManager — Bulk operations
// ─────────────────────────────────────────────

/// High-level evidence attachment operations on a Scenario.
/// All methods take `&mut Scenario` to keep the manager stateless.
pub struct EvidenceManager;

impl EvidenceManager {
    // === Attach ===

    /// Attach evidence to a branch (manual mode).
    /// Returns true if the link was created, false if it already exists.
    pub fn attach(
        scenario: &mut Scenario,
        evidence_id: Uuid,
        branch_id: Uuid,
        polarity: EvidencePolarity,
        weight: f64,
        analyst_id: Option<Uuid>,
    ) -> bool {
        // Verify branch exists
        if !scenario.branches.contains_key(&branch_id) {
            return false;
        }

        // Find evidence and check for duplicate link
        let evidence = match scenario.evidence.iter_mut().find(|e| e.id == evidence_id) {
            Some(e) => e,
            None => return false,
        };

        if evidence.links.iter().any(|l| l.branch_id == branch_id) {
            return false; // Already linked
        }

        evidence.links.push(EvidenceLink {
            branch_id,
            mode: AttachmentMode::Manual,
            polarity,
            weight: weight.clamp(0.0, 1.0),
            linked_at: Utc::now(),
            linked_by: analyst_id,
        });

        // Also register in the branch's evidence_ids
        if let Some(branch) = scenario.branches.get_mut(&branch_id) {
            if !branch.evidence_ids.contains(&evidence_id) {
                branch.evidence_ids.push(evidence_id);
            }
        }

        scenario.updated_at = Utc::now();
        true
    }

    /// Batch attach: link one evidence item to multiple branches.
    /// Returns the number of new links created.
    pub fn attach_to_many(
        scenario: &mut Scenario,
        evidence_id: Uuid,
        targets: &[(Uuid, EvidencePolarity, f64)],
        analyst_id: Option<Uuid>,
    ) -> usize {
        let mut count = 0;
        for &(branch_id, polarity, weight) in targets {
            if Self::attach(scenario, evidence_id, branch_id, polarity, weight, analyst_id) {
                count += 1;
            }
        }
        count
    }

    /// Batch attach: link multiple evidence items to one branch.
    /// Returns the number of new links created.
    pub fn attach_many_to(
        scenario: &mut Scenario,
        evidence_ids: &[Uuid],
        branch_id: Uuid,
        polarity: EvidencePolarity,
        weight: f64,
        analyst_id: Option<Uuid>,
    ) -> usize {
        let mut count = 0;
        for &eid in evidence_ids {
            if Self::attach(scenario, eid, branch_id, polarity, weight, analyst_id) {
                count += 1;
            }
        }
        count
    }

    // === Detach ===

    /// Detach evidence from a branch.
    /// Returns true if the link was removed.
    pub fn detach(
        scenario: &mut Scenario,
        evidence_id: Uuid,
        branch_id: Uuid,
    ) -> bool {
        let evidence = match scenario.evidence.iter_mut().find(|e| e.id == evidence_id) {
            Some(e) => e,
            None => return false,
        };

        let before = evidence.links.len();
        evidence.links.retain(|l| l.branch_id != branch_id);
        let removed = evidence.links.len() < before;

        if removed {
            // Also remove from branch's evidence_ids
            if let Some(branch) = scenario.branches.get_mut(&branch_id) {
                branch.evidence_ids.retain(|&id| id != evidence_id);
            }
            scenario.updated_at = Utc::now();
        }

        removed
    }

    /// Detach all evidence from a branch.
    /// Returns the number of links removed.
    pub fn detach_all_from_branch(
        scenario: &mut Scenario,
        branch_id: Uuid,
    ) -> usize {
        let mut count = 0;
        for evidence in &mut scenario.evidence {
            let before = evidence.links.len();
            evidence.links.retain(|l| l.branch_id != branch_id);
            count += before - evidence.links.len();
        }

        if let Some(branch) = scenario.branches.get_mut(&branch_id) {
            branch.evidence_ids.clear();
        }

        if count > 0 {
            scenario.updated_at = Utc::now();
        }
        count
    }

    /// Detach all branches from a specific evidence item.
    /// Returns the number of links removed.
    pub fn detach_all_from_evidence(
        scenario: &mut Scenario,
        evidence_id: Uuid,
    ) -> usize {
        let branch_ids: Vec<Uuid> = scenario
            .evidence
            .iter()
            .find(|e| e.id == evidence_id)
            .map(|e| e.links.iter().map(|l| l.branch_id).collect())
            .unwrap_or_default();

        let evidence = match scenario.evidence.iter_mut().find(|e| e.id == evidence_id) {
            Some(e) => e,
            None => return 0,
        };

        let count = evidence.links.len();
        evidence.links.clear();

        // Clean up branch references
        for bid in &branch_ids {
            if let Some(branch) = scenario.branches.get_mut(bid) {
                branch.evidence_ids.retain(|&id| id != evidence_id);
            }
        }

        if count > 0 {
            scenario.updated_at = Utc::now();
        }
        count
    }

    // === Re-weight ===

    /// Update the weight of an existing evidence-branch link.
    /// Returns the old weight, or None if the link doesn't exist.
    pub fn reweight(
        scenario: &mut Scenario,
        evidence_id: Uuid,
        branch_id: Uuid,
        new_weight: f64,
    ) -> Option<f64> {
        let evidence = scenario.evidence.iter_mut().find(|e| e.id == evidence_id)?;
        let link = evidence.links.iter_mut().find(|l| l.branch_id == branch_id)?;
        let old = link.weight;
        link.weight = new_weight.clamp(0.0, 1.0);
        scenario.updated_at = Utc::now();
        Some(old)
    }

    /// Update the polarity of an existing evidence-branch link.
    /// Returns the old polarity, or None if the link doesn't exist.
    pub fn repolarize(
        scenario: &mut Scenario,
        evidence_id: Uuid,
        branch_id: Uuid,
        new_polarity: EvidencePolarity,
    ) -> Option<EvidencePolarity> {
        let evidence = scenario.evidence.iter_mut().find(|e| e.id == evidence_id)?;
        let link = evidence.links.iter_mut().find(|l| l.branch_id == branch_id)?;
        let old = link.polarity;
        link.polarity = new_polarity;
        scenario.updated_at = Utc::now();
        Some(old)
    }

    // === Query ===

    /// Get all evidence linked to a specific branch.
    pub fn evidence_for_branch(scenario: &Scenario, branch_id: Uuid) -> Vec<(&Evidence, &EvidenceLink)> {
        scenario
            .evidence
            .iter()
            .filter_map(|e| {
                e.links
                    .iter()
                    .find(|l| l.branch_id == branch_id)
                    .map(|link| (e, link))
            })
            .collect()
    }

    /// Get all branches linked to a specific evidence item.
    pub fn branches_for_evidence(scenario: &Scenario, evidence_id: Uuid) -> Vec<(&BranchNode, &EvidenceLink)> {
        let evidence = match scenario.evidence.iter().find(|e| e.id == evidence_id) {
            Some(e) => e,
            None => return Vec::new(),
        };

        evidence
            .links
            .iter()
            .filter_map(|link| {
                scenario.branches.get(&link.branch_id).map(|b| (b, link))
            })
            .collect()
    }

    /// Get unlinked evidence (evidence not attached to any branch).
    pub fn unlinked_evidence(scenario: &Scenario) -> Vec<&Evidence> {
        scenario.evidence.iter().filter(|e| e.links.is_empty()).collect()
    }

    /// Get orphaned branches (active branches with no evidence).
    pub fn orphaned_branches(scenario: &Scenario) -> Vec<&BranchNode> {
        scenario
            .branches
            .values()
            .filter(|b| b.status == BranchStatus::Active && b.evidence_ids.is_empty())
            .collect()
    }

    /// Compute evidence coverage: fraction of active branches that have at least one evidence link.
    pub fn coverage(scenario: &Scenario) -> f64 {
        let active: Vec<_> = scenario
            .branches
            .values()
            .filter(|b| b.status == BranchStatus::Active)
            .collect();

        if active.is_empty() {
            return 1.0;
        }

        let covered = active.iter().filter(|b| !b.evidence_ids.is_empty()).count();
        covered as f64 / active.len() as f64
    }
}

// ─────────────────────────────────────────────
// 2. EvidenceConflict — Conflict detection
// ─────────────────────────────────────────────

/// A detected conflict between two evidence items on the same branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceConflict {
    /// First evidence ID
    pub evidence_a: Uuid,
    /// Second evidence ID
    pub evidence_b: Uuid,
    /// Branch where the conflict occurs
    pub branch_id: Uuid,
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// Severity score (0.0 = minor, 1.0 = critical)
    pub severity: f64,
    /// When the conflict was detected
    pub detected_at: DateTime<Utc>,
}

/// Classification of evidence conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictType {
    /// One supports, other contradicts the same branch
    PolarityConflict,
    /// Both support but with very different likelihood ratios (>10x difference)
    MagnitudeConflict,
    /// Same evidence type with contradictory conclusions
    TypeConflict,
    /// Temporal impossibility (evidence timestamps conflict)
    TemporalConflict,
}

/// Resolution strategy for evidence conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Use weighted average of both
    WeightedAverage,
    /// Prefer evidence A
    PreferA,
    /// Prefer evidence B
    PreferB,
    /// Hold both without resolution (flag for analyst)
    HoldBoth,
    /// Mark as unresolved
    Unresolved,
}

/// Detect conflicts between evidence items on the same branch.
pub fn detect_conflicts(scenario: &Scenario) -> Vec<EvidenceConflict> {
    let mut conflicts = Vec::new();

    // Group evidence by branch
    let mut branch_evidence: HashMap<Uuid, Vec<(Uuid, &EvidenceLink, &Evidence)>> = HashMap::new();
    for evidence in &scenario.evidence {
        for link in &evidence.links {
            branch_evidence
                .entry(link.branch_id)
                .or_default()
                .push((evidence.id, link, evidence));
        }
    }

    // Check each branch for conflicts
    for (branch_id, items) in &branch_evidence {
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                let (id_a, link_a, ev_a) = &items[i];
                let (id_b, link_b, ev_b) = &items[j];

                // Polarity conflict: one supports, other contradicts
                if (link_a.polarity == EvidencePolarity::Supporting
                    && link_b.polarity == EvidencePolarity::Contradicting)
                    || (link_a.polarity == EvidencePolarity::Contradicting
                        && link_b.polarity == EvidencePolarity::Supporting)
                {
                    let severity = (link_a.weight * ev_a.confidence
                        + link_b.weight * ev_b.confidence)
                        / 2.0;

                    conflicts.push(EvidenceConflict {
                        evidence_a: *id_a,
                        evidence_b: *id_b,
                        branch_id: *branch_id,
                        conflict_type: ConflictType::PolarityConflict,
                        severity,
                        detected_at: Utc::now(),
                    });
                }

                // Magnitude conflict: both support but LRs differ by >10x
                if link_a.polarity == link_b.polarity
                    && link_a.polarity != EvidencePolarity::Neutral
                {
                    let ratio = if ev_b.likelihood_ratio > 0.0 {
                        ev_a.likelihood_ratio / ev_b.likelihood_ratio
                    } else {
                        f64::INFINITY
                    };

                    if ratio > 10.0 || ratio < 0.1 {
                        conflicts.push(EvidenceConflict {
                            evidence_a: *id_a,
                            evidence_b: *id_b,
                            branch_id: *branch_id,
                            conflict_type: ConflictType::MagnitudeConflict,
                            severity: 0.5,
                            detected_at: Utc::now(),
                        });
                    }
                }

                // Temporal conflict: timestamps overlap impossibly
                if let (Some(ts_a), Some(ts_b)) = (ev_a.timestamp, ev_b.timestamp) {
                    // Same evidence type, same timestamp, different conclusions
                    if ev_a.evidence_type == ev_b.evidence_type
                        && (ts_a - ts_b).num_seconds().unsigned_abs() < 60
                        && link_a.polarity != link_b.polarity
                    {
                        conflicts.push(EvidenceConflict {
                            evidence_a: *id_a,
                            evidence_b: *id_b,
                            branch_id: *branch_id,
                            conflict_type: ConflictType::TemporalConflict,
                            severity: 0.8,
                            detected_at: Utc::now(),
                        });
                    }
                }
            }
        }
    }

    conflicts
}

// ─────────────────────────────────────────────
// 3. AutoAttacher — Embedding-based auto-attach
// ─────────────────────────────────────────────

/// Configuration for automatic evidence attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoAttachConfig {
    /// Minimum cosine similarity threshold for auto-attach (0.0 to 1.0)
    pub similarity_threshold: f64,
    /// Whether to auto-confirm or leave as SuggestedPending
    pub auto_confirm: bool,
    /// Default weight for auto-attached links
    pub default_weight: f64,
    /// Maximum number of branches to suggest per evidence item
    pub max_suggestions: usize,
}

impl Default for AutoAttachConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.7,
            auto_confirm: false,
            default_weight: 0.5,
            max_suggestions: 5,
        }
    }
}

/// A suggested evidence-branch link from the auto-attacher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentSuggestion {
    /// Evidence ID
    pub evidence_id: Uuid,
    /// Suggested branch ID
    pub branch_id: Uuid,
    /// Cosine similarity score
    pub similarity: f64,
    /// Suggested polarity (inferred from embedding direction)
    pub suggested_polarity: EvidencePolarity,
    /// Suggested weight
    pub suggested_weight: f64,
    /// When this suggestion was generated
    pub generated_at: DateTime<Utc>,
}

/// Auto-attach evidence to branches using text embedding similarity.
///
/// This uses a simple TF-IDF-like approach for text similarity when
/// no external embedding model is available. For production use,
/// replace `compute_similarity` with an ONNX model call via `ort`.
pub struct AutoAttacher {
    config: AutoAttachConfig,
}

impl AutoAttacher {
    /// Create a new auto-attacher with the given config.
    pub fn new(config: AutoAttachConfig) -> Self {
        Self { config }
    }

    /// Generate attachment suggestions for a single evidence item.
    /// Compares evidence text against branch labels and descriptions.
    pub fn suggest_for_evidence(
        &self,
        evidence: &Evidence,
        scenario: &Scenario,
    ) -> Vec<AttachmentSuggestion> {
        let evidence_text = Self::evidence_to_text(evidence);
        let evidence_tokens = Self::tokenize(&evidence_text);

        let mut suggestions: Vec<AttachmentSuggestion> = scenario
            .branches
            .iter()
            .filter(|(_, b)| b.status == BranchStatus::Active)
            .filter(|(id, _)| !evidence.links.iter().any(|l| l.branch_id == **id))
            .filter_map(|(&branch_id, branch)| {
                let branch_text = &branch.label;
                let branch_tokens = Self::tokenize(branch_text);
                let similarity = Self::cosine_similarity(&evidence_tokens, &branch_tokens);

                if similarity >= self.config.similarity_threshold {
                    Some(AttachmentSuggestion {
                        evidence_id: evidence.id,
                        branch_id,
                        similarity,
                        suggested_polarity: EvidencePolarity::Supporting,
                        suggested_weight: self.config.default_weight,
                        generated_at: Utc::now(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by similarity descending, take top N
        suggestions.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
        suggestions.truncate(self.config.max_suggestions);
        suggestions
    }

    /// Generate suggestions for all unlinked evidence in a scenario.
    pub fn suggest_all_unlinked(&self, scenario: &Scenario) -> Vec<AttachmentSuggestion> {
        let unlinked = EvidenceManager::unlinked_evidence(scenario);
        unlinked
            .into_iter()
            .flat_map(|e| self.suggest_for_evidence(e, scenario))
            .collect()
    }

    /// Apply suggestions: create links with SuggestedPending or SuggestedConfirmed mode.
    pub fn apply_suggestions(
        scenario: &mut Scenario,
        suggestions: &[AttachmentSuggestion],
        auto_confirm: bool,
    ) -> usize {
        let mode = if auto_confirm {
            AttachmentMode::SuggestedConfirmed
        } else {
            AttachmentMode::SuggestedPending
        };

        let mut count = 0;
        for suggestion in suggestions {
            // Verify branch still exists and evidence still unlinked to this branch
            if !scenario.branches.contains_key(&suggestion.branch_id) {
                continue;
            }

            let evidence = match scenario.evidence.iter_mut().find(|e| e.id == suggestion.evidence_id) {
                Some(e) => e,
                None => continue,
            };

            if evidence.links.iter().any(|l| l.branch_id == suggestion.branch_id) {
                continue;
            }

            evidence.links.push(EvidenceLink {
                branch_id: suggestion.branch_id,
                mode,
                polarity: suggestion.suggested_polarity,
                weight: suggestion.suggested_weight,
                linked_at: Utc::now(),
                linked_by: None,
            });

            if let Some(branch) = scenario.branches.get_mut(&suggestion.branch_id) {
                if !branch.evidence_ids.contains(&suggestion.evidence_id) {
                    branch.evidence_ids.push(suggestion.evidence_id);
                }
            }

            count += 1;
        }

        if count > 0 {
            scenario.updated_at = Utc::now();
        }
        count
    }

    /// Confirm a pending suggestion (upgrade SuggestedPending → SuggestedConfirmed).
    pub fn confirm_suggestion(
        scenario: &mut Scenario,
        evidence_id: Uuid,
        branch_id: Uuid,
    ) -> bool {
        let evidence = match scenario.evidence.iter_mut().find(|e| e.id == evidence_id) {
            Some(e) => e,
            None => return false,
        };

        let link = match evidence.links.iter_mut().find(|l| l.branch_id == branch_id) {
            Some(l) => l,
            None => return false,
        };

        if link.mode == AttachmentMode::SuggestedPending {
            link.mode = AttachmentMode::SuggestedConfirmed;
            scenario.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    /// Reject a pending suggestion (remove the link).
    pub fn reject_suggestion(
        scenario: &mut Scenario,
        evidence_id: Uuid,
        branch_id: Uuid,
    ) -> bool {
        EvidenceManager::detach(scenario, evidence_id, branch_id)
    }

    // === Internal text processing ===

    /// Convert evidence to searchable text.
    fn evidence_to_text(evidence: &Evidence) -> String {
        let mut parts = vec![evidence.label.clone()];
        if let Some(ref notes) = evidence.notes {
            parts.push(notes.clone());
        }
        parts.push(format!("{:?}", evidence.evidence_type));
        parts.join(" ")
    }

    /// Simple whitespace tokenizer with lowercasing and stop-word removal.
    fn tokenize(text: &str) -> HashMap<String, f64> {
        let stop_words = [
            "the", "a", "an", "is", "was", "are", "were", "be", "been",
            "being", "have", "has", "had", "do", "does", "did", "will",
            "would", "could", "should", "may", "might", "shall", "can",
            "of", "in", "to", "for", "with", "on", "at", "from", "by",
            "and", "or", "but", "not", "no", "if", "then", "else",
            "this", "that", "it", "its", "as", "so",
        ];

        let mut counts: HashMap<String, f64> = HashMap::new();
        for word in text.to_lowercase().split_whitespace() {
            let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
            if clean.len() > 1 && !stop_words.contains(&clean.as_str()) {
                *counts.entry(clean).or_default() += 1.0;
            }
        }

        // Normalize to unit vector
        let magnitude: f64 = counts.values().map(|v| v * v).sum::<f64>().sqrt();
        if magnitude > 0.0 {
            for v in counts.values_mut() {
                *v /= magnitude;
            }
        }

        counts
    }

    /// Cosine similarity between two token vectors.
    fn cosine_similarity(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> f64 {
        // Vectors are already normalized, so dot product = cosine similarity
        a.iter()
            .filter_map(|(key, val_a)| b.get(key).map(|val_b| val_a * val_b))
            .sum()
    }
}

// ─────────────────────────────────────────────
// 4. EvidenceQuery — Filtered queries
// ─────────────────────────────────────────────

/// Filter criteria for querying evidence.
#[derive(Debug, Clone, Default)]
pub struct EvidenceQuery {
    /// Filter by evidence type
    pub evidence_type: Option<EvidenceType>,
    /// Filter by minimum confidence
    pub min_confidence: Option<f64>,
    /// Filter by attachment mode
    pub attachment_mode: Option<AttachmentMode>,
    /// Filter by polarity (on any link)
    pub polarity: Option<EvidencePolarity>,
    /// Filter by branch ID (must be linked to this branch)
    pub branch_id: Option<Uuid>,
    /// Filter by time range (added_at)
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    /// Text search in label/notes
    pub text_search: Option<String>,
}

impl EvidenceQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_type(mut self, t: EvidenceType) -> Self {
        self.evidence_type = Some(t);
        self
    }

    pub fn with_min_confidence(mut self, c: f64) -> Self {
        self.min_confidence = Some(c);
        self
    }

    pub fn with_branch(mut self, id: Uuid) -> Self {
        self.branch_id = Some(id);
        self
    }

    pub fn with_polarity(mut self, p: EvidencePolarity) -> Self {
        self.polarity = Some(p);
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text_search = Some(text.into());
        self
    }

    /// Execute this query against a scenario's evidence pool.
    pub fn execute<'a>(&self, scenario: &'a Scenario) -> Vec<&'a Evidence> {
        scenario
            .evidence
            .iter()
            .filter(|e| {
                // Type filter
                if let Some(ref t) = self.evidence_type {
                    if e.evidence_type != *t {
                        return false;
                    }
                }

                // Confidence filter
                if let Some(min_c) = self.min_confidence {
                    if e.confidence < min_c {
                        return false;
                    }
                }

                // Branch filter
                if let Some(bid) = self.branch_id {
                    if !e.links.iter().any(|l| l.branch_id == bid) {
                        return false;
                    }
                }

                // Attachment mode filter
                if let Some(mode) = self.attachment_mode {
                    if !e.links.iter().any(|l| l.mode == mode) {
                        return false;
                    }
                }

                // Polarity filter
                if let Some(pol) = self.polarity {
                    if !e.links.iter().any(|l| l.polarity == pol) {
                        return false;
                    }
                }

                // Time range filter
                if let Some(after) = self.after {
                    if e.added_at < after {
                        return false;
                    }
                }
                if let Some(before) = self.before {
                    if e.added_at > before {
                        return false;
                    }
                }

                // Text search filter
                if let Some(ref text) = self.text_search {
                    let lower = text.to_lowercase();
                    let in_label = e.label.to_lowercase().contains(&lower);
                    let in_notes = e
                        .notes
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&lower))
                        .unwrap_or(false);
                    if !in_label && !in_notes {
                        return false;
                    }
                }

                true
            })
            .collect()
    }
}

// ─────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::types::{DataSourceRef, ScenarioScale};

    fn make_scenario() -> Scenario {
        let mut s = Scenario::new("Evidence Test", ScenarioScale::Micro);
        let root = s.set_root_branch("Root", 1.0);
        s.add_branch(root, "Suspect A", 0.5);
        s.add_branch(root, "Suspect B", 0.5);
        s
    }

    fn make_evidence(label: &str, lr: f64) -> Evidence {
        Evidence::new(
            label,
            EvidenceType::Physical,
            0.9,
            lr,
            DataSourceRef::ManualEntry {
                analyst_id: Uuid::new_v4(),
                timestamp: Utc::now(),
            },
        )
    }

    #[test]
    fn test_attach_and_detach() {
        let mut scenario = make_scenario();
        let evidence = make_evidence("DNA Match", 10.0);
        let eid = scenario.add_evidence(evidence);

        let branches: Vec<Uuid> = scenario.branches.keys().copied().collect();
        let branch_a = branches.iter().find(|&&id| {
            scenario.branch(id).unwrap().label == "Suspect A"
        }).copied().unwrap();

        // Attach
        assert!(EvidenceManager::attach(
            &mut scenario, eid, branch_a,
            EvidencePolarity::Supporting, 1.0, None,
        ));

        // Verify
        let linked = EvidenceManager::evidence_for_branch(&scenario, branch_a);
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].0.id, eid);

        // Duplicate attach should fail
        assert!(!EvidenceManager::attach(
            &mut scenario, eid, branch_a,
            EvidencePolarity::Supporting, 1.0, None,
        ));

        // Detach
        assert!(EvidenceManager::detach(&mut scenario, eid, branch_a));
        assert!(EvidenceManager::evidence_for_branch(&scenario, branch_a).is_empty());
    }

    #[test]
    fn test_conflict_detection() {
        let mut scenario = make_scenario();
        let branches: Vec<Uuid> = scenario.branches.keys().copied().collect();
        let branch_a = branches.iter().find(|&&id| {
            scenario.branch(id).unwrap().label == "Suspect A"
        }).copied().unwrap();

        // Add supporting evidence
        let mut ev1 = make_evidence("Fingerprint", 8.0);
        ev1.attach_to_branch(branch_a, AttachmentMode::Manual, EvidencePolarity::Supporting, 1.0);
        scenario.add_evidence(ev1);

        // Add contradicting evidence on same branch
        let mut ev2 = make_evidence("Alibi", 5.0);
        ev2.attach_to_branch(branch_a, AttachmentMode::Manual, EvidencePolarity::Contradicting, 0.8);
        scenario.add_evidence(ev2);

        let conflicts = detect_conflicts(&scenario);
        assert!(!conflicts.is_empty());
        assert_eq!(conflicts[0].conflict_type, ConflictType::PolarityConflict);
    }

    #[test]
    fn test_coverage() {
        let mut scenario = make_scenario();
        // No evidence attached — coverage should be 0
        let cov = EvidenceManager::coverage(&scenario);
        // Root has no evidence, children have no evidence
        assert!(cov < 0.01);

        // Attach evidence to one branch
        let evidence = make_evidence("Clue", 5.0);
        let eid = scenario.add_evidence(evidence);
        let branch_a = scenario.branches.values()
            .find(|b| b.label == "Suspect A")
            .unwrap().id;

        EvidenceManager::attach(
            &mut scenario, eid, branch_a,
            EvidencePolarity::Supporting, 1.0, None,
        );

        let cov = EvidenceManager::coverage(&scenario);
        // 1 of 3 active branches covered
        assert!((cov - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_evidence_query() {
        let mut scenario = make_scenario();
        let branch_a = scenario.branches.values()
            .find(|b| b.label == "Suspect A")
            .unwrap().id;

        let mut ev1 = make_evidence("DNA Match", 10.0);
        ev1.evidence_type = EvidenceType::Forensic;
        ev1.attach_to_branch(branch_a, AttachmentMode::Manual, EvidencePolarity::Supporting, 1.0);
        scenario.add_evidence(ev1);

        let mut ev2 = make_evidence("CCTV Footage", 3.0);
        ev2.evidence_type = EvidenceType::Surveillance;
        scenario.add_evidence(ev2);

        // Query forensic evidence
        let results = EvidenceQuery::new()
            .with_type(EvidenceType::Forensic)
            .execute(&scenario);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "DNA Match");

        // Query evidence on branch_a
        let results = EvidenceQuery::new()
            .with_branch(branch_a)
            .execute(&scenario);
        assert_eq!(results.len(), 1);

        // Text search
        let results = EvidenceQuery::new()
            .with_text("CCTV")
            .execute(&scenario);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "CCTV Footage");
    }
}
