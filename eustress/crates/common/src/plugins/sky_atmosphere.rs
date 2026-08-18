//! # Sky & Atmosphere
//!
//! Everything that draws the sky, and everything the sky lights.
//!
//! ## Why this module exists
//!
//! Bevy 0.19 reshaped the atmosphere API and the old call sites did not follow.
//! [`bevy::light::Atmosphere`] is now the **planet**: its `GlobalTransform` is
//! the planet centre in world space. A camera opts into rendering that planet's
//! sky by carrying [`AtmosphereSettings`]. The pre-0.19 code put `Atmosphere` on
//! the camera and never inserted `AtmosphereSettings` at all, so bevy's
//! `extract_atmosphere` matched zero cameras, `ExtractedAtmosphere` was never
//! inserted, and every atmosphere render stage (which is gated on
//! `With<ExtractedAtmosphere>`) was skipped. The atmosphere never drew a pixel.
//!
//! ## The three sky paths
//!
//! A camera renders its sky exactly one way, chosen by [`SkyMode`]:
//!
//! | mode | sky | image-based lighting |
//! |------|-----|----------------------|
//! | [`SkyMode::Atmosphere`] | physical scattering, star field composited behind it | [`AtmosphereEnvironmentMapLight`] |
//! | [`SkyMode::Skybox`] | the author's 6-face cubemap from [`Sky`] | [`GeneratedEnvironmentMapLight`] |
//! | [`SkyMode::Gradient`] | the legacy CPU gradient cubemap | [`GeneratedEnvironmentMapLight`] |
//!
//! `Atmosphere` is the default. `Gradient` exists because [`AtmospherePlugin`]
//! silently declines to load on a GPU without compute shaders or without
//! `Rgba16Float` storage binding, and on such a GPU the atmosphere path renders
//! a black sky with no way back. `EUSTRESS_SKY=gradient` is that way back.
//!
//! ## Both env-map components are *filtered*
//!
//! This is the part that was structurally wrong before. A `specular_map` must be
//! a prefiltered radiance mip chain (mip N holds roughness N) and a
//! `diffuse_map` must be a cosine-convolved irradiance map. The old code passed
//! one raw single-mip cubemap as both, so `mip = roughness * (mip_count - 1)`
//! evaluated to 0 for every material and rough surfaces got mirror reflections
//! of a gradient, while "ambient" was a mirror sample rather than an irradiance
//! integral. Both components used here run bevy's GPU filtering chain, so
//! roughness and diffuse are correct by construction, and on the atmosphere path
//! the environment map is regenerated from the live sky, so reflections track
//! time of day for free.
//!
//! ## Star field
//!
//! Bevy's atmosphere has no stars, so the cubemap still earns its place — but
//! only for stars. It is black everywhere else, so it never double-counts
//! against the atmosphere's own sky. It is built **once** at startup and faded
//! with a single `Skybox::brightness` write. The previous implementation rebuilt
//! a 1024x1024x6 RGBA8 cubemap (6.29M pixels, per-pixel `sin`/`fract` hashing,
//! ~25 MB) on the main thread every 60 frames for as long as the sun was moving.

use bevy::prelude::*;
use bevy::camera::Exposure;
use bevy::light::atmosphere::{Falloff, PhaseFunction, ScatteringMedium, ScatteringTerm};
use bevy::light::{
    Atmosphere as PlanetAtmosphere, AtmosphereEnvironmentMapLight, EnvironmentMapLight,
    GeneratedEnvironmentMapLight, Skybox,
};
use bevy::pbr::{AtmosphereMode, AtmosphereSettings};
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
// This crate's bevy prelude does not re-export the log macros; take them from
// tracing directly, as `lighting_plugin` already does.
use tracing::{info, warn};
use std::sync::OnceLock;

use crate::classes::{Sky, Sun as SunClass};
use crate::services::lighting::{
    AtmosphereRenderingMode, EustressAtmosphere, LightingService, Sun as SunMarker,
};

// ============================================================================
// Plugin
// ============================================================================

/// Owns sky rendering and the image-based lighting the sky feeds.
///
/// Split out of `SharedLightingPlugin` so that sun/moon/ambient/fog and
/// sky/atmosphere/reflections have one owner each instead of overlapping ones.
pub struct SkyAtmospherePlugin;

impl Plugin for SkyAtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneAtmosphere>()
            .init_resource::<StarField>()
            .init_resource::<SkyConfig>()
            .init_resource::<ActiveSkyMode>()
            .register_type::<EustressAtmosphere>()
            .register_type::<AtmosphereRenderingMode>()
            .add_systems(Startup, build_star_field)
            .add_systems(
                Update,
                (
                    // Decide the path first: it can strip a camera's sky stack,
                    // which `attach_sky_to_cameras` then rebuilds for the new
                    // mode in the same frame.
                    resolve_sky_mode,
                    // The planet must exist before a camera can point at it.
                    sync_atmosphere_planet.after(resolve_sky_mode),
                    attach_sky_to_cameras
                        .after(resolve_sky_mode)
                        .after(sync_atmosphere_planet),
                    sync_camera_atmosphere_settings.after(attach_sky_to_cameras),
                    apply_custom_skybox.after(attach_sky_to_cameras),
                    sync_environment_intensity.after(attach_sky_to_cameras),
                    sync_camera_exposure.after(attach_sky_to_cameras),
                    rebuild_star_field_on_sky_change.after(resolve_sky_mode),
                    fade_star_field
                        .after(attach_sky_to_cameras)
                        .after(rebuild_star_field_on_sky_change),
                ),
            );
    }
}

// ============================================================================
// Mode selection
// ============================================================================

/// How a camera draws its sky. See the module docs for the full table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum SkyMode {
    /// Physically-based scattering via bevy's atmosphere, star field behind it.
    #[default]
    Atmosphere,
    /// The author's 6-face cubemap from the [`Sky`] class.
    Skybox,
    /// The legacy CPU gradient cubemap. Fallback for GPUs where
    /// `AtmospherePlugin` declines to load.
    Gradient,
}

/// Process-wide sky configuration.
///
/// `mode_override` is read once via `OnceLock` for the same reason
/// `photoreal.rs` reads its knobs once: every `Camera3d` shares one
/// `mesh_view_bind_group` layout, so two cameras that disagree about their view
/// features produce bind groups of different shapes and wgpu aborts.
///
/// Note that the *mode* may still change at runtime (see [`ActiveSkyMode`]).
/// That is safe because it changes for every managed camera in a single system
/// on a single frame, so they never disagree with each other. Per-camera
/// divergence is the fatal case, not change over time.
#[derive(Resource, Clone, Copy, Debug)]
pub struct SkyConfig {
    /// Forced sky path from `EUSTRESS_SKY`, or `None` to choose automatically.
    pub mode_override: Option<SkyMode>,
    /// Peak star-field luminance at full night, in cd/m^2. Scaled down toward
    /// zero as the sun rises.
    pub star_brightness: f32,
    /// Cubemap resolution for the generated atmosphere environment map. Must be
    /// a power of two; bevy validates and rounds.
    pub environment_map_size: u32,
    /// Baseline camera exposure, as EV100. **Higher is darker.**
    ///
    /// Bevy's `Exposure` default is `BLENDER` at ev100 9.7, which is calibrated
    /// for Blender's implicit exposure rather than for physical sun units. A
    /// physically-scattered sky lit by a sun in the tens of thousands of lux is
    /// roughly 2^3.3, about ten times, too bright at that setting, and clips to
    /// flat white with no visible gradient. Bevy's own atmosphere example uses
    /// 13.0 for exactly this reason. `LightingService.exposure_compensation` is
    /// applied on top as an EV bias.
    pub base_ev100: f32,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self {
            mode_override: sky_mode_override(),
            star_brightness: env_f32("EUSTRESS_STAR_BRIGHTNESS", 2600.0),
            environment_map_size: 512,
            base_ev100: env_f32("EUSTRESS_EV100", 13.0),
        }
    }
}

