//! Off-thread processing of a freshly loaded splat cloud, run once on the
//! [`AsyncComputeTaskPool`](bevy::tasks::AsyncComputeTaskPool): floater cull,
//! optional splat budget, and the collider fit. The cloud arrives by value
//! and is filtered in place on its planar storage, so a multi-gigabyte cloud
//! is never copied. Pure functions with no ECS access.

use super::{collider, warn, ColliderStrategy, CompoundProxy, Instant, PlanarGaussian3d};

/// Floater cull grid resolution: how many voxels span the cloud's ROBUST
/// (percentile) extent, which sets the adaptive voxel size
/// (`robust_extent / GRID_RES`). A real floater is a splat that sits ALONE in
/// empty space; binning to a grid and asking "how much stuff shares my
/// neighbourhood?" separates isolated specks from dense-but-faint surface detail
/// (tree foliage) that a pure opacity cut wiped. The ROBUST box (not raw
/// min/max) is essential: one stray gaussian in the far background/sky shell
/// otherwise inflates the extent and neuters the cull. ~128 lands near 0.65 m
/// voxels on a MipNeRF360-scale capture — coarse enough that dense foliage is
/// never fragmented. Env override: `EUSTRESS_SPLAT_CULL_GRID`.
const FLOATER_GRID_RES: f32 = 128.0;

/// A splat survives only if the opacity-WEIGHTED mass in its 3×3×3 voxel
/// neighbourhood clears this. Mass (Σ post-sigmoid opacity), NOT raw count, so a
/// puff of near-invisible mist-gaussians (count-dense but mass-light) is culled
/// while a small solid object or layered low-alpha foliage (mass-heavy) survives.
/// GENTLE default — expect ~1% removal on a clean capture; raise for a more
/// aggressive cull. Env override: `EUSTRESS_SPLAT_CULL_MIN_MASS`.
const FLOATER_MIN_MASS: f32 = 2.5;

/// Unconditional dust floor: a splat with post-sigmoid opacity below this is
/// removed regardless of isolation. The official 3DGS trainer prunes at ~1/255
/// alpha, so anything fainter in a shipped `.ply` is dust, never real surface —
/// safe to drop everywhere (never touches foliage, which is far more opaque).
/// Env override: `EUSTRESS_SPLAT_CULL_DUST`.
const FLOATER_DUST_OPACITY: f32 = 0.005;

/// Safety cap: if the parameters would remove MORE than this fraction of the
/// cloud, treat them as mis-tuned and SKIP the isolation cull (the dust floor
/// still applies) — better to leave floaters than to wipe the scene. Retriable
/// by toggling `cull_floaters`. Env override: `EUSTRESS_SPLAT_CULL_MAX_REMOVE`.
const FLOATER_MAX_REMOVE_FRAC: f32 = 0.15;

/// Minimum effective opacity a splat must have to contribute to the collider,
/// so faint splats do not inflate the proxy.
const COLLIDER_MIN_OPACITY: f32 = 0.08;

/// What to do with one cloud.
pub(crate) struct ProcessRequest {
    /// Source path, for log lines only.
    pub source: String,
    pub cull_floaters: bool,
    /// Fit a collider proxy (false once the entity already has one).
    pub collider: bool,
}

/// A fitted collider proxy plus the numbers for its log line.
pub(crate) struct ColliderFit {
    pub proxy: CompoundProxy,
    pub voxel: f32,
    pub extent: f32,
    pub solid: usize,
}

pub(crate) struct Processed {
    pub cloud: PlanarGaussian3d,
    /// Whether the floater cull ran (it may still have removed nothing).
    pub culled: bool,
    pub collider: Option<ColliderFit>,
}

pub(crate) fn process_cloud(mut cloud: PlanarGaussian3d, req: ProcessRequest) -> Processed {
    let started = Instant::now();
    if req.cull_floaters {
        if let Some(keep) = floater_keep_mask(&cloud, &req.source) {
            retain_by_mask(&mut cloud, &keep);
        }
    }
    apply_budget(&mut cloud, &req.source);
    let cull_secs = started.elapsed().as_secs_f32();

    let fit_started = Instant::now();
    let collider = req.collider.then(|| fit_collider(&cloud));
    if req.cull_floaters || req.collider {
        warn!(
            "splat process: {} gaussians from {} (cull {:.2}s, collider {:.2}s)",
            cloud.position_visibility.len(),
            req.source,
            cull_secs,
            fit_started.elapsed().as_secs_f32(),
        );
    }
    Processed { cloud, culled: req.cull_floaters, collider }
}

