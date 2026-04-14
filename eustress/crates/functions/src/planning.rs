//! # Stage 12: Planning — Goal, Plan & Act (GOAP-style)
//!
//! Exposes a lightweight Goal-Oriented Action Planning interface to Rune scripts.
//! The bridge holds a set of registered actions and current world state.
//! No external GOAP crate dependency — the planner is implemented inline.
//!
//! ## Table of Contents
//! 1. Result types     — GoalResult, PlanStep, ActionResult
//! 2. PlanningBridge   — thread-local world state + action registry
//! 3. Rune functions   — goal / plan / act
//! 4. GOAP planner     — BFS over state space
//! 5. Module registration
//!
//! ## Functions
//!
//! | Function                        | Purpose                                             |
//! |---------------------------------|-----------------------------------------------------|
//! | `goal(description)`             | Define a high-level objective, returns a GoalResult |
//! | `plan(goal_key, constraints_csv)` | Generate an action sequence to achieve a goal     |
//! | `act(step_index)`               | Execute a single step from the current plan         |
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::planning;
//!
//! pub fn navigate_to_target(entity_bits) {
//!     let g = planning::goal("reach_target");
//!     if g.feasible {
//!         let steps = planning::plan("reach_target", "max_steps=5");
//!         for step in steps.steps {
//!             let result = planning::act(step.index);
//!             if !result.success { break; }
//!         }
//!     }
//! }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use tracing::{info, warn};

// ============================================================================
// 1. Result Types
// ============================================================================

/// Result from `goal()` — describes a high-level objective.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct GoalResult {
    /// The goal key/description
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub key: String,
    /// Whether this goal is achievable given current world state
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub feasible: bool,
    /// Estimated cost to achieve (sum of action costs on cheapest plan)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub estimated_cost: f64,
    /// Human-readable status message
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub message: String,
}

/// A single step in a generated plan.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct PlanStep {
    /// Step index in the plan (0-based)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub index: i64,
    /// Action name to execute at this step
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub action: String,
    /// Cost of this action
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub cost: f64,
    /// Expected world state keys changed by this action (CSV)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub effects_csv: String,
}

/// A generated plan from `plan()`.
#[derive(Debug)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct ActionPlan {
    /// Goal this plan achieves
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub goal: String,
    /// Ordered list of `PlanStep` values
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub steps: rune::runtime::Vec,
    /// Total cost of the plan
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub total_cost: f64,
    /// Number of steps
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub step_count: i64,
    /// Whether a valid plan was found
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub found: bool,
}

impl ActionPlan {
    fn empty(goal: &str) -> Self {
        Self {
            goal: goal.to_string(),
            steps: rune::runtime::Vec::new(),
            total_cost: 0.0,
            step_count: 0,
            found: false,
        }
    }
}

/// Result from executing a single plan step via `act()`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct ActionResult {
    /// Whether the action executed successfully
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub success: bool,
    /// The action that was executed
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub action: String,
    /// State keys that were updated as a result
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub updated_keys_csv: String,
    /// Human-readable result message
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub message: String,
}

// ============================================================================
// 2. PlanningBridge — thread-local world state + action registry
// ============================================================================

/// A registered action in the GOAP system.
#[derive(Debug, Clone)]
pub struct ActionDef {
    /// Unique action name
    pub name: String,
    /// Preconditions: world state keys that must be true to use this action
    pub preconditions: HashMap<String, bool>,
    /// Effects: world state changes this action produces
    pub effects: HashMap<String, bool>,
    /// Cost of executing this action
    pub cost: f64,
}

impl ActionDef {
    /// Create a new action
    pub fn new(name: impl Into<String>, cost: f64) -> Self {
        Self {
            name: name.into(),
            preconditions: HashMap::new(),
            effects: HashMap::new(),
            cost,
        }
    }

    /// Require a world state key to be true
    pub fn require(mut self, key: impl Into<String>, value: bool) -> Self {
        self.preconditions.insert(key.into(), value);
        self
    }

    /// Set an effect on a world state key
    pub fn effect(mut self, key: impl Into<String>, value: bool) -> Self {
        self.effects.insert(key.into(), value);
        self
    }
}

/// Goal definition — a desired world state configuration.
#[derive(Debug, Clone)]
pub struct GoalDef {
    /// Goal key/name
    pub key: String,
    /// Desired world state (key → required value)
    pub desired_state: HashMap<String, bool>,
}

/// Bridge holding world state, action registry, and active plan.
pub struct PlanningBridge {
    /// Current world state (boolean flags)
    pub world_state: HashMap<String, bool>,
    /// Registered actions
    pub actions: Vec<ActionDef>,
    /// Registered goals
    pub goals: HashMap<String, GoalDef>,
    /// The most recently generated plan (index → action name)
    pub active_plan: Vec<String>,
}

