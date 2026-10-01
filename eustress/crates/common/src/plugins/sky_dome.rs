//! # Sky dome
//!
//! The sky drawn when bevy's atmosphere is not: the `Gradient` path (the
//! fallback for GPUs without the atmosphere's compute shaders, and the light
//! mode Studio and the Player run in side by side, `EUSTRESS_SKY=gradient`),
//! and the sun's disc over an author's six-face skybox.
//!
//! The gradient cubemap alone could not be a sky: it held one daylight
//! gradient whatever the time, drew no sun, no sunset, no stars, and put its
//! zenith seven times darker than its horizon band, so a sunlit scene read as
//! a photographic negative, white ground under a navy sky, by day and by
//! night. This dome is drawn per pixel instead:
//!
//! - **The sky** is Preetham, Shirley and Smits' analytic clear sky ("A
//!   Practical Analytic Model for Daylight", 1999): a Perez distribution of
//!   luminance and chromaticity around the sun, with turbidity from the
//!   authored atmosphere. Its absolute level is not Preetham's: each lobe is
//!   scaled so the light it puts on level ground equals [`SkyLight`]'s
//!   `sky_lux`, the number the exposure, the ambient fill and the environment
//!   map already use. So the sky and the ground it lights never disagree.
//! - **Dusk and night**: below the horizon the sun's lobe keeps its sunset
//!   shape, falls away on the same twilight curve as `sky_lux`, and turns to
//!   the blue hour. The moon lights a second lobe with its own light, phase
//!   and day-for-night gain, graded blue as moonlight is. Stars come from the
//!   same star field the atmosphere path shows, turned with the sky.
//! - **The stars on every sky path.** On the atmosphere path the dome draws
//!   only them, so the star field looks the same whichever path draws the
//!   sky, twinkling more low down where their light crosses more air.
//! - **The sun's disc**, energy-conserving like bevy's `SunDisk` (the light's
//!   illuminance over the disc's solid angle, limb-darkened) and capped at
//!   [`SUN_DISC_PEAK`] for the 16-bit HDR target.
//!
//! The dome is the inside of a sphere around the camera. With a depth
//! prepass it is drawn behind everything and discards where the scene is;
//! without one it sits just inside the far plane and is depth tested. It
//! sorts before the moon's disc and the cloud dome, which draw over it.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::asset::embedded_asset;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin, MeshPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
// This crate's bevy prelude does not re-export the log macros.
use tracing::info;

use crate::classes::{Sky, Sun as SunClass};
use crate::plugins::lighting_plugin::{NIGHT_FLOOR_LUX, SUN_DISC_PEAK};
use crate::plugins::sky_atmosphere::{
    clear_sky_lux, luminance, sidereal_angle, star_field_brightness, star_sky_rotation, ActiveSkyMode,
    SceneAtmosphere, SkyBillboard, SkyCamera, SkyConfig, SkyExposure, SkyLight, SkyLightSet, SkyMode, StarField,
};
use crate::services::lighting::{EustressAtmosphere, LightingService, Sun as SunMarker};

/// Asset path of the embedded `sky_dome.wgsl`.
pub const SKY_DOME_SHADER_PATH: &str = "embedded://eustress_common/plugins/sky_dome.wgsl";

/// Transparent draws sort by distance plus this bias, most negative first.
/// The sky goes first; the moon (`MOON_SORT_BIAS`) and the clouds
/// (`CLOUD_SORT_BIAS`) draw over it.
pub const SKY_DOME_SORT_BIAS: f32 = -3.0e6;

/// The dome's radius as a fraction of the camera's far plane. Only matters
/// without a depth prepass, where the dome is depth tested: just inside the
/// far plane, so it hides as little distant scene as possible.
const DOME_FAR_FRACTION: f32 = 0.9;

/// Radius used when the camera's projection says nothing of a far plane.
const DOME_FALLBACK_RADIUS: f32 = 5_000.0;

/// The blue hour's hue, luminance 1: past sunset the sky overhead deepens to
/// blue while the glow stays low on the sun's side (ozone's Chappuis band
/// absorbing the orange once the light runs horizontally through it).
const BLUE_HOUR: Vec3 = Vec3::new(0.754, 0.988, 1.852);

/// Degrees of solar depression over which the blue hour sets in.
const BLUE_HOUR_DEPTH: f32 = 8.0;

