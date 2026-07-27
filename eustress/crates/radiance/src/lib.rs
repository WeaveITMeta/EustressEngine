//! Radiance-field rendering for Eustress (Gaussian Splatting).
//!
//! This crate wraps [`bevy_gaussian_splatting`] behind a small, stable,
//! engine-facing API so the rest of Eustress depends on `eustress_radiance`
//! rather than the upstream crate directly. That seam lets us swap the renderer
//! (custom `ViewNode`, 3DGUT projection, relighting) without touching call
//! sites.
//!
//! Roadmap (see `docs/architecture/GAUSSIAN_SPLATTING_BATTLE_PLAN.md`):
//! - **Phase 0 (this module):** adopt the crate, render `.ply`/`.gcloud`/glTF
//!   Gaussian clouds in-engine.
//! - **Phase 1 ([`collider`]):** extract Avian colliders from a cloud so splats
//!   become physical (visual splats + invisible proxy).
//! - **Phase 4 (relighting):** import inverse-rendered per-splat PBR and light
//!   with the engine's existing PBR + `LightClassPlugin` dynamic lights. PPISP
//!   (`eustress-ppisp`) is the photometric front-end that makes that
//!   decomposition physically grounded.

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy_gaussian_splatting::{
    camera::GaussianCameraPlugin,
    gaussian::{cloud::CloudPlugin, formats::planar_3d::PlanarGaussian3d, settings::SettingsPlugin},
    io::loader::{Gaussian3dLoader, Gaussian4dLoader},
    query::QueryPlugin,
    render::RenderPipelinePlugin,
    CloudSettings, Gaussian3d, Gaussian4d, GaussianCamera, PlanarGaussian3dHandle,
    PlanarStoragePlugin, SphericalHarmonicCoefficients,
};

pub mod collider;
pub use collider::{ColliderPrimitive, ColliderStrategy, CompoundProxy};

/// Adds Gaussian-Splatting / radiance-field rendering to the app.
///
/// Registers the upstream [`GaussianSplattingPlugin`] (render pipeline, GPU
/// depth sort, `.ply`/`.gcloud`/glTF `KHR_gaussian_splatting` loaders) plus
/// Eustress's wrapper types so a splat cloud can later surface as a first-class
/// editor object.
pub struct RadiancePlugin;

impl Plugin for RadiancePlugin {
    fn build(&self, app: &mut App) {
        // We replicate `bevy_gaussian_splatting::GaussianSplattingPlugin` EXACTLY
        // EXCEPT its glTF scene loader (`io::scene::GaussianScenePlugin`). That
        // loader registers an `AssetLoader` for `.glb`/`.gltf` and SHADOWS Bevy's
        // `GltfLoader`, so the engine's normal part meshes (`parts/block.glb`)
        // fail with "no KHR_gaussian_splatting primitives found" and vanish from
        // the scene. We only need `.ply`/`.gcloud` clouds, so we bring up the
        // render + cloud loaders WITHOUT the scene loader → splats and normal
        // meshes coexist. (If we later want glTF-embedded splat scenes, register
        // a loader scoped to a distinct extension instead of plain `.glb`.)
        app.register_type::<SphericalHarmonicCoefficients>();

        // == IoPlugin, minus GaussianScenePlugin ==
        app.init_asset_loader::<Gaussian3dLoader>();
        app.init_asset_loader::<Gaussian4dLoader>();

        app.add_plugins((
            GaussianCameraPlugin,
            SettingsPlugin,
            CloudPlugin::<Gaussian3d>::default(),
            CloudPlugin::<Gaussian4d>::default(),
        ));
        app.add_plugins((
            PlanarStoragePlugin::<Gaussian3d>::default(),
            PlanarStoragePlugin::<Gaussian4d>::default(),
        ));
        app.add_plugins((
            RenderPipelinePlugin::<Gaussian3d>::default(),
            RenderPipelinePlugin::<Gaussian4d>::default(),
        ));
        app.add_plugins((
            bevy_gaussian_splatting::material::MaterialPlugin,
            QueryPlugin,
        ));

        app.register_type::<SplatCloud>();
        app.register_type::<FloaterCullApplied>();
        // Per-cloud floater removal. Runs in Update, retrying until the cloud
        // asset finishes loading, then filters ONCE (guarded by
        // `FloaterCullApplied`) — see [`apply_floater_cull`].
        app.add_systems(Update, apply_floater_cull);
        // Physics-proxy extraction: once the (culled) cloud is resident, voxel-fit
        // a collider proxy from the splat centers so the ENGINE can attach a real
        // Avian collider (radiance stays Avian-free). See [`extract_splat_collider_proxy`].
        app.add_systems(Update, extract_splat_collider_proxy);
    }
}

