//! # Eustress Circumstances — Forecasting & Risk Models
//!
//! Table of Contents:
//! 1. DemandForecast / DemandSignal — Demand prediction from POS/order data
//! 2. DisruptionType — Classification of supply chain disruptions
//! 3. SupplierRiskScore — Continuous risk scoring with trend detection
//! 4. InventoryPolicy — Multi-echelon inventory optimization
//! 5. RecallTrace — Instant lot/serial trace through provenance chain
//! 6. RouteOption — Route & cost optimization alternatives

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::supply_chain::TransportMode;

// ─────────────────────────────────────────────
// 1. DemandForecast / DemandSignal
// ─────────────────────────────────────────────

/// A demand forecast for a product at a specific node over a time horizon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandForecast {
    /// Unique identifier
    pub id: Uuid,
    /// Product being forecasted
    pub product_id: Uuid,
    /// Node where demand is measured (e.g., a retail store)
    pub node_id: Uuid,
    /// Forecast horizon start
    pub horizon_start: DateTime<Utc>,
    /// Forecast horizon end
    pub horizon_end: DateTime<Utc>,
    /// Point estimate (expected units demanded)
    pub point_estimate: f64,
    /// Lower bound (e.g., 10th percentile from Monte Carlo)
    pub lower_bound: f64,
    /// Upper bound (e.g., 90th percentile from Monte Carlo)
    pub upper_bound: f64,
    /// Confidence level of the bounds (e.g., 0.80 for 80% CI)
    pub confidence_level: f64,
    /// Signals that informed this forecast
    pub signal_ids: Vec<Uuid>,
    /// When this forecast was generated
    pub generated_at: DateTime<Utc>,
    /// Model version / method used
    pub method: String,
}

/// A demand signal — a data point that informs demand forecasting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandSignal {
    /// Unique identifier
    pub id: Uuid,
    /// Signal type
    pub signal_type: DemandSignalType,
    /// Product ID (if product-specific)
    pub product_id: Option<Uuid>,
    /// Node ID (if location-specific)
    pub node_id: Option<Uuid>,
    /// Observed value
    pub value: f64,
    /// Timestamp of observation
    pub timestamp: DateTime<Utc>,
    /// Source description
    pub source: String,
}

/// Types of demand signals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DemandSignalType {
    /// Point-of-sale transaction count
    PosSales,
    /// Online order volume
    OnlineOrders,
    /// Search trend index (e.g., Google Trends)
    SearchTrend,
    /// Seasonal pattern (historical same-period)
    SeasonalPattern,
    /// Promotional event (sale, holiday, marketing campaign)
    Promotion,
    /// Weather-driven demand shift
    WeatherDriven,
    /// Competitor action (price drop, stockout, new product)
    CompetitorAction,
    /// Economic indicator (consumer confidence, employment)
    EconomicIndicator,
}

impl DemandForecast {
    /// Create a new demand forecast.
    pub fn new(
        product_id: Uuid,
        node_id: Uuid,
        horizon_start: DateTime<Utc>,
        horizon_end: DateTime<Utc>,
        point_estimate: f64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_id,
            node_id,
            horizon_start,
            horizon_end,
            point_estimate,
            lower_bound: point_estimate * 0.8,
            upper_bound: point_estimate * 1.2,
            confidence_level: 0.80,
            signal_ids: Vec::new(),
            generated_at: Utc::now(),
            method: "bayesian_mc".into(),
        }
    }

    /// Forecast uncertainty range (upper - lower).
    pub fn uncertainty_range(&self) -> f64 {
        self.upper_bound - self.lower_bound
    }

    /// Coefficient of variation (uncertainty relative to estimate).
    pub fn cv(&self) -> f64 {
        if self.point_estimate > 0.0 {
            self.uncertainty_range() / self.point_estimate
        } else {
            f64::INFINITY
        }
    }
}

// ─────────────────────────────────────────────
// 2. DisruptionType
// ─────────────────────────────────────────────

/// Classification of supply chain disruptions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DisruptionType {
    /// Natural disaster (earthquake, hurricane, flood, wildfire)
    NaturalDisaster(String),
    /// Geopolitical event (war, sanctions, trade restrictions)
    Geopolitical(String),
    /// Pandemic / health crisis
    Pandemic,
    /// Port congestion / closure
    PortCongestion,
    /// Factory shutdown (fire, equipment failure, labor dispute)
    FactoryShutdown,
    /// Raw material shortage
    MaterialShortage,
    /// Transportation disruption (route closure, carrier bankruptcy)
    TransportDisruption,
    /// Cyber attack on supply chain systems
    CyberAttack,
    /// Regulatory change (new tariff, import ban, safety requirement)
    RegulatoryChange,
    /// Supplier bankruptcy / financial distress
    SupplierFailure,
    /// Quality failure (contamination, defect batch)
    QualityFailure,
    /// Demand shock (unexpected surge or collapse)
    DemandShock,
    /// Custom disruption
    Custom(String),
}