/// Solar limb darkening: the disc's edge is this much dimmer than its centre.
/// `sky_dome.wgsl` uses the same 0.6.
pub const LIMB_DARKENING: f32 = 0.6;

/// The night sky's own glow (airglow and starlight), luminance 1: a deep
/// blue. Its level is the ambient floor's, `NIGHT_FLOOR_LUX` over pi, so a
/// moonless night's sky carries the light the floor puts on the ground,
/// and a lit ground never stands under a black sky.
const NIGHT_GLOW: Vec3 = Vec3::new(0.694, 0.992, 1.984);

/// How much brighter the night glow is at the horizon than overhead: airglow
/// is seen through more of its layer low down (the van Rhijn effect).
const NIGHT_GLOW_HORIZON: f32 = 1.5;

// ============================================================================
// The analytic sky
// ============================================================================

/// One Perez distribution, `F(theta, gamma) = (1 + A e^(B / cos theta))
/// (1 + C e^(D gamma) + E cos^2 gamma)`, where theta is a direction's angle
/// from the zenith and gamma its angle from the sun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Perez {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
}

impl Perez {
    /// `F(theta, gamma)`. The horizon is held just above `cos theta = 0`,
    /// where the first factor is finite and all but 1.
    pub fn f(&self, cos_theta: f32, gamma: f32) -> f32 {
        let cos_theta = cos_theta.max(0.01);
        let cos_gamma = gamma.cos();
        (1.0 + self.a * (self.b / cos_theta).exp())
            * (1.0 + self.c * (self.d * gamma).exp() + self.e * cos_gamma * cos_gamma)
    }

    fn abcd(&self) -> Vec4 {
        Vec4::new(self.a, self.b, self.c, self.d)
    }
}

/// Preetham's distributions of luminance (`Y`) and of the two
/// chromaticity coordinates (`x`, `y`) at `turbidity`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerezSet {
    pub luminance: Perez,
    pub x: Perez,
    pub y: Perez,
}

impl PerezSet {
    pub fn new(t: f32) -> Self {
        Self {
            luminance: Perez {
                a: 0.1787 * t - 1.4630,
                b: -0.3554 * t + 0.4275,
                c: -0.0227 * t + 5.3251,
                d: 0.1206 * t - 2.5771,
                e: -0.0670 * t + 0.3703,
            },
            x: Perez {
                a: -0.0193 * t - 0.2592,
                b: -0.0665 * t + 0.0008,
                c: -0.0004 * t + 0.2125,
                d: -0.0641 * t - 0.8989,
                e: -0.0033 * t + 0.0452,
            },
            y: Perez {
                a: -0.0167 * t - 0.2608,
                b: -0.0950 * t + 0.0092,
                c: -0.0079 * t + 0.2102,
                d: -0.0441 * t - 1.6537,
                e: -0.0109 * t + 0.0529,
            },
        }
    }
}

/// Preetham's zenith chromaticity `(x, y)` for a sun `theta_s` radians from
/// the zenith. Held at the horizon for a sun below it, where the model has
/// no data: the twilight is shaped by [`sun_lobe`] instead.
pub fn zenith_chromaticity(t: f32, theta_s: f32) -> Vec2 {
    let s = theta_s.clamp(0.0, FRAC_PI_2);
    let (s2, s3, t2) = (s * s, s * s * s, t * t);
    let x = t2 * (0.00166 * s3 - 0.00375 * s2 + 0.00209 * s)
        + t * (-0.02903 * s3 + 0.06377 * s2 - 0.03202 * s + 0.00394)
        + (0.11693 * s3 - 0.21196 * s2 + 0.06052 * s + 0.25886);
    let y = t2 * (0.00275 * s3 - 0.00610 * s2 + 0.00317 * s)
        + t * (-0.04214 * s3 + 0.08970 * s2 - 0.04153 * s + 0.00516)
        + (0.15346 * s3 - 0.26756 * s2 + 0.06670 * s + 0.26688);
    Vec2::new(x, y)
}

/// Turbidity from the authored atmosphere: 2.5, a clear day, at Earth-normal
/// density; thicker air and haze raise it toward a hazy 10.
pub fn turbidity(atmosphere: &EustressAtmosphere) -> f32 {
    (2.0 + atmosphere.density.max(0.0) + 0.8 * atmosphere.haze.max(0.0)).clamp(2.0, 10.0)
}

