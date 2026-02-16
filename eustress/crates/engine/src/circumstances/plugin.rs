//! # Eustress Circumstances — Bevy Plugin
//!
//! Table of Contents:
//! 1. CircumstancesPlugin — Bevy plugin registration
//! 2. CircumstanceStore — Bevy resource holding active circumstances
//! 3. Circumstance messages — Bevy messages for circumstance lifecycle
//! 4. Bevy systems — Forecast runner, signal processor, inventory checker

use std::collections::HashMap;

use bevy::prelude::*;
use uuid::Uuid;

use crate::scenarios::engine::{run_simulation, SimulationConfig, SimulationResult};
use crate::scenarios::types::ScenarioStatus;
use super::forecasting::{
    DemandForecast, InventoryPolicy, RecallTrace, SupplierRiskScore,
};
use super::supply_chain::{Product, Shipment, SupplyChainNode};
use super::types::{Circumstance, CircumstanceScale, Signal};

// ─────────────────────────────────────────────
// 1. CircumstancesPlugin
// ─────────────────────────────────────────────

/// Bevy plugin that registers all Circumstances resources, events, and systems.
/// Depends on ScenariosPlugin for the shared engine.
pub struct CircumstancesPlugin;

impl Plugin for CircumstancesPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<CircumstanceStore>()
            .init_resource::<SupplyChainState>()
            // Messages
            .add_message::<CreateCircumstanceEvent>()
            .add_message::<IngestSignalEvent>()
            .add_message::<RunForecastEvent>()
            .add_message::<ForecastCompleteEvent>()
            .add_message::<ReorderAlertEvent>()
            .add_message::<DisruptionAlertEvent>()
            .add_message::<RecallInitiatedEvent>()
            // Systems
            .add_systems(Update, (
                handle_create_circumstance,
                handle_ingest_signal,
                handle_run_forecast,
                check_inventory_levels,
                check_supplier_risks,
            ));
    }
}

// ─────────────────────────────────────────────
// 2. CircumstanceStore — Bevy resource
// ─────────────────────────────────────────────

/// Bevy resource holding all active circumstances.
#[derive(Resource, Default)]
pub struct CircumstanceStore {
    /// Active circumstances indexed by ID
    pub circumstances: HashMap<Uuid, Circumstance>,
    /// Most recent forecast results per circumstance
    pub results: HashMap<Uuid, SimulationResult>,
    /// Default simulation config for forecasting
    pub default_config: SimulationConfig,
}

impl CircumstanceStore {
    /// Get a circumstance by ID.
    pub fn get(&self, id: Uuid) -> Option<&Circumstance> {
        self.circumstances.get(&id)
    }

    /// Get a mutable circumstance by ID.
    pub fn get_mut(&mut self, id: Uuid) -> Option<&mut Circumstance> {
        self.circumstances.get_mut(&id)
    }

    /// Insert a new circumstance.
    pub fn insert(&mut self, circumstance: Circumstance) -> Uuid {
        let id = circumstance.id;
        self.circumstances.insert(id, circumstance);
        id
    }

    /// Remove a circumstance.
    pub fn remove(&mut self, id: Uuid) -> Option<Circumstance> {
        self.results.remove(&id);
        self.circumstances.remove(&id)
    }

    /// Count active circumstances.
    pub fn count(&self) -> usize {
        self.circumstances.len()
    }
}

/// Bevy resource holding supply chain state shared across circumstances.
#[derive(Resource, Default)]
pub struct SupplyChainState {
    /// Supply chain nodes
    pub nodes: HashMap<Uuid, SupplyChainNode>,
    /// Products
    pub products: HashMap<Uuid, Product>,
    /// Active shipments
    pub shipments: HashMap<Uuid, Shipment>,
    /// Inventory policies per (product, node)
    pub inventory: HashMap<(Uuid, Uuid), InventoryPolicy>,
    /// Supplier risk scores
    pub supplier_risks: HashMap<Uuid, SupplierRiskScore>,
    /// Active demand forecasts
    pub forecasts: HashMap<Uuid, DemandForecast>,
    /// Active recalls
    pub recalls: HashMap<Uuid, RecallTrace>,
    /// Pending signals to process
    pub pending_signals: Vec<Signal>,
}

// ─────────────────────────────────────────────
// 3. Circumstance messages
// ─────────────────────────────────────────────

/// Message: create a new circumstance.
#[derive(Message, Debug, Clone)]
pub struct CreateCircumstanceEvent {
    /// Name for the circumstance
    pub name: String,
    /// Scale (micro or macro)
    pub scale: CircumstanceScale,
    /// Optional root forecast label
    pub root_label: Option<String>,
}

/// Message: ingest a new signal into the system.
#[derive(Message, Debug, Clone)]
pub struct IngestSignalEvent {
    /// The signal to ingest
    pub signal: Signal,
    /// Target circumstance ID (None = broadcast to all)
    pub circumstance_id: Option<Uuid>,
}