/// Post-sigmoid opacity of every splat, clamped to [0, 1]. Clouds read from
/// `.ply` already hold activated opacity (the loader applies the sigmoid); a
/// `.gcloud` from another tool may hold raw logits, which a bounded sample
/// detects (any value outside [0, 1]).
fn effective_opacity(cloud: &PlanarGaussian3d) -> Vec<f32> {
    let so = &cloud.scale_opacity;
    let looks_logit = so.iter().take(4096).any(|s| s.opacity < -0.001 || s.opacity > 1.001);
    so.iter()
        .map(|s| {
            let a = if looks_logit { 1.0 / (1.0 + (-s.opacity).exp()) } else { s.opacity };
            a.clamp(0.0, 1.0)
        })
        .collect()
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

/// Floater cull: the dust floor everywhere, plus the spatial-isolation mass
/// test inside the robust box. Returns the keep mask, or `None` when nothing
/// is removed.
///
/// Each splat's cell is resolved through the hash map ONCE (to a dense cell
/// index); the mass sums, the 3×3×3 dilation and the per-splat test then run
/// on flat vectors. Summation order matches the original per-splat map, so
/// the result is bit-identical to it.
fn floater_keep_mask(cloud: &PlanarGaussian3d, source: &str) -> Option<Vec<bool>> {
    let grid_res = env_f32("EUSTRESS_SPLAT_CULL_GRID", FLOATER_GRID_RES).max(1.0);
    let min_mass = env_f32("EUSTRESS_SPLAT_CULL_MIN_MASS", FLOATER_MIN_MASS).max(0.0);
    let dust = env_f32("EUSTRESS_SPLAT_CULL_DUST", FLOATER_DUST_OPACITY);
    let max_remove = env_f32("EUSTRESS_SPLAT_CULL_MAX_REMOVE", FLOATER_MAX_REMOVE_FRAC);
    // > 0 ⇒ absolute voxel size in metres (overrides the adaptive size).
    let voxel_override = env_f32("EUSTRESS_SPLAT_CULL_VOXEL", 0.0);

    let pv = &cloud.position_visibility;
    let total = pv.len();
    if total == 0 {
        return None;
    }
    let eff = effective_opacity(cloud);

    // ── Robust extent: 2.5–97.5 percentile per axis ──
    // Raw min/max is dictated by the sparse far background/sky shell, which
    // over-sizes the voxel (→ a no-op cull) and would let the far field read as
    // "isolated". Percentile-box the scene, size the voxel off THAT, and only
    // cull inside it — everything outside is kept unconditionally.
    let robust_range = |axis: usize| -> (f32, f32) {
        let mut v: Vec<f32> = pv.iter().map(|p| p.position[axis]).collect();
        let mut at = |q: f32| {
            let idx = (((v.len() - 1) as f32) * q).round() as usize;
            *v.select_nth_unstable_by(idx, |a, b| a.total_cmp(b)).1
        };
        (at(0.025), at(0.975))
    };
    let (x, y, z) = (robust_range(0), robust_range(1), robust_range(2));
    let box_lo = [x.0, y.0, z.0];
    let box_hi = [x.1, y.1, z.1];
    let robust_extent = (0..3).map(|i| box_hi[i] - box_lo[i]).fold(0.0f32, f32::max).max(1e-3);
    let voxel = if voxel_override > 0.0 { voxel_override } else { (robust_extent / grid_res).max(1e-3) };
    let inv = 1.0 / voxel;
    let cell_of_pos = |p: [f32; 3]| -> [i64; 3] {
        [(p[0] * inv).floor() as i64, (p[1] * inv).floor() as i64, (p[2] * inv).floor() as i64]
    };
    let inside_box = |p: [f32; 3]| (0..3).all(|i| p[i] >= box_lo[i] && p[i] <= box_hi[i]);

    // ── Pass 1: per-cell opacity-weighted MASS, through a dense cell index ──
    let mut index: CellMap = CellMap::default();
    index.reserve(total / 4);
    let mut cells: Vec<[i64; 3]> = Vec::new();
    let mut mass: Vec<f32> = Vec::new();
    let mut cell_of: Vec<u32> = Vec::with_capacity(total);
    for (p, &e) in pv.iter().zip(&eff) {
        let c = cell_of_pos(p.position);
        let ci = *index.entry(pack_cell(c)).or_insert_with(|| {
            cells.push(c);
            mass.push(0.0);
            (mass.len() - 1) as u32
        });
        mass[ci as usize] += e;
        cell_of.push(ci);
    }
    // Dilate ONCE per occupied cell into its 3×3×3 neighbourhood mass. An
    // isolated floater lands where the whole neighbourhood is nearly empty;
    // dense-but-faint foliage does not (500 × 0.1 alpha = mass 50).
    let dilated: Vec<f32> = cells
        .iter()
        .map(|&[cx, cy, cz]| {
            let mut sum = 0.0f32;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        sum += index
                            .get(&pack_cell([cx + dx, cy + dy, cz + dz]))
                            .map_or(0.0, |&j| mass[j as usize]);
                    }
                }
            }
            sum
        })
        .collect();

    // ── Pass 2: the keep mask, the histogram and the counts in one sweep ──
    let mut keep = vec![true; total];
    let mut removed = 0usize;
    let mut dust_removed = 0usize;
    // In-box neighbourhood-mass histogram: the speck and surface populations
    // separate cleanly, so `min_mass` can be placed by measurement.
    let mut hist = [0usize; 6];
    for i in 0..total {
        let p = pv[i].position;
        let is_dust = eff[i] < dust;
        let inside = inside_box(p);
        let m = dilated[cell_of[i] as usize];
        if inside {
            let b = if m < 1.0 {
                0
            } else if m < 2.5 {
                1
            } else if m < 5.0 {
                2
            } else if m < 10.0 {
                3
            } else if m < 25.0 {
                4
            } else {
                5
            };
            hist[b] += 1;
        }
        if is_dust || (inside && m < min_mass) {
            keep[i] = false;
            removed += 1;
            dust_removed += is_dust as usize;
        }
    }
    warn!(
        "SplatCloud floater mass histogram (in-box 3x3x3): <1={} <2.5={} <5={} <10={} <25={} >=25={} (min_mass={})",
        hist[0], hist[1], hist[2], hist[3], hist[4], hist[5], min_mass
    );

    if removed as f32 / total as f32 > max_remove {
        // Over the cap → treat the isolation params as mis-tuned: keep the
        // isolation floaters but STILL drop dust.
        warn!(
            "SplatCloud floater cull: isolation pass would remove {:.1}% (> cap {:.0}%) \
             at voxel={:.3}m min_mass={} — SKIPPED (dropped {} dust only) for {}",
            (removed as f32 / total as f32) * 100.0,
            max_remove * 100.0,
            voxel,
            min_mass,
            dust_removed,
            source
        );
        if dust_removed == 0 {
            return None;
        }
        for (k, &e) in keep.iter_mut().zip(&eff) {
            *k = e >= dust;
        }
        return Some(keep);
    }
    warn!(
        "SplatCloud floater cull (spatial-mass): removed {} / {} gaussians ({:.2}%) \
         — voxel={:.3}m min_mass={} box=[{:.1},{:.1},{:.1}]..[{:.1},{:.1},{:.1}] from {}",
        removed,
        total,
        (removed as f32 / total as f32) * 100.0,
        voxel,
        min_mass,
        box_lo[0], box_lo[1], box_lo[2],
        box_hi[0], box_hi[1], box_hi[2],
        source
    );
    (removed > 0).then_some(keep)
}