/// Light on level ground from the luminance distribution normalised to 1 at
/// the zenith, for a sun `theta_s` radians from it: the number a lobe is
/// divided by so its ground light comes out as `SkyLight` says.
pub fn relative_irradiance(perez: &Perez, theta_s: f32) -> f32 {
    const RINGS: usize = 24;
    const SEGMENTS: usize = 48;
    let sun = Vec3::new(theta_s.sin(), theta_s.cos(), 0.0);
    let zenith = perez.f(1.0, theta_s);
    let (d_theta, d_phi) = (FRAC_PI_2 / RINGS as f32, TAU / SEGMENTS as f32);
    let mut sum = 0.0;
    for i in 0..RINGS {
        let theta = (i as f32 + 0.5) * d_theta;
        let (sin_t, cos_t) = theta.sin_cos();
        for j in 0..SEGMENTS {
            let phi = (j as f32 + 0.5) * d_phi;
            let ray = Vec3::new(sin_t * phi.cos(), cos_t, sin_t * phi.sin());
            let gamma = ray.dot(sun).clamp(-1.0, 1.0).acos();
            sum += perez.f(cos_t, gamma) * cos_t * sin_t;
        }
    }
    (sum * d_theta * d_phi / zenith).max(1e-4)
}

/// One lobe of the sky: a light (the sun or the moon) and the sky it lights.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lobe {
    /// Toward the light.
    pub toward: Vec3,
    /// x: zenith luminance over `F_Y(0, theta_s)`, cd/m^2. y, z: zenith
    /// chromaticity over `F_x(0, theta_s)` and `F_y(0, theta_s)`. w: 1 when
    /// the lobe lights anything.
    pub zenith: Vec4,
    /// Linear RGB tint, luminance 1.
    pub tint: Vec3,
}

/// A lobe for a light toward `toward` whose sky puts `ground_lux` on level
/// ground, in `tint`.
pub fn lobe(perez: &PerezSet, t: f32, toward: Vec3, ground_lux: f32, tint: Vec3) -> Lobe {
    if ground_lux <= 0.0 {
        return Lobe { toward, zenith: Vec4::ZERO, tint };
    }
    // The model has no sun below the horizon: shape the sky as for a sun on
    // it, and let `ground_lux` carry the twilight.
    let theta_s = toward.y.clamp(-1.0, 1.0).acos().min(FRAC_PI_2 - 1e-3);
    let chroma = zenith_chromaticity(t, theta_s);
    let luminance = ground_lux / relative_irradiance(&perez.luminance, theta_s);
    Lobe {
        toward,
        zenith: Vec4::new(
            luminance / perez.luminance.f(1.0, theta_s),
            chroma.x / perez.x.f(1.0, theta_s),
            chroma.y / perez.y.f(1.0, theta_s),
            1.0,
        ),
        tint,
    }
}

/// Linear RGB, cd/m^2, of `lobe` toward `ray` (above the horizon). The CPU
/// twin of `lobe()` in `sky_dome.wgsl`.
pub fn lobe_radiance(perez: &PerezSet, lobe: &Lobe, ray: Vec3) -> Vec3 {
    if lobe.zenith.w <= 0.0 {
        return Vec3::ZERO;
    }
    let cos_theta = ray.y;
    let gamma = ray.dot(lobe.toward).clamp(-1.0, 1.0).acos();
    let y_lum = lobe.zenith.x * perez.luminance.f(cos_theta, gamma);
    let cx = lobe.zenith.y * perez.x.f(cos_theta, gamma);
    let cy = (lobe.zenith.z * perez.y.f(cos_theta, gamma)).max(1e-4);
    let big_x = cx / cy * y_lum;
    let big_z = (1.0 - cx - cy) / cy * y_lum;
    let rgb = Vec3::new(
        3.2406 * big_x - 1.5372 * y_lum - 0.4986 * big_z,
        -0.9689 * big_x + 1.8758 * y_lum + 0.0415 * big_z,
        0.0557 * big_x - 0.2040 * y_lum + 1.0570 * big_z,
    );
    rgb.max(Vec3::ZERO) * lobe.tint
}

/// A colour scaled to luminance 1, or white when it has none.
fn unit_luminance(rgb: Vec3) -> Vec3 {
    let l = luminance(rgb);
    if l > 1e-6 { rgb / l } else { Vec3::ONE }
}

