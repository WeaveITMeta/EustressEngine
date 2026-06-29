//! The optimization loop (Phase 5 / Way 50, A1). A pluggable `Optimizer` drives
//! generate -> score -> optimize. A deterministic hill-climb baseline ships so
//! the loop runs end-to-end TODAY on closed-form fitness (Phase 5's first
//! milestone); CMA-ES / evolutionary / surrogate impls slot behind the trait.

use crate::candidate::ArchCandidate;
use crate::fitness::{Fitness, Score};

/// A scored candidate (the loop's memory).
#[derive(Clone, Debug)]
pub struct Evaluated {
    pub candidate: ArchCandidate,
    pub score: Score,
}

/// Proposes the next candidate from the history of evaluated ones.
pub trait Optimizer {
    /// Propose the next candidate to evaluate. `history` is every prior
    /// evaluation (NOT pre-sorted — pick the best yourself if you want it).
    fn propose(&mut self, history: &[Evaluated]) -> ArchCandidate;
}

/// Hill-climb baseline: perturb the best-so-far candidate's member areas.
/// Deterministic (a seeded xorshift, no RNG dep) so runs replay bit-identically.
pub struct HillClimb {
    seed: u64,
    /// Relative perturbation magnitude per step.
    pub step: f32,
    seed_candidate: ArchCandidate,
}
impl HillClimb {
    pub fn new(seed: u64, seed_candidate: ArchCandidate) -> Self {
        Self { seed: seed | 1, step: 0.1, seed_candidate }
    }
    fn next_unit(&mut self) -> f32 {
        // xorshift64 -> (0,1)
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f32 / (1u64 << 53) as f32
    }
}
impl Optimizer for HillClimb {
    fn propose(&mut self, history: &[Evaluated]) -> ArchCandidate {
        let base = history
            .iter()
            .max_by(|a, b| {
                a.score
                    .total
                    .partial_cmp(&b.score.total)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|e| e.candidate.clone())
            .unwrap_or_else(|| self.seed_candidate.clone());
        let mut next = base;
        for m in next.members.iter_mut() {
            let delta = (self.next_unit() - 0.5) * 2.0 * self.step;
            m.area = (m.area * (1.0 + delta)).max(1e-6);
        }
        next.id = next.id.wrapping_add(1);
        next
    }
}

/// Run the generate -> score -> optimize loop for `iters` and return every
/// evaluation. Phase 5's first milestone made concrete: the loop runs
/// end-to-end on closed-form fitness, deterministically.
pub fn run_loop(
    seed_candidate: ArchCandidate,
    fitness: &dyn Fitness,
    optimizer: &mut dyn Optimizer,
    iters: usize,
) -> Vec<Evaluated> {
    let mut history = Vec::with_capacity(iters + 1);
    let s0 = fitness.score(&seed_candidate);
    history.push(Evaluated { candidate: seed_candidate, score: s0 });
    for _ in 0..iters {
        let cand = optimizer.propose(&history);
        let score = fitness.score(&cand);
        history.push(Evaluated { candidate: cand, score });
    }
    history
}

/// The best evaluation in a history by total score.
pub fn best(history: &[Evaluated]) -> Option<&Evaluated> {
    history.iter().max_by(|a, b| {
        a.score
            .total
            .partial_cmp(&b.score.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::{ArchCandidate, BondKind, MaterialSpec, Member, Node, Support};
    use crate::fitness::ClosedFormFitness;

    fn seed() -> ArchCandidate {
        let mut c = ArchCandidate::new(1);
        c.materials.push(MaterialSpec::steel());
        c.nodes.push(Node { pos: [0.0; 3], support: Support::Fixed, load: [0.0; 3] });
        c.nodes.push(Node { pos: [1.0, 0.0, 0.0], support: Support::Pinned, load: [0.0; 3] });
        c.nodes.push(Node { pos: [0.5, 1.0, 0.0], support: Support::Free, load: [0.0, -5000.0, 0.0] });
        c.members.push(Member { from: 0, to: 2, area: 0.02, material: 0, bond: BondKind::Pinned });
        c.members.push(Member { from: 1, to: 2, area: 0.02, material: 0, bond: BondKind::Pinned });
        c
    }

    #[test]
    fn loop_runs_and_keeps_best() {
        let fit = ClosedFormFitness::default();
        let mut opt = HillClimb::new(42, seed());
        let history = run_loop(seed(), &fit, &mut opt, 50);
        assert_eq!(history.len(), 51);
        let b = best(&history).unwrap();
        // The best is at least the seed's score (the seed is in the history).
        assert!(b.score.total >= history[0].score.total - 1e-6);
        assert!(b.score.total > 0.0);
    }

    #[test]
    fn deterministic_replay() {
        let fit = ClosedFormFitness::default();
        let run = |s: u64| {
            let mut opt = HillClimb::new(s, seed());
            best(&run_loop(seed(), &fit, &mut opt, 30)).unwrap().score.total
        };
        assert_eq!(run(7), run(7), "same seed -> identical result");
    }
}
