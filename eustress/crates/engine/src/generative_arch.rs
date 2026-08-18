//! # Generative architecture loop (Phase 5)
//!
//! Wraps the engine-free [`eustress_genesis`] crate so the world-model's
//! generate -> score -> optimize loop runs inside a live engine instead of
//! only inside its own unit tests.
//!
//! ## Why the FEA gate is not optional
//!
//! `HillClimb` tunes exactly one variable: [`Member::area`]. Every other term
//! in `ClosedFormFitness` is area-invariant (stability counts nodes/members;
//! compliance only requires `area > 0`; style returns 1.0 with no target),
//! while efficiency is `load / mass` soft-normalized, which rises monotonically
//! as mass falls. An UNGATED loop therefore drives every member toward the
//! `1e-6` area floor while reporting a rising score: a structure that scores
//! well and cannot stand up.
//!
//! [`FeaGatedFitness`] closes that hole by wiring the crate's own 1D linear FEA
//! solver in as a hard yield check, which is the seam the `Fitness` trait was
//! designed for. With the gate, the loop converges on the lightest section that
//! still passes yield.
//!
//! ## Honesty caveats
//!
//! * The gate is **yield-only**. It does not model buckling, and a slender
//!   vertical member in compression is exactly where buckling governs. This is
//!   a feasibility gate against the degenerate zero-area optimum, not a column
//!   design.
//! * Self-weight is not in `loads`.
//! * [`Fea1d`] assigns each node a SINGLE scalar DOF, so it is exact for an
//!   axial chain and is not a 3D truss solve. [`verify_fea`] refuses to report
//!   numbers for a non-colinear candidate rather than fabricating them.
//!
//! Distinct from [`crate::generative_pipeline`], which is an unrelated
//! text-to-mesh stub.

use bevy::prelude::*;

use eustress_genesis::{
    ArchCandidate, BarElement, BondKind, ClosedFormFitness, Fea1d, Fitness, HillClimb,
    MaterialSpec, Member, Node, Score, Support, run_loop,
};
// `best` is deliberately not in the genesis crate-root re-export, so it must be
// pathed through its module.
use eustress_genesis::optimizer::best;

/// `HillClimb::propose` scans the whole history each step and `run_loop` retains
/// a clone per iteration, so the loop is O(n^2) time with O(n) memory. This runs
/// inline on a Bevy system, so it is capped.
pub const MAX_ITERS: usize = 2_000;