/// The voxel-box collider proxy extracted from a splat cloud, in the cloud's
/// LOCAL space. Avian-free: the engine reads this and builds an Avian compound
/// collider (see `collider.rs` — radiance is physics-engine-agnostic). Present
/// once extraction has run (empty proxy ⇒ nothing collidable, still marks done).
#[derive(Component, Debug, Clone)]
pub struct SplatColliderProxy {
    pub proxy: CompoundProxy,
}

/// Minimum EFFECTIVE opacity a splat must have to contribute to the collider —
/// reuse the floater threshold so ghost floaters don't inflate the proxy.
const COLLIDER_MIN_OPACITY: f32 = 0.08;

/// Voxel-fit an invisible physics proxy from a resident [`SplatCloud`]'s splat
/// centers. Runs AFTER the floater cull (gated on `FloaterCullApplied`, so the
/// handle is stable and the cloud is de-floatered) and ONCE per cloud (gated on
/// `Without<SplatColliderProxy>`). The voxel size is ADAPTIVE — ~48 cells across
/// the cloud's largest extent, clamped to [5 cm, 2 m] — so ANY imported cloud,
/// tabletop or building, yields a sane collider count instead of exploding on a
/// large scene. Deterministic (see [`collider::extract_colliders`]).
/// In-flight background collider extraction for one splat cloud. Result is
/// `(proxy, voxel_size, extent, solid_point_count)` for the completion log.
#[derive(Component)]
struct ColliderProxyTask(Task<(CompoundProxy, f32, f32, usize)>);

fn extract_splat_collider_proxy(
    mut commands: Commands,
    assets: Res<Assets<PlanarGaussian3d>>,
    mut query: Query<
        (Entity, &PlanarGaussian3dHandle, Option<&mut ColliderProxyTask>),
        (With<SplatCloud>, With<FloaterCullApplied>, Without<SplatColliderProxy>),
    >,
) {
    for (entity, handle, task) in &mut query {
        // Phase 2: a background extraction is in flight — poll it and attach
        // the proxy when it lands. Everything heavy happened off-thread.
        if let Some(mut task) = task {
            let Some((proxy, voxel, extent, solid)) = block_on(future::poll_once(&mut task.0))
            else {
                continue; // still crunching — no main-thread cost beyond the poll
            };
            let n = proxy.primitives.len();
            commands
                .entity(entity)
                .insert(SplatColliderProxy { proxy })
                .remove::<ColliderProxyTask>();
            warn!(
                "splat collider proxy: {} voxel boxes (voxel={:.3} m, extent={:.2} m, solid pts={}) for {:?}",
                n, voxel, extent, solid, entity
            );
            continue;
        }

        // Phase 1: cloud resident → snapshot the (position, opacity) pairs and
        // hand the whole voxel fit to the compute pool. The snapshot is ONE
        // linear pass; the multi-second part (voxel dedup over millions of
        // points) never touches the main thread.
        let Some(cloud) = assets.get(&handle.0) else {
            continue; // still loading — retry next frame
        };
        let samples: Vec<([f32; 3], f32)> = cloud
            .iter()
            .map(|g| {
                let p = g.position_visibility.position;
                ([p[0], p[1], p[2]], g.scale_opacity.opacity)
            })
            .collect();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            // Solid splat centers only (skip near-transparent floaters). Detect
            // the opacity convention the same way the cull does (logits ⇒ sigmoid).
            let looks_logit = samples
                .iter()
                .take(4096)
                .any(|(_, o)| *o < -0.001 || *o > 1.001);
            let effective = |raw: f32| -> f32 {
                if looks_logit { 1.0 / (1.0 + (-raw).exp()) } else { raw }
            };
            let points: Vec<[f32; 3]> = samples
                .iter()
                .filter(|(_, o)| effective(*o) >= COLLIDER_MIN_OPACITY)
                .map(|(p, _)| *p)
                .collect();
            if points.is_empty() {
                return (CompoundProxy::default(), 0.0, 0.0, 0);
            }
            // Adaptive voxel size from the cloud extent.
            let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &points {
                for i in 0..3 {
                    mn[i] = mn[i].min(p[i]);
                    mx[i] = mx[i].max(p[i]);
                }
            }
            let extent = (0..3).map(|i| mx[i] - mn[i]).fold(0.0f32, f32::max);
            let voxel = (extent / 48.0).clamp(0.05, 2.0);
            let proxy =
                collider::extract_colliders(&points, ColliderStrategy::CsgPrimitiveFit, voxel);
            (proxy, voxel, extent, points.len())
        });
        commands.entity(entity).insert(ColliderProxyTask(task));
    }
}

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