impl DisruptionType {
    /// Estimated typical duration in hours.
    pub fn typical_duration_hours(&self) -> f64 {
        match self {
            Self::NaturalDisaster(_) => 720.0,   // ~30 days
            Self::Geopolitical(_) => 2160.0,     // ~90 days
            Self::Pandemic => 4320.0,            // ~180 days
            Self::PortCongestion => 336.0,       // ~14 days
            Self::FactoryShutdown => 504.0,      // ~21 days
            Self::MaterialShortage => 720.0,     // ~30 days
            Self::TransportDisruption => 168.0,  // ~7 days
            Self::CyberAttack => 72.0,           // ~3 days
            Self::RegulatoryChange => 1440.0,    // ~60 days
            Self::SupplierFailure => 2160.0,     // ~90 days
            Self::QualityFailure => 336.0,       // ~14 days
            Self::DemandShock => 168.0,          // ~7 days
            Self::Custom(_) => 336.0,            // Default ~14 days
        }
    }
}

// ─────────────────────────────────────────────
// 3. SupplierRiskScore
// ─────────────────────────────────────────────

/// Continuous risk score for a supplier, updated by incoming signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierRiskScore {
    /// Supplier node ID
    pub node_id: Uuid,
    /// Current composite risk (0.0 = safe, 1.0 = critical)
    pub current_risk: f64,
    /// Risk trend (positive = worsening, negative = improving)
    pub trend: f64,
    /// Historical risk scores (timestamp, score)
    pub history: Vec<(DateTime<Utc>, f64)>,
    /// Active disruption types affecting this supplier
    pub active_disruptions: Vec<DisruptionType>,
    /// Recommended alternative supplier IDs
    pub alternatives: Vec<Uuid>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl SupplierRiskScore {
    /// Create a new risk score.
    pub fn new(node_id: Uuid, initial_risk: f64) -> Self {
        Self {
            node_id,
            current_risk: initial_risk.clamp(0.0, 1.0),
            trend: 0.0,
            history: vec![(Utc::now(), initial_risk)],
            active_disruptions: Vec::new(),
            alternatives: Vec::new(),
            updated_at: Utc::now(),
        }
    }

    /// Update risk score with a new observation.
    /// Uses exponential moving average for smoothing.
    pub fn update(&mut self, new_risk: f64, alpha: f64) {
        let clamped = new_risk.clamp(0.0, 1.0);
        let prev = self.current_risk;
        self.current_risk = alpha * clamped + (1.0 - alpha) * prev;
        self.trend = self.current_risk - prev;
        self.history.push((Utc::now(), self.current_risk));
        self.updated_at = Utc::now();
    }

    /// Whether the risk is trending worse.
    pub fn is_worsening(&self) -> bool {
        self.trend > 0.01
    }

    /// Whether this supplier should trigger an alert.
    pub fn should_alert(&self) -> bool {
        self.current_risk > 0.7 || (self.current_risk > 0.5 && self.is_worsening())
    }
}

// ─────────────────────────────────────────────
// 4. InventoryPolicy
// ─────────────────────────────────────────────

/// Inventory optimization policy for a product at a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryPolicy {
    /// Product ID
    pub product_id: Uuid,
    /// Node ID (warehouse, store)
    pub node_id: Uuid,
    /// Current inventory level (units)
    pub current_level: u64,
    /// Reorder point (trigger reorder when inventory drops below this)
    pub reorder_point: u64,
    /// Reorder quantity (how much to order)
    pub reorder_quantity: u64,
    /// Safety stock level (buffer against uncertainty)
    pub safety_stock: u64,
    /// Maximum inventory level
    pub max_level: u64,
    /// Average daily demand (units)
    pub avg_daily_demand: f64,
    /// Lead time for replenishment (days)
    pub lead_time_days: f64,
    /// Days of supply remaining at current demand rate
    pub days_of_supply: f64,
    /// Whether a reorder is currently recommended
    pub reorder_recommended: bool,
    /// Estimated stockout date at current consumption rate
    pub estimated_stockout: Option<DateTime<Utc>>,
}

