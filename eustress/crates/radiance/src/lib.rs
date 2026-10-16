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

use bevy::asset::LoadState;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy_gaussian_splatting::{
    camera::GaussianCameraPlugin,
    gaussian::{cloud::CloudPlugin, formats::planar_3d::PlanarGaussian3d, settings::SettingsPlugin},
    io::loader::Gaussian4dLoader,
    query::QueryPlugin,
    render::RenderPipelinePlugin,
    CloudSettings, Gaussian3d, Gaussian4d, GaussianCamera, PlanarGaussian3dHandle,
    PlanarStoragePlugin, SphericalHarmonicCoefficients,
};

pub mod collider;
pub use collider::{ColliderPrimitive, ColliderStrategy, CompoundProxy};

mod ply;
pub use ply::{FastGaussian3dLoader, RawSplatCloud, RawSplatCloudLoader};

mod process;

/// Adds Gaussian-Splatting / radiance-field rendering to the app.
///
/// Registers the upstream render pipeline, GPU depth sort and `.gcloud` / 4D
/// loaders, Eustress's fast `.ply` loaders (the `ply` module), and the system
/// that turns a loaded cloud into the one the GPU draws
/// ([`process_splat_clouds`]).
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

        // == IoPlugin, minus GaussianScenePlugin, with Eustress's .ply reader ==
        // `FastGaussian3dLoader` stands in for upstream's `Gaussian3dLoader`
        // (same asset type, same extensions); `RawSplatCloudLoader` feeds the
        // processing pipeline. Bevy picks between them by asset type.
        app.init_asset::<RawSplatCloud>();
        app.init_asset_loader::<FastGaussian3dLoader>();
        app.init_asset_loader::<RawSplatCloudLoader>();
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
        // One system takes every cloud from load to GPU: it moves each loaded
        // raw cloud into a compute task (floater cull + collider fit), hands
        // the result to the renderer, and reprocesses when `cull_floaters`
        // changes. See [`process_splat_clouds`].
        app.add_systems(Update, process_splat_clouds);
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

/// A cloud that is loading, or loaded and waiting to be processed. It holds a
/// [`RawSplatCloud`] handle: that asset type lives in the main world only, so
/// the raw cloud is never uploaded to the GPU. [`process_splat_clouds`] moves
/// the loaded data into a compute task and drops this component.
#[derive(Component)]
pub struct PendingSplatCloud(pub Handle<RawSplatCloud>);

/// Processing of one cloud in flight on the compute pool.
#[derive(Component)]
struct SplatProcessTask(Task<process::Processed>);

/// The `cull_floaters` value the live cloud was processed with. When the
/// entity's [`SplatCloud::cull_floaters`] no longer matches, the cloud is
/// reloaded and processed again.
#[derive(Component)]
struct SplatProcessedWith {
    cull_floaters: bool,
}

/// When this cloud's load started, for the "splat ready" log line.
#[derive(Component)]
struct SplatLoadTimer(Instant);