impl PlanningBridge {
    /// Create an empty bridge
    pub fn new() -> Self {
        Self {
            world_state: HashMap::new(),
            actions: Vec::new(),
            goals: HashMap::new(),
            active_plan: Vec::new(),
        }
    }

    /// Set a world state boolean flag
    pub fn set_state(&mut self, key: impl Into<String>, value: bool) {
        self.world_state.insert(key.into(), value);
    }

    /// Register an action
    pub fn register_action(&mut self, action: ActionDef) {
        self.actions.push(action);
    }

    /// Register a goal
    pub fn register_goal(&mut self, goal: GoalDef) {
        self.goals.insert(goal.key.clone(), goal);
    }

    /// Check if an action's preconditions are satisfied by a given state
    fn satisfies_preconditions(action: &ActionDef, state: &HashMap<String, bool>) -> bool {
        action.preconditions.iter().all(|(k, &required)| {
            state.get(k).copied().unwrap_or(false) == required
        })
    }

    /// Apply action effects to a state, returning the new state
    fn apply_effects(action: &ActionDef, state: &HashMap<String, bool>) -> HashMap<String, bool> {
        let mut new_state = state.clone();
        for (k, &v) in &action.effects {
            new_state.insert(k.clone(), v);
        }
        new_state
    }

    /// Check if a state satisfies the goal
    fn goal_satisfied(goal: &GoalDef, state: &HashMap<String, bool>) -> bool {
        goal.desired_state.iter().all(|(k, &required)| {
            state.get(k).copied().unwrap_or(false) == required
        })
    }

    /// BFS GOAP planner — finds shortest-cost action sequence to achieve the goal.
    /// Returns ordered action names, or empty if no plan found within max_steps.
    pub fn find_plan(&self, goal_key: &str, max_steps: usize) -> Vec<String> {
        let Some(goal) = self.goals.get(goal_key) else {
            return Vec::new();
        };

        // Already satisfied?
        if Self::goal_satisfied(goal, &self.world_state) {
            return Vec::new();
        }

        // BFS: (current_state, path_so_far)
        let mut queue: std::collections::VecDeque<(HashMap<String, bool>, Vec<String>)> =
            std::collections::VecDeque::new();
        queue.push_back((self.world_state.clone(), Vec::new()));

        while let Some((state, path)) = queue.pop_front() {
            if path.len() >= max_steps {
                continue;
            }

            for action in &self.actions {
                if Self::satisfies_preconditions(action, &state) {
                    let new_state = Self::apply_effects(action, &state);
                    let mut new_path = path.clone();
                    new_path.push(action.name.clone());

                    if Self::goal_satisfied(goal, &new_state) {
                        return new_path;
                    }

                    queue.push_back((new_state, new_path));
                }
            }
        }

        Vec::new()
    }
}

impl Default for PlanningBridge {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static PLANNING_BRIDGE: RefCell<Option<PlanningBridge>> = RefCell::new(None);
}

/// Install the planning bridge before Rune execution.
pub fn set_planning_bridge(bridge: PlanningBridge) {
    PLANNING_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Remove and return the bridge after Rune execution.
pub fn take_planning_bridge() -> Option<PlanningBridge> {
    PLANNING_BRIDGE.with(|cell| cell.borrow_mut().take())
}

fn with_bridge<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&PlanningBridge) -> R,
{
    PLANNING_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Planning] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

fn with_bridge_mut<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&mut PlanningBridge) -> R,
{
    PLANNING_BRIDGE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Planning] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// 3. Rune Functions
// ============================================================================

/// Define a high-level objective and check its feasibility.
///
/// Looks up the goal in the planning bridge's goal registry and runs a
/// quick feasibility check (can any action sequence reach the desired state?).
///
/// # Arguments
/// * `description` — Goal key registered in the bridge (e.g. `"reach_target"`)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn goal(description: &str) -> GoalResult {
    with_bridge(
        GoalResult {
            key: description.to_string(),
            feasible: false,
            estimated_cost: 0.0,
            message: "Planning bridge not available".to_string(),
        },
        |bridge| {
            let (feasible, cost, message) = match bridge.goals.get(description) {
                Some(goal_def) => {
                    // Check if already satisfied
                    if PlanningBridge::goal_satisfied(goal_def, &bridge.world_state) {
                        (true, 0.0, "Goal already satisfied".to_string())
                    } else {
                        // Quick feasibility: try planning with max 8 steps
                        let plan = bridge.find_plan(description, 8);
                        if plan.is_empty() {
                            (false, f64::MAX, format!("No plan found for '{}'", description))
                        } else {
                            // Estimate cost as sum of action costs
                            let total_cost: f64 = plan.iter()
                                .filter_map(|name| bridge.actions.iter().find(|a| &a.name == name))
                                .map(|a| a.cost)
                                .sum();
                            (true, total_cost, format!("Achievable in {} steps", plan.len()))
                        }
                    }
                }
                None => (
                    false,
                    0.0,
                    format!("Goal '{}' not registered in planning bridge", description),
                ),
            };

            info!(
                "[Planning] goal('{}') feasible={} cost={:.2}",
                description, feasible, cost
            );

            GoalResult {
                key: description.to_string(),
                feasible,
                estimated_cost: cost,
                message,
            }
        },
    )
}

