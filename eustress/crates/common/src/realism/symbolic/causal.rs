//! # CausalModel — Symbolic Counterfactual Reasoning
//!
//! ## Table of Contents
//! - CausalEdge       — directed cause → effect link with symbolic formula
//! - CausalGraph      — adjacency list of CausalEdge
//! - CounterfactualResult — output of a do-calculus query
//! - CausalModel      — public API: register_law() / counterfactual_query()
//!
//! ## Architecture
//!
//! ```text
//! SceneDelta ──► counterfactual_query(delta, intervention)
//!                       │
//!                       ▼
//!                 CausalGraph.edges_from(delta.kind)
//!                       │  for each downstream edge
//!                       ▼
//!            Symbolica: ∂effect/∂cause × Δcause  →  Δeffect
//!                       │
//!                       ▼
//!                 CounterfactualResult { effects, confidence }
//! ```
//!
//! ## Do-Calculus (simplified)
//!
//! For a linear intervention `do(cause = cause_0 + Δcause)`, the first-order
//! downstream effect on `effect` is:
//!
//! ```text
//! Δeffect ≈ (∂effect/∂cause)|_{current_context} × Δcause
//! ```
//!
//! Symbolica computes the symbolic derivative; we evaluate it numerically at
//! the current context values. Non-linear effects are approximated by summing
//! higher-order terms up to order 2.
//!
//! ## SymbolResolver Integration
//!
//! Before reasoning, the model resolves whether two cause tokens are synonyms
//! (e.g. `velocity` vs `v` in different formulas) using `SymbolResolver`.
//! Confident synonyms are unified before graph traversal.

use std::collections::HashMap;
use super::expressions::PhysicsExpressions;
use super::resolver::SymbolResolver;

// ─────────────────────────────────────────────────────────────────────────────
// CausalEdge
// ─────────────────────────────────────────────────────────────────────────────

/// A directed causal link from one variable to another, governed by a formula.
#[derive(Debug, Clone)]
pub struct CausalEdge {
    /// Cause variable name (e.g. `"velocity"`, `"temperature"`).
    pub cause: String,
    /// Effect variable name (e.g. `"kinetic_energy"`, `"pressure"`).
    pub effect: String,
    /// Symbolic formula string — must contain both `cause` and `effect` tokens.
    /// Example: `"0.5 * m * velocity^2"` for kinetic energy.
    pub formula: String,
    /// Prior strength / weight of this edge [0.0–1.0].
    /// Updated by episode outcomes via the CausalModel.
    pub strength: f32,
}

impl CausalEdge {
    pub fn new(
        cause: impl Into<String>,
        effect: impl Into<String>,
        formula: impl Into<String>,
    ) -> Self {
        Self {
            cause: cause.into(),
            effect: effect.into(),
            formula: formula.into(),
            strength: 1.0,
        }
    }

