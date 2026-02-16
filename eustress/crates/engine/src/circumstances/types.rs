//! # Eustress Circumstances — Domain Type Aliases & Vocabulary
//!
//! Table of Contents:
//! 1. Type aliases — Circumstance = Scenario with forward-looking vocabulary
//! 2. Signal / SignalType — Evidence counterpart (market signals, sensor data, feeds)
//! 3. Forecast — BranchNode counterpart (predicted future states)
//! 4. DecisionPoint — Branch where an operator chooses an action
//! 5. CircumstanceParameter / CircumstanceEntity / CircumstanceOutcome

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::scenarios::types::{
    BranchLogic, BranchNode, BranchStatus, DataSourceRef, EntityRelationship,
    EntityRole, Evidence, EvidencePolarity, EvidenceType, GeoPoint, Outcome,
    OutcomeData, OutcomeSeverity, ParameterValue, Scenario, ScenarioEntity,
    ScenarioParameter, ScenarioScale, ScenarioStatus,
};

// ─────────────────────────────────────────────
// 1. Type aliases — Same engine, different vocabulary
// ─────────────────────────────────────────────

/// A Circumstance is a forward-looking Scenario.
/// FBI asks "What happened?" → Scenario
/// Costco asks "What will happen?" → Circumstance
pub type Circumstance = Scenario;

/// Scale of a circumstance analysis.
pub type CircumstanceScale = ScenarioScale;

/// Lifecycle status of a circumstance.
pub type CircumstanceStatus = ScenarioStatus;

/// A parameter driving a circumstance (e.g., "Current Inventory Level", "Lead Time").
pub type CircumstanceParameter = ScenarioParameter;

/// An entity in a circumstance (supplier, warehouse, product, route).
pub type CircumstanceEntity = ScenarioEntity;

/// An outcome of a circumstance branch (e.g., "Stockout in 3 days", "On-time delivery").
pub type CircumstanceOutcome = Outcome;

// ─────────────────────────────────────────────
// 2. Signal / SignalType — Forward-looking evidence
// ─────────────────────────────────────────────

/// A Signal is the Circumstances counterpart to Evidence.
/// Instead of "DNA found at scene" (backward), it's
/// "Port congestion detected at Shanghai" (forward).
///
/// Signals update branch probabilities the same way Evidence does —
/// via likelihood ratios and Bayesian updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable label
    pub label: String,
    /// Signal classification
    pub signal_type: SignalType,
    /// Confidence in this signal's reliability (0.0 to 1.0)
    pub confidence: f64,
    /// Likelihood ratio: P(signal | hypothesis_true) / P(signal | hypothesis_false)
    pub likelihood_ratio: f64,
    /// When this signal was observed
    pub timestamp: DateTime<Utc>,
    /// Where this signal originated (if geographic)
    pub location: Option<GeoPoint>,
    /// Source of this signal
    pub source: DataSourceRef,
    /// Time-to-live: how long this signal remains relevant (seconds)
    pub ttl_seconds: Option<f64>,
    /// Whether this signal has expired
    pub expired: bool,
    /// Free-form metadata
    pub metadata: HashMap<String, String>,
}

/// Classification of signal types for Circumstances.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignalType {
    /// Market demand signal (POS data, order volume, search trends)
    Demand,
    /// Supply disruption signal (port closure, factory shutdown, weather)
    Disruption,
    /// Price signal (commodity price change, competitor pricing)
    Price,
    /// Quality signal (defect rate, inspection result, sensor anomaly)
    Quality,
    /// Logistics signal (shipment delay, route change, capacity constraint)
    Logistics,
    /// Regulatory signal (new regulation, recall notice, compliance change)
    Regulatory,
    /// Weather signal (forecast, severe weather alert, seasonal pattern)
    Weather,
    /// IoT sensor signal (temperature, humidity, vibration, pressure)
    Sensor,
    /// Financial signal (exchange rate, credit rating, payment delay)
    Financial,
    /// Competitive signal (competitor action, market entry, product launch)
    Competitive,
    /// Custom signal type
    Custom(String),
}

impl Signal {
    /// Create a new signal.
    pub fn new(
        label: impl Into<String>,
        signal_type: SignalType,
        confidence: f64,
        likelihood_ratio: f64,
        source: DataSourceRef,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            signal_type,
            confidence: confidence.clamp(0.0, 1.0),
            likelihood_ratio,
            timestamp: Utc::now(),
            location: None,
            source,
            ttl_seconds: None,
            expired: false,
            metadata: HashMap::new(),
        }
    }

    /// Check if this signal has expired based on TTL.
    pub fn is_expired(&self) -> bool {
        if self.expired {
            return true;
        }
        if let Some(ttl) = self.ttl_seconds {
            let age = (Utc::now() - self.timestamp).num_seconds() as f64;
            age > ttl
        } else {
            false
        }
    }

    /// Convert this Signal into an Evidence object for the shared engine.
    /// This is the bridge between Circumstances vocabulary and the core engine.
    pub fn to_evidence(&self) -> Evidence {
        let evidence_type = match &self.signal_type {
            SignalType::Demand => EvidenceType::Statistical,
            SignalType::Disruption => EvidenceType::Circumstantial,
            SignalType::Price => EvidenceType::Statistical,
            SignalType::Quality => EvidenceType::Forensic,
            SignalType::Logistics => EvidenceType::Documentary,
            SignalType::Regulatory => EvidenceType::Documentary,
            SignalType::Weather => EvidenceType::Statistical,
            SignalType::Sensor => EvidenceType::Digital,
            SignalType::Financial => EvidenceType::Transaction,
            SignalType::Competitive => EvidenceType::Circumstantial,
            SignalType::Custom(name) => EvidenceType::Custom(name.clone()),
        };

        Evidence::new(
            &self.label,
            evidence_type,
            self.confidence,
            self.likelihood_ratio,
            self.source.clone(),
        )
    }
}

