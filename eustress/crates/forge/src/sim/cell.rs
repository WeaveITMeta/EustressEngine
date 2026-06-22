//! Pure cell math + eustress-flavoured SimCell builders.
//!
//! No async, no I/O — every function here is unit-testable cheaply without
//! raft, fjall, or Bevy. This is the deterministic core of the
//! residency-cell → `forge_orchestration` SimCell mapping.

use super::{
    AgentPolicy, CellCoord, CoPlacement, Region3D, SimCell, SimWorld, CELL_EDGE_M,
};
use std::time::Duration;

// ── Cell ↔ world-space geometry ───────────────────────────────────────────
//
// Mirrors `eustress_worlddb::keys::world_to_cell` (chunk_size 256, +2^20 bias)
// in reverse so a residency cell maps back to a world-space region. The
// `+2^20` bias centres the origin mid-range; un-biasing recovers signed
// world metres.

/// Inverse of the residency `cell_of` mapping: cell → world-space MIN corner
/// (metres). `((cell - 2^20) * edge)` per axis.
pub fn cell_min_world(c: CellCoord) -> (f32, f32, f32) {
    let unbias = |v: u32| ((v as i64 - (1 << 20)) as f32) * CELL_EDGE_M;
    (unbias(c.0), unbias(c.1), unbias(c.2))
}

/// Cell CENTRE in world space — the natural anchor for a [`Region3D`] sphere.
pub fn cell_center_world(c: CellCoord) -> (f32, f32, f32) {
    let (x, y, z) = cell_min_world(c);
    let h = CELL_EDGE_M * 0.5;
    (x + h, y + h, z + h)
}

/// Half-diagonal of a `CELL_EDGE_M` cube = `(edge/2)·√3` ≈ 221.7 m for a
/// 256 m cell. The TIGHT sphere that just covers the cubic cell — the
/// default interest radius. (A looser radius matching the streaming
/// keep-box can be set per [`SimCellSpec`].)
pub fn cell_cover_radius() -> f32 {
    (CELL_EDGE_M * 0.5) * 3f32.sqrt()
}

// ── Stable, reversible cell-id strings ────────────────────────────────────
//
// `sim-cell-{x:07}-{y:07}-{z:07}` — zero-padded to 7 digits so lexical order
// matches numeric order (21-bit max is 2_097_151 = 7 digits). Reversible via
// [`parse_cell_id`] so a returned [`super::SimBinding::cell_id`] round-trips
// back to the residency cell that produced it.

/// Build the stable SimCell id for a residency cell.
pub fn cell_id(c: CellCoord) -> String {
    format!("sim-cell-{:07}-{:07}-{:07}", c.0, c.1, c.2)
}

/// Inverse of [`cell_id`]. `None` on a malformed / foreign id.
pub fn parse_cell_id(s: &str) -> Option<CellCoord> {
    let rest = s.strip_prefix("sim-cell-")?;
    let mut it = rest.split('-');
    let x = it.next()?.parse::<u32>().ok()?;
    let y = it.next()?.parse::<u32>().ok()?;
    let z = it.next()?.parse::<u32>().ok()?;
    if it.next().is_some() {
        return None; // trailing garbage
    }
    Some((x, y, z))
}

// ── Tick cadence ──────────────────────────────────────────────────────────

/// `frame_cadence(fps, cadence_frames)` → the wall-clock duration of one
/// residency cadence window (`cadence_frames / fps` seconds). This is the
/// natural [`SimCell::tick`] for an agent host driven off the engine's
/// per-N-frames residency pass (e.g. 12 frames at 60 fps = 200 ms).
pub fn frame_cadence(fps: u32, cadence_frames: u32) -> Duration {
    Duration::from_secs_f64(cadence_frames.max(1) as f64 / fps.max(1) as f64)
}

// ── Eustress agent + cell specs (pre-lowering descriptions) ───────────────

/// An eustress agent's resource footprint, BEFORE lowering to an
/// [`AgentPolicy`]. Phase 0b (Kernel + Rune DSL + validator, decision D3) is
/// the accept/reject gate that will validate the real agent shape; until then
/// agents are stub spatial-vortex defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpec {
    /// Member name (becomes the [`AgentPolicy::name`]).
    pub name: String,
    /// CPU millicores requested.
    pub cpu_millis: u64,
    /// Memory MB requested.
    pub memory_mb: u64,
    /// GPU memory MB requested (the agent runs co-located on a GPU node).
    pub gpu_memory_mb: u64,
}

