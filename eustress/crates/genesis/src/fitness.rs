//! Fitness scoring (Phase 5 / Way A4, 28, 49). Closed-form first (works today),
//! with the same trait that an Avian-settle or FEA-backed scorer slots behind
//! (the eval ladder: closed-form -> Avian settle-test -> FEA confirm, Way A3).

use serde::{Deserialize, Serialize};

use crate::candidate::{ArchCandidate, Support};

/// A multi-axis score. `total` is the scalar the optimizer maximizes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Score {
    /// Higher = more stable (supported, triangulated).
    pub stability: f32,
    /// Higher = more material-efficient (load carried per unit mass).
    pub efficiency: f32,
    /// 0..1 code-compliance (1 = passes all rule cards).
    pub compliance: f32,
    /// Higher = closer to the target style latent (1 = no target / matched).
    pub style: f32,
    /// Weighted scalar the optimizer maximizes.
    pub total: f32,
}

/// Anything that can score a candidate. Closed-form today; an FEA- or
/// Avian-backed impl confirms later behind the SAME trait.
pub trait Fitness {
    fn score(&self, c: &ArchCandidate) -> Score;
}

/// Relative weights for the scalarized objective.
#[derive(Clone, Copy, Debug)]
pub struct Weights {
    pub stability: f32,
    pub efficiency: f32,
    pub compliance: f32,
    pub style: f32,
}
impl Default for Weights {
    fn default() -> Self {
        Self { stability: 0.4, efficiency: 0.3, compliance: 0.2, style: 0.1 }
    }
}

/// Closed-form fitness — runs today, no physics, deterministic. The first rung
/// of the eval ladder.
pub struct ClosedFormFitness {
    pub weights: Weights,
    /// Target style latent to mimic (empty = invent / no style target).
    pub target_style: Vec<f32>,
}
impl Default for ClosedFormFitness {
    fn default() -> Self {
        Self { weights: Weights::default(), target_style: Vec::new() }
    }
}

impl Fitness for ClosedFormFitness {
    fn score(&self, c: &ArchCandidate) -> Score {
        let stability = stability_heuristic(c);
        let efficiency = efficiency_heuristic(c);
        let compliance = compliance_heuristic(c);
        let style = style_match(c, &self.target_style);
        let w = self.weights;
        let total = w.stability * stability
            + w.efficiency * efficiency
            + w.compliance * compliance
            + w.style * style;
        Score { stability, efficiency, compliance, style, total }
    }
}

/// Supported + triangulated structures score higher.
fn stability_heuristic(c: &ArchCandidate) -> f32 {
    if c.nodes.is_empty() || c.members.is_empty() {
        return 0.0;
    }
    let grounded = c.nodes.iter().filter(|n| !matches!(n.support, Support::Free)).count();
    let ground_score = (grounded as f32 / c.nodes.len() as f32).min(1.0);
    // Triangulation proxy: a determinate truss wants ~2 members per node.
    let ratio = c.members.len() as f32 / c.nodes.len().max(1) as f32;
    let tri_score = (ratio / 2.0).min(1.0);
    0.5 * ground_score + 0.5 * tri_score
}

/// Efficiency = total external load magnitude carried per unit mass, soft-normalized.
fn efficiency_heuristic(c: &ArchCandidate) -> f32 {
    let mass = c.total_mass();
    if mass <= 0.0 {
        return 0.0;
    }
    let load: f32 = c
        .nodes
        .iter()
        .map(|n| (n.load[0] * n.load[0] + n.load[1] * n.load[1] + n.load[2] * n.load[2]).sqrt())
        .sum();
    let raw = load / mass;
    raw / (raw + 1.0)
}

/// Placeholder code-compliance: 1.0 unless a structural-integrity rule is
/// violated. Real symbolic rule cards land in Way 28/49.
fn compliance_heuristic(c: &ArchCandidate) -> f32 {
    let n = c.nodes.len();
    let m = c.materials.len();
    let valid = c
        .members
        .iter()
        .all(|mem| mem.from < n && mem.to < n && mem.material < m && mem.area > 0.0);
    if valid {
        1.0
    } else {
        0.0
    }
}

/// Cosine similarity to the target style latent (1.0 if no target).
fn style_match(c: &ArchCandidate, target: &[f32]) -> f32 {
    if target.is_empty() {
        return 1.0;
    }
    let a = &c.style.latent;
    let len = a.len().min(target.len());
    if len == 0 {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..len {
        dot += a[i] * target[i];
        na += a[i] * a[i];
        nb += target[i] * target[i];
    }
    if na <= 0.0 || nb <= 0.0 {
        return 0.0;
    }
    (dot / (na.sqrt() * nb.sqrt())).clamp(0.0, 1.0)
}