/// Generate an action sequence to achieve a registered goal.
///
/// Runs the BFS GOAP planner from the current world state to the goal.
/// Constraint string supports `max_steps=N` (default 10).
///
/// # Arguments
/// * `goal_key`         — Goal name registered in the bridge
/// * `constraints_csv`  — Constraint string (e.g. `"max_steps=5"`)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn plan(goal_key: &str, constraints_csv: &str) -> ActionPlan {
    // Parse max_steps from constraints
    let max_steps = constraints_csv
        .split(',')
        .find_map(|part| {
            let kv: Vec<&str> = part.trim().splitn(2, '=').collect();
            if kv.len() == 2 && kv[0].trim() == "max_steps" {
                kv[1].trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(10);

    with_bridge(ActionPlan::empty(goal_key), |bridge| {
        let action_names = bridge.find_plan(goal_key, max_steps);

        if action_names.is_empty() {
            info!("[Planning] plan('{}') → no plan found", goal_key);
            return ActionPlan::empty(goal_key);
        }

        let mut steps = rune::runtime::Vec::new();
        let mut total_cost = 0.0;

        for (idx, name) in action_names.iter().enumerate() {
            let cost = bridge
                .actions
                .iter()
                .find(|a| &a.name == name)
                .map(|a| a.cost)
                .unwrap_or(1.0);

            let effects_csv = bridge
                .actions
                .iter()
                .find(|a| &a.name == name)
                .map(|a| {
                    a.effects
                        .keys()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();

            total_cost += cost;

            let step = PlanStep {
                index: idx as i64,
                action: name.clone(),
                cost,
                effects_csv,
            };
            if let Ok(v) = rune::to_value(step) {
                let _ = steps.push(v);
            }
        }

        let step_count = steps.len() as i64;
        info!(
            "[Planning] plan('{}', max_steps={}) → {} steps, cost={:.2}",
            goal_key, max_steps, step_count, total_cost
        );

        ActionPlan {
            goal: goal_key.to_string(),
            steps,
            total_cost,
            step_count,
            found: true,
        }
    })
}

/// Execute a single step from the active plan by step index.
///
/// Looks up the action at `step_index` in the bridge's active plan,
/// applies its effects to the world state, and returns the result.
///
/// # Arguments
/// * `step_index` — 0-based index into the active plan
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn act(step_index: i64) -> ActionResult {
    with_bridge_mut(
        ActionResult {
            success: false,
            action: String::new(),
            updated_keys_csv: String::new(),
            message: "Planning bridge not available".to_string(),
        },
        |bridge| {
            let idx = step_index as usize;
            let action_name = match bridge.active_plan.get(idx) {
                Some(name) => name.clone(),
                None => {
                    warn!("[Planning] act({}): step index out of range (plan has {} steps)", idx, bridge.active_plan.len());
                    return ActionResult {
                        success: false,
                        action: String::new(),
                        updated_keys_csv: String::new(),
                        message: format!("Step {} out of range", idx),
                    };
                }
            };

            // Find the action definition
            let action_pos = bridge.actions.iter().position(|a| a.name == action_name);
            let Some(pos) = action_pos else {
                return ActionResult {
                    success: false,
                    action: action_name.clone(),
                    updated_keys_csv: String::new(),
                    message: format!("Action '{}' not found in registry", action_name),
                };
            };

            // Check preconditions
            let preconditions_ok = PlanningBridge::satisfies_preconditions(
                &bridge.actions[pos],
                &bridge.world_state,
            );

            if !preconditions_ok {
                return ActionResult {
                    success: false,
                    action: action_name.clone(),
                    updated_keys_csv: String::new(),
                    message: format!("Preconditions for '{}' not met", action_name),
                };
            }

            // Apply effects
            let effects = bridge.actions[pos].effects.clone();
            let updated_keys: Vec<String> = effects.keys().cloned().collect();
            for (k, v) in effects {
                bridge.world_state.insert(k, v);
            }

            let updated_csv = updated_keys.join(",");
            info!(
                "[Planning] act({}) → '{}' applied, updated: [{}]",
                step_index, action_name, updated_csv
            );

            ActionResult {
                success: true,
                action: action_name,
                updated_keys_csv: updated_csv,
                message: "Action executed successfully".to_string(),
            }
        },
    )
}

// ============================================================================
// 5. Module Registration
// ============================================================================

/// Create the `planning` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_planning_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "planning"])?;

    module.ty::<GoalResult>()?;
    module.ty::<PlanStep>()?;
    module.ty::<ActionPlan>()?;
    module.ty::<ActionResult>()?;

    module.function_meta(goal)?;
    module.function_meta(plan)?;
    module.function_meta(act)?;

    Ok(module)
}