impl AgentSpec {
    /// A default GPU-backed agent footprint, named by ordinal. Stand-in for
    /// the real Kernel-valid agent until Phase 0b lands.
    pub fn spatial_vortex(ordinal: usize) -> Self {
        Self {
            name: format!("vortex-{ordinal:02}"),
            cpu_millis: 250,
            memory_mb: 256,
            gpu_memory_mb: 1024,
        }
    }
}

/// Eustress description of a sim-worthy residency cell, BEFORE lowering to a
/// [`SimCell`]. Carries everything the mapping needs; sane defaults via
/// [`SimCellSpec::new`].
#[derive(Debug, Clone)]
pub struct SimCellSpec {
    /// The residency cell this sim hosts.
    pub cell: CellCoord,
    /// World process CPU millicores.
    pub world_cpu_millis: u64,
    /// World process memory MB.
    pub world_memory_mb: u64,
    /// Interest sphere radius in metres — either [`cell_cover_radius`] (tight)
    /// or the streaming evict radius (loose). Tunable so a Region3D-overlap
    /// penalty can be dialled out in the Matrix test.
    pub interest_radius: f32,
    /// Agents co-placed with the world.
    pub agents: Vec<AgentSpec>,
    /// Scheduler tick cadence for the gang.
    pub tick: Duration,
    /// Gang priority (higher wins under preemption).
    pub priority: i32,
}

impl SimCellSpec {
    /// A spec with sane defaults for `cell`: 500 m CPU / 256 MB world, tight
    /// cover radius, 12-frames-at-60-fps tick, priority 0, no agents.
    pub fn new(cell: CellCoord) -> Self {
        Self {
            cell,
            world_cpu_millis: 500,
            world_memory_mb: 256,
            interest_radius: cell_cover_radius(),
            agents: Vec::new(),
            tick: frame_cadence(60, 12),
            priority: 0,
        }
    }

    /// Push `n` stub spatial-vortex agents (convenience for the demo/tests).
    pub fn with_vortex_agents(mut self, n: usize) -> Self {
        for ordinal in 0..n {
            self.agents.push(AgentSpec::spatial_vortex(ordinal));
        }
        self
    }

    /// Total gang members = 1 world + N agents. The all-or-nothing
    /// assertion target ([`super::driver::binding_is_complete`]).
    pub fn member_count(&self) -> usize {
        1 + self.agents.len()
    }
}

// ── THE CORE MAPPING: residency cell → forge SimCell + Region3D ───────────

