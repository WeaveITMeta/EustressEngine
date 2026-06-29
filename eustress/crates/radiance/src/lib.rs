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
    gaussian::{cloud::CloudPlugin, settings::SettingsPlugin},
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
    }
}

/// Marker + display metadata for a Gaussian-splat cloud entity.
///
/// Carried alongside the upstream handle so the cloud can be shown in the
/// Explorer/Properties polymorphic inspector (splat source, later: count,
/// bounds, SH degree) and round-tripped through `instance_create` / WorldDb.
#[derive(Component, Reflect, Debug, Clone, Default)]
#[reflect(Component)]
pub struct SplatCloud {
    /// Source asset path the cloud was loaded from (for display / round-trip).
    pub source: String,
}

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
            SplatCloud { source: path },
            Name::new("SplatCloud"),
        ))
        .id()
}