/// Optional hard splat budget (`EUSTRESS_SPLAT_BUDGET`, default off): when the
/// cloud still exceeds it, keep the N most opaque splats (ties go to the
/// earlier splat). A frame-time dial that trades visual density, so opt-in.
fn apply_budget(cloud: &mut PlanarGaussian3d, source: &str) {
    let budget = std::env::var("EUSTRESS_SPLAT_BUDGET")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let len = cloud.position_visibility.len();
    if budget == 0 || len <= budget {
        return;
    }
    let eff = effective_opacity(cloud);
    let mut order: Vec<u32> = (0..len as u32).collect();
    order.sort_unstable_by(|&a, &b| eff[b as usize].total_cmp(&eff[a as usize]).then(a.cmp(&b)));
    let mut keep = vec![false; len];
    for &i in &order[..budget] {
        keep[i as usize] = true;
    }
    retain_by_mask(cloud, &keep);
    warn!(
        "SplatCloud budget: decimated {} → {} splats (EUSTRESS_SPLAT_BUDGET={}) for {}",
        len, budget, budget, source
    );
}

/// Voxel-fit the collider proxy from the solid splat centres. The voxel is
/// adaptive (~48 cells across the largest extent, clamped to [5 cm, 2 m]) so
/// any cloud, tabletop or building, gives a sane box count.
fn fit_collider(cloud: &PlanarGaussian3d) -> ColliderFit {
    let eff = effective_opacity(cloud);
    let points: Vec<[f32; 3]> = cloud
        .position_visibility
        .iter()
        .zip(&eff)
        .filter(|(_, &e)| e >= COLLIDER_MIN_OPACITY)
        .map(|(p, _)| p.position)
        .collect();
    if points.is_empty() {
        return ColliderFit { proxy: CompoundProxy::default(), voxel: 0.0, extent: 0.0, solid: 0 };
    }
    let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &points {
        for i in 0..3 {
            mn[i] = mn[i].min(p[i]);
            mx[i] = mx[i].max(p[i]);
        }
    }
    let extent = (0..3).map(|i| mx[i] - mn[i]).fold(0.0f32, f32::max);
    let voxel = (extent / 48.0).clamp(0.05, 2.0);
    let proxy = collider::extract_colliders(&points, ColliderStrategy::CsgPrimitiveFit, voxel);
    ColliderFit { proxy, voxel, extent, solid: points.len() }
}

