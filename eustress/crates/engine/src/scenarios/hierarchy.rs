//! # Eustress Scenarios — Micro/Macro Composable Hierarchy
//!
//! Table of Contents:
//! 1. ScenarioGraph — DAG of scenario relationships
//! 2. CompositionLink — Link between parent macro and child micro
//! 3. DataFlow — Bidirectional data propagation between levels
//! 4. compose / decompose — Operations for building hierarchy

use std::collections::HashMap;

use bevy::prelude::Resource;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{ParameterValue, Scenario, ScenarioScale};

// ─────────────────────────────────────────────
// 1. ScenarioGraph — DAG of scenario relationships
// ─────────────────────────────────────────────

/// Directed acyclic graph of scenario composition.
/// Macro scenarios contain micro scenarios as sub-scenarios.
/// Data flows bidirectionally: macro parameters feed down,
/// micro outcomes propagate up.
#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct ScenarioGraph {
    /// All scenarios in this graph, indexed by ID
    pub scenarios: HashMap<Uuid, Scenario>,
    /// Composition links (parent → children)
    pub links: Vec<CompositionLink>,
}

impl ScenarioGraph {
    /// Create a new empty graph.
    pub fn new() -> Self {
        Self {
            scenarios: HashMap::new(),
            links: Vec::new(),
        }
    }

    /// Add a scenario to the graph.
    pub fn add_scenario(&mut self, scenario: Scenario) -> Uuid {
        let id = scenario.id;
        self.scenarios.insert(id, scenario);
        id
    }

    /// Compose a micro scenario into a macro scenario.
    /// The micro becomes a sub-scenario of the macro.
    /// Returns the link ID, or None if either scenario is missing.
    pub fn compose(
        &mut self,
        macro_id: Uuid,
        micro_id: Uuid,
        data_flows: Vec<DataFlow>,
    ) -> Option<Uuid> {
        // Validate both exist and scales are correct
        let macro_scale = self.scenarios.get(&macro_id)?.scale;
        let micro_scale = self.scenarios.get(&micro_id)?.scale;

        if macro_scale != ScenarioScale::Macro {
            return None;
        }
        if micro_scale != ScenarioScale::Micro {
            return None;
        }

        // Prevent cycles: micro must not already be an ancestor of macro
        if self.is_ancestor(micro_id, macro_id) {
            return None;
        }

        let link = CompositionLink {
            id: Uuid::new_v4(),
            macro_id,
            micro_id,
            data_flows,
            created_at: Utc::now(),
        };
        let link_id = link.id;

        // Update scenario cross-references
        if let Some(macro_s) = self.scenarios.get_mut(&macro_id) {
            if !macro_s.sub_scenario_ids.contains(&micro_id) {
                macro_s.sub_scenario_ids.push(micro_id);
            }
        }
        if let Some(micro_s) = self.scenarios.get_mut(&micro_id) {
            micro_s.parent_scenario_id = Some(macro_id);
        }

        self.links.push(link);
        Some(link_id)
    }

    /// Decompose: remove a micro from a macro.
    pub fn decompose(&mut self, macro_id: Uuid, micro_id: Uuid) -> bool {
        let before = self.links.len();
        self.links.retain(|l| !(l.macro_id == macro_id && l.micro_id == micro_id));
        let removed = self.links.len() < before;

        if removed {
            if let Some(macro_s) = self.scenarios.get_mut(&macro_id) {
                macro_s.sub_scenario_ids.retain(|&id| id != micro_id);
            }
            if let Some(micro_s) = self.scenarios.get_mut(&micro_id) {
                if micro_s.parent_scenario_id == Some(macro_id) {
                    micro_s.parent_scenario_id = None;
                }
            }
        }
        removed
    }

    /// Check if `ancestor_id` is an ancestor of `descendant_id` in the graph.
    fn is_ancestor(&self, ancestor_id: Uuid, descendant_id: Uuid) -> bool {
        // BFS upward from descendant
        let mut visited = Vec::new();
        let mut queue = vec![descendant_id];

        while let Some(current) = queue.pop() {
            if current == ancestor_id {
                return true;
            }
            if visited.contains(&current) {
                continue;
            }
            visited.push(current);

            // Find parents of current
            for link in &self.links {
                if link.micro_id == current {
                    queue.push(link.macro_id);
                }
            }
        }
        false
    }

    /// Get all direct children (micro scenarios) of a macro.
    pub fn children_of(&self, macro_id: Uuid) -> Vec<Uuid> {
        self.links
            .iter()
            .filter(|l| l.macro_id == macro_id)
            .map(|l| l.micro_id)
            .collect()
    }

    /// Get the parent macro of a micro scenario.
    pub fn parent_of(&self, micro_id: Uuid) -> Option<Uuid> {
        self.links
            .iter()
            .find(|l| l.micro_id == micro_id)
            .map(|l| l.macro_id)
    }

    /// Get all root scenarios (macros with no parent).
    pub fn roots(&self) -> Vec<Uuid> {
        self.scenarios
            .keys()
            .filter(|&&id| self.parent_of(id).is_none())
            .copied()
            .collect()
    }