impl InventoryPolicy {
    /// Create a new inventory policy.
    pub fn new(product_id: Uuid, node_id: Uuid, current_level: u64) -> Self {
        Self {
            product_id,
            node_id,
            current_level,
            reorder_point: 0,
            reorder_quantity: 0,
            safety_stock: 0,
            max_level: u64::MAX,
            avg_daily_demand: 0.0,
            lead_time_days: 0.0,
            days_of_supply: 0.0,
            reorder_recommended: false,
            estimated_stockout: None,
        }
    }

    /// Compute reorder point from demand and lead time.
    /// ROP = (avg_daily_demand * lead_time_days) + safety_stock
    pub fn compute_reorder_point(&mut self) {
        let demand_during_lead = (self.avg_daily_demand * self.lead_time_days).ceil() as u64;
        self.reorder_point = demand_during_lead + self.safety_stock;
    }

    /// Compute days of supply remaining.
    pub fn compute_days_of_supply(&mut self) {
        if self.avg_daily_demand > 0.0 {
            self.days_of_supply = self.current_level as f64 / self.avg_daily_demand;
            let hours = (self.days_of_supply * 24.0) as i64;
            self.estimated_stockout = Some(Utc::now() + chrono::Duration::hours(hours));
        } else {
            self.days_of_supply = f64::INFINITY;
            self.estimated_stockout = None;
        }
    }

    /// Check if reorder is recommended.
    pub fn check_reorder(&mut self) {
        self.compute_reorder_point();
        self.compute_days_of_supply();
        self.reorder_recommended = self.current_level <= self.reorder_point;
    }

    /// Whether this inventory is critically low (below safety stock).
    pub fn is_critical(&self) -> bool {
        self.current_level < self.safety_stock
    }
}

// ─────────────────────────────────────────────
// 5. RecallTrace
// ─────────────────────────────────────────────

/// Result of tracing a recalled product through the supply chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallTrace {
    /// Recall ID
    pub id: Uuid,
    /// Product being recalled
    pub product_id: Uuid,
    /// Lot/batch number being recalled
    pub lot_number: String,
    /// Reason for recall
    pub reason: String,
    /// Severity level
    pub severity: RecallSeverity,
    /// Nodes that currently hold affected inventory
    pub affected_nodes: Vec<AffectedNode>,
    /// Total affected units across all nodes
    pub total_affected_units: u64,
    /// Whether all affected units have been accounted for
    pub fully_traced: bool,
    /// Recall initiation timestamp
    pub initiated_at: DateTime<Utc>,
}

/// Severity of a recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecallSeverity {
    /// Class I: Serious health hazard or death
    ClassI,
    /// Class II: Temporary or reversible health effects
    ClassII,
    /// Class III: Not likely to cause adverse health effects
    ClassIII,
    /// Voluntary: Precautionary, no confirmed issues
    Voluntary,
}

/// A node affected by a recall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedNode {
    /// Node ID
    pub node_id: Uuid,
    /// Units of affected product at this node
    pub units: u64,
    /// Whether this node has been notified
    pub notified: bool,
    /// Whether affected units have been quarantined
    pub quarantined: bool,
    /// Whether affected units have been returned/destroyed
    pub resolved: bool,
}

impl RecallTrace {
    /// Create a new recall trace.
    pub fn new(
        product_id: Uuid,
        lot_number: impl Into<String>,
        reason: impl Into<String>,
        severity: RecallSeverity,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_id,
            lot_number: lot_number.into(),
            reason: reason.into(),
            severity,
            affected_nodes: Vec::new(),
            total_affected_units: 0,
            fully_traced: false,
            initiated_at: Utc::now(),
        }
    }

    /// Add an affected node.
    pub fn add_affected_node(&mut self, node_id: Uuid, units: u64) {
        self.affected_nodes.push(AffectedNode {
            node_id,
            units,
            notified: false,
            quarantined: false,
            resolved: false,
        });
        self.total_affected_units += units;
    }

    /// Percentage of affected units that have been resolved.
    pub fn resolution_pct(&self) -> f64 {
        if self.total_affected_units == 0 {
            return 100.0;
        }
        let resolved: u64 = self.affected_nodes
            .iter()
            .filter(|n| n.resolved)
            .map(|n| n.units)
            .sum();
        resolved as f64 / self.total_affected_units as f64 * 100.0
    }
}

// ─────────────────────────────────────────────
// 6. RouteOption — Route & cost optimization
// ─────────────────────────────────────────────