/// Remove near-transparent "floater" Gaussians from a [`SplatCloud`] whose
/// `cull_floaters` is set, and restore the pristine cloud when it is unset.
///
/// The renderer has no per-splat visibility mask we can drive from
/// [`CloudSettings`], so culling means rebuilding the cloud asset with the
/// floaters filtered out. That is an expensive one-shot (a multi-million-splat
/// cloud is rebuilt in-place), so it is gated behind [`FloaterCullApplied`] and
/// runs exactly once per enable. Turning the toggle OFF reloads the original
/// `.ply` from `source` (the on-disk file is never modified), so the operation
/// is fully reversible.
///
/// Opacity in a 3DGS `.ply` is stored as a RAW logit (the shader applies the
/// sigmoid activation); some pipelines instead store the activated `[0,1]`
/// value. We detect which by sampling: any value outside `[0,1]` means logits,
/// so we sigmoid before thresholding. This keeps the cull correct either way
/// and — with [`FLOATER_MIN_KEEP_FRACTION`] — refuses to run if the detection
/// still looks wrong.
/// In-flight background floater-cull analysis for one splat cloud. Resolves to
/// `Some(filtered_cloud)` when splats were removed, `None` for a no-op cull.
#[derive(Component)]
struct FloaterCullTask(Task<Option<PlanarGaussian3d>>);

/// A cloud asset that has been loaded (or is loading) but is NOT yet exposed
/// to the GPU renderer. [`attach_splat_cloud`] parks the handle here instead of
/// `PlanarGaussian3dHandle`; [`apply_floater_cull`] promotes it once the cull
/// has produced the smaller cloud. Rationale: exposing the raw multi-million-
/// splat cloud first meant (a) a full GPU upload + per-frame sort/draw of
/// splats that were about to be thrown away, (b) a ~2× VRAM spike while raw +
/// culled clouds coexisted, and (c) on marginal drivers a device-lost (TDR)
/// during that burst — the GPU should only ever see the final cloud.
#[derive(Component)]
pub struct PendingSplatCloud(pub Handle<PlanarGaussian3d>);

