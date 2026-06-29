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
use bevy_gaussian_splatting::{CloudSettings, GaussianSplattingPlugin, PlanarGaussian3dHandle};

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
        app.add_plugins(GaussianSplattingPlugin);
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
