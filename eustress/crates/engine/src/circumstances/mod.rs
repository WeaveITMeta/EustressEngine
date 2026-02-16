//! # Eustress Circumstances
//!
//! Forward-looking operational decision engine for supply chain,
//! logistics, business intelligence, manufacturing, healthcare, and agriculture.
//!
//! > **Think of Eustress Circumstances as something Costco would use.**
//!
//! Circumstances reuses the Scenarios probabilistic engine (Monte Carlo,
//! Bayesian updates, composable hierarchy) with domain-specific vocabulary
//! and data models. A Scenario asks "What happened?" — a Circumstance
//! asks "What will happen?" and "What should we do?"
//!
//! ## Module Structure
//!
//! - [`types`] — Domain type aliases and shared vocabulary
//! - [`supply_chain`] — Supply chain data models (nodes, products, shipments, BOM)
//! - [`forecasting`] — Demand forecasting, disruption prediction, supplier risk
//! - [`plugin`] — Bevy CircumstancesPlugin

pub mod types;
pub mod supply_chain;
pub mod forecasting;
pub mod plugin;

// Re-export core types
pub use types::{
    Circumstance, CircumstanceScale, CircumstanceStatus,
    Forecast, Signal, SignalType, DecisionPoint,
    CircumstanceParameter, CircumstanceEntity, CircumstanceOutcome,
};
pub use supply_chain::{
    SupplyChainNode, NodeType, Product, Shipment, ShipmentStatus,
    BomEntry, SupplierProfile,
};
pub use forecasting::{
    DemandForecast, DemandSignal, DisruptionType, SupplierRiskScore,
    InventoryPolicy, RecallTrace,
};
pub use plugin::CircumstancesPlugin;