/// Elevation, degrees, of a direction.
fn elevation_deg(d: Vec3) -> f32 {
    d.y.clamp(-1.0, 1.0).asin().to_degrees()
}

/// The sun's lobe: `SkyLight`'s sky light from the sun, in the sun's colour,
/// turning to the blue hour as the sun goes down.
pub fn sun_lobe(perez: &PerezSet, t: f32, sky_light: &SkyLight) -> Lobe {
    const RAW_SUNLIGHT: f32 = bevy::light::light_consts::lux::RAW_SUNLIGHT;
    let toward = sky_light.sun_direction;
    let elevation = elevation_deg(toward);
    let lux = clear_sky_lux(elevation) * luminance(sky_light.sun_light) / RAW_SUNLIGHT;
    let blue = ((-elevation) / BLUE_HOUR_DEPTH).clamp(0.0, 1.0);
    let tint = unit_luminance(unit_luminance(sky_light.sun_light).lerp(BLUE_HOUR, blue));
    lobe(perez, t, toward, lux, tint)
}

/// The moon's lobe: its sky light with the day-for-night gain, graded blue
/// as its light is (see `moonlight_color`).
pub fn moon_lobe(perez: &PerezSet, t: f32, sky_light: &SkyLight) -> Lobe {
    const RAW_SUNLIGHT: f32 = bevy::light::light_consts::lux::RAW_SUNLIGHT;
    let toward = sky_light.moon_direction;
    let lux = clear_sky_lux(elevation_deg(toward)) * luminance(sky_light.moon_light) / RAW_SUNLIGHT;
    lobe(perez, t, toward, lux, unit_luminance(sky_light.moon_light))
}

/// The disc's radiance, cd/m^2: the sun's light at the ground over the
/// disc's solid angle, energy-conserving as bevy draws it, and no brighter
/// than [`SUN_DISC_PEAK`] once exposed at `camera_ev100`.
pub fn sun_disc_radiance(sun_ground: Vec3, angular_radius: f32, camera_ev100: f32) -> Vec3 {
    let solid_angle = PI * angular_radius.max(1e-5).powi(2);
    let radiance = sun_ground.max(Vec3::ZERO) / solid_angle;
    let exposure = (-camera_ev100).exp2() / 1.2;
    let peak = luminance(radiance) * exposure;
    if peak > SUN_DISC_PEAK { radiance * (SUN_DISC_PEAK / peak) } else { radiance }
}

/// The night sky's glow overhead, cd/m^2: the ambient floor's light, seen.
/// With the horizon brightening `1 + h (1 - cos theta)^2`, a sky of zenith
/// luminance `L` puts `pi L (1 + h / 6)` on level ground, so dividing by
/// that makes the sky give the ground exactly the floor.
pub fn night_glow() -> Vec3 {
    NIGHT_GLOW * (NIGHT_FLOOR_LUX / PI) / (1.0 + NIGHT_GLOW_HORIZON / 6.0)
}

/// What the ground below the horizon shows: level ground of the atmosphere's
/// `decay` albedo lit by the sun, the moon and the sky, cd/m^2.
pub fn ground_radiance(atmosphere: &EustressAtmosphere, sky_light: &SkyLight) -> Vec3 {
    let albedo = Color::srgb(atmosphere.decay[0], atmosphere.decay[1], atmosphere.decay[2]).to_linear();
    let albedo = Vec3::new(albedo.red, albedo.green, albedo.blue);
    let light = sky_light.sun_ground * sky_light.sun_direction.y.max(0.0)
        + sky_light.moon_ground * sky_light.moon_direction.y.max(0.0)
        + Vec3::splat(sky_light.sky_lux);
    albedo * light / PI
}

// ============================================================================
// Material
// ============================================================================