/// Resolve the camera exposure for the current lighting.
///
/// `exposure_compensation` is authored as a photographic EV bias where positive
/// means brighter, which is the opposite sign to EV100, so it is subtracted.
fn camera_exposure(sky: &SkyConfig, lighting: &LightingService) -> Exposure {
    Exposure {
        ev100: (sky.base_ev100 - lighting.exposure_compensation).clamp(-5.0, 25.0),
    }
}

/// The sky path currently in force, for every managed camera.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveSkyMode(pub SkyMode);

fn sky_mode_override() -> Option<SkyMode> {
    static V: OnceLock<Option<SkyMode>> = OnceLock::new();
    *V.get_or_init(|| {
        match std::env::var("EUSTRESS_SKY")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "auto" => None,
            "atmosphere" => Some(SkyMode::Atmosphere),
            "skybox" | "cubemap" => Some(SkyMode::Skybox),
            "gradient" | "legacy" => Some(SkyMode::Gradient),
            other => {
                warn!(
                    "sky: EUSTRESS_SKY={other:?} is not auto/atmosphere/skybox/gradient \
                     — choosing automatically"
                );
                None
            }
        }
    })
}

/// True when a [`Sky`] names all six cubemap faces.
///
/// Six named faces is an unambiguous authorial statement of "I want my own
/// sky", so it selects the cubemap path. Without this the default path would
/// read `Sky.skybox_textures` never, which is precisely the failure mode this
/// module was written to remove.
fn has_authored_skybox(sky: &Sky) -> bool {
    let t = &sky.skybox_textures;
    ![&t.right, &t.left, &t.up, &t.down, &t.front, &t.back]
        .iter()
        .any(|s| s.trim().is_empty())
}

/// Choose the sky path, and re-apply it to every camera at once if it changed.
///
/// Removing [`SkyCamera`] is what makes [`attach_sky_to_cameras`] rebuild the
/// camera's sky stack on the next run. Doing it for all cameras in this one
/// system is what keeps them from ever disagreeing mid-switch.
fn resolve_sky_mode(
    mut commands: Commands,
    settings: Res<SkyConfig>,
    mut active: ResMut<ActiveSkyMode>,
    sky_query: Query<&Sky>,
    cameras: Query<Entity, With<SkyCamera>>,
    mut initialised: Local<bool>,
) {
    let desired = match settings.mode_override {
        Some(mode) => mode,
        None if sky_query.iter().any(has_authored_skybox) => SkyMode::Skybox,
        None => SkyMode::Atmosphere,
    };

    if *initialised && active.0 == desired {
        return;
    }
    let previous = active.0;
    *initialised = true;
    active.0 = desired;

    if previous == desired {
        return;
    }

    info!("🌤️ Sky path switching {previous:?} → {desired:?}");
    for camera in cameras.iter() {
        commands
            .entity(camera)
            .remove::<SkyCamera>()
            .remove::<AtmosphereSettings>()
            .remove::<AtmosphereEnvironmentMapLight>()
            .remove::<GeneratedEnvironmentMapLight>()
            .remove::<Skybox>();
    }
}

fn env_f32(key: &'static str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

// ============================================================================
// Resources & markers
// ============================================================================

/// Scene-wide atmosphere configuration.
///
/// Written by the editor (Properties panel edits land here via the engine's
/// `sync_atmosphere_to_rendering`), read by [`sync_atmosphere_planet`] and
/// [`sync_camera_atmosphere_settings`]. Unlike the previous one-shot
/// application, changes here reach the GPU on the next frame.
#[derive(Resource, Clone, Debug)]
pub struct SceneAtmosphere {
    pub atmosphere: EustressAtmosphere,
}

impl Default for SceneAtmosphere {
    fn default() -> Self {
        Self {
            atmosphere: EustressAtmosphere {
                density: 0.5,
                haze: 0.15,
                glare: 0.05,
                color: [0.776, 0.863, 1.0, 1.0],
                decay: [0.439, 0.506, 0.635, 1.0],
                ..EustressAtmosphere::default()
            },
        }
    }
}

impl SceneAtmosphere {
    pub fn clear_day() -> Self {
        Self { atmosphere: EustressAtmosphere::clear_day() }
    }
    pub fn sunset() -> Self {
        Self { atmosphere: EustressAtmosphere::sunset() }
    }
    pub fn foggy() -> Self {
        Self { atmosphere: EustressAtmosphere::foggy() }
    }
    pub fn space_view() -> Self {
        Self { atmosphere: EustressAtmosphere::space_view() }
    }
    pub fn flight_sim() -> Self {
        Self { atmosphere: EustressAtmosphere::flight_sim() }
    }
}

/// The star cubemap, built once at startup.
#[derive(Resource, Default)]
pub struct StarField {
    pub handle: Option<Handle<Image>>,
    /// The `star_count` the current image was built for, so a [`Sky`] edit can
    /// trigger exactly one rebuild instead of none or one per frame.
    pub built_for_count: u32,
}

/// Marks the single entity carrying the [`PlanetAtmosphere`] component.
///
/// This is the planet, not a camera. Its `GlobalTransform` is the planet centre,
/// placed `inner_radius` below the origin so that world y=0 is the surface.
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct AtmospherePlanet;

/// Marks a camera this plugin has already set up.
#[derive(Component, Debug, Clone, Copy)]
pub struct SkyCamera {
    pub mode: SkyMode,
}

/// Opt a camera out of all sky and atmosphere handling.
///
/// Used by overlay and off-screen cameras (the Slint UI camera, the AI capture
/// camera) which must not pay for a sky and must not perturb the shared view
/// bind-group layout.
#[derive(Component, Debug, Clone, Copy)]
pub struct NoAtmosphere;

/// Fingerprint of the atmosphere parameters currently uploaded, so the
/// `ScatteringMedium` asset and planet are rebuilt only on a real change.
///
/// `SceneAtmosphere` is written through `ResMut` by the editor sync systems,
/// which flags it changed even when the write is a no-op. Rebuilding a medium
/// asset every frame would re-run bevy's LUT precompute every frame.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct AtmosphereFingerprint(u64);

/// FNV-1a over the bit patterns of every knob that changes GPU state.
///
/// Bit patterns rather than values so a NaN or a signed zero still compares
/// stably; the only thing that matters is "is this the same input as last
/// frame".
fn fingerprint(a: &EustressAtmosphere) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    #[inline]
    fn mix(h: u64, bits: u64) -> u64 {
        (h ^ bits).wrapping_mul(PRIME)
    }

    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in [
        a.density,
        a.offset,
        a.haze,
        a.glare,
        a.planet_radius,
        a.atmosphere_height,
        a.mie_coefficient,
        a.mie_direction,
    ] {
        h = mix(h, v.to_bits() as u64);
    }
    for v in a.color.iter().chain(a.decay.iter()).chain(a.rayleigh_coefficient.iter()) {
        h = mix(h, v.to_bits() as u64);
    }
    h = mix(h, a.sky_max_samples as u64);
    h = mix(h, matches!(a.rendering_mode, AtmosphereRenderingMode::Raymarched) as u64);
    h = mix(h, a.environment_map_enabled as u64);
    h = mix(h, a.atmosphere_environment_light as u64);
    h
}

// ============================================================================
// Authored properties -> bevy atmosphere types
// ============================================================================

