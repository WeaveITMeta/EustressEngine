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
    }
}

/// Minimum EFFECTIVE (post-sigmoid) opacity a Gaussian must keep to survive the
/// floater cull. Real surface splats sit well above this; the faint "floater"
/// specks that hang in the air around a capture sit below it. Deliberately
/// conservative so the solid scene is never touched.
const FLOATER_MIN_OPACITY: f32 = 0.08;

/// Safety floor: if the cull would keep FEWER than this fraction of the cloud,
/// treat it as a mis-read opacity convention (not a real floater storm) and
/// SKIP — better to leave floaters than to wipe the scene. Retriable by
/// toggling `cull_floaters` off then on.
const FLOATER_MIN_KEEP_FRACTION: f32 = 0.25;

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
fn apply_floater_cull(
    mut assets: ResMut<Assets<PlanarGaussian3d>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    query: Query<(
        Entity,
        &SplatCloud,
        &PlanarGaussian3dHandle,
        Option<&FloaterCullApplied>,
    )>,
) {
    for (entity, cloud, handle, applied) in &query {
        match (cloud.cull_floaters, applied.is_some()) {
            // Enabled, not yet applied → filter once the asset is resident.
            (true, false) => {
                // Read + filter inside a scope so the immutable asset borrow
                // ends before the mutable write-back below.
                let filtered = {
                    let Some(data) = assets.get(&handle.0) else {
                        continue; // still loading — retry next frame
                    };
                    let total = data.iter().count();
                    if total == 0 {
                        continue;
                    }
                    // Convention probe on a bounded sample.
                    let looks_logit = data
                        .iter()
                        .take(4096)
                        .any(|g| g.scale_opacity.opacity < -0.001 || g.scale_opacity.opacity > 1.001);
                    let effective = |raw: f32| -> f32 {
                        if looks_logit {
                            1.0 / (1.0 + (-raw).exp())
                        } else {
                            raw
                        }
                    };
                    let kept: Vec<Gaussian3d> = data
                        .iter()
                        .filter(|g| effective(g.scale_opacity.opacity) >= FLOATER_MIN_OPACITY)
                        .collect();
                    let keep_frac = kept.len() as f32 / total as f32;
                    if keep_frac < FLOATER_MIN_KEEP_FRACTION {
                        warn!(
                            "SplatCloud floater cull would keep only {:.0}% of {} gaussians \
                             (opacity convention mis-read?) — skipping to avoid wiping {}",
                            keep_frac * 100.0,
                            total,
                            cloud.source
                        );
                        None
                    } else {
                        let removed = total - kept.len();
                        // `warn!` (not `info!`) so it survives the engine's
                        // hardcoded log filter during verification — the
                        // removal count is the one signal that confirms the
                        // cull ran and by how much. Drop to `info!` once the
                        // threshold is settled.
                        warn!(
                            "SplatCloud floater cull: removed {} / {} gaussians ({:.1}%) from {}",
                            removed,
                            total,
                            (removed as f32 / total as f32) * 100.0,
                            cloud.source
                        );
                        Some(PlanarGaussian3d::from_iter(kept))
                    }
                };
                if let Some(new_cloud) = filtered {
                    // Swap in a FRESH asset handle rather than mutating the
                    // existing asset in place. The upstream GPU planar-storage
                    // build reacts to a NEW cloud handle (exactly as at load
                    // time), NOT to an in-place `Assets` mutation — mutating
                    // the resident asset left the GPU buffers stale and the
                    // cloud rendered NOTHING ("splat disappeared after cull").
                    // Adding the filtered cloud as a new asset and re-pointing
                    // the handle re-runs the same load-time GPU derivation on
                    // the culled data, so it renders correctly.
                    let old_id = handle.0.id();
                    let new_handle = assets.add(new_cloud);
                    // Force-drop the PRE-cull cloud. `asset_server` keeps a
                    // strong handle to every path-loaded asset, so swapping the
                    // entity's handle alone does NOT free the original — its
                    // GPU planar storage stays resident and KEEPS RENDERING, so
                    // both the full and culled clouds draw at once (~2× the
                    // splats → the FPS floor + doubled memory). Explicitly
                    // removing the old asset drops its GPU version too, leaving
                    // only the culled cloud. (Single-cloud assumption: if two
                    // entities ever shared one `.ply` handle this would strand
                    // the other — revisit with per-entity cloud clones then.)
                    assets.remove(old_id);
                    commands
                        .entity(entity)
                        .insert(PlanarGaussian3dHandle(new_handle));
                }
                // Mark applied either way (a skipped cull must not retry every
                // frame); a toggle off→on clears the marker to re-evaluate.
                commands.entity(entity).insert(FloaterCullApplied);
            }
            // Disabled after being applied → reload the pristine cloud.
            (false, true) => {
                commands
                    .entity(entity)
                    .insert(PlanarGaussian3dHandle(asset_server.load(cloud.source.clone())))
                    .remove::<FloaterCullApplied>();
            }
            _ => {}
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
    /// When true, remove near-transparent "floater" Gaussians (the specks that
    /// hang in the air around a real capture) once the cloud asset loads. This
    /// is a GEOMETRIC prune (opacity threshold), distinct from [`Self::ppisp`]'s
    /// photometric correction. Surfaced as a Properties toggle; default ON.
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
        // The upstream sorter asserts `camera.order >= 0` (it uses the order as a
        // `usize` index into gaussian cameras — see bevy_gaussian_splatting
        // sort/mod.rs:166). The engine's offscreen / AI cameras use NEGATIVE
        // orders, so tagging them panics. Only tag on-screen (order >= 0)
        // cameras. Production should select the one intended viewport camera
        // explicitly rather than every order>=0 Camera3d.
        if camera.order >= 0 {
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
        PlanarGaussian3dHandle(asset_server.load(path.clone())),
        CloudSettings::default(),
        SplatCloud { source: path, cull_floaters, ppisp },
    ));
}