/// The dome's shader parameters: `SkyDomeParams` in `sky_dome.wgsl`,
/// binding 0. `sky_dome_params_layout_matches_the_wgsl_struct` holds the two
/// layouts together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, ShaderType)]
pub struct SkyDomeParams {
    /// xyz: toward the sun. w: the disc's angular radius, radians.
    pub sun: Vec4,
    /// rgb: the disc's radiance, cd/m^2. w: unused.
    pub sun_disc: Vec4,
    /// The sun's lobe: see [`Lobe::zenith`].
    pub sun_zenith: Vec4,
    /// rgb: the sun's lobe's tint.
    pub sun_tint: Vec4,
    /// xyz: toward the moon.
    pub moon: Vec4,
    pub moon_zenith: Vec4,
    pub moon_tint: Vec4,
    /// A, B, C, D of the luminance distribution.
    pub perez_lum: Vec4,
    /// A, B, C, D of the x chromaticity distribution.
    pub perez_x: Vec4,
    /// A, B, C, D of the y chromaticity distribution.
    pub perez_y: Vec4,
    /// E of the luminance, x and y distributions.
    pub perez_e: Vec4,
    /// rgb: the ground below the horizon, cd/m^2.
    pub ground: Vec4,
    /// rgb: the night sky's own glow overhead, cd/m^2. w: how much brighter
    /// it is at the horizon.
    pub night_sky: Vec4,
    /// x: the star field's gain. y: 1 draws the sky, 0 only the sun's disc
    /// (over an authored skybox), 2 only the stars (over bevy's atmosphere).
    pub stars: Vec4,
    /// Columns of the rotation from the world into the star map.
    pub star_x: Vec4,
    pub star_y: Vec4,
    pub star_z: Vec4,
}

/// The sky dome's material. See the module docs.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct SkyDomeMaterial {
    #[uniform(0)]
    pub params: SkyDomeParams,
    /// The star field (`StarField`), once built.
    #[texture(1, dimension = "cube", visibility(fragment))]
    #[sampler(2, visibility(fragment))]
    pub stars: Option<Handle<Image>>,
}

impl Material for SkyDomeMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_DOME_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // Premultiplied: the sky is opaque (alpha 1); over an authored
        // skybox only the disc is drawn, alpha its coverage.
        AlphaMode::Premultiplied
    }

    fn depth_bias(&self) -> f32 {
        SKY_DOME_SORT_BIAS
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The camera is inside the dome: draw its inner faces, once.
        descriptor.primitive.cull_mode = Some(Face::Front);
        // With a depth prepass the shader finds the scene itself and draws
        // only where there is none, so nothing is tested here.
        if key.mesh_key.contains(MeshPipelineKey::DEPTH_PREPASS) {
            if let Some(depth) = descriptor.depth_stencil.as_mut() {
                depth.depth_compare = Some(CompareFunction::Always);
                depth.depth_write_enabled = Some(false);
            }
        }
        Ok(())
    }
}

/// Marks the sky dome.
#[derive(Component, Debug, Clone, Copy)]
pub struct SkyDome;

/// The dome's drawing mode, `SkyDomeParams::stars.y`.
fn dome_mode(mode: SkyMode) -> f32 {
    match mode {
        // The whole sky: the gradient path.
        SkyMode::Gradient => 1.0,
        // The sun's disc over an authored skybox.
        SkyMode::Skybox => 0.0,
        // The stars and the sun's disc: bevy's atmosphere draws the sky.
        SkyMode::Atmosphere => 2.0,
    }
}

/// Draws the sky dome. Added by `SharedLightingPlugin`.
pub struct SkyDomePlugin;

impl Plugin for SkyDomePlugin {
    fn build(&self, app: &mut App) {
        // A material plugin sets up its render side in `build`; without a
        // renderer there is nothing to draw.
        if app.get_sub_app(bevy::render::RenderApp).is_none() {
            return;
        }
        embedded_asset!(app, "sky_dome.wgsl");
        app.add_plugins(MaterialPlugin::<SkyDomeMaterial>::default())
            .add_systems(Update, sync_sky_dome.after(SkyLightSet));
    }
}

/// Everything the dome's parameters are made from, for one frame.
pub struct DomeInputs<'a> {
    pub mode: SkyMode,
    pub atmosphere: &'a EustressAtmosphere,
    pub sky_light: &'a SkyLight,
    pub exposure: &'a SkyExposure,
    pub config: &'a SkyConfig,
    /// The sun's disc, angular radius in radians, when it is shown.
    pub disc_radius: Option<f32>,
    /// Celestial bodies shown (the Sky's switch).
    pub shown: bool,
    /// The star map's rotation onto the sky.
    pub star_rotation: Quat,
}

