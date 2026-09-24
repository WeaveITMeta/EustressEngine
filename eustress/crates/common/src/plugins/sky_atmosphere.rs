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
//! The field turns about the celestial pole with the time of day, the way the
//! real sky does, and its brightness is compensated for night adaptation.
//!
//! ## Light levels on the CPU
//!
//! Bevy computes transmittance and sky light on the GPU and hands nothing
//! back, while several CPU decisions depend on them: camera exposure, the
//! ambient fill, the colour of sunlight reaching the clouds. [`SkyMedium`]
//! integrates the same [`ScatteringMedium`] the GPU renders, and
//! [`SkyLight`] holds this frame's result.
//!
//! ## Night adaptation
//!
//! A single exposure cannot serve both noon and a moonlit night: the two
//! differ by nineteen stops. [`SkyExposure`] adapts toward the light the way
//! the eye does, part of the way and up to [`MAX_NIGHT_ADAPTATION_EV`], so a
//! night reads as night rather than as black.

use bevy::prelude::*;
use bevy::camera::Exposure;
use bevy::light::atmosphere::{Falloff, PhaseFunction, ScatteringMedium, ScatteringTerm};
use bevy::light::{
    Atmosphere as PlanetAtmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfig,
    EnvironmentMapLight, FogVolume, GeneratedEnvironmentMapLight, Skybox, VolumetricFog,
};
use bevy::pbr::{AtmosphereMode, AtmosphereSettings};
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
// This crate's bevy prelude does not re-export the log macros; take them from
// tracing directly, as `lighting_plugin` already does.
use tracing::{info, warn};
use std::sync::OnceLock;

