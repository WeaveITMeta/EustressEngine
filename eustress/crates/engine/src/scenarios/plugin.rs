//! # Eustress Scenarios — Bevy Plugin
//!
//! Table of Contents:
//! 1. ScenariosPlugin — Bevy plugin registration
//! 2. ScenarioStore — Bevy resource holding all active scenarios
//! 3. Scenario messages — Bevy messages for scenario lifecycle
//! 4. Bevy systems — Simulation runner, adapter poller, pruning

use std::collections::HashMap;

use bevy::prelude::*;
use uuid::Uuid;

use super::adapters::AdapterRegistry;
use super::engine::{run_simulation, SimulationConfig, SimulationResult};
use super::evidence::{
    AutoAttachConfig, AutoAttacher, EvidenceConflict, EvidenceManager,
    detect_conflicts,
};
use super::hierarchy::ScenarioGraph;
use super::pruning::{self, PruningConfig, PruningHistory, PruningResult};
use super::scripting::{ScenarioScriptEngine, ScriptEngineConfig, ScriptResult};
use super::types::{EvidencePolarity, Scenario, ScenarioStatus};

// ─────────────────────────────────────────────
// 1. ScenariosPlugin
// ─────────────────────────────────────────────

/// Bevy plugin that registers all Scenarios resources, events, and systems.
pub struct ScenariosPlugin;

impl Plugin for ScenariosPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<ScenarioStore>()
            .init_resource::<ScenarioGraph>()
            .init_resource::<ScenarioScriptEngineRes>()
            .init_resource::<ScenarioPruningHistories>()
            // Phase 0 Messages
            .add_message::<CreateScenarioEvent>()
            .add_message::<RunSimulationEvent>()
            .add_message::<SimulationCompleteEvent>()
            .add_message::<AddEvidenceEvent>()
            .add_message::<AddBranchEvent>()
            .add_message::<PruneEvent>()
            .add_message::<ScenarioStatusChanged>()
            // Phase 0.5 Messages — Evidence
            .add_message::<AttachEvidenceEvent>()
            .add_message::<DetachEvidenceEvent>()
            .add_message::<ReweightEvidenceEvent>()
            .add_message::<DetectConflictsEvent>()
            .add_message::<ConflictsDetectedEvent>()
            .add_message::<AutoAttachEvent>()
            // Phase 0.5 Messages — Pruning
            .add_message::<AdvancedPruneEvent>()
            .add_message::<RestoreBranchesEvent>()
            .add_message::<PruneCompleteEvent>()
            // Phase 0.5 Messages — Scripting
            .add_message::<RunBranchScriptEvent>()
            .add_message::<ScriptCompleteEvent>()
            // Phase 0 Systems
            .add_systems(Update, (
                handle_create_scenario,
                handle_run_simulation,
                handle_add_evidence,
                handle_add_branch,
                handle_prune,
            ))
            // Phase 0.5 Systems
            .add_systems(Update, (
                handle_attach_evidence,
                handle_detach_evidence,
                handle_reweight_evidence,
                handle_detect_conflicts,
                handle_auto_attach,
                handle_advanced_prune,
                handle_restore_branches,
                handle_run_branch_script,
            ));
    }
}

// ─────────────────────────────────────────────
// 2. ScenarioStore — Bevy resource
// ─────────────────────────────────────────────

/// Bevy resource holding all active scenarios and their adapter registries.
#[derive(Resource, Default)]
pub struct ScenarioStore {
    /// Active scenarios indexed by ID
    pub scenarios: HashMap<Uuid, Scenario>,
    /// Per-scenario adapter registries
    pub adapters: HashMap<Uuid, AdapterRegistry>,
    /// Most recent simulation results per scenario
    pub results: HashMap<Uuid, SimulationResult>,
    /// Default simulation config
    pub default_config: SimulationConfig,
}

impl ScenarioStore {
    /// Get a scenario by ID.
    pub fn get(&self, id: Uuid) -> Option<&Scenario> {
        self.scenarios.get(&id)
    }

    /// Get a mutable scenario by ID.
    pub fn get_mut(&mut self, id: Uuid) -> Option<&mut Scenario> {
        self.scenarios.get_mut(&id)
    }

    /// Insert a new scenario.
    pub fn insert(&mut self, scenario: Scenario) -> Uuid {
        let id = scenario.id;
        self.scenarios.insert(id, scenario);
        self.adapters.insert(id, AdapterRegistry::new());
        id
    }

    /// Remove a scenario.
    pub fn remove(&mut self, id: Uuid) -> Option<Scenario> {
        self.adapters.remove(&id);
        self.results.remove(&id);
        self.scenarios.remove(&id)
    }