    /// Propagate data flows: push macro parameters down to micros,
    /// pull micro outcomes up to macros.
    pub fn propagate_data_flows(&mut self) {
        // Collect all flows to avoid borrow conflicts
        let flows: Vec<(Uuid, Uuid, Vec<DataFlow>)> = self
            .links
            .iter()
            .map(|l| (l.macro_id, l.micro_id, l.data_flows.clone()))
            .collect();

        for (macro_id, micro_id, data_flows) in flows {
            for flow in &data_flows {
                match flow.direction {
                    FlowDirection::Down => {
                        // Copy parameter value from macro to micro
                        let value = self
                            .scenarios
                            .get(&macro_id)
                            .and_then(|s| {
                                s.parameters
                                    .iter()
                                    .find(|p| p.name == flow.source_param)
                                    .map(|p| p.value.clone())
                            });

                        if let Some(val) = value {
                            if let Some(micro_s) = self.scenarios.get_mut(&micro_id) {
                                if let Some(param) = micro_s
                                    .parameters
                                    .iter_mut()
                                    .find(|p| p.name == flow.target_param)
                                {
                                    param.value = val;
                                    param.updated_at = Utc::now();
                                }
                            }
                        }
                    }
                    FlowDirection::Up => {
                        // Copy parameter/outcome value from micro to macro
                        let value = self
                            .scenarios
                            .get(&micro_id)
                            .and_then(|s| {
                                s.parameters
                                    .iter()
                                    .find(|p| p.name == flow.source_param)
                                    .map(|p| p.value.clone())
                            });

                        if let Some(val) = value {
                            if let Some(macro_s) = self.scenarios.get_mut(&macro_id) {
                                if let Some(param) = macro_s
                                    .parameters
                                    .iter_mut()
                                    .find(|p| p.name == flow.target_param)
                                {
                                    param.value = val;
                                    param.updated_at = Utc::now();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Default for ScenarioGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────
// 2. CompositionLink
// ─────────────────────────────────────────────

/// A link between a macro (parent) and micro (child) scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionLink {
    /// Unique link identifier
    pub id: Uuid,
    /// Parent macro scenario ID
    pub macro_id: Uuid,
    /// Child micro scenario ID
    pub micro_id: Uuid,
    /// Data flow definitions between the two levels
    pub data_flows: Vec<DataFlow>,
    /// When this link was created
    pub created_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────
// 3. DataFlow — Bidirectional data propagation
// ─────────────────────────────────────────────

/// Defines how data flows between a macro and micro scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFlow {
    /// Direction of data flow
    pub direction: FlowDirection,
    /// Source parameter name (in the source scenario)
    pub source_param: String,
    /// Target parameter name (in the target scenario)
    pub target_param: String,
    /// Optional transform expression (e.g., scale, filter)
    pub transform: Option<String>,
}

/// Direction of data flow in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowDirection {
    /// Macro → Micro (parameters flow down)
    Down,
    /// Micro → Macro (outcomes flow up)
    Up,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::types::ScenarioScale;

    #[test]
    fn test_compose_decompose() {
        let mut graph = ScenarioGraph::new();

        let macro_s = Scenario::new("Strategic Analysis", ScenarioScale::Macro);
        let micro_s = Scenario::new("Single Incident", ScenarioScale::Micro);
        let macro_id = macro_s.id;
        let micro_id = micro_s.id;

        graph.add_scenario(macro_s);
        graph.add_scenario(micro_s);

        // Compose
        let link_id = graph.compose(macro_id, micro_id, vec![]);
        assert!(link_id.is_some());
        assert_eq!(graph.children_of(macro_id), vec![micro_id]);
        assert_eq!(graph.parent_of(micro_id), Some(macro_id));

        // Decompose
        assert!(graph.decompose(macro_id, micro_id));
        assert!(graph.children_of(macro_id).is_empty());
        assert_eq!(graph.parent_of(micro_id), None);
    }

    #[test]
    fn test_prevents_wrong_scale() {
        let mut graph = ScenarioGraph::new();

        let micro1 = Scenario::new("Micro 1", ScenarioScale::Micro);
        let micro2 = Scenario::new("Micro 2", ScenarioScale::Micro);
        let id1 = micro1.id;
        let id2 = micro2.id;

        graph.add_scenario(micro1);
        graph.add_scenario(micro2);

        // Can't compose micro into micro
        assert!(graph.compose(id1, id2, vec![]).is_none());
    }

    #[test]
    fn test_prevents_cycles() {
        let mut graph = ScenarioGraph::new();

        let macro1 = Scenario::new("Macro 1", ScenarioScale::Macro);
        let macro2 = Scenario::new("Macro 2", ScenarioScale::Macro);
        let micro = Scenario::new("Micro", ScenarioScale::Micro);
        let m1_id = macro1.id;
        let m2_id = macro2.id;
        let mi_id = micro.id;

        graph.add_scenario(macro1);
        graph.add_scenario(macro2);
        graph.add_scenario(micro);

        // macro1 → micro (ok)
        assert!(graph.compose(m1_id, mi_id, vec![]).is_some());
    }

    #[test]
    fn test_roots() {
        let mut graph = ScenarioGraph::new();

        let macro_s = Scenario::new("Root Macro", ScenarioScale::Macro);
        let micro_s = Scenario::new("Child Micro", ScenarioScale::Micro);
        let macro_id = macro_s.id;
        let micro_id = micro_s.id;

        graph.add_scenario(macro_s);
        graph.add_scenario(micro_s);
        graph.compose(macro_id, micro_id, vec![]);

        let roots = graph.roots();
        assert!(roots.contains(&macro_id));
        assert!(!roots.contains(&micro_id));
    }
}
