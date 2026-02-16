//! # Eustress Circumstances — Supply Chain Data Models
//!
//! Table of Contents:
//! 1. SupplyChainNode / NodeType — Nodes in the supply chain network
//! 2. Product — SKU/item with attributes and provenance
//! 3. Shipment / ShipmentStatus — Goods in transit
//! 4. BomEntry — Bill of materials entry
//! 5. SupplierProfile — Supplier risk and performance profile
//! 6. ProvenanceRecord — Chain of custody for anti-counterfeit

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::scenarios::types::{GeoPoint, ParameterValue};

// ─────────────────────────────────────────────
// 1. SupplyChainNode / NodeType
// ─────────────────────────────────────────────

/// A node in the supply chain network — a facility, organization,
/// or logical point where goods are produced, stored, or transferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyChainNode {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable name (e.g., "Shenzhen Factory #3", "Costco Warehouse LA")
    pub name: String,
    /// Node type
    pub node_type: NodeType,
    /// Geographic location
    pub location: Option<GeoPoint>,
    /// Organization that operates this node
    pub operator: Option<String>,
    /// Current operational status
    pub operational: bool,
    /// Capacity (units per day, or other domain-specific measure)
    pub capacity: Option<f64>,
    /// Current utilization (0.0 to 1.0)
    pub utilization: Option<f64>,
    /// Lead time from this node to downstream (in hours)
    pub lead_time_hours: Option<f64>,
    /// Upstream node IDs (suppliers feeding into this node)
    pub upstream_ids: Vec<Uuid>,
    /// Downstream node IDs (customers/destinations this node feeds)
    pub downstream_ids: Vec<Uuid>,
    /// Flexible attributes
    pub attributes: HashMap<String, ParameterValue>,
    /// When this node was last updated
    pub updated_at: DateTime<Utc>,
}

/// Type of supply chain node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    /// Raw material source (mine, farm, refinery)
    RawMaterial,
    /// Manufacturing facility
    Manufacturer,
    /// Assembly plant
    Assembler,
    /// Distribution center / warehouse
    DistributionCenter,
    /// Wholesale distributor
    Wholesaler,
    /// Retail store (e.g., Costco warehouse store)
    Retailer,
    /// E-commerce fulfillment center
    Fulfillment,
    /// Port / airport / rail terminal
    TransportHub,
    /// Cross-docking facility
    CrossDock,
    /// Cold storage facility
    ColdStorage,
    /// End consumer (for last-mile modeling)
    Consumer,
}

impl SupplyChainNode {
    /// Create a new supply chain node.
    pub fn new(name: impl Into<String>, node_type: NodeType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            node_type,
            location: None,
            operator: None,
            operational: true,
            capacity: None,
            utilization: None,
            lead_time_hours: None,
            upstream_ids: Vec::new(),
            downstream_ids: Vec::new(),
            attributes: HashMap::new(),
            updated_at: Utc::now(),
        }
    }

    /// Set geographic location.
    pub fn with_location(mut self, point: GeoPoint) -> Self {
        self.location = Some(point);
        self
    }

    /// Set operator name.
    pub fn with_operator(mut self, operator: impl Into<String>) -> Self {
        self.operator = Some(operator.into());
        self
    }

    /// Set capacity.
    pub fn with_capacity(mut self, capacity: f64) -> Self {
        self.capacity = Some(capacity);
        self
    }

    /// Whether this node is at risk (utilization > 90% or not operational).
    pub fn is_at_risk(&self) -> bool {
        !self.operational || self.utilization.map_or(false, |u| u > 0.9)
    }
}

// ─────────────────────────────────────────────
// 2. Product
// ─────────────────────────────────────────────

/// A product/SKU tracked through the supply chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    /// Unique identifier
    pub id: Uuid,
    /// SKU (Stock Keeping Unit)
    pub sku: String,
    /// UPC/EAN barcode
    pub upc: Option<String>,
    /// Human-readable product name
    pub name: String,
    /// Product category
    pub category: Option<String>,
    /// Unit cost (wholesale)
    pub unit_cost: Option<f64>,
    /// Unit price (retail)
    pub unit_price: Option<f64>,
    /// Weight per unit (kg)
    pub weight_kg: Option<f64>,
    /// Whether this product requires cold chain
    pub cold_chain: bool,
    /// Shelf life in days (None = non-perishable)
    pub shelf_life_days: Option<u32>,
    /// Bill of materials (components needed to produce this product)
    pub bom: Vec<BomEntry>,
    /// Lot/batch number (for recall tracing)
    pub lot_number: Option<String>,
    /// Serial number (for individual unit tracking)
    pub serial_number: Option<String>,
    /// Provenance chain (custody records)
    pub provenance: Vec<ProvenanceRecord>,
    /// Flexible attributes
    pub attributes: HashMap<String, ParameterValue>,
}