    /// List all scenario IDs.
    pub fn ids(&self) -> Vec<Uuid> {
        self.scenarios.keys().copied().collect()
    }

    /// Count active scenarios.
    pub fn count(&self) -> usize {
        self.scenarios.len()
    }
}


// ─────────────────────────────────────────────
// 3. Scenario messages
// ─────────────────────────────────────────────

/// Message: request to create a new scenario.
#[derive(Message, Debug, Clone)]
pub struct CreateScenarioEvent {
    /// Name for the new scenario
    pub name: String,
    /// Scale (micro or macro)
    pub scale: super::types::ScenarioScale,
    /// Optional initial root hypothesis label
    pub root_label: Option<String>,
}

/// Message: request to run a Monte Carlo simulation on a scenario.
#[derive(Message, Debug, Clone)]
pub struct RunSimulationEvent {
    /// Target scenario ID
    pub scenario_id: Uuid,
    /// Simulation configuration (None = use default)
    pub config: Option<SimulationConfig>,
}

/// Message: simulation completed.
#[derive(Message, Debug, Clone)]
pub struct SimulationCompleteEvent {
    /// Scenario ID
    pub scenario_id: Uuid,
    /// Simulation result
    pub result: SimulationResult,
}

/// Message: add evidence to a scenario.
#[derive(Message, Debug, Clone)]
pub struct AddEvidenceEvent {
    /// Target scenario ID
    pub scenario_id: Uuid,
    /// Evidence to add
    pub evidence: super::types::Evidence,
}

/// Message: add a branch to a scenario.
#[derive(Message, Debug, Clone)]
pub struct AddBranchEvent {
    /// Target scenario ID
    pub scenario_id: Uuid,
    /// Parent branch ID
    pub parent_id: Uuid,
    /// Label for the new branch
    pub label: String,
    /// Prior probability
    pub prior: f64,
}

/// Message: trigger soft-pruning on a scenario.
#[derive(Message, Debug, Clone)]
pub struct PruneEvent {
    /// Target scenario ID
    pub scenario_id: Uuid,
    /// Optional override threshold (None = use scenario's default)
    pub threshold: Option<f64>,
}

/// Message: scenario status changed.
#[derive(Message, Debug, Clone)]
pub struct ScenarioStatusChanged {
    /// Scenario ID
    pub scenario_id: Uuid,
    /// Previous status
    pub old_status: ScenarioStatus,
    /// New status
    pub new_status: ScenarioStatus,
}

// ─────────────────────────────────────────────
// 4. Bevy systems
// ─────────────────────────────────────────────

/// System: handle CreateScenarioEvent — creates a new scenario in the store.
fn handle_create_scenario(
    mut events: MessageReader<CreateScenarioEvent>,
    mut store: ResMut<ScenarioStore>,
    mut status_events: MessageWriter<ScenarioStatusChanged>,
) {
    for event in events.read() {
        let mut scenario = Scenario::new(&event.name, event.scale);
        if let Some(ref label) = event.root_label {
            scenario.set_root_branch(label, 1.0);
            scenario.status = ScenarioStatus::Active;
        }
        let id = scenario.id;
        let status = scenario.status;
        store.insert(scenario);

        status_events.write(ScenarioStatusChanged {
            scenario_id: id,
            old_status: ScenarioStatus::Initializing,
            new_status: status,
        });
    }
}

/// System: handle RunSimulationEvent — runs Monte Carlo simulation.
/// Uses rayon internally for parallel sampling.
fn handle_run_simulation(
    mut events: MessageReader<RunSimulationEvent>,
    mut store: ResMut<ScenarioStore>,
    mut complete_events: MessageWriter<SimulationCompleteEvent>,
    mut status_events: MessageWriter<ScenarioStatusChanged>,
) {
    for event in events.read() {
        let config = event
            .config
            .clone()
            .unwrap_or_else(|| store.default_config.clone());

        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            let old_status = scenario.status;
            scenario.status = ScenarioStatus::Simulating;

            status_events.write(ScenarioStatusChanged {
                scenario_id: event.scenario_id,
                old_status,
                new_status: ScenarioStatus::Simulating,
            });

            // Run simulation (rayon parallel internally)
            let result = run_simulation(scenario, &config);

            scenario.status = ScenarioStatus::Active;

            status_events.write(ScenarioStatusChanged {
                scenario_id: event.scenario_id,
                old_status: ScenarioStatus::Simulating,
                new_status: ScenarioStatus::Active,
            });

            complete_events.write(SimulationCompleteEvent {
                scenario_id: event.scenario_id,
                result: result.clone(),
            });

            store.results.insert(event.scenario_id, result);
        }
    }
}