fn apply_floater_cull(
    mut assets: ResMut<Assets<PlanarGaussian3d>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut query: Query<(
        Entity,
        &SplatCloud,
        Option<&PendingSplatCloud>,
        Option<&PlanarGaussian3dHandle>,
        Option<&FloaterCullApplied>,
        Option<&mut FloaterCullTask>,
    )>,
) {
    for (entity, cloud, pending, live, applied, task) in &mut query {
        // ── Poll an in-flight analysis (pending- or live-sourced alike). The
        // multi-second work (percentile boxing, mass grid, rebuild) happens in
        // the compute pool — this frame only pays the poll + handle swap.
        if let Some(mut task) = task {
            let Some(filtered) = block_on(future::poll_once(&mut task.0)) else {
                continue; // still crunching
            };
            match filtered {
                Some(new_cloud) => {
                    // Swap in a FRESH asset handle rather than mutating the
                    // existing asset in place — the upstream GPU planar storage
                    // only reacts to a NEW handle (see git history: in-place
                    // mutation rendered nothing). Drop the raw asset so its GPU
                    // copy (if it ever uploaded) is freed too.
                    let old_id = pending
                        .map(|p| p.0.id())
                        .or_else(|| live.map(|l| l.0.id()));
                    let new_handle = assets.add(new_cloud);
                    if let Some(id) = old_id {
                        assets.remove(id);
                    }
                    commands
                        .entity(entity)
                        .insert(PlanarGaussian3dHandle(new_handle))
                        .remove::<PendingSplatCloud>();
                }
                // No-op cull: promote the pending handle unchanged (live
                // handles are already exposed — nothing to do).
                None => {
                    if let Some(p) = pending {
                        commands
                            .entity(entity)
                            .insert(PlanarGaussian3dHandle(p.0.clone()))
                            .remove::<PendingSplatCloud>();
                    }
                }
            }
            // Mark applied either way (a no-op cull must not respawn the task
            // every frame); toggle off→on clears the marker.
            commands
                .entity(entity)
                .remove::<FloaterCullTask>()
                .insert(FloaterCullApplied);
            continue;
        }

        // ── Pending cloud, cull disabled → expose directly to the GPU.
        if let Some(p) = pending {
            if !cloud.cull_floaters {
                commands
                    .entity(entity)
                    .insert(PlanarGaussian3dHandle(p.0.clone()))
                    .remove::<PendingSplatCloud>();
                continue;
            }
        }

        // ── Arm the analysis once the source asset is resident. Source is the
        // pending handle when deferred, else the live handle (toggle off→on
        // re-cull of an already-exposed cloud).
        if cloud.cull_floaters && applied.is_none() {
            let Some(handle) = pending.map(|p| &p.0).or_else(|| live.map(|l| &l.0)) else {
                continue;
            };
            let Some(data) = assets.get(handle) else {
                continue; // still loading — retry next frame
            };
            // Snapshot: one linear planar→struct pass (the cheapest part of
            // the old synchronous stall); everything heavier moves off-thread.
            let raw: Vec<Gaussian3d> = data.iter().collect();
            if raw.is_empty() {
                continue;
            }
            let source = cloud.source.clone();
            let task =
                AsyncComputeTaskPool::get().spawn(async move { cull_gaussians(raw, source) });
            commands.entity(entity).insert(FloaterCullTask(task));
            continue;
        }

        // ── Cull disabled after being applied → reload the pristine cloud.
        if !cloud.cull_floaters && applied.is_some() {
            commands
                .entity(entity)
                .insert(PlanarGaussian3dHandle(asset_server.load(cloud.source.clone())))
                .remove::<FloaterCullApplied>()
                .remove::<FloaterCullTask>();
        }
    }
}