use crate::classes::{
    ecliptic_to_equatorial, solar_ecliptic_longitude, Moon as MoonClass, Sky, Sun as SunClass,
};
use crate::services::lighting::{
    AtmosphereRenderingMode, EustressAtmosphere, LightingService, Moon as MoonMarker,
    Sun as SunMarker,
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
            .init_resource::<SkyMedium>()
            .init_resource::<SkyLight>()
            .init_resource::<SkyExposure>()
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
                    // This frame's light levels, then the exposure they call for.
                    // Everything that reads either runs after `SkyLightSet`.
                    update_sky_light.in_set(SkyLightSet).after(sync_atmosphere_planet),
                    adapt_exposure.in_set(SkyLightSet).after(update_sky_light),
                    attach_sky_to_cameras
                        .after(resolve_sky_mode)
                        .after(sync_atmosphere_planet)
                        .after(SkyLightSet),
                    sync_camera_atmosphere_settings.after(attach_sky_to_cameras),
                    apply_custom_skybox.after(attach_sky_to_cameras),
                    sync_environment_intensity.after(attach_sky_to_cameras),
                    attach_exposure_to_opted_out_cameras.after(attach_sky_to_cameras),
                    sync_camera_exposure
                        .after(attach_sky_to_cameras)
                        .after(attach_exposure_to_opted_out_cameras),
                    rebuild_star_field_on_sky_change.after(resolve_sky_mode),
                    poll_star_field_build
                        .after(attach_sky_to_cameras)
                        .after(rebuild_star_field_on_sky_change),
                    fade_star_field
                        .after(attach_sky_to_cameras)
                        .after(poll_star_field_build),
                    sync_god_rays.after(attach_sky_to_cameras),
                ),
            )
            // Bevy's sky shaders assume a perspective camera; see
            // `orthographic_sky`. No-op without a renderer.
            .add_systems(
                Update,
                super::orthographic_sky::patch_sky_shaders
                    .run_if(resource_exists::<Assets<bevy::shader::Shader>>),
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
    ///
    /// This is the DAYLIGHT exposure. At dusk and at night [`SkyExposure`]
    /// opens it up by as much as [`MAX_NIGHT_ADAPTATION_EV`].
    pub base_ev100: f32,
    /// Whether the exposure adapts to the light at all. Off
    /// (`EUSTRESS_EXPOSURE_ADAPT=0`) pins every camera at `base_ev100`, which
    /// is what a measurement across times of day wants.
    pub exposure_adaptation: bool,
    /// Multiplier on the ground haze the sun's shafts are drawn in.
    /// `EUSTRESS_GOD_RAYS=0` switches the volumetric pass off entirely.
    pub god_rays: f32,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self {
            mode_override: sky_mode_override(),
            star_brightness: env_f32("EUSTRESS_STAR_BRIGHTNESS", 2600.0),
            environment_map_size: 512,
            base_ev100: env_f32("EUSTRESS_EV100", 13.0),
            exposure_adaptation: env_flag("EUSTRESS_EXPOSURE_ADAPT", true),
            god_rays: env_f32("EUSTRESS_GOD_RAYS", 1.0).clamp(0.0, 16.0),
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

/// An on/off switch: `0`, `false` and `off` are off, anything else set is on.
fn env_flag(key: &'static str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"),
        Err(_) => default,
    }
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

/// The star cubemap, built on its own thread: once at startup for the
/// default count, and again when a Space's Sky asks for another count.
#[derive(Resource, Default)]
pub struct StarField {
    pub handle: Option<Handle<Image>>,
    /// The `star_count` the current image was built for, so a [`Sky`] edit can
    /// trigger exactly one rebuild instead of none or one per frame.
    pub built_for_count: u32,
    /// A build in flight and the count it is for. `create_star_field` fills
    /// six 1024² faces with three noise samples per pixel, 2.3 s on the
    /// main thread when it ran inline (at startup, then once more when the
    /// Space's Sky loaded), so it runs on a thread and `poll_star_field_build`
    /// lands the result. `Receiver` is not `Sync`, hence the mutex.
    pub pending: Option<(u32, std::sync::Mutex<std::sync::mpsc::Receiver<Image>>)>,
}

impl StarField {
    /// Start a build for `count` unless one for that count is already in
    /// flight. A build for a different count supersedes the pending one:
    /// its image is dropped when it lands.
    fn spawn_build(&mut self, count: u32) {
        if self.pending.as_ref().map(|(c, _)| *c == count).unwrap_or(false) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<Image>();
        let spawned = std::thread::Builder::new()
            .name("eustress-star-field".into())
            .spawn(move || {
                let _ = tx.send(create_star_field(count));
            });
        match spawned {
            Ok(_) => self.pending = Some((count, std::sync::Mutex::new(rx))),
            Err(e) => warn!("star field build thread failed to spawn: {e} — no star field"),
        }
    }
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

    // Ozone sits in a band around 25 km with a ~30 km spread. Falloff
    // coordinates run from 1 at the ground to 0 at the top of the atmosphere
    // (bevy samples `Falloff` at `p = 1 - altitude / height`), so the band's
    // centre is `1 - 25 km / height`: 0.75 of a 100 km atmosphere, exactly
    // where `ScatteringMedium::earth` puts it. It used to be `25 km / height`,
    // which put the layer at 75 km, above almost all the air, where it could
    // no longer filter the grazing twilight light that makes the blue hour
    // blue. Clamped to stay inside the [0, 1] domain `Falloff::Tent` requires.
    let ozone_center = (1.0 - 25_000.0 / height).clamp(0.05, 0.95);
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
    exposure: Res<SkyExposure>,
    cameras: Query<Entity, (With<Camera3d>, Without<SkyCamera>, Without<NoAtmosphere>)>,
) {
    for camera in cameras.iter() {
        let mut ec = commands.entity(camera);
        // Exposure goes on every managed camera regardless of sky path: it is a
        // camera property, and leaving it at bevy's Blender-calibrated default
        // blows a physically-lit scene out to white.
        ec.insert((SkyCamera { mode: active.0 }, exposure.camera()));

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

/// Exposure for cameras that opt OUT of the sky ([`NoAtmosphere`]).
///
/// `attach_sky_to_cameras` grants `SkyCamera` and `Exposure` in one insert and
/// its query excludes `NoAtmosphere`, so a camera that opts out of the
/// atmosphere (the engine's off-screen AI camera, which must, to avoid the
/// multi-camera atmosphere prepare race) silently opted out of exposure as
/// well. It then rendered at bevy's Blender default, ev100 9.7, against a scene
/// calibrated for 13.0: roughly ten times too bright, every lit surface washed
/// to white, which made its captures useless for judging material or color.
/// Exposure is a camera property, not a sky property, so it goes on every
/// `Camera3d` the scene lights, atmosphere or not.
fn attach_exposure_to_opted_out_cameras(
    mut commands: Commands,
    exposure: Res<SkyExposure>,
    cameras: Query<Entity, (With<Camera3d>, With<NoAtmosphere>, Without<Exposure>)>,
) {
    for camera in cameras.iter() {
        commands.entity(camera).insert(exposure.camera());
    }
}

/// Carry the adapted exposure, and `exposure_compensation` edits, to every
/// camera that holds an `Exposure` this plugin manages (sky cameras and
/// opted-out ones alike, so an AI capture at night matches the viewport).
fn sync_camera_exposure(
    exposure: Res<SkyExposure>,
    mut cameras: Query<&mut Exposure, (With<Camera3d>, Or<(With<SkyCamera>, With<NoAtmosphere>)>)>,
) {
    if !exposure.is_changed() {
        return;
    }
    for mut camera in cameras.iter_mut() {
        if (camera.ev100 - exposure.camera_ev100).abs() > 1e-4 {
            camera.ev100 = exposure.camera_ev100;
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
// Sky light model
// ============================================================================

/// The systems that compute this frame's [`SkyLight`] and [`SkyExposure`].
/// Anything that reads either orders itself after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SkyLightSet;

/// Marks an object drawn at sky distance around the camera: the moon's disc
/// and the cloud dome. They are sized for a perspective view, so orthographic
/// views hide them.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SkyBillboard;

/// Moonlight is multiplied by this, so a moonlit night is legible.
///
/// A real full moon gives about 0.25 lux, nineteen stops under the sun. The
/// dark-adapted eye makes up most of that gap; an exposure that did the same
/// would blow every lamp and lit window out to white, since they are
/// authored against daylight. So the exposure adapts only
/// [`MAX_NIGHT_ADAPTATION_EV`] stops and the moon makes up the rest, the
/// film-maker's day-for-night. The sky/ground balance stays physical, because
/// the atmosphere scatters the moon's light exactly as it scatters the sun's.
pub const MOONLIGHT_GAIN: f32 = 30.0;

/// The most the exposure opens up for the night, EV.
pub const MAX_NIGHT_ADAPTATION_EV: f32 = 9.0;

/// The key illuminance, lux, at which the exposure sits at
/// [`SkyConfig::base_ev100`]: a clear sky with the sun high.
const KEY_LUX_AT_BASE: f32 = 70_000.0;

/// Fraction of a full log-luminance adaptation the exposure makes. Below 1 so
/// dusk and night still read darker than day, the way they do to the eye.
const ADAPTATION: f32 = 0.75;

/// Time constant of the adaptation, seconds. Short enough that dragging the
/// time of day never waits on it, long enough not to pop.
const ADAPTATION_SECONDS: f32 = 0.35;

/// Starlight and airglow on a moonless night, lux.
const AIRGLOW_LUX: f32 = 0.002;

/// Rec. 709 luminance of linear RGB.
#[inline]
pub fn luminance(rgb: Vec3) -> f32 {
    rgb.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// Linear RGB of an authored sRGB colour.
#[inline]
fn linear_rgb(srgb: [f32; 4]) -> Vec3 {
    let c = Color::srgb(srgb[0], srgb[1], srgb[2]).to_linear();
    Vec3::new(c.red, c.green, c.blue)
}

/// How much of a disc of `angular_radius` (radians) clears the horizon when
/// its centre stands at an elevation with sine `sin_elevation`. The linear
/// ramp bevy's `calculate_visible_sun_ratio` uses, so the CPU and the GPU
/// set the sun together.
#[inline]
pub fn disc_visibility(sin_elevation: f32, angular_radius: f32) -> f32 {
    let elevation = sin_elevation.clamp(-1.0, 1.0).asin();
    (0.5 + 0.5 * elevation / angular_radius.max(1e-4)).clamp(0.0, 1.0)
}

/// The authored atmosphere's scattering medium, on the CPU.
///
/// The same [`ScatteringMedium`] [`build_scattering_medium`] hands the GPU,
/// integrated here so that CPU decisions agree with the rendered sky.
#[derive(Resource, Clone)]
pub struct SkyMedium {
    medium: ScatteringMedium,
    planet_radius: f64,
    atmosphere_height: f64,
    fingerprint: u64,
}

impl Default for SkyMedium {
    fn default() -> Self {
        Self::from_atmosphere(&SceneAtmosphere::default().atmosphere)
    }
}

impl SkyMedium {
    pub fn from_atmosphere(a: &EustressAtmosphere) -> Self {
        Self {
            medium: build_scattering_medium(a),
            planet_radius: a.planet_radius.max(1_000.0) as f64,
            atmosphere_height: a.atmosphere_height.max(1_000.0) as f64,
            fingerprint: fingerprint(a),
        }
    }

    /// Extinction, per metre, at `altitude` metres above the ground.
    pub fn extinction(&self, altitude: f32) -> Vec3 {
        let p = (1.0 - altitude as f64 / self.atmosphere_height).clamp(0.0, 1.0) as f32;
        self.medium
            .terms
            .iter()
            .fold(Vec3::ZERO, |sum, term| {
                sum + (term.absorption + term.scattering) * term.falloff.sample(p)
            })
    }

    /// Transmittance from `altitude` metres out to space along a ray whose
    /// elevation has sine `sin_elevation`. Zero when the ray meets the ground.
    ///
    /// Integrated in double precision: the planet's radius squared is 4e13,
    /// and in f32 its rounding is as large as the horizon test's whole
    /// answer for a viewer a few metres up.
    pub fn transmittance(&self, altitude: f32, sin_elevation: f32) -> Vec3 {
        let r_ground = self.planet_radius;
        let r_top = r_ground + self.atmosphere_height;
        let r0 = r_ground + altitude.max(0.0) as f64;
        if r0 >= r_top {
            return Vec3::ONE;
        }
        let mu = sin_elevation.clamp(-1.0, 1.0) as f64;
        // Below the local horizontal the ray meets the ground if its closest
        // approach to the planet's centre falls inside the ground.
        if mu < 0.0 && r0 * r0 * (1.0 - mu * mu) <= r_ground * r_ground {
            return Vec3::ZERO;
        }
        let t_max = -r0 * mu + (r0 * r0 * (mu * mu - 1.0) + r_top * r_top).max(0.0).sqrt();
        // Steps crowd toward the start, where the air is thick: at a grazing
        // angle almost all the optical depth lies in the first tenth of a
        // path a thousand kilometres long.
        const STEPS: usize = 48;
        let mut depth = Vec3::ZERO;
        let mut previous = 0.0f64;
        for i in 1..=STEPS {
            let f = i as f64 / STEPS as f64;
            let t = t_max * f * f;
            let middle = 0.5 * (previous + t);
            let r = (r0 * r0 + middle * middle + 2.0 * r0 * middle * mu).sqrt();
            depth += self.extinction((r - r_ground) as f32) * (t - previous) as f32;
            previous = t;
        }
        (-depth).exp()
    }
}

/// This frame's light, from the sun and moon classes and the authored
/// atmosphere. Written by [`update_sky_light`] in [`SkyLightSet`].
#[derive(Resource, Clone, Debug)]
pub struct SkyLight {
    /// Toward the sun.
    pub sun_direction: Vec3,
    /// Toward the moon.
    pub moon_direction: Vec3,
    /// The sun's light above the atmosphere, as its `DirectionalLight`
    /// carries it on the atmosphere path: linear RGB times lux.
    pub sun_light: Vec3,
    /// The moon's the same way, its phase and [`MOONLIGHT_GAIN`] included.
    pub moon_light: Vec3,
    /// Sunlight reaching the ground, on a surface facing the sun.
    pub sun_ground: Vec3,
    /// Moonlight reaching the ground, on a surface facing the moon.
    pub moon_ground: Vec3,
    /// Diffuse light from the sky dome on a level surface, lux.
    pub sky_lux: f32,
    /// 0 in daylight, rising to 1 once the sun is 10 degrees down.
    pub night: f32,
    /// The illuminance the exposure adapts to, lux.
    pub key_lux: f32,
}

impl Default for SkyLight {
    fn default() -> Self {
        let sun = Vec3::splat(bevy::light::light_consts::lux::RAW_SUNLIGHT);
        Self {
            sun_direction: Vec3::Y,
            moon_direction: Vec3::NEG_Y,
            sun_light: sun,
            moon_light: Vec3::ZERO,
            sun_ground: sun,
            moon_ground: Vec3::ZERO,
            sky_lux: clear_sky_lux(90.0),
            night: 0.0,
            key_lux: KEY_LUX_AT_BASE,
        }
    }
}

/// Diffuse illuminance on a level surface from a clear sky lit by
/// `RAW_SUNLIGHT` standing at `elevation_deg`, lux.
///
/// Above the horizon, the clear-sky diffuse fit `0.8 + 15.5 sqrt(sin e)`
/// klux. Below it, twilight: about 0.42 decades per degree of depression,
/// from 800 lux at sunset to 2.4 at the end of civil twilight and 0.007 at
/// the end of nautical.
pub fn clear_sky_lux(elevation_deg: f32) -> f32 {
    if elevation_deg >= 0.0 {
        800.0 + 15_500.0 * elevation_deg.to_radians().sin().max(0.0).sqrt()
    } else {
        800.0 * 10f32.powf(0.42 * elevation_deg.max(-30.0))
    }
}

/// The Moon's brightness relative to full, from its elongation in degrees.
///
/// Allen's lunar phase law: the magnitude falls by `0.026 a + 4e-9 a^4` at
/// phase angle `a`. A quarter moon is about a tenth of a full one, not half:
/// most of the full moon's brilliance is the opposition surge, and the lit
/// fraction alone overstates every other phase.
pub fn moon_phase_brightness(elongation_deg: f32) -> f32 {
    let phase_angle = (180.0 - elongation_deg.rem_euclid(360.0)).abs();
    let delta_magnitude = 0.026 * phase_angle + 4.0e-9 * phase_angle.powi(4);
    10f32.powf(-0.4 * delta_magnitude)
}

/// The moon's light above the atmosphere, lux: the authored full-moon
/// illuminance, the phase and [`MOONLIGHT_GAIN`].
pub fn moonlight_lux(moon: &MoonClass) -> f32 {
    if !moon.enabled {
        return 0.0;
    }
    moon.full_intensity.max(0.0) * moon_phase_brightness(moon.elongation_from_sun()) * MOONLIGHT_GAIN
}

/// Moonlight's colour: the authored moon colour pulled toward the blue the
/// dark-adapted eye sees it as. Moonlight is physically a little redder than
/// sunlight; at night the rods take over and it reads blue, which is why
/// every convincing night is graded that way.
pub fn moonlight_color(moon: &MoonClass) -> Vec3 {
    linear_rgb(moon.color) * Vec3::new(0.78, 0.88, 1.14)
}

/// Integrate this frame's light: sun and moon above the atmosphere, what
/// reaches the ground, the sky's own light, and the key the exposure adapts
/// to.
fn update_sky_light(
    scene: Res<SceneAtmosphere>,
    lighting: Res<LightingService>,
    mut medium: ResMut<SkyMedium>,
    sun: Query<&SunClass, With<SunMarker>>,
    moon: Query<&MoonClass, With<MoonMarker>>,
    mut sky_light: ResMut<SkyLight>,
) {
    if medium.fingerprint != fingerprint(&scene.atmosphere) {
        *medium = SkyMedium::from_atmosphere(&scene.atmosphere);
    }
    let scale = super::lighting_plugin::brightness_scale(&lighting);
    const RAW_SUNLIGHT: f32 = bevy::light::light_consts::lux::RAW_SUNLIGHT;

    let authored_sun = sun.iter().next();
    let sun_class = authored_sun.cloned().unwrap_or_else(|| SunClass {
        time_of_day: lighting.time_of_day * 24.0,
        latitude: lighting.geographic_latitude,
        noon_color: lighting.sun_color,
        noon_intensity: lighting.sun_intensity,
        angular_size: lighting.sun_angular_radius * 2.0,
        ..default()
    });
    // Without a Sun class the light is aimed by the service's own model; the
    // sky must agree with where the light actually is.
    let sun_direction = match authored_sun {
        Some(s) => s.direction(),
        None => lighting.sun_direction(),
    };
    let sun_light = linear_rgb(sun_class.noon_color) * sun_class.noon_intensity.max(0.0) * scale;
    let sun_ground = sun_light
        * medium.transmittance(0.0, sun_direction.y)
        * disc_visibility(sun_direction.y, (sun_class.angular_size * 0.5).to_radians());

    // No Moon in the Space, no moonlight.
    let authored_moon = moon.iter().next();
    let moon_class = authored_moon.cloned().unwrap_or_default();
    let moon_direction = moon_class.direction_realistic(&sun_class);
    let moon_light = if authored_moon.is_some() {
        moonlight_color(&moon_class) * moonlight_lux(&moon_class) * scale
    } else {
        Vec3::ZERO
    };
    let moon_ground = moon_light
        * medium.transmittance(0.0, moon_direction.y)
        * disc_visibility(moon_direction.y, (moon_class.angular_size * 0.5).to_radians());

    let elevation = |d: Vec3| d.y.clamp(-1.0, 1.0).asin().to_degrees();
    let sun_elevation = elevation(sun_direction);
    let sky_lux = clear_sky_lux(sun_elevation) * luminance(sun_light) / RAW_SUNLIGHT
        + clear_sky_lux(elevation(moon_direction)) * luminance(moon_light) / RAW_SUNLIGHT;
    let night = ((2.0 - sun_elevation) / 12.0).clamp(0.0, 1.0);
    // Half the direct light: a view holds as much in shadow and facing away
    // as it does facing the light.
    let key_lux = 0.5 * (luminance(sun_ground) + luminance(moon_ground)) + sky_lux + AIRGLOW_LUX;

    *sky_light = SkyLight {
        sun_direction,
        moon_direction,
        sun_light,
        moon_light,
        sun_ground,
        moon_ground,
        sky_lux,
        night,
        key_lux,
    };
}

/// The exposure every managed camera runs at, adapted to the light.
#[derive(Resource, Clone, Copy, Debug)]
pub struct SkyExposure {
    /// Adapted EV100, before the author's `exposure_compensation`.
    pub adapted_ev100: f32,
    /// What the cameras carry: the adapted value, compensation applied.
    pub camera_ev100: f32,
    settled: bool,
}

impl Default for SkyExposure {
    fn default() -> Self {
        let base = SkyConfig::default().base_ev100;
        Self { adapted_ev100: base, camera_ev100: base, settled: false }
    }
}

impl SkyExposure {
    /// The camera component for the current exposure.
    pub fn camera(&self) -> Exposure {
        Exposure { ev100: self.camera_ev100 }
    }
}

/// The EV100 the view settles at under `key_lux`: part of the way from
/// `base_ev100` toward full adaptation, at most [`MAX_NIGHT_ADAPTATION_EV`]
/// below it and never above it, so daylight stays exactly as calibrated.
pub fn adapted_ev100(base_ev100: f32, key_lux: f32) -> f32 {
    let stops = (key_lux.max(1e-6) / KEY_LUX_AT_BASE).log2() * ADAPTATION;
    base_ev100 + stops.clamp(-MAX_NIGHT_ADAPTATION_EV, 0.0)
}

/// Move the exposure toward what this frame's light calls for.
fn adapt_exposure(
    time: Res<Time>,
    sky: Res<SkyConfig>,
    lighting: Res<LightingService>,
    sky_light: Res<SkyLight>,
    new_sun: Query<(), Added<SunMarker>>,
    mut exposure: ResMut<SkyExposure>,
) {
    let target = if sky.exposure_adaptation {
        adapted_ev100(sky.base_ev100, sky_light.key_lux)
    } else {
        sky.base_ev100
    };
    // A Space arriving (its sun appearing) snaps to its light instead of
    // fading up from whatever the last Space was lit by.
    let snap = !exposure.settled || !new_sun.is_empty();
    let adapted = if snap {
        target
    } else {
        let k = 1.0 - (-time.delta_secs() / ADAPTATION_SECONDS).exp();
        exposure.adapted_ev100 + (target - exposure.adapted_ev100) * k
    };
    let camera =
        (camera_exposure(&sky, &lighting).ev100 + (adapted - sky.base_ev100)).clamp(-5.0, 25.0);
    if snap
        || (adapted - exposure.adapted_ev100).abs() > 1e-4
        || (camera - exposure.camera_ev100).abs() > 1e-4
    {
        exposure.adapted_ev100 = adapted;
        exposure.camera_ev100 = camera;
        exposure.settled = true;
    }
}

// ============================================================================
// God rays
// ============================================================================

/// The haze the sun's shafts are drawn in: one camera-following
/// [`FogVolume`], lit through the sun's shadow maps by bevy's volumetric fog.
#[derive(Component, Debug, Clone, Copy)]
pub struct GodRayHaze;

/// Aerosol density of the haze at strength 1, per metre. Thin enough that
/// the scene reads clear side-on; toward the sun forward scattering makes it
/// glow, and wherever geometry shadows it the glow breaks into shafts.
const GOD_RAY_DENSITY: f32 = 0.0006;

/// Half the haze box's width, metres. Bevy lights fog only inside the sun's
/// shadow cascades, and haze beyond them would only darken what lies behind
/// it, so a shorter cascade range shrinks the box to match.
const GOD_RAY_REACH: f32 = 450.0;

/// Keep the god ray haze and every sky camera's `VolumetricFog` in step with
/// the sun.
fn sync_god_rays(
    mut commands: Commands,
    sky: Res<SkyConfig>,
    active: Res<ActiveSkyMode>,
    scene: Res<SceneAtmosphere>,
    sky_light: Res<SkyLight>,
    suns: Query<(&SunClass, Option<&CascadeShadowConfig>), With<SunMarker>>,
    cameras: Query<(Entity, &Camera, &GlobalTransform, Has<VolumetricFog>), With<SkyCamera>>,
    mut haze: Query<(Entity, &mut FogVolume, &mut Transform), With<GodRayHaze>>,
) {
    let sun = suns.iter().next();
    let authored = sun.map_or(1.0, |(s, _)| s.god_rays_intensity.max(0.0));
    let elevation = sky_light.sun_direction.y.clamp(-1.0, 1.0).asin().to_degrees();
    // Shafts need a sun to cast them. Fade in over the first degrees of
    // daylight rather than switching on at the horizon.
    let daylight = ((elevation + 1.0) / 6.0).clamp(0.0, 1.0);
    let strength = sky.god_rays * authored * daylight;

    if active.0 != SkyMode::Atmosphere || strength <= 1e-3 {
        for (camera, _, _, has_fog) in cameras.iter() {
            if has_fog {
                commands.entity(camera).remove::<VolumetricFog>();
            }
        }
        for (entity, ..) in haze.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    for (camera, _, _, has_fog) in cameras.iter() {
        if !has_fog {
            commands.entity(camera).insert(VolumetricFog {
                // The environment map already lights the scene from the sky;
                // ambient light in the fog would add it a second time.
                ambient_intensity: 0.0,
                jitter: 0.0,
                step_count: 64,
                ..default()
            });
        }
    }

    let Some((_, _, view, _)) = cameras
        .iter()
        .filter(|(_, camera, ..)| camera.is_active)
        .min_by_key(|(_, camera, ..)| camera.order)
    else {
        return;
    };
    let reach = sun
        .and_then(|(_, cascades)| cascades)
        .and_then(|c| c.bounds.last().copied())
        .map_or(GOD_RAY_REACH, |far| (far * 0.85).min(GOD_RAY_REACH))
        .max(20.0);
    let center = view.translation() + Vec3::Y * (reach * 0.12);
    let scale = Vec3::new(reach * 2.0, reach * 0.9, reach * 2.0);
    let density = GOD_RAY_DENSITY * strength * (1.0 + 2.0 * scene.atmosphere.haze.clamp(0.0, 5.0));

    match haze.iter_mut().next() {
        Some((_, mut volume, mut transform)) => {
            if (volume.density_factor - density).abs() > density * 0.01 {
                volume.density_factor = density;
            }
            if transform.translation.distance_squared(center) > 1.0
                || transform.scale.distance_squared(scale) > 1.0
            {
                transform.translation = center;
                transform.scale = scale;
            }
        }
        None => {
            commands.spawn((
                FogVolume {
                    density_factor: density,
                    // Aerosol scatters far more than it absorbs, which keeps
                    // the haze bright rather than smoky.
                    absorption: 0.08,
                    scattering: 0.55,
                    // Forward-peaked, as haze is: the glow and the shafts
                    // gather toward the sun.
                    scattering_asymmetry: 0.65,
                    ..default()
                },
                Transform::from_translation(center).with_scale(scale),
                GodRayHaze,
                Name::new("God Ray Haze"),
            ));
        }
    }
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
fn build_star_field(mut stars: ResMut<StarField>) {
    // Started unconditionally: the sky path is not settled at Startup (a
    // Space's Sky entity loads later and can select the cubemap path), and
    // the field is wanted as soon as the path resolves to atmosphere or
    // gradient. It lands through `poll_star_field_build`; until then the sky
    // simply has no stars, which is what the fade shows at dusk anyway.
    let count = Sky::default().star_count;
    stars.spawn_build(count);
    info!("✨ Star field build started ({count} stars, {STAR_FIELD_SIZE}px faces) on its own thread");
}

/// Land a finished star-field build: one image upload, every sky camera
/// re-pointed, and a skybox attached to any atmosphere camera that had none
/// because the handle was not ready when `attach_sky_to_cameras` ran.
fn poll_star_field_build(
    mut commands: Commands,
    active: Res<ActiveSkyMode>,
    mut images: ResMut<Assets<Image>>,
    mut stars: ResMut<StarField>,
    mut cameras: Query<(Entity, Option<&mut Skybox>), With<SkyCamera>>,
) {
    let (count, image) = {
        let Some((count, rx)) = stars.pending.as_ref() else { return };
        let image = match rx.lock() {
            Ok(r) => match r.try_recv() {
                Ok(img) => Some(img),
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
            },
            Err(_) => None,
        };
        (*count, image)
    };
    stars.pending = None;
    let Some(image) = image else {
        warn!("star field build thread ended without an image — no star field");
        return;
    };
    let handle = images.add(image);
    stars.built_for_count = count;
    stars.handle = Some(handle.clone());
    // Only the atmosphere path composites stars behind its sky; on the
    // gradient path the skybox slot holds the gradient itself.
    if active.0 == SkyMode::Atmosphere {
        for (camera, skybox) in cameras.iter_mut() {
            match skybox {
                Some(mut skybox) => skybox.image = Some(handle.clone()),
                None => {
                    commands.entity(camera).insert(Skybox {
                        image: Some(handle.clone()),
                        brightness: 0.0,
                        rotation: Quat::IDENTITY,
                    });
                }
            }
        }
    }
    info!("✨ Star field ready for star_count = {count}");
}

/// Rebuild the star field when an author changes `Sky.star_count`.
///
/// Guarded by `built_for_count` so editing any *other* Sky property, or
/// re-saving the same value, does not pay for a regeneration. This is the only
/// thing that rebuilds the cubemap: sun movement no longer does, because the
/// image no longer depends on the sun.
fn rebuild_star_field_on_sky_change(
    active: Res<ActiveSkyMode>,
    mut stars: ResMut<StarField>,
    sky_query: Query<&Sky, Changed<Sky>>,
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
    stars.spawn_build(sky.star_count);
    info!("✨ Star field rebuild started for star_count = {}", sky.star_count);
}

/// Fade the star field with the sun, keep it at its calibrated brightness
/// through night adaptation, and turn it with the sky.
///
/// One float and one quaternion per camera per frame, against the previous
/// implementation's 25 MB cubemap rebuild every 60 frames.
fn fade_star_field(
    sky: Res<SkyConfig>,
    active: Res<ActiveSkyMode>,
    lighting: Res<LightingService>,
    exposure: Res<SkyExposure>,
    sun: Query<&SunClass, With<SunMarker>>,
    skies: Query<&Sky>,
    mut cameras: Query<(&mut Skybox, Option<&Projection>), With<SkyCamera>>,
) {
    if active.0 != SkyMode::Atmosphere {
        // On the cubemap paths the skybox IS the sky (an authored cubemap or
        // the gradient), at its own brightness and orientation; fading it
        // like a star field blacked the gradient out by day.
        return;
    }
    let sun_class = sun.iter().next();
    let sun_dir = sun_class
        .map(|s| s.direction())
        .unwrap_or_else(|| lighting.sun_direction());

    // Full brightness once the sun is 6 degrees below the horizon (civil dusk),
    // gone by the time it is 3 degrees above.
    let elevation = sun_dir.y.clamp(-1.0, 1.0).asin().to_degrees();
    let night = ((3.0 - elevation) / 9.0).clamp(0.0, 1.0);
    let shown = skies.iter().next().map_or(true, |s| s.celestial_bodies_shown);
    // Bevy multiplies the skybox by the camera's exposure, so as the exposure
    // opens up for the night the stars would brighten with it, 512 times over
    // at full adaptation. Scaling by the adaptation keeps them where their
    // brightness was calibrated. Only the adaptation is taken out:
    // `exposure_compensation` is the author's, and should reach the stars.
    let adaptation = (exposure.adapted_ev100 - sky.base_ev100).exp2();
    let brightness = if shown { sky.star_brightness * night * night * adaptation } else { 0.0 };

    let rotation = match sun_class {
        Some(s) => star_sky_rotation(s.latitude, sidereal_angle(s)),
        None => star_sky_rotation(lighting.geographic_latitude, lighting.time_of_day * 360.0),
    };

    for (mut skybox, projection) in cameras.iter_mut() {
        // An orthographic view's rays are parallel, so every pixel would show
        // the one texel of the star map straight ahead: black, or a star
        // flooding the view as the camera turns. It shows none.
        let brightness = if matches!(projection, Some(Projection::Orthographic(_))) {
            0.0
        } else {
            brightness
        };
        if (skybox.brightness - brightness).abs() > (brightness * 0.01).max(1e-3) {
            skybox.brightness = brightness;
        }
        if skybox.rotation.angle_between(rotation) > 1e-5 {
            skybox.rotation = rotation;
        }
    }
}

/// The local sidereal angle, degrees: how far the sky has turned about the
/// celestial pole. The Sun's hour angle plus its right ascension, so the
/// stars keep their place relative to the Sun and gain one turn a year.
pub fn sidereal_angle(sun: &SunClass) -> f32 {
    let (sun_ra, _) = ecliptic_to_equatorial(solar_ecliptic_longitude(sun.day_of_year), 0.0);
    (sun.time_of_day - 12.0) * 15.0 + sun_ra
}

/// The rotation that carries the star cubemap onto the sky at `latitude`
/// after the sky has turned `sidereal_degrees` about the pole.
///
/// The cubemap's +Y is the north celestial pole. Turning about it by the
/// sidereal angle is the Earth's rotation; tilting +Y onto the pole at
/// `(0, sin lat, cos lat)` (a turn about the east axis) places that pole in
/// this latitude's sky. A positive turn about +Y carries east into the
/// southern meridian and on to the west, which is the way everything in the
/// sky moves through a day.
pub fn star_sky_rotation(latitude: f32, sidereal_degrees: f32) -> Quat {
    Quat::from_rotation_x((90.0 - latitude).to_radians())
        * Quat::from_rotation_y(sidereal_degrees.to_radians())
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
            exposure_adaptation: true,
            god_rays: 1.0,
        }
    }

    #[test]
    fn the_ozone_layer_sits_at_25_km() {
        // Falloff coordinates run 1 at the ground to 0 at the top, so the
        // tent's peak must be at `1 - 25 km / height`. It was at 75 km.
        let a = EustressAtmosphere::default();
        let m = build_scattering_medium(&a);
        let ozone = &m.terms[2].falloff;
        let at = |altitude: f32| ozone.sample(1.0 - altitude / a.atmosphere_height);
        assert!((at(25_000.0) - 1.0).abs() < 1e-4, "peak {}", at(25_000.0));
        assert!(at(75_000.0) < 0.05, "no ozone at 75 km: {}", at(75_000.0));
        assert!(at(25_000.0) > at(10_000.0) && at(25_000.0) > at(40_000.0));
    }

    #[test]
    fn a_high_sun_loses_little_and_a_setting_sun_is_red() {
        let medium = SkyMedium::default();
        let noon = medium.transmittance(0.0, 60f32.to_radians().sin());
        assert!(noon.x > 0.85 && noon.z > 0.6, "high sun {noon:?}");
        let low = medium.transmittance(0.0, 2f32.to_radians().sin());
        assert!(low.x > low.y && low.y > low.z, "a low sun reddens: {low:?}");
        assert!(low.z < noon.z * 0.2, "and loses most of its blue: {low:?}");
        // Monotonic in elevation, in every channel.
        let mut previous = Vec3::ZERO;
        for degrees in [0.5f32, 2.0, 5.0, 10.0, 20.0, 45.0, 90.0] {
            let t = medium.transmittance(0.0, degrees.to_radians().sin());
            assert!(t.cmpge(previous).all(), "{degrees} deg: {t:?} after {previous:?}");
            previous = t;
        }
    }

    #[test]
    fn the_ground_blocks_a_ray_below_the_horizon_but_altitude_sees_past_it() {
        let medium = SkyMedium::default();
        let below = (-0.5f32).to_radians().sin();
        assert_eq!(medium.transmittance(0.0, below), Vec3::ZERO);
        // 2.7 km up the horizon dips about 1.7 degrees: a sun half a degree
        // down still lights a cloud top.
        assert!(medium.transmittance(2_700.0, below).x > 0.0);
        // Space is clear.
        assert_eq!(medium.transmittance(200_000.0, 0.3), Vec3::ONE);
    }

    #[test]
    fn a_quarter_moon_is_a_tenth_of_a_full_one() {
        assert!((moon_phase_brightness(180.0) - 1.0).abs() < 1e-6);
        let quarter = moon_phase_brightness(90.0);
        assert!((0.06..0.14).contains(&quarter), "quarter moon {quarter}");
        assert!(moon_phase_brightness(0.0) < 1e-3, "a new moon gives no light");
        // Waxing and waning phases match.
        assert!((moon_phase_brightness(120.0) - moon_phase_brightness(240.0)).abs() < 1e-6);
    }

    #[test]
    fn night_adaptation_is_bounded_and_day_is_untouched() {
        let base = 13.0;
        // Bright daylight never darkens past the calibrated exposure.
        assert_eq!(adapted_ev100(base, 150_000.0), base);
        assert_eq!(adapted_ev100(base, KEY_LUX_AT_BASE), base);
        // Dusk opens up part of the way.
        let dusk = adapted_ev100(base, 700.0);
        assert!(dusk < base - 3.0 && dusk > base - MAX_NIGHT_ADAPTATION_EV, "dusk {dusk}");
        // A moonless night stops at the bound.
        assert_eq!(adapted_ev100(base, 0.001), base - MAX_NIGHT_ADAPTATION_EV);
        // Monotonic: less light is never a darker exposure.
        let mut previous = f32::INFINITY;
        for lux in [100_000.0f32, 20_000.0, 3_000.0, 400.0, 30.0, 2.0, 0.1] {
            let ev = adapted_ev100(base, lux);
            assert!(ev <= previous);
            previous = ev;
        }
    }

    #[test]
    fn adapted_stars_keep_their_calibrated_brightness() {
        // Bevy multiplies the skybox by the camera's exposure; the fade
        // divides the adaptation back out, so on-screen star brightness is
        // the same at any adaptation.
        let sky = default_config();
        let on_screen = |adapted_ev100: f32| {
            let skybox = sky.star_brightness * (adapted_ev100 - sky.base_ev100).exp2();
            skybox * Exposure { ev100: adapted_ev100 }.exposure()
        };
        let day = on_screen(sky.base_ev100);
        let night = on_screen(sky.base_ev100 - MAX_NIGHT_ADAPTATION_EV);
        assert!((day - night).abs() < day * 1e-4, "{day} vs {night}");
    }

    #[test]
    fn the_star_field_turns_about_the_celestial_pole() {
        for latitude in [-33.0f32, 0.0, 41.7, 70.0] {
            let pole = Vec3::new(0.0, latitude.to_radians().sin(), latitude.to_radians().cos());
            for sidereal in [0.0f32, 77.0, 190.0] {
                let r = star_sky_rotation(latitude, sidereal);
                assert!((r * Vec3::Y - pole).length() < 1e-5, "the cubemap pole must stay on the sky's");
            }
        }
    }

    #[test]
    fn the_stars_rise_in_the_east_and_set_in_the_west() {
        // A star on the celestial equator, 45 degrees north: at one sidereal
        // angle it is due east on the horizon, a quarter turn later on the
        // southern meridian, then due west. The same way the sun goes.
        let star = Vec3::X;
        let east = star_sky_rotation(45.0, 0.0) * star;
        let south = star_sky_rotation(45.0, 90.0) * star;
        let west = star_sky_rotation(45.0, 180.0) * star;
        assert!((east - Vec3::X).length() < 1e-5, "{east:?}");
        assert!(south.z < -0.5 && south.y > 0.5, "culminates in the south: {south:?}");
        assert!((west - Vec3::NEG_X).length() < 1e-5, "{west:?}");
        // And the sun's own path runs the same way through the day.
        let mut sun = SunClass { latitude: 45.0, day_of_year: 80, ..default() };
        sun.time_of_day = 6.0;
        let morning = sun.direction();
        sun.time_of_day = 18.0;
        let evening = sun.direction();
        assert!(morning.x > 0.9 && evening.x < -0.9, "{morning:?} {evening:?}");
    }

    #[test]
    fn the_sky_direction_is_the_suns_own_alt_azimuth() {
        // Moon and sun must share one conversion, or the lit side of the moon
        // cannot face the rendered sun.
        let sun = SunClass { latitude: 41.7, day_of_year: 172, time_of_day: 9.5, ..default() };
        let declination = 23.45 * ((360.0f32 / 365.0) * (sun.day_of_year as f32 - 81.0)).to_radians().sin();
        let via_sky = crate::classes::sky_direction((sun.time_of_day - 12.0) * 15.0, declination, sun.latitude);
        assert!((via_sky - sun.direction()).length() < 1e-3, "{via_sky:?} vs {:?}", sun.direction());
    }

    #[test]
    fn a_full_moon_stands_opposite_the_sun() {
        // The failure this guards: the old model put a summer full moon 133
        // degrees from the sun, high in the sky at midnight.
        let sun = SunClass { latitude: 45.0, day_of_year: 172, time_of_day: 0.0, ..default() };
        let moon = MoonClass { lunar_day: MoonClass::SYNODIC_MONTH / 2.0, ascending_node: 90.0, ..default() };
        let angle = sun.direction().angle_between(moon.direction_realistic(&sun)).to_degrees();
        assert!(angle > 170.0, "full moon {angle:.1} deg from the sun");
        // A summer full moon runs low: the sun's declination, reversed.
        let elevation = moon.direction_realistic(&sun).y.asin().to_degrees();
        assert!(elevation < 30.0, "summer full moon at {elevation:.1} deg");
        // A first quarter is a right angle from the sun.
        let quarter = MoonClass { lunar_day: MoonClass::SYNODIC_MONTH / 4.0, orbital_inclination: 0.0, ..default() };
        let noon = SunClass { time_of_day: 18.0, ..sun.clone() };
        let angle = noon.direction().angle_between(quarter.direction_realistic(&noon)).to_degrees();
        assert!((angle - 90.0).abs() < 3.0, "first quarter {angle:.1} deg from the sun");
    }

    #[test]
    fn the_sky_light_follows_the_sun_down() {
        let noon = clear_sky_lux(60.0);
        let sunset = clear_sky_lux(0.0);
        let civil = clear_sky_lux(-6.0);
        let nautical = clear_sky_lux(-12.0);
        assert!(noon > 10_000.0 && noon < 25_000.0, "clear midday sky {noon}");
        assert!((sunset - 800.0).abs() < 1.0);
        assert!((1.0..10.0).contains(&civil), "end of civil twilight {civil}");
        assert!(nautical < 0.05, "end of nautical twilight {nautical}");
    }

    #[test]
    fn the_sun_disc_eases_down_over_its_own_width() {
        let radius = 0.2665f32.to_radians();
        assert_eq!(disc_visibility(0.3, radius), 1.0);
        assert!((disc_visibility(0.0, radius) - 0.5).abs() < 1e-5);
        assert_eq!(disc_visibility(-0.3, radius), 0.0);
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