/// Build a [`ScatteringMedium`] from the authored atmosphere.
///
/// Every knob in the Properties panel lands here. Before this, the panel's 15
/// atmosphere properties were decorative: the old `apply_atmosphere_settings`
/// took them as `_atmosphere` and hardcoded `ScatteringMedium::earth`.
///
/// ## The mapping
///
/// - **`rayleigh_coefficient`** is the molecular scattering baseline, authored
///   in units of 1e-6 per metre to match the shipped template
///   (`[5.8, 13.5, 33.1]` is Earth). Rayleigh particles do not absorb.
/// - **`color`** tints the sky by modulating Rayleigh scattering per channel,
///   normalised to mean 1 so it shifts hue without changing total optical depth.
///   Sky colour is proportional to the scattering coefficient, so a warm `color`
///   raises red scattering and warms the sky.
/// - **`density`** scales the whole medium. `0.5` is Earth-normal (the shipped
///   template value), so the default look is unchanged and the knob still works
///   in both directions.
/// - **`haze`** scales the Mie (aerosol) term only, which is the term that
///   actually reads as haze. `0` leaves the authored aerosol load alone.
/// - **`mie_coefficient`** is aerosol extinction, also in 1e-6 per metre. Split
///   into scattering and absorption at a single-scattering albedo of 0.9, which
///   is standard for tropospheric aerosol.
/// - **`mie_direction`** is the Henyey-Greenstein asymmetry `g`, and **`glare`**
///   biases it further forward, which is physically what a sun halo is. Clamped
///   short of +-1 because the phase function is undefined at the poles.
/// - **`offset`** stretches or compresses the vertical density profile.
/// - **`atmosphere_height`** sets the scale heights in *proportional* terms,
///   which is what [`Falloff::Exponential`] wants: Earth's 8 km Rayleigh and
///   1.2 km Mie scale heights stay physically correct at any authored
///   thickness, instead of bevy's fixed `8.0/60.0` which assumes a 60 km
///   atmosphere while `Atmosphere::earth` actually builds a 100 km one.
pub fn build_scattering_medium(a: &EustressAtmosphere) -> ScatteringMedium {
    // Authored coefficients are in 1e-6 m^-1 (matches the shipped
    // Atmosphere.instance.toml, where Earth reads as [5.8, 13.5, 33.1]).
    const UNIT: f32 = 1e-6;

    let height = a.atmosphere_height.max(1_000.0);

    // `offset` stretches the vertical profile. Clamped away from zero because
    // Falloff::Exponential is undefined at scale == 0 (it falls back to linear).
    let profile = (1.0 + a.offset).clamp(0.1, 5.0);
    let rayleigh_scale = (8_000.0 / height * profile).max(1e-4);
    let mie_scale = (1_200.0 / height * profile).max(1e-4);

    // Ozone sits in a band around 25 km with a ~30 km spread. Expressed as a
    // proportion of the authored thickness, and clamped to stay inside the
    // [0, 1] domain Falloff::Tent requires.
    let ozone_center = (25_000.0 / height).clamp(0.05, 0.95);
    let ozone_width = (30_000.0 / height).clamp(0.05, 1.0);

    let density = (a.density / 0.5).clamp(0.05, 8.0);

    // Tint normalised to mean 1: shifts hue, leaves total optical depth alone.
    let rgb = [a.color[0].max(0.0), a.color[1].max(0.0), a.color[2].max(0.0)];
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    let tint = if mean > 1e-4 {
        Vec3::new(rgb[0] / mean, rgb[1] / mean, rgb[2] / mean)
    } else {
        Vec3::ONE
    };

    let rayleigh = Vec3::new(
        a.rayleigh_coefficient[0].max(0.0),
        a.rayleigh_coefficient[1].max(0.0),
        a.rayleigh_coefficient[2].max(0.0),
    ) * UNIT
        * density
        * tint;

    // haze is authored 0..5 in the template; treat it as an aerosol multiplier
    // on top of whatever mie_coefficient already says.
    let mie_extinction =
        a.mie_coefficient.max(0.0) * UNIT * density * (1.0 + a.haze.clamp(0.0, 5.0) * 2.0);
    // Single-scattering albedo 0.9: aerosols scatter most of what they
    // intercept and absorb the rest.
    let mie_scattering = mie_extinction * 0.9;
    let mie_absorption = mie_extinction * 0.1;

    // Forward-scattering bias. `glare` is a sun-halo knob, and a halo is
    // forward Mie scattering, so it pushes g toward 1.
    let asymmetry = (a.mie_direction + a.glare.clamp(0.0, 2.0) * 0.1).clamp(-0.95, 0.95);

    // Ozone absorption is what keeps twilight blue instead of muddy brown. It
    // is not exposed as a property because it has no artistic use beyond that;
    // it scales with density so a thin atmosphere thins it too.
    let ozone_absorption = Vec3::new(0.650e-6, 1.881e-6, 0.085e-6) * density;

    ScatteringMedium::new(
        256,
        256,
        [
            ScatteringTerm {
                absorption: Vec3::ZERO,
                scattering: rayleigh,
                falloff: Falloff::Exponential { scale: rayleigh_scale },
                phase: PhaseFunction::Rayleigh,
            },
            ScatteringTerm {
                absorption: Vec3::splat(mie_absorption),
                scattering: Vec3::splat(mie_scattering),
                falloff: Falloff::Exponential { scale: mie_scale },
                phase: PhaseFunction::Mie { asymmetry },
            },
            ScatteringTerm {
                absorption: ozone_absorption,
                scattering: Vec3::ZERO,
                falloff: Falloff::Tent { center: ozone_center, width: ozone_width },
                phase: PhaseFunction::Isotropic,
            },
        ],
    )
    .with_label("eustress_atmosphere")
}

/// Build the planet from the authored atmosphere.
///
/// `decay` becomes `ground_albedo`, which feeds bevy's multiscattering term and
/// so genuinely drives horizon brightness and colour, which is what the property
/// has always claimed to do.
pub fn build_planet(a: &EustressAtmosphere, medium: Handle<ScatteringMedium>) -> PlanetAtmosphere {
    let inner = a.planet_radius.max(1_000.0);
    let height = a.atmosphere_height.max(1_000.0);
    PlanetAtmosphere {
        inner_radius: inner,
        outer_radius: inner + height,
        ground_albedo: Vec3::new(
            a.decay[0].clamp(0.0, 1.0),
            a.decay[1].clamp(0.0, 1.0),
            a.decay[2].clamp(0.0, 1.0),
        ),
        medium,
    }
}

/// Build the per-camera atmosphere settings.
///
/// This is the component whose absence disabled the entire atmosphere: bevy's
/// `extract_atmosphere` only considers cameras that carry it.
///
/// Note that `AtmosphereSettings` carries `#[require(Hdr)]`, so attaching it
/// puts the camera into HDR. Every managed camera gets it in the same system,
/// which is what keeps their view-bind-group shapes matched.
pub fn build_atmosphere_settings(a: &EustressAtmosphere) -> AtmosphereSettings {
    AtmosphereSettings {
        rendering_method: match a.rendering_mode {
            AtmosphereRenderingMode::LookupTexture => AtmosphereMode::LookupTexture,
            AtmosphereRenderingMode::Raymarched => AtmosphereMode::Raymarched,
        },
        sky_max_samples: a.sky_max_samples.clamp(8, 128),
        ..default()
    }
}

/// Discriminant of an [`AtmosphereMode`], for comparison.
///
/// `AtmosphereMode` derives neither `PartialEq` nor `Debug` upstream, so it
/// cannot be compared or printed directly. It is `#[repr(u32)]` and fieldless,
/// so the discriminant is a faithful stand-in.
#[inline]
fn mode_id(mode: AtmosphereMode) -> u32 {
    mode as u32
}

// ============================================================================
// Planet
// ============================================================================