impl Product {
    /// Create a new product.
    pub fn new(sku: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            sku: sku.into(),
            upc: None,
            name: name.into(),
            category: None,
            unit_cost: None,
            unit_price: None,
            weight_kg: None,
            cold_chain: false,
            shelf_life_days: None,
            bom: Vec::new(),
            lot_number: None,
            serial_number: None,
            provenance: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    /// Gross margin per unit.
    pub fn gross_margin(&self) -> Option<f64> {
        match (self.unit_price, self.unit_cost) {
            (Some(price), Some(cost)) => Some(price - cost),
            _ => None,
        }
    }

    /// Gross margin percentage.
    pub fn margin_pct(&self) -> Option<f64> {
        match (self.unit_price, self.unit_cost) {
            (Some(price), Some(cost)) if price > 0.0 => Some((price - cost) / price * 100.0),
            _ => None,
        }
    }

    /// Whether this product is perishable.
    pub fn is_perishable(&self) -> bool {
        self.shelf_life_days.is_some()
    }
}

// ─────────────────────────────────────────────
// 3. Shipment / ShipmentStatus
// ─────────────────────────────────────────────

/// A shipment of goods moving between supply chain nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shipment {
    /// Unique identifier
    pub id: Uuid,
    /// Tracking number
    pub tracking_number: Option<String>,
    /// Origin node ID
    pub origin_id: Uuid,
    /// Destination node ID
    pub destination_id: Uuid,
    /// Products in this shipment (product_id → quantity)
    pub contents: HashMap<Uuid, u64>,
    /// Current status
    pub status: ShipmentStatus,
    /// Departure time (actual or scheduled)
    pub departed_at: Option<DateTime<Utc>>,
    /// Expected arrival time
    pub eta: Option<DateTime<Utc>>,
    /// Actual arrival time
    pub arrived_at: Option<DateTime<Utc>>,
    /// Current location (if in transit)
    pub current_location: Option<GeoPoint>,
    /// Transport mode
    pub transport_mode: TransportMode,
    /// Carrier name
    pub carrier: Option<String>,
    /// Total weight (kg)
    pub total_weight_kg: Option<f64>,
    /// Shipping cost
    pub cost: Option<f64>,
    /// Temperature readings (for cold chain)
    pub temperature_log: Vec<(DateTime<Utc>, f64)>,
}

/// Status of a shipment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShipmentStatus {
    /// Shipment is being prepared
    Preparing,
    /// Shipment has departed origin
    InTransit,
    /// Shipment is at a transfer point
    AtHub,
    /// Shipment is out for delivery
    OutForDelivery,
    /// Shipment has been delivered
    Delivered,
    /// Shipment is delayed
    Delayed,
    /// Shipment is lost or missing
    Lost,
    /// Shipment was returned
    Returned,
    /// Shipment was cancelled
    Cancelled,
}

/// Mode of transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportMode {
    /// Ocean freight (container ship)
    Ocean,
    /// Air freight
    Air,
    /// Ground / truck
    Ground,
    /// Rail
    Rail,
    /// Intermodal (multiple modes)
    Intermodal,
    /// Last-mile delivery (van, bike, drone)
    LastMile,
}

impl Shipment {
    /// Create a new shipment.
    pub fn new(origin_id: Uuid, destination_id: Uuid, transport_mode: TransportMode) -> Self {
        Self {
            id: Uuid::new_v4(),
            tracking_number: None,
            origin_id,
            destination_id,
            contents: HashMap::new(),
            status: ShipmentStatus::Preparing,
            departed_at: None,
            eta: None,
            arrived_at: None,
            current_location: None,
            transport_mode,
            carrier: None,
            total_weight_kg: None,
            cost: None,
            temperature_log: Vec::new(),
        }
    }

    /// Add product to shipment.
    pub fn add_product(&mut self, product_id: Uuid, quantity: u64) {
        *self.contents.entry(product_id).or_insert(0) += quantity;
    }

    /// Whether the shipment is late (past ETA and not delivered).
    pub fn is_late(&self) -> bool {
        if self.status == ShipmentStatus::Delivered {
            return false;
        }
        self.eta.map_or(false, |eta| Utc::now() > eta)
    }

    /// Whether any temperature reading exceeded a threshold.
    pub fn has_temperature_excursion(&self, max_temp: f64) -> bool {
        self.temperature_log.iter().any(|(_, temp)| *temp > max_temp)
    }
}

// ─────────────────────────────────────────────
// 4. BomEntry — Bill of Materials
// ─────────────────────────────────────────────

/// A single entry in a bill of materials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BomEntry {
    /// Component product ID
    pub component_id: Uuid,
    /// Quantity needed per unit of parent product
    pub quantity_per_unit: f64,
    /// Whether this component is critical (no substitutes)
    pub critical: bool,
    /// Alternative component IDs (substitutes)
    pub alternatives: Vec<Uuid>,
    /// Lead time to procure this component (hours)
    pub lead_time_hours: Option<f64>,
}