/// The dome's parameters for this frame.
pub fn sky_dome_params(inputs: &DomeInputs) -> SkyDomeParams {
    let t = turbidity(inputs.atmosphere);
    let perez = PerezSet::new(t);
    let sky_light = inputs.sky_light;
    let sun = sun_lobe(&perez, t, sky_light);
    let moon = moon_lobe(&perez, t, sky_light);
    let radius = inputs.disc_radius.unwrap_or(0.0);
    let disc = match inputs.disc_radius {
        Some(r) => sun_disc_radiance(sky_light.sun_ground, r, inputs.exposure.camera_ev100),
        None => Vec3::ZERO,
    };
    let stars = star_field_brightness(
        inputs.config,
        elevation_deg(sky_light.sun_direction),
        inputs.shown,
        inputs.exposure,
    );
    let rotation = Mat3::from_quat(inputs.star_rotation.inverse());
    SkyDomeParams {
        sun: sky_light.sun_direction.extend(radius),
        sun_disc: disc.extend(0.0),
        sun_zenith: sun.zenith,
        sun_tint: sun.tint.extend(0.0),
        moon: sky_light.moon_direction.extend(0.0),
        moon_zenith: moon.zenith,
        moon_tint: moon.tint.extend(0.0),
        perez_lum: perez.luminance.abcd(),
        perez_x: perez.x.abcd(),
        perez_y: perez.y.abcd(),
        perez_e: Vec4::new(perez.luminance.e, perez.x.e, perez.y.e, 0.0),
        ground: ground_radiance(inputs.atmosphere, sky_light).extend(0.0),
        night_sky: night_glow().extend(NIGHT_GLOW_HORIZON),
        stars: Vec4::new(stars, dome_mode(inputs.mode), 0.0, 0.0),
        star_x: rotation.x_axis.extend(0.0),
        star_y: rotation.y_axis.extend(0.0),
        star_z: rotation.z_axis.extend(0.0),
    }
}