/// The floater-cull analysis body, run on the [`AsyncComputeTaskPool`]: dust
/// floor + spatial-isolation mass cull (+ optional `EUSTRESS_SPLAT_BUDGET`
/// decimation), returning the rebuilt cloud (`None` ⇒ nothing removed). Pure
/// function of its inputs — no ECS access — so it can run on any thread.
fn cull_gaussians(raw: Vec<Gaussian3d>, source: String) -> Option<PlanarGaussian3d> {
    let total = raw.len();
    let filtered = {
        {

                    // ── Tunables — env overrides give live tuning with no rebuild ──
                    let env_f = |k: &str, d: f32| {
                        std::env::var(k)
                            .ok()
                            .and_then(|s| s.parse::<f32>().ok())
                            .filter(|v| v.is_finite())
                            .unwrap_or(d)
                    };
                    let grid_res = env_f("EUSTRESS_SPLAT_CULL_GRID", FLOATER_GRID_RES).max(1.0);
                    let min_mass = env_f("EUSTRESS_SPLAT_CULL_MIN_MASS", FLOATER_MIN_MASS).max(0.0);
                    let dust = env_f("EUSTRESS_SPLAT_CULL_DUST", FLOATER_DUST_OPACITY);
                    let max_remove = env_f("EUSTRESS_SPLAT_CULL_MAX_REMOVE", FLOATER_MAX_REMOVE_FRAC);
                    // > 0 ⇒ absolute voxel size in metres (overrides the adaptive size).
                    let voxel_override = env_f("EUSTRESS_SPLAT_CULL_VOXEL", 0.0);

                    // Opacity is stored either as a RAW logit or an activated [0,1]
                    // value; probe a bounded sample and sigmoid the logit case so the
                    // mass weighting + dust floor read the same either way.
                    let looks_logit = raw
                        .iter()
                        .take(4096)
                        .any(|g| g.scale_opacity.opacity < -0.001 || g.scale_opacity.opacity > 1.001);
                    let effective = move |o: f32| -> f32 {
                        let a = if looks_logit { 1.0 / (1.0 + (-o).exp()) } else { o };
                        a.clamp(0.0, 1.0)
                    };

                    // ── Robust extent: 2.5–97.5 percentile per axis ──
                    // Raw min/max is dictated by the sparse far background/sky shell,
                    // which over-sizes the voxel (→ a no-op cull) and would let the far
                    // field read as "isolated". Percentile-box the scene, size the
                    // voxel off THAT, and only cull inside it — everything outside is
                    // kept unconditionally (sky / background is legitimately sparse).
                    let mut xs: Vec<f32> = Vec::with_capacity(total);
                    let mut ys: Vec<f32> = Vec::with_capacity(total);
                    let mut zs: Vec<f32> = Vec::with_capacity(total);
                    for g in raw.iter() {
                        let p = g.position_visibility.position;
                        xs.push(p[0]);
                        ys.push(p[1]);
                        zs.push(p[2]);
                    }
                    let pct = |v: &mut Vec<f32>, q: f32| -> f32 {
                        if v.is_empty() {
                            return 0.0;
                        }
                        let idx = (((v.len() - 1) as f32) * q).round() as usize;
                        v.select_nth_unstable_by(idx, |a, b| a.total_cmp(b));
                        v[idx]
                    };
                    let box_lo = [pct(&mut xs, 0.025), pct(&mut ys, 0.025), pct(&mut zs, 0.025)];
                    let box_hi = [pct(&mut xs, 0.975), pct(&mut ys, 0.975), pct(&mut zs, 0.975)];
                    drop((xs, ys, zs)); // partially reordered by select_nth — do not reuse
                    let robust_extent =
                        (0..3).map(|i| box_hi[i] - box_lo[i]).fold(0.0f32, f32::max).max(1e-3);
                    let voxel = if voxel_override > 0.0 {
                        voxel_override
                    } else {
                        (robust_extent / grid_res).max(1e-3)
                    };
                    let inv = 1.0 / voxel;
                    let key = |p: [f32; 3]| -> (i64, i64, i64) {
                        (
                            (p[0] * inv).floor() as i64,
                            (p[1] * inv).floor() as i64,
                            (p[2] * inv).floor() as i64,
                        )
                    };
                    let inside_box = |p: [f32; 3]| -> bool {
                        (0..3).all(|i| p[i] >= box_lo[i] && p[i] <= box_hi[i])
                    };

                    // ── Pass 1: per-cell opacity-weighted MASS (Σ effective opacity) ──
                    let mut mass: std::collections::HashMap<(i64, i64, i64), f32> =
                        std::collections::HashMap::new();
                    for g in raw.iter() {
                        *mass.entry(key(g.position_visibility.position)).or_insert(0.0) +=
                            effective(g.scale_opacity.opacity);
                    }
                    // Dilate ONCE per occupied cell into its 3×3×3 neighbourhood mass.
                    // An isolated floater lands where the whole neighbourhood is nearly
                    // empty; dense-but-faint foliage does not (500 × 0.1 alpha = mass 50)
                    // — the distinction a pure opacity threshold could not make.
                    let mut dilated: std::collections::HashMap<(i64, i64, i64), f32> =
                        std::collections::HashMap::with_capacity(mass.len());
                    for &(cx, cy, cz) in mass.keys() {
                        let mut sum = 0.0f32;
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                for dz in -1..=1 {
                                    sum += mass.get(&(cx + dx, cy + dy, cz + dz)).copied().unwrap_or(0.0);
                                }
                            }
                        }
                        dilated.insert((cx, cy, cz), sum);
                    }

                    // Removal predicate: dust everywhere, isolation only inside the box.
                    let is_floater = |g: &Gaussian3d| -> bool {
                        let p = g.position_visibility.position;
                        if effective(g.scale_opacity.opacity) < dust {
                            return true; // dust floor — remove regardless of location
                        }
                        if !inside_box(p) {
                            return false; // outside the robust box → keep (sky / far field)
                        }
                        dilated.get(&key(p)).copied().unwrap_or(0.0) < min_mass
                    };

                    // Neighbourhood-mass histogram (in-box), logged once: the speck and
                    // surface populations separate cleanly, so `min_mass` can be placed
                    // by measurement rather than guessed.
                    {
                        let mut b = [0usize; 6];
                        for g in raw.iter() {
                            let p = g.position_visibility.position;
                            if !inside_box(p) {
                                continue;
                            }
                            let m = dilated.get(&key(p)).copied().unwrap_or(0.0);
                            let i = if m < 1.0 {
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
                            b[i] += 1;
                        }
                        warn!(
                            "SplatCloud floater mass histogram (in-box 3x3x3): <1={} <2.5={} <5={} <10={} <25={} >=25={} (min_mass={})",
                            b[0], b[1], b[2], b[3], b[4], b[5], min_mass
                        );
                    }

                    // ── Pass 2: apply, with a safety cap ──
                    let kept: Vec<Gaussian3d> =
                        raw.iter().filter(|g| !is_floater(g)).cloned().collect();
                    let removed = total - kept.len();
                    if removed as f32 / total as f32 > max_remove {
                        // Over the cap → treat the isolation params as mis-tuned: keep
                        // the isolation floaters but STILL drop dust, and skip the swap.
                        let kept_dust: Vec<Gaussian3d> = raw
                            .iter()
                            .filter(|g| effective(g.scale_opacity.opacity) >= dust)
                            .cloned()
                            .collect();
                        let dust_removed = total - kept_dust.len();
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
                            None
                        } else {
                            Some(PlanarGaussian3d::from_iter(kept_dust))
                        }
                    } else {
                        // `warn!` (not `info!`) so it survives the engine's hardcoded
                        // log filter during verification.
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
                        if removed == 0 {
                            None
                        } else {
                            Some(PlanarGaussian3d::from_iter(kept))
                        }
                    }
        }
    };

    // ── Optional hard splat budget (EUSTRESS_SPLAT_BUDGET, default off) ──
    // A raw FPS dial for GPU-bound clouds: when set (> 0) and the surviving
    // cloud still exceeds it, keep the N most-opaque splats. Opt-in because it
    // trades visual density for frame time — floater culling above is loss-
    // free by design, this is not.
    let budget = std::env::var("EUSTRESS_SPLAT_BUDGET")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    if budget == 0 {
        return filtered;
    }
    let mut survivors: Vec<Gaussian3d> = match &filtered {
        Some(c) => c.iter().collect(),
        None => raw,
    };
    if survivors.len() <= budget {
        return filtered;
    }
    let looks_logit = survivors
        .iter()
        .take(4096)
        .any(|g| g.scale_opacity.opacity < -0.001 || g.scale_opacity.opacity > 1.001);
    let eff = |o: f32| -> f32 {
        if looks_logit { 1.0 / (1.0 + (-o).exp()) } else { o }
    };
    survivors.sort_unstable_by(|a, b| {
        eff(b.scale_opacity.opacity).total_cmp(&eff(a.scale_opacity.opacity))
    });
    let before = survivors.len();
    survivors.truncate(budget);
    warn!(
        "SplatCloud budget: decimated {} → {} splats (EUSTRESS_SPLAT_BUDGET={}) for {}",
        before, budget, budget, source
    );
    Some(PlanarGaussian3d::from_iter(survivors))
}