/// Lower a [`SimCellSpec`] into a `forge_orchestration` [`SimCell`].
///
/// Verified 0.6.0 builder chain:
/// `SimCell::new(id, SimWorld::cpu(cpu_millis, memory_mb), tick)` →
/// `.with_region(Region3D::new(x, y, z, radius))` (all **f64**) →
/// `.with_co_placement(CoPlacement::InterconnectLocalGpu)` →
/// `.with_priority(i32)` → loop `.with_agent(AgentPolicy::gpu(name, cpu, mem,
/// gpu_mem))` (all **u64**). `tick` is a [`std::time::Duration`];
/// `next_deadline` is defaulted by `SimCell::new`.
pub fn to_sim_cell(spec: &SimCellSpec) -> SimCell {
    let (cx, cy, cz) = cell_center_world(spec.cell);
    let mut sc = SimCell::new(
        cell_id(spec.cell),
        SimWorld::cpu(spec.world_cpu_millis, spec.world_memory_mb),
        spec.tick,
    )
    .with_region(Region3D::new(
        cx as f64,
        cy as f64,
        cz as f64,
        spec.interest_radius as f64,
    ))
    .with_co_placement(CoPlacement::InterconnectLocalGpu)
    .with_priority(spec.priority);

    for a in &spec.agents {
        sc = sc.with_agent(AgentPolicy::gpu(
            a.name.clone(),
            a.cpu_millis,
            a.memory_mb,
            a.gpu_memory_mb,
        ));
    }
    sc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_id_roundtrips() {
        for c in [(0, 0, 0), (1, 2, 3), (1_048_576, 7, 99), (2_097_151, 0, 2_097_151)] {
            let id = cell_id(c);
            assert_eq!(parse_cell_id(&id), Some(c), "roundtrip {c:?}");
        }
    }

    #[test]
    fn parse_cell_id_rejects_foreign() {
        assert_eq!(parse_cell_id("forge/jobs/x"), None);
        assert_eq!(parse_cell_id("sim-cell-1-2"), None); // too few
        assert_eq!(parse_cell_id("sim-cell-1-2-3-4"), None); // too many
        assert_eq!(parse_cell_id("sim-cell-a-b-c"), None); // non-numeric
    }

    #[test]
    fn lexical_order_matches_numeric() {
        // Zero-padding to 7 digits keeps lexical == numeric within an axis.
        assert!(cell_id((1, 0, 0)) < cell_id((2, 0, 0)));
        assert!(cell_id((9, 0, 0)) < cell_id((10, 0, 0)));
    }

    #[test]
    fn center_is_min_plus_half_edge() {
        let c = (1_048_576, 1_048_576, 1_048_576); // the biased origin cell
        let (mnx, mny, mnz) = cell_min_world(c);
        // Origin cell min corner sits at world (0,0,0).
        assert!(mnx.abs() < 0.001 && mny.abs() < 0.001 && mnz.abs() < 0.001);
        let (cxx, cyy, czz) = cell_center_world(c);
        let h = CELL_EDGE_M * 0.5;
        assert!((cxx - h).abs() < 0.001 && (cyy - h).abs() < 0.001 && (czz - h).abs() < 0.001);
    }

    #[test]
    fn cover_radius_is_half_diagonal() {
        // 128 * sqrt(3) ≈ 221.7025 for a 256 m cell.
        let r = cell_cover_radius();
        assert!((r - 221.7025).abs() < 0.01, "got {r}");
    }

    #[test]
    fn frame_cadence_math() {
        assert_eq!(frame_cadence(60, 12), Duration::from_secs_f64(0.2));
        assert_eq!(frame_cadence(60, 60), Duration::from_secs(1));
        // Guards against divide-by-zero.
        assert_eq!(frame_cadence(0, 0), Duration::from_secs_f64(1.0));
    }

    #[test]
    fn to_sim_cell_maps_fields() {
        let spec = SimCellSpec::new((100, 200, 300)).with_vortex_agents(2);
        let sc = to_sim_cell(&spec);

        // id round-trips back to the cell.
        assert_eq!(parse_cell_id(&sc.id), Some((100, 200, 300)));

        // region centred on the cell centre, tight radius, GPU co-placement.
        let region = sc.region.expect("region set");
        let (cx, cy, cz) = cell_center_world((100, 200, 300));
        assert!((region.x - cx as f64).abs() < 0.001);
        assert!((region.y - cy as f64).abs() < 0.001);
        assert!((region.z - cz as f64).abs() < 0.001);
        assert!((region.radius - spec.interest_radius as f64).abs() < 0.001);
        assert_eq!(sc.co_placement, CoPlacement::InterconnectLocalGpu);

        // agents lowered 1:1; member_count = 1 world + 2 agents.
        assert_eq!(sc.agents.len(), 2);
        assert_eq!(sc.member_count(), spec.member_count());
        assert_eq!(spec.member_count(), 3);
    }

    /// GUARD: [`CELL_EDGE_M`] must stay locked to the worlddb encoder's chunk
    /// size, and the bias must stay 2^20. If a worlddb const changes, this
    /// breaks loudly instead of silently mis-mapping every cell.
    #[test]
    fn cell_edge_matches_worlddb_chunk_size() {
        assert_eq!(CELL_EDGE_M, super::super::worlddb_chunk_size());
        // The +2^20 bias is also load-bearing: encode a known world position
        // through worlddb and confirm cell_min_world inverts it.
        let cell_x = eustress_worlddb::keys::world_to_cell(0.0, CELL_EDGE_M);
        assert_eq!(cell_x, 1 << 20, "origin must land in the biased centre cell");
        let (mnx, _, _) = cell_min_world((cell_x, cell_x, cell_x));
        assert!(mnx.abs() < CELL_EDGE_M, "origin cell min near world origin");
    }
}
