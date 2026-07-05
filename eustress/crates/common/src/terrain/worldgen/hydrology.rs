//! Pass 2 — hydrology: depression filling + D8 flow routing + accumulation.
//!
//! Turns a raw heightfield into a drainage model:
//!
//! 1. [`fill_depressions`] — **priority-flood** (Barnes et al. 2014) with an
//!    epsilon gradient, so every cell has a strictly-descending path to the
//!    grid edge. No pits ⇒ no undrained artifacts, and the epsilon keeps
//!    flow directions well-defined on flats.
//! 2. D8 flow direction — each cell drains to its steepest-descent neighbour
//!    (8-connected), computed on the *filled* surface. Near-ties (all
//!    descents within [`TIE_BAND`] of the steepest) are resolved by a
//!    **salted per-cell hash** pick among the tied candidates, and every
//!    descent comparison carries a tiny deterministic per-(cell, direction)
//!    multiplicative jitter ([`DIR_JITTER_AMP`]). Both are pure functions of
//!    the cell index — fully deterministic — and exist to kill the
//!    lowest-index bias that used to lock long runs into one cardinal
//!    direction on smooth filled surfaces (dead-straight channels,
//!    90-degree junctions, comb-teeth parallel tributaries).
//! 3. Flow accumulation — topological (in-degree queue) traversal; every cell
//!    contributes `1.0` plus any [`BoundaryInflow`] injected at the region
//!    edge by the coarse world pass (that is how a river *entering* a region
//!    carries its upstream discharge across the border).
//!
//! **Inflow coordinate convention:** `BoundaryInflow::{ix, iz}` are LOCAL to
//! the grid passed to [`compute_flow`]. When the pipeline generates a region
//! on an apron-expanded grid it offsets the fine-coordinate inflows by
//! `APRON` *before* calling in — this module never offsets.
//!
//! Determinism contract: no HashMap iteration, no RNG — `Vec` + fixed
//! iteration orders (row-major cells, [`D8_OFFSETS`] neighbours, slice-order
//! inflows), total-ordered heap tie-breaks, and pure-hash direction
//! tie-breaks/jitter (stateless functions of the cell index). Same inputs ⇒
//! bit-identical [`FlowField`].

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, VecDeque};

/// D8 neighbour offsets, index 0..8 → (dx, dz). Order is FIXED (part of the
/// determinism contract and the meaning of `FlowField::dirs`).
pub const D8_OFFSETS: [(i32, i32); 8] = [
    (1, 0),   // 0 E
    (1, 1),   // 1 SE
    (0, 1),   // 2 S
    (-1, 1),  // 3 SW
    (-1, 0),  // 4 W
    (-1, -1), // 5 NW
    (0, -1),  // 6 N
    (1, -1),  // 7 NE
];

/// Grid-step length per D8 direction (cell-size units): `1` for cardinals,
/// `√2` for diagonals. Divide height drops by this (× cell size in metres)
/// to compare slopes fairly across cardinal/diagonal neighbours.
pub const D8_DIST: [f32; 8] = [
    1.0,
    std::f32::consts::SQRT_2,
    1.0,
    std::f32::consts::SQRT_2,
    1.0,
    std::f32::consts::SQRT_2,
    1.0,
    std::f32::consts::SQRT_2,
];

/// Sentinel direction: cell is an outlet (drains off the grid edge).
pub const DIR_OUTLET: u8 = 8;

/// Epsilon gradient (metres) applied while flooding so filled flats become
/// strictly-descending staircases. Small enough not to distort terrain,
/// large enough that f32 addition still changes the value on typical height
/// magnitudes (heights ≲ 2 km — above that the [`bump`] `next_up` guard
/// keeps the gradient strict, but it stops being geometrically meaningful).
const FILL_EPSILON: f32 = 1e-3;

/// Strictly increase `x` by the epsilon gradient. The `next_up` arm
/// guarantees strict monotonicity even where `x + FILL_EPSILON` would round
/// back to `x` (ULP at 8192 m is ~9.8e-4).
#[inline]
fn bump(x: f32) -> f32 {
    (x + FILL_EPSILON).max(x.next_up())
}