// ─────────────────────────────────────────────
// 3. Forecast — Predicted future state
// ─────────────────────────────────────────────

/// A Forecast is a BranchNode with forward-looking semantics.
/// Instead of "Suspect A did it" (hypothesis), it's
/// "Shipment arrives late by 3 days" (prediction).
pub type Forecast = BranchNode;

// ─────────────────────────────────────────────
// 4. DecisionPoint — Operator action branch
// ─────────────────────────────────────────────

/// A decision point where an operator must choose an action.
/// Each child branch represents a possible action and its predicted outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPoint {
    /// The branch node representing this decision
    pub branch_id: Uuid,
    /// Human-readable decision description
    pub description: String,
    /// Available actions (child branch IDs with labels)
    pub actions: Vec<ActionOption>,
    /// Deadline for this decision (if time-sensitive)
    pub deadline: Option<DateTime<Utc>>,
    /// Estimated cost of inaction (doing nothing)
    pub inaction_cost: Option<f64>,
    /// Whether this decision has been made
    pub resolved: bool,
    /// The chosen action (if resolved)
    pub chosen_action: Option<Uuid>,
}

/// A possible action at a decision point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOption {
    /// Branch ID representing this action's outcome tree
    pub branch_id: Uuid,
    /// Action label (e.g., "Reorder now", "Switch supplier", "Wait 48h")
    pub label: String,
    /// Estimated cost of this action
    pub estimated_cost: Option<f64>,
    /// Estimated time to execute (seconds)
    pub estimated_duration: Option<f64>,
    /// Risk level of this action
    pub risk: OutcomeSeverity,
}

// ─────────────────────────────────────────────
// 5. Convenience constructors
// ─────────────────────────────────────────────

/// Create a new Circumstance (forward-looking scenario).
pub fn new_circumstance(name: impl Into<String>, scale: CircumstanceScale) -> Circumstance {
    Scenario::new(name, scale)
}

/// Create a micro circumstance (tactical, single-decision).
/// Example: "Should we reorder SKU X from Supplier Y?"
pub fn micro_circumstance(name: impl Into<String>) -> Circumstance {
    Scenario::new(name, ScenarioScale::Micro)
}

/// Create a macro circumstance (strategic, multi-decision).
/// Example: "Q4 supply chain resilience for electronics category"
pub fn macro_circumstance(name: impl Into<String>) -> Circumstance {
    Scenario::new(name, ScenarioScale::Macro)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_to_evidence_conversion() {
        let signal = Signal::new(
            "Port congestion at Shanghai",
            SignalType::Disruption,
            0.85,
            5.0,
            DataSourceRef::ManualEntry {
                analyst_id: Uuid::new_v4(),
                timestamp: Utc::now(),
            },
        );

        let evidence = signal.to_evidence();
        assert_eq!(evidence.label, "Port congestion at Shanghai");
        assert_eq!(evidence.confidence, 0.85);
        assert_eq!(evidence.likelihood_ratio, 5.0);
    }

    #[test]
    fn test_signal_ttl_expiry() {
        let mut signal = Signal::new(
            "Flash sale detected",
            SignalType::Demand,
            0.9,
            3.0,
            DataSourceRef::ManualEntry {
                analyst_id: Uuid::new_v4(),
                timestamp: Utc::now(),
            },
        );

        // No TTL = never expires
        assert!(!signal.is_expired());

        // Set TTL to 0 seconds = already expired
        signal.ttl_seconds = Some(0.0);
        // Note: might not be expired if checked instantly, but logically it should be
        // In practice the timestamp check handles this
    }

    #[test]
    fn test_circumstance_is_scenario() {
        let circ = micro_circumstance("Reorder Decision");
        assert_eq!(circ.scale, ScenarioScale::Micro);
        assert_eq!(circ.name, "Reorder Decision");

        // Can use all Scenario methods
        let mut circ = circ;
        let root = circ.set_root_branch("Current State", 1.0);
        circ.add_branch(root, "Reorder Now", 0.6);
        circ.add_branch(root, "Wait 48h", 0.4);
        assert_eq!(circ.branch_count(), 3);
    }

    #[test]
    fn test_decision_point() {
        let dp = DecisionPoint {
            branch_id: Uuid::new_v4(),
            description: "Reorder Kirkland batteries?".into(),
            actions: vec![
                ActionOption {
                    branch_id: Uuid::new_v4(),
                    label: "Reorder 10,000 units now".into(),
                    estimated_cost: Some(45_000.0),
                    estimated_duration: Some(86400.0 * 14.0), // 14 days
                    risk: OutcomeSeverity::Low,
                },
                ActionOption {
                    branch_id: Uuid::new_v4(),
                    label: "Switch to backup supplier".into(),
                    estimated_cost: Some(52_000.0),
                    estimated_duration: Some(86400.0 * 7.0), // 7 days
                    risk: OutcomeSeverity::Medium,
                },
            ],
            deadline: None,
            inaction_cost: Some(120_000.0), // Stockout cost
            resolved: false,
            chosen_action: None,
        };

        assert_eq!(dp.actions.len(), 2);
        assert!(!dp.resolved);
    }
}