/// System: handle AddEvidenceEvent — adds evidence to a scenario.
fn handle_add_evidence(
    mut events: MessageReader<AddEvidenceEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            scenario.add_evidence(event.evidence.clone());
        }
    }
}

/// System: handle AddBranchEvent — adds a branch to a scenario.
fn handle_add_branch(
    mut events: MessageReader<AddBranchEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            scenario.add_branch(event.parent_id, &event.label, event.prior);
        }
    }
}

/// System: handle PruneEvent — applies soft-pruning to a scenario.
fn handle_prune(
    mut events: MessageReader<PruneEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            if let Some(threshold) = event.threshold {
                scenario.collapse_threshold = threshold;
            }
            scenario.apply_soft_pruning();
        }
    }
}

// ─────────────────────────────────────────────
// 5. Phase 0.5 — Resources
// ─────────────────────────────────────────────

/// Bevy resource wrapping the ScenarioScriptEngine.
#[derive(Resource)]
pub struct ScenarioScriptEngineRes {
    pub engine: ScenarioScriptEngine,
}

impl Default for ScenarioScriptEngineRes {
    fn default() -> Self {
        Self {
            engine: ScenarioScriptEngine::new(ScriptEngineConfig::default()),
        }
    }
}

/// Bevy resource holding pruning histories per scenario.
#[derive(Resource, Default)]
pub struct ScenarioPruningHistories {
    pub histories: HashMap<Uuid, PruningHistory>,
}

// ─────────────────────────────────────────────
// 6. Phase 0.5 — Evidence Messages
// ─────────────────────────────────────────────

/// Message: attach evidence to a branch.
#[derive(Message, Debug, Clone)]
pub struct AttachEvidenceEvent {
    pub scenario_id: Uuid,
    pub evidence_id: Uuid,
    pub branch_id: Uuid,
    pub polarity: EvidencePolarity,
    pub weight: f64,
    pub analyst_id: Option<Uuid>,
}

/// Message: detach evidence from a branch.
#[derive(Message, Debug, Clone)]
pub struct DetachEvidenceEvent {
    pub scenario_id: Uuid,
    pub evidence_id: Uuid,
    pub branch_id: Uuid,
}

/// Message: update evidence-branch link weight.
#[derive(Message, Debug, Clone)]
pub struct ReweightEvidenceEvent {
    pub scenario_id: Uuid,
    pub evidence_id: Uuid,
    pub branch_id: Uuid,
    pub new_weight: f64,
}

/// Message: request conflict detection on a scenario.
#[derive(Message, Debug, Clone)]
pub struct DetectConflictsEvent {
    pub scenario_id: Uuid,
}

/// Message: conflicts detected (response).
#[derive(Message, Debug, Clone)]
pub struct ConflictsDetectedEvent {
    pub scenario_id: Uuid,
    pub conflicts: Vec<EvidenceConflict>,
}

/// Message: run auto-attach on unlinked evidence.
#[derive(Message, Debug, Clone)]
pub struct AutoAttachEvent {
    pub scenario_id: Uuid,
    pub config: Option<AutoAttachConfig>,
    pub auto_confirm: bool,
}

// ─────────────────────────────────────────────
// 7. Phase 0.5 — Pruning Messages
// ─────────────────────────────────────────────

/// Message: advanced pruning with full PruningConfig.
#[derive(Message, Debug, Clone)]
pub struct AdvancedPruneEvent {
    pub scenario_id: Uuid,
    pub config: PruningConfig,
}

/// Message: restore collapsed branches.
#[derive(Message, Debug, Clone)]
pub struct RestoreBranchesEvent {
    pub scenario_id: Uuid,
    /// Branch IDs to restore (empty = restore all).
    pub branch_ids: Vec<Uuid>,
}

/// Message: pruning pass completed (response).
#[derive(Message, Debug, Clone)]
pub struct PruneCompleteEvent {
    pub scenario_id: Uuid,
    pub result: PruningResult,
}

// ─────────────────────────────────────────────
// 8. Phase 0.5 — Scripting Messages
// ─────────────────────────────────────────────

/// Message: execute a Rune script on a branch.
#[derive(Message, Debug, Clone)]
pub struct RunBranchScriptEvent {
    pub scenario_id: Uuid,
    pub branch_id: Uuid,
    pub source: String,
}