/// Message: run a forecast (Monte Carlo simulation) on a circumstance.
#[derive(Message, Debug, Clone)]
pub struct RunForecastEvent {
    /// Target circumstance ID
    pub circumstance_id: Uuid,
    /// Simulation config (None = use default)
    pub config: Option<SimulationConfig>,
}

/// Message: forecast completed.
#[derive(Message, Debug, Clone)]
pub struct ForecastCompleteEvent {
    /// Circumstance ID
    pub circumstance_id: Uuid,
    /// Simulation result
    pub result: SimulationResult,
}

/// Message: inventory reorder alert.
#[derive(Message, Debug, Clone)]
pub struct ReorderAlertEvent {
    /// Product ID
    pub product_id: Uuid,
    /// Node ID
    pub node_id: Uuid,
    /// Current inventory level
    pub current_level: u64,
    /// Reorder point
    pub reorder_point: u64,
    /// Days of supply remaining
    pub days_of_supply: f64,
    /// Whether this is critical (below safety stock)
    pub critical: bool,
}

/// Message: supply chain disruption alert.
#[derive(Message, Debug, Clone)]
pub struct DisruptionAlertEvent {
    /// Supplier node ID
    pub node_id: Uuid,
    /// Current risk score
    pub risk_score: f64,
    /// Risk trend
    pub trend: f64,
    /// Active disruption types
    pub disruptions: Vec<super::forecasting::DisruptionType>,
}

/// Message: recall initiated.
#[derive(Message, Debug, Clone)]
pub struct RecallInitiatedEvent {
    /// Recall trace
    pub recall: RecallTrace,
}

// ─────────────────────────────────────────────
// 4. Bevy systems
// ─────────────────────────────────────────────

/// System: handle CreateCircumstanceEvent.
fn handle_create_circumstance(
    mut events: MessageReader<CreateCircumstanceEvent>,
    mut store: ResMut<CircumstanceStore>,
) {
    for event in events.read() {
        let mut circ = Circumstance::new(&event.name, event.scale);
        if let Some(ref label) = event.root_label {
            circ.set_root_branch(label, 1.0);
            circ.status = ScenarioStatus::Active;
        }
        store.insert(circ);
    }
}

/// System: handle IngestSignalEvent — convert signals to evidence and attach.
fn handle_ingest_signal(
    mut events: MessageReader<IngestSignalEvent>,
    mut store: ResMut<CircumstanceStore>,
    mut sc_state: ResMut<SupplyChainState>,
) {
    for event in events.read() {
        // Convert signal to evidence for the shared engine
        let evidence = event.signal.to_evidence();

        if let Some(circ_id) = event.circumstance_id {
            // Targeted: add to specific circumstance
            if let Some(circ) = store.circumstances.get_mut(&circ_id) {
                circ.add_evidence(evidence);
            }
        } else {
            // Broadcast: add to all active circumstances
            for circ in store.circumstances.values_mut() {
                if circ.status == ScenarioStatus::Active {
                    circ.add_evidence(evidence.clone());
                }
            }
        }

        // Also store in pending signals for supply chain processing
        sc_state.pending_signals.push(event.signal.clone());
    }
}

/// System: handle RunForecastEvent — run Monte Carlo forecast.
fn handle_run_forecast(
    mut events: MessageReader<RunForecastEvent>,
    mut store: ResMut<CircumstanceStore>,
    mut complete_events: MessageWriter<ForecastCompleteEvent>,
) {
    for event in events.read() {
        let config = event
            .config
            .clone()
            .unwrap_or_else(|| store.default_config.clone());

        if let Some(circ) = store.circumstances.get_mut(&event.circumstance_id) {
            let result = run_simulation(circ, &config);

            complete_events.write(ForecastCompleteEvent {
                circumstance_id: event.circumstance_id,
                result: result.clone(),
            });

            store.results.insert(event.circumstance_id, result);
        }
    }
}

/// System: check inventory levels and emit reorder alerts.
fn check_inventory_levels(
    mut sc_state: ResMut<SupplyChainState>,
    mut alerts: MessageWriter<ReorderAlertEvent>,
) {
    for ((product_id, node_id), policy) in sc_state.inventory.iter_mut() {
        policy.check_reorder();

        if policy.reorder_recommended {
            alerts.write(ReorderAlertEvent {
                product_id: *product_id,
                node_id: *node_id,
                current_level: policy.current_level,
                reorder_point: policy.reorder_point,
                days_of_supply: policy.days_of_supply,
                critical: policy.is_critical(),
            });
        }
    }
}

/// System: check supplier risk scores and emit disruption alerts.
fn check_supplier_risks(
    sc_state: Res<SupplyChainState>,
    mut alerts: MessageWriter<DisruptionAlertEvent>,
) {
    for (node_id, risk) in &sc_state.supplier_risks {
        if risk.should_alert() {
            alerts.write(DisruptionAlertEvent {
                node_id: *node_id,
                risk_score: risk.current_risk,
                trend: risk.trend,
                disruptions: risk.active_disruptions.clone(),
            });
        }
    }
}