/// A possible route for shipping goods between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteOption {
    /// Unique identifier
    pub id: Uuid,
    /// Origin node ID
    pub origin_id: Uuid,
    /// Destination node ID
    pub destination_id: Uuid,
    /// Intermediate waypoints (node IDs in order)
    pub waypoints: Vec<Uuid>,
    /// Transport mode for each leg
    pub leg_modes: Vec<TransportMode>,
    /// Estimated total cost
    pub estimated_cost: f64,
    /// Estimated total transit time (hours)
    pub estimated_hours: f64,
    /// Estimated CO2 emissions (kg)
    pub estimated_co2_kg: Option<f64>,
    /// Reliability score (0.0 to 1.0, based on historical on-time rate)
    pub reliability: f64,
    /// Risk score (0.0 to 1.0, based on disruption probability along route)
    pub risk: f64,
    /// Monte Carlo probability of on-time arrival
    pub on_time_probability: f64,
}

impl RouteOption {
    /// Cost-effectiveness score (lower is better).
    /// Balances cost, time, and reliability.
    pub fn score(&self, cost_weight: f64, time_weight: f64, reliability_weight: f64) -> f64 {
        let normalized_cost = self.estimated_cost / 10_000.0; // Normalize to ~0-1 range
        let normalized_time = self.estimated_hours / 720.0;   // Normalize to ~0-1 range
        let reliability_penalty = 1.0 - self.reliability;

        cost_weight * normalized_cost
            + time_weight * normalized_time
            + reliability_weight * reliability_penalty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demand_forecast_cv() {
        let forecast = DemandForecast {
            id: Uuid::new_v4(),
            product_id: Uuid::new_v4(),
            node_id: Uuid::new_v4(),
            horizon_start: Utc::now(),
            horizon_end: Utc::now() + chrono::Duration::days(7),
            point_estimate: 1000.0,
            lower_bound: 800.0,
            upper_bound: 1200.0,
            confidence_level: 0.80,
            signal_ids: Vec::new(),
            generated_at: Utc::now(),
            method: "test".into(),
        };

        assert!((forecast.uncertainty_range() - 400.0).abs() < 0.01);
        assert!((forecast.cv() - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_inventory_reorder() {
        let mut policy = InventoryPolicy::new(Uuid::new_v4(), Uuid::new_v4(), 100);
        policy.avg_daily_demand = 20.0;
        policy.lead_time_days = 7.0;
        policy.safety_stock = 50;

        policy.check_reorder();

        // ROP = 20 * 7 + 50 = 190
        assert_eq!(policy.reorder_point, 190);
        // 100 < 190, so reorder recommended
        assert!(policy.reorder_recommended);
        // Days of supply = 100 / 20 = 5
        assert!((policy.days_of_supply - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_supplier_risk_update() {
        let mut risk = SupplierRiskScore::new(Uuid::new_v4(), 0.3);
        assert!(!risk.should_alert());

        // Spike in risk
        risk.update(0.9, 0.5);
        assert!(risk.current_risk > 0.5);
        assert!(risk.is_worsening());
    }

    #[test]
    fn test_recall_trace() {
        let mut recall = RecallTrace::new(
            Uuid::new_v4(),
            "LOT-2025-001",
            "Potential contamination",
            RecallSeverity::ClassII,
        );

        recall.add_affected_node(Uuid::new_v4(), 500);
        recall.add_affected_node(Uuid::new_v4(), 300);

        assert_eq!(recall.total_affected_units, 800);
        assert!((recall.resolution_pct() - 0.0).abs() < 0.01);

        // Resolve first node
        recall.affected_nodes[0].resolved = true;
        assert!((recall.resolution_pct() - 62.5).abs() < 0.1);
    }

    #[test]
    fn test_disruption_duration() {
        assert!(DisruptionType::Pandemic.typical_duration_hours() > 1000.0);
        assert!(DisruptionType::CyberAttack.typical_duration_hours() < 100.0);
    }

    #[test]
    fn test_route_scoring() {
        let route = RouteOption {
            id: Uuid::new_v4(),
            origin_id: Uuid::new_v4(),
            destination_id: Uuid::new_v4(),
            waypoints: Vec::new(),
            leg_modes: vec![TransportMode::Ocean],
            estimated_cost: 5000.0,
            estimated_hours: 360.0,
            estimated_co2_kg: Some(1200.0),
            reliability: 0.85,
            risk: 0.2,
            on_time_probability: 0.82,
        };

        let score = route.score(0.4, 0.3, 0.3);
        assert!(score > 0.0);
        assert!(score < 2.0);
    }
}