/// Descents within this margin of the steepest (per-distance-normalised
/// drop units) count as tied; the salted hash picks among them. A few ×
/// [`FILL_EPSILON`] so it engages on filled-flat epsilon staircases (where
/// the old lowest-index rule printed Manhattan drainage) but is invisible
/// on real slopes.
const TIE_BAND: f32 = 4.0 * FILL_EPSILON;

/// Amplitude of the deterministic per-(cell, direction) multiplicative
/// jitter on the descent comparison (±fraction). Small enough that clearly
/// distinct descents never reorder (cardinal vs diagonal on a uniform plane
/// differ by 29%), big enough to break the cardinal lock where two D8
/// sectors are near-equally steep (smooth dome flanks — the source of the
/// dead-straight parallel-rill "comb teeth" drainage).
const DIR_JITTER_AMP: f32 = 0.04;

/// Salt for the tie-break / jitter hashes (arbitrary odd constant — part of
/// the deterministic output contract).
const TIE_SALT: u64 = 0xD1CE_0FF5_EED0_0D8A;

/// SplitMix64-style finaliser: pure integer hash — the deterministic
/// stand-in for randomness this module is allowed (no RNG, no state).
#[inline]
fn mix64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    x
}

/// Deterministic jitter factor in `[1 − AMP, 1 + AMP]` for (cell, dir).
#[inline]
fn dir_jitter(c: usize, d: usize) -> f32 {
    let h = mix64((c as u64).wrapping_mul(8).wrapping_add(d as u64) ^ TIE_SALT);
    let unit = (h >> 11) as f32 / (1u64 << 53) as f32; // [0,1)
    1.0 + DIR_JITTER_AMP * (2.0 * unit - 1.0)
}

/// Discharge injected at a border cell by the coarse pass — the upstream
/// flow of a river that crosses into this region.
///
/// Coordinates are LOCAL to the grid handed to [`compute_flow`] (the
/// pipeline applies any apron offset before calling — see module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryInflow {
    pub ix: u32,
    pub iz: u32,
    /// Upstream accumulation (in cell-count-equivalent units) to add.
    pub discharge: f32,
}

/// The drainage model for one grid.
#[derive(Clone, Debug)]
pub struct FlowField {
    pub res_x: u32,
    pub res_z: u32,
    /// Depression-filled heights (metres) the directions were derived from.
    pub filled: Vec<f32>,
    /// D8 direction per cell (`0..8` into [`D8_OFFSETS`], [`DIR_OUTLET`] at
    /// outlets). [`compute_flow`] never emits an in-range direction whose
    /// target is off-grid — off-grid descent is always expressed as
    /// [`DIR_OUTLET`], so `dirs` is self-consistent.
    pub dirs: Vec<u8>,
    /// Upstream accumulation per cell (own cell = 1.0, plus inflows). Stays
    /// in raw cell-count units — the material pass thresholds depend on it.
    pub accum: Vec<f32>,
}

impl FlowField {
    #[inline]
    pub fn idx(&self, ix: u32, iz: u32) -> usize {
        (iz as usize) * (self.res_x as usize) + ix as usize
    }

    /// Downstream neighbour of a cell, or `None` at an outlet.
    pub fn downstream(&self, ix: u32, iz: u32) -> Option<(u32, u32)> {
        let d = self.dirs[self.idx(ix, iz)];
        if d >= 8 {
            return None;
        }
        let (dx, dz) = D8_OFFSETS[d as usize];
        let nx = ix as i64 + dx as i64;
        let nz = iz as i64 + dz as i64;
        if nx < 0 || nz < 0 || nx >= self.res_x as i64 || nz >= self.res_z as i64 {
            None
        } else {
            Some((nx as u32, nz as u32))
        }
    }

    /// Downstream neighbour as a linear cell index, or `None` at an outlet.
    ///
    /// The bounds check is a dead safety net (see `dirs` docs) — it exists
    /// for defence, not as a semantic channel.
    #[inline]
    pub fn downstream_index(&self, c: usize) -> Option<usize> {
        downstream_of(&self.dirs, self.res_x, self.res_z, c)
    }
}