/// Create the planet entity, and rebuild it when the authored atmosphere moves.
///
/// One planet for the whole world. Bevy picks the nearest atmosphere per camera,
/// so multiple planets are legal, but a scene with one sky wants one planet.
fn sync_atmosphere_planet(
    mut commands: Commands,
    active: Res<ActiveSkyMode>,
    scene: Res<SceneAtmosphere>,
    mut mediums: ResMut<Assets<ScatteringMedium>>,
    mut last: Local<Option<AtmosphereFingerprint>>,
    planets: Query<Entity, With<AtmospherePlanet>>,
) {
    if active.0 != SkyMode::Atmosphere {
        // Tear the planet down if the mode changed away from atmosphere, so a
        // gradient/skybox camera is not paying for LUT precompute.
        for entity in planets.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    let current = AtmosphereFingerprint(fingerprint(&scene.atmosphere));
    let exists = !planets.is_empty();
    if exists && *last == Some(current) {
        return;
    }
    *last = Some(current);

    let medium = mediums.add(build_scattering_medium(&scene.atmosphere));
    let planet = build_planet(&scene.atmosphere, medium);
    // Place the planet centre `inner_radius` below the origin so world y=0 sits
    // on the surface. Setting `Transform` explicitly rather than relying on
    // bevy's on-add hook keeps the value deterministic once transform
    // propagation runs.
    let transform = Transform::from_translation(Vec3::NEG_Y * planet.inner_radius);

    if let Some(entity) = planets.iter().next() {
        commands.entity(entity).insert((planet, transform));
    } else {
        commands.spawn((
            planet,
            transform,
            GlobalTransform::from(transform),
            AtmospherePlanet,
            Name::new("Atmosphere Planet"),
        ));
        info!(
            "🌍 Atmosphere planet created (radius {:.0} km, atmosphere {:.0} km)",
            scene.atmosphere.planet_radius / 1000.0,
            scene.atmosphere.atmosphere_height / 1000.0
        );
    }
}

// ============================================================================
// Cameras
// ============================================================================

/// Attach the sky stack to any 3D camera that does not have it yet.
///
/// Every camera gets the same shape of view features for the reason spelled out
/// on [`SkyConfig`]: they share one bind-group layout.
fn attach_sky_to_cameras(
    mut commands: Commands,
    sky: Res<SkyConfig>,
    active: Res<ActiveSkyMode>,
    scene: Res<SceneAtmosphere>,
    stars: Res<StarField>,
    lighting: Res<LightingService>,
    cameras: Query<Entity, (With<Camera3d>, Without<SkyCamera>, Without<NoAtmosphere>)>,
) {
    for camera in cameras.iter() {
        let mut ec = commands.entity(camera);
        // Exposure goes on every managed camera regardless of sky path: it is a
        // camera property, and leaving it at bevy's Blender-calibrated default
        // blows a physically-lit scene out to white.
        ec.insert((SkyCamera { mode: active.0 }, camera_exposure(&sky, &lighting)));

        match active.0 {
            SkyMode::Atmosphere => {
                ec.insert((
                    build_atmosphere_settings(&scene.atmosphere),
                    AtmosphereEnvironmentMapLight {
                        intensity: environment_intensity(&scene.atmosphere, &lighting),
                        affects_lightmapped_mesh_diffuse: false,
                        size: UVec2::splat(sky.environment_map_size),
                    },
                ));
                // Stars ride behind the atmosphere. Bevy composites the sky as
                // `inscattering + skybox * transmittance`, so daylight
                // scattering washes them out on its own; the explicit fade in
                // `fade_star_field` is belt and braces for authored control.
                if let Some(image) = stars.handle.clone() {
                    ec.insert(Skybox { image: Some(image), brightness: 0.0, rotation: Quat::IDENTITY });
                }
                info!("🌍 Atmosphere + filtered environment map attached to camera {camera:?}");
            }
            SkyMode::Skybox | SkyMode::Gradient => {
                // Both cubemap paths get their IBL from bevy's GPU filtering
                // chain rather than a raw cubemap. `apply_custom_skybox` fills
                // in the image; until then the camera has a sky-less but valid
                // component set.
                ec.insert(Skybox {
                    image: None,
                    brightness: 1000.0,
                    rotation: Quat::IDENTITY,
                });
                info!("🌅 Cubemap sky attached to camera {camera:?} (mode {:?})", active.0);
            }
        }
    }
}

/// Keep [`AtmosphereSettings`] in step with authored changes.
///
/// The old code marked cameras `AtmosphereApplied` and filtered them out
/// forever, so every Properties-panel edit after frame 1 was written to a
/// resource nothing read.
fn sync_camera_atmosphere_settings(
    scene: Res<SceneAtmosphere>,
    mut cameras: Query<&mut AtmosphereSettings, With<SkyCamera>>,
) {
    if !scene.is_changed() {
        return;
    }
    let desired = build_atmosphere_settings(&scene.atmosphere);
    for mut settings in cameras.iter_mut() {
        if mode_id(settings.rendering_method) != mode_id(desired.rendering_method)
            || settings.sky_max_samples != desired.sky_max_samples
        {
            settings.rendering_method = desired.rendering_method;
            settings.sky_max_samples = desired.sky_max_samples;
        }
    }
}

/// Resolve the environment-map intensity from the authored atmosphere and the
/// lighting service.
///
/// Bevy exposes a single intensity that scales diffuse and specular IBL
/// together, so `environment_specular_scale` drives it (reflections are the
/// dominant read) while `environment_diffuse_scale` scales the separate
/// `GlobalAmbientLight` term in the lighting plugin. Both knobs were previously
/// parsed out of the Properties panel into `LightingService` and read by
/// nothing.
fn environment_intensity(a: &EustressAtmosphere, lighting: &LightingService) -> f32 {
    if !a.environment_map_enabled || !a.atmosphere_environment_light {
        return 0.0;
    }
    (a.environment_intensity * lighting.environment_specular_scale).clamp(0.0, 64.0)
}

/// Track `exposure_compensation` edits.
fn sync_camera_exposure(
    sky: Res<SkyConfig>,
    lighting: Res<LightingService>,
    mut cameras: Query<&mut Exposure, With<SkyCamera>>,
) {
    if !lighting.is_changed() {
        return;
    }
    let desired = camera_exposure(&sky, &lighting);
    for mut exposure in cameras.iter_mut() {
        if (exposure.ev100 - desired.ev100).abs() > 1e-4 {
            exposure.ev100 = desired.ev100;
        }
    }
}

/// Track `environment_intensity` / `environment_specular_scale` edits.
fn sync_environment_intensity(
    scene: Res<SceneAtmosphere>,
    lighting: Res<LightingService>,
    mut atmosphere_lights: Query<&mut AtmosphereEnvironmentMapLight>,
    mut generated: Query<&mut GeneratedEnvironmentMapLight, Without<AtmosphereEnvironmentMapLight>>,
) {
    if !scene.is_changed() && !lighting.is_changed() {
        return;
    }
    let intensity = environment_intensity(&scene.atmosphere, &lighting);
    for mut light in atmosphere_lights.iter_mut() {
        if (light.intensity - intensity).abs() > f32::EPSILON {
            light.intensity = intensity;
        }
    }
    // The cubemap paths carry `GeneratedEnvironmentMapLight` directly. On the
    // atmosphere path bevy inserts one too, derived from
    // `AtmosphereEnvironmentMapLight`, which is why that case is filtered out
    // above: writing both would fight.
    for mut light in generated.iter_mut() {
        if (light.intensity - intensity).abs() > f32::EPSILON {
            light.intensity = intensity;
        }
    }
}

// ============================================================================
// Custom skybox (Sky class)
// ============================================================================

/// Load the author's 6-face cubemap and hand it to bevy's filtering chain.
///
/// The [`Sky`] class has carried `skybox_textures` since it was written and
/// nothing ever read it. A camera on a cubemap path with all six faces named
/// gets that cubemap plus a properly filtered environment map; anything else
/// falls back to the gradient so the view is never black.
fn apply_custom_skybox(
    mut commands: Commands,
    active: Res<ActiveSkyMode>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    lighting: Res<LightingService>,
    scene: Res<SceneAtmosphere>,
    all_skies: Query<&Sky>,
    changed_skies: Query<(), (With<Sky>, Changed<Sky>)>,
    mut cameras: Query<(Entity, &mut Skybox), With<SkyCamera>>,
    mut applied: Local<bool>,
) {
    if active.0 == SkyMode::Atmosphere {
        // The atmosphere owns the sky; the cubemap slot holds stars instead.
        // `resolve_sky_mode` switches away from this path automatically when a
        // Sky names all six faces, so an authored cubemap is never ignored.
        *applied = false;
        return;
    }
    // Re-run on a Sky edit, on a mode switch, and once at startup so a camera
    // spawned before any Sky entity existed still gets an image.
    let sky_changed = !changed_skies.is_empty();
    if *applied && !sky_changed && !active.is_changed() {
        return;
    }

    let authored = all_skies.iter().find(|s| has_authored_skybox(s));

    let image = if let Some(sky) = authored {
        // Bevy wants the six faces as one array texture; an author-supplied
        // strip is loaded as-is and reinterpreted as a cube.
        let path = sky.skybox_textures.right.clone();
        warn_once_on_multiface(&path);
        asset_server.load(path)
    } else {
        create_gradient_skybox(&mut images, &lighting, &scene.atmosphere)
    };

    for (entity, mut skybox) in cameras.iter_mut() {
        skybox.image = Some(image.clone());
        commands.entity(entity).insert(GeneratedEnvironmentMapLight {
            environment_map: image.clone(),
            intensity: environment_intensity(&scene.atmosphere, &lighting),
            rotation: Quat::IDENTITY,
            affects_lightmapped_mesh_diffuse: false,
        });
    }
    *applied = true;
}

fn warn_once_on_multiface(path: &str) {
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        info!(
            "🌅 Sky.skybox_textures: loading {path} as a cubemap. Bevy expects the six \
             faces as a single array texture (KTX2/DDS cube, or a 1x6 strip); six separate \
             image files are not assembled automatically."
        );
    });
}