/// Marker + display metadata for a Gaussian-splat cloud entity.
///
/// Carried alongside the upstream handle so the cloud can be shown in the
/// Explorer/Properties polymorphic inspector (splat source, later: count,
/// bounds, SH degree) and round-tripped through `instance_create` / WorldDb.
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct SplatCloud {
    /// Source asset path the cloud was loaded from (for display / round-trip).
    pub source: String,
    /// When true, remove "floater" Gaussians — the isolated specks that hang in
    /// the air around a real capture — once the cloud asset loads. This is a
    /// GEOMETRIC prune by SPATIAL ISOLATION (a splat with almost no neighbours in
    /// a voxel grid), NOT an opacity cut: dense-but-faint surface detail like
    /// tree foliage is low-opacity yet must be KEPT, so opacity is the wrong
    /// signal. Distinct from [`Self::ppisp`]'s photometric correction. Tunable
    /// via `EUSTRESS_SPLAT_CULL_*` env vars. Surfaced as a Properties toggle;
    /// default ON.
    pub cull_floaters: bool,
    /// When true, apply the PPISP (Physically-Plausible ISP) photometric
    /// correction to the cloud — the exposure/vignette/color/CRF front-end that
    /// makes a multi-camera capture physically grounded. Only the exposure
    /// stage is implemented today (see [`eustress_ppisp`]); the rest is
    /// scaffolded. Surfaced as a Properties toggle; default ON.
    pub ppisp: bool,
}