/// Message: script execution completed (response).
#[derive(Message, Debug, Clone)]
pub struct ScriptCompleteEvent {
    pub scenario_id: Uuid,
    pub branch_id: Uuid,
    pub result: ScriptResult,
    pub error: Option<String>,
}

// ─────────────────────────────────────────────
// 9. Phase 0.5 — Evidence Systems
// ─────────────────────────────────────────────

/// System: handle AttachEvidenceEvent — link evidence to a branch.
fn handle_attach_evidence(
    mut events: MessageReader<AttachEvidenceEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            EvidenceManager::attach(
                scenario,
                event.evidence_id,
                event.branch_id,
                event.polarity,
                event.weight,
                event.analyst_id,
            );
        }
    }
}

/// System: handle DetachEvidenceEvent — unlink evidence from a branch.
fn handle_detach_evidence(
    mut events: MessageReader<DetachEvidenceEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            EvidenceManager::detach(scenario, event.evidence_id, event.branch_id);
        }
    }
}

/// System: handle ReweightEvidenceEvent — update link weight.
fn handle_reweight_evidence(
    mut events: MessageReader<ReweightEvidenceEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            EvidenceManager::reweight(
                scenario,
                event.evidence_id,
                event.branch_id,
                event.new_weight,
            );
        }
    }
}

/// System: handle DetectConflictsEvent — find evidence conflicts.
fn handle_detect_conflicts(
    mut events: MessageReader<DetectConflictsEvent>,
    store: Res<ScenarioStore>,
    mut conflict_events: MessageWriter<ConflictsDetectedEvent>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get(&event.scenario_id) {
            let conflicts = detect_conflicts(scenario);
            conflict_events.write(ConflictsDetectedEvent {
                scenario_id: event.scenario_id,
                conflicts,
            });
        }
    }
}

/// System: handle AutoAttachEvent — auto-attach unlinked evidence.
fn handle_auto_attach(
    mut events: MessageReader<AutoAttachEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            let config = event.config.clone().unwrap_or_default();
            let attacher = AutoAttacher::new(config);
            let suggestions = attacher.suggest_all_unlinked(scenario);
            AutoAttacher::apply_suggestions(scenario, &suggestions, event.auto_confirm);
        }
    }
}

// ─────────────────────────────────────────────
// 10. Phase 0.5 — Pruning Systems
// ─────────────────────────────────────────────

/// System: handle AdvancedPruneEvent — pruning with full config and history.
fn handle_advanced_prune(
    mut events: MessageReader<AdvancedPruneEvent>,
    mut store: ResMut<ScenarioStore>,
    mut histories: ResMut<ScenarioPruningHistories>,
    mut complete_events: MessageWriter<PruneCompleteEvent>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            let result = pruning::apply_soft_pruning(scenario, &event.config);

            // Record in history
            histories
                .histories
                .entry(event.scenario_id)
                .or_insert_with(PruningHistory::new)
                .record(result.clone());

            complete_events.write(PruneCompleteEvent {
                scenario_id: event.scenario_id,
                result,
            });
        }
    }
}

/// System: handle RestoreBranchesEvent — restore collapsed branches.
fn handle_restore_branches(
    mut events: MessageReader<RestoreBranchesEvent>,
    mut store: ResMut<ScenarioStore>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            if event.branch_ids.is_empty() {
                pruning::restore_all(scenario);
            } else {
                pruning::restore_branches(scenario, &event.branch_ids);
            }
        }
    }
}

// ─────────────────────────────────────────────
// 11. Phase 0.5 — Scripting Systems
// ─────────────────────────────────────────────

/// System: handle RunBranchScriptEvent — execute Rune script on a branch.
fn handle_run_branch_script(
    mut events: MessageReader<RunBranchScriptEvent>,
    mut store: ResMut<ScenarioStore>,
    script_engine: Res<ScenarioScriptEngineRes>,
    mut complete_events: MessageWriter<ScriptCompleteEvent>,
) {
    for event in events.read() {
        if let Some(scenario) = store.scenarios.get_mut(&event.scenario_id) {
            match script_engine.engine.execute_and_apply(
                &event.source,
                scenario,
                event.branch_id,
            ) {
                Ok(result) => {
                    complete_events.write(ScriptCompleteEvent {
                        scenario_id: event.scenario_id,
                        branch_id: event.branch_id,
                        result,
                        error: None,
                    });
                }
                Err(err) => {
                    complete_events.write(ScriptCompleteEvent {
                        scenario_id: event.scenario_id,
                        branch_id: event.branch_id,
                        result: ScriptResult::default(),
                        error: Some(err.to_string()),
                    });
                }
            }
        }
    }
}