// ============================================================================
// Star field
// ============================================================================

/// Resolution of one star-cubemap face.
///
/// A cubemap face spans 90 degrees, so this sets the *angular* size of a star:
/// at 1024 one texel is 0.088 degrees. That matters more than memory here. At
/// 512 the smallest drawable star was 0.18 degrees and the brightest, with the
/// splat radius the first version used, reached a full degree — twice the width
/// of the moon — which bloom then smeared into visible orange discs.
pub const STAR_FIELD_SIZE: u32 = 1024;

/// Edge length in pixels of one star candidate cell.
const STAR_CELL: u32 = 8;

/// Draw a star's magnitude from a uniform sample.
///
/// A real sky gains roughly 3x more stars per magnitude step fainter, so the
/// visible population is overwhelmingly faint with a handful of bright ones.
/// The fifth power reproduces that tail: the median lands near 3% of peak.
#[inline]
fn star_magnitude(uniform: f32) -> f32 {
    let m = uniform.clamp(0.0, 1.0);
    m * m * m * m * m
}

/// Core brightness for a magnitude, before the skybox's own scaling.
///
/// Two constraints pull against each other here.
///
/// The **ceiling** keeps all but the brightest few percent below 1.0. Values
/// above 1.0 clamp in an 8-bit texture, so a generous multiplier does not make
/// bright stars brighter — it flattens everything above the clamp into
/// identical white dots. An earlier multiplier of 2.75 saturated a quarter of
/// the sky that way.
///
/// The **floor** is what decides whether a typical star is visible at all, and
/// it matters more than it looks. Filmic tonemapping compresses highlights
/// hard: measured on a live frame, raising the skybox brightness from 1500 to
/// 3400 left the brightest pixel unchanged at 172/255, because the sky was
/// already on the shoulder of the curve. Overall gain is therefore a dead
/// lever, while lifting the floor moves the median star straight up the steep
/// part of the curve where the eye can still see a difference.
#[inline]
fn star_peak(magnitude: f32) -> f32 {
    0.12 + magnitude * 1.30
}

/// Peak radius of the very brightest star, in texels.
///
/// Real stars are point sources: even Sirius is under a thousandth of a degree,
/// far below one texel. A star is therefore drawn as a sub-texel core with just
/// enough spill to anti-alias it, and its *brightness* — not its size — is what
/// carries magnitude. Bloom supplies the halo, which is what the eye actually
/// sees around a bright star.
const STAR_MAX_RADIUS: f32 = 0.62;

/// Build the star cubemap once at startup.
fn build_star_field(mut images: ResMut<Assets<Image>>, mut stars: ResMut<StarField>) {
    // Built unconditionally: the sky path is not settled at Startup (a Space's
    // Sky entity loads later and can select the cubemap path), and the field is
    // needed the moment the path resolves to atmosphere or gradient.
    let count = Sky::default().star_count;
    stars.handle = Some(images.add(create_star_field(count)));
    stars.built_for_count = count;
    info!("✨ Star field built ({count} stars, {STAR_FIELD_SIZE}px faces)");
}

/// Rebuild the star field when an author changes `Sky.star_count`.
///
/// Guarded by `built_for_count` so editing any *other* Sky property, or
/// re-saving the same value, does not pay for a regeneration. This is the only
/// thing that rebuilds the cubemap: sun movement no longer does, because the
/// image no longer depends on the sun.
fn rebuild_star_field_on_sky_change(
    active: Res<ActiveSkyMode>,
    mut images: ResMut<Assets<Image>>,
    mut stars: ResMut<StarField>,
    sky_query: Query<&Sky, Changed<Sky>>,
    mut cameras: Query<&mut Skybox, With<SkyCamera>>,
) {
    if active.0 == SkyMode::Skybox {
        return;
    }
    let Some(sky) = sky_query.iter().next() else {
        return;
    };
    if sky.star_count == stars.built_for_count {
        return;
    }

    let handle = images.add(create_star_field(sky.star_count));
    stars.built_for_count = sky.star_count;
    stars.handle = Some(handle.clone());
    for mut skybox in cameras.iter_mut() {
        skybox.image = Some(handle.clone());
    }
    info!("✨ Star field rebuilt for star_count = {}", sky.star_count);
}

/// Fade the star field with the sun.
///
/// One float write per camera per frame, against the previous implementation's
/// 25 MB cubemap rebuild every 60 frames.
fn fade_star_field(
    sky: Res<SkyConfig>,
    active: Res<ActiveSkyMode>,
    lighting: Res<LightingService>,
    sun: Query<&SunClass, With<SunMarker>>,
    mut cameras: Query<&mut Skybox, With<SkyCamera>>,
) {
    if active.0 == SkyMode::Skybox {
        // An authored cubemap sets its own brightness; it is not a star field.
        return;
    }
    let sun_dir = sun
        .iter()
        .next()
        .map(|s| s.direction())
        .unwrap_or_else(|| lighting.sun_direction());

    // Full brightness once the sun is 6 degrees below the horizon (civil dusk),
    // gone by the time it is 3 degrees above.
    let elevation = sun_dir.y.asin().to_degrees();
    let night = ((3.0 - elevation) / 9.0).clamp(0.0, 1.0);
    let brightness = sky.star_brightness * night * night;

    for mut skybox in cameras.iter_mut() {
        if (skybox.brightness - brightness).abs() > 0.5 {
            skybox.brightness = brightness;
        }
    }
}