impl Default for SplatCloud {
    fn default() -> Self {
        Self {
            source: String::new(),
            // Both correction passes default ON: a freshly-imported real-world
            // capture almost always wants floater removal + photometric
            // grounding, and the user can toggle either off per-cloud.
            cull_floaters: true,
            ppisp: true,
        }
    }
}

/// One-shot marker: set on a [`SplatCloud`] entity once its floater cull has
/// run, so the (asset-mutating) cull system does not re-filter every frame.
/// Removed when `cull_floaters` is toggled so the cull can re-evaluate.
#[derive(Component, Reflect, Debug, Clone, Default)]
#[reflect(Component)]
pub struct FloaterCullApplied;

/// Optional demo: when the `EUSTRESS_SPLAT` env var is set to a cloud path
/// (a `file://` URI or an asset-relative path), spawn that cloud at the origin
/// on startup. Lets you eyeball the Phase-0 render path end to end:
///
/// ```text
/// EUSTRESS_SPLAT=scenes/sample_sphere.ply \
///   cargo run -p eustress-engine --features gaussian-splatting
/// ```
pub struct RadianceDemoPlugin;

impl Plugin for RadianceDemoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, demo_spawn_from_env);
        app.add_systems(Update, demo_tag_gaussian_cameras);
    }
}