/// Take every [`SplatCloud`] from load to GPU.
///
/// 1. A [`PendingSplatCloud`] whose raw asset has loaded: the data is MOVED
///    out of the asset store (no copy) into a compute task that runs the
///    floater cull and the collider fit on the cloud's planar storage.
/// 2. A finished task: its cloud goes into `Assets<PlanarGaussian3d>`, the
///    store the renderer uploads from, behind a `PlanarGaussian3dHandle`, so
///    the GPU receives exactly one cloud, the final one. The collider proxy
///    rides along.
/// 3. A live cloud whose `cull_floaters` changed: reload the source and go
///    round again. A change that lands while a task runs is caught when the
///    task ends.
///
/// The main thread pays one poll and a handle swap per cloud; parsing,
/// culling and fitting all run on workers.
fn process_splat_clouds(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut raw_clouds: ResMut<Assets<RawSplatCloud>>,
    mut clouds: ResMut<Assets<PlanarGaussian3d>>,
    mut query: Query<(
        Entity,
        &SplatCloud,
        Option<&PendingSplatCloud>,
        Option<&PlanarGaussian3dHandle>,
        Option<&SplatProcessedWith>,
        Option<&SplatLoadTimer>,
        Has<SplatColliderProxy>,
        Option<&mut SplatProcessTask>,
    )>,
) {
    for (entity, cloud, pending, live, processed_with, timer, has_proxy, task) in &mut query {
        if let Some(mut task) = task {
            let Some(done) = block_on(future::poll_once(&mut task.0)) else {
                continue; // still crunching
            };
            let count = done.cloud.position_visibility.len();
            let handle = clouds.add(done.cloud);
            // Reprocessing after a toggle: drop the previous cloud's main-world
            // and GPU copies.
            if let Some(old) = live {
                clouds.remove(old.0.id());
            }
            let mut ec = commands.entity(entity);
            ec.insert((
                PlanarGaussian3dHandle(handle),
                SplatProcessedWith { cull_floaters: done.culled },
            ))
            .remove::<SplatProcessTask>();
            if done.culled {
                ec.insert(FloaterCullApplied);
            } else {
                ec.remove::<FloaterCullApplied>();
            }
            if let Some(fit) = done.collider {
                warn!(
                    "splat collider proxy: {} voxel boxes (voxel={:.3} m, extent={:.2} m, solid pts={}) for {:?}",
                    fit.proxy.primitives.len(),
                    fit.voxel,
                    fit.extent,
                    fit.solid,
                    entity
                );
                ec.insert(SplatColliderProxy { proxy: fit.proxy });
            }
            if let Some(timer) = timer {
                warn!(
                    "splat ready: {} gaussians from {} in {:.2}s",
                    count,
                    cloud.source,
                    timer.0.elapsed().as_secs_f32()
                );
                ec.remove::<SplatLoadTimer>();
            }
            continue;
        }

        if let Some(pending) = pending {
            if let Some(raw) = raw_clouds.remove(&pending.0) {
                let request = process::ProcessRequest {
                    source: cloud.source.clone(),
                    cull_floaters: cloud.cull_floaters,
                    collider: !has_proxy,
                };
                let task = AsyncComputeTaskPool::get()
                    .spawn(async move { process::process_cloud(raw.0, request) });
                commands
                    .entity(entity)
                    .insert(SplatProcessTask(task))
                    .remove::<PendingSplatCloud>();
                continue;
            }
            match asset_server.get_load_state(&pending.0) {
                Some(LoadState::Failed(err)) => {
                    warn!("splat cloud {} failed to load: {err}", cloud.source);
                    commands
                        .entity(entity)
                        .remove::<(PendingSplatCloud, SplatLoadTimer)>();
                }
                // Loaded, yet gone from the store: another entity with the
                // same source took the shared asset first. Load our own copy.
                Some(LoadState::Loaded) => asset_server.reload(cloud.source.clone()),
                _ => {} // still loading
            }
            continue;
        }

        if processed_with.is_some_and(|w| w.cull_floaters != cloud.cull_floaters) {
            commands.entity(entity).insert((
                PendingSplatCloud(asset_server.load(cloud.source.clone())),
                SplatLoadTimer(Instant::now()),
            ));
        }
    }
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

/// Set on a [`SplatCloud`] entity whose live cloud was processed with the
/// floater cull on, and removed when it was processed with the cull off.
/// Informational: [`process_splat_clouds`] tracks its own state.
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

/// Spawn a Gaussian-splat cloud from an asset path (`.ply` / `.gcloud`).
///
/// Returns the spawned entity. The cloud goes through the same load, process
/// and upload pipeline as an imported one ([`process_splat_clouds`]);
/// `Visibility` arrives with the renderer handle once processing ends. We also
/// attach a [`Transform`], a [`SplatCloud`] for editor integration, and a
/// [`Name`].
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
            PendingSplatCloud(asset_server.load(path.clone())),
            SplatLoadTimer(Instant::now()),
            cloud_settings(),
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
        // The raw cloud loads into a main-world-only asset; the renderer
        // handle arrives once processing has produced the final cloud.
        PendingSplatCloud(asset_server.load(path.clone())),
        SplatLoadTimer(Instant::now()),
        cloud_settings(),
        SplatCloud { source: path, cull_floaters, ppisp },
    ));
}

/// Render settings shared by every Eustress splat cloud.
fn cloud_settings() -> CloudSettings {
    CloudSettings {
        // 16-bit depth keys halve the GPU radix-sort passes vs the 32-bit
        // default. A single captured scene spans metres–hundreds of
        // metres, so 65K depth buckets are far below visible popping.
        radix_sort_depth_bits:
            bevy_gaussian_splatting::gaussian::settings::RadixSortDepthBits::Bits16,
        ..Default::default()
    }
}