/// Place the dome around the main camera and keep its parameters current.
/// Drawn on the gradient path (the whole sky) and over an authored skybox
/// (the sun's disc alone); the atmosphere path draws its own.
fn sync_sky_dome(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyDomeMaterial>>,
    active: Res<ActiveSkyMode>,
    scene: Res<SceneAtmosphere>,
    config: Res<SkyConfig>,
    sky_light: Res<SkyLight>,
    exposure: Res<SkyExposure>,
    star_field: Res<StarField>,
    lighting: Res<LightingService>,
    suns: Query<&SunClass, With<SunMarker>>,
    skies: Query<&Sky>,
    cameras: Query<(&Camera, &GlobalTransform, Option<&Projection>), With<SkyCamera>>,
    mut domes: Query<(&MeshMaterial3d<SkyDomeMaterial>, &mut Transform), With<SkyDome>>,
) {
    let Some((_, view, projection)) = cameras
        .iter()
        .filter(|(camera, ..)| camera.is_active)
        .min_by_key(|(camera, ..)| camera.order)
    else {
        return;
    };
    let (radius, perspective) = match projection {
        Some(Projection::Perspective(p)) => (p.far * DOME_FAR_FRACTION, true),
        Some(Projection::Orthographic(_)) => (DOME_FALLBACK_RADIUS, false),
        _ => (DOME_FALLBACK_RADIUS, true),
    };
    let sun = suns.iter().next();
    let shown = skies.iter().next().map_or(true, |s| s.celestial_bodies_shown);
    // The Sun's disc at its AngularSize, on every sky path: bevy's own disc
    // is off (`lighting_plugin::sync_sun_disk`), so its light can keep the
    // true size it sets by.
    let disc_radius = sun
        .filter(|s| s.enabled && shown)
        .map(|s| (s.angular_size.clamp(0.05, 20.0) * 0.5).to_radians());
    let star_rotation = match sun {
        Some(s) => star_sky_rotation(s.latitude, sidereal_angle(s)),
        None => star_sky_rotation(lighting.geographic_latitude, lighting.time_of_day * 360.0),
    };
    let params = sky_dome_params(&DomeInputs {
        mode: active.0,
        atmosphere: &scene.atmosphere,
        sky_light: &sky_light,
        exposure: &exposure,
        config: &config,
        disc_radius,
        shown,
        star_rotation,
    });
    // Parallel rays have no sky to draw; the skybox shows its flat colour.
    // On the atmosphere path the dome carries the stars and the sun's disc,
    // so it is left out entirely only while it has neither.
    let visible = perspective
        && (active.0 != SkyMode::Atmosphere || params.stars.x > 0.0 || params.sun.w > 0.0);

    let placed = Transform::from_translation(view.translation())
        .with_scale(Vec3::splat(if visible { radius } else { 0.0 }));

    match domes.iter_mut().next() {
        Some((material, mut transform)) => {
            if transform.translation.distance_squared(placed.translation) > 1e-6 || transform.scale != placed.scale {
                *transform = placed;
            }
            if visible {
                if let Some(mut dome) = materials.get_mut(&material.0) {
                    if dome.params != params {
                        dome.params = params;
                    }
                    if dome.stars != star_field.handle {
                        dome.stars = star_field.handle.clone();
                    }
                }
            }
        }
        None => {
            let material = materials.add(SkyDomeMaterial { params, stars: star_field.handle.clone() });
            commands.spawn((
                Mesh3d(meshes.add(
                    Sphere::new(1.0).mesh().ico(4).unwrap_or_else(|_| Sphere::new(1.0).mesh().uv(64, 32)),
                )),
                MeshMaterial3d(material),
                placed,
                NotShadowCaster,
                NotShadowReceiver,
                NoFrustumCulling,
                SkyBillboard,
                SkyDome,
                Name::new("Sky Dome"),
            ));
            info!("🌌 Sky dome ready (sky path {:?})", active.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::surface_material::wgsl_layout::{layout, struct_fields};
    use std::mem::size_of;

    const SHADER: &str = include_str!("sky_dome.wgsl");

    #[test]
    fn sky_dome_params_layout_matches_the_wgsl_struct() {
        let (members, size, _) = layout(&struct_fields(SHADER, "SkyDomeParams"));
        let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "sun", "sun_disc", "sun_zenith", "sun_tint", "moon", "moon_zenith", "moon_tint", "perez_lum",
                "perez_x", "perez_y", "perez_e", "ground", "night_sky", "stars", "star_x", "star_y", "star_z"
            ]
        );
        assert_eq!(size, 272);
        assert_eq!(size_of::<SkyDomeParams>(), 272);
        assert_eq!(SkyDomeParams::min_size().get(), 272);
    }

    fn sky_light_at(elevation_deg: f32) -> SkyLight {
        let e = elevation_deg.to_radians();
        SkyLight {
            sun_direction: Vec3::new(0.0, e.sin(), -e.cos()),
            sky_lux: clear_sky_lux(elevation_deg),
            ..SkyLight::default()
        }
    }

    fn up(elevation_deg: f32, toward_sun: bool) -> Vec3 {
        let e = elevation_deg.to_radians();
        Vec3::new(0.0, e.sin(), if toward_sun { -e.cos() } else { e.cos() })
    }

    #[test]
    fn a_lobe_puts_exactly_the_sky_light_on_the_ground() {
        // Integrate the lobe's radiance over the hemisphere: the result is
        // what `SkyLight` says, so the exposure, the ambient and the dome
        // all agree about how bright the sky is.
        let t = 2.5;
        let perez = PerezSet::new(t);
        for elevation in [60.0f32, 25.0, 5.0] {
            let light = sky_light_at(elevation);
            let lobe = sun_lobe(&perez, t, &light);
            let (rings, segments) = (48, 96);
            let mut lux = 0.0;
            for i in 0..rings {
                let theta = (i as f32 + 0.5) * FRAC_PI_2 / rings as f32;
                for j in 0..segments {
                    let phi = (j as f32 + 0.5) * TAU / segments as f32;
                    let ray = Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin());
                    lux += luminance(lobe_radiance(&perez, &lobe, ray)) * theta.cos() * theta.sin();
                }
            }
            lux *= (FRAC_PI_2 / rings as f32) * (TAU / segments as f32);
            let want = clear_sky_lux(elevation);
            assert!((lux / want - 1.0).abs() < 0.08, "{elevation} deg: {lux} lux against {want}");
        }
    }

    #[test]
    fn the_day_sky_is_blue_overhead_and_brighter_at_the_horizon() {
        // The old gradient put its zenith seven times under its horizon band
        // and the whole sky well under a sunlit ground: a negative.
        let t = 2.5;
        let perez = PerezSet::new(t);
        let lobe = sun_lobe(&perez, t, &sky_light_at(35.0));
        let zenith = lobe_radiance(&perez, &lobe, Vec3::Y);
        let horizon = lobe_radiance(&perez, &lobe, up(5.0, false));
        assert!(zenith.z > zenith.x * 1.2, "zenith {zenith:?} is not blue");
        assert!(luminance(horizon) > luminance(zenith), "horizon {horizon:?} under zenith {zenith:?}");
        assert!(luminance(horizon) < luminance(zenith) * 4.0, "horizon {horizon:?} blown against {zenith:?}");
        // Thousands of cd/m^2 overhead, as a clear sky is.
        assert!((1_500.0..8_000.0).contains(&luminance(zenith)), "zenith {} cd/m^2", luminance(zenith));
    }

    #[test]
    fn sunset_glows_warm_toward_the_sun_and_blue_overhead() {
        let t = 2.5;
        let perez = PerezSet::new(t);
        let lobe = sun_lobe(&perez, t, &sky_light_at(1.0));
        let glow = lobe_radiance(&perez, &lobe, up(3.0, true));
        let overhead = lobe_radiance(&perez, &lobe, Vec3::Y);
        assert!(glow.x / glow.z > overhead.x / overhead.z * 1.3, "glow {glow:?}, overhead {overhead:?}");
    }

    #[test]
    fn the_blue_hour_is_dim_and_blue() {
        let t = 2.5;
        let perez = PerezSet::new(t);
        let day = sun_lobe(&perez, t, &sky_light_at(20.0));
        let dusk = sun_lobe(&perez, t, &sky_light_at(-6.0));
        let day_zenith = lobe_radiance(&perez, &day, Vec3::Y);
        let dusk_zenith = lobe_radiance(&perez, &dusk, Vec3::Y);
        assert!(luminance(dusk_zenith) < luminance(day_zenith) * 0.01);
        assert!(dusk_zenith.z > dusk_zenith.x * 2.0, "dusk zenith {dusk_zenith:?}");
    }

    #[test]
    fn the_disc_never_overflows_the_hdr_target() {
        let sun_ground = Vec3::splat(100_000.0);
        let r = (0.53f32 * 0.5).to_radians();
        let disc = sun_disc_radiance(sun_ground, r, 13.0);
        let exposed = luminance(disc) * (-13.0f32).exp2() / 1.2;
        assert!(exposed <= SUN_DISC_PEAK * 1.001, "{exposed}");
        // A dim sun is left physical.
        let faint = sun_disc_radiance(Vec3::splat(1.0), r, 13.0);
        assert!((luminance(faint) - 1.0 / (PI * r * r)).abs() < 1.0);
    }

    #[test]
    fn a_switched_off_light_lights_no_sky() {
        let t = 2.5;
        let perez = PerezSet::new(t);
        let dark = SkyLight { sun_light: Vec3::ZERO, ..sky_light_at(40.0) };
        let lobe = sun_lobe(&perez, t, &dark);
        assert_eq!(lobe.zenith.w, 0.0);
        assert_eq!(lobe_radiance(&perez, &lobe, Vec3::Y), Vec3::ZERO);
    }

    #[test]
    fn a_moonless_night_sky_glows_deep_blue_at_the_floor_level() {
        // The ambient floor lights the ground at night; the sky shows that
        // light too, or a lit ground stands under a black sky: a negative.
        let glow = night_glow();
        assert!(glow.z > glow.x * 2.0, "{glow:?} is not deep blue");
        // Light on level ground from the glow and its horizon brightening,
        // integrated: close to the floor.
        let (rings, mut lux) = (64, 0.0);
        for i in 0..rings {
            let theta = (i as f32 + 0.5) * FRAC_PI_2 / rings as f32;
            let lift = 1.0 + NIGHT_GLOW_HORIZON * (1.0 - theta.cos()).powi(2);
            lux += luminance(glow) * lift * theta.cos() * theta.sin() * TAU * (FRAC_PI_2 / rings as f32);
        }
        assert!((lux / NIGHT_FLOOR_LUX - 1.0).abs() < 0.02, "{lux} lux against {NIGHT_FLOOR_LUX}");
    }

    #[test]
    fn turbidity_is_clear_at_earth_normal_and_rises_with_haze() {
        let clear = EustressAtmosphere::default();
        assert!((turbidity(&clear) - 2.5).abs() < 1e-5);
        let hazy = EustressAtmosphere { haze: 3.0, ..EustressAtmosphere::default() };
        assert!(turbidity(&hazy) > 4.0);
    }
}