/// Decode `dirs[c]` into a downstream linear index (`None` at outlets or —
/// dead safety net — off-grid targets).
#[inline]
fn downstream_of(dirs: &[u8], res_x: u32, res_z: u32, c: usize) -> Option<usize> {
    let d = dirs[c];
    if d >= 8 {
        return None;
    }
    let (dx, dz) = D8_OFFSETS[d as usize];
    let rx = res_x as usize;
    let nx = (c % rx) as i64 + dx as i64;
    let nz = (c / rx) as i64 + dz as i64;
    if nx < 0 || nz < 0 || nx >= res_x as i64 || nz >= res_z as i64 {
        None
    } else {
        Some((nz as usize) * rx + nx as usize)
    }
}

/// Heap entry with a TOTAL order: height ascending (via `total_cmp`), ties
/// broken by linear cell index ascending. `idx` is unique per entry, so the
/// order is total and `BinaryHeap` pop order is fully determined — this is
/// the deterministic tie-break the module contract requires.
#[derive(Clone, Copy, Debug)]
struct HeapCell {
    h: f32,
    idx: u32,
}

impl PartialEq for HeapCell {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for HeapCell {}
impl PartialOrd for HeapCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapCell {
    fn cmp(&self, other: &Self) -> Ordering {
        self.h.total_cmp(&other.h).then(self.idx.cmp(&other.idx))
    }
}

/// Priority-flood depression filling with epsilon gradient.
///
/// Seeds a min-heap with every border cell (kept at raw height), then floods
/// inward: when a cell is first reached from a popped neighbour `c` its
/// final height is fixed at `max(own_height, bump(filled[c]))`, where
/// [`bump`] adds [`FILL_EPSILON`] with a `next_up` guard so the gradient is
/// strict even at large magnitudes. Every cell is visited exactly once, in
/// ascending filled-height order (heap ties → lowest linear cell index), so
/// the result is bit-deterministic and every non-border cell ends up with a
/// strictly-descending path to the grid edge.
///
/// Grids with `res_x < 3 || res_z < 3` are all-border by definition and are
/// returned unchanged.
pub fn fill_depressions(heights: &[f32], res_x: u32, res_z: u32) -> Vec<f32> {
    let n = (res_x as usize) * (res_z as usize);
    debug_assert_eq!(heights.len(), n, "heights must be res_x*res_z");
    if res_x < 3 || res_z < 3 {
        // Every cell is a border cell — nothing to fill.
        return heights.to_vec();
    }

    let rx = res_x as usize;
    let mut filled = heights.to_vec();
    let mut visited = vec![false; n];
    let mut heap: BinaryHeap<Reverse<HeapCell>> = BinaryHeap::with_capacity(n);

    // Seed: every border cell, in row-major order, at its raw height.
    for iz in 0..res_z {
        for ix in 0..res_x {
            if ix == 0 || ix == res_x - 1 || iz == 0 || iz == res_z - 1 {
                let i = (iz as usize) * rx + ix as usize;
                visited[i] = true;
                heap.push(Reverse(HeapCell {
                    h: heights[i],
                    idx: i as u32,
                }));
            }
        }
    }

    // Flood inward, lowest first.
    while let Some(Reverse(cell)) = heap.pop() {
        let ci = cell.idx as usize;
        let cx = (ci % rx) as i64;
        let cz = (ci / rx) as i64;
        for (dx, dz) in D8_OFFSETS {
            let nx = cx + dx as i64;
            let nz = cz + dz as i64;
            if nx < 0 || nz < 0 || nx >= res_x as i64 || nz >= res_z as i64 {
                continue;
            }
            let ni = (nz as usize) * rx + nx as usize;
            if visited[ni] {
                continue;
            }
            visited[ni] = true;
            filled[ni] = heights[ni].max(bump(filled[ci]));
            heap.push(Reverse(HeapCell {
                h: filled[ni],
                idx: ni as u32,
            }));
        }
    }

    filled
}

/// Full hydrology pass: fill → D8 directions → accumulation (with boundary
/// inflows). See module docs for the algorithm and determinism contract.
///
/// Directions are chosen on the **filled** surface among strictly-lower
/// neighbours only (`drop > 0`), jittered per (cell, direction) by
/// [`dir_jitter`]; all candidates within [`TIE_BAND`] of the steepest form
/// the tie set and a salted per-cell hash picks one (fixed [`D8_OFFSETS`]
/// candidate order ⇒ deterministic). Cells with no strictly-lower neighbour
/// become [`DIR_OUTLET`]. Because the filled surface has strict epsilon
/// gradients, interior cells always drain; only border-local-minima become
/// outlets. An edge exists only where the filled surface strictly decreases
/// (candidacy requires the RAW drop > 0), so the flow graph is acyclic by
/// construction.
///
/// Accumulation is `1.0` per cell plus `inflows` (added in slice order,
/// coordinates local to THIS grid), propagated downstream by an in-degree
/// FIFO queue seeded in ascending cell-index order — a fully deterministic
/// topological summation.
pub fn compute_flow(
    heights: &[f32],
    res_x: u32,
    res_z: u32,
    inflows: &[BoundaryInflow],
) -> FlowField {
    let n = (res_x as usize) * (res_z as usize);
    debug_assert_eq!(heights.len(), n, "heights must be res_x*res_z");
    let rx = res_x as usize;

    let filled = fill_depressions(heights, res_x, res_z);

    // --- D8 steepest descent on the filled surface -----------------------
    // Candidates = strictly-lower neighbours (raw drop > 0 ⇒ acyclic by
    // construction). The comparison uses jittered drops; every candidate
    // within TIE_BAND of the jittered best forms the tie set, resolved by a
    // salted per-cell hash (fixed D8 order ⇒ deterministic). See module docs
    // for why this replaced the old lowest-index tie-break.
    let mut dirs = vec![DIR_OUTLET; n];
    for iz in 0..res_z as i64 {
        for ix in 0..res_x as i64 {
            let c = (iz as usize) * rx + ix as usize;
            let hc = filled[c];
            let mut drops = [0.0f32; 8]; // jittered; 0 = not a candidate
            let mut best_drop = 0.0f32;
            for (d, (dx, dz)) in D8_OFFSETS.iter().enumerate() {
                let nx = ix + *dx as i64;
                let nz = iz + *dz as i64;
                if nx < 0 || nz < 0 || nx >= res_x as i64 || nz >= res_z as i64 {
                    continue; // off-grid descent is expressed as DIR_OUTLET
                }
                let ni = (nz as usize) * rx + nx as usize;
                // cell_size cancels for direction CHOICE — only the
                // cardinal/diagonal ratio matters here.
                let drop = (hc - filled[ni]) / D8_DIST[d];
                if drop > 0.0 {
                    let dj = drop * dir_jitter(c, d);
                    drops[d] = dj;
                    if dj > best_drop {
                        best_drop = dj;
                    }
                }
            }
            if best_drop > 0.0 {
                // Tie set in fixed D8 order; hash pick among its members.
                let floor = best_drop - TIE_BAND;
                let mut tied = [0u8; 8];
                let mut count = 0usize;
                for (d, &dj) in drops.iter().enumerate() {
                    if dj > 0.0 && dj >= floor {
                        tied[count] = d as u8;
                        count += 1;
                    }
                }
                debug_assert!(count >= 1, "best_drop > 0 implies a candidate");
                let pick = (mix64(c as u64 ^ TIE_SALT) % count as u64) as usize;
                dirs[c] = tied[pick];
            }
        }
    }

    // --- Flow accumulation (O(n) topological, in-degree queue) -----------
    let mut accum = vec![1.0f32; n];
    for inflow in inflows {
        debug_assert!(
            inflow.ix < res_x && inflow.iz < res_z,
            "inflow ({}, {}) outside {}x{} grid",
            inflow.ix,
            inflow.iz,
            res_x,
            res_z
        );
        accum[(inflow.iz as usize) * rx + inflow.ix as usize] += inflow.discharge;
    }

    let mut indeg = vec![0u32; n];
    for c in 0..n {
        if let Some(ni) = downstream_of(&dirs, res_x, res_z, c) {
            indeg[ni] += 1;
        }
    }
    let mut queue: VecDeque<u32> = VecDeque::with_capacity(n);
    for c in 0..n {
        if indeg[c] == 0 {
            queue.push_back(c as u32);
        }
    }
    let mut processed = 0usize;
    while let Some(c) = queue.pop_front() {
        processed += 1;
        let c = c as usize;
        if let Some(ni) = downstream_of(&dirs, res_x, res_z, c) {
            accum[ni] += accum[c];
            indeg[ni] -= 1;
            if indeg[ni] == 0 {
                queue.push_back(ni as u32);
            }
        }
    }
    debug_assert_eq!(
        processed, n,
        "flow graph must be acyclic — every cell drains to an outlet"
    );

    FlowField {
        res_x,
        res_z,
        filled,
        dirs,
        accum,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::worldgen::{generate_base, GenParams};

    fn lin(ix: u32, iz: u32, res_x: u32) -> usize {
        (iz as usize) * (res_x as usize) + ix as usize
    }

    /// Realistic rugged heights from pass 1 (global field, deterministic).
    fn base_heights(seed: u64, res: u32) -> Vec<f32> {
        generate_base(&GenParams {
            seed,
            res_x: res,
            res_z: res,
            size_x: 256.0,
            size_z: 256.0,
            ridge_blend: 0.7,
            ..Default::default()
        })
        .heights
    }

    /// Steps for `start` to reach an outlet; panics if it exceeds `n`
    /// (i.e. a cycle or an undrained pit survived).
    fn steps_to_outlet(flow: &FlowField, start: usize) -> usize {
        let n = flow.filled.len();
        let mut c = start;
        for step in 0..=n {
            match flow.downstream_index(c) {
                None => return step,
                Some(nc) => c = nc,
            }
        }
        panic!("cell {start} did not reach an outlet within {n} steps");
    }

    /// Border ring at 10 m, interior shelf at 1 m, dead-centre pit at 0 m —
    /// a bowl that MUST be filled for anything to drain.
    fn bowl(res: u32) -> Vec<f32> {
        let mut h = vec![1.0f32; (res * res) as usize];
        for iz in 0..res {
            for ix in 0..res {
                if ix == 0 || ix == res - 1 || iz == 0 || iz == res - 1 {
                    h[lin(ix, iz, res)] = 10.0;
                }
            }
        }
        h[lin(res / 2, res / 2, res)] = 0.0;
        h
    }

    #[test]
    fn fill_never_lowers_any_cell() {
        for heights in [bowl(17), base_heights(42, 48)] {
            let res = (heights.len() as f64).sqrt() as u32;
            let filled = fill_depressions(&heights, res, res);
            for i in 0..heights.len() {
                assert!(
                    filled[i] >= heights[i],
                    "fill lowered cell {i}: {} -> {}",
                    heights[i],
                    filled[i]
                );
            }
        }
    }

    #[test]
    fn bowl_fills_to_a_drained_staircase() {
        let res = 17u32;
        let flow = compute_flow(&bowl(res), res, res, &[]);
        // Every cell (including the former pit) reaches an outlet.
        for c in 0..flow.filled.len() {
            steps_to_outlet(&flow, c);
        }
        // Interior cells always find strict descent on the filled surface —
        // only border cells may be outlets.
        for iz in 1..res - 1 {
            for ix in 1..res - 1 {
                let c = lin(ix, iz, res);
                assert!(
                    flow.dirs[c] < 8,
                    "interior cell ({ix},{iz}) is an outlet — pit survived fill"
                );
            }
        }
    }

    #[test]
    fn dirs_strictly_descend_on_filled_surface() {
        let res = 48u32;
        let heights = base_heights(7, res);
        let flow = compute_flow(&heights, res, res, &[]);
        for c in 0..heights.len() {
            if let Some(ni) = flow.downstream_index(c) {
                assert!(
                    flow.filled[c] > flow.filled[ni],
                    "edge {c}->{ni} does not descend: {} <= {}",
                    flow.filled[c],
                    flow.filled[ni]
                );
            }
        }
    }

    #[test]
    fn hydrology_is_bit_deterministic() {
        let res = 48u32;
        let heights = base_heights(1234, res);
        let inflows = [
            BoundaryInflow { ix: 0, iz: 20, discharge: 5000.0 },
            BoundaryInflow { ix: 11, iz: 0, discharge: 2500.0 },
        ];
        let a = compute_flow(&heights, res, res, &inflows);
        let b = compute_flow(&heights, res, res, &inflows);
        assert_eq!(a.dirs, b.dirs, "dirs differ between identical runs");
        for i in 0..heights.len() {
            assert_eq!(a.filled[i].to_bits(), b.filled[i].to_bits(), "filled[{i}]");
            assert_eq!(a.accum[i].to_bits(), b.accum[i].to_bits(), "accum[{i}]");
            assert!(a.filled[i].is_finite(), "filled[{i}] not finite");
            assert!(a.accum[i].is_finite(), "accum[{i}] not finite");
        }
    }

    #[test]
    fn outlet_accumulation_conserves_mass() {
        let res = 64u32;
        let heights = base_heights(99, res);
        let inflows = [
            BoundaryInflow { ix: 0, iz: 10, discharge: 5000.0 },
            BoundaryInflow { ix: 3, iz: 0, discharge: 2500.0 },
        ];
        let flow = compute_flow(&heights, res, res, &inflows);
        let n = heights.len();
        let outlet_sum: f64 = (0..n)
            .filter(|&c| flow.dirs[c] == DIR_OUTLET)
            .map(|c| flow.accum[c] as f64)
            .sum();
        let expected = n as f64 + 5000.0 + 2500.0;
        let rel = (outlet_sum - expected).abs() / expected;
        assert!(
            rel < 1e-3,
            "outlet mass {outlet_sum} != expected {expected} (rel err {rel})"
        );
    }

    #[test]
    fn boundary_inflow_propagates_downstream() {
        // Plane tilted 1 m per cell toward +X: flow is due E everywhere,
        // last column outlets, so accum(ix, iz) = ix + 1 exactly — plus the
        // injected discharge everywhere at/downstream of the inflow cell.
        let (res_x, res_z) = (16u32, 8u32);
        let mut heights = vec![0.0f32; (res_x * res_z) as usize];
        for iz in 0..res_z {
            for ix in 0..res_x {
                heights[lin(ix, iz, res_x)] = (res_x - 1 - ix) as f32;
            }
        }
        let inflow = BoundaryInflow { ix: 2, iz: 5, discharge: 100.0 };
        let flow = compute_flow(&heights, res_x, res_z, &[inflow]);

        for ix in 0..res_x - 1 {
            assert_eq!(flow.dirs[lin(ix, 5, res_x)], 0, "row 5 must flow due E");
        }
        assert_eq!(flow.dirs[lin(res_x - 1, 5, res_x)], DIR_OUTLET);

        // Upstream of the injection: untouched.
        assert_eq!(flow.accum[lin(1, 5, res_x)], 2.0);
        // At and downstream of the injection: discharge is carried along.
        assert_eq!(flow.accum[lin(2, 5, res_x)], 3.0 + 100.0);
        assert_eq!(flow.accum[lin(10, 5, res_x)], 11.0 + 100.0);
        assert_eq!(flow.accum[lin(res_x - 1, 5, res_x)], res_x as f32 + 100.0);
        // Other rows: unaffected.
        assert_eq!(flow.accum[lin(10, 4, res_x)], 11.0);
    }

    #[test]
    fn d8_ties_break_deterministically_via_salted_hash() {
        // UPDATED for the judge fix replacing lowest-index tie-breaking (it
        // printed Manhattan drainage): ties now resolve by a salted per-cell
        // hash. 3x3 spike: centre 1 m above a flat ring — all four cardinal
        // drops tie at 1.0 (> diagonal 1/sqrt(2), far outside the jitter
        // band), so the pick must be ONE of the cardinals, and the same one
        // on every run.
        let mut heights = [0.0f32; 9];
        heights[4] = 1.0;
        let a = compute_flow(&heights, 3, 3, &[]);
        let b = compute_flow(&heights, 3, 3, &[]);
        assert!(
            [0u8, 2, 4, 6].contains(&a.dirs[4]),
            "cardinal tie must resolve to a cardinal, got {}",
            a.dirs[4]
        );
        assert_eq!(a.dirs[4], b.dirs[4], "hash tie-break must be deterministic");
    }

    #[test]
    fn flat_grid_is_stable_and_drains() {
        // A perfectly flat field: fill turns it into an epsilon staircase
        // descending toward the border; the result must be deterministic and
        // fully drained.
        let res = 9u32;
        let heights = vec![5.0f32; (res * res) as usize];
        let a = compute_flow(&heights, res, res, &[]);
        let b = compute_flow(&heights, res, res, &[]);
        assert_eq!(a.dirs, b.dirs, "flat-grid tie-breaks unstable");
        for c in 0..heights.len() {
            steps_to_outlet(&a, c);
        }
        for iz in 1..res - 1 {
            for ix in 1..res - 1 {
                assert!(a.dirs[lin(ix, iz, res)] < 8, "interior flat cell stuck");
            }
        }
    }
}