/// Demo helper: the upstream renderer only draws clouds to cameras tagged
/// [`GaussianCamera`]. The editor's `Camera3d` is not, so without this, clouds
/// load but never render (the log shows "no gaussian cameras found"). Tag any
/// untagged 3D camera so the demo is visible.
///
/// Production should tag only the intended viewport camera rather than every
/// `Camera3d` (force-tagging all cameras, incl. AI/offscreen cameras, is a demo
/// convenience) — see the roadmap engine-contention audit.
fn demo_tag_gaussian_cameras(
    mut commands: Commands,
    cameras: Query<(Entity, &Camera), (With<Camera3d>, Without<GaussianCamera>)>,
) {
    for (entity, camera) in &cameras {
        // Tag ONLY the order-0 main viewport camera. The upstream pipeline
        // builds per-GaussianCamera sort buffers and draws every cloud into
        // every tagged view, so tagging any additional Camera3d (the engine's
        // order-300 Slint UI overlay camera is one) re-renders + re-sorts the
        // full multi-million-splat cloud once more per frame. Negative-order
        // cameras (AI capture rigs) additionally panic the upstream sorter,
        // which asserts `camera.order >= 0`.
        if camera.order == 0 {
            commands.entity(entity).insert(GaussianCamera::default());
        }
    }
}

fn demo_spawn_from_env(mut commands: Commands, asset_server: Res<AssetServer>) {
    if let Ok(path) = std::env::var("EUSTRESS_SPLAT") {
        if !path.is_empty() {
            // `eprintln!` (not `info!`) so this crate needs no bevy_log feature.
            eprintln!("[radiance] EUSTRESS_SPLAT set -> spawning splat cloud: {path}");
            spawn_splat_cloud(&mut commands, &asset_server, path, Transform::IDENTITY);
        }
    }
}

/// Spawn a Gaussian-splat cloud from an asset path (`.ply` / `.gcloud` / glTF).
///
/// Returns the spawned entity. The upstream plugin adds `CloudSettings` and
/// `Visibility` automatically; we also attach a [`Transform`], a [`SplatCloud`]
/// for editor integration, and a [`Name`].
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use eustress_radiance::spawn_splat_cloud;
/// fn setup(mut commands: Commands, assets: Res<AssetServer>) {
///     spawn_splat_cloud(&mut commands, &assets, "scenes/icecream.gcloud", Transform::IDENTITY);
/// }
/// ```
pub fn spawn_splat_cloud(
    commands: &mut Commands,
    asset_server: &AssetServer,
    path: impl Into<String>,
    transform: Transform,
) -> Entity {
    let path = path.into();
    commands
        .spawn((
            PlanarGaussian3dHandle(asset_server.load(path.clone())),
            CloudSettings::default(),
            transform,
            SplatCloud { source: path, ..Default::default() },
            Name::new("SplatCloud"),
        ))
        .id()
}

/// Attach the radiance-field rendering components to an ALREADY-SPAWNED
/// entity (`Transform`/`Name`/`Instance` already attached by the caller — the
/// instance-loader's generic no-mesh bundle). `PlanarGaussian3dHandle` and
/// `CloudSettings` are re-exported from `bevy_gaussian_splatting` but their
/// constructors/fields are private outside that crate, so only code IN this
/// crate can build them — this is the entity-attach twin of
/// [`spawn_splat_cloud`] (which spawns a brand-new entity instead).
pub fn attach_splat_cloud(
    ec: &mut bevy::ecs::system::EntityCommands,
    asset_server: &AssetServer,
    path: impl Into<String>,
    cull_floaters: bool,
    ppisp: bool,
) {
    let path = path.into();
    ec.insert((
        // Park the handle in `PendingSplatCloud`: the GPU only sees the cloud
        // once the floater cull has produced the final (smaller) asset — see
        // the component's docs for why exposing the raw cloud first was a
        // VRAM/TDR hazard. `apply_floater_cull` promotes it (immediately when
        // `cull_floaters` is off).
        PendingSplatCloud(asset_server.load(path.clone())),
        CloudSettings {
            // 16-bit depth keys halve the GPU radix-sort passes vs the 32-bit
            // default. A single captured scene spans metres–hundreds of
            // metres, so 65K depth buckets are far below visible popping.
            radix_sort_depth_bits:
                bevy_gaussian_splatting::gaussian::settings::RadixSortDepthBits::Bits16,
            ..Default::default()
        },
        SplatCloud { source: path, cull_floaters, ppisp },
    ));
}