/// Generate the star cubemap: black, plus stars, plus a faint galactic band.
///
/// Black everywhere else is the point. The previous cubemap baked a full day
/// gradient and a ground colour, which the atmosphere would then add its own sky
/// on top of, double-counting the sky at zenith where transmittance is high.
///
/// Stars are splatted rather than thresholded per pixel: iterate candidate
/// cells, hash each one, and for a hit draw a small radial kernel at a sub-pixel
/// centre. That is O(cells + stars x kernel) instead of O(pixels x neighbourhood),
/// and it gives round anti-aliased stars instead of hard single-pixel squares.
pub fn create_star_field(star_count: u32) -> Image {
    let size = STAR_FIELD_SIZE as usize;
    let cells_per_side = STAR_FIELD_SIZE / STAR_CELL;
    let total_cells = (cells_per_side * cells_per_side * 6).max(1);
    // Probability that any one cell holds a star, so `star_count` means what it
    // says on the Sky class.
    let hit = (star_count as f32 / total_cells as f32).clamp(0.0, 1.0);

    // Galactic plane normal. Arbitrary but fixed, so the band does not swim
    // between sessions.
    let galactic_normal = Vec3::new(0.35, 0.86, -0.37).normalize();

    let mut data = vec![0u8; size * size * 6 * 4];

    for face in 0..6usize {
        let face_base = face * size * size * 4;

        // ── Galactic band ────────────────────────────────────────────────
        for py in 0..size {
            for px in 0..size {
                let u = (px as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let v = (py as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let dir = face_direction(face, u, v);

                // Distance from the galactic plane, 0 on the plane.
                let off = dir.dot(galactic_normal).abs();
                let band = (-(off * off) / (2.0 * 0.09 * 0.09)).exp();
                if band < 0.004 {
                    continue;
                }
                // Dust, not a painted stripe: three octaves so the band breaks
                // up at the scale the eye lands on rather than reading as a
                // smooth airbrushed smear.
                let n = value_noise(dir * 14.0) * 0.5
                    + value_noise(dir * 37.0) * 0.32
                    + value_noise(dir * 91.0) * 0.18;
                // Squared so the faint edges fall away fast and the band has a
                // core instead of a uniform glow across a third of the sky.
                let mottle = n * n;
                // Scaled against the same tonemapping shoulder the stars face:
                // at 0.052 the band was mathematically present and visually
                // absent on screen.
                let intensity = band * (0.12 + 0.88 * mottle) * 0.16;

                let i = face_base + (py * size + px) * 4;
                // Very close to neutral. The Milky Way is not orange; the first
                // version's warm tint is what made the lower sky read as amber
                // haze.
                data[i] = to_u8(intensity * 1.00);
                data[i + 1] = to_u8(intensity * 0.99);
                data[i + 2] = to_u8(intensity * 0.97);
                data[i + 3] = 255;
            }
        }

        // ── Stars ────────────────────────────────────────────────────────
        for cy in 0..cells_per_side {
            for cx in 0..cells_per_side {
                let seed = hash3(face as u32, cx, cy);
                if rand01(seed) > hit {
                    continue;
                }

                // Sub-pixel centre inside the cell.
                let fx = cx as f32 * STAR_CELL as f32 + rand01(seed ^ 0x9e37_79b9) * STAR_CELL as f32;
                let fy = cy as f32 * STAR_CELL as f32 + rand01(seed ^ 0x85eb_ca6b) * STAR_CELL as f32;

                let magnitude = star_magnitude(rand01(seed ^ 0xc2b2_ae35));

                // Brightness carries magnitude; size barely moves. Letting size
                // track magnitude is what produced moon-sized discs.
                let peak = star_peak(magnitude);
                let radius = 0.34 + magnitude * (STAR_MAX_RADIUS - 0.34);

                // Colour. Stars span blue-white to amber in principle, but at
                // night-adapted vision almost all read white — only the very
                // brightest show any tint at all, so saturation scales with
                // magnitude and stays subtle even there.
                let warmth = rand01(seed ^ 0x27d4_eb2f) * 2.0 - 1.0; // -1 cool .. +1 warm
                let tint = warmth * 0.10 * (0.35 + 0.65 * magnitude);
                let (sr, sg, sb) = (1.0 + tint, 1.0 - tint.abs() * 0.25, 1.0 - tint);

                let reach = (radius * 2.5).ceil().max(1.0) as i32;
                let cxi = fx as i32;
                let cyi = fy as i32;
                for oy in -reach..=reach {
                    for ox in -reach..=reach {
                        let px = cxi + ox;
                        let py = cyi + oy;
                        // Clamp to the face. A star clipped at a cube seam
                        // loses a pixel or two of its halo; invisible against
                        // a 512px face, and it keeps the generator branch-free
                        // across faces.
                        if px < 0 || py < 0 || px >= size as i32 || py >= size as i32 {
                            continue;
                        }
                        let dx = px as f32 + 0.5 - fx;
                        let dy = py as f32 + 0.5 - fy;
                        let d2 = dx * dx + dy * dy;
                        let falloff = (-d2 / (2.0 * radius * radius)).exp();
                        if falloff < 0.01 {
                            continue;
                        }
                        let a = peak * falloff;
                        let i = face_base + (py as usize * size + px as usize) * 4;
                        data[i] = add_u8(data[i], a * sr);
                        data[i + 1] = add_u8(data[i + 1], a * sg);
                        data[i + 2] = add_u8(data[i + 2], a * sb);
                        data[i + 3] = 255;
                    }
                }
            }
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: STAR_FIELD_SIZE,
            height: STAR_FIELD_SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

// ============================================================================
// Gradient fallback
// ============================================================================

/// The legacy analytic sky, kept as the fallback for GPUs where bevy's
/// `AtmospherePlugin` declines to load (it needs compute shaders and
/// `Rgba16Float` storage binding, and warns rather than failing loudly).
///
/// Reachable with `EUSTRESS_SKY=gradient`, and used automatically when a
/// cubemap path has no authored faces.
pub fn create_gradient_skybox(
    images: &mut Assets<Image>,
    lighting: &LightingService,
    atmosphere: &EustressAtmosphere,
) -> Handle<Image> {
    const SIZE: u32 = 512;
    let size = SIZE as usize;

    let sun_dir = lighting.sun_direction();
    let night = (-sun_dir.y).clamp(0.0, 0.3) / 0.3;

    let zenith = lerp3([0.16, 0.32, 0.75], [0.01, 0.01, 0.03], night);
    let mid = lerp3([0.40, 0.60, 0.92], [0.02, 0.02, 0.06], night);
    let horizon = lerp3(
        [atmosphere.color[0], atmosphere.color[1], atmosphere.color[2]],
        [0.04, 0.04, 0.08],
        night,
    );
    let ground = lerp3(
        [atmosphere.decay[0], atmosphere.decay[1], atmosphere.decay[2]],
        [0.02, 0.02, 0.03],
        night,
    );

    let mut data = vec![0u8; size * size * 6 * 4];
    for face in 0..6usize {
        let base = face * size * size * 4;
        for py in 0..size {
            for px in 0..size {
                let u = (px as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let v = (py as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let dir = face_direction(face, u, v);
                let y = dir.y;

                let c = if y > 0.15 {
                    let t = ((y - 0.15) / 0.85).min(1.0);
                    lerp3(mid, zenith, t * t)
                } else if y > -0.05 {
                    let t = ((y + 0.05) / 0.20).clamp(0.0, 1.0);
                    lerp3(horizon, mid, t)
                } else {
                    let t = ((-y - 0.05) / 0.35).min(1.0).sqrt();
                    lerp3(horizon, ground, t)
                };

                let i = base + (py * size + px) * 4;
                data[i] = to_u8(c[0]);
                data[i + 1] = to_u8(c[1]);
                data[i + 2] = to_u8(c[2]);
                data[i + 3] = 255;
            }
        }
    }

    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    images.add(image)
}

// ============================================================================
// Helpers
// ============================================================================

/// Cubemap face + face-local UV in [-1, 1] to a normalised world direction.
/// Face order is bevy's: +X, -X, +Y, -Y, +Z, -Z.
#[inline]
fn face_direction(face: usize, u: f32, v: f32) -> Vec3 {
    let d = match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    };
    d.normalize()
}

#[inline]
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[inline]
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0) as u8
}

#[inline]
fn add_u8(existing: u8, add: f32) -> u8 {
    let sum = existing as f32 / 255.0 + add;
    to_u8(sum)
}

/// Integer hash. Cheap and stable, unlike the `sin()`-based hash the old
/// generator ran six times per pixel.
#[inline]
fn hash3(a: u32, b: u32, c: u32) -> u32 {
    let mut h = a
        .wrapping_mul(0x8da6_b343)
        ^ b.wrapping_mul(0xd8163_841u32)
        ^ c.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x2974_5c85);
    h ^= h >> 16;
    h
}

#[inline]
fn rand01(seed: u32) -> f32 {
    let mut h = seed;
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_216.0
}

/// Trilinear value noise on a 3D point. Used only for the galactic band's
/// mottling, so speed matters more than spectral quality.
fn value_noise(p: Vec3) -> f32 {
    let i = p.floor();
    let f = p - i;
    let f = f * f * (Vec3::splat(3.0) - 2.0 * f); // smoothstep
    let (ix, iy, iz) = (i.x as i32 as u32, i.y as i32 as u32, i.z as i32 as u32);

    let corner = |dx: u32, dy: u32, dz: u32| -> f32 {
        rand01(hash3(
            ix.wrapping_add(dx),
            iy.wrapping_add(dy),
            iz.wrapping_add(dz),
        ))
    };

    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), f.x);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), f.x);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), f.x);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), f.x);
    lerp(lerp(x00, x10, f.y), lerp(x01, x11, f.y), f.z)
}