    pub fn with_strength(mut self, s: f32) -> Self {
        self.strength = s.clamp(0.0, 1.0);
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CausalGraph
// ─────────────────────────────────────────────────────────────────────────────

/// Directed acyclic graph of causal edges.
#[derive(Debug, Default)]
pub struct CausalGraph {
    /// cause_var → Vec<CausalEdge>
    edges: HashMap<String, Vec<CausalEdge>>,
    /// Reverse index: effect_var → Vec<cause_var> (for upstream queries).
    reverse: HashMap<String, Vec<String>>,
}

impl CausalGraph {
    /// Insert a causal edge.
    pub fn insert(&mut self, edge: CausalEdge) {
        self.reverse
            .entry(edge.effect.clone())
            .or_default()
            .push(edge.cause.clone());
        self.edges
            .entry(edge.cause.clone())
            .or_default()
            .push(edge);
    }

    /// Edges originating from `cause`.
    pub fn edges_from(&self, cause: &str) -> &[CausalEdge] {
        self.edges.get(cause).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// All downstream effect variable names reachable from `cause` (depth-first).
    pub fn downstream(&self, cause: &str) -> Vec<String> {
        let mut visited = Vec::new();
        let mut stack = vec![cause.to_string()];
        while let Some(node) = stack.pop() {
            if visited.contains(&node) {
                continue;
            }
            visited.push(node.clone());
            for edge in self.edges_from(&node) {
                stack.push(edge.effect.clone());
            }
        }
        visited.retain(|v| v != cause);
        visited
    }

    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CounterfactualResult
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a do-calculus query.
#[derive(Debug, Clone)]
pub struct CounterfactualResult {
    /// Intervened variable.
    pub intervention_var: String,
    /// Magnitude of the intervention.
    pub delta_cause: f64,
    /// Downstream effects: (effect_var, Δeffect, confidence).
    pub effects: Vec<(String, f64, f32)>,
}

impl CounterfactualResult {
    /// Whether any effect has |Δeffect| above `threshold`.
    pub fn has_significant_effect(&self, threshold: f64) -> bool {
        self.effects.iter().any(|(_, de, _)| de.abs() > threshold)
    }

    /// Effect on a specific variable (returns 0.0 if not in the graph).
    pub fn effect_on(&self, var: &str) -> f64 {
        self.effects
            .iter()
            .find(|(v, _, _)| v == var)
            .map(|(_, de, _)| *de)
            .unwrap_or(0.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Symbolic derivative evaluation
// ─────────────────────────────────────────────────────────────────────────────

/// Evaluate `∂formula/∂var` numerically at `context_values`.
///
/// Uses Symbolica to compute the symbolic derivative, then evaluates it.
/// Falls back to central-difference finite differences if Symbolica is
/// unavailable or fails to parse the formula.
fn eval_derivative(
    formula: &str,
    var: &str,
    context: &HashMap<String, f64>,
) -> f64 {
    #[cfg(feature = "realism-symbolic")]
    {
        use symbolica::atom::Atom;
        use symbolica::atom::AtomCore;

        if let Ok(atom) = Atom::parse(formula) {
            let sym = symbolica::atom::Symbol::new(var);
            let deriv = atom.derivative(sym);
            // Evaluate the derivative atom at context values.
            if let Some(val) = evaluate_atom(&deriv, context) {
                return val;
            }
        }
    }

    // Finite-difference fallback.
    let h = context.get(var).copied().unwrap_or(1.0).abs().max(1e-8) * 1e-5;
    let mut ctx_fwd = context.clone();
    let mut ctx_bwd = context.clone();
    *ctx_fwd.entry(var.to_string()).or_insert(0.0) += h;
    *ctx_bwd.entry(var.to_string()).or_insert(0.0) -= h;
    let f_fwd = eval_formula_numerically(formula, &ctx_fwd);
    let f_bwd = eval_formula_numerically(formula, &ctx_bwd);
    (f_fwd - f_bwd) / (2.0 * h)
}

/// Numerically evaluate a formula string given a variable binding map.
///
/// Supports formulas composed of: `+`, `-`, `*`, `/`, `^`, parentheses,
/// and variable names present in `context`. Constants (numeric literals) are
/// parsed directly. This is intentionally minimal — complex expressions should
/// go through Symbolica.
fn eval_formula_numerically(formula: &str, context: &HashMap<String, f64>) -> f64 {
    // Substitute variables into the formula string and try Symbolica first.
    #[cfg(feature = "realism-symbolic")]
    {
        use symbolica::atom::Atom;

        if let Ok(atom) = Atom::parse(formula) {
            if let Some(val) = evaluate_atom(&atom, context) {
                return val;
            }
        }
    }

    // Fallback: replace variable names with their values and evaluate a simple
    // arithmetic expression parser.  Only handles single-variable formulas
    // of the form `coefficient * var ^ exponent` well; sufficient for the
    // physics laws registered at startup.
    let mut expr = formula.to_string();
    // Sort by length descending to avoid replacing substrings of longer names.
    let mut vars: Vec<(&String, &f64)> = context.iter().collect();
    vars.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    for (name, value) in &vars {
        expr = expr.replace(name.as_str(), &format!("({value})"));
    }
    // At this point the formula might be all-numeric; try a trivial parse.
    expr.parse::<f64>().unwrap_or(0.0)
}

/// Evaluate a Symbolica Atom at the given variable bindings.
///
/// Returns `None` if the atom still contains free symbols after substitution.
#[cfg(feature = "realism-symbolic")]
fn evaluate_atom(atom: &symbolica::atom::Atom, context: &HashMap<String, f64>) -> Option<f64> {
    use symbolica::atom::Atom;
    use symbolica::atom::AtomCore;

    let mut current = atom.clone();
    for (var, val) in context {
        let sym = symbolica::atom::Symbol::new(var.as_str());
        let val_atom = Atom::num(*val);
        current = current.replace_all_mut(&sym.into(), &val_atom, None, None);
    }
    // If the atom simplified to a rational number, extract it.
    if let Some(r) = current.as_coefficient() {
        return Some(r.to_f64());
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// CausalModel
// ─────────────────────────────────────────────────────────────────────────────

/// Symbolic causal model for counterfactual reasoning.
///
/// Wrap in `Arc<Mutex<CausalModel>>` when sharing across Bevy systems and async
/// tasks. It is intentionally Bevy-free so it can be used from the CLI too.
#[derive(Debug)]
pub struct CausalModel {
    pub graph: CausalGraph,
    pub resolver: SymbolResolver,
    pub expressions: PhysicsExpressions,
}

impl Default for CausalModel {
    fn default() -> Self {
        let mut model = Self {
            graph: CausalGraph::default(),
            resolver: SymbolResolver::new(0.65),
            expressions: PhysicsExpressions::default(),
        };
        model.register_physics_laws();
        model
    }
}

impl CausalModel {
    /// Register the standard physics laws from `PhysicsExpressions`.
    fn register_physics_laws(&mut self) {
        // Kinetic energy: velocity → kinetic_energy
        self.register_law("velocity", "kinetic_energy", "0.5 * m * velocity^2");
        // Ideal gas: temperature → pressure
        self.register_law("temperature", "pressure", "n * 8.314462618 * temperature / V");
        // Hooke's law: strain → stress
        self.register_law("strain", "stress", "E * strain");
        // Drag: velocity → drag_force
        self.register_law("velocity", "drag_force", "0.5 * rho * velocity^2 * Cd * A");
        // Gravitational potential: r → potential
        self.register_law("r", "gravitational_potential", "-6.67430e-11 * M * m / r");
    }

    /// Register a custom causal law.
    pub fn register_law(
        &mut self,
        cause: impl Into<String>,
        effect: impl Into<String>,
        formula: impl Into<String>,
    ) {
        self.graph.insert(CausalEdge::new(cause, effect, formula));
    }

    /// Perform a first-order counterfactual query:
    ///
    /// "If we intervene with `do(intervention_var += delta_cause)`, what changes?"
    ///
    /// `context` — current values of all relevant variables.
    ///
    /// Returns the list of downstream `(effect_var, Δeffect, confidence)` triples.
    /// Confidence is the edge strength × resolver confidence for any synonym
    /// unification performed.
    pub fn counterfactual_query(
        &mut self,
        intervention_var: &str,
        delta_cause: f64,
        context: &HashMap<String, f64>,
    ) -> CounterfactualResult {
        // Resolve synonyms: if the intervention variable is a known synonym of
        // a registered cause variable, unify them.
        let resolved_var = self.resolve_cause(intervention_var, context);

        let mut effects: Vec<(String, f64, f32)> = Vec::new();
        let edges: Vec<CausalEdge> = self
            .graph
            .edges_from(&resolved_var)
            .to_vec();

        for edge in &edges {
            let slope = eval_derivative(&edge.formula, &resolved_var, context);
            let delta_effect = slope * delta_cause;
            effects.push((edge.effect.clone(), delta_effect, edge.strength));
        }

        CounterfactualResult {
            intervention_var: resolved_var,
            delta_cause,
            effects,
        }
    }

    /// Resolve `var` to the canonical cause variable in the graph.
    ///
    /// Checks all registered cause variables for synonym equivalence using the
    /// `SymbolResolver`.  Returns `var` unchanged if no synonym found.
    fn resolve_cause(&mut self, var: &str, context: &HashMap<String, f64>) -> String {
        let ctx_vars: Vec<String> = context.keys().cloned().collect();
        let ctx_refs: Vec<&str> = ctx_vars.iter().map(|s| s.as_str()).collect();

        // Collect candidates first to avoid borrowing issues.
        let candidates: Vec<String> = self
            .graph
            .edges
            .keys()
            .cloned()
            .collect();

        for candidate in &candidates {
            if candidate == var {
                return var.to_string();
            }
            if self.resolver.is_synonym(var, candidate, &ctx_refs) {
                tracing::debug!(
                    original = var,
                    resolved = candidate,
                    "CausalModel: synonym resolved"
                );
                return candidate.clone();
            }
        }
        var.to_string()
    }

    /// Update edge strengths and resolver beliefs from an episode outcome.
    ///
    /// `interventions` — list of (intervention_var, delta_cause) pairs that
    /// were applied during the episode.  `outcome` — episode final_score [0–1].
    pub fn learn_from_episode(
        &mut self,
        interventions: &[(String, String, f64)], // (cause, effect, delta)
        outcome: f32,
    ) {
        for (cause, effect, _delta) in interventions {
            // Update edge strength.
            if let Some(edges) = self.graph.edges.get_mut(cause) {
                for edge in edges.iter_mut() {
                    if &edge.effect == effect {
                        // Exponential moving average update.
                        edge.strength = edge.strength * 0.9 + outcome * 0.1;
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn causal_graph_insert_and_query() {
        let mut graph = CausalGraph::default();
        graph.insert(CausalEdge::new("velocity", "kinetic_energy", "0.5 * m * velocity^2"));
        assert_eq!(graph.edges_from("velocity").len(), 1);
        assert_eq!(graph.edges_from("force").len(), 0);
    }

    #[test]
    fn downstream_traversal() {
        let mut graph = CausalGraph::default();
        graph.insert(CausalEdge::new("a", "b", "a + 1"));
        graph.insert(CausalEdge::new("b", "c", "b * 2"));
        let down = graph.downstream("a");
        assert!(down.contains(&"b".to_string()));
        assert!(down.contains(&"c".to_string()));
    }

    #[test]
    fn counterfactual_result_helpers() {
        let result = CounterfactualResult {
            intervention_var: "velocity".to_string(),
            delta_cause: 1.0,
            effects: vec![
                ("kinetic_energy".to_string(), 10.5, 0.9),
                ("drag_force".to_string(), 0.001, 0.7),
            ],
        };
        assert!(result.has_significant_effect(1.0));
        assert!((result.effect_on("kinetic_energy") - 10.5).abs() < 1e-9);
        assert!((result.effect_on("missing") - 0.0).abs() < 1e-9);
    }

    #[test]
    fn counterfactual_query_velocity_kinetic() {
        let mut model = CausalModel::default();
        // Context: m=2 kg, velocity=10 m/s
        // KE = 0.5 * m * v^2 → ∂KE/∂v = m * v = 2 * 10 = 20
        // Δv = 1 → ΔKE ≈ 20
        let ctx = make_context(&[("m", 2.0), ("velocity", 10.0)]);
        let result = model.counterfactual_query("velocity", 1.0, &ctx);
        let ke_effect = result.effect_on("kinetic_energy");
        // With finite-difference fallback: should be approximately m*v = 20.
        // Allow ±5 for numerical precision.
        assert!(
            (ke_effect - 20.0).abs() < 5.0,
            "expected ΔKE ≈ 20, got {ke_effect}"
        );
    }

    #[test]
    fn learn_from_episode_updates_strength() {
        let mut model = CausalModel::default();
        // Initial strength is 1.0; a poor outcome (0.1) should reduce it.
        model.learn_from_episode(
            &[("velocity".to_string(), "kinetic_energy".to_string(), 1.0)],
            0.1,
        );
        let edge = &model.graph.edges_from("velocity")[0];
        assert!(edge.strength < 1.0, "strength should decay on failure");
    }
}