// ─────────────────────────────────────────────
// 5. SupplierProfile
// ─────────────────────────────────────────────

/// Performance and risk profile for a supplier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierProfile {
    /// Supplier node ID
    pub node_id: Uuid,
    /// On-time delivery rate (0.0 to 1.0)
    pub on_time_rate: f64,
    /// Quality pass rate (0.0 to 1.0)
    pub quality_rate: f64,
    /// Average lead time (hours)
    pub avg_lead_time_hours: f64,
    /// Lead time variability (standard deviation in hours)
    pub lead_time_std_hours: f64,
    /// Financial stability score (0.0 to 1.0)
    pub financial_stability: f64,
    /// Geographic risk score (0.0 = safe, 1.0 = high risk)
    pub geo_risk: f64,
    /// Number of past disruptions
    pub disruption_count: u32,
    /// Composite risk score (computed)
    pub composite_risk: f64,
    /// Last assessment date
    pub assessed_at: DateTime<Utc>,
}

impl SupplierProfile {
    /// Compute composite risk score from individual factors.
    pub fn compute_composite_risk(&mut self) {
        // Weighted average of risk factors
        let delivery_risk = 1.0 - self.on_time_rate;
        let quality_risk = 1.0 - self.quality_rate;
        let stability_risk = 1.0 - self.financial_stability;
        let disruption_risk = (self.disruption_count as f64 / 10.0).min(1.0);

        self.composite_risk = 0.25 * delivery_risk
            + 0.20 * quality_risk
            + 0.20 * stability_risk
            + 0.20 * self.geo_risk
            + 0.15 * disruption_risk;

        self.composite_risk = self.composite_risk.clamp(0.0, 1.0);
    }

    /// Whether this supplier is high-risk (composite > 0.6).
    pub fn is_high_risk(&self) -> bool {
        self.composite_risk > 0.6
    }
}

// ─────────────────────────────────────────────
// 6. ProvenanceRecord — Chain of custody
// ─────────────────────────────────────────────

/// A single record in a product's chain of custody.
/// Used for anti-counterfeit verification and recall tracing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    /// Unique record ID
    pub id: Uuid,
    /// Node that handled the product
    pub node_id: Uuid,
    /// Action performed
    pub action: ProvenanceAction,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Location
    pub location: Option<GeoPoint>,
    /// Blake3 hash of this record + previous record (chain integrity)
    pub hash: String,
    /// Previous record's hash (None for first record)
    pub prev_hash: Option<String>,
    /// Digital signature (optional, for high-security chains)
    pub signature: Option<String>,
}

/// Actions in a provenance chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProvenanceAction {
    /// Product was manufactured/produced
    Produced,
    /// Product was inspected/tested
    Inspected,
    /// Product was packaged
    Packaged,
    /// Product was shipped
    Shipped,
    /// Product was received at a node
    Received,
    /// Product was stored
    Stored,
    /// Product was sold to end customer
    Sold,
    /// Product was recalled
    Recalled,
    /// Product was destroyed/disposed
    Destroyed,
    /// Custom action
    Custom(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_product_margin() {
        let mut product = Product::new("KS-BATT-AA48", "Kirkland AA Batteries 48-pack");
        product.unit_cost = Some(8.50);
        product.unit_price = Some(14.99);

        assert!((product.gross_margin().unwrap() - 6.49).abs() < 0.01);
        assert!((product.margin_pct().unwrap() - 43.3).abs() < 0.1);
    }

    #[test]
    fn test_supplier_risk() {
        let mut profile = SupplierProfile {
            node_id: Uuid::new_v4(),
            on_time_rate: 0.85,
            quality_rate: 0.95,
            avg_lead_time_hours: 336.0, // 14 days
            lead_time_std_hours: 48.0,
            financial_stability: 0.7,
            geo_risk: 0.4,
            disruption_count: 2,
            composite_risk: 0.0,
            assessed_at: Utc::now(),
        };

        profile.compute_composite_risk();
        assert!(profile.composite_risk > 0.0);
        assert!(profile.composite_risk < 1.0);
        assert!(!profile.is_high_risk()); // Should be moderate risk
    }

    #[test]
    fn test_shipment_late() {
        let mut shipment = Shipment::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            TransportMode::Ocean,
        );
        shipment.status = ShipmentStatus::InTransit;
        shipment.eta = Some(Utc::now() - chrono::Duration::hours(24));

        assert!(shipment.is_late());
    }

    #[test]
    fn test_node_at_risk() {
        let mut node = SupplyChainNode::new("Test Warehouse", NodeType::DistributionCenter);
        assert!(!node.is_at_risk());

        node.utilization = Some(0.95);
        assert!(node.is_at_risk());

        node.utilization = Some(0.5);
        node.operational = false;
        assert!(node.is_at_risk());
    }
}