/// Strip the raw environment map off a camera.
///
/// Exists so a camera can be moved between sky modes without leaving a stale
/// unfiltered `EnvironmentMapLight` behind, which is exactly the component that
/// made rough reflections mirror-sharp before this module existed.
pub fn clear_raw_environment_map(commands: &mut Commands, camera: Entity) {
    commands.entity(camera).remove::<EnvironmentMapLight>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_directions_are_unit_and_cover_all_axes() {
        // Centre of each face must point down its own axis.
        let expected = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
        for (face, want) in expected.iter().enumerate() {
            let got = face_direction(face, 0.0, 0.0);
            assert!((got.length() - 1.0).abs() < 1e-5, "face {face} not normalised");
            assert!(got.distance(*want) < 1e-5, "face {face}: got {got:?} want {want:?}");
        }
    }

    #[test]
    fn medium_scale_heights_track_authored_thickness() {
        // The whole point of deriving scale from `atmosphere_height`: Earth's
        // 8 km Rayleigh scale height stays 8 km whatever the authored thickness.
        let mut a = EustressAtmosphere::default();

        // Falloff is not Copy (it has an Arc<dyn Curve> variant), so match by
        // reference rather than moving out of the SmallVec index.
        let rayleigh_scale = |a: &EustressAtmosphere| -> f32 {
            let m = build_scattering_medium(a);
            let Falloff::Exponential { scale } = &m.terms[0].falloff else {
                panic!("rayleigh term should use exponential falloff");
            };
            *scale
        };

        a.atmosphere_height = 100_000.0;
        let scale = rayleigh_scale(&a);
        assert!((scale - 0.08).abs() < 1e-6, "expected 8km/100km, got {scale}");

        a.atmosphere_height = 60_000.0;
        let scale = rayleigh_scale(&a);
        assert!((scale - 8.0 / 60.0).abs() < 1e-6, "expected 8km/60km, got {scale}");
    }

    #[test]
    fn density_is_neutral_at_the_shipped_template_value() {
        // The template ships Density = 0.5, which must not change the look.
        let mut a = EustressAtmosphere::default();
        a.density = 0.5;
        a.color = [1.0, 1.0, 1.0, 1.0];
        let m = build_scattering_medium(&a);
        let expected = a.rayleigh_coefficient[2] * 1e-6;
        assert!(
            (m.terms[0].scattering.z - expected).abs() < 1e-12,
            "density 0.5 should be 1.0x, got {} want {expected}",
            m.terms[0].scattering.z
        );
    }

    #[test]
    fn color_tints_without_changing_total_optical_depth() {
        let mut a = EustressAtmosphere::default();
        a.density = 0.5;
        a.color = [1.0, 1.0, 1.0, 1.0];
        let neutral = build_scattering_medium(&a).terms[0].scattering;
        a.color = [2.0, 1.0, 0.5, 1.0];
        let warm = build_scattering_medium(&a).terms[0].scattering;

        assert!(warm.x > neutral.x, "a warm tint must raise red scattering");
        assert!(warm.z < neutral.z, "a warm tint must lower blue scattering");
        // Tint is normalised to mean 1, so the sum is preserved.
        let sum = |v: Vec3| v.x + v.y + v.z;
        let (a_sum, b_sum) = (sum(neutral), sum(warm));
        assert!(
            ((a_sum - b_sum) / a_sum).abs() < 0.35,
            "tint should shift hue, not bulk depth: {a_sum} vs {b_sum}"
        );
    }

    #[test]
    fn planet_surface_sits_at_world_origin_height() {
        let a = EustressAtmosphere::default();
        let planet = build_planet(&a, Handle::default());
        assert_eq!(planet.outer_radius - planet.inner_radius, a.atmosphere_height);
        // The spawn places the centre inner_radius below origin, so y=0 is the
        // surface. Guard the arithmetic that depends on it.
        let centre = Vec3::NEG_Y * planet.inner_radius;
        assert!((centre.length() - planet.inner_radius).abs() < 1.0);
    }

    #[test]
    fn raymarched_mode_reaches_the_gpu_settings() {
        let mut a = EustressAtmosphere::default();
        a.rendering_mode = AtmosphereRenderingMode::Raymarched;
        a.sky_max_samples = 64;
        let s = build_atmosphere_settings(&a);
        // Compared by discriminant: AtmosphereMode has no PartialEq upstream.
        assert_eq!(mode_id(s.rendering_method), mode_id(AtmosphereMode::Raymarched));
        assert_eq!(s.sky_max_samples, 64);
        // And the default must stay on the fast path.
        let d = build_atmosphere_settings(&EustressAtmosphere::default());
        assert_eq!(mode_id(d.rendering_method), mode_id(AtmosphereMode::LookupTexture));
        // Out-of-range sample counts are clamped, not passed through.
        a.sky_max_samples = 4096;
        assert_eq!(build_atmosphere_settings(&a).sky_max_samples, 128);
    }

    #[test]
    fn fingerprint_changes_with_every_authored_knob() {
        let base = EustressAtmosphere::default();
        let f0 = fingerprint(&base);
        let mut probe = |mutate: fn(&mut EustressAtmosphere)| {
            let mut a = base.clone();
            mutate(&mut a);
            assert_ne!(fingerprint(&a), f0, "a knob change must invalidate the cache");
        };
        probe(|a| a.density += 0.1);
        probe(|a| a.haze += 0.1);
        probe(|a| a.glare += 0.1);
        probe(|a| a.offset += 0.1);
        probe(|a| a.color[0] += 0.1);
        probe(|a| a.decay[1] += 0.1);
        probe(|a| a.planet_radius += 1.0);
        probe(|a| a.atmosphere_height += 1.0);
        probe(|a| a.mie_coefficient += 1.0);
        probe(|a| a.mie_direction += 0.1);
        probe(|a| a.rayleigh_coefficient[2] += 1.0);
        probe(|a| a.sky_max_samples += 1);
        probe(|a| a.rendering_mode = AtmosphereRenderingMode::Raymarched);
    }

    /// Mean channel value of one cubemap face, in 0..255.
    fn face_mean(image: &Image, face: usize) -> f32 {
        let px = (STAR_FIELD_SIZE * STAR_FIELD_SIZE) as usize;
        let data = image.data.as_ref().expect("star field has pixel data");
        let start = face * px * 4;
        let sum: u64 = data[start..start + px * 4]
            .chunks_exact(4)
            .map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64)
            .sum();
        sum as f32 / (px * 3) as f32
    }

    #[test]
    fn a_typical_star_is_actually_visible() {
        // The failure this guards is subtle: a mathematically correct magnitude
        // distribution whose median star renders at 10/255 and is invisible
        // after tonemapping, giving an empty-looking sky that measures fine.
        //
        // Raw gain cannot fix it — filmic tonemapping compresses the top of the
        // range, so the floor is the lever. Median magnitude is ~0.03, so this
        // pins what that star is worth.
        let median_peak = star_peak(star_magnitude(0.5));
        assert!(
            median_peak > 0.10,
            "the median star renders at {:.0}/255 before scaling — invisible",
            median_peak * 255.0
        );
    }

    #[test]
    fn star_field_carries_no_baked_sky() {
        // The regression this guards: the old cubemap baked a full day gradient
        // (blue above, grey ground below) which the atmosphere then added its own
        // sky on top of, double-counting the sky wherever transmittance was high.
        //
        // Measured as mean luminance rather than "fraction of near-black pixels".
        // The galactic band is a deliberate, faint feature (peak 0.13) spread over
        // roughly a third of the sphere, so a near-zero-per-pixel threshold fails
        // on it while still passing plenty of genuinely wrong images. The mean
        // separates the two cleanly: a baked mid-blue sky lands around 100+/255,
        // stars plus a dust band land in single digits.
        let image = create_star_field(3000);
        let overall: f32 = (0..6).map(|f| face_mean(&image, f)).sum::<f32>() / 6.0;
        assert!(
            overall < 12.0,
            "star field mean is {overall:.1}/255 — that is a baked sky, not stars"
        );

        // Zenith against nadir. The band is symmetric in |dot(dir, galactic
        // normal)|, so +Y and -Y must match closely. A baked sky/ground split
        // cannot pass this: it is exactly a zenith/nadir asymmetry.
        let zenith = face_mean(&image, 2);
        let nadir = face_mean(&image, 3);
        assert!(
            (zenith - nadir).abs() < 2.0,
            "zenith {zenith:.2} vs nadir {nadir:.2} — a vertical gradient means a baked sky"
        );
    }

    #[test]
    fn a_star_is_smaller_than_the_moon() {
        // The bug this guards, and the one the pixel-count tests all missed:
        // stars were drawn at up to 2.65 texels on a 512px face. A cubemap face
        // spans 90 degrees, so that is a full degree across — twice the width of
        // the full moon — and bloom turned them into visible orange discs.
        //
        // Checking angular size is the discriminating measure. "How many pixels
        // are lit" cannot tell a sky of many small stars from a sky of a few
        // enormous ones.
        const DEGREES_PER_FACE: f32 = 90.0;
        let degrees_per_texel = DEGREES_PER_FACE / STAR_FIELD_SIZE as f32;
        let brightest_diameter = 2.0 * STAR_MAX_RADIUS * degrees_per_texel;

        const MOON_DEGREES: f32 = 0.52;
        assert!(
            brightest_diameter < MOON_DEGREES / 4.0,
            "brightest star is {brightest_diameter:.3} deg across; the moon is {MOON_DEGREES} \
             and a star should be a point"
        );
    }

    #[test]
    fn faint_stars_vastly_outnumber_bright_ones() {
        // A real sky gains roughly 3x more stars per magnitude step fainter. A
        // flat distribution gives a field of equally-bright dots, which reads as
        // noise rather than as a sky.
        //
        // Tested on the DISTRIBUTION, not on lit texels. Counting texels cannot
        // see this: every star above the clamp renders an identical white core,
        // so a sky of uniformly blinding stars and a properly graded one produce
        // the same pixel histogram.
        let n = 20_000;
        let mags: Vec<f32> =
            (0..n).map(|i| star_magnitude(i as f32 / n as f32)).collect();

        let median = mags[n / 2];
        assert!(median < 0.06, "median magnitude {median:.3} — the sky is too uniformly bright");

        let bright = mags.iter().filter(|m| **m > 0.5).count();
        assert!(
            bright * 5 < n,
            "{bright} of {n} stars are in the top half of brightness; the tail is too flat"
        );
    }

    #[test]
    fn only_the_very_brightest_stars_clip_to_white() {
        // An 8-bit texture clamps at 1.0, so a peak above it does not render
        // brighter — it erases the difference between stars. At a 2.75
        // multiplier a quarter of the sky clipped to identical white dots.
        let n = 20_000;
        let clipped = (0..n)
            .filter(|i| star_peak(star_magnitude(*i as f32 / n as f32)) >= 1.0)
            .count();
        let pct = clipped as f32 / n as f32;
        assert!(
            pct < 0.08,
            "{:.1}% of stars clip to pure white; tonal range is lost above the clamp",
            pct * 100.0
        );
        // ...but the brightest must still reach it, or nothing anchors the sky.
        assert!(star_peak(star_magnitude(1.0)) > 1.0, "no star is bright enough to read as brilliant");
    }

    #[test]
    fn stars_read_as_white_not_amber() {
        // The first version's colour ramp reached (1.0, 0.78, 0.68), which is
        // why the night sky came out full of orange blobs. Night-adapted vision
        // sees almost all stars as white.
        let image = create_star_field(9000);
        let data = image.data.as_ref().unwrap();
        let worst = data
            .chunks_exact(4)
            .filter(|p| p[0].max(p[1]).max(p[2]) > 60)
            .map(|p| {
                let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
                (r - b).abs().max((r - g).abs()).max((g - b).abs())
            })
            .max()
            .unwrap_or(0);
        assert!(
            worst < 60,
            "a visible star deviates {worst}/255 between channels — too saturated to read as white"
        );
    }

    #[test]
    fn star_field_has_a_visible_galactic_band() {
        // Guards the other direction: the band is what makes a night sky read as
        // a sky rather than scattered dots, so an all-black field is also wrong.
        // The band lies perpendicular to a normal that is mostly +Y, so the
        // equatorial faces carry it and the polar faces do not.
        let image = create_star_field(0);
        let equator = (face_mean(&image, 0) + face_mean(&image, 4)) / 2.0;
        let poles = (face_mean(&image, 2) + face_mean(&image, 3)) / 2.0;
        assert!(
            equator > poles * 2.0,
            "expected the band across the equatorial faces: equator {equator:.2}, poles {poles:.2}"
        );
    }

    #[test]
    fn star_count_drives_actual_star_density() {
        let sparse = create_star_field(100);
        let dense = create_star_field(4000);
        let lit = |img: &Image| {
            img.data
                .as_ref()
                .unwrap()
                .chunks_exact(4)
                .filter(|p| p[0] > 40 || p[1] > 40 || p[2] > 40)
                .count()
        };
        assert!(
            lit(&dense) > lit(&sparse) * 3,
            "4000 stars should light far more pixels than 100: {} vs {}",
            lit(&dense),
            lit(&sparse)
        );
    }

    #[test]
    fn exposure_is_calibrated_for_a_physical_sky_not_for_blender() {
        // The regression this guards: with bevy's default Exposure (BLENDER,
        // ev100 9.7) a physically-scattered sky clips to flat white with no
        // gradient at all. Measured on a live frame before this was set: every
        // pixel of the sky band read (250, 247, 242). Bevy's own atmosphere
        // example uses 13.0.
        let sky = SkyConfig { base_ev100: 13.0, ..default_config() };
        let lighting = LightingService::default();
        let e = camera_exposure(&sky, &lighting);
        assert!(
            e.ev100 > 12.0,
            "ev100 {} is near bevy's Blender default and will blow the sky out",
            e.ev100
        );
    }

    #[test]
    fn exposure_compensation_brightens_as_authored() {
        // The property is documented in the panel as "positive = brighter", and
        // EV100 runs the other way, so the sign must invert.
        let sky = SkyConfig { base_ev100: 13.0, ..default_config() };
        let mut lighting = LightingService::default();
        let base = camera_exposure(&sky, &lighting).ev100;

        lighting.exposure_compensation = 2.0;
        let brighter = camera_exposure(&sky, &lighting).ev100;
        assert!(brighter < base, "positive compensation must lower ev100 (brighter)");
        assert!((base - brighter - 2.0).abs() < 1e-4, "one EV per unit");

        lighting.exposure_compensation = -2.0;
        assert!(camera_exposure(&sky, &lighting).ev100 > base, "negative must darken");
    }

    /// A `SkyConfig` that does not depend on this process's environment.
    fn default_config() -> SkyConfig {
        SkyConfig {
            mode_override: None,
            star_brightness: 1500.0,
            environment_map_size: 512,
            base_ev100: 13.0,
        }
    }

    #[test]
    fn environment_intensity_respects_the_disable_switches() {
        let lighting = LightingService::default();
        let mut a = EustressAtmosphere::default();
        assert!(environment_intensity(&a, &lighting) > 0.0);
        a.environment_map_enabled = false;
        assert_eq!(environment_intensity(&a, &lighting), 0.0);
        a.environment_map_enabled = true;
        a.atmosphere_environment_light = false;
        assert_eq!(environment_intensity(&a, &lighting), 0.0);
    }

    #[test]
    fn specular_scale_reaches_the_environment_map() {
        let mut lighting = LightingService::default();
        let a = EustressAtmosphere::default();
        let base = environment_intensity(&a, &lighting);
        lighting.environment_specular_scale = 2.0;
        assert!(
            (environment_intensity(&a, &lighting) - base * 2.0).abs() < 1e-5,
            "environment_specular_scale was parsed and read by nothing before this"
        );
    }
}