/// Tolerance (m) for treating a candidate's nodes as colinear along Y.
const COLINEAR_EPS: f32 = 1e-4;

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Reject a structurally malformed candidate BEFORE scoring it.
///
/// This is mandatory, not defensive. `ClosedFormFitness::score` runs
/// `efficiency_heuristic` (which reaches `total_mass()` -> unchecked
/// `materials[m.material]` and `nodes[m.from]`) BEFORE `compliance_heuristic`,
/// which is the only bounds check in the crate. An out-of-range index from an
/// agent-supplied candidate would panic the process instead of scoring 0.
pub fn validate_candidate(c: &ArchCandidate) -> Result<(), String> {
    if c.nodes.is_empty() {
        return Err("candidate has no nodes".into());
    }
    if c.members.is_empty() {
        return Err("candidate has no members".into());
    }
    if c.materials.is_empty() {
        return Err("candidate has no materials".into());
    }
    for (i, m) in c.members.iter().enumerate() {
        if m.from >= c.nodes.len() || m.to >= c.nodes.len() {
            return Err(format!(
                "member {i} references node {}/{} out of {} nodes",
                m.from,
                m.to,
                c.nodes.len()
            ));
        }
        if m.from == m.to {
            return Err(format!("member {i} is degenerate (from == to)"));
        }
        if m.material >= c.materials.len() {
            return Err(format!(
                "member {i} references material {} out of {}",
                m.material,
                c.materials.len()
            ));
        }
        if !(m.area > 0.0) || !m.area.is_finite() {
            return Err(format!("member {i} has non-positive or non-finite area"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Demo candidate
// ---------------------------------------------------------------------------

/// A vertical steel mast: node 0 fixed at the origin, `levels` free nodes
/// stacked 1 m apart up +Y, tip load pulling down.
///
/// Colinear along Y by construction, which is a correctness requirement rather
/// than a cosmetic choice: [`Fea1d`] models one scalar DOF per node, so only an
/// axial chain yields true stresses.
pub fn demo_mast(levels: usize, tip_load_n: f32) -> ArchCandidate {
    let levels = levels.max(1);
    let mut c = ArchCandidate::new(1);
    c.materials.push(MaterialSpec::steel());
    c.nodes.push(Node { pos: [0.0, 0.0, 0.0], support: Support::Fixed, load: [0.0; 3] });
    for i in 1..=levels {
        let is_tip = i == levels;
        c.nodes.push(Node {
            pos: [0.0, i as f32, 0.0],
            support: Support::Free,
            load: if is_tip { [0.0, -tip_load_n, 0.0] } else { [0.0; 3] },
        });
    }
    for i in 0..levels {
        c.members.push(Member {
            from: i,
            to: i + 1,
            area: 0.02,
            material: 0,
            bond: BondKind::Rigid,
        });
    }
    c
}

// ---------------------------------------------------------------------------
// FEA verification
// ---------------------------------------------------------------------------

/// What the 1D FEA verifier concluded about a candidate.
#[derive(Debug, Clone, Copy, Default)]
pub struct FeaVerdict {
    /// Peak `|stress| / yield_strength` across members. `>= 1.0` fails.
    pub max_utilization: f64,
    pub max_abs_stress_pa: f64,
    /// Largest absolute nodal displacement (m).
    pub tip_displacement_m: f64,
    /// Under-constrained: `K` was singular.
    pub singular: bool,
    /// False when the candidate is not an axial chain, in which case every other
    /// field is meaningless and must not be reported.
    pub colinear: bool,
}

/// True when every node shares one X and one Z, so the structure is an axial
/// chain along Y and [`Fea1d`]'s single-DOF-per-node model is exact.
fn is_colinear_y(c: &ArchCandidate) -> bool {
    let Some(first) = c.nodes.first() else { return false };
    c.nodes
        .iter()
        .all(|n| (n.pos[0] - first.pos[0]).abs() < COLINEAR_EPS && (n.pos[2] - first.pos[2]).abs() < COLINEAR_EPS)
}

/// Run the 1D linear FEA verifier over a candidate.
///
/// Returns `colinear: false` (and nothing else meaningful) when the candidate is
/// not an axial chain, rather than reporting fabricated stresses.
pub fn verify_fea(c: &ArchCandidate) -> FeaVerdict {
    if !is_colinear_y(c) {
        return FeaVerdict { colinear: false, ..Default::default() };
    }
    let elements: Vec<BarElement> = c
        .members
        .iter()
        .map(|m| BarElement {
            from: m.from,
            to: m.to,
            youngs_modulus: c.materials[m.material].youngs_modulus as f64,
            area: m.area as f64,
            length: c.member_length(m) as f64,
        })
        .collect();
    let loads: Vec<f64> = c.nodes.iter().map(|n| n.load[1] as f64).collect();
    let fixed: Vec<usize> = c
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| !matches!(n.support, Support::Free))
        .map(|(i, _)| i)
        .collect();

    let problem = Fea1d { num_nodes: c.nodes.len(), elements, loads, fixed };
    let Some(result) = problem.solve() else {
        return FeaVerdict {
            max_utilization: f64::INFINITY,
            singular: true,
            colinear: true,
            ..Default::default()
        };
    };

    let mut max_utilization = 0.0f64;
    let mut max_abs_stress_pa = 0.0f64;
    for (m, stress) in c.members.iter().zip(result.element_stress.iter()) {
        let yield_pa = c.materials[m.material].yield_strength as f64;
        let abs = stress.abs();
        max_abs_stress_pa = max_abs_stress_pa.max(abs);
        if yield_pa > 0.0 {
            max_utilization = max_utilization.max(abs / yield_pa);
        }
    }
    let tip_displacement_m = result
        .displacements
        .iter()
        .fold(0.0f64, |acc, d| acc.max(d.abs()));

    FeaVerdict {
        max_utilization,
        max_abs_stress_pa,
        tip_displacement_m,
        singular: false,
        colinear: true,
    }
}

/// [`ClosedFormFitness`] plus a hard FEA yield gate.
///
/// Without this the optimizer shrinks every member toward zero area (see the
/// module docs). A candidate that is singular or over yield scores zero, so the
/// climb converges on the lightest section that still stands.
pub struct FeaGatedFitness {
    pub inner: ClosedFormFitness,
}

impl Default for FeaGatedFitness {
    fn default() -> Self {
        Self { inner: ClosedFormFitness::default() }
    }
}

impl Fitness for FeaGatedFitness {
    fn score(&self, c: &ArchCandidate) -> Score {
        let mut score = self.inner.score(c);
        let verdict = verify_fea(c);
        // A non-colinear candidate cannot be verified by the 1D solver, so it is
        // passed through on closed-form merit alone rather than failed outright.
        if verdict.colinear && (verdict.singular || verdict.max_utilization > 1.0) {
            score.compliance = 0.0;
            score.total = 0.0;
        }
        score
    }
}

// ---------------------------------------------------------------------------
// Messages + ledger
// ---------------------------------------------------------------------------

/// Ask the plugin to run one generate -> score -> optimize pass.
#[derive(Message, Debug, Clone)]
pub struct RunGenerativeArchEvent {
    pub seed: u64,
    pub iters: usize,
    /// Levels for the built-in demo mast (ignored when `candidate_json` is set).
    pub levels: usize,
    /// Tip load in newtons for the demo mast.
    pub tip_load_n: f32,
    /// A serialized [`ArchCandidate`] to optimize instead of the demo mast.
    pub candidate_json: Option<String>,
}

impl Default for RunGenerativeArchEvent {
    fn default() -> Self {
        Self { seed: 42, iters: 400, levels: 6, tip_load_n: 50_000.0, candidate_json: None }
    }
}

/// The outcome of one completed run.
#[derive(Message, Debug, Clone, Copy)]
pub struct GenerativeArchCompleted {
    pub run_id: u64,
    pub seed_total: f32,
    pub best_total: f32,
    pub seed_mass_kg: f32,
    pub best_mass_kg: f32,
    pub max_utilization: f64,
    /// True when the winning candidate passed the FEA gate.
    pub feasible: bool,
    pub iters: usize,
}

/// Completed runs plus the best candidate seen, so a bridge verb can read them.
#[derive(Resource, Default)]
pub struct GenerativeArchLedger {
    pub runs: Vec<GenerativeArchCompleted>,
    pub best: Option<ArchCandidate>,
    pub best_score: Option<Score>,
    pub best_fea: Option<FeaVerdict>,
}

impl GenerativeArchLedger {
    pub const CAP: usize = 256;
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn sys_run_requested(
    mut requests: MessageReader<RunGenerativeArchEvent>,
    mut ledger: ResMut<GenerativeArchLedger>,
    mut completed: MessageWriter<GenerativeArchCompleted>,
    mut notes: ResMut<crate::notifications::NotificationManager>,
) {
    for req in requests.read() {
        let seed_candidate = match req.candidate_json.as_deref() {
            Some(json) => match serde_json::from_str::<ArchCandidate>(json) {
                Ok(c) => c,
                Err(e) => {
                    warn!(target: "eustress_engine::generative_arch", error = %e, "candidate_json did not parse; skipping run");
                    continue;
                }
            },
            None => demo_mast(req.levels, req.tip_load_n),
        };

        if let Err(e) = validate_candidate(&seed_candidate) {
            warn!(target: "eustress_engine::generative_arch", error = %e, "invalid candidate; skipping run");
            continue;
        }

        let iters = req.iters.min(MAX_ITERS);
        let fitness = FeaGatedFitness::default();
        let mut optimizer = HillClimb::new(req.seed, seed_candidate.clone());
        let history = run_loop(seed_candidate.clone(), &fitness, &mut optimizer, iters);

        let Some(winner) = best(&history) else {
            warn!(target: "eustress_engine::generative_arch", "run produced no evaluations");
            continue;
        };
        let verdict = verify_fea(&winner.candidate);
        let seed_mass_kg = seed_candidate.total_mass();
        let best_mass_kg = winner.candidate.total_mass();
        let feasible = verdict.colinear && !verdict.singular && verdict.max_utilization <= 1.0;

        let record = GenerativeArchCompleted {
            run_id: winner.candidate.id,
            seed_total: history[0].score.total,
            best_total: winner.score.total,
            seed_mass_kg,
            best_mass_kg,
            max_utilization: verdict.max_utilization,
            feasible,
            iters,
        };

        info!(
            target: "eustress_engine::generative_arch",
            iters,
            seed_total = record.seed_total,
            best_total = record.best_total,
            seed_mass_kg,
            best_mass_kg,
            max_utilization = verdict.max_utilization,
            max_abs_stress_pa = verdict.max_abs_stress_pa,
            tip_displacement_m = verdict.tip_displacement_m,
            feasible,
            fea = if verdict.colinear { "verified" } else { "skipped (non-colinear)" },
            "generative-arch run complete"
        );

        let improved = ledger
            .best_score
            .map(|s| winner.score.total > s.total)
            .unwrap_or(true);
        if improved {
            ledger.best = Some(winner.candidate.clone());
            ledger.best_score = Some(winner.score);
            ledger.best_fea = Some(verdict);
        }
        ledger.runs.push(record);
        let overflow = ledger.runs.len().saturating_sub(GenerativeArchLedger::CAP);
        if overflow > 0 {
            ledger.runs.drain(0..overflow);
        }

        notes.success(format!(
            "Generative arch: {:.1} kg -> {:.1} kg ({} iters, utilization {:.2})",
            seed_mass_kg, best_mass_kg, iters, verdict.max_utilization
        ));

        completed.write(record);
    }
}

/// Fire one run at startup when `EUSTRESS_GENESIS_ITERS` is set. No-op
/// otherwise, matching the `EUSTRESS_SPLAT` demo convention.
fn sys_env_demo(mut requests: MessageWriter<RunGenerativeArchEvent>) {
    let Ok(raw) = std::env::var("EUSTRESS_GENESIS_ITERS") else { return };
    let Ok(iters) = raw.trim().parse::<usize>() else {
        warn!(target: "eustress_engine::generative_arch", value = %raw, "EUSTRESS_GENESIS_ITERS is not a number");
        return;
    };
    let seed = std::env::var("EUSTRESS_GENESIS_SEED")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(42);
    let levels = std::env::var("EUSTRESS_GENESIS_LEVELS")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(6);
    info!(target: "eustress_engine::generative_arch", iters, seed, levels, "EUSTRESS_GENESIS_ITERS set — queuing demo run");
    requests.write(RunGenerativeArchEvent { seed, iters, levels, ..Default::default() });
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

/// Headless-safe core: the loop, the ledger, and the messages. Inert until a
/// [`RunGenerativeArchEvent`] arrives.
pub struct GenerativeArchPlugin;

impl Plugin for GenerativeArchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GenerativeArchLedger>()
            .add_message::<RunGenerativeArchEvent>()
            .add_message::<GenerativeArchCompleted>()
            .add_systems(Startup, sys_env_demo)
            .add_systems(Update, sys_run_requested);
        info!(target: "eustress_engine::generative_arch", "GenerativeArchPlugin ready (set EUSTRESS_GENESIS_ITERS to run a demo)");
    }
}

/// Editor-tier overlay: draws the best candidate as gizmo lines coloured by FEA
/// utilization.
///
/// Split out of [`GenerativeArchPlugin`] because the headless shell registers no
/// `GizmoPlugin`, and a `Gizmos` system param there fails validation and
/// silently skips.
pub struct GenerativeArchGizmoPlugin;

impl Plugin for GenerativeArchGizmoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sys_draw_best_candidate);
    }
}

fn sys_draw_best_candidate(ledger: Res<GenerativeArchLedger>, mut gizmos: Gizmos) {
    let Some(candidate) = ledger.best.as_ref() else { return };
    let utilization = ledger
        .best_fea
        .map(|v| v.max_utilization.clamp(0.0, 1.0) as f32)
        .unwrap_or(0.0);
    // Green at rest, red at yield.
    let colour = Color::srgb(utilization, 1.0 - utilization, 0.2);

    for m in &candidate.members {
        // `validate_candidate` gates everything that reaches the ledger, but the
        // ledger is `pub`, so index defensively rather than trusting it.
        let (Some(a), Some(b)) = (candidate.nodes.get(m.from), candidate.nodes.get(m.to)) else {
            continue;
        };
        gizmos.line(Vec3::from_array(a.pos), Vec3::from_array(b.pos), colour);
    }
    for n in &candidate.nodes {
        let pos = Vec3::from_array(n.pos);
        if matches!(n.support, Support::Free) {
            gizmos.sphere(Isometry3d::from_translation(pos), 0.05, colour);
        } else {
            gizmos.cube(
                Transform::from_translation(pos).with_scale(Vec3::splat(0.15)),
                Color::srgb(0.2, 0.6, 1.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_out_of_range_material() {
        let mut c = demo_mast(2, 1000.0);
        c.members[0].material = 7;
        assert!(validate_candidate(&c).is_err(), "out-of-range material must be rejected");
    }

    #[test]
    fn validate_rejects_out_of_range_node() {
        let mut c = demo_mast(2, 1000.0);
        c.members[0].to = 99;
        assert!(validate_candidate(&c).is_err(), "out-of-range node must be rejected");
    }

    #[test]
    fn demo_mast_is_colinear_and_valid() {
        let c = demo_mast(6, 50_000.0);
        assert!(validate_candidate(&c).is_ok());
        let v = verify_fea(&c);
        assert!(v.colinear, "demo mast must be FEA-verifiable");
        assert!(!v.singular, "demo mast is supported, so K is non-singular");
        assert!(v.max_utilization > 0.0, "a loaded mast carries stress");
    }

    #[test]
    fn fea_gate_zeroes_an_overstressed_candidate() {
        // Same mast, areas collapsed far below what the tip load needs.
        let mut c = demo_mast(4, 50_000.0);
        for m in c.members.iter_mut() {
            m.area = 1e-6;
        }
        let v = verify_fea(&c);
        assert!(v.max_utilization > 1.0, "1e-6 m^2 under 50 kN must exceed yield");
        let gated = FeaGatedFitness::default().score(&c);
        assert_eq!(gated.total, 0.0, "the gate must zero an overstressed candidate");
        let ungated = ClosedFormFitness::default().score(&c);
        assert!(ungated.total > 0.0, "ungated fitness rewards the collapsed section");
    }

    #[test]
    fn gate_prevents_area_collapse() {
        let seed = demo_mast(6, 50_000.0);
        let fitness = FeaGatedFitness::default();
        let mut opt = HillClimb::new(42, seed.clone());
        let history = run_loop(seed.clone(), &fitness, &mut opt, 300);
        let winner = best(&history).expect("history is non-empty");
        assert!(
            winner.candidate.total_mass() < seed.total_mass(),
            "the loop should still find a lighter section"
        );
        let v = verify_fea(&winner.candidate);
        assert!(
            v.max_utilization <= 1.0,
            "the winner must be feasible, got utilization {}",
            v.max_utilization
        );
    }

    #[test]
    fn run_is_deterministic() {
        let seed = demo_mast(5, 40_000.0);
        let run = || {
            let fitness = FeaGatedFitness::default();
            let mut opt = HillClimb::new(7, seed.clone());
            best(&run_loop(seed.clone(), &fitness, &mut opt, 120))
                .expect("history is non-empty")
                .score
                .total
        };
        assert_eq!(run(), run(), "same seed must replay identically");
    }
}