/// Drop every splat whose mask entry is `false`, in place, across all four
/// planar fields. `Vec::retain` visits elements once, in order.
fn retain_by_mask(cloud: &mut PlanarGaussian3d, keep: &[bool]) {
    fn retain<T>(v: &mut Vec<T>, keep: &[bool]) {
        let mut i = 0;
        v.retain(|_| {
            let k = keep[i];
            i += 1;
            k
        });
    }
    retain(&mut cloud.position_visibility, keep);
    retain(&mut cloud.spherical_harmonic, keep);
    retain(&mut cloud.rotation, keep);
    retain(&mut cloud.scale_opacity, keep);
}

/// Pack a voxel cell into one `u64` key, 21 bits per axis. Cells more than
/// 2^20 voxels from the origin clamp onto the edge; only far-field splats
/// (outside the robust box, which are never culled for isolation) live there.
fn pack_cell(c: [i64; 3]) -> u64 {
    const HALF: i64 = 1 << 20;
    let f = |v: i64| ((v.clamp(-HALF, HALF - 1) + HALF) as u64) & 0x1F_FFFF;
    (f(c[0]) << 42) | (f(c[1]) << 21) | f(c[2])
}

/// `u64 → dense cell index` map. The default SipHash costs several times more
/// per lookup than this on the tens of millions of lookups the cull makes.
type CellMap = std::collections::HashMap<u64, u32, std::hash::BuildHasherDefault<CellHasher>>;

/// splitmix64 finaliser. A plain multiply hash is not enough here: products
/// only carry entropy upward, and the map takes its bucket from the LOW bits,
/// which would then depend on the packed z coordinate alone and pile whole
/// columns of cells into one probe chain.
#[derive(Default)]
struct CellHasher(u64);

impl std::hash::Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, v: u64) {
        let mut z = self.0 ^ v;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        self.0 = z ^ (z >> 31);
    }
}
